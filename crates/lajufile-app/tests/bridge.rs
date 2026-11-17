mod support;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use lajufile_app::bridge::{BridgeState, health_check};
use lajufile_app::engine::{Command, EngineConfig, EngineHandle, Snapshot};
use lajufile_core::cookie::unprotect_cookie;
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
        // Generous: a loaded machine can take seconds to bind the bridge.
        Duration::from_secs(15),
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
    post_download_with_token(port, VALID_TOKEN, body).await
}

async fn post_download_with_token(port: u16, token: &str, body: String) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}/downloads"))
        .header(
            HeaderName::from_static("x-lajufile-token"),
            HeaderValue::from_str(token).unwrap(),
        )
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

    let blocked = reqwest::Client::new()
        .request(reqwest::Method::OPTIONS, &url)
        .header("Origin", "https://evil.example")
        .header("Access-Control-Request-Method", "POST")
        .header("Access-Control-Request-Headers", "content-type")
        .send()
        .await
        .unwrap();
    assert!(
        !blocked
            .headers()
            .contains_key("access-control-allow-origin")
    );

    let bad_scheme =
        post_download(port, r#"{"url":"file:///C:/Windows/win.ini"}"#.to_string()).await;
    assert_eq!(bad_scheme.status(), reqwest::StatusCode::BAD_REQUEST);
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
    let final_path = snapshot.downloads[0].save_path.clone();
    let temp_path = snapshot.downloads[0].temp_path.clone();
    assert_eq!(std::fs::metadata(&final_path).unwrap().len(), 1_048_576);
    assert!(!temp_path.exists());
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

#[tokio::test(flavor = "multi_thread")]
async fn bridge_context_is_persisted_encrypted() {
    let server = start_server();
    let (handle, dir) = start_engine(0);
    let port = bound_port(&handle);
    let (name, value) = token_header();
    let body = serde_json::json!({
        "url": format!("{}/auth.bin", server.base),
        "referrer": "https://example.test/page",
        "user_agent": "LajuFileTest/1.0",
        "cookie": "session=abc"
    })
    .to_string();
    let response = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}/downloads"))
        .header(name, value)
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    wait_for(
        &handle,
        |snapshot| {
            snapshot.downloads.len() == 1 && snapshot.downloads[0].status == DownloadStatus::Done
        },
        Duration::from_secs(10),
    );
    handle.shutdown();

    let store = Store::open(&dir.path().join("lajufile.db")).unwrap();
    let download = &store.list_downloads().unwrap()[0];
    let encrypted = download.cookie_enc.as_deref().unwrap();
    assert_ne!(encrypted, b"session=abc");
    assert!(!encrypted.windows(7).any(|window| window == b"session"));
    assert_eq!(unprotect_cookie(encrypted).unwrap(), "session=abc");
    assert_eq!(
        download.referrer.as_deref(),
        Some("https://example.test/page")
    );
    assert_eq!(download.user_agent.as_deref(), Some("LajuFileTest/1.0"));
}

#[test]
fn health_check_detects_bridge() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        }
    });
    assert!(health_check(port, Duration::from_secs(2)).is_ok());
    server.join().unwrap();

    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let closed_port = closed.local_addr().unwrap().port();
    drop(closed);
    assert!(health_check(closed_port, Duration::from_secs(1)).is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn token_rotation_applies_to_live_bridge() {
    const SECOND_TOKEN: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
    let (handle, _dir) = start_engine(0);
    let port = bound_port(&handle);

    let first = post_download_with_token(port, VALID_TOKEN, "{}".to_string()).await;
    assert_eq!(first.status(), reqwest::StatusCode::BAD_REQUEST);

    let mut settings = wait_for(
        &handle,
        |snapshot| snapshot.bridge.port == port,
        Duration::from_secs(5),
    )
    .settings;
    settings.bridge_token = SECOND_TOKEN.to_string();
    handle.send(Command::UpdateSettings(settings)).unwrap();
    wait_for(
        &handle,
        |snapshot| snapshot.settings.bridge_token == SECOND_TOKEN,
        Duration::from_secs(5),
    );

    let old = post_download_with_token(port, VALID_TOKEN, "{}".to_string()).await;
    assert_eq!(old.status(), reqwest::StatusCode::UNAUTHORIZED);
    let new = post_download_with_token(port, SECOND_TOKEN, "{}".to_string()).await;
    assert_eq!(new.status(), reqwest::StatusCode::BAD_REQUEST);
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

#[tokio::test(flavor = "multi_thread")]
async fn empty_configured_token_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings {
        download_dir: dir.path().join("downloads"),
        bridge_port: 0,
        bridge_token: String::new(),
        ..Settings::default()
    };
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings,
    })
    .unwrap();
    let port = bound_port(&handle);
    let response = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}/downloads"))
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
    handle.shutdown();
    let store = Store::open(&dir.path().join("lajufile.db")).unwrap();
    assert!(store.list_downloads().unwrap().is_empty());
}

