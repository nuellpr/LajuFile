use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::sync::mpsc::Sender;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::engine::Command;

pub const BRIDGE_HEADER: &str = "x-lajufile-token";
pub const MAX_BODY_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BridgeAddRequest {
    pub url: String,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub referrer: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub cookie: Option<String>,
    /// yt-dlp selector; when set the request is queued as a video job.
    #[serde(default)]
    pub video_format: Option<String>,
    /// Pre-estimated byte size of the selected video stream + audio.
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct FormatsRequest {
    url: String,
    #[serde(default)]
    cookie: Option<String>,
}

#[derive(Debug)]
pub enum BridgeAddOutcome {
    Created {
        id: i64,
        filename: String,
        size: Option<u64>,
    },
    Conflict {
        id: i64,
    },
    Invalid(String),
    ProbeFailed(String),
    Canceled,
    Internal(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeState {
    Starting,
    Bound,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeStatus {
    pub state: BridgeState,
    pub port: u16,
    pub message: Option<String>,
}

impl BridgeStatus {
    pub fn starting(port: u16) -> Self {
        Self {
            state: BridgeState::Starting,
            port,
            message: None,
        }
    }
}

pub struct BridgeServer {
    pub status: BridgeStatus,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl BridgeServer {
    pub fn shutdown(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}

#[derive(Clone)]
struct BridgeAppState {
    token: Arc<RwLock<String>>,
    commands: Sender<Command>,
}

pub async fn start(
    port: u16,
    token: Arc<RwLock<String>>,
    commands: Sender<Command>,
) -> BridgeServer {
    let listener = match tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await {
        Ok(listener) => listener,
        Err(error) => {
            return BridgeServer {
                status: BridgeStatus {
                    state: BridgeState::Error,
                    port,
                    message: Some(format!("bind 127.0.0.1:{port}: {error}")),
                },
                shutdown: None,
            };
        }
    };
    let bound_port = match listener.local_addr() {
        Ok(addr) => addr.port(),
        Err(error) => {
            return BridgeServer {
                status: BridgeStatus {
                    state: BridgeState::Error,
                    port,
                    message: Some(format!("read bridge address: {error}")),
                },
                shutdown: None,
            };
        }
    };

    let state = BridgeAppState { token, commands };
    let app = router(state);
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await;
    });

    BridgeServer {
        status: BridgeStatus {
            state: BridgeState::Bound,
            port: bound_port,
            message: None,
        },
        shutdown: Some(shutdown_tx),
    }
}

fn is_local_origin(origin: &str, prefix: &str) -> bool {
    match origin.strip_prefix(prefix) {
        Some("") => true,
        Some(rest) => rest.starts_with(':'),
        None => false,
    }
}

fn is_allowed_origin(origin: &HeaderValue) -> bool {
    let Ok(s) = origin.to_str() else {
        return false;
    };
    s.starts_with("chrome-extension://")
        || s.starts_with("moz-extension://")
        || s.starts_with("extension://")
        || s.starts_with("safari-web-extension://")
        || is_local_origin(s, "http://127.0.0.1")
        || is_local_origin(s, "http://localhost")
        || is_local_origin(s, "http://[::1]")
}

fn router(state: BridgeAppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/rules", get(rules))
        .route("/downloads", post(downloads))
        .route("/video", post(video))
        .route("/video/formats", post(video_formats))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::predicate(|origin, _| {
                    is_allowed_origin(origin)
                }))
                .allow_methods([Method::GET, Method::POST])
                .allow_headers([header::CONTENT_TYPE, HeaderName::from_static(BRIDGE_HEADER)]),
        )
        .with_state(state)
}

async fn health() -> Json<Value> {
    Json(json!({ "version": env!("CARGO_PKG_VERSION") }))
}

fn authorize_and_pair(
    headers: &HeaderMap,
    state: &BridgeAppState,
) -> Result<(), (StatusCode, Json<Value>)> {
    authorize(headers, &state.token)?;
    let _ = state.commands.send(Command::MarkBrowserPaired);
    Ok(())
}

async fn rules(
    State(state): State<BridgeAppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    authorize_and_pair(&headers, &state)?;
    let (reply, response) = tokio::sync::oneshot::channel();
    if state.commands.send(Command::GetRules { reply }).is_err() {
        return Err(internal("engine stopped"));
    }
    let (excluded_domains, captured_extensions) =
        tokio::time::timeout(Duration::from_secs(5), response)
            .await
            .map_err(|_| internal("engine timeout"))?
            .map_err(|_| internal("engine stopped"))?;
    Ok(Json(json!({
        "excluded_domains": excluded_domains,
        "captured_extensions": captured_extensions,
    })))
}

async fn downloads(
    State(state): State<BridgeAppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    authorize_and_pair(&headers, &state)?;
    let request: BridgeAddRequest = serde_json::from_slice(&body).map_err(|_| invalid_body())?;
    queue_add(&state, request).await
}

async fn video(
    State(state): State<BridgeAppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    authorize_and_pair(&headers, &state)?;
    let request: BridgeAddRequest = serde_json::from_slice(&body).map_err(|_| invalid_body())?;
    if request
        .video_format
        .as_deref()
        .map(str::trim)
        .is_none_or(str::is_empty)
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "video_format is required" })),
        ));
    }
    queue_add(&state, request).await
}

