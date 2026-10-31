//! The player actor: owns queue, history and the current track, runs the
//! 20 ms audio tick (PCM → gain → Opus) and publishes state snapshots.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use audiopus::coder::Encoder;
use audiopus::{Application, Bitrate, Channels, SampleRate};
use tokio::sync::{mpsc, watch};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::audio::{Decoder, Pull, FRAME_LEN};
use crate::config::Config;
use crate::model::*;
use crate::resolver::{Resolver, StreamInfo};
use crate::sponsorblock;
use crate::store::{Persisted, Store};
use crate::ts::AudioOut;

const HISTORY_MAX: usize = 50;
/// Stop instead of skipping after this many unresolvable tracks in a row.
const MAX_RESOLVE_FAILURES: u32 = 3;
/// Ticks later than this count as late (risk of audible jitter).
const LATE_TICK: Duration = Duration::from_millis(10);
const PREBUFFER_FRAMES: usize = 25; // 0.5 s before we start talking
const STREAM_CACHE_TTL: Duration = Duration::from_secs(45 * 60);
const MAX_RETRIES: u8 = 2;
/// Pause once nobody has been listening in our channel for this long.
const EMPTY_CHANNEL_GRACE: Duration = Duration::from_secs(60);

pub enum Cmd {
	Add { tracks: Vec<Track>, requested_by: Option<String>, next: bool, play_now: bool },
	Skip,
	Prev,
	Pause,
	Resume,
	Toggle,
	Stop,
	Seek(f64),
	Remove(Uuid),
	Move { id: Uuid, to: usize },
	PlayItem(Uuid),
	Clear,
	TsStatus(TsStatus),
}

#[derive(Clone)]
pub struct PlayerHandle {
	cmd_tx: mpsc::Sender<Cmd>,
	state_rx: watch::Receiver<Arc<StateSnapshot>>,
}

impl PlayerHandle {
	pub async fn send(&self, cmd: Cmd) {
		if self.cmd_tx.send(cmd).await.is_err() {
			error!("player actor is gone");
		}
	}
	pub fn snapshot(&self) -> Arc<StateSnapshot> { self.state_rx.borrow().clone() }
	pub fn subscribe(&self) -> watch::Receiver<Arc<StateSnapshot>> { self.state_rx.clone() }
}

struct LoadResult {
	gen: u64,
	/// Track the result belongs to (prefetches are cached under it).
	url: String,
	result: Result<(StreamInfo, Vec<Segment>)>,
}

struct Current {
	item: QueueItem,
	gen: u64,
	status: PlaybackStatus,
	decoder: Option<Decoder>,
	stream: Option<StreamInfo>,
	segments: Vec<Segment>,
	/// Position where the current decoder started (after seek/resume).
	base_pos: f64,
	frames_played: u64,
	prebuffered: bool,
	starved_ticks: u32,
	retries: u8,
	pipe_fallback: bool,
	talking: bool,
	stats: PlayStats,
}

/// Per-track playback health, logged when the track ends.
#[derive(Default)]
struct PlayStats {
	/// Frames the TeamSpeak thread could not take (silently lost audio).
	dropped: u32,
	/// Decoder underruns after prebuffering, and their total length.
	stalls: u32,
	stall_ticks: u32,
	/// Ticks that fired more than LATE_TICK late.
	late_ticks: u32,
	max_late_ms: u64,
}

impl Current {
	fn position(&self) -> f64 { self.base_pos + self.frames_played as f64 * 0.02 }
	fn duration(&self) -> Option<f64> {
		self.stream.as_ref().and_then(|s| s.duration).or(self.item.track.duration)
	}
}

