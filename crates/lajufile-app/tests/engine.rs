mod support;

use std::path::Path;
use std::time::{Duration, Instant};

use lajufile_app::engine::{AddRequest, Command, EngineConfig, EngineHandle, Snapshot};
use lajufile_core::settings::{DuplicatePolicy, Settings};
use lajufile_core::store::{Category, DownloadStatus, NewDownload, Segment, Store, now_millis};
use support::{SEG_SIZE, seg_payload, start_server};

const TEST_BRIDGE_TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn test_settings(dir: &Path) -> Settings {
    Settings {
        download_dir: dir.join("downloads"),
        bridge_port: 0,
        bridge_token: TEST_BRIDGE_TOKEN.to_string(),
        ..Settings::default()
    }
}

#[test]
fn add_probes_and_inserts() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings {
        download_dir: dir.path().join("downloads"),
        ..test_settings(dir.path())
    };
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings,
    })
    .unwrap();

    handle
        .send(Command::Add(AddRequest {
            url: format!(
                "HTTP://{}/file.bin",
                server.base.trim_start_matches("http://")
            ),
            file_name: None,
            save_dir: None,
            start_now: false,
            connections: None,
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format: None,
            video_size: None,
            expected_sha256: None,
        }))
        .unwrap();

    let snap = wait_for(&handle, |s| s.downloads.len() == 1, Duration::from_secs(15));
    assert_eq!(snap.downloads[0].file_name, "file.bin");
    assert_eq!(snap.downloads[0].url, format!("{}/file.bin", server.base));

    handle.shutdown();
    let store = Store::open(&dir.path().join("lajufile.db")).unwrap();
    assert_eq!(store.list_downloads().unwrap().len(), 1);
}

#[test]
fn add_missing_url_keeps_store_empty() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings {
        download_dir: dir.path().join("downloads"),
        ..test_settings(dir.path())
    };
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings,
    })
    .unwrap();

    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/missing", server.base),
            file_name: None,
            save_dir: None,
            start_now: false,
            connections: None,
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format: None,
            video_size: None,
            expected_sha256: None,
        }))
        .unwrap();

    let snap = wait_for(
        &handle,
        |s| s.message.as_deref().is_some_and(|m| m.contains("failed")),
        Duration::from_secs(15),
    );
    assert!(snap.downloads.is_empty());

    handle.shutdown();
    let store = Store::open(&dir.path().join("lajufile.db")).unwrap();
    assert!(store.list_downloads().unwrap().is_empty());
}

#[test]
fn add_duplicate_active_is_rejected() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings {
        download_dir: dir.path().join("downloads"),
        ..test_settings(dir.path())
    };
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings,
    })
    .unwrap();

    let request = AddRequest {
        url: format!("{}/slow.bin", server.base),
        file_name: None,
        save_dir: None,
        start_now: true,
        connections: None,
        referrer: None,
        user_agent: None,
        cookie: None,
        video_format: None,
        video_size: None,
        expected_sha256: None,
    };
    handle.send(Command::Add(request)).unwrap();
    let first = wait_for(
        &handle,
        |s| {
            s.downloads
                .first()
                .is_some_and(|d| d.status == DownloadStatus::Downloading)
        },
        Duration::from_secs(15),
    );
    let first_id = first.downloads[0].id;

    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/slow.bin", server.base),
            file_name: None,
            save_dir: None,
            start_now: true,
            connections: None,
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format: None,
            video_size: None,
            expected_sha256: None,
        }))
        .unwrap();
    let second = wait_for(
        &handle,
        |s| s.message.as_deref().is_some_and(|m| m.contains("Already")),
        Duration::from_secs(15),
    );

    assert_eq!(second.downloads.len(), 1);
    assert_eq!(second.downloads[0].id, first_id);
    handle.shutdown();
}

#[test]
fn back_to_back_duplicate_is_rejected() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings {
        download_dir: dir.path().join("downloads"),
        ..test_settings(dir.path())
    };
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings,
    })
    .unwrap();

    let url = format!("{}/slow-headers.bin", server.base);
    for _ in 0..2 {
        handle
            .send(Command::Add(AddRequest {
                url: url.clone(),
                file_name: None,
                save_dir: None,
                start_now: true,
                connections: None,
                referrer: None,
                user_agent: None,
                cookie: None,
                video_format: None,
                video_size: None,
                expected_sha256: None,
            }))
            .unwrap();
    }

    let snap = wait_for(
        &handle,
        |s| s.downloads.len() == 1 && s.message.as_deref().is_some_and(|m| m.contains("Already")),
        Duration::from_secs(15),
    );
    assert_eq!(snap.downloads.len(), 1);
    handle.shutdown();
}

#[test]
fn same_explicit_name_gets_distinct_paths() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings {
        download_dir: dir.path().join("downloads"),
        ..test_settings(dir.path())
    };
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings,
    })
    .unwrap();

    for query in ["a", "b"] {
        handle
            .send(Command::Add(AddRequest {
                url: format!("{}/slow-headers.bin?{query}", server.base),
                file_name: Some("same.bin".to_string()),
                save_dir: None,
                start_now: true,
                connections: None,
                referrer: None,
                user_agent: None,
                cookie: None,
                video_format: None,
                video_size: None,
                expected_sha256: None,
            }))
            .unwrap();
    }

    let snap = wait_for(
        &handle,
        |s| s.downloads.len() == 2 && s.downloads.iter().all(|d| d.status == DownloadStatus::Done),
        Duration::from_secs(10),
    );
    assert_ne!(snap.downloads[0].save_path, snap.downloads[1].save_path);
    assert_ne!(snap.downloads[0].temp_path, snap.downloads[1].temp_path);
    assert!(snap.downloads.iter().all(|d| d.save_path.exists()));
    handle.shutdown();
}

