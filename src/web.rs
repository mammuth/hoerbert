//! HTTP API + WebSocket state feed + embedded SPA.

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rust_embed::Embed;
use serde::Deserialize;
use serde_json::json;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::model::{LoopMode, Track};
use crate::player::{Cmd, PlayerHandle};
use crate::resolver::Resolver;

#[derive(Embed)]
#[folder = "web/dist/"]
struct Assets;

#[derive(Clone)]
pub struct AppState {
	pub player: PlayerHandle,
	pub resolver: Resolver,
}

type ApiResult = Result<Response, ApiError>;

pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
	fn into_response(self) -> Response {
		(self.0, Json(json!({ "error": self.1 }))).into_response()
	}
}

impl From<anyhow::Error> for ApiError {
	fn from(e: anyhow::Error) -> Self { ApiError(StatusCode::BAD_GATEWAY, format!("{e:#}")) }
}

fn bad(msg: impl Into<String>) -> ApiError { ApiError(StatusCode::BAD_REQUEST, msg.into()) }
fn not_found() -> ApiError { ApiError(StatusCode::NOT_FOUND, "not found".into()) }
fn ok() -> ApiResult { Ok(Json(json!({ "ok": true })).into_response()) }

pub fn router(state: AppState) -> Router {
	let api = Router::new()
		.route("/state", get(get_state))
		.route("/ws", get(ws))
		.route("/search", get(search))
		.route("/add", post(add))
		.route("/player/{action}", post(player_action))
		.route("/player/seek", post(seek))
		.route("/player/volume", post(volume))
		.route("/player/loop", post(set_loop))
		.route("/player/sponsorblock", post(set_sponsorblock))
		.route("/queue/clear", post(queue_clear))
		.route("/queue/shuffle", post(queue_shuffle))
		.route("/queue/{id}/remove", post(queue_remove))
		.route("/queue/{id}/move", post(queue_move))
		.route("/queue/{id}/play", post(queue_play))
		.route("/playlists", get(playlists_list).post(playlists_create))
		.route("/playlists/from-queue", post(playlists_from_queue))
		.route("/playlists/{id}", get(playlist_get).delete(playlist_delete).patch(playlist_rename))
		.route("/playlists/{id}/tracks", post(playlist_add_tracks))
		.route("/playlists/{id}/tracks/{index}", axum::routing::delete(playlist_remove_track))
		.route("/playlists/{id}/enqueue", post(playlist_enqueue))
		.route("/playlists/{id}/import", post(playlist_import));
	Router::new()
		.nest("/api", api)
		.fallback(static_handler)
		.layer(tower_http::trace::TraceLayer::new_for_http())
		.with_state(state)
}

// ------------------------------------------------------------------ static

async fn static_handler(uri: Uri) -> Response {
	let path = uri.path().trim_start_matches('/');
	let path = match path {
		"" => "index.html",
		"favicon.ico" => "favicon.svg", // browsers probe this regardless of <link rel=icon>
		p => p,
	};
	match Assets::get(path) {
		Some(file) => {
			let mime = mime_guess::from_path(path).first_or_octet_stream();
			let cache = if path.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
			([(header::CONTENT_TYPE, mime.as_ref()), (header::CACHE_CONTROL, cache)], file.data).into_response()
		}
		None => match Assets::get("index.html") {
			Some(index) => ([(header::CONTENT_TYPE, "text/html")], index.data).into_response(),
			None => (StatusCode::NOT_FOUND, "web ui not built (run `npm run build` in web/)").into_response(),
		},
	}
}

// ------------------------------------------------------------------- state

async fn get_state(State(s): State<AppState>) -> Response {
	let snap = s.player.snapshot();
	Json(&*snap).into_response()
}

async fn ws(ws: WebSocketUpgrade, State(s): State<AppState>) -> Response {
	ws.on_upgrade(move |socket| ws_loop(socket, s.player))
}

async fn ws_loop(mut socket: WebSocket, player: PlayerHandle) {
	let mut rx = player.subscribe();
	let send = |snap: Arc<crate::model::StateSnapshot>| Message::Text(serde_json::to_string(&*snap).unwrap_or_default().into());
	let first = rx.borrow().clone();
	if socket.send(send(first)).await.is_err() {
		return;
	}
	loop {
		tokio::select! {
			changed = rx.changed() => {
				if changed.is_err() { break; }
				let snap = rx.borrow_and_update().clone();
				if socket.send(send(snap)).await.is_err() { break; }
			}
			msg = socket.recv() => {
				match msg {
					Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
					Some(Ok(Message::Ping(p))) => { let _ = socket.send(Message::Pong(p)).await; }
					_ => {}
				}
			}
		}
	}
	debug!("ws client disconnected");
}

// ------------------------------------------------------------------ search

