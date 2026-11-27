use lajufile_core::store::{Category, DownloadStatus, NewDownload, Segment, Store, StoreError};
use std::path::PathBuf;

fn sample(url: &str, size: u64) -> NewDownload {
    NewDownload {
        url: url.to_string(),
        final_url: Some("https://cdn.example.com/final.iso".to_string()),
        file_name: "a.iso".to_string(),
        save_path: PathBuf::from(r"C:\Downloads\a.iso"),
        temp_path: PathBuf::from(r"C:\Downloads\a.iso.lajufile"),
        size: Some(size),
        status: DownloadStatus::Queued,
        category: Category::Other,
        kind: "http".to_string(),
        video_format: None,
        connections: 1,
        ranged: true,
        etag: Some("\"abc\"".to_string()),
        last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".to_string()),
        cookie_enc: Some(vec![1, 2, 3]),
        referrer: Some("https://example.com/".to_string()),
        user_agent: Some("test-agent".to_string()),
        queue_order: 1,
        created_at: 1_700_000_000_000,
        speed_limit_kbps: 0,
        expected_sha256: None,
        segments: vec![Segment {
            idx: 0,
            start: 0,
            end: size.saturating_sub(1),
            done: 0,
        }],
    }
}

#[test]
fn open_creates_schema_v1_and_reopen_keeps_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lajufile.db");
    let mut store = Store::open(&path).unwrap();
    let id = store
        .insert_download(&sample("https://example.com/a.iso", 1000))
        .unwrap();
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.get_download(id).unwrap().id, id);
}

#[test]
fn insert_and_get_roundtrip_all_fields() {
    let mut store = Store::open_in_memory().unwrap();
    let id = store
        .insert_download(&sample("https://example.com/a.iso", 1000))
        .unwrap();
    let d = store.get_download(id).unwrap();
    assert_eq!(d.url, "https://example.com/a.iso");
    assert_eq!(
        d.final_url.as_deref(),
        Some("https://cdn.example.com/final.iso")
    );
    assert_eq!(d.file_name, "a.iso");
    assert_eq!(d.save_path, PathBuf::from(r"C:\Downloads\a.iso"));
    assert_eq!(d.temp_path, PathBuf::from(r"C:\Downloads\a.iso.lajufile"));
    assert_eq!(d.size, Some(1000));
    assert_eq!(d.status, DownloadStatus::Queued);
    assert_eq!(d.category, Category::Other);
    assert_eq!(d.connections, 1);
    assert!(d.ranged);
    assert_eq!(d.etag.as_deref(), Some("\"abc\""));
    assert_eq!(
        d.last_modified.as_deref(),
        Some("Wed, 21 Oct 2015 07:28:00 GMT")
    );
    assert_eq!(d.cookie_enc, Some(vec![1, 2, 3]));
    assert_eq!(d.referrer.as_deref(), Some("https://example.com/"));
    assert_eq!(d.user_agent.as_deref(), Some("test-agent"));
    assert_eq!(d.downloaded, 0);
    assert_eq!(d.error, None);
    assert_eq!(d.queue_order, 1);
    assert_eq!(d.created_at, 1_700_000_000_000);
    assert_eq!(d.completed_at, None);
}

#[test]
fn insert_persists_segments_in_order() {
    let mut store = Store::open_in_memory().unwrap();
    let mut new = sample("https://example.com/segments.bin", 1000);
    new.segments = vec![
        Segment {
            idx: 1,
            start: 500,
            end: 999,
            done: 0,
        },
        Segment {
            idx: 0,
            start: 0,
            end: 499,
            done: 10,
        },
    ];
    let id = store.insert_download(&new).unwrap();
    let segments = store.segments(id).unwrap();
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].idx, 0);
    assert_eq!(segments[0].done, 10);
    assert_eq!(segments[1].idx, 1);
    assert_eq!(segments[1].start, 500);
}

