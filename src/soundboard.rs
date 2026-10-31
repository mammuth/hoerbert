//! Soundboard: short clips listed in a JSON file, triggered by `!<slug>`
//! or the web UI and mixed over whatever is playing.
//!
//! ```json
//! [{ "slug": "airhorn", "name": "Airhorn", "emoji": "📯", "url": "sounds/airhorn.mp3", "volume": 0.8 }]
//! ```
//! `url` is anything ffmpeg can read; relative paths resolve against the
//! file's directory. `volume` (default 1.0) evens out loud and quiet clips.
//! The file is re-read on every lookup, so edits apply without a restart.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::warn;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sound {
	pub slug: String,
	pub name: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub emoji: Option<String>,
	/// Not exposed to the web UI (may be a local path).
	#[serde(skip_serializing)]
	pub url: String,
	#[serde(default = "default_volume", skip_serializing)]
	pub volume: f32,
}

fn default_volume() -> f32 { 1.0 }

#[derive(Clone)]
pub struct Soundboard {
	path: PathBuf,
}

impl Soundboard {
	pub fn new(path: PathBuf) -> Self { Self { path } }

	pub fn list(&self) -> Vec<Sound> {
		let bytes = match std::fs::read(&self.path) {
			Ok(b) => b,
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => return vec![],
			Err(e) => {
				warn!(error = %e, path = %self.path.display(), "cannot read soundboard file");
				return vec![];
			}
		};
		let sounds: Vec<Sound> = match serde_json::from_slice(&bytes) {
			Ok(s) => s,
			Err(e) => {
				warn!(error = %e, path = %self.path.display(), "soundboard file is invalid");
				return vec![];
			}
		};
		let base = self.path.parent().unwrap_or(Path::new("."));
		sounds
			.into_iter()
			.filter_map(|mut s| {
				s.slug = s.slug.trim().to_ascii_lowercase();
				if s.slug.is_empty() || !s.slug.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
					warn!(slug = %s.slug, "skipping sound: slug must be [a-z0-9_-]");
					return None;
				}
				if !s.url.contains("://") && Path::new(&s.url).is_relative() {
					s.url = base.join(&s.url).to_string_lossy().into_owned();
				}
				Some(s)
			})
			.collect()
	}

	pub fn get(&self, slug: &str) -> Option<Sound> {
		self.list().into_iter().find(|s| s.slug == slug)
	}
}
