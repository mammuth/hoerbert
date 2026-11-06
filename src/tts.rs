//! Text to speech with espeak-ng: `!tts <text>` or the web UI, spoken over
//! the (ducked) music. Robotic on purpose.
//!
//! Voices are espeak-ng languages with an optional variant: `en`, `de+klatt`,
//! or just `+whisper` for a variant of the default language.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tracing::{info, warn};

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
	pub voice: String,
	/// Shown in the history; greetings are not.
	pub logged: bool,
}

#[derive(Clone)]
pub struct Tts {
	cfg: Config,
	/// espeak-ng languages and variants by lowercase name; `None`: TTS is off.
	voices: Option<Arc<(Voices, Voices)>>,
	/// Last message per user, for the cooldown.
	last: Arc<Mutex<HashMap<String, Instant>>>,
}

impl Tts {
	/// Asks espeak-ng for its voices once; without espeak-ng, TTS stays off.
	pub fn new(cfg: Config) -> Self {
		let voices = if cfg.tts {
			match (list_voices(&cfg.espeak_bin, "--voices"), list_voices(&cfg.espeak_bin, "--voices=variant")) {
				(Some(langs), Some(variants)) => {
					info!(languages = langs.len(), variants = variants.len(), "tts enabled");
					Some(Arc::new((langs, variants)))
				}
				_ => {
					warn!(bin = %cfg.espeak_bin, "espeak-ng not found, tts disabled");
					None
				}
			}
		} else {
			None
		};
		let tts = Self { cfg, voices, last: Default::default() };
		if tts.enabled() {
			if let Err(e) = tts.voice(None) {
				warn!(error = %e, "TTS_VOICE is not an espeak-ng voice");
			}
		}
		tts
	}

	pub fn enabled(&self) -> bool { self.voices.is_some() }

	pub fn max_chars(&self) -> usize { self.cfg.tts_max_chars }

	/// Cleans the text, checks the voice and enforces the per-user cooldown.
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

	/// Cleans the text and checks the voice, no cooldown.
	pub fn speech(&self, text: &str, voice: Option<&str>) -> Result<Speech, TtsError> {
		let text = clean(text, self.cfg.tts_max_chars);
		if text.is_empty() {
			return Err(TtsError::Invalid("nothing to say".into()));
		}
		Ok(Speech { voice: self.voice(voice)?, text, logged: true })
	}

	/// `None`: the default voice. `+variant` keeps the default language.
	fn voice(&self, voice: Option<&str>) -> Result<String, TtsError> {
		let Some((langs, variants)) = self.voices.as_deref() else {
			return Err(TtsError::Invalid("tts is off".into()));
		};
		let v = voice.map(str::trim).filter(|v| !v.is_empty()).unwrap_or(&self.cfg.tts_voice).to_lowercase();
		let (lang, variant) = match v.split_once('+') {
			Some(("", var)) => (self.cfg.tts_voice.split('+').next().unwrap_or("en").to_lowercase(), Some(var)),
			Some((l, var)) => (l.to_string(), Some(var)),
			None => (v.clone(), None),
		};
		if !langs.contains_key(&lang) {
			return Err(TtsError::Invalid(format!("unknown language {lang}")));
		}
		Ok(match variant {
			// Variant names are case-sensitive (`Demonic`); a wrong case silently falls back to the plain voice.
			Some(var) => match variants.get(var) {
				Some(name) => format!("{lang}+{name}"),
				None => return Err(TtsError::Invalid(format!("unknown voice +{var}"))),
			},
			None => lang,
		})
	}
}

/// Lowercase name → name as espeak-ng spells it.
type Voices = HashMap<String, String>;

/// Language codes (`de`, `en-us`) or variant names (`whisper`, `Demonic`)
/// from `espeak-ng --voices[=variant]`.
fn list_voices(bin: &str, arg: &str) -> Option<Voices> {
	let out = std::process::Command::new(bin).arg(arg).output().ok().filter(|o| o.status.success())?;
	let names = String::from_utf8_lossy(&out.stdout)
		.lines()
		.skip(1)
		.flat_map(|l| {
			let cols: Vec<&str> = l.split_whitespace().collect();
			// Pty Language Age/Gender VoiceName File (alias prio)…; variants live in `!v/<name>`.
			// Aliases matter: plain `en` only shows up as `(en 2)` next to `en-gb`.
			let names: Vec<&str> = match cols.get(4).and_then(|f| f.strip_prefix("!v/")) {
				Some(variant) => vec![variant],
				None => cols.get(1).into_iter().copied().chain(cols.iter().skip(5).filter_map(|c| c.strip_prefix('('))).collect(),
			};
			names.into_iter().map(|n| (n.to_lowercase(), n.to_string())).collect::<Vec<_>>()
		})
		.collect();
	Some(names)
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
	fn keeps_variant_case() {
		let Some(variants) = list_voices("espeak-ng", "--voices=variant") else { return }; // no espeak-ng
		assert_eq!(variants.get("demonic").map(String::as_str), Some("Demonic"));
		assert_eq!(variants.get("croak").map(String::as_str), Some("croak"));
	}
}
