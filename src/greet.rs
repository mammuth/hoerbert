//! Greets people joining the bot's channel with a random line from a JSON
//! file, spoken with TTS.
//!
//! ```json
//! [
//!   { "text": "Welcome back, {name}!", "lang": "en" },
//!   { "text": "Psst. {name} ist da.", "lang": "de" }
//! ]
//! ```
//! `{name}` is whoever joined. `lang` is the voice's language (`en`, `de`)
//! like `TTS_VOICE`, which is also the default. The file is created with the
//! defaults below if missing and re-read on every join, so edits apply
//! without a restart.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rand::seq::IndexedRandom;
use serde::Deserialize;
use tracing::{info, warn};

use crate::tts::{Speech, Tts};

/// Someone hopping between channels is greeted once per this.
const COOLDOWN: Duration = Duration::from_secs(60);
/// Give the newcomer's client a moment to start playing our audio.
pub const DELAY: Duration = Duration::from_secs(1);

const DEFAULTS: &str = r#"[
  { "text": "Hey {name}!", "lang": "en" },
  { "text": "Look who's here. It's {name}!", "lang": "en" },
  { "text": "Attention please, {name} has entered the chat.", "lang": "en" },
  { "text": "Welcome, {name}. We have been expecting you.", "lang": "en" },
  { "text": "A wild {name} appears!", "lang": "en" },
  { "text": "Oh, {name}! You scared me.", "lang": "en" },
  { "text": "{name} is here. Let the party begin!", "lang": "en" },
  { "text": "Psst. {name} is here.", "lang": "en" },
  { "text": "Moin {name}!", "lang": "de" },
  { "text": "Na, {name}, auch mal wieder hier?", "lang": "de" }
]
"#;

#[derive(Debug, Deserialize)]
struct Greeting {
	text: String,
	#[serde(default)]
	lang: Option<String>,
}

#[derive(Clone)]
pub struct Greeter {
	path: PathBuf,
	tts: Tts,
	/// Last greeting per user, for the cooldown.
	last: Arc<Mutex<HashMap<String, Instant>>>,
}

impl Greeter {
	/// Writes the default greetings if the file does not exist yet.
	pub fn new(path: PathBuf, tts: Tts) -> Self {
		if !path.exists() {
			match std::fs::write(&path, DEFAULTS) {
				Ok(()) => info!(path = %path.display(), "wrote default greetings"),
				Err(e) => warn!(error = %e, path = %path.display(), "cannot write default greetings"),
			}
		}
		Self { path, tts, last: Default::default() }
	}

	/// A random greeting for `name`; `None` during the cooldown or without usable greetings.
	pub fn greeting(&self, name: &str) -> Option<Speech> {
		let greetings = self.load()?;
		{
			let mut last = self.last.lock().unwrap();
			last.retain(|_, t| t.elapsed() < COOLDOWN);
			let key = name.trim().to_lowercase();
			if last.contains_key(&key) {
				return None;
			}
			last.insert(key, Instant::now());
		}
		self.pick(&greetings, name)
	}

	/// A random greeting for `name`, ignoring the cooldown (someone asked for it).
	pub fn greeting_now(&self, name: &str) -> Option<Speech> { self.pick(&self.load()?, name) }

	fn pick(&self, greetings: &[Greeting], name: &str) -> Option<Speech> {
		let g = greetings.choose(&mut rand::rng())?;
		match self.tts.speech(&g.text.replace("{name}", name), g.lang.as_deref()) {
			Ok(s) => Some(Speech { logged: false, greeting: true, ..s }),
			Err(e) => {
				warn!(error = %e, text = %g.text, "skipping greeting");
				None
			}
		}
	}

	fn load(&self) -> Option<Vec<Greeting>> {
		let bytes = match std::fs::read(&self.path) {
			Ok(b) => b,
			Err(e) => {
				warn!(error = %e, path = %self.path.display(), "cannot read greetings");
				return None;
			}
		};
		match serde_json::from_slice(&bytes) {
			Ok(g) => Some(g),
			Err(e) => {
				warn!(error = %e, path = %self.path.display(), "greetings file is invalid");
				None
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn defaults_parse() {
		let g: Vec<Greeting> = serde_json::from_str(DEFAULTS).unwrap();
		assert_eq!(g.len(), 10);
		assert!(g.iter().all(|g| g.text.contains("{name}") && g.lang.is_some()));
	}
}
