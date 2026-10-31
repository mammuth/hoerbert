mod audio;
mod chat;
mod config;
mod model;
mod player;
mod resolver;
mod sponsorblock;
mod store;
mod ts;
mod web;

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
	tracing_subscriber::fmt()
		.with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,hoerbert=debug,tsproto=warn,tsclientlib=info")))
		.init();

	let cfg = config::Config::from_env()?;
	info!(address = %cfg.ts_address, web = %cfg.web_bind, data = %cfg.data_dir.display(), "starting Hörbert");

	let store = store::Store::new(&cfg.data_dir)?;
	let resolver = resolver::Resolver::new(cfg.clone());

	// Opus frames: player → TeamSpeak thread. Small buffer: we pace at 20 ms.
	let (audio_tx, audio_rx) = mpsc::channel(32);
	let (ts_cmd_tx, ts_cmd_rx) = mpsc::channel(32);
	let (ts_event_tx, ts_event_rx) = mpsc::channel(64);

	let player = player::spawn(cfg.clone(), store, resolver.clone(), audio_tx, ts_cmd_tx.clone())?;

	let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
	let (ts_done_tx, mut ts_done_rx) = mpsc::channel(1);
	ts::spawn(cfg.clone(), ts::TsChannels {
		audio_rx,
		cmd_rx: ts_cmd_rx,
		event_tx: ts_event_tx,
		shutdown: shutdown_rx,
		done_tx: ts_done_tx,
	});

	tokio::spawn(chat::run(
		chat::ChatContext { cfg: cfg.clone(), player: player.clone(), resolver: resolver.clone(), ts_tx: ts_cmd_tx },
		ts_event_rx,
	));

	let app = web::router(web::AppState { player, resolver });
	let listener = tokio::net::TcpListener::bind(&cfg.web_bind).await.with_context(|| format!("binding {}", cfg.web_bind))?;
	info!("web ui listening on http://{}", cfg.web_bind);
	axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;
	// Leave TeamSpeak politely so the server frees our client slot right away.
	let _ = shutdown_tx.send(true);
	let _ = tokio::time::timeout(std::time::Duration::from_secs(5), ts_done_rx.recv()).await;
	Ok(())
}

async fn shutdown_signal() {
	let ctrl_c = tokio::signal::ctrl_c();
	#[cfg(unix)]
	{
		let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("sigterm handler");
		tokio::select! {
			_ = ctrl_c => {}
			_ = term.recv() => {}
		}
	}
	#[cfg(not(unix))]
	{
		let _ = ctrl_c.await;
	}
	info!("shutting down");
}
