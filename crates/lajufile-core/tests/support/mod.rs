use std::time::Duration;

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
    routing::get,
};
use tokio::net::TcpListener;

pub const SIZE: usize = 1_048_576;

pub fn payload() -> Vec<u8> {
    (0..SIZE).map(|i| (i % 251) as u8).collect()
}

pub async fn start_server() -> String {
    let app = Router::new()
        .route("/range", get(range))
        .route("/plain", get(plain))
        .route("/empty", get(empty))
        .route("/missing", get(missing))
        .route("/redirect", get(redirect))
        .route("/encoded", get(encoded))
        .route("/slow", get(slow))
        .route("/range-nolen", get(range_nolen))
        .route("/empty-probe", get(empty_probe));

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
