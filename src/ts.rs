//! TeamSpeak client thread. `tsclientlib::Connection` is !Send, so it lives
//! on its own thread with a current-thread runtime and talks to the rest of
//! the app over channels.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use futures::StreamExt;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use tsclientlib::events::Event;
use tsclientlib::prelude::*;
use tsclientlib::{ChannelId, Connection, Identity, Invoker, MessageTarget, StreamItem};
use tsproto_packets::packets::{AudioData, CodecType, OutAudio};

use crate::config::Config;
use crate::model::TsStatus;

pub enum AudioOut {
	/// One 20 ms Opus frame.
	Frame(Vec<u8>),
	/// Stop talking (sends the empty end-of-stream packet).
	End,
}

pub enum TsCmd {
	SendMessage { target: MessageTarget, text: String },
}

pub enum TsEvent {
	Status(TsStatus),
	Message { target: MessageTarget, invoker: Invoker, text: String },
}

pub struct TsChannels {
	pub audio_rx: mpsc::Receiver<AudioOut>,
	pub cmd_rx: mpsc::Receiver<TsCmd>,
	pub event_tx: mpsc::Sender<TsEvent>,
	/// Flipped to `true` on shutdown: disconnect cleanly so the server frees
	/// our slot immediately (otherwise a restart hits ClientTooManyClonesConnected
	/// until the ghost client times out).
	pub shutdown: tokio::sync::watch::Receiver<bool>,
	/// Signalled once we have disconnected.
	pub done_tx: mpsc::Sender<()>,
}

pub fn spawn(cfg: Config, chans: TsChannels) {
	std::thread::Builder::new()
		.name("teamspeak".into())
		.spawn(move || {
			let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("ts runtime");
			rt.block_on(run(cfg, chans));
		})
		.expect("spawn ts thread");
}

fn identity_path(cfg: &Config) -> PathBuf { cfg.data_dir.join("identity.json") }

fn load_or_create_identity(cfg: &Config) -> Result<Identity> {
	let path = identity_path(cfg);
	if let Ok(bytes) = std::fs::read(&path) {
		match serde_json::from_slice::<Identity>(&bytes) {
			Ok(id) => return Ok(id),
			Err(e) => {
				// Keep the broken file around instead of silently replacing a
				// (possibly privileged) identity, but don't get stuck forever.
				let backup = path.with_extension("json.corrupt");
				warn!(error = %e, backup = %backup.display(), "identity.json is corrupt, generating a new identity");
				let _ = std::fs::rename(&path, &backup);
			}
		}
	}
	info!("generating new TeamSpeak identity");
	let id = Identity::create();
	save_identity(cfg, &id)?;
	Ok(id)
}

/// Atomic write (tmp + rename) so a crash or full disk never leaves a
/// half-written identity behind.
fn save_identity(cfg: &Config, id: &Identity) -> Result<()> {
	std::fs::create_dir_all(&cfg.data_dir).context("creating data dir")?;
	let path = identity_path(cfg);
	let tmp = path.with_extension("json.tmp");
	std::fs::write(&tmp, serde_json::to_vec_pretty(id)?).context("writing identity")?;
	std::fs::rename(&tmp, &path).context("renaming identity")?;
	Ok(())
}

async fn run(cfg: Config, mut chans: TsChannels) {
	let mut backoff = Duration::from_secs(2);
	loop {
		if *chans.shutdown.borrow() {
			let _ = chans.done_tx.send(()).await;
			return;
		}
		match session(&cfg, &mut chans).await {
			Ok(()) => {
				if *chans.shutdown.borrow() {
					info!("teamspeak disconnected");
					let _ = chans.done_tx.send(()).await;
					return;
				}
				info!("teamspeak session ended, reconnecting");
				backoff = Duration::from_secs(2);
			}
			Err(e) => {
				error!(error = %format!("{e:#}"), "teamspeak connection failed");
				let _ = chans
					.event_tx
					.send(TsEvent::Status(TsStatus {
						connected: false,
						server: cfg.ts_address.clone(),
						channel: None,
						listeners: 0,
						error: Some(format!("{e:#}")),
					}))
					.await;
				let mut shutdown = chans.shutdown.clone();
				tokio::select! {
					_ = tokio::time::sleep(backoff) => {}
					_ = shutdown.changed() => {}
				}
				backoff = (backoff * 2).min(Duration::from_secs(60));
			}
		}
	}
}