#[test]
fn active_by_url_only_matches_queued_downloading_paused() {
    let mut store = Store::open_in_memory().unwrap();

    let queued = store
        .insert_download(&sample("https://example.com/queued", 100))
        .unwrap();
    let found = store
        .active_by_url("https://example.com/queued")
        .unwrap()
        .unwrap();
    assert_eq!(found.id, queued);
    store
        .update_status(queued, DownloadStatus::Done, None)
        .unwrap();
    assert!(
        store
            .active_by_url("https://example.com/queued")
            .unwrap()
            .is_none()
    );

    let mut paused = sample("https://example.com/paused", 100);
    paused.status = DownloadStatus::Paused;
    let paused_id = store.insert_download(&paused).unwrap();
    assert_eq!(
        store
            .active_by_url("https://example.com/paused")
            .unwrap()
            .unwrap()
            .id,
        paused_id
    );

    let mut downloading = sample("https://example.com/downloading", 100);
    downloading.status = DownloadStatus::Downloading;
    let downloading_id = store.insert_download(&downloading).unwrap();
    assert_eq!(
        store
            .active_by_url("https://example.com/downloading")
            .unwrap()
            .unwrap()
            .id,
        downloading_id
    );

    let error = store
        .insert_download(&sample("https://example.com/error", 100))
        .unwrap();
    store
        .update_status(error, DownloadStatus::Error, Some("boom"))
        .unwrap();
    assert!(
        store
            .active_by_url("https://example.com/error")
            .unwrap()
            .is_none()
    );

    assert!(
        store
            .active_by_url("https://example.com/queued-other")
            .unwrap()
            .is_none()
    );
}

#[test]
fn update_progress_updates_download_and_segment() {
    let mut store = Store::open_in_memory().unwrap();
    let id = store
        .insert_download(&sample("https://example.com/progress.bin", 1000))
        .unwrap();
    store.update_progress(id, 0, 500).unwrap();
    assert_eq!(store.get_download(id).unwrap().downloaded, 500);
    assert_eq!(store.segments(id).unwrap()[0].done, 500);
    assert!(matches!(
        store.update_progress(id, 99, 1),
        Err(StoreError::NotFound(_))
    ));
}

#[test]
fn mark_done_sets_status_completed_at_and_downloaded() {
    let mut store = Store::open_in_memory().unwrap();
    let id = store
        .insert_download(&sample("https://example.com/done.bin", 1000))
        .unwrap();
    store.mark_done(id, 123).unwrap();
    let d = store.get_download(id).unwrap();
    assert_eq!(d.status, DownloadStatus::Done);
    assert_eq!(d.completed_at, Some(123));
    assert_eq!(d.downloaded, 1000);
    assert_eq!(d.error, None);
}

#[test]
fn delete_cascades_segments() {
    let mut store = Store::open_in_memory().unwrap();
    let id = store
        .insert_download(&sample("https://example.com/delete.bin", 1000))
        .unwrap();
    store.delete_download(id).unwrap();
    assert!(matches!(
        store.get_download(id),
        Err(StoreError::NotFound(_))
    ));
    assert!(store.segments(id).unwrap().is_empty());
}

#[test]
fn category_mapping_is_case_insensitive() {
    assert_eq!(Category::from_file_name("movie.MP4"), Category::Video);
    assert_eq!(Category::from_file_name("song.mp3"), Category::Music);
    assert_eq!(Category::from_file_name("photo.png"), Category::Images);
    assert_eq!(Category::from_file_name("pic.WEBP"), Category::Images);
    assert_eq!(Category::from_file_name("report.pdf"), Category::Documents);
    assert_eq!(Category::from_file_name("setup.EXE"), Category::Programs);
    assert_eq!(
        Category::from_file_name("archive.zip"),
        Category::Compressed
    );
    assert_eq!(Category::from_file_name("ubuntu.iso"), Category::Other);
    assert_eq!(Category::from_file_name("README"), Category::Other);
}

fn sample_with_segments(size: u64, segments: Vec<Segment>) -> NewDownload {
    let mut download = sample("https://example.com/segments.bin", size);
    download.segments = segments;
    download
}

#[test]
fn update_progress_aggregates_segments() {
    let mut store = Store::open_in_memory().unwrap();
    let id = store
        .insert_download(&sample_with_segments(
            200,
            vec![
                Segment {
                    idx: 0,
                    start: 0,
                    end: 99,
                    done: 0,
                },
                Segment {
                    idx: 1,
                    start: 100,
                    end: 199,
                    done: 0,
                },
            ],
        ))
        .unwrap();

    assert_eq!(store.update_progress(id, 0, 100).unwrap(), 100);
    assert_eq!(store.update_progress(id, 1, 50).unwrap(), 150);
    assert_eq!(store.get_download(id).unwrap().downloaded, 150);
    assert_eq!(store.segments(id).unwrap()[1].done, 50);
}

