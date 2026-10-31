//! yt-dlp wrapper: turns user input (URL, playlist URL, search query) into
//! tracks and resolves a track to a playable stream URL right before playback.

use std::collections::HashMap;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use serde_json::Value;
use tokio::process::Command;
use tracing::{debug, warn};

use crate::config::Config;
use crate::model::Track;

const YTDLP_TIMEOUT: Duration = Duration::from_secs(90);
const MAX_PLAYLIST_ENTRIES: usize = 500;

/// Everything ffmpeg needs to play a track.
#[derive(Clone, Debug)]
pub struct StreamInfo {
	pub url: String,
	pub headers: HashMap<String, String>,
	pub is_live: bool,
	/// Duration as reported at resolve time (may be more exact than the flat one).
	pub duration: Option<f64>,
	/// Set when the stream is a direct URL that bypassed yt-dlp.
	pub direct: bool,
	/// Unix seconds before which YouTube answers the URL with 403.
	pub available_at: Option<f64>,
}

impl StreamInfo {
	/// YouTube URLs only become fetchable a few seconds after extraction
	/// (yt-dlp: "Sleeping … as required by the site").
	pub async fn wait_until_available(&self) {
		let Some(at) = self.available_at else { return };
		let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs_f64();
		if at > now {
			let wait = at - now + 0.3;
			debug!(wait, "waiting until stream url is available");
			tokio::time::sleep(Duration::from_secs_f64(wait)).await;
		}
	}
}

#[derive(Clone)]
pub struct Resolver {
	cfg: Config,
}

#[derive(Deserialize, Debug, Default)]
struct YtEntry {
	#[serde(default)]
	id: Option<String>,
	#[serde(default)]
	title: Option<String>,
	#[serde(default)]
	url: Option<String>,
	#[serde(default)]
	webpage_url: Option<String>,
	#[serde(default)]
	original_url: Option<String>,
	#[serde(default)]
	duration: Option<f64>,
	#[serde(default)]
	uploader: Option<String>,
	#[serde(default)]
	channel: Option<String>,
	#[serde(default)]
	artist: Option<String>,
	#[serde(default)]
	thumbnail: Option<String>,
	#[serde(default)]
	thumbnails: Option<Vec<YtThumb>>,
	#[serde(default)]
	extractor_key: Option<String>,
	#[serde(default)]
	ie_key: Option<String>,
	#[serde(default)]
	extractor: Option<String>,
	#[serde(default)]
	is_live: Option<bool>,
	#[serde(default)]
	live_status: Option<String>,
	#[serde(default)]
	availability: Option<String>,
}

#[derive(Deserialize, Debug)]
struct YtThumb {
	url: String,
}

pub fn is_url(input: &str) -> bool {
	let s = input.trim();
	(s.starts_with("http://") || s.starts_with("https://")) && !s.contains(char::is_whitespace)
}

/// Search hits considered when picking a single track for free-text input.
const SEARCH_CANDIDATES: usize = 8;

/// Keep YouTube's relevance order but skip livestreams, Shorts and
/// hour-long loops/compilations when a normal-length hit exists.
fn pick_best(tracks: Vec<Track>) -> Option<Track> {
	let song_like = |t: &&Track| !t.is_live && t.duration.is_some_and(|d| (60.0..=1200.0).contains(&d));
	tracks
		.iter()
		.find(song_like)
		.or_else(|| tracks.iter().find(|t| !t.is_live))
		.or_else(|| tracks.first())
		.cloned()
}

fn looks_like_direct_media(url: &str) -> bool {
	let path = url.split(['?', '#']).next().unwrap_or("").to_ascii_lowercase();
	[".mp3", ".ogg", ".oga", ".opus", ".flac", ".m4a", ".aac", ".wav", ".webm", ".mp4", ".m3u8", ".pls"]
		.iter()
		.any(|ext| path.ends_with(ext))
}

impl YtEntry {
	fn source(&self) -> Option<String> {
		self.extractor_key
			.clone()
			.or_else(|| self.ie_key.clone())
			.or_else(|| self.extractor.clone())
			.map(|s| s.to_ascii_lowercase())
	}