async fn video_formats(
    State(state): State<BridgeAppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    authorize_and_pair(&headers, &state)?;
    let request: FormatsRequest = serde_json::from_slice(&body).map_err(|_| invalid_body())?;

    let (reply, response) = tokio::sync::oneshot::channel();
    if state
        .commands
        .send(Command::VideoFormats {
            url: request.url,
            cookie: request.cookie,
            reply,
        })
        .is_err()
    {
        return Err(internal("engine stopped"));
    }
    let listing = tokio::time::timeout(Duration::from_secs(65), response)
        .await
        .map_err(|_| internal("engine timeout"))?
        .map_err(|_| internal("engine stopped"))?;

    match listing {
        Ok(listing) => {
            let formats: Vec<Value> = listing
                .choices
                .iter()
                .map(|choice| {
                    json!({
                        "selector": choice.selector,
                        "label": choice.label,
                        "fps": choice.fps,
                        "size": choice.size,
                    })
                })
                .collect();
            Ok(Json(json!({ "title": listing.title, "formats": formats })))
        }
        Err(message) => Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": message })),
        )),
    }
}

async fn queue_add(
    state: &BridgeAppState,
    request: BridgeAddRequest,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let (reply, response) = tokio::sync::oneshot::channel();
    if state
        .commands
        .send(Command::BridgeAdd { request, reply })
        .is_err()
    {
        return Err(internal("engine stopped"));
    }

    let outcome = tokio::time::timeout(Duration::from_secs(65), response)
        .await
        .map_err(|_| internal("engine timeout"))?
        .map_err(|_| internal("engine stopped"))?;

    Ok(match outcome {
        BridgeAddOutcome::Created { id, filename, size } => (
            StatusCode::CREATED,
            Json(json!({ "id": id, "filename": filename, "size": size })),
        ),
        BridgeAddOutcome::Conflict { id } => (
            StatusCode::CONFLICT,
            Json(json!({ "id": id, "error": "already in LajuFile" })),
        ),
        BridgeAddOutcome::Invalid(message) => {
            (StatusCode::BAD_REQUEST, Json(json!({ "error": message })))
        }
        BridgeAddOutcome::ProbeFailed(message) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": message })),
        ),
        BridgeAddOutcome::Canceled => (StatusCode::NO_CONTENT, Json(json!({}))),
        BridgeAddOutcome::Internal(message) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": message })),
        ),
    })
}

fn internal(message: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": message })),
    )
}

fn invalid_body() -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": "invalid request" })),
    )
}

fn authorize(
    headers: &HeaderMap,
    token: &Arc<RwLock<String>>,
) -> Result<(), (StatusCode, Json<Value>)> {
    let supplied = headers
        .get(BRIDGE_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let expected = token
        .read()
        .ok()
        .map(|token| token.clone())
        .unwrap_or_default();
    if valid_token(&expected) && constant_time_eq(supplied.as_bytes(), expected.as_bytes()) {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "unauthorized" })),
        ))
    }
}

fn valid_token(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in left.iter().zip(right) {
        difference |= left ^ right;
    }
    difference == 0
}

pub fn health_check(port: u16, timeout: Duration) -> Result<(), String> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, timeout)
        .map_err(|error| format!("connect failed: {error}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| format!("read timeout: {error}"))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|error| format!("write timeout: {error}"))?;
    stream
        .write_all(
            format!("GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .map_err(|error| format!("request failed: {error}"))?;
    let mut buffer = [0u8; 64];
    let read = stream
        .read(&mut buffer)
        .map_err(|error| format!("response failed: {error}"))?;
    let status = String::from_utf8_lossy(&buffer[..read]);
    let mut parts = status.split_whitespace();
    let protocol = parts.next().unwrap_or_default();
    let code = parts.next().unwrap_or_default();
    if protocol.starts_with("HTTP/") && code == "200" {
        Ok(())
    } else {
        Err(format!("unexpected health response: {status:?}"))
    }
}
