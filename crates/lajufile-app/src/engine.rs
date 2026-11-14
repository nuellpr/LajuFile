use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use lajufile_core::http::{DEFAULT_USER_AGENT, RequestOptions, build_client};
use lajufile_core::naming::{dedupe_path, resolve_file_name, temp_path_for};
use lajufile_core::paths::ensure_dir;
use lajufile_core::probe::{ProbeError, ProbeRequest, ProbeResult, probe};
use lajufile_core::settings::Settings;
use lajufile_core::store::{
    Category, Download, DownloadStatus, NewDownload, Segment, Store, StoreError, now_millis,
};
use tokio::runtime::Runtime;
use url::Url;

pub struct EngineConfig {
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub settings: Settings,
}

pub struct EngineHandle {
    commands: Sender<Command>,
    snapshots: Receiver<Snapshot>,
    join: Option<JoinHandle<()>>,
}

impl EngineHandle {
    pub fn start(config: EngineConfig) -> Result<Self, EngineError> {
        let store = Store::open(&config.db_path)?;
        let (command_tx, command_rx) = mpsc::channel();
        let (snapshot_tx, snapshot_rx) = mpsc::channel();

        let join = thread::Builder::new()
            .name("lajufile-engine".to_string())
            .spawn(move || engine_loop(store, config, command_rx, snapshot_tx))?;

        Ok(Self {
            commands: command_tx,
            snapshots: snapshot_rx,
            join: Some(join),
        })
    }

    pub fn send(&self, command: Command) -> Result<(), EngineError> {
        self.commands
            .send(command)
            .map_err(|_| EngineError::Stopped)
    }

    pub fn latest_snapshot(&self) -> Option<Snapshot> {
        let mut latest = None;
        while let Ok(snapshot) = self.snapshots.try_recv() {
            latest = Some(snapshot);
        }
        latest
    }

