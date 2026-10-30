//! Configuration from environment variables (12-factor, docker friendly).

use std::path::PathBuf;

use anyhow::{bail, Result};

#[derive(Clone, Debug)]
pub struct Config {
	pub ts_address: String,
	pub ts_name: String,
	pub ts_channel: Option<String>,
	pub ts_channel_password: Option<String>,
	pub ts_server_password: Option<String>,
	pub web_bind: String,
	pub public_url: Option<String>,
	pub data_dir: PathBuf,
	pub ytdlp_bin: String,
	pub ffmpeg_bin: String,
	pub ytdlp_args: Vec<String>,
	pub sponsorblock: bool,
	pub sponsorblock_categories: Vec<String>,
	pub opus_bitrate: i32,
	pub default_volume: f32,
	pub command_prefix: String,
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
		Ok(Self {
			ts_address,
			ts_name: env("TS_NAME").unwrap_or_else(|| "Hörbert".into()),
			ts_channel: env("TS_CHANNEL"),
			ts_channel_password: env("TS_CHANNEL_PASSWORD"),
			ts_server_password: env("TS_SERVER_PASSWORD"),
			web_bind: env("WEB_BIND").unwrap_or_else(|| "0.0.0.0:8080".into()),
			public_url: env("PUBLIC_URL").map(|u| u.trim_end_matches('/').to_string()),
			data_dir: env("DATA_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("data")),
			ytdlp_bin: env("YTDLP_BIN").unwrap_or_else(|| "yt-dlp".into()),
			ffmpeg_bin: env("FFMPEG_BIN").unwrap_or_else(|| "ffmpeg".into()),
			ytdlp_args,
			sponsorblock: env("SPONSORBLOCK").map(|v| v != "false" && v != "0").unwrap_or(true),
			sponsorblock_categories,
			opus_bitrate: env("OPUS_BITRATE").and_then(|v| v.parse().ok()).unwrap_or(128_000),
			default_volume: env("DEFAULT_VOLUME").and_then(|v| v.parse().ok()).unwrap_or(0.1),
			command_prefix: env("COMMAND_PREFIX").unwrap_or_else(|| "!".into()),
		})
	}
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
