//! Audio decoding: ffmpeg → 48 kHz stereo s16le PCM in 20 ms frames.
//!
//! A reader task pulls PCM out of ffmpeg into a bounded channel (a few
//! seconds of buffer), the player consumes exactly one frame per 20 ms tick.

use std::process::Stdio;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use tokio::io::{AsyncReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tracing::debug;

use crate::config::Config;
use crate::resolver::StreamInfo;

pub const CHANNELS: usize = 2;
/// 20 ms at 48 kHz.
pub const FRAME_SAMPLES: usize = 960;
pub const FRAME_LEN: usize = FRAME_SAMPLES * CHANNELS;
const FRAME_BYTES: usize = FRAME_LEN * 2;
/// ~8 seconds of audio buffered ahead of playback.
const BUFFER_FRAMES: usize = 400;

pub type Frame = Vec<i16>;

pub enum Pull {
	Frame(Frame),
	/// Buffer is empty but ffmpeg is still running (network stall).
	Starved,
	/// ffmpeg finished and the buffer is drained.
	Ended,
}

pub struct Decoder {
	children: Vec<Child>,
	rx: mpsc::Receiver<Frame>,
	stderr: Arc<Mutex<String>>,
	pub produced_frames: u64,
}

impl Decoder {
	/// ffmpeg reads the (already resolved) stream URL directly.
	pub fn spawn_url(cfg: &Config, stream: &StreamInfo, start_secs: f64) -> Result<Decoder> {
		let mut cmd = Command::new(&cfg.ffmpeg_bin);
		cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
		if stream.url.starts_with("http") {
			cmd.args([
				"-reconnect", "1",
				"-reconnect_streamed", "1",
				"-reconnect_on_network_error", "1",
				"-reconnect_delay_max", "5",
				"-rw_timeout", "15000000",
			]);
			if !stream.headers.is_empty() {
				let mut h = String::new();
				for (k, v) in &stream.headers {
					h.push_str(k);
					h.push_str(": ");
					h.push_str(v);
					h.push_str("\r\n");
				}
				cmd.arg("-headers").arg(h);
			}
		}
		if start_secs > 0.0 && !stream.is_live {
			cmd.arg("-ss").arg(format!("{start_secs:.3}"));
		}
		cmd.arg("-i").arg(&stream.url);
		Self::output_args(&mut cmd);
		cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
		debug!(?cmd, "spawning ffmpeg");
		let child = cmd.spawn().context("failed to start ffmpeg")?;
		Self::wrap(vec![child])
	}

	/// Fallback: let yt-dlp download and pipe into ffmpeg. Slower to start,
	/// not seekable, but yt-dlp handles every quirk of the CDN.
	pub fn spawn_pipe(cfg: &Config, page_url: &str, start_secs: f64) -> Result<Decoder> {
		let mut yt = Command::new(&cfg.ytdlp_bin);
		yt.args(["--no-warnings", "--no-progress", "--ignore-config", "--no-playlist", "-q"]);
		yt.args(&cfg.ytdlp_args);
		yt.args(["-f", "bestaudio[acodec=opus]/bestaudio/best", "-o", "-", page_url]);
		yt.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
		debug!(?yt, "spawning yt-dlp pipe");
		let mut yt_child = yt.spawn().context("failed to start yt-dlp")?;
		let yt_stdout = yt_child.stdout.take().ok_or_else(|| anyhow!("yt-dlp stdout missing"))?;
		let yt_stdout: Stdio = yt_stdout.try_into().map_err(|_| anyhow!("failed to convert yt-dlp stdout"))?;

		let mut ff = Command::new(&cfg.ffmpeg_bin);
		ff.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
		if start_secs > 0.0 {
			ff.arg("-ss").arg(format!("{start_secs:.3}"));
		}
		ff.args(["-i", "pipe:0"]);
		Self::output_args(&mut ff);
		ff.stdin(yt_stdout).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
		let ff_child = ff.spawn().context("failed to start ffmpeg")?;
		Self::wrap(vec![yt_child, ff_child])
	}

	fn output_args(cmd: &mut Command) {
		cmd.args(["-vn", "-sn", "-dn", "-ac", "2", "-ar", "48000", "-f", "s16le", "pipe:1"]);
	}

	fn wrap(mut children: Vec<Child>) -> Result<Decoder> {
		let stderr_buf = Arc::new(Mutex::new(String::new()));
		for child in children.iter_mut() {
			if let Some(err) = child.stderr.take() {
				let buf = stderr_buf.clone();
				tokio::spawn(async move {
					let mut reader = BufReader::new(err);
					let mut s = String::new();
					let _ = reader.read_to_string(&mut s).await;
					if !s.trim().is_empty() {
						let mut b = buf.lock().unwrap();
						b.push_str(s.trim());
						b.push('\n');
						// keep the tail only
						if b.len() > 2000 {
							let cut = b.len() - 2000;
							b.drain(..cut);
						}
					}
				});
			}
		}
		let last = children.last_mut().ok_or_else(|| anyhow!("no child"))?;
		let stdout = last.stdout.take().ok_or_else(|| anyhow!("ffmpeg stdout missing"))?;
		let (tx, rx) = mpsc::channel::<Frame>(BUFFER_FRAMES);
		tokio::spawn(async move {
			let mut reader = BufReader::with_capacity(FRAME_BYTES * 8, stdout);
			let mut buf = vec![0u8; FRAME_BYTES];
			loop {
				match reader.read_exact(&mut buf).await {
					Ok(_) => {
						let frame: Frame = buf.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
						if tx.send(frame).await.is_err() {
							break;
						}
					}
					Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
					Err(_) => break,
				}
			}
			// sender dropped here → receiver sees Disconnected once drained
		});
		Ok(Decoder { children, rx, stderr: stderr_buf, produced_frames: 0 })
	}

	pub fn pull(&mut self) -> Pull {
		match self.rx.try_recv() {
			Ok(f) => {
				self.produced_frames += 1;
				Pull::Frame(f)
			}
			Err(mpsc::error::TryRecvError::Empty) => Pull::Starved,
			Err(mpsc::error::TryRecvError::Disconnected) => Pull::Ended,
		}
	}

	pub fn buffered(&self) -> usize { self.rx.len() }

	pub fn is_finished(&self) -> bool { self.rx.is_closed() && self.rx.is_empty() }

	pub fn error_tail(&self) -> String { self.stderr.lock().unwrap().trim().to_string() }

	pub async fn kill(mut self) {
		for c in self.children.iter_mut() {
			let _ = c.start_kill();
		}
		for c in self.children.iter_mut() {
			let _ = c.wait().await;
		}
	}
}
