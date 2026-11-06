//! Text to speech: `!tts <text>` or the web UI, spoken over the (ducked)
//! music with Piper voices from the TTS server (`tts/piper_server.py`).
//!
//! A voice is a language (`en`, `de`); the server speaks languages it has
//! no voice for with its default one.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tracing::info;

use crate::config::Config;

#[derive(Debug, thiserror::Error)]
pub enum TtsError {
	#[error("{0}")]
	Invalid(String),
	#[error("not so fast, wait {0}s")]
	Cooldown(u64),
}

/// A message waiting to be spoken.
pub struct Speech {
	pub text: String,
	/// Language of the voice.
	pub voice: String,
	/// Shown in the history; greetings are not.
	pub logged: bool,
}

#[derive(Clone)]
pub struct Tts {
	cfg: Config,
	/// Last message per user, for the cooldown.
	last: Arc<Mutex<HashMap<String, Instant>>>,
}

impl Tts {
	pub fn new(cfg: Config) -> Self {
		let tts = Self { cfg, last: Default::default() };
		if tts.enabled() {
			info!(url = ?tts.cfg.tts_url, "tts enabled");
		}
		tts
	}

	/// Needs `TTS` on and a TTS server.
	pub fn enabled(&self) -> bool { self.cfg.tts && self.cfg.tts_url.is_some() }

	pub fn max_chars(&self) -> usize { self.cfg.tts_max_chars }

	/// Cleans the text, picks the voice and enforces the per-user cooldown.
	pub fn prepare(&self, text: &str, voice: Option<&str>, by: &str) -> Result<Speech, TtsError> {
		let speech = self.speech(text, voice)?;
		self.cooldown(by)?;
		Ok(speech)
	}

	/// Per-user cooldown: fails if `by` spoke too recently, otherwise starts it.
	pub fn cooldown(&self, by: &str) -> Result<(), TtsError> {
		let cooldown = Duration::from_secs(self.cfg.tts_cooldown);
		let mut last = self.last.lock().unwrap();
		last.retain(|_, t| t.elapsed() < cooldown);
		let key = by.trim().to_lowercase();
		if let Some(t) = last.get(&key) {
			return Err(TtsError::Cooldown((cooldown - t.elapsed()).as_secs() + 1));
		}
		last.insert(key, Instant::now());
		Ok(())
	}

	/// Cleans the text and picks the voice, no cooldown.
	pub fn speech(&self, text: &str, voice: Option<&str>) -> Result<Speech, TtsError> {
		if !self.enabled() {
			return Err(TtsError::Invalid("tts is off".into()));
		}
		let text = clean(text, self.cfg.tts_max_chars);
		if text.is_empty() {
			return Err(TtsError::Invalid("nothing to say".into()));
		}
		let voice = voice.and_then(language).or_else(|| language(&self.cfg.tts_voice)).unwrap_or_else(|| "en".into());
		Ok(Speech { voice, text, logged: true })
	}
}

/// `de`, `en-us`… from a voice; drops old espeak-ng variants (`en+croak` → `en`)
/// and anything that could not be a language.
fn language(voice: &str) -> Option<String> {
	let lang = voice.split('+').next()?.trim().to_lowercase();
	(!lang.is_empty() && lang.len() <= 10 && lang.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')).then_some(lang)
}

/// Links read out loud are no fun: say "link" instead. Collapses whitespace
/// and cuts the text at `max` characters.
fn clean(text: &str, max: usize) -> String {
	let words: Vec<&str> = text
		.split_whitespace()
		.map(|w| if w.contains("://") || w.starts_with("www.") { "link" } else { w })
		.collect();
	words.join(" ").chars().take(max).collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn cleans_links_and_length() {
		assert_eq!(clean("  hi\n https://x.y/z  www.foo.de  ok ", 100), "hi link link ok");
		assert_eq!(clean("abcdef", 3), "abc");
	}

	#[test]
	fn languages() {
		assert_eq!(language("de").as_deref(), Some("de"));
		assert_eq!(language("en+croak").as_deref(), Some("en"));
		assert_eq!(language("+whisper"), None);
		assert_eq!(language("../x"), None);
	}
}
