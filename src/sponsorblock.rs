//! SponsorBlock client: fetch skippable segments for YouTube videos so the
//! bot can jump over sponsor reads, self-promo and non-music parts.

use std::time::Duration;

use anyhow::Result;
use serde::Deserialize;

use crate::model::Segment;

#[derive(Deserialize)]
struct SbSegment {
	category: String,
	#[serde(rename = "actionType", default)]
	action_type: Option<String>,
	segment: [f64; 2],
}

pub async fn fetch_segments(client: &reqwest::Client, video_id: &str, categories: &[String]) -> Result<Vec<Segment>> {
	if categories.is_empty() {
		return Ok(vec![]);
	}
	let cats = serde_json::to_string(categories)?;
	let url = format!(
		"https://sponsor.ajay.app/api/skipSegments?videoID={}&categories={}&actionTypes=%5B%22skip%22%5D",
		urlencoding(video_id),
		urlencoding(&cats)
	);
	let resp = client.get(&url).timeout(Duration::from_secs(8)).send().await?;
	if resp.status() == reqwest::StatusCode::NOT_FOUND {
		return Ok(vec![]); // no segments submitted — the normal case
	}
	let segs: Vec<SbSegment> = resp.error_for_status()?.json().await?;
	let mut out: Vec<Segment> = segs
		.into_iter()
		.filter(|s| s.action_type.as_deref().map(|a| a == "skip").unwrap_or(true))
		.filter(|s| s.segment[1] > s.segment[0] + 0.5)
		.map(|s| Segment { start: s.segment[0], end: s.segment[1], category: s.category })
		.collect();
	out.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
	// Merge overlaps so one seek skips a whole cluster.
	let mut merged: Vec<Segment> = Vec::with_capacity(out.len());
	for s in out {
		if let Some(last) = merged.last_mut() {
			if s.start <= last.end + 0.25 {
				if s.end > last.end {
					last.end = s.end;
				}
				continue;
			}
		}
		merged.push(s);
	}
	Ok(merged)
}

fn urlencoding(s: &str) -> String {
	let mut out = String::with_capacity(s.len() * 3);
	for b in s.bytes() {
		match b {
			b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
			_ => out.push_str(&format!("%{b:02X}")),
		}
	}
	out
}
