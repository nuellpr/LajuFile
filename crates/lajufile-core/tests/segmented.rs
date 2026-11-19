use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lajufile_core::http::{RequestOptions, build_client};
use lajufile_core::segment::plan_segments;
use lajufile_core::segmented::{SegmentedError, SegmentedSpec, download_segmented, prefix_matches};
use lajufile_core::speed::RateLimiter;
use lajufile_core::store::Segment;
use url::Url;

mod support;

use support::{SEG_SIZE, seg_payload, start_server, start_server_with_ranges};

fn spec(
    base: &str,
    path: &str,
    temp_path: PathBuf,
    final_path: PathBuf,
    segments: Vec<Segment>,
    retry_delays: Vec<Duration>,
    if_range: Option<String>,
) -> SegmentedSpec {
    SegmentedSpec {
        url: Url::parse(&format!("{base}{path}")).unwrap(),
        temp_path,
        final_path,
        size: SEG_SIZE as u64,
        segments,
        options: RequestOptions::default(),
        if_range,
        limiter: Arc::new(RateLimiter::new(0)),
        retry_delays,
    }
}

fn segments() -> Vec<Segment> {
    plan_segments(SEG_SIZE as u64, true, 4)
}

fn client() -> reqwest::Client {
    build_client(Duration::from_secs(5), Duration::from_secs(5)).unwrap()
}

#[tokio::test]
async fn downloads_all_segments_and_finalizes() {
    let base = start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let temp = dir.path().join("file.bin.lajufile");
    let final_path = dir.path().join("file.bin");
    let spec = spec(
        &base,
        "/seg",
        temp.clone(),
        final_path.clone(),
        segments(),
        Vec::new(),
        None,
    );
    let progress = Arc::new(Mutex::new(0_u64));
    let progress_writer = progress.clone();

    let outcome = download_segmented(&client(), &spec, move |_, _, total| {
        *progress_writer.lock().unwrap() = total;
    })
    .await
    .unwrap();

    assert_eq!(std::fs::read(&final_path).unwrap(), seg_payload());
    assert!(!temp.exists());
    assert_eq!(outcome.bytes, SEG_SIZE as u64);
    assert_eq!(*progress.lock().unwrap(), SEG_SIZE as u64);
}

#[tokio::test]
async fn rejects_range_ignored_response() {
    let base = start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let temp = dir.path().join("file.bin.lajufile");
    let final_path = dir.path().join("file.bin");
    let spec = spec(
        &base,
        "/seg-ignore",
        temp.clone(),
        final_path.clone(),
        segments(),
        Vec::new(),
        None,
    );

    let error = download_segmented(&client(), &spec, |_, _, _| {})
        .await
        .unwrap_err();

    assert!(matches!(error, SegmentedError::RangeIgnored));
    assert!(!final_path.exists());
    assert!(temp.exists());
}

#[tokio::test]
async fn rejects_invalid_content_range() {
    let base = start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let temp = dir.path().join("file.bin.lajufile");
    let final_path = dir.path().join("file.bin");
    let spec = spec(
        &base,
        "/seg-bad-range",
        temp,
        final_path.clone(),
        segments(),
        Vec::new(),
        None,
    );

    let error = download_segmented(&client(), &spec, |_, _, _| {})
        .await
        .unwrap_err();

    assert!(matches!(error, SegmentedError::Download(_)));
    assert!(!final_path.exists());
}

#[tokio::test]
async fn retries_segment_and_succeeds() {
    let base = start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let temp = dir.path().join("file.bin.lajufile");
    let final_path = dir.path().join("file.bin");
    let spec = spec(
        &base,
        "/seg-flaky",
        temp,
        final_path.clone(),
        segments(),
        vec![Duration::ZERO],
        None,
    );

    download_segmented(&client(), &spec, |_, _, _| {})
        .await
        .unwrap();

    assert_eq!(std::fs::read(&final_path).unwrap(), seg_payload());
}

#[tokio::test]
async fn retry_exhaustion_leaves_temp_and_no_final() {
    let base = start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let temp = dir.path().join("file.bin.lajufile");
    let final_path = dir.path().join("file.bin");
    let spec = spec(
        &base,
        "/seg-always-500",
        temp.clone(),
        final_path.clone(),
        segments(),
        vec![Duration::ZERO, Duration::ZERO],
        None,
    );

    let error = download_segmented(&client(), &spec, |_, _, _| {})
        .await
        .unwrap_err();

    assert!(matches!(error, SegmentedError::Download(_)));
    assert!(temp.exists());
    assert!(!final_path.exists());
}

#[tokio::test]
async fn refuses_existing_final() {
    let base = start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let temp = dir.path().join("file.bin.lajufile");
    let final_path = dir.path().join("file.bin");
    std::fs::write(&final_path, b"keep").unwrap();
    let spec = spec(
        &base,
        "/seg",
        temp,
        final_path.clone(),
        segments(),
        Vec::new(),
        None,
    );

    let error = download_segmented(&client(), &spec, |_, _, _| {})
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        SegmentedError::Download(lajufile_core::download::DownloadError::DestinationExists(ref path))
            if path == &final_path
    ));
    assert_eq!(std::fs::read(&final_path).unwrap(), b"keep");
}