pub struct Player {
	cfg: Config,
	store: Store,
	resolver: Resolver,
	http: reqwest::Client,
	queue: Vec<QueueItem>,
	history: Vec<QueueItem>,
	current: Option<Current>,
	volume: f32,
	ts_status: TsStatus,
	last_error: Option<String>,
	/// Consecutive tracks whose stream could not be resolved.
	resolve_failures: u32,
	/// Since when nobody is listening in our channel (`None`: someone is).
	empty_since: Option<Instant>,
	/// Paused because the channel emptied; resumes once someone listens again.
	auto_paused: bool,
	version: u64,
	gen_counter: u64,
	state_tx: watch::Sender<Arc<StateSnapshot>>,
	audio_tx: mpsc::Sender<AudioOut>,
	encoder: Encoder,
	opus_buf: Vec<u8>,
	load_tx: mpsc::Sender<LoadResult>,
	stream_cache: HashMap<String, (Instant, StreamInfo)>,
	/// URL of the prefetch currently in flight.
	prefetching: Option<String>,
	dirty: bool,
	last_save: Instant,
	last_publish: Instant,
}

pub fn spawn(cfg: Config, store: Store, resolver: Resolver, audio_tx: mpsc::Sender<AudioOut>) -> Result<PlayerHandle> {
	let persisted = store.load();
	let mut encoder = Encoder::new(SampleRate::Hz48000, Channels::Stereo, Application::Audio)?;
	encoder.set_bitrate(Bitrate::BitsPerSecond(cfg.opus_bitrate))?;
	// Some loss on the uplink is expected; trade a little bitrate for robustness.
	encoder.set_inband_fec(true)?;
	encoder.set_packet_loss_perc(5)?;
	let ts_status = TsStatus { connected: false, server: cfg.ts_address.clone(), channel: None, listeners: 0, error: None };
	let (cmd_tx, cmd_rx) = mpsc::channel(64);
	let (load_tx, load_rx) = mpsc::channel(8);
	let mut player = Player {
		volume: cfg.default_volume.clamp(0.0, 1.5),
		queue: persisted.queue,
		history: persisted.history,
		cfg,
		store,
		resolver,
		http: reqwest::Client::builder().user_agent("hoerbert/0.1").build()?,
		current: None,
		ts_status,
		last_error: None,
		resolve_failures: 0,
		empty_since: Some(Instant::now()),
		auto_paused: false,
		version: 0,
		gen_counter: 0,
		state_tx: watch::channel(Arc::new(empty_snapshot())).0,
		audio_tx,
		encoder,
		opus_buf: vec![0u8; 4000],
		load_tx,
		stream_cache: HashMap::new(),
		prefetching: None,
		dirty: false,
		last_save: Instant::now(),
		last_publish: Instant::now(),
	};
	let (state_tx, state_rx) = watch::channel(Arc::new(player.snapshot()));
	player.state_tx = state_tx;
	tokio::spawn(async move { player.run(cmd_rx, load_rx).await });
	Ok(PlayerHandle { cmd_tx, state_rx })
}

fn empty_snapshot() -> StateSnapshot {
	StateSnapshot {
		version: 0,
		ts: TsStatus { connected: false, server: String::new(), channel: None, listeners: 0, error: None },
		playback: Playback {
			status: PlaybackStatus::Idle,
			item: None,
			position: 0.0,
			position_at: now_ms(),
			segments: vec![],
			error: None,
		},
		queue: vec![],
		history: vec![],
	}
}

