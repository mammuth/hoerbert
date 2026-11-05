//! HTTP API + WebSocket state feed + embedded SPA.

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rust_embed::Embed;
use serde::Deserialize;
use serde_json::json;
use tracing::debug;
use uuid::Uuid;

use crate::model::Track;
use crate::player::{Cmd, PlayerHandle};
use crate::resolver::Resolver;
use crate::soundboard::Soundboard;
use crate::tts::{Tts, TtsError};

#[derive(Embed)]
#[folder = "web/dist/"]
struct Assets;

#[derive(Clone)]
pub struct AppState {
	pub player: PlayerHandle,
	pub resolver: Resolver,
	pub soundboard: Soundboard,
	pub tts: Tts,
	pub auth: Auth,
}

// -------------------------------------------------------------------- auth

const COOKIE: &str = "hoerbert_key";
/// Browsers cap cookie lifetime at 400 days; we renew it on every page load.
const COOKIE_MAX_AGE: u32 = 400 * 24 * 3600;

/// Shared web key. Wrong guesses are serialized and slowed to one per second,
/// which is what makes a short key good enough.
#[derive(Clone)]
pub struct Auth {
	key: Arc<str>,
	guesses: Arc<tokio::sync::Mutex<()>>,
}

impl Auth {
	pub fn new(key: String) -> Self { Self { key: key.into(), guesses: Default::default() } }

	async fn check(&self, candidate: &str) -> bool {
		if candidate == &*self.key {
			return true;
		}
		let _turn = self.guesses.lock().await;
		tokio::time::sleep(std::time::Duration::from_secs(1)).await;
		false
	}

	fn cookie(&self) -> String {
		format!("{COOKIE}={}; Path=/; Max-Age={COOKIE_MAX_AGE}; HttpOnly; SameSite=Lax", self.key)
	}
}

fn cookie_key(headers: &HeaderMap) -> Option<String> {
	headers
		.get_all(header::COOKIE)
		.iter()
		.filter_map(|v| v.to_str().ok())
		.flat_map(|v| v.split(';'))
		.find_map(|c| c.trim().strip_prefix(COOKIE)?.strip_prefix('=').map(str::to_owned))
}

async fn require_key(State(s): State<AppState>, req: Request, next: Next) -> Response {
	let ok = match cookie_key(req.headers()) {
		Some(k) => s.auth.check(&k).await,
		None => false,
	};
	if !ok {
		return ApiError(StatusCode::UNAUTHORIZED, "key required".into()).into_response();
	}
	next.run(req).await
}

#[derive(Deserialize)]
struct LoginBody {
	key: String,
}

async fn login(State(s): State<AppState>, Json(b): Json<LoginBody>) -> ApiResult {
	if !s.auth.check(b.key.trim()).await {
		return Err(ApiError(StatusCode::UNAUTHORIZED, "wrong key".into()));
	}
	Ok(([(header::SET_COOKIE, s.auth.cookie())], Json(json!({ "ok": true }))).into_response())
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

/// Web requests must say who they are; anonymous adds are rejected.
fn requester(r: Option<String>) -> Result<String, ApiError> {
	r.map(|r| r.trim().to_string()).filter(|r| !r.is_empty()).ok_or_else(|| bad("enter your name before adding songs"))
}

/// Optional `{"by": "name"}` body of player actions, for the history's event log.
#[derive(Deserialize)]
struct ByBody {
	#[serde(default)]
	by: Option<String>,
}

fn actor(body: Option<Json<ByBody>>) -> Option<String> {
	body.and_then(|Json(b)| b.by).map(|b| b.trim().to_string()).filter(|b| !b.is_empty())
}

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
		.route("/queue/clear", post(queue_clear))
		.route("/queue/{id}/remove", post(queue_remove))
		.route("/queue/{id}/move", post(queue_move))
		.route("/queue/{id}/play", post(queue_play))
		.route("/sounds", get(sounds))
		.route("/sounds/{slug}/play", post(sound_play))
		.route("/tts", post(tts))
		.route_layer(middleware::from_fn_with_state(state.clone(), require_key))
		.route("/login", post(login));
	Router::new()
		.nest("/api", api)
		.fallback(static_handler)
		.layer(tower_http::trace::TraceLayer::new_for_http())
		.with_state(state)
}

// ------------------------------------------------------------------ static

