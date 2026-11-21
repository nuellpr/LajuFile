use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
    routing::get,
};
use tokio_stream::StreamExt;

pub const SEG_SIZE: usize = 4 * 1024 * 1024;
const CHUNK: usize = 64 * 1024;

#[derive(Clone)]
struct ServerState {
    ranges: Arc<Mutex<Vec<u64>>>,
    once_hits: Arc<std::sync::atomic::AtomicUsize>,
}

pub struct TestServer {
    pub base: String,
    #[allow(dead_code)] // Used by the engine test target only.
    ranges: Arc<Mutex<Vec<u64>>>,
    #[allow(dead_code)] // Used by the engine test target only.
    once_hits: Arc<std::sync::atomic::AtomicUsize>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl TestServer {
    #[allow(dead_code)] // Used by the engine test target only.
    pub fn ranges(&self) -> Vec<u64> {
        self.ranges.lock().unwrap().clone()
    }

    #[allow(dead_code)] // Used by the engine test target only.
    pub fn once_hits(&self) -> usize {
        self.once_hits.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

pub fn start_server() -> TestServer {
    let (addr_tx, addr_rx) = std::sync::mpsc::channel();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let state = ServerState {
        ranges: Arc::new(Mutex::new(Vec::new())),
        once_hits: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };
    let server_ranges = Arc::clone(&state.ranges);
    let server_once_hits = Arc::clone(&state.once_hits);

    let join = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind test server");
            let addr = listener.local_addr().expect("test server address");
            addr_tx.send(addr).expect("send test server address");
            axum::serve(listener, app(state))
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .expect("serve test server");
        });
    });

    let addr: SocketAddr = addr_rx.recv().expect("test server address");
    TestServer {
        base: format!("http://{addr}"),
        ranges: server_ranges,
        once_hits: server_once_hits,
        shutdown: Some(shutdown_tx),
        join: Some(join),
    }
}

fn app(state: ServerState) -> Router {
    Router::new()
        .route("/file.bin", get(file))
        .route("/slow.bin", get(slow))
        .route("/slow-headers.bin", get(slow_headers))
        .route("/seg.bin", get(seg_bin))
        .route("/range0-only.bin", get(range0_only))
        .route("/slow-seg.bin", get(slow_seg))
        .route("/slow-probe.bin", get(slow_probe))
        .route("/auth.bin", get(auth_bin))
        .route("/chunked.bin", get(chunked_bin))
        .route("/once.bin", get(once_bin))
        .route("/missing", get(|| async { StatusCode::NOT_FOUND }))
        .with_state(state)
}

async fn once_bin(State(state): State<ServerState>) -> Response {
    let hit = state
        .once_hits
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    if hit > 0 {
        let mut gone = Response::new(Body::empty());
        *gone.status_mut() = StatusCode::GONE;
        return gone;
    }
    let body = b"one-time-payload-bytes-0123456789".repeat(128);
    let mut response = Response::new(Body::from(body));
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"once.bin\""),
    );
    response
}

async fn slow() -> Response {
    let body = b"LajuFile".repeat(1024 * 128);
    let total = body.len();
    let half = total / 2;
    let first = Bytes::copy_from_slice(&body[..half]);
    let second = Bytes::copy_from_slice(&body[half..]);
    let stream = tokio_stream::iter([(50_u64, first), (450_u64, second)]).then(
        |(delay_ms, chunk)| async move {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            Ok::<Bytes, std::io::Error>(chunk)
        },
    );
    let mut response = Response::new(Body::from_stream(stream));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&total.to_string()).unwrap(),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"file.bin\""),
    );
    response
}

async fn slow_headers() -> impl axum::response::IntoResponse {
    tokio::time::sleep(Duration::from_millis(300)).await;
    file().await
}

async fn slow_probe() -> impl axum::response::IntoResponse {
    tokio::time::sleep(Duration::from_secs(4)).await;
    file().await
}

async fn auth_bin(headers: HeaderMap) -> Response {
    let authorized = headers.get(header::COOKIE).and_then(|v| v.to_str().ok())
        == Some("session=abc")
        && headers.get(header::REFERER).and_then(|v| v.to_str().ok())
            == Some("https://example.test/page")
        && headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            == Some("LajuFileTest/1.0");
    if !authorized {
        return Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .body(Body::empty())
            .unwrap();
    }
    let mut response = Response::new(Body::from(b"LajuFile".repeat(1024 * 128)));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    response
}