#[test]
fn pause_persists_every_segment_before_status_change() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings {
        download_dir: dir.path().join("downloads"),
        max_connections: 4,
        max_concurrent: 1,
        global_speed_limit_kbps: 256,
        ..test_settings(dir.path())
    };
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings,
    })
    .unwrap();

    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/slow-seg.bin", server.base),
            file_name: None,
            save_dir: None,
            start_now: true,
            connections: None,
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format: None,
            video_size: None,
            expected_sha256: None,
        }))
        .unwrap();
    let snap = wait_for(
        &handle,
        |s| {
            s.downloads.first().is_some_and(|d| {
                d.status == DownloadStatus::Downloading
                    && d.downloaded >= 256 * 1024
                    && s.segments
                        .get(&d.id)
                        .is_some_and(|segs| segs.len() == 4 && segs.iter().all(|seg| seg.done > 0))
            })
        },
        Duration::from_secs(15),
    );
    let id = snap.downloads[0].id;

    handle.send(Command::Pause(id)).unwrap();
    wait_for(
        &handle,
        |s| s.downloads[0].status == DownloadStatus::Paused,
        Duration::from_secs(15),
    );
    handle.shutdown();

    let store = Store::open(&dir.path().join("lajufile.db")).unwrap();
    let download = store.get_download(id).unwrap();
    let segments = store.segments(id).unwrap();
    assert_eq!(segments.len(), 4);
    assert!(
        segments.iter().all(|segment| segment.done > 0),
        "expected all segments > 0, got {segments:?} (downloaded={})",
        download.downloaded
    );
    let sum: u64 = segments.iter().map(|segment| segment.done).sum();
    assert_eq!(download.downloaded, sum);
}

#[test]
fn corrupt_cookie_marks_download_error() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("lajufile.db");
    let mut store = Store::open(&db_path).unwrap();
    let id = store
        .insert_download(&NewDownload {
            url: format!("{}/file.bin", server.base),
            final_url: None,
            file_name: "cookie.bin".to_string(),
            save_path: dir.path().join("downloads/cookie.bin"),
            temp_path: dir.path().join("downloads/cookie.bin.lajufile"),
            size: Some(1024 * 1024),
            status: DownloadStatus::Queued,
            category: Category::Other,
            kind: "http".to_string(),
            video_format: None,
            connections: 1,
            ranged: false,
            etag: None,
            last_modified: None,
            cookie_enc: Some(vec![1, 2, 3]),
            referrer: None,
            user_agent: None,
            queue_order: 0,
            created_at: now_millis(),
            speed_limit_kbps: 0,
            expected_sha256: None,
            segments: vec![Segment {
                idx: 0,
                start: 0,
                end: 1024 * 1024 - 1,
                done: 0,
            }],
        })
        .unwrap();
    let valid_id = store
        .insert_download(&NewDownload {
            url: format!("{}/file.bin", server.base),
            final_url: None,
            file_name: "valid.bin".to_string(),
            save_path: dir.path().join("downloads/valid.bin"),
            temp_path: dir.path().join("downloads/valid.bin.lajufile"),
            size: Some(1024 * 1024),
            status: DownloadStatus::Queued,
            category: Category::Other,
            kind: "http".to_string(),
            video_format: None,
            connections: 1,
            ranged: false,
            etag: None,
            last_modified: None,
            cookie_enc: None,
            referrer: None,
            user_agent: None,
            queue_order: 1,
            created_at: now_millis(),
            speed_limit_kbps: 0,
            expected_sha256: None,
            segments: vec![Segment {
                idx: 0,
                start: 0,
                end: 1024 * 1024 - 1,
                done: 0,
            }],
        })
        .unwrap();
    drop(store);

    let settings = Settings {
        download_dir: dir.path().join("downloads"),
        bridge_port: 0,
        bridge_token: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
            .to_string(),
        ..test_settings(dir.path())
    };
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path,
        settings,
    })
    .unwrap();
    let snap = wait_for(
        &handle,
        |s| {
            let corrupt = s
                .downloads
                .iter()
                .find(|d| d.id == id)
                .is_some_and(|d| d.status == DownloadStatus::Error);
            let valid = s
                .downloads
                .iter()
                .find(|d| d.id == valid_id)
                .is_some_and(|d| d.status == DownloadStatus::Done);
            corrupt && valid
        },
        Duration::from_secs(5),
    );
    assert!(
        snap.downloads
            .iter()
            .find(|d| d.id == id)
            .and_then(|d| d.error.as_deref())
            .is_some_and(|error| error.contains("cookie"))
    );
    assert!(
        snap.downloads
            .iter()
            .find(|d| d.id == valid_id)
            .is_some_and(|d| d.save_path.exists())
    );
    handle.shutdown();
}

#[test]
fn video_row_without_tools_marks_error() {
    let dir = tempfile::tempdir().unwrap();
    let tools = dir.path().join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    // Process-wide, so this is the only test that runs a video job.
    unsafe { std::env::set_var("LAJUFILE_TOOLS_DIR", &tools) };

    let db_path = dir.path().join("lajufile.db");
    let mut store = Store::open(&db_path).unwrap();
    let id = store
        .insert_download(&NewDownload {
            url: "https://www.youtube.com/watch?v=abc".to_string(),
            final_url: None,
            file_name: "clip.mp4".to_string(),
            save_path: dir.path().join("downloads/clip.mp4"),
            temp_path: dir.path().join("downloads/clip.mp4.lajufile"),
            size: None,
            status: DownloadStatus::Queued,
            category: Category::Video,
            kind: "video".to_string(),
            video_format: Some("bv*[height<=360]+ba".to_string()),
            connections: 1,
            ranged: false,
            etag: None,
            last_modified: None,
            cookie_enc: None,
            referrer: None,
            user_agent: None,
            queue_order: 0,
            created_at: now_millis(),
            speed_limit_kbps: 0,
            expected_sha256: None,
            segments: vec![Segment {
                idx: 0,
                start: 0,
                end: 0,
                done: 0,
            }],
        })
        .unwrap();
    drop(store);

    let settings = Settings {
        download_dir: dir.path().join("downloads"),
        ..test_settings(dir.path())
    };
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path,
        settings,
    })
    .unwrap();

    let snap = wait_for(
        &handle,
        |s| {
            s.downloads
                .iter()
                .find(|d| d.id == id)
                .is_some_and(|d| d.status == DownloadStatus::Error)
        },
        Duration::from_secs(10),
    );
    let download = snap.downloads.iter().find(|d| d.id == id).unwrap();
    assert!(
        download
            .error
            .as_deref()
            .is_some_and(|error| error.contains("tools")),
        "unexpected error: {:?}",
        download.error
    );
    handle.shutdown();
    unsafe { std::env::remove_var("LAJUFILE_TOOLS_DIR") };
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