#[derive(Deserialize)]
struct SearchQuery {
	q: String,
	#[serde(default)]
	limit: Option<usize>,
}

async fn search(State(s): State<AppState>, Query(q): Query<SearchQuery>) -> ApiResult {
	let q_trim = q.q.trim();
	if q_trim.is_empty() {
		return Err(bad("empty query"));
	}
	let tracks = if crate::resolver::is_url(q_trim) {
		s.resolver.resolve_input(q_trim).await?.1
	} else {
		s.resolver.search(q_trim, q.limit.unwrap_or(10)).await?
	};
	Ok(Json(json!({ "results": tracks })).into_response())
}

#[derive(Deserialize)]
struct AddBody {
	#[serde(default)]
	input: Option<String>,
	#[serde(default)]
	track: Option<Track>,
	#[serde(default)]
	tracks: Option<Vec<Track>>,
	#[serde(default)]
	requested_by: Option<String>,
	#[serde(default)]
	next: bool,
	#[serde(default)]
	play_now: bool,
}

async fn add(State(s): State<AppState>, Json(body): Json<AddBody>) -> ApiResult {
	let mut name = None;
	let tracks = if let Some(t) = body.track {
		vec![t]
	} else if let Some(ts) = body.tracks {
		ts
	} else if let Some(input) = body.input.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
		let (n, t) = s.resolver.resolve_input(input).await?;
		name = n;
		t
	} else {
		return Err(bad("provide `input`, `track` or `tracks`"));
	};
	if tracks.is_empty() {
		return Err(ApiError(StatusCode::NOT_FOUND, "nothing found".into()));
	}
	let added = tracks.clone();
	let requested_by = body.requested_by.filter(|r| !r.trim().is_empty());
	s.player.send(Cmd::Add { tracks, requested_by, next: body.next, play_now: body.play_now }).await;
	Ok(Json(json!({ "added": added, "playlist": name })).into_response())
}

// ------------------------------------------------------------------ player

async fn player_action(State(s): State<AppState>, Path(action): Path<String>) -> ApiResult {
	let cmd = match action.as_str() {
		"skip" => Cmd::Skip,
		"prev" => Cmd::Prev,
		"pause" => Cmd::Pause,
		"resume" | "play" => Cmd::Resume,
		"toggle" => Cmd::Toggle,
		"stop" => Cmd::Stop,
		_ => return Err(not_found()),
	};
	s.player.send(cmd).await;
	ok()
}

#[derive(Deserialize)]
struct SeekBody {
	position: f64,
}
async fn seek(State(s): State<AppState>, Json(b): Json<SeekBody>) -> ApiResult {
	s.player.send(Cmd::Seek(b.position)).await;
	ok()
}

#[derive(Deserialize)]
struct VolumeBody {
	volume: f32,
}
async fn volume(State(s): State<AppState>, Json(b): Json<VolumeBody>) -> ApiResult {
	s.player.send(Cmd::Volume(b.volume)).await;
	ok()
}

#[derive(Deserialize)]
struct LoopBody {
	mode: LoopMode,
}
async fn set_loop(State(s): State<AppState>, Json(b): Json<LoopBody>) -> ApiResult {
	s.player.send(Cmd::SetLoop(b.mode)).await;
	ok()
}

#[derive(Deserialize)]
struct SbBody {
	enabled: bool,
}
async fn set_sponsorblock(State(s): State<AppState>, Json(b): Json<SbBody>) -> ApiResult {
	s.player.send(Cmd::SetSponsorBlock(b.enabled)).await;
	ok()
}

// ------------------------------------------------------------------- queue

async fn queue_clear(State(s): State<AppState>) -> ApiResult {
	s.player.send(Cmd::Clear).await;
	ok()
}
async fn queue_shuffle(State(s): State<AppState>) -> ApiResult {
	s.player.send(Cmd::Shuffle).await;
	ok()
}
async fn queue_remove(State(s): State<AppState>, Path(id): Path<Uuid>) -> ApiResult {
	s.player.send(Cmd::Remove(id)).await;
	ok()
}
#[derive(Deserialize)]
struct MoveBody {
	to: usize,
}
async fn queue_move(State(s): State<AppState>, Path(id): Path<Uuid>, Json(b): Json<MoveBody>) -> ApiResult {
	s.player.send(Cmd::Move { id, to: b.to }).await;
	ok()
}
async fn queue_play(State(s): State<AppState>, Path(id): Path<Uuid>) -> ApiResult {
	s.player.send(Cmd::PlayItem(id)).await;
	ok()
}

// --------------------------------------------------------------- playlists

async fn playlists_list(State(s): State<AppState>) -> ApiResult {
	Ok(Json(json!({ "playlists": s.player.snapshot().playlists })).into_response())
}

