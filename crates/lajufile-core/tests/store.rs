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