#[test]
fn add_start_now_downloads_file() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings {
        download_dir: dir.path().join("downloads"),
        max_concurrent: 1,
        ..test_settings(dir.path())
    };
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings,
    })
    .unwrap();

    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/file.bin", server.base),
            file_name: None,
            save_dir: None,
            start_now: true,
            connections: None,
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format: None,
            video_size: None,
            expected_sha256: None,
        }))
        .unwrap();

    let snap = wait_for(
        &handle,
        |s| s.downloads.len() == 1 && s.downloads[0].status == DownloadStatus::Done,
        Duration::from_secs(10),
    );
    let download = &snap.downloads[0];
    assert!(download.save_path.exists());
    assert_eq!(
        std::fs::metadata(&download.save_path).unwrap().len(),
        1024 * 1024
    );
    assert!(!download.temp_path.exists());
    let id = download.id;

    handle.shutdown();
    let store = Store::open(&dir.path().join("lajufile.db")).unwrap();
    assert_eq!(store.get_download(id).unwrap().status, DownloadStatus::Done);
}

#[test]
fn queue_never_exceeds_max_concurrent() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings {
        download_dir: dir.path().join("downloads"),
        max_concurrent: 1,
        ..test_settings(dir.path())
    };
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings,
    })
    .unwrap();

    for (query, name) in [("a", "a.bin"), ("b", "b.bin")] {
        handle
            .send(Command::Add(AddRequest {
                url: format!("{}/slow.bin?{query}", server.base),
                file_name: Some(name.to_string()),
                save_dir: None,
                start_now: true,
                connections: None,
                referrer: None,
                user_agent: None,
                cookie: None,
                video_format: None,
                video_size: None,
                expected_sha256: None,
            }))
            .unwrap();
    }

    let snap = wait_for(
        &handle,
        |s| {
            s.downloads.len() == 2
                && s.downloads
                    .iter()
                    .any(|d| d.status == DownloadStatus::Downloading)
        },
        Duration::from_secs(15),
    );
    assert_eq!(
        snap.downloads
            .iter()
            .filter(|d| d.status == DownloadStatus::Downloading)
            .count(),
        1
    );
    assert_eq!(
        snap.downloads
            .iter()
            .filter(|d| d.status == DownloadStatus::Queued)
            .count(),
        1
    );

    let done = wait_for(
        &handle,
        |s| s.downloads.len() == 2 && s.downloads.iter().all(|d| d.status == DownloadStatus::Done),
        Duration::from_secs(10),
    );
    assert!(done.downloads.iter().all(|d| d.save_path.exists()));
    handle.shutdown();
}

#[test]
fn pause_wins_over_late_finish_and_resume_completes() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings {
        download_dir: dir.path().join("downloads"),
        max_concurrent: 1,
        ..test_settings(dir.path())
    };
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings,
    })
    .unwrap();

    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/slow.bin", server.base),
            file_name: None,
            save_dir: None,
            start_now: true,
            connections: None,
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format: None,
            video_size: None,
            expected_sha256: None,
        }))
        .unwrap();
    let snap = wait_for(
        &handle,
        |s| {
            s.downloads
                .first()
                .is_some_and(|d| d.status == DownloadStatus::Downloading)
        },
        Duration::from_secs(15),
    );
    let id = snap.downloads[0].id;

    handle.send(Command::Pause(id)).unwrap();
    wait_for(
        &handle,
        |s| s.downloads[0].status == DownloadStatus::Paused,
        Duration::from_secs(15),
    );
    std::thread::sleep(Duration::from_millis(600));
    let still_paused = handle.latest_snapshot().unwrap();
    assert_eq!(still_paused.downloads[0].status, DownloadStatus::Paused);

    handle.send(Command::Resume(id)).unwrap();
    let done = wait_for(
        &handle,
        |s| s.downloads[0].status == DownloadStatus::Done,
        Duration::from_secs(10),
    );
    assert!(done.downloads[0].save_path.exists());
    handle.shutdown();
}

#[test]
fn remove_deletes_record_and_temp() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings {
        download_dir: dir.path().join("downloads"),
        max_concurrent: 1,
        ..test_settings(dir.path())
    };
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings,
    })
    .unwrap();

    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/slow.bin", server.base),
            file_name: None,
            save_dir: None,
            start_now: true,
            connections: None,
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format: None,
            video_size: None,
            expected_sha256: None,
        }))
        .unwrap();
    let snap = wait_for(
        &handle,
        |s| {
            s.downloads
                .first()
                .is_some_and(|d| d.status == DownloadStatus::Downloading)
        },
        Duration::from_secs(15),
    );
    let id = snap.downloads[0].id;
    let temp_path = snap.downloads[0].temp_path.clone();

    handle.send(Command::Remove(id)).unwrap();
    wait_for(&handle, |s| s.downloads.is_empty(), Duration::from_secs(15));
    std::thread::sleep(Duration::from_millis(600));
    assert!(handle.latest_snapshot().unwrap().downloads.is_empty());

    handle.shutdown();
    let store = Store::open(&dir.path().join("lajufile.db")).unwrap();
    assert!(store.list_downloads().unwrap().is_empty());
    assert!(!temp_path.exists());
}

