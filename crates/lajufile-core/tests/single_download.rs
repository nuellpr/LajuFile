use std::time::Duration;

use lajufile_core::download::{DownloadSpec, download_single};
use lajufile_core::http::{DEFAULT_USER_AGENT, RequestOptions, build_client};
use lajufile_core::naming::{dedupe_path, resolve_file_name, temp_path_for};
use lajufile_core::probe::{ProbeRequest, probe};
use lajufile_core::store::{Category, DownloadStatus, NewDownload, Segment, Store, now_millis};
use url::Url;

mod support;

#[tokio::test]
async fn probe_insert_download_and_finish() {
    let base = support::start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let download_dir = dir.path().join("downloads");
    std::fs::create_dir_all(&download_dir).unwrap();

    let mut store = Store::open(&dir.path().join("lajufile.db")).unwrap();
    let client = build_client(Duration::from_secs(5), Duration::from_secs(60)).unwrap();
    let probe_req = ProbeRequest {
        url: Url::parse(&format!("{base}/range")).unwrap(),
        options: RequestOptions::default(),
        timeout: Duration::from_secs(2),
    };
    let meta = probe(&client, &probe_req).await.unwrap();
    assert_eq!(meta.size, Some(1_048_576));

    let name = resolve_file_name(None, meta.content_disposition.as_deref(), &meta.final_url);
    assert_eq!(name, "ubuntu test.iso");
    let final_path = dedupe_path(&download_dir, &name);
    let temp_path = temp_path_for(&final_path);
    let segment = Segment {
        idx: 0,
        start: 0,
        end: meta.size.unwrap() - 1,
        done: 0,
    };
    let new = NewDownload {
        url: probe_req.url.to_string(),
        final_url: Some(meta.final_url.to_string()),
        file_name: name.clone(),
        save_path: final_path.clone(),
        temp_path: temp_path.clone(),
        size: meta.size,
        status: DownloadStatus::Queued,
        category: Category::from_file_name(&name),
        connections: 1,
        ranged: meta.ranged,
        etag: meta.etag.clone(),
        last_modified: meta.last_modified.clone(),
        cookie_enc: None,
        referrer: None,
        user_agent: Some(DEFAULT_USER_AGENT.to_string()),
        queue_order: 1,
        created_at: now_millis(),
        segments: vec![segment],
    };
    let id = store.insert_download(&new).unwrap();
    store
        .update_status(id, DownloadStatus::Downloading, None)
        .unwrap();

    let spec = DownloadSpec {
        url: meta.final_url.clone(),
        temp_path,
        final_path: final_path.clone(),
        expected_size: meta.size,
        options: RequestOptions::default(),
    };
    let outcome = download_single(&client, &spec, |_| {}).await.unwrap();
    store.update_progress(id, 0, outcome.bytes).unwrap();
    store.mark_done(id, now_millis()).unwrap();

    let row = store.get_download(id).unwrap();
    assert_eq!(row.status, DownloadStatus::Done);
    assert_eq!(row.downloaded, 1_048_576);
    assert!(row.completed_at.is_some());
    assert_eq!(std::fs::read(&final_path).unwrap(), support::payload());
    assert!(!spec.temp_path.exists());
    assert_eq!(store.active_by_url(&row.url).unwrap(), None);
}