fn status_from_book(cfg: &Config, con: &Connection, connected: bool) -> TsStatus {
	let mut status = TsStatus { connected, server: cfg.ts_address.clone(), channel: None, listeners: 0, error: None };
	if let Ok(book) = con.get_state() {
		if let Some(me) = book.clients.get(&book.own_client) {
			status.channel = book.channels.get(&me.channel).map(|c| c.name.clone());
			status.listeners = book
				.clients
				.values()
				.filter(|c| c.channel == me.channel && c.id != me.id && !c.output_muted && !c.away_message.is_some())
				.count() as u32;
		}
		if !status.server.contains(&book.server.name) {
			status.server = format!("{} ({})", book.server.name, cfg.ts_address);
		}
	}
	status
}

async fn session(cfg: &Config, chans: &mut TsChannels) -> Result<()> {
	let identity = load_or_create_identity(cfg)?;
	let saved_counter = identity.counter();
	let mut opts = Connection::build(cfg.ts_address.clone())
		.identity(identity)
		.name(cfg.ts_name.clone())
		.output_hardware_enabled(true)
		.input_hardware_enabled(true);
	if let Some(ch) = &cfg.ts_channel {
		opts = opts.channel(ch.clone());
	}
	if let Some(pw) = &cfg.ts_channel_password {
		opts = opts.channel_password(pw.clone());
	}
	if let Some(pw) = &cfg.ts_server_password {
		opts = opts.password(pw.clone());
	}
	info!(address = %cfg.ts_address, name = %cfg.ts_name, "connecting to teamspeak");
	let mut con = opts.connect().context("connect")?;

	// Wait until connected (first BookEvents) — the library handles identity
	// level upgrades on its own, which can take a while on strict servers.
	loop {
		let item = tokio::time::timeout(Duration::from_secs(300), con.events().next())
			.await
			.context("timeout while connecting")?;
		match item {
			Some(Ok(StreamItem::BookEvents(_))) => break,
			Some(Ok(StreamItem::IdentityLevelIncreasing(lvl))) => info!(level = lvl, "server requires a higher identity level, computing…"),
			Some(Ok(StreamItem::IdentityLevelIncreased)) => info!("identity level increased"),
			Some(Ok(other)) => debug!(?other, "pre-connect event"),
			Some(Err(e)) => return Err(e).context("while connecting"),
			None => anyhow::bail!("connection closed before it was established"),
		}
	}
	if let Some(id) = con.get_options().get_identity() {
		if id.counter() != saved_counter {
			if let Err(e) = save_identity(cfg, id) {
				warn!(error = %e, "could not persist upgraded identity");
			}
		}
	}
	info!("connected to teamspeak");
	let mut own_id = con.get_state().ok().map(|b| b.own_client);
	let send_status = |con: &Connection, connected: bool| {
		let _ = chans.event_tx.try_send(TsEvent::Status(status_from_book(cfg, con, connected)));
	};
	send_status(&con, true);
	if !con.can_send_audio() {
		warn!("bot is muted (talk power / mute flags) — audio will be dropped by the server");
	}

	// Try to make sure we are not flagged as muted (some servers/clients
	// show the bot muted otherwise).
	if let Ok(book) = con.get_state() {
		let cmd = tsclientlib::messages::c2s::OutClientUpdateMessage::new(&mut std::iter::once(
			book.client_update().set_input_muted(false).set_output_muted(false),
		));
		let _ = cmd.send(&mut con);
	}

	let mut packet_id: u16 = 0;
	let mut muted_warned = false;
	let mut shutdown = chans.shutdown.clone();
	let mut net_stats = tokio::time::interval(Duration::from_secs(30));
	loop {
		tokio::select! {
			_ = net_stats.tick() => log_network_stats(&con),
			_ = shutdown.changed() => {
				info!("disconnecting from teamspeak");
				let _ = con.disconnect(tsclientlib::DisconnectOptions::new());
				let _ = tokio::time::timeout(Duration::from_secs(3), async {
					while let Some(_) = con.events().next().await {}
				}).await;
				return Ok(());
			}
			item = async { con.events().next().await } => {
				match item {
					Some(Ok(StreamItem::BookEvents(events))) => {
						let mut status_changed = false;
						for ev in events {
							match ev {
								Event::Message { target, invoker, message } => {
									if Some(invoker.id) == own_id { continue; }
									let _ = chans.event_tx.send(TsEvent::Message { target, invoker, text: message }).await;
								}
								Event::PropertyAdded { .. } | Event::PropertyChanged { .. } | Event::PropertyRemoved { .. } => {
									status_changed = true;
								}
							}
						}
						if status_changed {
							own_id = con.get_state().ok().map(|b| b.own_client);
							send_status(&con, true);
						}
					}
					Some(Ok(StreamItem::DisconnectedTemporarily(reason))) => {
						warn!(?reason, "temporarily disconnected, library is reconnecting");
						send_status(&con, false);
					}
					Some(Ok(StreamItem::AudioChange(ev))) => {
						debug!(?ev, "audio state changed");
						muted_warned = false;
					}
					Some(Ok(_)) => {}
					Some(Err(e)) => return Err(e).context("connection error"),
					None => return Ok(()),
				}
			}
			Some(a) = chans.audio_rx.recv() => {
				let (codec, data): (CodecType, Vec<u8>) = match a {
					AudioOut::Frame(d) => (CodecType::OpusMusic, d),
					AudioOut::End => (CodecType::OpusMusic, Vec::new()),
				};
				if !con.can_send_audio() {
					if !muted_warned {
						warn!("cannot send audio right now (muted / missing talk power)");
						muted_warned = true;
					}
					continue;
				}
				packet_id = packet_id.wrapping_add(1);
				let packet = OutAudio::new(&AudioData::C2S { id: packet_id, codec, data: &data });
				if let Err(e) = con.send_audio(packet) {
					warn!(error = %e, "failed to send audio packet");
				}
			}
			Some(cmd) = chans.cmd_rx.recv() => {
				match cmd {
					TsCmd::SendMessage { target, text } => {
						let outs: Vec<_> = match con.get_state() {
							Ok(book) => chunk_message(&text).iter().map(|c| book.send_message(target, c)).collect(),
							Err(_) => Vec::new(),
						};
						for out in outs {
							if let Err(e) = out.send(&mut con) {
								warn!(error = %e, "failed to send chat message");
							}
						}
					}
				}
			}
		}
	}
}