#[test]
fn segmented_download_uses_multiple_ranges_and_completes() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings {
        download_dir: dir.path().join("downloads"),
        max_connections: 4,
        max_concurrent: 1,
        ..test_settings(dir.path())
    };
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings,
    })
    .unwrap();

    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/seg.bin", server.base),
            file_name: None,
            save_dir: None,
            start_now: true,
            connections: None,
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format: None,
            video_size: None,
            expected_sha256: None,
        }))
        .unwrap();

    let snap = wait_for(
        &handle,
        |s| s.downloads.len() == 1 && s.downloads[0].status == DownloadStatus::Done,
        Duration::from_secs(15),
    );
    let download = &snap.downloads[0];
    assert_eq!(download.connections, 4);
    assert_eq!(std::fs::read(&download.save_path).unwrap(), seg_payload());
    assert!(server.ranges().len() >= 4);
    handle.shutdown();
}

#[test]
fn range_ignored_falls_back_to_single() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings {
        download_dir: dir.path().join("downloads"),
        max_connections: 4,
        max_concurrent: 1,
        ..test_settings(dir.path())
    };
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings,
    })
    .unwrap();

    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/range0-only.bin", server.base),
            file_name: None,
            save_dir: None,
            start_now: true,
            connections: None,
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format: None,
            video_size: None,
            expected_sha256: None,
        }))
        .unwrap();

    let snap = wait_for(
        &handle,
        |s| s.downloads.len() == 1 && s.downloads[0].status == DownloadStatus::Done,
        Duration::from_secs(20),
    );
    let download = &snap.downloads[0];
    assert_eq!(download.connections, 1);
    assert!(!download.ranged);
    assert_eq!(std::fs::read(&download.save_path).unwrap(), seg_payload());
    handle.shutdown();
}

#[test]
fn pause_and_resume_continue_from_offset() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let mut settings = Settings {
        download_dir: dir.path().join("downloads"),
        max_connections: 4,
        max_concurrent: 1,
        global_speed_limit_kbps: 256,
        ..test_settings(dir.path())
    };
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings: settings.clone(),
    })
    .unwrap();

    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/slow-seg.bin", server.base),
            file_name: None,
            save_dir: None,
            start_now: true,
            connections: None,
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format: None,
            video_size: None,
            expected_sha256: None,
        }))
        .unwrap();
    let snap = wait_for(
        &handle,
        |s| {
            s.downloads.first().is_some_and(|d| {
                d.status == DownloadStatus::Downloading
                    && d.downloaded >= 256 * 1024
                    && s.segments
                        .get(&d.id)
                        .is_some_and(|segs| segs.len() == 4 && segs.iter().all(|seg| seg.done > 0))
            })
        },
        Duration::from_secs(15),
    );
    let id = snap.downloads[0].id;

    handle.send(Command::Pause(id)).unwrap();
    let paused = wait_for(
        &handle,
        |s| {
            s.downloads
                .first()
                .is_some_and(|d| d.status == DownloadStatus::Paused)
        },
        Duration::from_secs(15),
    );
    assert!(
        paused.downloads[0].downloaded < SEG_SIZE as u64,
        "download finished before pause settled: {}",
        paused.downloads[0].downloaded
    );
    let ranges_before = server.ranges().len();

    settings.global_speed_limit_kbps = 0;
    handle.send(Command::UpdateSettings(settings)).unwrap();
    handle.send(Command::Resume(id)).unwrap();
    let done = wait_for(
        &handle,
        |s| {
            s.downloads
                .first()
                .is_some_and(|d| d.status == DownloadStatus::Done)
        },
        Duration::from_secs(20),
    );
    assert_eq!(
        std::fs::read(&done.downloads[0].save_path).unwrap(),
        seg_payload()
    );
    let ranges = server.ranges();
    let resumed = &ranges[ranges_before..];
    assert!(
        resumed
            .iter()
            .any(|start| *start > 0 && *start % (1024 * 1024) != 0),
        "expected a resumed range at a partial offset, got {resumed:?}"
    );
    handle.shutdown();
}

#[test]
fn add_records_requested_connections() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings {
        download_dir: dir.path().join("downloads"),
        max_connections: 8,
        ..test_settings(dir.path())
    };
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings,
    })
    .unwrap();

    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/seg.bin", server.base),
            file_name: None,
            save_dir: None,
            start_now: false,
            connections: Some(2),
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format: None,
            video_size: None,
            expected_sha256: None,
        }))
        .unwrap();

    let snap = wait_for(&handle, |s| s.downloads.len() == 1, Duration::from_secs(15));
    let id = snap.downloads[0].id;
    assert_eq!(snap.downloads[0].connections, 2);
    handle.shutdown();

    let store = Store::open(&dir.path().join("lajufile.db")).unwrap();
    let download = store.get_download(id).unwrap();
    let segments = store.segments(id).unwrap();
    assert_eq!(download.connections, 2);
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].start, 0);
    assert_eq!(segments[1].end as usize, SEG_SIZE - 1);
}

#[test]
fn startup_settings_are_clamped() {
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings {
        download_dir: dir.path().join("downloads"),
        max_concurrent: 99,
        max_connections: 99,
        global_speed_limit_kbps: 2_000_000,
        ..test_settings(dir.path())
    };
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings,
    })
    .unwrap();

    let snap = wait_for(&handle, |_| true, Duration::from_secs(15));
    assert_eq!(snap.settings.max_concurrent, 10);
    assert_eq!(snap.settings.max_connections, 32);
    assert_eq!(snap.settings.global_speed_limit_kbps, 1_000_000);
    handle.shutdown();
}

