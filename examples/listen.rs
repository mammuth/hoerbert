//! Test helper: joins the server as a second client and counts the audio
//! packets it receives from other clients. Used to verify end-to-end that
//! the bot's audio actually arrives in the channel.
//!
//!   cargo run --example listen -- 127.0.0.1 [channel-path] [seconds] [chat message…]
//!
//! If a chat message is given it is sent to the channel after connecting and
//! any replies are printed (for testing `!play …` style commands).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::Result;
use futures::StreamExt;
use tsclientlib::{Connection, StreamItem};
use tsproto_packets::packets::AudioData;

#[tokio::main]
async fn main() -> Result<()> {
	tracing_subscriber::fmt().with_env_filter("info,tsproto=warn").init();
	let mut args = std::env::args().skip(1);
	let addr = args.next().unwrap_or_else(|| "127.0.0.1".into());
	let channel = args.next();
	let secs: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(15);
	let chat: Vec<String> = args.collect();
	let chat = (!chat.is_empty()).then(|| chat.join(" "));

	let mut opts = Connection::build(addr).name("listener");
	if let Some(ch) = channel {
		opts = opts.channel(ch);
	}
	let mut con = opts.connect()?;
	// wait until connected
	loop {
		match con.events().next().await {
			Some(Ok(StreamItem::BookEvents(_))) => break,
			Some(Ok(_)) => {}
			Some(Err(e)) => return Err(e.into()),
			None => anyhow::bail!("closed"),
		}
	}
	println!("listener connected");
	if let Some(msg) = &chat {
		use tsclientlib::prelude::*;
		let cmd = con.get_state()?.send_message(tsclientlib::MessageTarget::Channel, msg);
		cmd.send(&mut con)?;
		println!("sent chat: {msg}");
	}
	let mut counts: HashMap<u16, (u64, u64, String)> = HashMap::new(); // from -> (packets, bytes, codec)
	let deadline = Instant::now() + Duration::from_secs(secs);
	while Instant::now() < deadline {
		let item = tokio::time::timeout_at(deadline.into(), con.events().next()).await;
		match item {
			Ok(Some(Ok(StreamItem::Audio(packet)))) => {
				if let AudioData::S2C { from, codec, data, .. } = packet.data().data() {
					let e = counts.entry(*from).or_insert((0, 0, format!("{codec:?}")));
					e.0 += 1;
					e.1 += data.len() as u64;
				}
			}
			Ok(Some(Ok(StreamItem::BookEvents(evs)))) => {
				for ev in evs {
					if let tsclientlib::events::Event::Message { invoker, message, .. } = ev {
						println!("CHAT <{}> {}", invoker.name, message.replace('\n', " | "));
					}
				}
			}
			Ok(Some(Ok(_))) => {}
			Ok(Some(Err(e))) => return Err(e.into()),
			Ok(None) => break,
			Err(_) => break,
		}
	}
	let names: HashMap<u16, String> = con
		.get_state()
		.map(|b| b.clients.iter().map(|(id, c)| (id.0, c.name.clone())).collect())
		.unwrap_or_default();
	let mut total = 0;
	for (from, (n, bytes, codec)) in &counts {
		println!(
			"from {} ({}): {} packets, {} bytes, codec {}, {:.1} pkt/s",
			from,
			names.get(from).cloned().unwrap_or_default(),
			n,
			bytes,
			codec,
			*n as f64 / secs as f64
		);
		total += n;
	}
	println!("RESULT total_audio_packets={total}");
	con.disconnect(tsclientlib::DisconnectOptions::new())?;
	con.events().for_each(|_| futures::future::ready(())).await;
	Ok(())
}
