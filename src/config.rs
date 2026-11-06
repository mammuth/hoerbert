//! Configuration from environment variables (12-factor, docker friendly).

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rand::Rng;

#[derive(Clone, Debug)]
pub struct Config {
	pub ts_address: String,
	pub ts_name: String,
	pub ts_channel: Option<String>,
	pub ts_channel_password: Option<String>,
	pub ts_server_password: Option<String>,
	pub web_bind: String,
	pub public_url: Option<String>,
	/// Shared secret for the web UI, handed out via the `!help` link.
	pub web_key: String,
	/// Show every channel and who is in it in the web UI.
	pub server_view: bool,
	pub data_dir: PathBuf,
	pub ytdlp_bin: String,
	pub ffmpeg_bin: String,
	pub ytdlp_args: Vec<String>,
	pub sponsorblock: bool,
	pub sponsorblock_categories: Vec<String>,
	pub opus_bitrate: i32,
	pub default_volume: f32,
	pub command_prefix: String,
	pub help_suffix: Option<String>,
	pub soundboard_file: PathBuf,
	pub soundboard_volume: f32,
	/// Drop history tracks and events older than this.
	pub history_hours: u64,
	pub tts: bool,
	pub espeak_bin: String,
	/// espeak-ng voice, optionally with a variant: `en`, `de+croak`.
	pub tts_voice: String,
	pub tts_speed: u32,
	pub tts_max_chars: usize,
	/// Seconds a user has to wait between two messages.
	pub tts_cooldown: u64,
	/// Speech loudness relative to the music.
	pub tts_volume: f32,
	/// Music gain while someone is talking.
	pub tts_duck: f32,
	/// Say hello to people joining the channel.
	pub greet: bool,
	pub greetings_file: PathBuf,
}

fn env(name: &str) -> Option<String> {
	std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

impl Config {
	pub fn from_env() -> Result<Self> {
		let Some(ts_address) = env("TS_ADDRESS") else {
			bail!("TS_ADDRESS is required (e.g. ts.example.com or ts.example.com:9987)");
		};
		let ytdlp_args = env("YTDLP_ARGS")
			.map(|s| shell_split(&s))
			.unwrap_or_default();
		let sponsorblock_categories = env("SPONSORBLOCK_CATEGORIES")
			.unwrap_or_else(|| "sponsor,selfpromo,interaction,music_offtopic".into())
			.split(',')
			.map(|s| s.trim().to_string())
			.filter(|s| !s.is_empty())
			.collect();
		let data_dir = env("DATA_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("data"));
		let web_key = match env("WEB_KEY") {
			Some(k) => k,
			None => load_or_create_web_key(&data_dir)?,
		};
		if !web_key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
			bail!("WEB_KEY may only contain letters, digits, '-' and '_'");
		}
		Ok(Self {
			ts_address,
			ts_name: env("TS_NAME").unwrap_or_else(|| "Hörbert".into()),
			ts_channel: env("TS_CHANNEL"),
			ts_channel_password: env("TS_CHANNEL_PASSWORD"),
			ts_server_password: env("TS_SERVER_PASSWORD"),
			web_bind: env("WEB_BIND").unwrap_or_else(|| "0.0.0.0:8080".into()),
			public_url: env("PUBLIC_URL").map(|u| u.trim_end_matches('/').to_string()),
			web_key,
			server_view: env("SERVER_VIEW").map(|v| v != "false" && v != "0").unwrap_or(true),
			soundboard_file: env("SOUNDBOARD_FILE").map(PathBuf::from).unwrap_or_else(|| data_dir.join("sounds.json")),
			soundboard_volume: env("SOUNDBOARD_VOLUME").and_then(|v| v.parse().ok()).unwrap_or(5.0),
			history_hours: env("HISTORY_HOURS").and_then(|v| v.parse().ok()).unwrap_or(168),
			tts: env("TTS").map(|v| v != "false" && v != "0").unwrap_or(true),
			espeak_bin: env("ESPEAK_BIN").unwrap_or_else(|| "espeak-ng".into()),
			tts_voice: env("TTS_VOICE").unwrap_or_else(|| "en".into()),
			tts_speed: env("TTS_SPEED").and_then(|v| v.parse().ok()).unwrap_or(160),
			tts_max_chars: env("TTS_MAX_CHARS").and_then(|v| v.parse().ok()).unwrap_or(200),
			tts_cooldown: env("TTS_COOLDOWN").and_then(|v| v.parse().ok()).unwrap_or(1),
			tts_volume: env("TTS_VOLUME").and_then(|v| v.parse().ok()).unwrap_or(6.0),
			tts_duck: env("TTS_DUCK").and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.3).clamp(0.0, 1.0),
			greet: env("GREET").map(|v| v != "false" && v != "0").unwrap_or(true),
			greetings_file: env("GREETINGS_FILE").map(PathBuf::from).unwrap_or_else(|| data_dir.join("greetings.json")),
			data_dir,
			ytdlp_bin: env("YTDLP_BIN").unwrap_or_else(|| "yt-dlp".into()),
			ffmpeg_bin: env("FFMPEG_BIN").unwrap_or_else(|| "ffmpeg".into()),
			ytdlp_args,
			sponsorblock: env("SPONSORBLOCK").map(|v| v != "false" && v != "0").unwrap_or(true),
			sponsorblock_categories,
			opus_bitrate: env("OPUS_BITRATE").and_then(|v| v.parse().ok()).unwrap_or(128_000),
			default_volume: env("DEFAULT_VOLUME").and_then(|v| v.parse().ok()).unwrap_or(0.1),
			command_prefix: env("COMMAND_PREFIX").unwrap_or_else(|| "!".into()),
			help_suffix: env("HELP_SUFFIX").map(|s| s.replace("\\n", "\n")),
		})
	}
}

impl Config {
	/// Public web UI link that also unlocks it.
	pub fn web_link(&self) -> Option<String> {
		self.public_url.as_ref().map(|u| format!("{u}/?key={}", self.web_key))
	}
}

/// Short and readable rather than secure: guesses are throttled server-side.
/// Leaves out l and o, which read like 1 and 0.
fn load_or_create_web_key(data_dir: &Path) -> Result<String> {
	let path = data_dir.join("web_key");
	if let Ok(k) = std::fs::read_to_string(&path) {
		let k = k.trim();
		if !k.is_empty() {
			return Ok(k.to_string());
		}
	}
	const ALPHABET: &[u8] = b"abcdefghijkmnpqrstuvwxyz";
	let mut rng = rand::rng();
	let key: String = (0..5).map(|_| ALPHABET[rng.random_range(0..ALPHABET.len())] as char).collect();
	std::fs::create_dir_all(data_dir).with_context(|| format!("creating data dir {}", data_dir.display()))?;
	std::fs::write(&path, &key).with_context(|| format!("writing {}", path.display()))?;
	Ok(key)
}

/// Minimal shell-like splitting (supports double/single quotes).
fn shell_split(s: &str) -> Vec<String> {
	let mut out = Vec::new();
	let mut cur = String::new();
	let mut quote: Option<char> = None;
	let mut had = false;
	for c in s.chars() {
		match (quote, c) {
			(Some(q), c) if c == q => quote = None,
			(Some(_), c) => cur.push(c),
			(None, '"' | '\'') => { quote = Some(c); had = true; }
			(None, c) if c.is_whitespace() => {
				if had || !cur.is_empty() { out.push(std::mem::take(&mut cur)); had = false; }
			}
			(None, c) => cur.push(c),
		}
	}
	if had || !cur.is_empty() { out.push(cur); }
	out
}