/// Path health; packet loss is measured server → bot (keepalives).
fn log_network_stats(con: &Connection) {
	let Ok(s) = con.get_network_stats() else { return };
	let (rtt_ms, rtt_dev_ms) = (s.rtt.as_millis() as u64, s.rtt_dev.as_millis() as u64);
	let loss = s.get_packetloss_s2c_keepalive();
	// What tsclientlib reports to the server (shown in the TS client's connection info).
	let (speech, control, total) = (s.get_packetloss_s2c_speech(), s.get_packetloss_s2c_control(), s.get_packetloss_s2c_total());
	if loss > 0.01 || total > 0.01 || rtt_dev_ms > 30 {
		warn!(rtt_ms, rtt_dev_ms, loss, speech, control, total, "network stats: unstable connection");
	} else {
		debug!(rtt_ms, rtt_dev_ms, loss, speech, control, total, "network stats");
	}
}

/// TeamSpeak limits messages to 1024 bytes; split on lines.
fn chunk_message(text: &str) -> Vec<String> {
	let mut out = Vec::new();
	let mut cur = String::new();
	for line in text.lines() {
		if cur.len() + line.len() + 1 > 900 && !cur.is_empty() {
			out.push(std::mem::take(&mut cur));
		}
		if !cur.is_empty() {
			cur.push('\n');
		}
		cur.push_str(line);
	}
	if !cur.is_empty() {
		out.push(cur);
	}
	out
}

#[allow(dead_code)]
pub fn channel_id(id: u64) -> ChannelId { ChannelId(id) }
