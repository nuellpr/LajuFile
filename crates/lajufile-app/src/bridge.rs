use std::net::Ipv4Addr;
use std::sync::mpsc::Sender;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, HeaderName, Method, StatusCode, header},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tower_http::cors::{Any, CorsLayer};

use crate::engine::Command;

pub const BRIDGE_HEADER: &str = "x-lajufile-token";
pub const MAX_BODY_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BridgeAddRequest {
    pub url: String,
    #[serde(default)]
    pub filename: Option<String>,
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

fn router(state: BridgeAppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/downloads", post(downloads))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET, Method::POST])
                .allow_headers([header::CONTENT_TYPE, HeaderName::from_static(BRIDGE_HEADER)]),
        )
        .with_state(state)
}

async fn health() -> Json<Value> {
    Json(json!({ "version": env!("CARGO_PKG_VERSION") }))
}

async fn downloads(
    State(state): State<BridgeAppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    authorize(&headers, &state.token)?;
    let request: BridgeAddRequest = serde_json::from_slice(&body).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid request" })),
        )
    })?;

    let (reply, response) = tokio::sync::oneshot::channel();
    if state
        .commands
        .send(Command::BridgeAdd { request, reply })
        .is_err()
    {
        return Err(internal("engine stopped"));
    }

    let outcome = tokio::time::timeout(Duration::from_secs(5), response)
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

fn authorize(
    headers: &HeaderMap,
    token: &Arc<RwLock<String>>,
) -> Result<(), (StatusCode, Json<Value>)> {
    let supplied = headers
        .get(BRIDGE_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let expected = token.read().map(|token| token.clone()).unwrap_or_default();
    if constant_time_eq(supplied.as_bytes(), expected.as_bytes()) {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "unauthorized" })),
        ))
    }
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
