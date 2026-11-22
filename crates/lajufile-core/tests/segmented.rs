use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lajufile_core::http::{RequestOptions, build_client};
use lajufile_core::segment::plan_segments;
use lajufile_core::segmented::{
    SegmentedError, SegmentedSpec, download_segmented, download_segmented_with_splits,
    prefix_matches,
};
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
        item_limiter: None,
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

#[tokio::test]
async fn resume_with_if_range_requires_existing_temp_file_of_sufficient_size() {
    let base = start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let missing_temp = dir.path().join("missing.bin.lajufile");
    let final_path = dir.path().join("missing.bin");
    let mut segs = segments();
    segs[0].done = 512 * 1024;
    let missing_spec = spec(
        &base,
        "/seg",
        missing_temp,
        final_path.clone(),
        segs.clone(),
        Vec::new(),
        Some("\"seg-v1\"".to_string()),
    );
    let err = download_segmented(&client(), &missing_spec, |_, _, _| {})
        .await
        .unwrap_err();
    assert!(matches!(err, SegmentedError::RestartRequired));
    assert!(!final_path.exists());

    // Truncated temp file below saved segment offset must also trigger restart instead of zero-fill.
    let truncated_temp = dir.path().join("trunc.bin.lajufile");
    let truncated_final = dir.path().join("trunc.bin");
    std::fs::write(&truncated_temp, &seg_payload()[..1024]).unwrap();
    let trunc_spec = spec(
        &base,
        "/seg",
        truncated_temp,
        truncated_final.clone(),
        segs,
        Vec::new(),
        Some("\"seg-v1\"".to_string()),
    );
    let err = download_segmented(&client(), &trunc_spec, |_, _, _| {})
        .await
        .unwrap_err();
    assert!(matches!(err, SegmentedError::RestartRequired));
    assert!(!truncated_final.exists());
}

#[tokio::test]
async fn dynamic_segment_splitting_steals_slow_segment_and_assembles_exact_file() {
    let base = start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let temp = dir.path().join("split.bin.lajufile");
    let final_path = dir.path().join("split.bin");
    let segs = plan_segments(SEG_SIZE as u64, true, 2);
    let split_spec = spec(
        &base,
        "/seg-uneven",
        temp,
        final_path.clone(),
        segs,
        Vec::new(),
        None,
    );
    let splits = Arc::new(Mutex::new(Vec::<Vec<Segment>>::new()));
    let splits_seen = Arc::clone(&splits);

    let outcome = download_segmented_with_splits(
        &client(),
        &split_spec,
        |_, _, _| {},
        move |snapshot| {
            splits_seen.lock().unwrap().push(snapshot);
        },
    )
    .await
    .unwrap();

    assert_eq!(outcome.bytes, SEG_SIZE as u64);
    assert_eq!(std::fs::read(&final_path).unwrap(), seg_payload());
    let recorded = splits.lock().unwrap();
    assert!(
        !recorded.is_empty(),
        "expected fast worker to steal trailing bytes from slow segment 0"
    );
    assert!(recorded.last().unwrap().len() >= 3);
}

#[tokio::test]
async fn resume_succeeds_after_dynamic_split_produces_non_sequential_indices() {
    let base = start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let temp = dir.path().join("resumed-split.bin.lajufile");
    let final_path = dir.path().join("resumed-split.bin");
    let payload = seg_payload();
    let size = SEG_SIZE as u64;
    let q = size / 4;

    // Simulate SQLite `ORDER BY idx` after segment 0 finished and split segment 1 into idx=1 and idx=4.
    let mut partial = vec![0_u8; SEG_SIZE];
    partial[..q as usize].copy_from_slice(&payload[..q as usize]);
    partial[q as usize..(q + 256 * 1024) as usize]
        .copy_from_slice(&payload[q as usize..(q + 256 * 1024) as usize]);
    std::fs::write(&temp, &partial).unwrap();

    let split_segments = vec![
        Segment {
            idx: 0,
            start: 0,
            end: q - 1,
            done: q,
        },
        Segment {
            idx: 1,
            start: q,
            end: q + q / 2 - 1,
            done: 256 * 1024,
        },
        Segment {
            idx: 2,
            start: 2 * q,
            end: 3 * q - 1,
            done: 0,
        },
        Segment {
            idx: 3,
            start: 3 * q,
            end: size - 1,
            done: 0,
        },
        Segment {
            idx: 4,
            start: q + q / 2,
            end: 2 * q - 1,
            done: 0,
        },
    ];

    let resume_spec = spec(
        &base,
        "/seg",
        temp.clone(),
        final_path.clone(),
        split_segments,
        Vec::new(),
        None,
    );
    let outcome = download_segmented_with_splits(&client(), &resume_spec, |_, _, _| {}, |_| {})
        .await
        .unwrap();

    assert_eq!(outcome.bytes, size);
    assert_eq!(std::fs::read(&final_path).unwrap(), payload);
    assert!(!temp.exists());
}
