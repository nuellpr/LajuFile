use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
    routing::get,
};
use tokio::net::TcpListener;

pub const SIZE: usize = 1_048_576;
pub const SEG_SIZE: usize = 4_194_304;

pub fn payload() -> Vec<u8> {
    (0..SIZE).map(|i| (i % 251) as u8).collect()
}

pub fn seg_payload() -> Vec<u8> {
    (0..SEG_SIZE).map(|i| (i % 251) as u8).collect()
}

#[derive(Clone)]
struct SegState {
    flaky_failed: Arc<AtomicBool>,
}

pub async fn start_server() -> String {
    let app = Router::new()
        .route("/range", get(range))
        .route("/plain", get(plain))
        .route("/empty", get(empty))
        .route("/missing", get(missing))
        .route("/redirect", get(redirect))
        .route("/encoded", get(encoded))
        .route("/encoded-multi", get(encoded_multi))
        .route("/slow", get(slow))
        .route("/range-nolen", get(range_nolen))
        .route("/empty-probe", get(empty_probe))
        .route("/partial", get(partial))
        .route("/partial-unknown", get(partial_unknown))
        .route("/partial-full", get(partial_full))
        .route("/short", get(short))
        .route("/long", get(long))
        .route("/seg", get(seg))
        .route("/seg-ignore", get(seg_ignore))
        .route("/seg-bad-range", get(seg_bad_range))
        .route("/seg-flaky", get(seg_flaky))
        .route("/seg-always-500", get(seg_always_500))
        .with_state(SegState {
            flaky_failed: Arc::new(AtomicBool::new(false)),
        });

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

fn response(
    status: StatusCode,
    body: Body,
    headers: &[(header::HeaderName, &'static str)],
) -> Response {
    let mut response = Response::new(body);
    *response.status_mut() = status;
    for (name, value) in headers {
        response
            .headers_mut()
            .insert(name.clone(), HeaderValue::from_static(value));
    }
    response
}

fn chunked(data: Vec<u8>) -> Body {
    Body::from_stream(tokio_stream::iter(vec![Ok::<Bytes, std::io::Error>(
        Bytes::from(data),
    )]))
}

async fn range(headers: HeaderMap) -> Response {
    if headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|value| value.to_str().ok())
        != Some("identity")
    {
        return response(StatusCode::BAD_REQUEST, Body::empty(), &[]);
    }

    let ranged = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        == Some("bytes=0-0");

    if ranged {
        let body = Body::from(payload()[..1].to_vec());
        response(
            StatusCode::PARTIAL_CONTENT,
            body,
            &[
                (header::CONTENT_RANGE, "bytes 0-0/1048576"),
                (header::CONTENT_LENGTH, "1"),
                (header::ETAG, "\"v1\""),
                (header::LAST_MODIFIED, "Wed, 21 Oct 2015 07:28:00 GMT"),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename*=UTF-8''ubuntu%20test.iso",
                ),
            ],
        )
    } else {
        response(
            StatusCode::OK,
            Body::from(payload()),
            &[
                (header::CONTENT_LENGTH, "1048576"),
                (header::ETAG, "\"v1\""),
            ],
        )
    }
}

async fn plain() -> Response {
    response(
        StatusCode::OK,
        Body::from(payload()),
        &[(header::CONTENT_LENGTH, "1048576")],
    )
}

async fn empty() -> Response {
    response(
        StatusCode::OK,
        Body::empty(),
        &[(header::CONTENT_LENGTH, "0")],
    )
}

async fn missing() -> Response {
    response(StatusCode::NOT_FOUND, Body::empty(), &[])
}

async fn redirect() -> Response {
    response(
        StatusCode::TEMPORARY_REDIRECT,
        Body::empty(),
        &[(header::LOCATION, "/range")],
    )
}

async fn encoded() -> Response {
    response(
        StatusCode::OK,
        Body::from("fake"),
        &[
            (header::CONTENT_ENCODING, "gzip"),
            (header::CONTENT_LENGTH, "4"),
        ],
    )
}

async fn encoded_multi() -> Response {
    let mut response = response(
        StatusCode::OK,
        Body::from("fake"),
        &[(header::CONTENT_LENGTH, "4")],
    );
    response.headers_mut().append(
        header::CONTENT_ENCODING,
        HeaderValue::from_static("identity"),
    );
    response
        .headers_mut()
        .append(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
    response
}

async fn slow() -> Response {
    tokio::time::sleep(Duration::from_millis(200)).await;
    response(
        StatusCode::OK,
        Body::from(payload()),
        &[(header::CONTENT_LENGTH, "1048576")],
    )
}

async fn range_nolen() -> Response {
    response(
        StatusCode::PARTIAL_CONTENT,
        Body::from(payload()[..1].to_vec()),
        &[
            (header::CONTENT_RANGE, "bytes 0-0/*"),
            (header::CONTENT_LENGTH, "1"),
        ],
    )
}

async fn empty_probe() -> Response {
    response(
        StatusCode::RANGE_NOT_SATISFIABLE,
        Body::empty(),
        &[(header::CONTENT_RANGE, "bytes */0")],
    )
}

async fn partial() -> Response {
    response(
        StatusCode::PARTIAL_CONTENT,
        Body::from(payload()[..1].to_vec()),
        &[
            (header::CONTENT_RANGE, "bytes 0-0/1048576"),
            (header::CONTENT_LENGTH, "1"),
        ],
    )
}

async fn partial_unknown() -> Response {
    response(
        StatusCode::PARTIAL_CONTENT,
        Body::from(payload()[..1].to_vec()),
        &[
            (header::CONTENT_RANGE, "bytes 0-0/*"),
            (header::CONTENT_LENGTH, "1"),
        ],
    )
}

async fn partial_full() -> Response {
    response(
        StatusCode::PARTIAL_CONTENT,
        Body::from(payload()),
        &[
            (header::CONTENT_RANGE, "bytes 0-1048575/1048576"),
            (header::CONTENT_LENGTH, "1048576"),
        ],
    )
}

async fn short() -> Response {
    response(StatusCode::OK, chunked(payload()[..SIZE / 2].to_vec()), &[])
}

async fn long() -> Response {
    let mut data = payload();
    data.push(0);
    response(StatusCode::OK, chunked(data), &[])
}

fn response_strings(
    status: StatusCode,
    body: Body,
    headers: &[(header::HeaderName, String)],
) -> Response {
    let mut response = Response::new(body);
    *response.status_mut() = status;
    for (name, value) in headers {
        response.headers_mut().insert(
            name.clone(),
            HeaderValue::from_str(value).expect("valid response header"),
        );
    }
    response
}

fn parse_range(headers: &HeaderMap) -> Option<(usize, usize)> {
    let value = headers.get(header::RANGE)?.to_str().ok()?;
    let (start, end) = value.strip_prefix("bytes=")?.split_once('-')?;
    Some((start.parse().ok()?, end.parse().ok()?))
}

fn seg_full() -> Response {
    response(
        StatusCode::OK,
        Body::from(seg_payload()),
        &[
            (header::CONTENT_LENGTH, "4194304"),
            (header::ETAG, "\"seg-v1\""),
        ],
    )
}

fn seg_range(headers: &HeaderMap, start: usize, end: usize) -> Response {
    if let Some(value) = headers.get(header::IF_RANGE)
        && value.to_str().ok() != Some("\"seg-v1\"")
    {
        return seg_full();
    }
    let body = seg_payload()[start..=end].to_vec();
    response_strings(
        StatusCode::PARTIAL_CONTENT,
        Body::from(body),
        &[
            (
                header::CONTENT_RANGE,
                format!("bytes {start}-{end}/{SEG_SIZE}"),
            ),
            (header::CONTENT_LENGTH, (end - start + 1).to_string()),
            (header::ETAG, "\"seg-v1\"".to_string()),
        ],
    )
}

async fn seg(headers: HeaderMap) -> Response {
    match parse_range(&headers) {
        Some((start, end)) => seg_range(&headers, start, end),
        None => seg_full(),
    }
}

async fn seg_ignore() -> Response {
    seg_full()
}

async fn seg_bad_range(headers: HeaderMap) -> Response {
    let Some((start, end)) = parse_range(&headers) else {
        return seg_full();
    };
    let shifted_start = start + 1;
    let body = if shifted_start <= end {
        seg_payload()[shifted_start..=end].to_vec()
    } else {
        Vec::new()
    };
    response_strings(
        StatusCode::PARTIAL_CONTENT,
        Body::from(body),
        &[
            (
                header::CONTENT_RANGE,
                format!("bytes {shifted_start}-{end}/{SEG_SIZE}"),
            ),
            (header::CONTENT_LENGTH, (end - start).to_string()),
        ],
    )
}

async fn seg_flaky(State(state): State<SegState>, headers: HeaderMap) -> Response {
    let Some((start, end)) = parse_range(&headers) else {
        return seg_full();
    };
    if start == SEG_SIZE / 4 && !state.flaky_failed.swap(true, Ordering::SeqCst) {
        return response(StatusCode::INTERNAL_SERVER_ERROR, Body::empty(), &[]);
    }
    seg_range(&headers, start, end)
}

async fn seg_always_500() -> Response {
    response(StatusCode::INTERNAL_SERVER_ERROR, Body::empty(), &[])
}
