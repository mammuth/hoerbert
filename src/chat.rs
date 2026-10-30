//! Chat commands typed in TeamSpeak (`!play …`, `!skip`, …).

use tokio::sync::mpsc;
use tracing::{info, warn};
use tsclientlib::{Invoker, MessageTarget};

use crate::config::Config;
use crate::model::{fmt_duration, LoopMode, PlaybackStatus};
use crate::player::{Cmd, PlayerHandle};
use crate::resolver::Resolver;
use crate::ts::{TsCmd, TsEvent};

pub struct ChatContext {
	pub cfg: Config,
	pub player: PlayerHandle,
	pub resolver: Resolver,
	pub ts_tx: mpsc::Sender<TsCmd>,
}

pub async fn run(ctx: ChatContext, mut events: mpsc::Receiver<TsEvent>) {
	while let Some(ev) = events.recv().await {
		match ev {
			TsEvent::Status(s) => ctx.player.send(Cmd::TsStatus(s)).await,
			TsEvent::Message { target, invoker, text } => {
				let reply_target = match target {
					MessageTarget::Channel => MessageTarget::Channel,
					MessageTarget::Server => MessageTarget::Server,
					MessageTarget::Client(_) | MessageTarget::Poke(_) => MessageTarget::Client(invoker.id),
				};
				if let Some(reply) = handle(&ctx, &invoker, &text).await {
					let _ = ctx.ts_tx.send(TsCmd::SendMessage { target: reply_target, text: reply }).await;
				}
			}
		}
	}
}

fn strip_bbcode(s: &str) -> String {
	// TS wraps pasted links: [URL]https://…[/URL]
	let mut out = String::with_capacity(s.len());
	let mut rest = s;
	while let Some(start) = rest.find('[') {
		out.push_str(&rest[..start]);
		match rest[start..].find(']') {
			Some(end) => rest = &rest[start + end + 1..],
			None => {
				out.push_str(&rest[start..]);
				rest = "";
			}
		}
	}
	out.push_str(rest);
	out
}

