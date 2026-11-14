mod support;

use std::time::{Duration, Instant};

use lajufile_app::engine::{AddRequest, Command, EngineConfig, EngineHandle, Snapshot};
use lajufile_core::settings::Settings;
use lajufile_core::store::{DownloadStatus, Store};
use support::start_server;

#[test]
fn add_probes_and_inserts() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings {
        download_dir: dir.path().join("downloads"),
        ..Settings::default()
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
        }))
        .unwrap();

    let snap = wait_for(&handle, |s| s.downloads.len() == 1, Duration::from_secs(5));
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
        ..Settings::default()
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
        }))
        .unwrap();

    let snap = wait_for(
        &handle,
        |s| s.message.as_deref().is_some_and(|m| m.contains("failed")),
        Duration::from_secs(5),
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
        ..Settings::default()
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
    };
    handle.send(Command::Add(request)).unwrap();
    let first = wait_for(
        &handle,
        |s| {
            s.downloads
                .first()
                .is_some_and(|d| d.status == DownloadStatus::Downloading)
        },
        Duration::from_secs(5),
    );
    let first_id = first.downloads[0].id;

    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/slow.bin", server.base),
            file_name: None,
            save_dir: None,
            start_now: true,
        }))
        .unwrap();
    let second = wait_for(
        &handle,
        |s| s.message.as_deref().is_some_and(|m| m.contains("Already")),
        Duration::from_secs(5),
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
        ..Settings::default()
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
            }))
            .unwrap();
    }

    let snap = wait_for(
        &handle,
        |s| s.downloads.len() == 1 && s.message.as_deref().is_some_and(|m| m.contains("Already")),
        Duration::from_secs(5),
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
        ..Settings::default()
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
        ..Settings::default()
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
        ..Settings::default()
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
        Duration::from_secs(5),
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
        ..Settings::default()
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
        }))
        .unwrap();
    let snap = wait_for(
        &handle,
        |s| {
            s.downloads
                .first()
                .is_some_and(|d| d.status == DownloadStatus::Downloading)
        },
        Duration::from_secs(5),
    );
    let id = snap.downloads[0].id;

    handle.send(Command::Pause(id)).unwrap();
    wait_for(
        &handle,
        |s| s.downloads[0].status == DownloadStatus::Paused,
        Duration::from_secs(5),
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
        ..Settings::default()
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
        }))
        .unwrap();
    let snap = wait_for(
        &handle,
        |s| {
            s.downloads
                .first()
                .is_some_and(|d| d.status == DownloadStatus::Downloading)
        },
        Duration::from_secs(5),
    );
    let id = snap.downloads[0].id;
    let temp_path = snap.downloads[0].temp_path.clone();

    handle.send(Command::Remove(id)).unwrap();
    wait_for(&handle, |s| s.downloads.is_empty(), Duration::from_secs(5));
    std::thread::sleep(Duration::from_millis(600));
    assert!(handle.latest_snapshot().unwrap().downloads.is_empty());

    handle.shutdown();
    let store = Store::open(&dir.path().join("lajufile.db")).unwrap();
    assert!(store.list_downloads().unwrap().is_empty());
    assert!(!temp_path.exists());
}