#[tokio::test]
async fn resume_uses_done_offsets() {
    let (base, ranges) = start_server_with_ranges().await;
    let dir = tempfile::tempdir().unwrap();
    let temp = dir.path().join("file.bin.lajufile");
    let final_path = dir.path().join("file.bin");
    std::fs::write(&temp, &seg_payload()[..512 * 1024]).unwrap();
    let mut segments = segments();
    segments[0].done = 512 * 1024;
    let spec = spec(
        &base,
        "/seg",
        temp,
        final_path.clone(),
        segments,
        Vec::new(),
        None,
    );

    download_segmented(&client(), &spec, |_, _, _| {})
        .await
        .unwrap();

    assert_eq!(std::fs::read(&final_path).unwrap(), seg_payload());
    let starts = ranges.lock().unwrap().clone();
    for expected in [524_288, 1_048_576, 2_097_152, 3_145_728] {
        assert!(
            starts.contains(&expected),
            "missing resumed range start {expected}: {starts:?}"
        );
    }
}

#[tokio::test]
async fn if_range_match_resumes_from_done_offset() {
    let (base, ranges) = start_server_with_ranges().await;
    let dir = tempfile::tempdir().unwrap();
    let temp = dir.path().join("file.bin.lajufile");
    let final_path = dir.path().join("file.bin");
    std::fs::write(&temp, &seg_payload()[..512 * 1024]).unwrap();
    let mut segments = segments();
    segments[0].done = 512 * 1024;
    let spec = spec(
        &base,
        "/seg",
        temp,
        final_path.clone(),
        segments,
        Vec::new(),
        Some("\"seg-v1\"".to_string()),
    );

    download_segmented(&client(), &spec, |_, _, _| {})
        .await
        .unwrap();

    assert_eq!(std::fs::read(&final_path).unwrap(), seg_payload());
    let starts = ranges.lock().unwrap().clone();
    assert!(
        starts.contains(&524_288),
        "resume did not request the saved offset: {starts:?}"
    );
}

#[tokio::test]
async fn prefix_mismatch_returns_restart() {
    let base = start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let temp = dir.path().join("file.bin.lajufile");
    let final_path = dir.path().join("file.bin");
    std::fs::write(&temp, vec![0_u8; 512 * 1024]).unwrap();
    let mut segments = segments();
    segments[0].done = 512 * 1024;
    let spec = spec(
        &base,
        "/seg",
        temp.clone(),
        final_path.clone(),
        segments,
        Vec::new(),
        None,
    );

    let error = download_segmented(&client(), &spec, |_, _, _| {})
        .await
        .unwrap_err();

    assert!(matches!(error, SegmentedError::RestartRequired));
    assert!(!final_path.exists());
    assert!(temp.exists());
}

#[tokio::test]
async fn if_range_mismatch_returns_range_ignored() {
    let base = start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let temp = dir.path().join("file.bin.lajufile");
    let final_path = dir.path().join("file.bin");
    std::fs::write(&temp, &seg_payload()[..512 * 1024]).unwrap();
    let mut segments = segments();
    segments[0].done = 512 * 1024;
    let spec = spec(
        &base,
        "/seg",
        temp,
        final_path.clone(),
        segments,
        Vec::new(),
        Some("\"old\"".to_string()),
    );

    let error = download_segmented(&client(), &spec, |_, _, _| {})
        .await
        .unwrap_err();

    assert!(matches!(error, SegmentedError::RangeIgnored));
    assert!(!final_path.exists());
}

#[tokio::test]
async fn prefix_matches_accepts_matching_and_rejects_plain_200() {
    let base = start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let prefix = dir.path().join("prefix.bin");
    std::fs::write(&prefix, &seg_payload()[..64 * 1024]).unwrap();
    let options = RequestOptions::default();

    let matching = Url::parse(&format!("{base}/seg")).unwrap();
    assert!(prefix_matches(&client(), &matching, &prefix, SEG_SIZE as u64, &options).await);

    let ignored = Url::parse(&format!("{base}/seg-ignore")).unwrap();
    assert!(!prefix_matches(&client(), &ignored, &prefix, SEG_SIZE as u64, &options).await);
}

#[tokio::test]
async fn permanent_http_status_does_not_sleep_on_retries() {
    let base = start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let spec = spec(
        &base,
        "/missing",
        dir.path().join("missing.bin.lajufile"),
        dir.path().join("missing.bin"),
        segments(),
        vec![Duration::from_secs(5), Duration::from_secs(5)],
        None,
    );

    let started = std::time::Instant::now();
    let error = download_segmented(&client(), &spec, |_, _, _| {})
        .await
        .unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(matches!(
        error,
        SegmentedError::Download(lajufile_core::download::DownloadError::Status(404))
    ));
}