async fn handle(ctx: &ChatContext, invoker: &Invoker, raw: &str) -> Option<String> {
	let text = strip_bbcode(raw);
	let text = text.trim();
	let prefix = &ctx.cfg.command_prefix;
	let body = text.strip_prefix(prefix.as_str())?;
	let (cmd, arg) = match body.split_once(char::is_whitespace) {
		Some((c, a)) => (c.to_ascii_lowercase(), a.trim()),
		None => (body.to_ascii_lowercase(), ""),
	};
	let who = Some(invoker.name.clone());
	info!(user = %invoker.name, command = %cmd, arg = %arg, "chat command");
	let snap = ctx.player.snapshot();
	match cmd.as_str() {
		"help" | "h" | "?" => Some(help(prefix, ctx.cfg.public_url.as_deref())),
		"play" | "p" | "add" | "search" | "yt" | "next" | "playnow" => {
			if arg.is_empty() {
				if snap.playback.status == PlaybackStatus::Paused {
					ctx.player.send(Cmd::Resume).await;
					return Some("▶ resumed".into());
				}
				return Some(format!("usage: {prefix}play <url or search terms>"));
			}
			match ctx.resolver.resolve_input(arg).await {
				Ok((name, tracks)) if !tracks.is_empty() => {
					let n = tracks.len();
					let first = tracks[0].title.clone();
					let play_now = cmd == "playnow";
					let next = cmd == "next";
					ctx.player.send(Cmd::Add { tracks, requested_by: who, next, play_now }).await;
					Some(if n == 1 {
						format!("➕ queued: {first}")
					} else {
						format!("➕ queued {n} tracks{}", name.map(|n| format!(" from \"{n}\"")).unwrap_or_default())
					})
				}
				Ok(_) => Some("nothing found".into()),
				Err(e) => {
					warn!(error = %e, "chat add failed");
					Some(format!("✖ {e}"))
				}
			}
		}
		"skip" | "s" | "n" => {
			ctx.player.send(Cmd::Skip).await;
			Some("⏭ skipped".into())
		}
		"prev" | "back" => {
			ctx.player.send(Cmd::Prev).await;
			None
		}
		"pause" => {
			ctx.player.send(Cmd::Pause).await;
			Some("⏸ paused".into())
		}
		"resume" | "unpause" | "continue" => {
			ctx.player.send(Cmd::Resume).await;
			Some("▶ resumed".into())
		}
		"toggle" => {
			ctx.player.send(Cmd::Toggle).await;
			None
		}
		"stop" => {
			ctx.player.send(Cmd::Stop).await;
			Some("⏹ stopped".into())
		}
		"np" | "now" | "nowplaying" | "current" => Some(now_playing(&snap)),
		"queue" | "q" | "list" => {
			if snap.queue.is_empty() {
				return Some(format!("{}\nqueue is empty", now_playing(&snap)));
			}
			let mut s = now_playing(&snap);
			s.push_str("\nup next:");
			for (i, item) in snap.queue.iter().take(10).enumerate() {
				s.push_str(&format!(
					"\n{}. {} [{}]{}",
					i + 1,
					item.track.title,
					item.track.duration.map(fmt_duration).unwrap_or_else(|| "live".into()),
					item.requested_by.as_ref().map(|r| format!(" – {r}")).unwrap_or_default()
				));
			}
			if snap.queue.len() > 10 {
				s.push_str(&format!("\n… and {} more", snap.queue.len() - 10));
			}
			Some(s)
		}
		"vol" | "volume" | "v" => {
			if arg.is_empty() {
				return Some(format!("🔊 volume: {}%", (snap.playback.volume * 100.0).round()));
			}
			let v: f32 = arg.trim_end_matches('%').parse().ok()?;
			let v = (v / 100.0).clamp(0.0, 1.5);
			ctx.player.send(Cmd::Volume(v)).await;
			Some(format!("🔊 volume: {}%", (v * 100.0).round()))
		}
		"seek" => {
			let secs = parse_time(arg)?;
			ctx.player.send(Cmd::Seek(secs)).await;
			Some(format!("⏩ {}", fmt_duration(secs)))
		}
		"shuffle" => {
			ctx.player.send(Cmd::Shuffle).await;
			Some("🔀 queue shuffled".into())
		}
		"clear" => {
			ctx.player.send(Cmd::Clear).await;
			Some("🗑 queue cleared".into())
		}
		"loop" | "repeat" => {
			let mode = match arg {
				"one" | "track" | "1" => LoopMode::One,
				"all" | "queue" => LoopMode::All,
				"off" | "0" | "none" => LoopMode::Off,
				"" => match snap.playback.loop_mode {
					LoopMode::Off => LoopMode::All,
					LoopMode::All => LoopMode::One,
					LoopMode::One => LoopMode::Off,
				},
				_ => return Some(format!("usage: {prefix}loop off|one|all")),
			};
			ctx.player.send(Cmd::SetLoop(mode)).await;
			Some(format!("🔁 loop: {}", serde_json::to_value(mode).ok()?.as_str()?))
		}
		"remove" | "rm" | "del" => {
			let idx: usize = arg.parse().ok()?;
			let item = snap.queue.get(idx.checked_sub(1)?)?;
			ctx.player.send(Cmd::Remove(item.id)).await;
			Some(format!("🗑 removed: {}", item.track.title))
		}
		"playlists" | "pls" => {
			if snap.playlists.is_empty() {
				return Some("no playlists yet — create them in the web ui".into());
			}
			let mut s = String::from("playlists:");
			for p in &snap.playlists {
				s.push_str(&format!("\n• {} ({} tracks)", p.name, p.count));
			}
			s.push_str(&format!("\n{prefix}pl <name> to queue one"));
			Some(s)
		}
		"pl" | "playlist" => {
			if arg.is_empty() {
				return Some(format!("usage: {prefix}pl <saved playlist or search terms>"));
			}
			let needle = arg.to_lowercase();
			let saved = snap
				.playlists
				.iter()
				.find(|p| p.name.to_lowercase() == needle)
				.or_else(|| snap.playlists.iter().find(|p| p.name.to_lowercase().contains(&needle)));
			if let Some(pl) = saved {
				return match ctx.player.playlist_enqueue(pl.id, false, who).await {
					Ok(n) => Some(format!("➕ queued {n} tracks from \"{}\"", pl.name)),
					Err(e) => Some(format!("✖ {e}")),
				};
			}
			// No saved playlist: queue the top YouTube playlist for the query.
			let url = match ctx.resolver.search_playlist(arg).await {
				Ok(Some(u)) => u,
				Ok(None) => return Some("nothing found".into()),
				Err(e) => return Some(format!("✖ {e}")),
			};
			match ctx.resolver.resolve_input(&url).await {
				Ok((name, tracks)) if !tracks.is_empty() => {
					let n = tracks.len();
					ctx.player.send(Cmd::Add { tracks, requested_by: who, next: false, play_now: false }).await;
					Some(format!("➕ queued {n} tracks from \"{}\"", name.unwrap_or(url)))
				}
				Ok(_) => Some("nothing found".into()),
				Err(e) => Some(format!("✖ {e}")),
			}
		}
		"link" | "web" | "ui" => Some(match &ctx.cfg.public_url {
			Some(u) => format!("🎛 {u}"),
			None => "web ui url not configured (PUBLIC_URL)".into(),
		}),
		"sb" | "sponsorblock" => {
			let on = match arg {
				"on" | "1" | "true" => true,
				"off" | "0" | "false" => false,
				_ => !snap.sponsorblock,
			};
			ctx.player.send(Cmd::SetSponsorBlock(on)).await;
			Some(format!("sponsorblock: {}", if on { "on" } else { "off" }))
		}
		_ => None,
	}
}

