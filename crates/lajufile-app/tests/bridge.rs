use std::time::{Duration, Instant};

use lajufile_app::bridge::BridgeState;
use lajufile_app::engine::{EngineConfig, EngineHandle, Snapshot};
use lajufile_core::settings::Settings;
use reqwest::header::{HeaderName, HeaderValue};

const VALID_TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn start_engine(port: u16) -> (EngineHandle, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings {
        download_dir: dir.path().join("downloads"),
        bridge_port: port,
        bridge_token: VALID_TOKEN.to_string(),
        ..Settings::default()
    };
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings,
    })
    .unwrap();
    (handle, dir)
}

fn wait_for(
    handle: &EngineHandle,
    predicate: impl Fn(&Snapshot) -> bool,
    timeout: Duration,
) -> Snapshot {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(snapshot) = handle.latest_snapshot() {
            if predicate(&snapshot) {
                return snapshot;
            }
        }
        assert!(Instant::now() < deadline, "timed out waiting for snapshot");
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn bound_port(handle: &EngineHandle) -> u16 {
    let snapshot = wait_for(
        handle,
        |snapshot| snapshot.bridge.state == BridgeState::Bound && snapshot.bridge.port != 0,
        Duration::from_secs(5),
    );
    snapshot.bridge.port
}

fn token_header() -> (HeaderName, HeaderValue) {
    (
        HeaderName::from_static("x-lajufile-token"),
        HeaderValue::from_static(VALID_TOKEN),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn health_is_public_and_returns_version() {
    let (handle, _dir) = start_engine(0);
    let port = bound_port(&handle);
    let response = reqwest::get(format!("http://127.0.0.1:{port}/health"))
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let json: serde_json::Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
    assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
    handle.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn bridge_rejects_missing_and_wrong_tokens() {
    let (handle, _dir) = start_engine(0);
    let port = bound_port(&handle);
    let url = format!("http://127.0.0.1:{port}/downloads");
    let client = reqwest::Client::new();

    let missing = client
        .post(&url)
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), reqwest::StatusCode::UNAUTHORIZED);

    let wrong = client
        .post(&url)
        .header("x-lajufile-token", "wrong")
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), reqwest::StatusCode::UNAUTHORIZED);
    handle.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn options_allows_extension_cors_headers() {
    let (handle, _dir) = start_engine(0);
    let port = bound_port(&handle);
    let url = format!("http://127.0.0.1:{port}/downloads");
    let response = reqwest::Client::new()
        .request(reqwest::Method::OPTIONS, &url)
        .header("Origin", "chrome-extension://abc")
        .header("Access-Control-Request-Method", "POST")
        .header(
            "Access-Control-Request-Headers",
            "content-type,x-lajufile-token",
        )
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    assert!(
        response
            .headers()
            .contains_key("access-control-allow-origin")
    );
    let allowed = response
        .headers()
        .get("access-control-allow-headers")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    assert!(allowed.contains("content-type"));
    assert!(allowed.contains("x-lajufile-token"));
    handle.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn oversized_body_is_rejected() {
    let (handle, _dir) = start_engine(0);
    let port = bound_port(&handle);
    let url = format!("http://127.0.0.1:{port}/downloads");
    let (name, value) = token_header();
    let response = reqwest::Client::new()
        .post(&url)
        .header(name, value)
        .body(vec![b'a'; 65 * 1024])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
    handle.shutdown();
}

#[test]
fn bridge_bind_failure_is_reported() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (handle, _dir) = start_engine(port);
    let snapshot = wait_for(
        &handle,
        |snapshot| snapshot.bridge.state == BridgeState::Error,
        Duration::from_secs(5),
    );
    assert!(
        snapshot
            .bridge
            .message
            .as_deref()
            .is_some_and(|m| !m.is_empty())
    );
    drop(listener);
    handle.shutdown();
}
