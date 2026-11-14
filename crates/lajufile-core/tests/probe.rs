use std::time::Duration;

use lajufile_core::http::{RequestOptions, build_client};
use lajufile_core::probe::{ProbeError, ProbeRequest, probe};
use url::Url;

mod support;

fn client() -> reqwest::Client {
    build_client(Duration::from_secs(5), Duration::from_secs(60)).unwrap()
}

fn request(base: &str, path: &str, timeout: Duration) -> ProbeRequest {
    ProbeRequest {
        url: Url::parse(&format!("{base}{path}")).unwrap(),
        options: RequestOptions::default(),
        timeout,
    }
}

#[tokio::test]
async fn probe_206_returns_size_and_metadata() {
    let base = support::start_server().await;
    let req = request(&base, "/range", Duration::from_secs(2));

    let result = probe(&client(), &req).await.unwrap();

    assert_eq!(result.size, Some(1_048_576));
    assert!(result.ranged);
    assert_eq!(result.etag.as_deref(), Some("\"v1\""));
    assert!(result.content_disposition.unwrap().contains("ubuntu"));
    assert!(result.final_url.as_str().ends_with("/range"));
}

#[tokio::test]
async fn probe_200_without_range_sets_ranged_false_and_size() {
    let base = support::start_server().await;
    let req = request(&base, "/plain", Duration::from_secs(2));

    let result = probe(&client(), &req).await.unwrap();

    assert_eq!(result.size, Some(1_048_576));
    assert!(!result.ranged);
}

#[tokio::test]
async fn probe_404_maps_to_status_error() {
    let base = support::start_server().await;
    let req = request(&base, "/missing", Duration::from_secs(2));

    let error = probe(&client(), &req).await.unwrap_err();

    assert!(matches!(error, ProbeError::Status(404)));
}

#[tokio::test]
async fn probe_follows_redirect_and_returns_final_url() {
    let base = support::start_server().await;
    let req = request(&base, "/redirect", Duration::from_secs(2));

    let result = probe(&client(), &req).await.unwrap();

    assert!(result.final_url.as_str().ends_with("/range"));
    assert_eq!(result.size, Some(1_048_576));
}

#[tokio::test]
async fn probe_416_with_zero_total_is_empty_file() {
    let base = support::start_server().await;
    let req = request(&base, "/empty-probe", Duration::from_secs(2));

    let result = probe(&client(), &req).await.unwrap();

    assert_eq!(result.size, Some(0));
    assert!(result.ranged);
}

#[tokio::test]
async fn probe_rejects_encoded_response() {
    let base = support::start_server().await;
    let req = request(&base, "/encoded", Duration::from_secs(2));

    let error = probe(&client(), &req).await.unwrap_err();

    assert!(matches!(
        error,
        ProbeError::UnsupportedEncoding(value) if value == "gzip"
    ));
}

#[tokio::test]
async fn probe_times_out() {
    let base = support::start_server().await;
    let req = request(&base, "/slow", Duration::from_millis(50));

    let error = probe(&client(), &req).await.unwrap_err();

    assert!(matches!(
        error,
        ProbeError::Request(error) if error.is_timeout()
    ));
}

#[tokio::test]
async fn probe_handles_unknown_total() {
    let base = support::start_server().await;
    let req = request(&base, "/range-nolen", Duration::from_secs(2));

    let result = probe(&client(), &req).await.unwrap();

    assert_eq!(result.size, None);
    assert!(result.ranged);
}

#[tokio::test]
async fn probe_rejects_unsupported_scheme() {
    let req = ProbeRequest {
        url: Url::parse("file:///tmp/file.iso").unwrap(),
        options: RequestOptions::default(),
        timeout: Duration::from_secs(2),
    };

    let error = probe(&client(), &req).await.unwrap_err();

    assert!(matches!(
        error,
        ProbeError::UnsupportedScheme(scheme) if scheme == "file"
    ));
}

#[tokio::test]
async fn probe_rejects_multiple_content_encodings() {
    let base = support::start_server().await;
    let req = request(&base, "/encoded-multi", Duration::from_secs(2));

    let error = probe(&client(), &req).await.unwrap_err();

    assert!(matches!(
        error,
        ProbeError::UnsupportedEncoding(value) if value == "gzip"
    ));
}