async fn static_handler(State(s): State<AppState>, headers: HeaderMap, uri: Uri) -> Response {
	// `?key=…` magic link: store the key, then drop it from the address bar.
	let key_param = uri.query().and_then(|q| url::form_urlencoded::parse(q.as_bytes()).find(|(k, _)| k == "key").map(|(_, v)| v.into_owned()));
	if let Some(k) = key_param {
		let to = Redirect::to(uri.path());
		return match s.auth.check(&k).await {
			true => ([(header::SET_COOKIE, s.auth.cookie())], to).into_response(),
			false => to.into_response(),
		};
	}
	let path = uri.path().trim_start_matches('/');
	let mut res = serve_asset(path);
	// Renew a valid cookie on page loads so it effectively never expires.
	if path.is_empty() && cookie_key(&headers).is_some_and(|k| *k == *s.auth.key) {
		if let Ok(v) = s.auth.cookie().parse() {
			res.headers_mut().insert(header::SET_COOKIE, v);
		}
	}
	res
}

fn serve_asset(path: &str) -> Response {
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
	// `server_now` lets clients extrapolate `position_at` without trusting their own clock.
	let send = |snap: Arc<crate::model::StateSnapshot>| {
		let mut v = serde_json::to_value(&*snap).unwrap_or_default();
		v["server_now"] = json!(crate::model::now_ms());
		Message::Text(v.to_string().into())
	};
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
	let requested_by = Some(requester(body.requested_by)?);
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
	s.player.send(Cmd::Add { tracks, requested_by, next: body.next, play_now: body.play_now }).await;
	Ok(Json(json!({ "added": added, "playlist": name })).into_response())
}

// ------------------------------------------------------------------ player

async fn player_action(State(s): State<AppState>, Path(action): Path<String>, body: Option<Json<ByBody>>) -> ApiResult {
	let cmd = match action.as_str() {
		"skip" => Cmd::Skip,
		"prev" => Cmd::Prev,
		"pause" => Cmd::Pause,
		"resume" | "play" => Cmd::Resume,
		"toggle" => Cmd::Toggle,
		"stop" => Cmd::Stop,
		_ => return Err(not_found()),
	};
	s.player.send_as(cmd, actor(body)).await;
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

// ------------------------------------------------------------------- queue

async fn queue_clear(State(s): State<AppState>, body: Option<Json<ByBody>>) -> ApiResult {
	s.player.send_as(Cmd::Clear, actor(body)).await;
	ok()
}
async fn queue_remove(State(s): State<AppState>, Path(id): Path<Uuid>, body: Option<Json<ByBody>>) -> ApiResult {
	s.player.send_as(Cmd::Remove(id), actor(body)).await;
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

// -------------------------------------------------------------- soundboard

async fn sounds(State(s): State<AppState>) -> Response {
	let tts = s.tts.enabled().then(|| json!({ "max_chars": s.tts.max_chars() }));
	Json(json!({ "sounds": s.soundboard.list(), "tts": tts })).into_response()
}
async fn sound_play(State(s): State<AppState>, Path(slug): Path<String>, body: Option<Json<ByBody>>) -> ApiResult {
	let sound = s.soundboard.get(&slug.to_ascii_lowercase()).ok_or_else(not_found)?;
	s.player.send_as(Cmd::PlaySound(sound), actor(body)).await;
	ok()
}

#[derive(Deserialize)]
struct TtsBody {
	text: String,
	#[serde(default)]
	voice: Option<String>,
	#[serde(default)]
	by: Option<String>,
}
async fn tts(State(s): State<AppState>, Json(b): Json<TtsBody>) -> ApiResult {
	if !s.tts.enabled() {
		return Err(not_found());
	}
	// The cooldown is per name, so speaking needs one.
	let by = b.by.map(|r| r.trim().to_string()).filter(|r| !r.is_empty()).ok_or_else(|| bad("enter your name first"))?;
	let speech = s.tts.prepare(&b.text, b.voice.as_deref(), &by).map_err(|e| match e {
		TtsError::Cooldown(_) => ApiError(StatusCode::TOO_MANY_REQUESTS, e.to_string()),
		TtsError::Invalid(_) => bad(e.to_string()),
	})?;
	s.player.send_as(Cmd::Say(speech), Some(by)).await;
	ok()
}
