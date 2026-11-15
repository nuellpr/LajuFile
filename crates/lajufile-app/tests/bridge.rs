mod support;

use std::time::{Duration, Instant};

use lajufile_app::bridge::BridgeState;
use lajufile_app::engine::{EngineConfig, EngineHandle, Snapshot};
use lajufile_core::settings::Settings;
use lajufile_core::store::{DownloadStatus, Store};
use reqwest::header::{HeaderName, HeaderValue};
use support::start_server;

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

async fn post_download(port: u16, body: String) -> reqwest::Response {
    let (name, value) = token_header();
    reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}/downloads"))
        .header(name, value)
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .unwrap()
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

#[tokio::test(flavor = "multi_thread")]
async fn bridge_add_creates_download_and_returns_201() {
    let server = start_server();
    let (handle, dir) = start_engine(0);
    let port = bound_port(&handle);

    let response = post_download(port, format!(r#"{{"url":"{}/file.bin"}}"#, server.base)).await;
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    let json: serde_json::Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
    assert!(json["id"].as_i64().is_some_and(|id| id > 0));
    assert_eq!(json["filename"], "file.bin");
    assert_eq!(json["size"], 1_048_576);

    let snapshot = wait_for(
        &handle,
        |snapshot| {
            snapshot.downloads.len() == 1 && snapshot.downloads[0].status == DownloadStatus::Done
        },
        Duration::from_secs(15),
    );
    assert!(snapshot.downloads[0].save_path.exists());
    handle.shutdown();
    let store = Store::open(&dir.path().join("lajufile.db")).unwrap();
    assert_eq!(store.list_downloads().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn bridge_duplicate_returns_409() {
    let server = start_server();
    let (handle, dir) = start_engine(0);
    let port = bound_port(&handle);
    let body = format!(r#"{{"url":"{}/slow.bin"}}"#, server.base);

    let first = post_download(port, body.clone()).await;
    assert_eq!(first.status(), reqwest::StatusCode::CREATED);
    let first_json: serde_json::Value =
        serde_json::from_slice(&first.bytes().await.unwrap()).unwrap();
    let first_id = first_json["id"].as_i64().unwrap();

    let second = post_download(port, body).await;
    assert_eq!(second.status(), reqwest::StatusCode::CONFLICT);
    let second_json: serde_json::Value =
        serde_json::from_slice(&second.bytes().await.unwrap()).unwrap();
    assert_eq!(second_json["id"], first_id);
    handle.shutdown();
    let store = Store::open(&dir.path().join("lajufile.db")).unwrap();
    assert_eq!(store.list_downloads().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn bridge_probe_failure_returns_422_without_record() {
    let server = start_server();
    let (handle, dir) = start_engine(0);
    let port = bound_port(&handle);

    let response = post_download(port, format!(r#"{{"url":"{}/missing"}}"#, server.base)).await;
    assert_eq!(response.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);
    handle.shutdown();
    let store = Store::open(&dir.path().join("lajufile.db")).unwrap();
    assert!(store.list_downloads().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn bridge_rejects_invalid_url_with_400() {
    let (handle, dir) = start_engine(0);
    let port = bound_port(&handle);
    let response = post_download(port, r#"{"url":"not a url"}"#.to_string()).await;
    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    handle.shutdown();
    let store = Store::open(&dir.path().join("lajufile.db")).unwrap();
    assert!(store.list_downloads().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn bridge_probe_timeout_returns_422_without_record() {
    let server = start_server();
    let (handle, dir) = start_engine(0);
    let port = bound_port(&handle);
    let start = Instant::now();

    let response = post_download(
        port,
        format!(r#"{{"url":"{}/slow-probe.bin"}}"#, server.base),
    )
    .await;
    assert_eq!(response.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);
    assert!(start.elapsed() >= Duration::from_secs(2));
    assert!(start.elapsed() < Duration::from_secs(6));
    handle.shutdown();
    let store = Store::open(&dir.path().join("lajufile.db")).unwrap();
    assert!(store.list_downloads().unwrap().is_empty());
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