#[derive(Deserialize)]
struct PlaylistCreateBody {
	name: String,
	#[serde(default)]
	tracks: Vec<Track>,
	/// Optional URL (e.g. a YouTube playlist) to import tracks from.
	#[serde(default)]
	import: Option<String>,
}

async fn playlists_create(State(s): State<AppState>, Json(b): Json<PlaylistCreateBody>) -> ApiResult {
	let mut name = b.name.trim().to_string();
	let mut tracks = b.tracks;
	if let Some(url) = b.import.as_deref().map(str::trim).filter(|u| !u.is_empty()) {
		let (n, t) = s.resolver.resolve_input(url).await?;
		if name.is_empty() {
			name = n.unwrap_or_else(|| "Imported".into());
		}
		tracks.extend(t);
	}
	if name.is_empty() {
		return Err(bad("name required"));
	}
	let pl = s.player.playlist_create(name, tracks).await.ok_or_else(|| bad("player gone"))?;
	Ok(Json(pl).into_response())
}

#[derive(Deserialize)]
struct FromQueueBody {
	name: String,
}
async fn playlists_from_queue(State(s): State<AppState>, Json(b): Json<FromQueueBody>) -> ApiResult {
	let name = b.name.trim().to_string();
	if name.is_empty() {
		return Err(bad("name required"));
	}
	let pl = s.player.playlist_from_queue(name).await.ok_or_else(|| bad("player gone"))?;
	Ok(Json(pl).into_response())
}

async fn playlist_get(State(s): State<AppState>, Path(id): Path<Uuid>) -> ApiResult {
	let pl = s.player.get_playlist(id).await.ok_or_else(not_found)?;
	Ok(Json(pl).into_response())
}
async fn playlist_delete(State(s): State<AppState>, Path(id): Path<Uuid>) -> ApiResult {
	s.player.send(Cmd::PlaylistDelete(id)).await;
	ok()
}
#[derive(Deserialize)]
struct RenameBody {
	name: String,
}
async fn playlist_rename(State(s): State<AppState>, Path(id): Path<Uuid>, Json(b): Json<RenameBody>) -> ApiResult {
	if b.name.trim().is_empty() {
		return Err(bad("name required"));
	}
	s.player.send(Cmd::PlaylistRename { id, name: b.name.trim().to_string() }).await;
	ok()
}

#[derive(Deserialize)]
struct AddTracksBody {
	#[serde(default)]
	track: Option<Track>,
	#[serde(default)]
	tracks: Option<Vec<Track>>,
	#[serde(default)]
	input: Option<String>,
}
async fn playlist_add_tracks(State(s): State<AppState>, Path(id): Path<Uuid>, Json(b): Json<AddTracksBody>) -> ApiResult {
	let mut tracks = b.tracks.unwrap_or_default();
	if let Some(t) = b.track {
		tracks.push(t);
	}
	if let Some(input) = b.input.as_deref().map(str::trim).filter(|u| !u.is_empty()) {
		tracks.extend(s.resolver.resolve_input(input).await?.1);
	}
	if tracks.is_empty() {
		return Err(bad("no tracks"));
	}
	let n = tracks.len();
	s.player.send(Cmd::PlaylistAddTracks { id, tracks }).await;
	Ok(Json(json!({ "added": n })).into_response())
}
async fn playlist_remove_track(State(s): State<AppState>, Path((id, index)): Path<(Uuid, usize)>) -> ApiResult {
	s.player.send(Cmd::PlaylistRemoveTrack { id, index }).await;
	ok()
}

#[derive(Deserialize, Default)]
struct EnqueueBody {
	#[serde(default)]
	shuffle: bool,
	#[serde(default)]
	requested_by: Option<String>,
}
async fn playlist_enqueue(State(s): State<AppState>, Path(id): Path<Uuid>, body: Option<Json<EnqueueBody>>) -> ApiResult {
	let b = body.map(|j| j.0).unwrap_or_default();
	let n = s.player.playlist_enqueue(id, b.shuffle, b.requested_by).await?;
	Ok(Json(json!({ "queued": n })).into_response())
}

#[derive(Deserialize)]
struct ImportBody {
	url: String,
}
async fn playlist_import(State(s): State<AppState>, Path(id): Path<Uuid>, Json(b): Json<ImportBody>) -> ApiResult {
	let (_, tracks) = s.resolver.resolve_input(b.url.trim()).await?;
	if tracks.is_empty() {
		warn!("import yielded no tracks");
		return Err(ApiError(StatusCode::NOT_FOUND, "nothing found".into()));
	}
	let n = tracks.len();
	s.player.send(Cmd::PlaylistAddTracks { id, tracks }).await;
	Ok(Json(json!({ "added": n })).into_response())
}
