//! Persistence: one JSON file for queue/history/settings, one for the
//! TeamSpeak identity. Small and personal — no database needed.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::model::{LoopMode, QueueItem};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Persisted {
	#[serde(default)]
	pub queue: Vec<QueueItem>,
	#[serde(default)]
	pub history: Vec<QueueItem>,
	#[serde(default)]
	pub volume: Option<f32>,
	#[serde(default)]
	pub loop_mode: LoopMode,
	#[serde(default)]
	pub sponsorblock: Option<bool>,
}

pub struct Store {
	path: PathBuf,
}

impl Store {
	pub fn new(data_dir: &Path) -> Result<Self> {
		std::fs::create_dir_all(data_dir)
			.with_context(|| format!("creating data dir {}", data_dir.display()))?;
		Ok(Self { path: data_dir.join("state.json") })
	}

	pub fn load(&self) -> Persisted {
		match std::fs::read(&self.path) {
			Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
				tracing::warn!(error = %e, "state.json is corrupt, starting fresh");
				Persisted::default()
			}),
			Err(_) => Persisted::default(),
		}
	}

	/// Atomic write (tmp + rename) so a crash never truncates the file.
	pub fn save(&self, data: &Persisted) -> Result<()> {
		let tmp = self.path.with_extension("json.tmp");
		let json = serde_json::to_vec_pretty(data)?;
		std::fs::write(&tmp, json)?;
		std::fs::rename(&tmp, &self.path)?;
		Ok(())
	}
}
