use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use lajufile_core::download::{DownloadError, DownloadSpec, download_single};
use lajufile_core::http::{RequestOptions, build_client};
use lajufile_core::naming::temp_path_for;
use url::Url;

mod support;

fn client() -> reqwest::Client {
    build_client(Duration::from_secs(5), Duration::from_secs(60)).unwrap()
}

#[tokio::test]
async fn downloads_file_and_removes_temp() {
    let base = support::start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("ubuntu.iso");
    let temp_path = temp_path_for(&final_path);
    let spec = DownloadSpec {
        url: Url::parse(&format!("{base}/plain")).unwrap(),
        temp_path: temp_path.clone(),
        final_path: final_path.clone(),
        expected_size: Some(support::SIZE as u64),
        options: RequestOptions::default(),
    };
    let last = Arc::new(AtomicU64::new(0));
    let last2 = last.clone();

    let out = download_single(&client(), &spec, move |n| {
        last2.store(n, Ordering::SeqCst);
    })
    .await
    .unwrap();

    assert_eq!(out.final_path, final_path);
    assert_eq!(out.bytes, support::SIZE as u64);
    assert_eq!(std::fs::read(&final_path).unwrap(), support::payload());
    assert!(!temp_path.exists());
    assert_eq!(last.load(Ordering::SeqCst), support::SIZE as u64);
}

#[tokio::test]
async fn refuses_existing_destination() {
    let base = support::start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("ubuntu.iso");
    let temp_path = temp_path_for(&final_path);
    std::fs::write(&final_path, b"existing").unwrap();
    let spec = DownloadSpec {
        url: Url::parse(&format!("{base}/plain")).unwrap(),
        temp_path: temp_path.clone(),
        final_path: final_path.clone(),
        expected_size: Some(support::SIZE as u64),
        options: RequestOptions::default(),
    };

    let err = download_single(&client(), &spec, |_| {}).await.unwrap_err();

    assert!(matches!(err, DownloadError::DestinationExists(ref p) if p == &final_path));
    assert_eq!(std::fs::read(&final_path).unwrap(), b"existing");
    assert!(!temp_path.exists());
}

#[tokio::test]
async fn size_mismatch_leaves_temp_and_no_final() {
    let base = support::start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("ubuntu.iso");
    let temp_path = temp_path_for(&final_path);
    let expected = support::SIZE as u64 + 1;
    let spec = DownloadSpec {
        url: Url::parse(&format!("{base}/plain")).unwrap(),
        temp_path: temp_path.clone(),
        final_path: final_path.clone(),
        expected_size: Some(expected),
        options: RequestOptions::default(),
    };

    let err = download_single(&client(), &spec, |_| {}).await.unwrap_err();

    match err {
        DownloadError::SizeMismatch {
            expected: got_expected,
            actual,
        } => {
            assert_eq!(got_expected, expected);
            assert_eq!(actual, support::SIZE as u64);
        }
        other => panic!("expected SizeMismatch, got {other:?}"),
    }
    assert!(temp_path.exists());
    assert!(!final_path.exists());
}

#[tokio::test]
async fn zero_byte_download_creates_empty_file() {
    let base = support::start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("empty.bin");
    let temp_path = temp_path_for(&final_path);
    let spec = DownloadSpec {
        url: Url::parse(&format!("{base}/empty")).unwrap(),
        temp_path: temp_path.clone(),
        final_path: final_path.clone(),
        expected_size: Some(0),
        options: RequestOptions::default(),
    };

    let out = download_single(&client(), &spec, |_| {}).await.unwrap();

    assert_eq!(out.bytes, 0);
    assert_eq!(std::fs::read(&final_path).unwrap(), Vec::<u8>::new());
    assert!(!temp_path.exists());
}

#[tokio::test]
async fn rejects_encoded_response() {
    let base = support::start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("encoded.txt");
    let temp_path = temp_path_for(&final_path);
    let spec = DownloadSpec {
        url: Url::parse(&format!("{base}/encoded")).unwrap(),
        temp_path: temp_path.clone(),
        final_path: final_path.clone(),
        expected_size: None,
        options: RequestOptions::default(),
    };

    let err = download_single(&client(), &spec, |_| {}).await.unwrap_err();

    assert!(matches!(err, DownloadError::UnsupportedEncoding(ref e) if e == "gzip"));
    assert!(!final_path.exists());
    assert!(temp_path.exists());
}

#[tokio::test]
async fn missing_url_maps_to_status_error() {
    let base = support::start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("missing.bin");
    let temp_path = temp_path_for(&final_path);
    let spec = DownloadSpec {
        url: Url::parse(&format!("{base}/missing")).unwrap(),
        temp_path,
        final_path: final_path.clone(),
        expected_size: None,
        options: RequestOptions::default(),
    };

    let err = download_single(&client(), &spec, |_| {}).await.unwrap_err();

    assert!(matches!(err, DownloadError::Status(404)));
    assert!(!final_path.exists());
}

