//! Voice commands spoken in the channel: "Hörbert play <song>", "Hörbert skip", …
//!
//! The server only sends us voice while someone in our channel is talking, so
//! this costs nothing when the bot is alone or everyone is quiet. Each speaker's
//! Opus stream is decoded straight to 16 kHz mono and cut into utterances at
//! pauses. Every utterance goes through a small Whisper model (`base`, ~10 % of
//! a core while people talk) that only looks for the wake word; utterances with
//! a command are transcribed again with a bigger model for an accurate title.
//!
//! Dedicated keyword spotters (sherpa-onnx KWS) were tried first and missed
//! most wake words on real TeamSpeak audio.

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use audiopus::coder::Decoder as OpusDecoder;
use audiopus::packet::Packet;
use audiopus::{Channels, MutSignals, SampleRate};
use sherpa_rs::whisper::{WhisperConfig, WhisperRecognizer};
use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::config::Config;
use crate::ts::TsEvent;

pub const RATE: usize = 16_000;

/// Frame RMS below this counts as silence (≈ -40 dBFS).
const QUIET_RMS: f32 = 0.01;
/// Silence inside a transmission that ends an utterance (open-mic users).
const END_SILENCE: usize = RATE * 6 / 10;
/// Longer utterances are processed in pieces.
const MAX_UTTERANCE: usize = RATE * 8;
/// No packets for this long = speaker paused / released push-to-talk.
const GAP: Duration = Duration::from_millis(500);
/// After a bare wake word (beep), the next utterance within this time is the command.
const AWAIT_COMMAND: Duration = Duration::from_secs(6);
const IDLE_DROP: Duration = Duration::from_secs(300);
/// The wake word counts within the first few words of an utterance (or
/// anywhere when a command word follows it).
const WAKE_WITHIN: usize = 3;
/// After the beep, longer sentences without "play" are conversation, not titles.
const MAX_TITLE_WORDS: usize = 8;

pub struct VoicePacket {
	pub from: u16,
	pub name: String,
	/// One Opus frame (1-byte end-of-transmission markers are ignored).
	pub opus: Vec<u8>,
}

#[derive(Debug)]
pub struct VoiceCommand {
	pub from: u16,
	pub name: String,
	/// Chat command without prefix, e.g. `play bohemian rhapsody`.
	pub text: String,
}

/// Played after a bare wake word, so people know to say the command now.
pub fn beep_path(cfg: &Config) -> std::path::PathBuf { cfg.data_dir.join("voice-beep.wav") }

/// Rising two-tone "bip-bip", 48 kHz mono 16-bit WAV.
fn write_beep(path: &Path) -> Result<()> {
	const SR: usize = 48_000;
	let mut pcm: Vec<i16> = Vec::new();
	for (freq, ms) in [(660.0f32, 70usize), (0.0, 30), (990.0, 70)] {
		let n = SR * ms / 1000;
		let fade = SR * 5 / 1000;
		pcm.extend((0..n).map(|i| {
			let env = (i.min(n - 1 - i) as f32 / fade as f32).min(1.0);
			let x = (2.0 * std::f32::consts::PI * freq * i as f32 / SR as f32).sin();
			(x * env * 0.5 * i16::MAX as f32) as i16
		}));
	}
	let data_len = (pcm.len() * 2) as u32;
	let mut wav = Vec::with_capacity(44 + data_len as usize);
	wav.extend_from_slice(b"RIFF");
	wav.extend_from_slice(&(36 + data_len).to_le_bytes());
	wav.extend_from_slice(b"WAVEfmt ");
	wav.extend_from_slice(&16u32.to_le_bytes());
	wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
	wav.extend_from_slice(&1u16.to_le_bytes()); // mono
	wav.extend_from_slice(&(SR as u32).to_le_bytes());
	wav.extend_from_slice(&(SR as u32 * 2).to_le_bytes());
	wav.extend_from_slice(&2u16.to_le_bytes());
	wav.extend_from_slice(&16u16.to_le_bytes());
	wav.extend_from_slice(b"data");
	wav.extend_from_slice(&data_len.to_le_bytes());
	wav.extend(pcm.iter().flat_map(|s| s.to_le_bytes()));
	std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
	std::fs::write(path, wav)?;
	Ok(())
}