#[test]
fn startup_resumes_a_crashed_download() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let payload = seg_payload();
    let half = SEG_SIZE / 2;
    let save_path = dir.path().join("downloads/recovered.bin");
    let temp_path = dir.path().join("downloads/recovered.bin.lajufile");
    std::fs::create_dir_all(save_path.parent().unwrap()).unwrap();
    std::fs::write(&temp_path, &payload[..half]).unwrap();

    let db_path = dir.path().join("lajufile.db");
    let mut store = Store::open(&db_path).unwrap();
    let id = store
        .insert_download(&NewDownload {
            url: format!("{}/seg.bin", server.base),
            final_url: None,
            file_name: "recovered.bin".to_string(),
            save_path: save_path.clone(),
            temp_path: temp_path.clone(),
            size: Some(SEG_SIZE as u64),
            status: DownloadStatus::Downloading,
            category: Category::Other,
            kind: "http".to_string(),
            video_format: None,
            connections: 2,
            ranged: true,
            etag: None,
            last_modified: None,
            cookie_enc: None,
            referrer: None,
            user_agent: None,
            queue_order: 0,
            created_at: now_millis(),
            speed_limit_kbps: 0,
            expected_sha256: None,
            segments: vec![
                Segment {
                    idx: 0,
                    start: 0,
                    end: half as u64 - 1,
                    done: half as u64,
                },
                Segment {
                    idx: 1,
                    start: half as u64,
                    end: SEG_SIZE as u64 - 1,
                    done: 0,
                },
            ],
        })
        .unwrap();
    drop(store);

    let settings = Settings {
        download_dir: dir.path().join("downloads"),
        max_connections: 2,
        max_concurrent: 1,
        auto_resume: true,
        ..test_settings(dir.path())
    };
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path,
        settings,
    })
    .unwrap();

    let snap = wait_for(
        &handle,
        |s| {
            s.downloads
                .iter()
                .find(|d| d.id == id)
                .is_some_and(|d| d.status == DownloadStatus::Done)
        },
        Duration::from_secs(20),
    );
    assert_eq!(
        std::fs::read(&snap.downloads[0].save_path).unwrap(),
        payload
    );
    assert!(!temp_path.exists());
    let ranges = server.ranges();
    assert!(
        ranges
            .iter()
            .all(|start| *start == 0 || *start >= half as u64),
        "resume must not refetch the finished half, got {ranges:?}"
    );
    handle.shutdown();
}

#[test]
fn remove_completed_clears_done_rows_and_keeps_files() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings {
        download_dir: dir.path().join("downloads"),
        max_concurrent: 1,
        ..test_settings(dir.path())
    };
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings,
    })
    .unwrap();

    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/file.bin", server.base),
            file_name: Some("done.bin".to_string()),
            save_dir: None,
            start_now: true,
            connections: None,
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format: None,
            video_size: None,
            expected_sha256: None,
        }))
        .unwrap();
    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/slow-seg.bin", server.base),
            file_name: Some("later.bin".to_string()),
            save_dir: None,
            start_now: false,
            connections: Some(2),
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format: None,
            video_size: None,
            expected_sha256: None,
        }))
        .unwrap();

    let snap = wait_for(
        &handle,
        |s| {
            s.downloads.len() == 2
                && s.downloads
                    .iter()
                    .any(|d| d.file_name == "done.bin" && d.status == DownloadStatus::Done)
                && s.downloads
                    .iter()
                    .any(|d| d.file_name == "later.bin" && d.status == DownloadStatus::Paused)
        },
        Duration::from_secs(15),
    );
    let done_path = snap
        .downloads
        .iter()
        .find(|d| d.file_name == "done.bin")
        .unwrap()
        .save_path
        .clone();
    let later_id = snap
        .downloads
        .iter()
        .find(|d| d.file_name == "later.bin")
        .unwrap()
        .id;
    assert_eq!(snap.segments.get(&later_id).map(Vec::len), Some(2));

    handle.send(Command::RemoveCompleted).unwrap();
    let after = wait_for(&handle, |s| s.downloads.len() == 1, Duration::from_secs(5));
    assert_eq!(after.downloads[0].file_name, "later.bin");
    assert_eq!(after.downloads[0].status, DownloadStatus::Paused);
    assert!(done_path.exists());
    handle.shutdown();
}

#[test]
fn remove_with_file_deletes_saved_file_when_requested() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let settings = test_settings(dir.path());
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings,
    })
    .unwrap();

    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/file.bin", server.base),
            file_name: Some("erase.bin".to_string()),
            save_dir: None,
            start_now: true,
            connections: None,
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format: None,
            video_size: None,
            expected_sha256: None,
        }))
        .unwrap();

    let snap = wait_for(
        &handle,
        |s| {
            s.downloads
                .first()
                .is_some_and(|d| d.status == DownloadStatus::Done)
        },
        Duration::from_secs(15),
    );
    let id = snap.downloads[0].id;
    let saved = snap.downloads[0].save_path.clone();
    assert!(saved.exists());

    handle
        .send(Command::RemoveWithFile {
            id,
            delete_file: true,
        })
        .unwrap();
    wait_for(&handle, |s| s.downloads.is_empty(), Duration::from_secs(10));
    assert!(!saved.exists());
    handle.shutdown();
}

#[test]
fn move_rename_moves_completed_file_and_updates_store() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let settings = test_settings(dir.path());
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings,
    })
    .unwrap();

    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/file.bin", server.base),
            file_name: Some("orig.bin".to_string()),
            save_dir: None,
            start_now: true,
            connections: None,
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format: None,
            video_size: None,
            expected_sha256: None,
        }))
        .unwrap();

    let snap = wait_for(
        &handle,
        |s| {
            s.downloads
                .first()
                .is_some_and(|d| d.status == DownloadStatus::Done)
        },
        Duration::from_secs(15),
    );
    let id = snap.downloads[0].id;
    let old_path = snap.downloads[0].save_path.clone();
    let target_dir = dir.path().join("moved_folder");

    handle
        .send(Command::MoveRename {
            id,
            file_name: "renamed.bin".to_string(),
            dir: Some(target_dir.clone()),
        })
        .unwrap();

    let moved = wait_for(
        &handle,
        |s| {
            s.downloads
                .first()
                .is_some_and(|d| d.file_name == "renamed.bin")
        },
        Duration::from_secs(10),
    );
    let new_path = target_dir.join("renamed.bin");
    assert_eq!(moved.downloads[0].save_path, new_path);
    assert!(!old_path.exists());
    assert!(new_path.exists());
    handle.shutdown();
}