    pub fn shutdown(mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

#[derive(Debug)]
pub enum Command {
    Add(AddRequest),
    Pause(i64),
    Resume(i64),
    Remove(i64),
    PauseAll,
    ResumeAll,
    UpdateSettings(Settings),
    Shutdown,
}

#[derive(Debug, Clone)]
pub struct AddRequest {
    pub url: String,
    pub file_name: Option<String>,
    pub save_dir: Option<PathBuf>,
    pub start_now: bool,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub downloads: Vec<Download>,
    pub settings: Settings,
    pub speeds: HashMap<i64, u64>,
    pub message: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("store: {0}")]
    Store(#[from] StoreError),
    #[error("engine thread failed to start: {0}")]
    Thread(#[from] std::io::Error),
    #[error("engine stopped")]
    Stopped,
}

enum Event {
    Probe {
        request: AddRequest,
        result: Result<ProbeResult, ProbeError>,
    },
}

fn engine_loop(
    mut store: Store,
    config: EngineConfig,
    commands: Receiver<Command>,
    snapshots: Sender<Snapshot>,
) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("build engine runtime");
    let client = match build_client(Duration::from_secs(10), Duration::from_secs(30)) {
        Ok(client) => client,
        Err(error) => {
            publish(
                &store,
                &config.settings,
                Some(format!("Engine failed: {error}")),
                &snapshots,
            );
            return;
        }
    };

    normalize_startup(&mut store, &config.settings);
    let (event_tx, events) = mpsc::channel();
    let mut message = None;
    publish(&store, &config.settings, None, &snapshots);
    let mut last_publish = Instant::now();

    loop {
        while let Ok(command) = commands.try_recv() {
            match command {
                Command::Add(request) => {
                    if let Some(text) =
                        handle_add(&mut store, &client, &runtime, &event_tx, request)
                    {
                        message = Some(text);
                    }
                }
                Command::Shutdown => {
                    publish(&store, &config.settings, message, &snapshots);
                    return;
                }
                _ => {}
            }
        }

        while let Ok(event) = events.try_recv() {
            match event {
                Event::Probe { request, result } => {
                    message = Some(handle_probe(&mut store, &config.settings, request, result));
                }
            }
        }

        if last_publish.elapsed() >= Duration::from_millis(250) {
            publish(&store, &config.settings, message.take(), &snapshots);
            last_publish = Instant::now();
        }

        thread::sleep(Duration::from_millis(25));
    }
}

fn normalize_startup(store: &mut Store, settings: &Settings) {
    let Ok(downloads) = store.list_downloads() else {
        return;
    };
    for download in downloads {
        if download.status == DownloadStatus::Downloading {
            let status = if settings.auto_resume {
                DownloadStatus::Queued
            } else {
                DownloadStatus::Paused
            };
            let _ = store.update_status(download.id, status, None);
        }
    }
}

fn handle_add(
    store: &mut Store,
    client: &reqwest::Client,
    runtime: &Runtime,
    events: &Sender<Event>,
    request: AddRequest,
) -> Option<String> {
    let url = match Url::parse(&request.url) {
        Ok(url) => url,
        Err(error) => return Some(format!("Add failed: {error}")),
    };

    match store.active_by_url(url.as_str()) {
        Ok(Some(existing)) => {
            return Some(format!("Already in LajuFile: {}", existing.file_name));
        }
        Ok(None) => {}
        Err(error) => return Some(format!("Add failed: {error}")),
    }

    let probe_request = ProbeRequest {
        url,
        options: RequestOptions {
            user_agent: Some(DEFAULT_USER_AGENT.to_string()),
            ..RequestOptions::default()
        },
        timeout: Duration::from_secs(20),
    };
    let client = client.clone();
    let events = events.clone();
    runtime.spawn(async move {
        let result = probe(&client, &probe_request).await;
        let _ = events.send(Event::Probe { request, result });
    });
    None
}

fn handle_probe(
    store: &mut Store,
    settings: &Settings,
    request: AddRequest,
    result: Result<ProbeResult, ProbeError>,
) -> String {
    let result = match result {
        Ok(result) => result,
        Err(error) => return format!("Add failed: {error}"),
    };

    let dir = request
        .save_dir
        .clone()
        .unwrap_or_else(|| settings.download_dir.clone());
    if let Err(error) = ensure_dir(&dir) {
        return format!("Add failed: {error}");
    }

    let file_name = resolve_file_name(
        request.file_name.as_deref(),
        result.content_disposition.as_deref(),
        &result.final_url,
    );
    let final_path = dedupe_path(&dir, &file_name);
    let file_name = final_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or(file_name);
    let temp_path = temp_path_for(&final_path);
    let end = result.size.unwrap_or(1).saturating_sub(1);

    let new = NewDownload {
        url: request.url,
        final_url: Some(result.final_url.to_string()),
        file_name: file_name.clone(),
        save_path: final_path,
        temp_path,
        size: result.size,
        status: DownloadStatus::Queued,
        category: Category::from_file_name(&file_name),
        connections: 1,
        ranged: result.ranged,
        etag: result.etag,
        last_modified: result.last_modified,
        cookie_enc: None,
        referrer: None,
        user_agent: Some(DEFAULT_USER_AGENT.to_string()),
        queue_order: match store.next_queue_order() {
            Ok(order) => order,
            Err(error) => return format!("Add failed: {error}"),
        },
        created_at: now_millis(),
        segments: vec![Segment {
            idx: 0,
            start: 0,
            end,
            done: 0,
        }],
    };

    match store.insert_download(&new) {
        Ok(_) => format!("Added {file_name}"),
        Err(error) => format!("Add failed: {error}"),
    }
}

fn publish(store: &Store, settings: &Settings, message: Option<String>, output: &Sender<Snapshot>) {
    let Ok(downloads) = store.list_downloads() else {
        return;
    };
    let _ = output.send(Snapshot {
        downloads,
        settings: settings.clone(),
        speeds: HashMap::new(),
        message,
    });
}