/// Loads the models on a dedicated thread (the sherpa handles are not Send)
/// and returns the sender the TeamSpeak thread feeds packets into.
pub fn spawn(cfg: &Config, out: mpsc::Sender<TsEvent>) -> Result<std_mpsc::SyncSender<VoicePacket>> {
	write_beep(&beep_path(cfg)).context("writing voice beep")?;
	let cfg = cfg.clone();
	let (tx, rx) = std_mpsc::sync_channel::<VoicePacket>(512);
	let (ready_tx, ready_rx) = std_mpsc::channel();
	std::thread::Builder::new().name("voice".into()).spawn(move || {
		let mut engine = match Engine::load(&cfg) {
			Ok(e) => {
				let _ = ready_tx.send(Ok(()));
				e
			}
			Err(e) => {
				let _ = ready_tx.send(Err(e));
				return;
			}
		};
		loop {
			let mut cmds = Vec::new();
			match rx.recv_timeout(Duration::from_millis(100)) {
				// End-of-transmission marker: pauses are detected by time in `tick`.
				Ok(p) if p.opus.len() <= 1 => {}
				Ok(p) => cmds.extend(engine.opus(p.from, &p.name, &p.opus)),
				Err(std_mpsc::RecvTimeoutError::Timeout) => {}
				Err(std_mpsc::RecvTimeoutError::Disconnected) => return,
			}
			cmds.extend(engine.tick());
			for name in engine.wakes.drain(..) {
				let _ = out.blocking_send(TsEvent::VoiceListening { name });
			}
			for c in cmds {
				info!(user = %c.name, command = %c.text, "voice command");
				if out.blocking_send(TsEvent::Voice { from: c.from, name: c.name, text: c.text }).is_err() {
					return;
				}
			}
		}
	})?;
	ready_rx.recv().context("voice thread died")??;
	Ok(tx)
}

fn whisper(cfg: &Config, model: &str) -> Result<WhisperRecognizer> {
	let dir = cfg.voice_model_dir.join(format!("sherpa-onnx-whisper-{model}"));
	let file = |f: String| -> Result<String> {
		let p = dir.join(f);
		if !p.exists() {
			bail!("missing voice model file {}", p.display());
		}
		Ok(p.to_string_lossy().into_owned())
	};
	WhisperRecognizer::new(WhisperConfig {
		encoder: file(format!("{model}-encoder.int8.onnx"))?,
		decoder: file(format!("{model}-decoder.int8.onnx"))?,
		tokens: file(format!("{model}-tokens.txt"))?,
		language: cfg.voice_language.clone(),
		num_threads: Some(2),
		provider: Some("cpu".into()),
		..Default::default()
	})
	.map_err(|e| anyhow::anyhow!("loading whisper {model}: {e}"))
}

struct Speaker {
	name: String,
	opus: OpusDecoder,
	/// Current utterance.
	utt: Vec<f32>,
	quiet: usize,
	voiced: bool,
	/// Audio received since the last pause.
	talking: bool,
	last_audio: Instant,
	/// Bare wake word heard (beep played), waiting for the command.
	awaiting: Option<Instant>,
}

pub struct Engine {
	speakers: HashMap<u16, Speaker>,
	/// Small model that runs on everything, only to spot the wake word.
	gate: WhisperRecognizer,
	/// Bigger model for the actual command (song titles).
	command: WhisperRecognizer,
	wake: String,
	pcm: Vec<f32>,
	debug_user: Option<String>,
	debug_dir: std::path::PathBuf,
	/// Speakers that said a bare wake word and are now saying the command.
	pub wakes: Vec<String>,
}

impl Engine {
	pub fn load(cfg: &Config) -> Result<Self> {
		let t = Instant::now();
		let gate = whisper(cfg, &cfg.voice_gate_model)?;
		let command = whisper(cfg, &cfg.voice_whisper_model)?;
		info!(wake = %cfg.voice_wake, gate = %cfg.voice_gate_model, command = %cfg.voice_whisper_model, took_ms = t.elapsed().as_millis() as u64, "voice commands enabled");
		Ok(Self {
			speakers: HashMap::new(),
			gate,
			command,
			wake: cfg.voice_wake.clone(),
			pcm: vec![0.0; RATE * 120 / 1000],
			debug_user: cfg.voice_debug_user.clone(),
			debug_dir: cfg.data_dir.join("voice-debug"),
			wakes: Vec::new(),
		})
	}