#[test]
fn update_url_and_resume_completes_with_refreshed_url() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let settings = test_settings(dir.path());
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings,
    })
    .unwrap();

    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/file.bin", server.base),
            file_name: Some("refreshed.bin".to_string()),
            save_dir: None,
            start_now: false,
            connections: None,
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format: None,
            video_size: None,
            expected_sha256: None,
        }))
        .unwrap();

    let snap = wait_for(
        &handle,
        |s| {
            s.downloads
                .first()
                .is_some_and(|d| d.status == DownloadStatus::Paused)
        },
        Duration::from_secs(15),
    );
    let id = snap.downloads[0].id;
    let refreshed_url = format!("{}/file.bin?token=fresh", server.base);
    handle
        .send(Command::UpdateUrlAndResume {
            id,
            url: refreshed_url.clone(),
        })
        .unwrap();

    let done = wait_for(
        &handle,
        |s| {
            s.downloads
                .first()
                .is_some_and(|d| d.status == DownloadStatus::Done)
        },
        Duration::from_secs(15),
    );
    assert_eq!(done.downloads[0].url, refreshed_url);
    assert!(done.downloads[0].save_path.exists());
    handle.shutdown();
}

#[test]
fn duplicate_policy_resume_and_numbered_behavior() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let mut settings = Settings {
        duplicate_policy: DuplicatePolicy::ResumeExisting,
        ..test_settings(dir.path())
    };
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings: settings.clone(),
    })
    .unwrap();

    let add_req = |url: String, start_now: bool| AddRequest {
        url,
        file_name: None,
        save_dir: None,
        start_now,
        connections: None,
        referrer: None,
        user_agent: None,
        cookie: None,
        video_format: None,
        video_size: None,
        expected_sha256: None,
    };

    let first_url = format!("{}/file.bin", server.base);
    handle
        .send(Command::Add(add_req(first_url.clone(), false)))
        .unwrap();
    let paused = wait_for(
        &handle,
        |s| {
            s.downloads
                .first()
                .is_some_and(|d| d.status == DownloadStatus::Paused)
        },
        Duration::from_secs(15),
    );
    let paused_id = paused.downloads[0].id;

    // Adding the same URL again with ResumeExisting should resume the paused row.
    handle
        .send(Command::Add(add_req(first_url, false)))
        .unwrap();
    let resumed = wait_for(
        &handle,
        |s| {
            s.downloads
                .first()
                .is_some_and(|d| d.status == DownloadStatus::Done)
        },
        Duration::from_secs(15),
    );
    assert_eq!(resumed.downloads.len(), 1);
    assert_eq!(resumed.downloads[0].id, paused_id);

    // Switch to RenameNumbered and enqueue the same URL twice without waiting for completion.
    settings.duplicate_policy = DuplicatePolicy::RenameNumbered;
    handle.send(Command::UpdateSettings(settings)).unwrap();
    let dup_url = format!("{}/slow-headers.bin", server.base);
    handle
        .send(Command::Add(add_req(dup_url.clone(), false)))
        .unwrap();
    let first_slow = wait_for(&handle, |s| s.downloads.len() == 2, Duration::from_secs(15));
    assert_eq!(first_slow.downloads.len(), 2);

    handle.send(Command::Add(add_req(dup_url, false))).unwrap();
    let both_slow = wait_for(&handle, |s| s.downloads.len() == 3, Duration::from_secs(15));
    let slow_rows: Vec<_> = both_slow
        .downloads
        .iter()
        .filter(|d| d.url.ends_with("/slow-headers.bin"))
        .collect();
    assert_eq!(slow_rows.len(), 2);
    assert_ne!(slow_rows[0].file_name, slow_rows[1].file_name);
    assert_ne!(slow_rows[0].save_path, slow_rows[1].save_path);
    handle.shutdown();
}

#[test]
fn wake_window_invokes_callback_and_updates_wake_seq() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let dir = tempfile::tempdir().unwrap();
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings: test_settings(dir.path()),
    })
    .unwrap();

    let wakes = Arc::new(AtomicUsize::new(0));
    let wakes_cb = Arc::clone(&wakes);
    handle.set_wake_callback(Arc::new(move || {
        wakes_cb.fetch_add(1, Ordering::SeqCst);
    }));

    handle.send(Command::WakeWindow).unwrap();
    let snap = wait_for(&handle, |s| s.wake_seq >= 1, Duration::from_secs(5));
    assert!(snap.wake_seq >= 1);
    assert!(wakes.load(Ordering::SeqCst) >= 1);
    handle.shutdown();
}

#[test]
fn category_dirs_routes_default_downloads_by_category() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let archives_dir = dir.path().join("archives");
    let mut settings = test_settings(dir.path());
    settings
        .category_dirs
        .insert("Compressed".to_string(), archives_dir.clone());

    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings,
    })
    .unwrap();

    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/file.bin", server.base),
            file_name: Some("bundle.zip".to_string()),
            save_dir: None,
            start_now: true,
            connections: None,
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format: None,
            video_size: None,
            expected_sha256: None,
        }))
        .unwrap();

    let done = wait_for(
        &handle,
        |s| {
            s.downloads
                .first()
                .is_some_and(|d| d.status == DownloadStatus::Done)
        },
        Duration::from_secs(15),
    );
    assert_eq!(done.downloads[0].category, Category::Compressed);
    assert_eq!(
        done.downloads[0].save_path.parent(),
        Some(archives_dir.as_path())
    );
    assert!(done.downloads[0].save_path.is_file());
    handle.shutdown();
}