impl Player {
	async fn run(mut self, mut cmd_rx: mpsc::Receiver<Cmd>, mut load_rx: mpsc::Receiver<LoadResult>) {
		let mut tick = tokio::time::interval(Duration::from_millis(20));
		tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);
		// Start idle: the restored queue waits for an explicit play.
		self.publish();
		loop {
			tokio::select! {
				t = tick.tick() => self.on_tick(t.elapsed()).await,
				Some(cmd) = cmd_rx.recv() => {
					self.handle(cmd).await;
					// The queue head may have changed (request added, moved…).
					if self.current.is_some() {
						self.prefetch_next();
					}
					self.publish();
				}
				Some(res) = load_rx.recv() => {
					self.on_loaded(res).await;
					self.publish();
				}
			}
			if self.dirty && self.last_save.elapsed() > Duration::from_secs(1) {
				self.save();
			}
		}
	}

	fn save(&mut self) {
		let data = Persisted {
			queue: self.queue.clone(),
			history: self.history.clone(),
		};
		if let Err(e) = self.store.save(&data) {
			error!(error = %e, "failed to persist state");
		}
		self.dirty = false;
		self.last_save = Instant::now();
	}

	fn snapshot(&self) -> StateSnapshot {
		let playback = match &self.current {
			Some(c) => Playback {
				status: c.status,
				item: Some(c.item.clone()),
				position: c.position(),
				position_at: now_ms(),
				segments: c.segments.clone(),
				error: self.last_error.clone(),
			},
			None => Playback {
				status: PlaybackStatus::Idle,
				item: None,
				position: 0.0,
				position_at: now_ms(),
				segments: vec![],
				error: self.last_error.clone(),
			},
		};
		StateSnapshot {
			version: self.version,
			ts: self.ts_status.clone(),
			playback,
			queue: self.queue.clone(),
			history: self.history.clone(),
		}
	}

	fn publish(&mut self) {
		self.version += 1;
		let snap = Arc::new(self.snapshot());
		let _ = self.state_tx.send(snap);
		self.last_publish = Instant::now();
	}

	// ---------------------------------------------------------------- commands

	async fn handle(&mut self, cmd: Cmd) {
		// Someone is using the bot: give them a fresh grace period before auto-pause.
		if !matches!(cmd, Cmd::TsStatus(_)) && self.empty_since.is_some() {
			self.empty_since = Some(Instant::now());
		}
		match cmd {
			Cmd::Add { tracks, requested_by, next, play_now } => {
				// A single track is a request; playlists/mixes are background
				// music that only plays once all requests are done.
				let request = tracks.len() == 1;
				let items: Vec<QueueItem> =
					tracks.into_iter().map(|t| QueueItem { request, ..QueueItem::new(t, requested_by.clone()) }).collect();
				if items.is_empty() {
					return;
				}
				if next || play_now {
					let mut rest = std::mem::take(&mut self.queue);
					self.queue = items;
					self.queue.append(&mut rest);
				} else if request {
					let at = self.queue.iter().position(|i| !i.request).unwrap_or(self.queue.len());
					self.queue.splice(at..at, items);
				} else {
					self.queue.extend(items);
				}
				self.dirty = true;
				if play_now && self.current.is_some() {
					self.finish_current().await;
					self.start_next().await;
				} else if self.current.is_none() {
					self.start_next().await;
				}
			}
			Cmd::Skip => {
				if self.current.is_some() {
					self.finish_current().await;
					self.start_next().await;
				} else {
					self.start_next().await;
				}
			}
			Cmd::Prev => {
				if let Some(prev) = (!self.history.is_empty()).then(|| self.history.remove(0)) {
					if let Some(c) = self.current.take() {
						if let Some(d) = c.decoder {
							d.kill().await;
						}
						self.queue.insert(0, c.item);
					}
					self.queue.insert(0, prev);
					self.dirty = true;
					self.start_next().await;
				} else {
					self.seek(0.0);
				}
			}
			Cmd::Pause => self.set_paused(true),
			Cmd::Resume if self.current.is_none() => self.start_next().await,
			Cmd::Resume => self.set_paused(false),
			Cmd::Toggle => {
				let paused = matches!(self.current.as_ref().map(|c| c.status), Some(PlaybackStatus::Paused));
				if self.current.is_none() {
					self.start_next().await;
				} else {
					self.set_paused(!paused);
				}
			}
			Cmd::Stop => self.stop().await,
			Cmd::Seek(pos) => self.seek(pos),
			Cmd::Remove(id) => {
				self.queue.retain(|i| i.id != id);
				self.dirty = true;
			}
			Cmd::Move { id, to } => {
				if let Some(from) = self.queue.iter().position(|i| i.id == id) {
					let item = self.queue.remove(from);
					let to = to.min(self.queue.len());
					self.queue.insert(to, item);
					self.dirty = true;
				}
			}
			Cmd::PlayItem(id) => {
				if let Some(idx) = self.queue.iter().position(|i| i.id == id) {
					let item = self.queue.remove(idx);
					self.queue.insert(0, item);
					self.finish_current().await;
					self.start_next().await;
				}
			}
			Cmd::Clear => {
				self.queue.clear();
				self.dirty = true;
			}
			Cmd::TsStatus(s) => {
				let listening = s.listeners > 0;
				self.ts_status = s;
				if !listening {
					self.empty_since.get_or_insert_with(Instant::now);
				} else {
					self.empty_since = None;
					if self.auto_paused {
						info!("listeners are back, resuming");
						self.set_paused(false);
					}
				}
			}
		}
	}

	fn set_paused(&mut self, paused: bool) {
		self.auto_paused = false;
		let mut reload = None;
		if let Some(c) = &mut self.current {
			if paused && c.status == PlaybackStatus::Playing {
				c.status = PlaybackStatus::Paused;
				if c.talking {
					let _ = self.audio_tx.try_send(AudioOut::End);
					c.talking = false;
				}
			} else if !paused && c.status == PlaybackStatus::Paused {
				if c.decoder.is_some() {
					c.status = PlaybackStatus::Playing;
				} else {
					// Auto-pause dropped the decoder: re-resolve (the URL may have
					// expired meanwhile) and continue from `base_pos`.
					c.status = PlaybackStatus::Loading;
					reload = Some((c.gen, c.item.track.clone()));
				}
			}
		}
		if let Some((gen, track)) = reload {
			self.spawn_load(gen, track, 0.0);
		}
	}

	/// Nobody is listening: pause and free the stream so we don't keep
	/// streaming into an empty channel. Resumes once listeners are back.
	async fn auto_pause(&mut self) {
		let Some(c) = &mut self.current else { return };
		info!(title = %c.item.track.title, "channel empty, pausing");
		c.base_pos = c.position();
		c.frames_played = 0;
		c.prebuffered = false;
		c.starved_ticks = 0;
		let decoder = c.decoder.take();
		self.set_paused(true);
		self.auto_paused = true;
		if let Some(d) = decoder {
			d.kill().await;
		}
	}

	fn seek(&mut self, pos: f64) {
		let cfg = self.cfg.clone();
		let Some(c) = &mut self.current else { return };
		let Some(stream) = c.stream.clone() else { return };
		if stream.is_live {
			return;
		}
		let pos = pos.max(0.0);
		if let Some(d) = c.duration() {
			if pos >= d - 0.5 {
				// Seeking past the end == finishing the track; handled on next tick.
				if let Some(dec) = c.decoder.take() {
					tokio::spawn(dec.kill());
				}
				c.base_pos = d;
				c.frames_played = 0;
				c.prebuffered = true;
				return;
			}
		}
		if let Some(dec) = c.decoder.take() {
			tokio::spawn(dec.kill());
		}
		c.base_pos = pos;
		c.frames_played = 0;
		c.prebuffered = false;
		c.starved_ticks = 0;
		match if c.pipe_fallback {
			Decoder::spawn_pipe(&cfg, &c.item.track.url, pos)
		} else {
			Decoder::spawn_url(&cfg, &stream, pos)
		} {
			Ok(dec) => c.decoder = Some(dec),
			Err(e) => {
				error!(error = %e, "seek failed to restart decoder");
				self.last_error = Some(format!("{e:#}"));
			}
		}
	}

	// ------------------------------------------------------------- lifecycle

	/// Pop the next item off the queue and begin loading it.
	async fn start_next(&mut self) {
		if self.queue.is_empty() {
			let _ = self.audio_tx.try_send(AudioOut::End);
			return;
		}
		let item = self.queue.remove(0);
		self.dirty = true;
		self.gen_counter += 1;
		let gen = self.gen_counter;
		info!(title = %item.track.title, url = %item.track.url, "loading track");
		self.current = Some(Current {
			item: item.clone(),
			gen,
			status: PlaybackStatus::Loading,
			decoder: None,
			stream: None,
			segments: vec![],
			base_pos: 0.0,
			frames_played: 0,
			prebuffered: false,
			starved_ticks: 0,
			retries: 0,
			stats: PlayStats::default(),
			pipe_fallback: false,
			talking: false,
		});
		self.last_error = None;
		self.spawn_load(gen, item.track, 0.0);
	}

	fn cached_stream(&mut self, url: &str) -> Option<StreamInfo> {
		self.stream_cache.retain(|_, (t, _)| t.elapsed() < STREAM_CACHE_TTL);
		self.stream_cache.get(url).map(|(_, s)| s.clone())
	}

	fn spawn_load(&mut self, gen: u64, track: Track, _start: f64) {
		let cached = self.cached_stream(&track.url);
		let resolver = self.resolver.clone();
		let http = self.http.clone();
		let tx = self.load_tx.clone();
		let sb_on = self.cfg.sponsorblock;
		let cats = self.cfg.sponsorblock_categories.clone();
		tokio::spawn(async move {
			let stream_fut = async {
				match cached {
					Some(s) => Ok(s),
					None => resolver.stream_info(&track).await,
				}
			};
			let sb_fut = async {
				let is_yt = track.source.as_deref().map(|s| s.starts_with("youtube")).unwrap_or(false);
				match (&track.source_id, sb_on && is_yt) {
					(Some(id), true) => sponsorblock::fetch_segments(&http, id, &cats).await.unwrap_or_else(|e| {
						warn!(error = %e, "sponsorblock lookup failed");
						vec![]
					}),
					_ => vec![],
				}
			};
			let (stream, segments) = tokio::join!(stream_fut, sb_fut);
			if let Ok(s) = &stream {
				s.wait_until_available().await;
			}
			let _ = tx.send(LoadResult { gen, url: track.url, result: stream.map(|s| (s, segments)) }).await;
		});
	}

	/// Pre-resolve the next queued track so the gap between songs is short.
	fn prefetch_next(&mut self) {
		let Some(next) = self.queue.first() else { return };
		let track = next.track.clone();
		if self.cached_stream(&track.url).is_some()
			|| track.source.as_deref() == Some("direct")
			|| self.prefetching.as_deref() == Some(track.url.as_str())
		{
			return;
		}
		self.prefetching = Some(track.url.clone());
		let resolver = self.resolver.clone();
		let tx = self.load_tx.clone();
		tokio::spawn(async move {
			// gen 0 == prefetch result, never matches a live load
			let result = resolver.stream_info(&track).await.map(|s| (s, vec![]));
			let _ = tx.send(LoadResult { gen: 0, url: track.url, result }).await;
		});
	}

	async fn on_loaded(&mut self, res: LoadResult) {
		if res.gen == 0 {
			if self.prefetching.as_deref() == Some(res.url.as_str()) {
				self.prefetching = None;
			}
			if let Ok((s, _)) = res.result {
				if self.cached_stream(&res.url).is_none() {
					self.stream_cache.insert(res.url, (Instant::now(), s));
				}
			}
			return;
		}
		let cfg = self.cfg.clone();
		let Some(c) = &mut self.current else { return };
		if c.gen != res.gen {
			return; // stale (user skipped while loading)
		}
		match res.result {
			Ok((stream, segments)) => {
				self.stream_cache.insert(c.item.track.url.clone(), (Instant::now(), stream.clone()));
				if c.item.track.duration.is_none() && stream.duration.is_some() {
					c.item.track.duration = stream.duration;
				}
				c.segments = segments;
				match Decoder::spawn_url(&cfg, &stream, c.base_pos) {
					Ok(dec) => {
						c.decoder = Some(dec);
						c.stream = Some(stream);
						c.status = PlaybackStatus::Playing;
						c.prebuffered = false;
						info!(title = %c.item.track.title, segments = c.segments.len(), "playing");
						self.resolve_failures = 0;
						self.prefetch_next();
					}
					Err(e) => {
						error!(error = %e, "ffmpeg spawn failed");
						self.last_error = Some(format!("{e:#}"));
						self.finish_current().await;
						self.start_next().await;
					}
				}
			}
			Err(e) => {
				self.last_error = Some(format!("{}: {e:#}", c.item.track.title));
				self.resolve_failures += 1;
				// A bot check / login wall hits every track: skipping would just
				// drain the queue. Stop and keep the track instead.
				let msg = format!("{e:#}");
				if (msg.contains("Sign in to confirm you") && msg.contains("bot")) || self.resolve_failures >= MAX_RESOLVE_FAILURES {
					error!(error = %e, failures = self.resolve_failures, "resolving keeps failing, stopping playback");
					self.resolve_failures = 0;
					self.stop().await;
				} else {
					warn!(error = %e, title = %c.item.track.title, "could not resolve stream, skipping");
					self.finish_current().await;
					self.start_next().await;
				}
			}
		}
	}

	/// Stop playback and put the current item back at the front of the queue.
	async fn stop(&mut self) {
		if let Some(c) = self.current.take() {
			if let Some(d) = c.decoder {
				d.kill().await;
			}
			self.queue.insert(0, c.item);
			self.dirty = true;
		}
		let _ = self.audio_tx.try_send(AudioOut::End);
	}

	/// Move the current item to history.
	async fn finish_current(&mut self) {
		let Some(c) = self.current.take() else { return };
		let s = &c.stats;
		info!(
			title = %c.item.track.title,
			frames = c.frames_played,
			dropped = s.dropped,
			stalls = s.stalls,
			stall_ms = s.stall_ticks * 20,
			late_ticks = s.late_ticks,
			max_late_ms = s.max_late_ms,
			"track stats"
		);
		if let Some(d) = c.decoder {
			d.kill().await;
		}
		if c.talking {
			let _ = self.audio_tx.try_send(AudioOut::End);
		}
		self.history.insert(0, c.item);
		self.history.truncate(HISTORY_MAX);
		self.dirty = true;
	}

	// ------------------------------------------------------------------ tick

	/// `late`: how far behind schedule this tick fired.
	async fn on_tick(&mut self, late: Duration) {
		let mut finished = false;
		let mut retry: Option<(f64, bool)> = None; // (position, use_pipe)
		let mut skip_to: Option<f64> = None;
		let mut publish = false;

		if self.empty_since.is_some_and(|t| t.elapsed() > EMPTY_CHANNEL_GRACE)
			&& self.current.as_ref().is_some_and(|c| c.status == PlaybackStatus::Playing)
		{
			self.auto_pause().await;
			publish = true;
		}

		if let Some(c) = &mut self.current {
			if c.status == PlaybackStatus::Playing {
				let (cur_pos, cur_dur) = (c.position(), c.duration());
				let is_live = c.stream.as_ref().map(|s| s.is_live).unwrap_or(false);
				if let Some(dec) = &mut c.decoder {
					if !c.prebuffered {
						if dec.buffered() >= PREBUFFER_FRAMES || dec.is_finished() {
							c.prebuffered = true;
							publish = true;
						}
					}
					if c.prebuffered {
						if late > LATE_TICK {
							c.stats.late_ticks += 1;
							c.stats.max_late_ms = c.stats.max_late_ms.max(late.as_millis() as u64);
						}
						match dec.pull() {
							Pull::Frame(mut pcm) => {
								c.frames_played += 1;
								c.starved_ticks = 0;
								apply_gain(&mut pcm, self.volume);
								match self.encoder.encode(&pcm[..FRAME_LEN], &mut self.opus_buf) {
									Ok(len) => {
										if self.audio_tx.try_send(AudioOut::Frame(self.opus_buf[..len].to_vec())).is_ok() {
											c.talking = true;
										} else {
											c.stats.dropped += 1;
										}
									}
									Err(e) => warn!(error = %e, "opus encode failed"),
								}
								let pos = c.position();
								if let Some(seg) = c.segments.iter().find(|s| pos >= s.start && pos < s.end - 0.1) {
									debug!(category = %seg.category, from = pos, to = seg.end, "sponsorblock skip");
									skip_to = Some(seg.end);
								}
							}
							Pull::Starved => {
								if c.starved_ticks == 0 {
									c.stats.stalls += 1;
								}
								c.stats.stall_ticks += 1;
								c.starved_ticks += 1;
								if c.starved_ticks == 150 {
									warn!(title = %c.item.track.title, "audio starved for 3 s");
								}
								if c.starved_ticks > 1500 {
									// 30 s without data → restart the stream
									warn!("stream stalled, restarting");
									retry = Some((c.position(), c.pipe_fallback));
								}
							}
							Pull::Ended => {
								let pos = cur_pos;
								let dur = cur_dur;
								let err = dec.error_tail();
								let produced = dec.produced_frames;
								let ended_early = dur.map(|d| pos < d - 10.0).unwrap_or(false);
								if produced == 0 && c.retries < MAX_RETRIES {
									// Nothing decoded: likely an expired/403 URL → re-resolve or pipe.
									warn!(error = %err, "decoder produced no audio, retrying");
									retry = Some((c.base_pos, true));
								} else if ended_early && c.retries < MAX_RETRIES && !is_live {
									warn!(pos, ?dur, error = %err, "stream ended early, resuming");
									retry = Some((pos, c.pipe_fallback));
								} else {
									if produced == 0 && !err.is_empty() {
										self.last_error = Some(format!("{}: {}", c.item.track.title, err.lines().last().unwrap_or("")));
									}
									finished = true;
								}
							}
						}
					}
				} else if c.stream.is_some() {
					// decoder gone (seek past end) → finished
					finished = true;
				}
			}
		}

		if let Some(pos) = skip_to {
			self.seek(pos);
			publish = true;
		}
		if let Some((pos, use_pipe)) = retry {
			self.retry_current(pos, use_pipe).await;
			publish = true;
		}
		if finished {
			let title = self.current.as_ref().map(|c| c.item.track.title.clone()).unwrap_or_default();
			info!(title = %title, "track finished");
			self.finish_current().await;
			self.start_next().await;
			publish = true;
		}
		if publish || (self.current.is_some() && self.last_publish.elapsed() > Duration::from_secs(3)) {
			self.publish();
		}
	}

	async fn retry_current(&mut self, pos: f64, use_pipe: bool) {
		let cfg = self.cfg.clone();
		let Some(c) = &mut self.current else { return };
		c.retries += 1;
		if let Some(d) = c.decoder.take() {
			d.kill().await;
		}
		c.base_pos = pos;
		c.frames_played = 0;
		c.prebuffered = false;
		c.starved_ticks = 0;
		self.stream_cache.remove(&c.item.track.url);
		let direct = c.stream.as_ref().map(|s| s.direct).unwrap_or(false);
		if use_pipe && !direct {
			c.pipe_fallback = true;
			match Decoder::spawn_pipe(&cfg, &c.item.track.url, pos) {
				Ok(d) => c.decoder = Some(d),
				Err(e) => {
					self.last_error = Some(format!("{e:#}"));
					self.finish_current().await;
					self.start_next().await;
				}
			}
		} else {
			// Re-resolve the stream URL and restart ffmpeg from `pos`.
			c.status = PlaybackStatus::Loading;
			let gen = c.gen;
			let track = c.item.track.clone();
			self.spawn_load(gen, track, pos);
		}
	}
}

fn apply_gain(pcm: &mut [i16], gain: f32) {
	if (gain - 1.0).abs() < 0.001 {
		return;
	}
	for s in pcm.iter_mut() {
		*s = (*s as f32 * gain).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
	}
}