	/// One Opus frame from a speaker.
	pub fn opus(&mut self, from: u16, name: &str, data: &[u8]) -> Option<VoiceCommand> {
		if !self.speakers.contains_key(&from) {
			let opus = OpusDecoder::new(SampleRate::Hz16000, Channels::Mono).ok()?;
			self.speakers.insert(from, Speaker {
				name: name.to_string(),
				opus,
				utt: Vec::new(),
				quiet: 0,
				voiced: false,
				talking: false,
				last_audio: Instant::now(),
				awaiting: None,
			});
		}
		let s = self.speakers.get_mut(&from)?;
		if !name.is_empty() && s.name != name {
			s.name = name.to_string();
		}
		let mut pcm = std::mem::take(&mut self.pcm);
		let decoded = Packet::try_from(data)
			.map_err(anyhow::Error::from)
			.and_then(|p| Ok(s.opus.decode_float(Some(p), MutSignals::try_from(&mut pcm[..])?, false)?));
		let out = match decoded {
			Ok(n) => self.pcm16k(from, &pcm[..n]),
			Err(e) => {
				debug!(from, len = data.len(), error = %e, "undecodable voice packet");
				None
			}
		};
		self.pcm = pcm;
		out
	}

	/// 16 kHz mono samples from a known speaker (≈ 20 ms at a time).
	fn pcm16k(&mut self, from: u16, samples: &[f32]) -> Option<VoiceCommand> {
		let s = self.speakers.get_mut(&from)?;
		s.last_audio = Instant::now();
		s.talking = true;
		s.utt.extend_from_slice(samples);
		let rms = (samples.iter().map(|x| x * x).sum::<f32>() / samples.len().max(1) as f32).sqrt();
		if rms < QUIET_RMS {
			s.quiet += samples.len();
		} else {
			s.quiet = 0;
			s.voiced = true;
		}
		if (s.voiced && s.quiet >= END_SILENCE) || s.utt.len() >= MAX_UTTERANCE {
			return self.utterance(from);
		}
		None
	}

	/// Pauses, expired beeps, idle speakers.
	pub fn tick(&mut self) -> Vec<VoiceCommand> {
		let now = Instant::now();
		let ids: Vec<u16> = self.speakers.iter().filter(|(_, s)| s.talking && now - s.last_audio > GAP).map(|(id, _)| *id).collect();
		let cmds = ids.into_iter().filter_map(|id| self.utterance(id)).collect();
		for s in self.speakers.values_mut() {
			if s.awaiting.is_some_and(|t| now - t > AWAIT_COMMAND) && !s.talking {
				debug!(user = %s.name, "wake word without a command");
				s.awaiting = None;
			}
		}
		self.speakers.retain(|_, s| now - s.last_audio < IDLE_DROP);
		cmds
	}

	/// The current utterance of a speaker is complete.
	fn utterance(&mut self, from: u16) -> Option<VoiceCommand> {
		let s = self.speakers.get_mut(&from)?;
		let samples = std::mem::take(&mut s.utt);
		let voiced = std::mem::replace(&mut s.voiced, false);
		s.quiet = 0;
		s.talking = false;
		let name = s.name.clone();
		let awaiting = s.awaiting.take().is_some();
		if !voiced {
			return None;
		}
		if self.debug_user.as_deref() == Some(name.as_str()) {
			// Raw 16 kHz mono f32le, replay with the `wav_files` test.
			let path = self.debug_dir.join(format!("{}-{name}.f32", unix_ms()));
			let bytes: Vec<u8> = samples.iter().flat_map(|x| x.to_le_bytes()).collect();
			let _ = std::fs::create_dir_all(&self.debug_dir).and_then(|_| std::fs::write(&path, bytes));
		}
		let ms = samples.len() * 1000 / RATE;
		if awaiting {
			// After the beep: the whole utterance is the command. Voice activation
			// often clips the "p" of "play", so anything else counts as a title.
			let t = Instant::now();
			let heard = self.command.transcribe(RATE as u32, &samples).text;
			info!(user = %name, heard = %heard.trim(), audio_ms = ms, took_ms = t.elapsed().as_millis() as u64, "command after beep");
			let text = parse_command(&heard).or_else(|| title(&heard).filter(|t| t.split(' ').count() <= MAX_TITLE_WORDS).map(|t| format!("play {t}")))?;
			return Some(VoiceCommand { from, name, text });
		}
		let t = Instant::now();
		let heard = self.gate.transcribe(RATE as u32, &samples).text;
		debug!(user = %name, heard = %heard.trim(), audio_ms = ms, took_ms = t.elapsed().as_millis() as u64, "gate");
		let rest = after_wake(&heard, &self.wake)?;
		if rest.is_empty() {
			info!(user = %name, heard = %heard.trim(), "wake word, waiting for the command");
			self.speakers.get_mut(&from)?.awaiting = Some(Instant::now());
			self.wakes.push(name);
			return None;
		}
		// Wake word + command in one go: transcribe again with the bigger model.
		let t = Instant::now();
		let full = self.command.transcribe(RATE as u32, &samples).text;
		info!(user = %name, gate = %heard.trim(), heard = %full.trim(), audio_ms = ms, took_ms = t.elapsed().as_millis() as u64, "command");
		let text = after_wake(&full, &self.wake).and_then(|r| parse_command(&r)).or_else(|| parse_command(&rest))?;
		Some(VoiceCommand { from, name, text })
	}
}