#[test]
fn update_progress_batch_rolls_back_on_unknown_segment() {
    let mut store = Store::open_in_memory().unwrap();
    let id = store
        .insert_download(&sample_with_segments(
            200,
            vec![
                Segment {
                    idx: 0,
                    start: 0,
                    end: 99,
                    done: 0,
                },
                Segment {
                    idx: 1,
                    start: 100,
                    end: 199,
                    done: 0,
                },
            ],
        ))
        .unwrap();

    assert_eq!(
        store
            .update_progress_batch(id, &[(0, 100), (1, 50)])
            .unwrap(),
        150
    );
    assert_eq!(store.get_download(id).unwrap().downloaded, 150);
    let segments = store.segments(id).unwrap();
    assert_eq!(segments[0].done, 100);
    assert_eq!(segments[1].done, 50);

    assert!(matches!(
        store.update_progress_batch(id, &[(0, 10), (7, 10)]),
        Err(StoreError::NotFound(_))
    ));
    assert_eq!(store.get_download(id).unwrap().downloaded, 150);
    assert_eq!(store.segments(id).unwrap()[0].done, 100);
}

#[test]
fn replace_segments_resets_progress_and_metadata() {
    let mut store = Store::open_in_memory().unwrap();
    let id = store
        .insert_download(&sample_with_segments(
            100,
            vec![Segment {
                idx: 0,
                start: 0,
                end: 99,
                done: 40,
            }],
        ))
        .unwrap();

    let total = store
        .replace_segments(
            id,
            &[
                Segment {
                    idx: 0,
                    start: 0,
                    end: 49,
                    done: 10,
                },
                Segment {
                    idx: 1,
                    start: 50,
                    end: 99,
                    done: 5,
                },
            ],
            2,
            true,
        )
        .unwrap();

    assert_eq!(total, 15);
    let download = store.get_download(id).unwrap();
    assert_eq!(download.connections, 2);
    assert!(download.ranged);
    assert_eq!(download.downloaded, 15);
    assert_eq!(store.segments(id).unwrap().len(), 2);
}

#[test]
fn set_size_only_fills_an_unknown_size() {
    let mut store = Store::open_in_memory().unwrap();
    let mut new = sample("https://example.com/a.iso", 1000);
    new.size = None;
    let id = store.insert_download(&new).unwrap();

    store.set_size(id, 500).unwrap();
    assert_eq!(store.get_download(id).unwrap().size, Some(500));

    store.set_size(id, 900).unwrap();
    assert_eq!(store.get_download(id).unwrap().size, Some(500));

    let mut video = sample("https://www.youtube.com/watch?v=x", 1024);
    video.kind = "video".to_string();
    video.size = None;
    let vid = store.insert_download(&video).unwrap();
    store.set_video_size(vid, 50_000_000).unwrap();
    assert_eq!(store.get_download(vid).unwrap().size, Some(50_000_000));
    store.mark_video_done(vid, Some(48_500_000), 999).unwrap();
    let done = store.get_download(vid).unwrap();
    assert_eq!(done.status, DownloadStatus::Done);
    assert_eq!(done.size, Some(48_500_000));
    assert_eq!(done.downloaded, 48_500_000);
}

#[test]
fn update_url_clears_validators_and_error() {
    let mut store = Store::open_in_memory().unwrap();
    let mut new = sample("https://example.com/old.zip", 100);
    new.etag = Some("\"v1\"".to_string());
    new.last_modified = Some("Mon, 01 Jan 2024 00:00:00 GMT".to_string());
    let id = store.insert_download(&new).unwrap();
    store
        .update_status(id, DownloadStatus::Error, Some("403 expired"))
        .unwrap();

    store
        .update_url(id, "https://example.com/new.zip?token=fresh")
        .unwrap();
    let updated = store.get_download(id).unwrap();
    assert_eq!(updated.url, "https://example.com/new.zip?token=fresh");
    assert_eq!(updated.final_url, None);
    assert_eq!(updated.etag, None);
    assert_eq!(updated.last_modified, None);
    assert_eq!(updated.error, None);
}