fn now_playing(snap: &crate::model::StateSnapshot) -> String {
	match (&snap.playback.item, snap.playback.status) {
		(Some(item), status) => {
			let pos = snap.playback.position;
			let dur = item.track.duration.map(fmt_duration).unwrap_or_else(|| "live".into());
			let icon = match status {
				PlaybackStatus::Paused => "⏸",
				PlaybackStatus::Loading => "⏳",
				_ => "▶",
			};
			format!(
				"{icon} {} [{} / {}]{}",
				item.track.title,
				fmt_duration(pos),
				dur,
				item.requested_by.as_ref().map(|r| format!(" – requested by {r}")).unwrap_or_default()
			)
		}
		(None, _) => "nothing playing".into(),
	}
}

fn parse_time(s: &str) -> Option<f64> {
	let s = s.trim();
	if s.is_empty() {
		return None;
	}
	if let Ok(v) = s.parse::<f64>() {
		return Some(v);
	}
	let parts: Vec<&str> = s.split(':').collect();
	let mut total = 0.0;
	for p in parts {
		total = total * 60.0 + p.parse::<f64>().ok()?;
	}
	Some(total)
}

fn help(prefix: &str, url: Option<&str>) -> String {
	let mut s = format!(
		"Hörbert commands:\n\
		 {p}play <url|search> – queue a track/playlist (also {p}search, {p}next, {p}playnow)\n\
		 {p}skip · {p}prev · {p}pause · {p}resume · {p}stop\n\
		 {p}np · {p}queue · {p}remove <n> · {p}shuffle · {p}clear\n\
		 {p}vol <0-100> · {p}seek <mm:ss> · {p}loop off|one|all · {p}sb on|off\n\
		 {p}playlists · {p}pl <saved name|search>",
		p = prefix
	);
	if let Some(u) = url {
		s.push_str(&format!("\nweb ui: {u}"));
	}
	s
}