fn unix_ms() -> u128 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0) }

fn words(text: &str) -> Vec<String> {
	text.split_whitespace()
		.map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '&' && c != '/').to_string())
		.filter(|w| !w.is_empty())
		.collect()
}

fn levenshtein(a: &str, b: &str) -> usize {
	let b: Vec<char> = b.chars().collect();
	let mut prev: Vec<usize> = (0..=b.len()).collect();
	for (i, ca) in a.chars().enumerate() {
		let mut cur = vec![i + 1];
		for (j, cb) in b.iter().enumerate() {
			cur.push((prev[j] + (ca != *cb) as usize).min(prev[j + 1] + 1).min(cur[j] + 1));
		}
		prev = cur;
	}
	prev[b.len()]
}

/// How Whisper writes the wake word (Herbert, Herbette, Hør bært, Siri, Seri, …).
fn is_wake(word: &str, wake: &str) -> bool {
	let w = word.to_lowercase();
	match wake {
		"siri" => ["siri", "seri", "siehri", "siry"].iter().any(|v| levenshtein(&w, v) <= 1),
		_ => w == "robert" || ["herbert", "hörbert", "hoerbert", "hubert"].iter().any(|v| levenshtein(&w, v) <= 2),
	}
}

fn is_command_word(word: &str) -> bool { parse_command(&format!("{word} x")).is_some() }

/// Text after the wake word, `None` if the utterance isn't addressed to us.
fn after_wake(text: &str, wake: &str) -> Option<String> {
	let words = words(text);
	// Two-word spellings like "Hør bært".
	let joined: Vec<String> = words.windows(2).map(|w| format!("{}{}", w[0], w[1])).collect();
	let hit = (0..words.len()).find_map(|i| {
		let len = if is_wake(&words[i], wake) {
			1
		} else if joined.get(i).is_some_and(|j| is_wake(j, wake)) {
			2
		} else {
			return None;
		};
		let next = words.get(i + len).map(String::as_str).unwrap_or("");
		(i < WAKE_WITHIN || is_command_word(next)).then_some(i + len)
	})?;
	Some(words[hit..].iter().skip_while(|w| is_wake(w, wake)).cloned().collect::<Vec<_>>().join(" "))
}

const FILLER: [&str; 8] = ["hey", "ok", "okay", "bitte", "please", "mal", "jetzt", "und"];

/// A song title from free text: drops punctuation-only words, fillers and
/// Whisper's well-known hallucinations on near-silence.
fn title(raw: &str) -> Option<String> {
	let lower = raw.to_lowercase();
	if ["untertitel", "amara.org", "thank you for watching", "thanks for watching", "[music", "[video"].iter().any(|h| lower.contains(h)) {
		return None;
	}
	let words = words(raw);
	let t = words.iter().skip_while(|w| FILLER.contains(&w.to_lowercase().as_str())).cloned().collect::<Vec<_>>().join(" ");
	(!t.is_empty() && !["you", "thank you", "bye"].contains(&t.to_lowercase().as_str())).then_some(t)
}