#[test]
fn update_url_strips_cookies_and_referrer_across_hosts_or_https_downgrade() {
    let mut store = Store::open_in_memory().unwrap();
    let mut new = sample("https://origin.example/old.zip", 100);
    new.cookie_enc = Some(b"sealed-secret".to_vec());
    new.referrer = Some("https://origin.example/page".to_string());
    let id = store.insert_download(&new).unwrap();

    // Same HTTPS host preserves cookie & referrer.
    store
        .update_url(id, "https://origin.example/refreshed.zip")
        .unwrap();
    let same_host = store.get_download(id).unwrap();
    assert_eq!(
        same_host.cookie_enc.as_deref(),
        Some(b"sealed-secret".as_slice())
    );
    assert_eq!(
        same_host.referrer.as_deref(),
        Some("https://origin.example/page")
    );

    // Different host strips cookie & referrer.
    store
        .update_url(id, "https://mirror.example/refreshed.zip")
        .unwrap();
    let diff_host = store.get_download(id).unwrap();
    assert_eq!(diff_host.cookie_enc, None);
    assert_eq!(diff_host.referrer, None);

    // HTTPS -> HTTP downgrade on same host also strips cookie & referrer.
    let mut https_row = sample("https://origin.example/file.zip", 100);
    https_row.cookie_enc = Some(b"sealed-secret".to_vec());
    https_row.referrer = Some("https://origin.example/page".to_string());
    let id2 = store.insert_download(&https_row).unwrap();
    store
        .update_url(id2, "http://origin.example/file.zip")
        .unwrap();
    let downgraded = store.get_download(id2).unwrap();
    assert_eq!(downgraded.cookie_enc, None);
    assert_eq!(downgraded.referrer, None);
}

#[test]
fn update_location_renames_and_updates_paths() {
    let mut store = Store::open_in_memory().unwrap();
    let id = store
        .insert_download(&sample("https://example.com/a.iso", 100))
        .unwrap();
    let save = PathBuf::from("D:/Renamed/ubuntu.iso");
    let temp = PathBuf::from("D:/Renamed/ubuntu.iso.lajufile");
    store
        .update_location(id, "ubuntu.iso", &save, &temp)
        .unwrap();
    let d = store.get_download(id).unwrap();
    assert_eq!(d.file_name, "ubuntu.iso");
    assert_eq!(d.save_path, save);
    assert_eq!(d.temp_path, temp);
}

#[test]
fn move_to_top_speed_limit_and_checksum_status_persist() {
    let mut store = Store::open_in_memory().unwrap();
    let mut a = sample("https://example.com/a.iso", 100);
    a.queue_order = 10;
    let id_a = store.insert_download(&a).unwrap();

    let mut b = sample("https://example.com/b.iso", 200);
    b.queue_order = 20;
    b.speed_limit_kbps = 512;
    b.expected_sha256 = Some("a".repeat(64));
    let id_b = store.insert_download(&b).unwrap();

    store.move_to_top(id_b).unwrap();
    let rows = store.list_downloads().unwrap();
    assert_eq!(rows[0].id, id_b);
    assert_eq!(rows[1].id, id_a);

    store.update_speed_limit(id_b, 1024).unwrap();
    store
        .update_checksum_result(id_b, "mismatch", Some("SHA-256 mismatch"))
        .unwrap();
    let updated = store.get_download(id_b).unwrap();
    assert_eq!(updated.speed_limit_kbps, 1024);
    assert_eq!(
        updated.expected_sha256.as_deref(),
        Some("a".repeat(64).as_str())
    );
    assert_eq!(updated.checksum_status.as_deref(), Some("mismatch"));
    assert_eq!(updated.status, DownloadStatus::Error);
    assert_eq!(updated.error.as_deref(), Some("SHA-256 mismatch"));
}

#[test]
fn update_download_source_refreshes_url_and_headers_preserving_progress() {
    let mut store = Store::open_in_memory().unwrap();
    let mut new = sample("https://cdn.example.com/old.zip?sig=expired", 1000);
    new.cookie_enc = Some(b"old-cookie".to_vec());
    new.referrer = Some("https://example.com/old".to_string());
    let id = store.insert_download(&new).unwrap();
    store.update_progress(id, 0, 400).unwrap();
    store
        .update_status(id, DownloadStatus::Error, Some("HTTP 403"))
        .unwrap();

    store
        .update_download_source(
            id,
            "https://cdn2.example.com/new.zip?sig=fresh",
            Some(b"fresh-cookie"),
            Some("https://example.com/new"),
            Some("Browser/2.0"),
        )
        .unwrap();

    let refreshed = store.get_download(id).unwrap();
    assert_eq!(refreshed.url, "https://cdn2.example.com/new.zip?sig=fresh");
    assert_eq!(refreshed.final_url, None);
    assert_eq!(refreshed.etag, None);
    assert_eq!(refreshed.last_modified, None);
    assert_eq!(refreshed.error, None);
    assert_eq!(refreshed.downloaded, 400);
    assert_eq!(
        refreshed.cookie_enc.as_deref(),
        Some(b"fresh-cookie".as_slice())
    );
    assert_eq!(
        refreshed.referrer.as_deref(),
        Some("https://example.com/new")
    );
    assert_eq!(refreshed.user_agent.as_deref(), Some("Browser/2.0"));
}