async fn post_json(port: u16, path: &str, body: serde_json::Value) -> reqwest::Response {
    let (name, value) = token_header();
    reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}{path}"))
        .header(name, value)
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .unwrap()
}

/// A tools dir with both files present, so `tools_ready` passes; the empty
/// `yt-dlp.exe` then fails to spawn, which is the error path under test.
fn stub_tools() -> std::path::PathBuf {
    static DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join("lajufile-bridge-tools");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(lajufile_app::tools::YTDLP), b"").unwrap();
        std::fs::write(dir.join(lajufile_app::tools::FFMPEG), b"").unwrap();
        unsafe { std::env::set_var("LAJUFILE_TOOLS_DIR", &dir) };
        dir
    })
    .clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn video_formats_reports_a_broken_yt_dlp_with_422() {
    stub_tools();
    let (handle, _dir) = start_engine(0);
    let port = bound_port(&handle);
    let response = post_json(
        port,
        "/video/formats",
        serde_json::json!({ "url": "https://www.youtube.com/watch?v=abc" }),
    )
    .await;
    assert_eq!(response.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);
    let json: serde_json::Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
    assert!(
        json["error"].as_str().is_some_and(|e| e.contains("yt-dlp")),
        "unexpected body: {json}"
    );
    handle.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn video_post_queues_a_video_row() {
    stub_tools();
    let (handle, dir) = start_engine(0);
    let port = bound_port(&handle);

    let missing = post_json(
        port,
        "/video",
        serde_json::json!({ "url": "https://www.youtube.com/watch?v=abc" }),
    )
    .await;
    assert_eq!(missing.status(), reqwest::StatusCode::BAD_REQUEST);

    let response = post_json(
        port,
        "/video",
        serde_json::json!({
            "url": "https://www.youtube.com/watch?v=abc",
            "video_format": "bv*[height<=360]+ba"
        }),
    )
    .await;
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    let json: serde_json::Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
    assert_eq!(json["filename"], "abc.mp4");
    handle.shutdown();

    let store = Store::open(&dir.path().join("lajufile.db")).unwrap();
    let downloads = store.list_downloads().unwrap();
    assert_eq!(downloads.len(), 1);
    assert_eq!(downloads[0].kind, "video");
    assert_eq!(
        downloads[0].video_format.as_deref(),
        Some("bv*[height<=360]+ba")
    );
}

fn start_engine_prompt(port: u16) -> (EngineHandle, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings {
        download_dir: dir.path().join("downloads"),
        bridge_port: port,
        bridge_token: VALID_TOKEN.to_string(),
        ask_where_to_save: true,
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

#[tokio::test(flavor = "multi_thread")]
async fn capture_prompt_queues_into_chosen_folder() {
    let server = start_server();
    let (handle, dir) = start_engine_prompt(0);
    let port = bound_port(&handle);
    let chosen = dir.path().join("chosen");
    let body = format!(r#"{{"url":"{}/file.bin"}}"#, server.base);
    let request = tokio::spawn(async move { post_download(port, body).await });

    let snapshot = wait_for(
        &handle,
        |snapshot| snapshot.pending_capture.is_some(),
        Duration::from_secs(5),
    );
    let key = snapshot.pending_capture.as_ref().unwrap().key;
    handle
        .send(Command::CaptureFolderDecision {
            key,
            path: Some(chosen.clone()),
        })
        .unwrap();

    let response = request.await.unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    let snapshot = wait_for(
        &handle,
        |snapshot| {
            snapshot
                .downloads
                .iter()
                .any(|download| download.status == DownloadStatus::Done)
        },
        Duration::from_secs(10),
    );
    assert!(snapshot.downloads[0].save_path.starts_with(&chosen));
    handle.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn capture_prompt_cancel_returns_204_without_record() {
    let server = start_server();
    let (handle, dir) = start_engine_prompt(0);
    let port = bound_port(&handle);
    let body = format!(r#"{{"url":"{}/file.bin"}}"#, server.base);
    let request = tokio::spawn(async move { post_download(port, body).await });

    let snapshot = wait_for(
        &handle,
        |snapshot| snapshot.pending_capture.is_some(),
        Duration::from_secs(5),
    );
    let key = snapshot.pending_capture.as_ref().unwrap().key;
    handle
        .send(Command::CaptureFolderDecision { key, path: None })
        .unwrap();

    let response = request.await.unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::NO_CONTENT);
    handle.shutdown();
    let store = Store::open(&dir.path().join("lajufile.db")).unwrap();
    assert!(store.list_downloads().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn capture_decision_supports_custom_name_and_download_later() {
    let server = start_server();
    let (handle, dir) = start_engine_prompt(0);
    let port = bound_port(&handle);
    let chosen = dir.path().join("chosen");
    let body = format!(r#"{{"url":"{}/file.bin"}}"#, server.base);
    let request = tokio::spawn(async move { post_download(port, body).await });

    let snapshot = wait_for(
        &handle,
        |snapshot| {
            snapshot
                .pending_capture
                .as_ref()
                .is_some_and(|prompt| prompt.size == Some(1_048_576))
        },
        Duration::from_secs(5),
    );
    let prompt = snapshot.pending_capture.as_ref().unwrap();
    assert_eq!(prompt.file_name, "file.bin");
    let key = prompt.key;

    handle
        .send(Command::CaptureDecision {
            key,
            folder: chosen.clone(),
            file_name: Some("custom.bin".to_string()),
            start_now: false,
        })
        .unwrap();

    let response = request.await.unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    let json: serde_json::Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
    assert_eq!(json["filename"], "custom.bin");
    assert_eq!(json["size"], 1_048_576);

    let snapshot = wait_for(
        &handle,
        |snapshot| snapshot.downloads.len() == 1,
        Duration::from_secs(5),
    );
    assert_eq!(snapshot.downloads[0].file_name, "custom.bin");
    assert_eq!(snapshot.downloads[0].status, DownloadStatus::Paused);
    assert_eq!(snapshot.downloads[0].save_path, chosen.join("custom.bin"));
    handle.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn capture_prompts_are_served_in_fifo_order() {
    let server = start_server();
    let (handle, dir) = start_engine_prompt(0);
    let port = bound_port(&handle);
    let chosen = dir.path().join("chosen");

    let first_body = format!(r#"{{"url":"{}/file.bin"}}"#, server.base);
    let first_req = tokio::spawn(async move { post_download(port, first_body).await });

    let first_snapshot = wait_for(
        &handle,
        |snapshot| snapshot.pending_capture.is_some(),
        Duration::from_secs(5),
    );
    let first_prompt = first_snapshot.pending_capture.as_ref().unwrap();
    assert_eq!(first_prompt.key, 0);

    let second_body = format!(r#"{{"url":"{}/slow.bin"}}"#, server.base);
    let second_req = tokio::spawn(async move { post_download(port, second_body).await });
    tokio::time::sleep(Duration::from_millis(350)).await;

    handle
        .send(Command::CaptureFolderDecision {
            key: 0,
            path: Some(chosen.clone()),
        })
        .unwrap();
    assert_eq!(
        first_req.await.unwrap().status(),
        reqwest::StatusCode::CREATED
    );

    let second_snapshot = wait_for(
        &handle,
        |snapshot| {
            snapshot
                .pending_capture
                .as_ref()
                .is_some_and(|prompt| prompt.key == 1)
        },
        Duration::from_secs(5),
    );
    assert!(
        second_snapshot
            .pending_capture
            .as_ref()
            .unwrap()
            .url
            .ends_with("/slow.bin")
    );

    handle
        .send(Command::CaptureFolderDecision {
            key: 1,
            path: Some(chosen),
        })
        .unwrap();
    assert_eq!(
        second_req.await.unwrap().status(),
        reqwest::StatusCode::CREATED
    );
    handle.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn rules_requires_token_and_returns_settings_filters() {
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings {
        download_dir: dir.path().join("downloads"),
        bridge_port: 0,
        bridge_token: VALID_TOKEN.to_string(),
        excluded_domains: "ads.example.com, *.blocked.net".to_string(),
        captured_extensions: "zip, iso".to_string(),
        ..Settings::default()
    };
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings,
    })
    .unwrap();
    let port = bound_port(&handle);
    let url = format!("http://127.0.0.1:{port}/rules");
    let client = reqwest::Client::new();

    let unauthorized = client.get(&url).send().await.unwrap();
    assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);

    let (name, value) = token_header();
    let authorized = client.get(&url).header(name, value).send().await.unwrap();
    assert_eq!(authorized.status(), reqwest::StatusCode::OK);
    let json: serde_json::Value =
        serde_json::from_slice(&authorized.bytes().await.unwrap()).unwrap();
    assert_eq!(json["excluded_domains"], "ads.example.com, *.blocked.net");
    assert_eq!(json["captured_extensions"], "zip, iso");
    let paired = wait_for(
        &handle,
        |snapshot| snapshot.settings.browser_paired,
        Duration::from_secs(5),
    );
    assert!(paired.settings.browser_paired);
    handle.shutdown();
    let persisted = Settings::load_or_create(dir.path()).unwrap();
    assert!(persisted.browser_paired);
}

#[tokio::test(flavor = "multi_thread")]
async fn bridge_rejects_dns_rebinding_host_headers() {
    let (handle, _dir) = start_engine(0);
    let port = bound_port(&handle);
    let client = reqwest::Client::new();

    let rebound_health = client
        .get(format!("http://127.0.0.1:{port}/health"))
        .header("host", format!("evil.example:{port}"))
        .send()
        .await
        .unwrap();
    assert_eq!(rebound_health.status(), reqwest::StatusCode::FORBIDDEN);

    let (name, value) = token_header();
    let rebound_post = client
        .post(format!("http://127.0.0.1:{port}/downloads"))
        .header(name, value)
        .header("host", format!("evil.example:{port}"))
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(rebound_post.status(), reqwest::StatusCode::FORBIDDEN);

    let valid_localhost = client
        .get(format!("http://127.0.0.1:{port}/health"))
        .header("host", format!("localhost:{port}"))
        .send()
        .await
        .unwrap();
    assert_eq!(valid_localhost.status(), reqwest::StatusCode::OK);

    let mut raw = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    raw.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    raw.write_all(b"GET /health HTTP/1.1\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut buf = String::new();
    raw.read_to_string(&mut buf).unwrap();
    assert!(
        buf.starts_with("HTTP/1.1 403"),
        "expected 403 for missing Host header, got: {buf:?}"
    );

    handle.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn bridge_cors_allows_extensions_only_and_blocks_localhost_web_origins() {
    let (handle, _dir) = start_engine(0);
    let port = bound_port(&handle);
    let url = format!("http://127.0.0.1:{port}/downloads");
    let client = reqwest::Client::new();

    for blocked_origin in [
        "http://localhost:3000",
        "http://127.0.0.1:8080",
        "http://[::1]:5173",
    ] {
        let resp = client
            .request(reqwest::Method::OPTIONS, &url)
            .header("Origin", blocked_origin)
            .header("Access-Control-Request-Method", "POST")
            .header(
                "Access-Control-Request-Headers",
                "content-type,x-lajufile-token",
            )
            .send()
            .await
            .unwrap();
        assert!(
            !resp.headers().contains_key("access-control-allow-origin"),
            "unexpected CORS allow for web origin {blocked_origin}"
        );
    }

    handle.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn wake_requires_token_and_preserves_browser_paired_false() {
    let (handle, _dir) = start_engine(0);
    let port = bound_port(&handle);
    let url = format!("http://127.0.0.1:{port}/wake");
    let client = reqwest::Client::new();

    let unauthorized = client.post(&url).send().await.unwrap();
    assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);

    let (name, value) = token_header();
    let authorized = client.post(&url).header(name, value).send().await.unwrap();
    assert_eq!(authorized.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&authorized.bytes().await.unwrap()).unwrap();
    assert_eq!(body["ok"], true);

    let woken = wait_for(
        &handle,
        |snapshot| snapshot.wake_seq >= 1,
        Duration::from_secs(5),
    );
    assert!(!woken.settings.browser_paired);
    handle.shutdown();
}