#[test]
fn chunked_download_persists_final_byte_size() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings: test_settings(dir.path()),
    })
    .unwrap();

    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/chunked.bin", server.base),
            file_name: Some("stream.bin".to_string()),
            save_dir: None,
            start_now: true,
            connections: None,
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format: None,
            video_size: None,
            expected_sha256: None,
        }))
        .unwrap();

    let done = wait_for(
        &handle,
        |s| {
            s.downloads
                .first()
                .is_some_and(|d| d.status == DownloadStatus::Done)
        },
        Duration::from_secs(15),
    );
    assert_eq!(done.downloads[0].size, Some(16_384));
    assert_eq!(done.downloads[0].downloaded, 16_384);
    handle.shutdown();
}

#[test]
fn redownload_preserves_encrypted_cookie_and_headers() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings: test_settings(dir.path()),
    })
    .unwrap();

    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/auth.bin", server.base),
            file_name: Some("secret.bin".to_string()),
            save_dir: None,
            start_now: true,
            connections: None,
            referrer: Some("https://example.test/page".to_string()),
            user_agent: Some("LajuFileTest/1.0".to_string()),
            cookie: Some("session=abc".to_string()),
            video_format: None,
            video_size: None,
            expected_sha256: None,
        }))
        .unwrap();

    let first = wait_for(
        &handle,
        |s| {
            s.downloads
                .first()
                .is_some_and(|d| d.status == DownloadStatus::Done)
        },
        Duration::from_secs(15),
    );
    let first_id = first.downloads[0].id;

    handle.send(Command::Redownload(first_id)).unwrap();
    let second = wait_for(
        &handle,
        |s| s.downloads.len() == 2 && s.downloads.iter().all(|d| d.status == DownloadStatus::Done),
        Duration::from_secs(15),
    );
    assert_eq!(second.downloads.len(), 2);
    assert_ne!(second.downloads[0].save_path, second.downloads[1].save_path);
    handle.shutdown();
}

#[test]
fn redownload_on_paused_download_succeeds_under_reject_policy() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let mut settings = test_settings(dir.path());
    settings.duplicate_policy = DuplicatePolicy::Reject;
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings,
    })
    .unwrap();

    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/file.bin", server.base),
            file_name: Some("paused.bin".to_string()),
            save_dir: None,
            start_now: false,
            connections: None,
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format: None,
            video_size: None,
            expected_sha256: None,
        }))
        .unwrap();

    let first = wait_for(
        &handle,
        |s| {
            s.downloads
                .first()
                .is_some_and(|d| d.status == DownloadStatus::Paused)
        },
        Duration::from_secs(15),
    );
    let paused_id = first.downloads[0].id;

    handle.send(Command::Redownload(paused_id)).unwrap();
    let second = wait_for(
        &handle,
        |s| s.downloads.len() == 2 && s.downloads.iter().any(|d| d.status == DownloadStatus::Done),
        Duration::from_secs(15),
    );
    assert_eq!(second.downloads.len(), 2);
    assert_ne!(second.downloads[0].save_path, second.downloads[1].save_path);
    handle.shutdown();
}

#[test]
fn one_time_200_url_streams_directly_from_probe_without_second_request() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings: test_settings(dir.path()),
    })
    .unwrap();

    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/once.bin", server.base),
            file_name: None,
            save_dir: None,
            start_now: true,
            connections: None,
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format: None,
            video_size: None,
            expected_sha256: None,
        }))
        .unwrap();

    let done = wait_for(
        &handle,
        |s| {
            s.downloads
                .first()
                .is_some_and(|d| d.status == DownloadStatus::Done)
        },
        Duration::from_secs(15),
    );
    let saved = std::fs::read(&done.downloads[0].save_path).unwrap();
    assert_eq!(saved, b"one-time-payload-bytes-0123456789".repeat(128));
    assert_eq!(server.once_hits(), 1);
    handle.shutdown();
}

#[test]
fn move_to_top_and_force_start_prioritize_queued_downloads() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let mut settings = test_settings(dir.path());
    settings.max_concurrent = 1;
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings,
    })
    .unwrap();

    for (q, name) in [("first", "first.bin"), ("second", "second.bin")] {
        handle
            .send(Command::Add(AddRequest {
                url: format!("{}/slow.bin?{q}", server.base),
                file_name: Some(name.to_string()),
                save_dir: None,
                start_now: true,
                connections: None,
                referrer: None,
                user_agent: None,
                cookie: None,
                video_format: None,
                video_size: None,
                expected_sha256: None,
            }))
            .unwrap();
    }

    let snap = wait_for(
        &handle,
        |s| {
            s.downloads.len() == 2
                && s.downloads
                    .iter()
                    .any(|d| d.status == DownloadStatus::Downloading)
                && s.downloads
                    .iter()
                    .any(|d| d.status == DownloadStatus::Queued)
        },
        Duration::from_secs(15),
    );
    let queued_id = snap
        .downloads
        .iter()
        .find(|d| d.status == DownloadStatus::Queued)
        .unwrap()
        .id;

    // ForceStart bypasses max_concurrent=1 and moves the row to top of queue order.
    handle.send(Command::ForceStart(queued_id)).unwrap();
    let forced = wait_for(
        &handle,
        |s| {
            s.downloads.len() == 2
                && s.downloads
                    .iter()
                    .find(|d| d.id == queued_id)
                    .is_some_and(|d| {
                        matches!(d.status, DownloadStatus::Downloading | DownloadStatus::Done)
                    })
                && s.downloads[0].id == queued_id
        },
        Duration::from_secs(15),
    );
    assert_eq!(forced.downloads[0].id, queued_id);
    handle.shutdown();
}

