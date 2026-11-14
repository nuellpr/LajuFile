use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use lajufile_core::download::{DownloadError, DownloadOutcome, DownloadSpec, download_single};
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
        result: Box<Result<ProbeResult, ProbeError>>,
    },
    Progress {
        id: i64,
        generation: u64,
        done: u64,
    },
    Finished {
        id: i64,
        generation: u64,
        result: Result<DownloadOutcome, DownloadError>,
    },
}

struct ActiveDownload {
    generation: u64,
    handle: tokio::task::JoinHandle<()>,
}

struct EngineState {
    store: Store,
    settings: Settings,
    data_dir: PathBuf,
    client: reqwest::Client,
    runtime: Runtime,
    events: Sender<Event>,
    snapshots: Sender<Snapshot>,
    active: HashMap<i64, ActiveDownload>,
    cancelled: HashSet<(i64, u64)>,
    generation: u64,
    speeds: HashMap<i64, u64>,
    progress: HashMap<i64, (u64, Instant)>,
    last_persist: HashMap<i64, Instant>,
    message: Option<String>,
}

fn engine_loop(
    store: Store,
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
            let _ = snapshots.send(Snapshot {
                downloads: store.list_downloads().unwrap_or_default(),
                settings: config.settings,
                speeds: HashMap::new(),
                message: Some(format!("Engine failed: {error}")),
            });
            return;
        }
    };

    let (event_tx, event_rx) = mpsc::channel();
    let mut state = EngineState {
        store,
        settings: config.settings,
        data_dir: config.data_dir,
        client,
        runtime,
        events: event_tx,
        snapshots,
        active: HashMap::new(),
        cancelled: HashSet::new(),
        generation: 0,
        speeds: HashMap::new(),
        progress: HashMap::new(),
        last_persist: HashMap::new(),
        message: None,
    };

    state.normalize_startup();
    state.pump_queue();
    state.publish();
    let mut last_publish = Instant::now();

    loop {
        while let Ok(command) = commands.try_recv() {
            if state.handle_command(command) {
                state.publish();
                return;
            }
        }

        while let Ok(event) = event_rx.try_recv() {
            state.handle_event(event);
        }

        if last_publish.elapsed() >= Duration::from_millis(250) {
            state.publish();
            last_publish = Instant::now();
        }

        thread::sleep(Duration::from_millis(25));
    }
}

impl EngineState {
    fn normalize_startup(&mut self) {
        let Ok(downloads) = self.store.list_downloads() else {
            return;
        };
        for download in downloads {
            if download.status == DownloadStatus::Downloading {
                let status = if self.settings.auto_resume {
                    DownloadStatus::Queued
                } else {
                    DownloadStatus::Paused
                };
                let _ = self.store.update_status(download.id, status, None);
            }
        }
    }

    fn handle_command(&mut self, command: Command) -> bool {
        match command {
            Command::Add(request) => {
                if let Some(text) = self.handle_add(request) {
                    self.message = Some(text);
                }
            }
            Command::Pause(id) => self.pause(id),
            Command::Resume(id) => self.resume(id),
            Command::Remove(id) => self.remove(id),
            Command::PauseAll => self.pause_all(),
            Command::ResumeAll => self.resume_all(),
            Command::UpdateSettings(settings) => self.update_settings(settings),
            Command::Shutdown => {
                self.shutdown();
                return true;
            }
        }
        false
    }

