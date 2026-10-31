//! Chat commands typed in TeamSpeak (`!play …`, `!skip`, …).

use tokio::sync::mpsc;
use tracing::{info, warn};
use tsclientlib::{ClientId, Invoker, MessageTarget};

use crate::config::Config;
use crate::model::{fmt_duration, PlaybackStatus};
use crate::player::{Cmd, PlayerHandle};
use crate::resolver::Resolver;
use crate::soundboard::Soundboard;
use crate::ts::{TsCmd, TsEvent};

pub struct ChatContext {
	pub cfg: Config,
	pub player: PlayerHandle,
	pub resolver: Resolver,
	pub soundboard: Soundboard,
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
			TsEvent::VoiceListening { name } => {
				info!(user = %name, "listening for a command");
				let url = crate::voice::beep_path(&ctx.cfg).to_string_lossy().into_owned();
				ctx.player.send(Cmd::PlaySound { url, volume: 1.0 }).await;
			}
			TsEvent::Voice { from, name, text } => {
				// `pause`/`resume` toggle in chat; spoken ones should only do what they say.
				let paused = ctx.player.snapshot().playback.status == PlaybackStatus::Paused;
				if (text == "pause" && paused) || (text == "resume" && !paused) {
					continue;
				}
				let invoker = Invoker { name: name.clone(), id: ClientId(from), uid: None };
				let reply = handle(&ctx, &invoker, &format!("{}{text}", ctx.cfg.command_prefix)).await;
				let msg = match reply {
					Some(r) => format!("🎙 {name}: {text}\n{r}"),
					None => format!("🎙 {name}: {text}"),
				};
				let _ = ctx.ts_tx.send(TsCmd::SendMessage { target: MessageTarget::Channel, text: msg }).await;
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
		"help" | "h" | "?" => {
			let has_sounds = !ctx.soundboard.list().is_empty();
			Some(help(prefix, ctx.cfg.public_url.as_deref(), ctx.cfg.help_suffix.as_deref(), has_sounds))
		}
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
		"pause" | "resume" => {
			if snap.playback.item.is_none() {
				return Some("nothing playing".into());
			}
			if snap.playback.status == PlaybackStatus::Paused {
				ctx.player.send(Cmd::Resume).await;
				return Some("▶ resumed".into());
			}
			ctx.player.send(Cmd::Pause).await;
			Some("⏸ paused".into())
		}
		"now" | "status" => Some(now_playing(&snap)),
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
		"clear" => {
			ctx.player.send(Cmd::Clear).await;
			Some("🗑 queue cleared".into())
		}
		"link" | "web" | "ui" => Some(match &ctx.cfg.public_url {
			Some(u) => format!("🎛 {u}"),
			None => "web ui url not configured (PUBLIC_URL)".into(),
		}),
		"soundboard" | "sounds" | "sb" => {
			let sounds = ctx.soundboard.list();
			if sounds.is_empty() {
				return Some("no sounds configured".into());
			}
			let mut s = "🔊 Soundboard".to_string();
			for sound in sounds {
				s.push_str(&format!("\n{} {prefix}{} – {}", sound.emoji.as_deref().unwrap_or("🔈"), sound.slug, sound.name));
			}
			Some(s)
		}
		// Built-in commands above win over sounds with the same slug.
		slug => {
			let sound = ctx.soundboard.get(slug)?;
			ctx.player.send(Cmd::PlaySound { url: sound.url, volume: sound.volume }).await;
			None
		}
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

fn help(prefix: &str, url: Option<&str>, suffix: Option<&str>, has_sounds: bool) -> String {
	let mut s = format!(
		"🎵 Hörbert\n\
		 {p}play <link or search> – add to queue\n\
		 {p}next <link or search> – play right after the current track\n\
		 {p}skip · {p}prev · {p}pause\n\
		 {p}now · {p}queue · {p}clear",
		p = prefix
	);
	if has_sounds {
		s.push_str(&format!("\n{prefix}soundboard – list sounds"));
	}
	if let Some(u) = url {
		s.push_str(&format!("\n🎛 {u}"));
	}
	if let Some(x) = suffix {
		s.push('\n');
		s.push_str(x);
	}
	s
}