#[test]
fn auto_verify_sha256_marks_verified_on_match_and_error_on_mismatch() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings: test_settings(dir.path()),
    })
    .unwrap();

    let expected_ok = lajufile_core::checksum::sha256_bytes(&b"LajuFile".repeat(1024 * 128));
    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/file.bin?good", server.base),
            file_name: Some("good.bin".to_string()),
            save_dir: None,
            start_now: true,
            connections: None,
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format: None,
            video_size: None,
            expected_sha256: Some(format!("sha256:{expected_ok}")),
        }))
        .unwrap();

    let good = wait_for(
        &handle,
        |s| {
            s.downloads
                .iter()
                .find(|d| d.file_name == "good.bin")
                .is_some_and(|d| d.checksum_status.as_deref() == Some("verified"))
        },
        Duration::from_secs(15),
    );
    let good_row = good
        .downloads
        .iter()
        .find(|d| d.file_name == "good.bin")
        .unwrap();
    assert_eq!(good_row.status, DownloadStatus::Done);
    assert!(good_row.save_path.exists());

    // Wrong digest marks Error + mismatch while preserving the downloaded file on disk.
    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/file.bin?bad", server.base),
            file_name: Some("bad.bin".to_string()),
            save_dir: None,
            start_now: true,
            connections: None,
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format: None,
            video_size: None,
            expected_sha256: Some("0".repeat(64)),
        }))
        .unwrap();

    let bad = wait_for(
        &handle,
        |s| {
            s.downloads
                .iter()
                .find(|d| d.file_name == "bad.bin")
                .is_some_and(|d| d.checksum_status.as_deref() == Some("mismatch"))
        },
        Duration::from_secs(15),
    );
    let bad_row = bad
        .downloads
        .iter()
        .find(|d| d.file_name == "bad.bin")
        .unwrap();
    assert_eq!(bad_row.status, DownloadStatus::Error);
    assert!(bad_row.save_path.exists());
    handle.shutdown();
}

#[test]
fn per_item_speed_limit_persists_and_applies_live() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings: test_settings(dir.path()),
    })
    .unwrap();

    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/file.bin", server.base),
            file_name: Some("capped.bin".to_string()),
            save_dir: None,
            start_now: false,
            connections: None,
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format: None,
            video_size: None,
            expected_sha256: None,
        }))
        .unwrap();

    let paused = wait_for(&handle, |s| s.downloads.len() == 1, Duration::from_secs(15));
    let id = paused.downloads[0].id;
    handle
        .send(Command::SetItemSpeedLimit { id, kbps: 256 })
        .unwrap();
    let updated = wait_for(
        &handle,
        |s| s.downloads[0].speed_limit_kbps == 256,
        Duration::from_secs(15),
    );
    assert_eq!(updated.downloads[0].speed_limit_kbps, 256);
    handle.shutdown();
}

#[test]
fn scheduler_holds_new_downloads_outside_window_until_schedule_opens() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let now_mins = lajufile_app::platform::local_minutes_since_midnight().unwrap();
    let start_mins = (now_mins + 120) % 1440;
    let stop_mins = (now_mins + 180) % 1440;
    let fmt_hhmm = |m: u16| format!("{:02}:{:02}", m / 60, m % 60);

    let mut settings = test_settings(dir.path());
    settings.schedule_enabled = true;
    settings.schedule_start = fmt_hhmm(start_mins);
    settings.schedule_stop = fmt_hhmm(stop_mins);
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings: settings.clone(),
    })
    .unwrap();

    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/file.bin", server.base),
            file_name: Some("scheduled.bin".to_string()),
            save_dir: None,
            start_now: true,
            connections: None,
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format: None,
            video_size: None,
            expected_sha256: None,
        }))
        .unwrap();

    // Must remain queued while outside the scheduled window.
    let queued = wait_for(
        &handle,
        |s| s.downloads.len() == 1 && s.downloads[0].status == DownloadStatus::Queued,
        Duration::from_secs(15),
    );
    std::thread::sleep(Duration::from_millis(300));
    let still_queued = handle.latest_snapshot().unwrap();
    assert_eq!(still_queued.downloads[0].id, queued.downloads[0].id);
    assert_eq!(still_queued.downloads[0].status, DownloadStatus::Queued);

    // Updating the schedule window to include now starts the queued download immediately.
    let mut open_settings = settings;
    open_settings.schedule_start = fmt_hhmm((now_mins + 1440 - 30) % 1440);
    open_settings.schedule_stop = fmt_hhmm((now_mins + 60) % 1440);
    handle.send(Command::UpdateSettings(open_settings)).unwrap();

    let done = wait_for(
        &handle,
        |s| s.downloads.len() == 1 && s.downloads[0].status == DownloadStatus::Done,
        Duration::from_secs(15),
    );
    assert!(done.downloads[0].save_path.exists());
    handle.shutdown();
}

#[test]
fn queue_complete_action_waits_for_checksum_post_task_and_resets_one_shot() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let mut settings = test_settings(dir.path());
    settings.on_queue_complete = lajufile_core::settings::QueueCompleteAction::ExitApp;
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings,
    })
    .unwrap();

    let expected = lajufile_core::checksum::sha256_bytes(&b"LajuFile".repeat(1024 * 128));
    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/file.bin", server.base),
            file_name: Some("post-task.bin".to_string()),
            save_dir: None,
            start_now: true,
            connections: None,
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format: None,
            video_size: None,
            expected_sha256: Some(expected),
        }))
        .unwrap();

    let armed = wait_for(
        &handle,
        |s| {
            matches!(
                s.pending_queue_action,
                Some((lajufile_core::settings::QueueCompleteAction::ExitApp, _))
            )
        },
        Duration::from_secs(15),
    );
    assert_eq!(
        armed.downloads[0].checksum_status.as_deref(),
        Some("verified"),
        "queue completion action must not arm before post-download checksum completes"
    );
    assert_eq!(
        armed.settings.on_queue_complete,
        lajufile_core::settings::QueueCompleteAction::None
    );

    handle.send(Command::CancelQueueComplete).unwrap();
    let canceled = wait_for(
        &handle,
        |s| s.pending_queue_action.is_none(),
        Duration::from_secs(5),
    );
    assert!(!canceled.exit_requested);
    handle.shutdown();
}
