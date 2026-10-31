//! Shared data types: tracks, queue items and the state snapshot
//! that is broadcast to the web UI.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A playable item. `url` is the page URL (YouTube watch URL, SoundCloud
/// page, direct audio URL, ...). The actual stream URL is resolved lazily
/// right before playback because googlevideo URLs expire.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Track {
	pub url: String,
	pub title: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub artist: Option<String>,
	/// Duration in seconds, `None` for live streams / unknown.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub duration: Option<f64>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub thumbnail: Option<String>,
	/// yt-dlp extractor key (e.g. "youtube", "soundcloud", "generic").
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub source: Option<String>,
	/// Extractor-specific id (YouTube video id) — used for SponsorBlock.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub source_id: Option<String>,
	#[serde(default)]
	pub is_live: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueueItem {
	pub id: Uuid,
	pub track: Track,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub requested_by: Option<String>,
	/// Unix millis when the item was added.
	pub added_at: u64,
	/// Individually requested song: plays before playlist/mix tracks.
	#[serde(default)]
	pub request: bool,
}

impl QueueItem {
	pub fn new(track: Track, requested_by: Option<String>) -> Self {
		Self { id: Uuid::new_v4(), track, requested_by, added_at: now_ms(), request: false }
	}
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PlaybackStatus {
	Idle,
	Loading,
	Playing,
	Paused,
}

/// A SponsorBlock segment `[start, end)` in seconds with its category.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Segment {
	pub start: f64,
	pub end: f64,
	pub category: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct TsStatus {
	pub connected: bool,
	pub server: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub channel: Option<String>,
	pub listeners: u32,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Playback {
	pub status: PlaybackStatus,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub item: Option<QueueItem>,
	/// Position in seconds at `position_at`.
	pub position: f64,
	/// Unix millis at which `position` was sampled. Clients extrapolate.
	pub position_at: u64,
	pub segments: Vec<Segment>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub error: Option<String>,
}

/// Full state pushed to every web client on each change.
#[derive(Clone, Debug, Serialize)]
pub struct StateSnapshot {
	pub version: u64,
	pub ts: TsStatus,
	pub playback: Playback,
	pub queue: Vec<QueueItem>,
	pub history: Vec<QueueItem>,
}

pub fn now_ms() -> u64 {
	std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map(|d| d.as_millis() as u64)
		.unwrap_or(0)
}

pub fn fmt_duration(secs: f64) -> String {
	let s = secs.max(0.0).round() as u64;
	let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
	if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}