#[tokio::test]
async fn retry_truncates_stale_temp() {
    let base = support::start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("ubuntu.iso");
    let temp_path = temp_path_for(&final_path);
    std::fs::write(&temp_path, vec![0xAA; 4096]).unwrap();
    let spec = DownloadSpec {
        url: Url::parse(&format!("{base}/plain")).unwrap(),
        temp_path: temp_path.clone(),
        final_path: final_path.clone(),
        expected_size: Some(support::SIZE as u64),
        options: RequestOptions::default(),
    };

    let out = download_single(&client(), &spec, |_| {}).await.unwrap();

    assert_eq!(out.bytes, support::SIZE as u64);
    assert_eq!(std::fs::read(&final_path).unwrap(), support::payload());
    assert!(!temp_path.exists());
}

#[tokio::test]
async fn rejects_multiple_content_encodings() {
    let base = support::start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("encoded.txt");
    let temp_path = temp_path_for(&final_path);
    let spec = DownloadSpec {
        url: Url::parse(&format!("{base}/encoded-multi")).unwrap(),
        temp_path: temp_path.clone(),
        final_path: final_path.clone(),
        expected_size: None,
        options: RequestOptions::default(),
    };

    let err = download_single(&client(), &spec, |_| {}).await.unwrap_err();

    assert!(matches!(err, DownloadError::UnsupportedEncoding(ref e) if e == "gzip"));
    assert!(!final_path.exists());
    assert!(temp_path.exists());
}

#[tokio::test]
async fn rejects_partial_response_with_partial_range() {
    let base = support::start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("partial.bin");
    let temp_path = temp_path_for(&final_path);
    let spec = DownloadSpec {
        url: Url::parse(&format!("{base}/partial")).unwrap(),
        temp_path: temp_path.clone(),
        final_path: final_path.clone(),
        expected_size: None,
        options: RequestOptions::default(),
    };

    let err = download_single(&client(), &spec, |_| {}).await.unwrap_err();

    assert!(matches!(err, DownloadError::ContentRange(ref raw) if raw.contains("0-0/1048576")));
    assert!(!final_path.exists());
    assert!(temp_path.exists());
}

#[tokio::test]
async fn rejects_partial_response_with_unknown_total() {
    let base = support::start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("partial.bin");
    let temp_path = temp_path_for(&final_path);
    let spec = DownloadSpec {
        url: Url::parse(&format!("{base}/partial-unknown")).unwrap(),
        temp_path: temp_path.clone(),
        final_path: final_path.clone(),
        expected_size: None,
        options: RequestOptions::default(),
    };

    let err = download_single(&client(), &spec, |_| {}).await.unwrap_err();

    assert!(matches!(err, DownloadError::ContentRange(ref raw) if raw.contains("0-0/*")));
    assert!(!final_path.exists());
    assert!(temp_path.exists());
}

#[tokio::test]
async fn accepts_full_range_partial_response() {
    let base = support::start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("full.bin");
    let temp_path = temp_path_for(&final_path);
    let spec = DownloadSpec {
        url: Url::parse(&format!("{base}/partial-full")).unwrap(),
        temp_path: temp_path.clone(),
        final_path: final_path.clone(),
        expected_size: Some(support::SIZE as u64),
        options: RequestOptions::default(),
    };

    let out = download_single(&client(), &spec, |_| {}).await.unwrap();

    assert_eq!(out.bytes, support::SIZE as u64);
    assert_eq!(std::fs::read(&final_path).unwrap(), support::payload());
    assert!(!temp_path.exists());
}

#[tokio::test]
async fn short_stream_is_size_mismatch() {
    let base = support::start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("short.bin");
    let temp_path = temp_path_for(&final_path);
    let spec = DownloadSpec {
        url: Url::parse(&format!("{base}/short")).unwrap(),
        temp_path: temp_path.clone(),
        final_path: final_path.clone(),
        expected_size: Some(support::SIZE as u64),
        options: RequestOptions::default(),
    };

    let err = download_single(&client(), &spec, |_| {}).await.unwrap_err();

    match err {
        DownloadError::SizeMismatch { expected, actual } => {
            assert_eq!(expected, support::SIZE as u64);
            assert_eq!(actual, (support::SIZE / 2) as u64);
        }
        other => panic!("expected SizeMismatch, got {other:?}"),
    }
    assert!(temp_path.exists());
    assert!(!final_path.exists());
}

#[tokio::test]
async fn long_stream_is_size_mismatch() {
    let base = support::start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("long.bin");
    let temp_path = temp_path_for(&final_path);
    let spec = DownloadSpec {
        url: Url::parse(&format!("{base}/long")).unwrap(),
        temp_path: temp_path.clone(),
        final_path: final_path.clone(),
        expected_size: Some(support::SIZE as u64),
        options: RequestOptions::default(),
    };

    let err = download_single(&client(), &spec, |_| {}).await.unwrap_err();

    match err {
        DownloadError::SizeMismatch { expected, actual } => {
            assert_eq!(expected, support::SIZE as u64);
            assert_eq!(actual, support::SIZE as u64 + 1);
        }
        other => panic!("expected SizeMismatch, got {other:?}"),
    }
    assert!(temp_path.exists());
    assert!(!final_path.exists());
}