    fn handle_add(&mut self, request: AddRequest) -> Option<String> {
        let url = match Url::parse(&request.url) {
            Ok(url) => url,
            Err(error) => return Some(format!("Add failed: {error}")),
        };

        match self.store.active_by_url(url.as_str()) {
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
        let client = self.client.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let result = probe(&client, &probe_request).await;
            let _ = events.send(Event::Probe {
                request,
                result: Box::new(result),
            });
        });
        None
    }

    fn handle_probe(
        &mut self,
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
            .unwrap_or_else(|| self.settings.download_dir.clone());
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
        let start_now = request.start_now;

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
            queue_order: match self.store.next_queue_order() {
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

        match self.store.insert_download(&new) {
            Ok(_) => {
                if start_now {
                    self.pump_queue();
                }
                format!("Added {file_name}")
            }
            Err(error) => format!("Add failed: {error}"),
        }
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Probe { request, result } => {
                self.message = Some(self.handle_probe(request, *result));
            }
            Event::Progress {
                id,
                generation,
                done,
            } => {
                if self.active.get(&id).map(|active| active.generation) != Some(generation) {
                    return;
                }
                let now = Instant::now();
                if let Some((previous, previous_time)) = self.progress.get(&id) {
                    let seconds = now.duration_since(*previous_time).as_secs_f64();
                    if seconds >= 0.1 && done >= *previous {
                        self.speeds
                            .insert(id, ((done - *previous) as f64 / seconds) as u64);
                    }
                }
                self.progress.insert(id, (done, now));
                let should_persist = self
                    .last_persist
                    .get(&id)
                    .is_none_or(|at| at.elapsed() >= Duration::from_secs(1));
                if should_persist {
                    let _ = self.store.update_progress(id, 0, done);
                    self.last_persist.insert(id, now);
                }
            }
            Event::Finished {
                id,
                generation,
                result,
            } => {
                let Some(active) = self.active.get(&id) else {
                    return;
                };
                if active.generation != generation {
                    return;
                }
                self.active.remove(&id);
                if self.cancelled.remove(&(id, generation)) {
                    self.pump_queue();
                    return;
                }
                match result {
                    Ok(_) => {
                        let _ = self.store.mark_done(id, now_millis());
                    }
                    Err(error) => {
                        let _ = self.store.update_status(
                            id,
                            DownloadStatus::Error,
                            Some(&error.to_string()),
                        );
                    }
                }
                self.speeds.remove(&id);
                self.progress.remove(&id);
                self.last_persist.remove(&id);
                self.pump_queue();
            }
        }
    }

    fn start_download(&mut self, download: &Download) -> bool {
        if self.active.contains_key(&download.id) {
            return false;
        }
        let url = match Url::parse(&download.url) {
            Ok(url) => url,
            Err(error) => {
                let _ = self.store.update_status(
                    download.id,
                    DownloadStatus::Error,
                    Some(&format!("invalid URL: {error}")),
                );
                return false;
            }
        };

        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        if self
            .store
            .update_status(download.id, DownloadStatus::Downloading, None)
            .is_err()
        {
            return false;
        }

        let spec = DownloadSpec {
            url,
            temp_path: download.temp_path.clone(),
            final_path: download.save_path.clone(),
            expected_size: download.size,
            options: RequestOptions {
                user_agent: Some(DEFAULT_USER_AGENT.to_string()),
                referrer: download.referrer.clone(),
                cookie: None,
            },
        };
        let id = download.id;
        let client = self.client.clone();
        let events = self.events.clone();
        let handle = self.runtime.spawn(async move {
            let result = download_single(&client, &spec, |done| {
                let _ = events.send(Event::Progress {
                    id,
                    generation,
                    done,
                });
            })
            .await;
            let _ = events.send(Event::Finished {
                id,
                generation,
                result,
            });
        });
        self.active
            .insert(id, ActiveDownload { generation, handle });
        true
    }

    fn pump_queue(&mut self) {
        let limit = self.settings.max_concurrent.max(1) as usize;
        loop {
            if self.active.len() >= limit {
                break;
            }
            let next = match self.store.list_downloads() {
                Ok(downloads) => downloads
                    .into_iter()
                    .filter(|download| {
                        download.status == DownloadStatus::Queued
                            && !self.active.contains_key(&download.id)
                    })
                    .min_by_key(|download| download.queue_order),
                Err(_) => None,
            };
            let Some(download) = next else {
                break;
            };
            if !self.start_download(&download) {
                break;
            }
        }
    }

    fn pause(&mut self, id: i64) {
        self.pause_with_pump(id, true);
    }

    fn pause_with_pump(&mut self, id: i64, pump: bool) {
        if let Some(active) = self.active.remove(&id) {
            self.cancelled.insert((id, active.generation));
            active.handle.abort();
            self.speeds.remove(&id);
            self.progress.remove(&id);
            self.last_persist.remove(&id);
        }
        if let Ok(download) = self.store.get_download(id) {
            if matches!(
                download.status,
                DownloadStatus::Downloading | DownloadStatus::Queued
            ) {
                let _ = self.store.update_status(id, DownloadStatus::Paused, None);
            }
        }
        if pump {
            self.pump_queue();
        }
    }

    fn resume(&mut self, id: i64) {
        if let Ok(download) = self.store.get_download(id) {
            if matches!(
                download.status,
                DownloadStatus::Paused | DownloadStatus::Error | DownloadStatus::Queued
            ) {
                let _ = self.store.update_status(id, DownloadStatus::Queued, None);
            }
        }
        self.pump_queue();
    }

    fn remove(&mut self, id: i64) {
        if let Some(active) = self.active.remove(&id) {
            self.cancelled.insert((id, active.generation));
            active.handle.abort();
        }
        let temp_path = self
            .store
            .get_download(id)
            .ok()
            .map(|download| download.temp_path);
        if self.store.delete_download(id).is_ok() {
            if let Some(temp_path) = temp_path {
                let _ = std::fs::remove_file(temp_path);
            }
        }
        self.speeds.remove(&id);
        self.progress.remove(&id);
        self.last_persist.remove(&id);
        self.pump_queue();
    }

    fn pause_all(&mut self) {
        let ids: Vec<i64> = self.active.keys().copied().collect();
        let queued: Vec<i64> = self
            .store
            .list_downloads()
            .unwrap_or_default()
            .into_iter()
            .filter(|download| download.status == DownloadStatus::Queued)
            .map(|download| download.id)
            .collect();
        for id in ids.into_iter().chain(queued) {
            self.pause_with_pump(id, false);
        }
    }

    fn resume_all(&mut self) {
        let paused: Vec<i64> = self
            .store
            .list_downloads()
            .unwrap_or_default()
            .into_iter()
            .filter(|download| {
                matches!(
                    download.status,
                    DownloadStatus::Paused | DownloadStatus::Error | DownloadStatus::Queued
                )
            })
            .map(|download| download.id)
            .collect();
        for id in paused {
            let _ = self.store.update_status(id, DownloadStatus::Queued, None);
        }
        self.pump_queue();
    }

    fn update_settings(&mut self, mut settings: Settings) {
        settings.max_concurrent = settings.max_concurrent.clamp(1, 10);
        match settings.save(&self.data_dir) {
            Ok(()) => {
                self.settings = settings;
                self.pump_queue();
            }
            Err(error) => {
                self.message = Some(format!("Settings save failed: {error}"));
            }
        }
    }

    fn shutdown(&mut self) {
        let active: Vec<i64> = self.active.keys().copied().collect();
        for id in active {
            if let Some(download) = self.active.remove(&id) {
                download.handle.abort();
                let _ = self.store.update_status(id, DownloadStatus::Paused, None);
            }
        }
    }

    fn publish(&mut self) {
        let Ok(downloads) = self.store.list_downloads() else {
            return;
        };
        let _ = self.snapshots.send(Snapshot {
            downloads,
            settings: self.settings.clone(),
            speeds: self.speeds.clone(),
            message: self.message.take(),
        });
    }
}
