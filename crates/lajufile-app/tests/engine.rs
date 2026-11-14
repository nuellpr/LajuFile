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
            url: format!("{}/file.bin", server.base),
            file_name: None,
            save_dir: None,
            start_now: false,
        }))
        .unwrap();

    let snap = wait_for(&handle, |s| s.downloads.len() == 1, Duration::from_secs(5));
    assert_eq!(snap.downloads[0].file_name, "file.bin");
    assert_eq!(snap.downloads[0].status, DownloadStatus::Queued);

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
        url: format!("{}/file.bin", server.base),
        file_name: None,
        save_dir: None,
        start_now: false,
    };
    handle.send(Command::Add(request)).unwrap();
    let first = wait_for(&handle, |s| s.downloads.len() == 1, Duration::from_secs(5));
    let first_id = first.downloads[0].id;

    handle
        .send(Command::Add(AddRequest {
            url: format!("{}/file.bin", server.base),
            file_name: None,
            save_dir: None,
            start_now: false,
        }))
        .unwrap();
    let second = wait_for(
        &handle,
        |s| s.message.as_deref().is_some_and(|m| m.contains("Already")),
        Duration::from_secs(5),
    );

    assert_eq!(second.downloads.len(), 1);
    assert_eq!(second.downloads[0].id, first_id);
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