	fn into_track(self) -> Option<Track> {
		let source = self.source();
		let is_yt = source.as_deref().map(|s| s.starts_with("youtube")).unwrap_or(false);
		// Private/deleted entries in playlists have no duration and a placeholder title.
		if matches!(self.availability.as_deref(), Some("private" | "needs_auth"))
			|| matches!(self.title.as_deref(), Some("[Private video]" | "[Deleted video]"))
		{
			return None;
		}
		let url = self
			.webpage_url
			.clone()
			.or_else(|| self.url.clone())
			.or_else(|| {
				if is_yt { self.id.as_ref().map(|id| format!("https://www.youtube.com/watch?v={id}")) } else { None }
			})
			.or_else(|| self.original_url.clone())?;
		let thumbnail = self
			.thumbnail
			.clone()
			.or_else(|| if is_yt { self.id.as_ref().map(|id| format!("https://i.ytimg.com/vi/{id}/mqdefault.jpg")) } else { None })
			.or_else(|| self.thumbnails.as_ref().and_then(|t| t.last()).map(|t| t.url.clone()));
		let is_live = self.is_live.unwrap_or(false) || matches!(self.live_status.as_deref(), Some("is_live"));
		Some(Track {
			title: self.title.clone().unwrap_or_else(|| url.clone()),
			artist: self.artist.clone().or_else(|| self.channel.clone()).or_else(|| self.uploader.clone()),
			duration: if is_live { None } else { self.duration.filter(|d| *d > 0.0) },
			thumbnail,
			source,
			source_id: self.id.clone(),
			is_live,
			url,
		})
	}
}

impl Resolver {
	pub fn new(cfg: Config) -> Self { Self { cfg } }

	fn base_cmd(&self) -> Command {
		let mut cmd = Command::new(&self.cfg.ytdlp_bin);
		cmd.arg("--no-warnings").arg("--no-progress").arg("--ignore-config");
		cmd.args(&self.cfg.ytdlp_args);
		cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
		cmd.kill_on_drop(true);
		cmd
	}

	async fn run_json(&self, mut cmd: Command) -> Result<Value> {
		debug!(?cmd, "running yt-dlp");
		let out = tokio::time::timeout(YTDLP_TIMEOUT, cmd.output())
			.await
			.map_err(|_| anyhow!("yt-dlp timed out"))?
			.context("failed to start yt-dlp")?;
		if !out.status.success() {
			let err = String::from_utf8_lossy(&out.stderr);
			let line = err
				.lines()
				.rev()
				.find(|l| l.contains("ERROR"))
				.or_else(|| err.lines().last())
				.unwrap_or("unknown error")
				.trim_start_matches("ERROR: ")
				.to_string();
			bail!("yt-dlp: {line}");
		}
		serde_json::from_slice(&out.stdout).context("yt-dlp returned invalid JSON")
	}

	fn tracks_from_json(v: Value) -> Result<(Option<String>, Vec<Track>)> {
		if v.get("_type").and_then(Value::as_str) == Some("playlist") {
			let name = v.get("title").and_then(Value::as_str).map(str::to_string);
			let entries = v.get("entries").and_then(Value::as_array).cloned().unwrap_or_default();
			let tracks = entries
				.into_iter()
				.take(MAX_PLAYLIST_ENTRIES)
				.filter(|e| !e.is_null())
				.filter_map(|e| serde_json::from_value::<YtEntry>(e).ok())
				.filter_map(YtEntry::into_track)
				.collect();
			Ok((name, tracks))
		} else {
			let entry: YtEntry = serde_json::from_value(v)?;
			Ok((None, entry.into_track().into_iter().collect()))
		}
	}

	/// Resolve free-form input: URL (single or playlist) or a search query
	/// (first YouTube hit). Returns an optional playlist name plus tracks.
	pub async fn resolve_input(&self, input: &str) -> Result<(Option<String>, Vec<Track>)> {
		let input = input.trim();
		if input.is_empty() {
			bail!("empty input");
		}
		if is_url(input) {
			if looks_like_direct_media(input) {
				// Don't bother yt-dlp with plain files / radio streams.
				return Ok((None, vec![direct_track(input)]));
			}
			let mut cmd = self.base_cmd();
			cmd.arg("-J").arg("--flat-playlist").arg(input);
			match self.run_json(cmd).await {
				Ok(v) => Self::tracks_from_json(v),
				// Only unknown sites may be raw media; real extractor errors
				// (bot check, private, geo-block) must reach the user.
				Err(e) if e.to_string().contains("Unsupported URL") => {
					warn!(error = %e, url = input, "yt-dlp failed, falling back to direct URL");
					Ok((None, vec![direct_track(input)]))
				}
				Err(e) => Err(e),
			}
		} else {
			let tracks = self.search(input, SEARCH_CANDIDATES).await?;
			match pick_best(tracks) {
				Some(t) => Ok((None, vec![t])),
				None => bail!("no results for \"{input}\""),
			}
		}
	}

	pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<Track>> {
		let mut cmd = self.base_cmd();
		cmd.arg("-J").arg("--flat-playlist").arg(format!("ytsearch{}:{}", limit.clamp(1, 25), query.trim()));
		let v = self.run_json(cmd).await?;
		Ok(Self::tracks_from_json(v)?.1)
	}

