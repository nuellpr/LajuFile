mod support;

use std::path::Path;
use std::time::{Duration, Instant};

use lajufile_app::engine::{AddRequest, Command, EngineConfig, EngineHandle, Snapshot};
use lajufile_core::settings::Settings;
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
        }))
        .unwrap();
    let snap = wait_for(
        &handle,
        |s| {
            s.downloads.first().is_some_and(|d| {
                d.status == DownloadStatus::Downloading && d.downloaded >= 256 * 1024
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
    assert!(segments.iter().all(|segment| segment.done > 0));
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
            url: format!("{}/slow-seg.bin", server.base),
            file_name: None,
            save_dir: None,
            start_now: true,
            connections: None,
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format: None,
        }))
        .unwrap();
    let snap = wait_for(
        &handle,
        |s| s.downloads.first().is_some_and(|d| d.downloaded > 0),
        Duration::from_secs(15),
    );
    let id = snap.downloads[0].id;
    std::thread::sleep(Duration::from_millis(100));

    handle.send(Command::Pause(id)).unwrap();
    wait_for(
        &handle,
        |s| {
            s.downloads
                .first()
                .is_some_and(|d| d.status == DownloadStatus::Paused)
        },
        Duration::from_secs(15),
    );
    let ranges_before = server.ranges().len();

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