async fn chunked_bin() -> Response {
    let chunk = Bytes::from(b"LajuFile".repeat(1024));
    let stream = tokio_stream::iter([
        Ok::<Bytes, std::io::Error>(chunk.clone()),
        Ok::<Bytes, std::io::Error>(chunk),
    ]);
    let mut response = Response::new(Body::from_stream(stream));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    response
}

async fn file() -> impl axum::response::IntoResponse {
    let body = b"LajuFile".repeat(1024 * 128);
    (
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"file.bin\"",
            ),
        ],
        Body::from(body),
    )
}

async fn seg_bin(State(state): State<ServerState>, headers: HeaderMap) -> Response {
    let body = seg_payload();
    let Some((start, end)) = parse_range(&headers) else {
        return full_response(&body, "seg.bin");
    };
    state.ranges.lock().unwrap().push(start as u64);
    range_response(&body, start, end, "seg.bin")
}

async fn range0_only(headers: HeaderMap) -> Response {
    let body = seg_payload();
    match parse_range(&headers) {
        Some((0, 0)) => range_response(&body, 0, 0, "range0-only.bin"),
        _ => full_response(&body, "range0-only.bin"),
    }
}

async fn slow_seg(State(state): State<ServerState>, headers: HeaderMap) -> Response {
    let body = seg_payload();
    let Some((start, end)) = parse_range(&headers) else {
        return full_response(&body, "slow-seg.bin");
    };
    state.ranges.lock().unwrap().push(start as u64);
    if start == 0 && end == 0 {
        return range_response(&body, 0, 0, "slow-seg.bin");
    }
    slow_range_response(body, start, end, "slow-seg.bin")
}

pub fn seg_payload() -> Vec<u8> {
    (0..SEG_SIZE).map(|index| (index % 251) as u8).collect()
}

fn parse_range(headers: &HeaderMap) -> Option<(usize, usize)> {
    let value = headers.get(header::RANGE)?.to_str().ok()?;
    let value = value.strip_prefix("bytes=")?;
    let (start, end) = value.split_once('-')?;
    let start = start.parse().ok()?;
    let end = end.parse().ok()?;
    (start <= end).then_some((start, end))
}

fn full_response(body: &[u8], file_name: &str) -> Response {
    let mut response = Response::new(Body::from(body.to_vec()));
    add_common_headers(&mut response, file_name);
    response
}

fn range_response(body: &[u8], start: usize, end: usize, file_name: &str) -> Response {
    let slice = body.get(start..=end).unwrap_or_default().to_vec();
    let mut response = Response::new(Body::from(slice));
    *response.status_mut() = StatusCode::PARTIAL_CONTENT;
    add_common_headers(&mut response, file_name);
    response.headers_mut().insert(
        header::CONTENT_RANGE,
        HeaderValue::from_str(&format!("bytes {start}-{end}/{SEG_SIZE}")).unwrap(),
    );
    response
}

fn slow_range_response(body: Vec<u8>, start: usize, end: usize, file_name: &str) -> Response {
    let slice = body.get(start..=end).unwrap_or_default().to_vec();
    let chunks: Vec<Bytes> = slice.chunks(CHUNK).map(Bytes::copy_from_slice).collect();
    let stream = tokio_stream::iter(chunks).then(|chunk| async move {
        tokio::time::sleep(Duration::from_millis(40)).await;
        Ok::<Bytes, std::io::Error>(chunk)
    });
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = StatusCode::PARTIAL_CONTENT;
    add_common_headers(&mut response, file_name);
    response.headers_mut().insert(
        header::CONTENT_RANGE,
        HeaderValue::from_str(&format!("bytes {start}-{end}/{SEG_SIZE}")).unwrap(),
    );
    response
}

fn add_common_headers(response: &mut Response, file_name: &str) {
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(header::ETAG, HeaderValue::from_static("\"seg-v1\""));
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{file_name}\"")).unwrap(),
    );
}