	/// URL of the top YouTube playlist for a search query.
	pub async fn search_playlist(&self, query: &str) -> Result<Option<String>> {
		let mut url = url::Url::parse_with_params("https://www.youtube.com/results", &[("search_query", query.trim())])?.to_string();
		url.push_str("&sp=EgIQAw%253D%253D"); // YouTube's "type: playlist" filter
		let mut cmd = self.base_cmd();
		cmd.arg("-J").arg("--flat-playlist").arg("--playlist-items").arg("1").arg(url);
		let v = self.run_json(cmd).await?;
		Ok(v.get("entries")
			.and_then(Value::as_array)
			.and_then(|e| e.first())
			.and_then(|e| e.get("url"))
			.and_then(Value::as_str)
			.map(str::to_string))
	}

	/// Resolve the actual audio stream for a track.
	pub async fn stream_info(&self, track: &Track) -> Result<StreamInfo> {
		if track.source.as_deref() == Some("direct") || looks_like_direct_media(&track.url) {
			let duration = match track.duration {
				Some(d) => Some(d),
				None => self.probe_duration(&track.url).await,
			};
			return Ok(StreamInfo {
				url: track.url.clone(),
				headers: HashMap::new(),
				is_live: track.is_live || duration.is_none(),
				duration,
				direct: true,
				available_at: None,
			});
		}
		let mut cmd = self.base_cmd();
		cmd.arg("-J")
			.arg("--no-playlist")
			.arg("-f")
			.arg("bestaudio[acodec=opus]/bestaudio/best")
			.arg(&track.url);
		let v = self.run_json(cmd).await?;
		// With `-f bestaudio` there is a single selected format at top level.
		// If yt-dlp picked a merge (video+audio) prefer the audio part.
		let fmt = v
			.get("requested_formats")
			.and_then(Value::as_array)
			.and_then(|fs| fs.iter().find(|f| f.get("vcodec").and_then(Value::as_str).map(|c| c == "none").unwrap_or(false)))
			.cloned()
			.unwrap_or_else(|| v.clone());
		let url = fmt
			.get("url")
			.and_then(Value::as_str)
			.or_else(|| v.get("url").and_then(Value::as_str))
			.ok_or_else(|| anyhow!("yt-dlp returned no stream url"))?
			.to_string();
		let headers = fmt
			.get("http_headers")
			.or_else(|| v.get("http_headers"))
			.and_then(Value::as_object)
			.map(|m| {
				m.iter()
					.filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
					.collect()
			})
			.unwrap_or_default();
		let is_live = v.get("is_live").and_then(Value::as_bool).unwrap_or(false)
			|| v.get("live_status").and_then(Value::as_str) == Some("is_live");
		let duration = v.get("duration").and_then(Value::as_f64).filter(|d| *d > 0.0);
		debug!(format = ?fmt.get("format_id"), acodec = ?fmt.get("acodec"), abr = ?fmt.get("abr"), "selected stream");
		let available_at = fmt.get("available_at").or_else(|| v.get("available_at")).and_then(Value::as_f64);
		Ok(StreamInfo { url, headers, is_live, duration, direct: false, available_at })
	}
}

impl Resolver {
	/// Ask ffprobe for the duration of a direct URL (None for live streams).
	async fn probe_duration(&self, url: &str) -> Option<f64> {
		let ffprobe = self.cfg.ffmpeg_bin.replace("ffmpeg", "ffprobe");
		let mut cmd = Command::new(ffprobe);
		cmd.args(["-v", "error", "-show_entries", "format=duration", "-of", "default=noprint_wrappers=1:nokey=1"])
			.arg(url)
			.stdin(Stdio::null())
			.stdout(Stdio::piped())
			.stderr(Stdio::null())
			.kill_on_drop(true);
		let out = tokio::time::timeout(Duration::from_secs(20), cmd.output()).await.ok()?.ok()?;
		let text = String::from_utf8_lossy(&out.stdout);
		text.trim().parse::<f64>().ok().filter(|d| d.is_finite() && *d > 0.0)
	}
}

fn direct_track(url: &str) -> Track {
	let name = url
		.split(['?', '#'])
		.next()
		.unwrap_or(url)
		.rsplit('/')
		.next()
		.filter(|s| !s.is_empty())
		.unwrap_or(url)
		.to_string();
	Track {
		url: url.to_string(),
		title: name,
		artist: url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)),
		duration: None,
		thumbnail: None,
		source: Some("direct".into()),
		source_id: None,
		is_live: false,
	}
}