/// Whisper output → chat command (`play <title>`, `skip`, …).
fn parse_command(raw: &str) -> Option<String> {
	let words = words(raw);
	let mut it = words.iter().skip_while(|w| FILLER.contains(&w.to_lowercase().as_str()));
	let first = it.next()?.to_lowercase();
	let rest = it.cloned().collect::<Vec<_>>().join(" ");
	match first.as_str() {
		// "lay": the clipped "play".
		"play" | "plays" | "lay" | "spiel" | "spiele" | "spielt" | "playing" => title(&rest).map(|t| format!("play {t}")),
		"skip" | "next" | "nächster" | "nächstes" | "nächste" | "weiter" | "überspringen" => Some("skip".into()),
		"pause" | "stop" | "stopp" | "halt" | "pausiere" => Some("pause".into()),
		"resume" | "continue" | "fortsetzen" => Some("resume".into()),
		_ => None,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Feeds raw 16 kHz mono f32le files through the engine, one utterance each, e.g.
	/// `TS_ADDRESS=x VOICE_TEST_FILES=a.f32,b.f32 cargo test --release wav_files -- --ignored --nocapture`
	#[test]
	#[ignore]
	fn wav_files() {
		let _ = tracing_subscriber::fmt().with_env_filter("hoerbert=debug").without_time().with_ansi(false).try_init();
		let cfg = Config::from_env().unwrap();
		let mut engine = Engine::load(&cfg).unwrap();
		for path in std::env::var("VOICE_TEST_FILES").unwrap().split(',') {
			let bytes = std::fs::read(path).unwrap();
			let samples: Vec<f32> = bytes.chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect();
			engine.opus(1, "test", &[]); // registers the speaker (undecodable)
			let mut cmds: Vec<VoiceCommand> = samples.chunks(RATE / 50).filter_map(|c| engine.pcm16k(1, c)).collect();
			cmds.extend(engine.utterance(1));
			println!("{path}: {:?}", cmds.iter().map(|c| &c.text).collect::<Vec<_>>());
		}
	}

	#[test]
	fn finds_wake_word() {
		let h = |s: &str| after_wake(s, "hoerbert");
		assert_eq!(h("Herbert play Bohemian Repsody"), Some("play Bohemian Repsody".into()));
		assert_eq!(h("Herbette, play another one bites the dust."), Some("play another one bites the dust".into()));
		assert_eq!(h("Hør bært, play Bohemian Rhapsody."), Some("play Bohemian Rhapsody".into()));
		assert_eq!(h("Robert Play Bohemia & Rhapsody"), Some("Play Bohemia & Rhapsody".into()));
		assert_eq!(h("Hörbert"), Some("".into()));
		assert_eq!(h("Aber es gibt ja nochmal den Herbert."), None);
		assert_eq!(h("ich hab dem Herbert gesagt, play mal was"), None);
		let s = |t: &str| after_wake(t, "siri");
		assert_eq!(s("online. Siri."), Some("".into()));
		assert_eq!(s("Siri Siri Siri"), Some("".into()));
		assert_eq!(s("I can't get the positive course, please. Siri, lay Bohemian Rhapsody."), Some("lay Bohemian Rhapsody".into()));
		assert_eq!(s("ob es mit Siri besser klappt"), None);
	}

	#[test]
	fn parses_commands() {
		let p = |s: &str| parse_command(s);
		assert_eq!(p(" Play Bohemian Rhapsody."), Some("play Bohemian Rhapsody".into()));
		assert_eq!(p("spiel bitte Atemlos durch die Nacht!"), Some("play Atemlos durch die Nacht".into()));
		assert_eq!(p("lay Bohemian Rhapsody"), Some("play Bohemian Rhapsody".into()));
		assert_eq!(p("Play AC/DC, Highway to Hell"), Some("play AC/DC Highway to Hell".into()));
		assert_eq!(p("Skip."), Some("skip".into()));
		assert_eq!(p("Nächster!"), Some("skip".into()));
		assert_eq!(p("Stopp"), Some("pause".into()));
		assert_eq!(p("play"), None);
		assert_eq!(p("Wie spät ist es?"), None);
		assert_eq!(title("Labohemian Rhapsody."), Some("Labohemian Rhapsody".into()));
		assert_eq!(title("Thank you."), None);
		assert_eq!(title("Untertitel im Auftrag des ZDF"), None);
	}
}
