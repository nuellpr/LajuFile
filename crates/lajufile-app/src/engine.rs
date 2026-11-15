use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use lajufile_core::cookie::{protect_cookie, unprotect_cookie};
use lajufile_core::download::{
    DownloadError, DownloadOutcome, DownloadSpec, download_single_with_limiter,
};
use lajufile_core::http::{DEFAULT_USER_AGENT, RequestOptions, build_client};
use lajufile_core::naming::{dedupe_path, resolve_file_name, temp_path_for};
use lajufile_core::paths::ensure_dir;
use lajufile_core::probe::{ProbeError, ProbeRequest, ProbeResult, probe};
use lajufile_core::segment::plan_segments;
use lajufile_core::segmented::{SegmentedError, SegmentedSpec, download_segmented};
use lajufile_core::settings::{Settings, generate_token};
use lajufile_core::speed::RateLimiter;
use lajufile_core::store::{
    Category, Download, DownloadStatus, NewDownload, Store, StoreError, now_millis,
};
use tokio::runtime::Runtime;
use url::Url;

use crate::bridge::{BridgeAddOutcome, BridgeAddRequest};

use crate::bridge::{self, BridgeServer, BridgeStatus};

pub struct EngineConfig {
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub settings: Settings,
}

pub struct EngineHandle {
    commands: Sender<Command>,
    snapshots: Arc<Mutex<Option<Snapshot>>>,
    join: Option<JoinHandle<()>>,
}

impl EngineHandle {
    pub fn start(config: EngineConfig) -> Result<Self, EngineError> {
        let store = Store::open(&config.db_path)?;
        let (command_tx, command_rx) = mpsc::channel();
        let bridge_commands = command_tx.clone();
        let snapshots = Arc::new(Mutex::new(None));
        let engine_snapshots = Arc::clone(&snapshots);

        let join = thread::Builder::new()
            .name("lajufile-engine".to_string())
            .spawn(move || {
                engine_loop(store, config, command_rx, bridge_commands, engine_snapshots)
            })?;

        Ok(Self {
            commands: command_tx,
            snapshots,
            join: Some(join),
        })
    }

    pub fn send(&self, command: Command) -> Result<(), EngineError> {
        self.commands
            .send(command)
            .map_err(|_| EngineError::Stopped)
    }

    pub fn latest_snapshot(&self) -> Option<Snapshot> {
        self.snapshots
            .lock()
            .ok()
            .and_then(|mut snapshot| snapshot.take())
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
    BridgeAdd {
        request: BridgeAddRequest,
        reply: tokio::sync::oneshot::Sender<BridgeAddOutcome>,
    },
    CaptureFolderDecision {
        key: u64,
        path: Option<PathBuf>,
    },
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
    pub connections: Option<u32>,
    pub referrer: Option<String>,
    pub user_agent: Option<String>,
    pub cookie: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CapturePrompt {
    pub key: u64,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub downloads: Vec<Download>,
    pub settings: Settings,
    pub speeds: HashMap<i64, u64>,
    pub message: Option<String>,
    pub bridge: BridgeStatus,
    pub pending_capture: Option<CapturePrompt>,
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

fn valid_bridge_token(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

enum Event {
    Probe {
        request: AddRequest,
        result: Box<Result<ProbeResult, ProbeError>>,
    },
    BridgeProbe {
        request: AddRequest,
        reply: tokio::sync::oneshot::Sender<BridgeAddOutcome>,
        result: Box<Result<ProbeResult, ProbeError>>,
    },
    Progress {
        id: i64,
        generation: u64,
        done: u64,
    },
    SegmentProgress {
        id: i64,
        generation: u64,
        idx: u32,
        segment_done: u64,
        total: u64,
    },
    Fallback {
        id: i64,
        generation: u64,
        reason: String,
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

struct Inserted {
    id: i64,
    file_name: String,
    size: Option<u64>,
}

enum InsertError {
    Duplicate(Box<Download>),
    Failed(String),
}

struct EngineState {
    store: Store,
    settings: Settings,
    data_dir: PathBuf,
    client: reqwest::Client,
    runtime: Runtime,
    events: Sender<Event>,
    snapshots: Arc<Mutex<Option<Snapshot>>>,
    active: HashMap<i64, ActiveDownload>,
    limiter: Arc<RateLimiter>,
    bridge: Option<BridgeServer>,
    bridge_status: BridgeStatus,
    bridge_token: Arc<RwLock<String>>,
    generation: u64,
    speeds: HashMap<i64, u64>,
    progress: HashMap<i64, (u64, Instant)>,
    pending_segments: HashMap<(i64, u32), u64>,
    last_persist: HashMap<i64, Instant>,
    message: Option<String>,
    pending_captures: HashMap<u64, PendingCapture>,
    next_capture_key: u64,
}

struct PendingCapture {
    request: BridgeAddRequest,
    reply: tokio::sync::oneshot::Sender<BridgeAddOutcome>,
}

fn engine_loop(
    store: Store,
    config: EngineConfig,
    commands: Receiver<Command>,
    bridge_commands: Sender<Command>,
    snapshots: Arc<Mutex<Option<Snapshot>>>,
) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("build engine runtime");
    let client = match build_client(Duration::from_secs(10), Duration::from_secs(30)) {
        Ok(client) => client,
        Err(error) => {
            if let Ok(mut slot) = snapshots.lock() {
                *slot = Some(Snapshot {
                    downloads: store.list_downloads().unwrap_or_default(),
                    settings: config.settings.clone(),
                    speeds: HashMap::new(),
                    message: Some(format!("Engine failed: {error}")),
                    bridge: BridgeStatus::starting(config.settings.bridge_port),
                    pending_capture: None,
                });
            }
            return;
        }
    };

    let (event_tx, event_rx) = mpsc::channel();
    let mut settings = config.settings;
    settings.max_concurrent = settings.max_concurrent.clamp(1, 10);
    settings.max_connections = settings.max_connections.clamp(1, 32);
    settings.global_speed_limit_kbps = settings.global_speed_limit_kbps.min(1_000_000);
    if !valid_bridge_token(&settings.bridge_token) {
        settings.bridge_token = generate_token();
        let _ = settings.save(&config.data_dir);
    }
    let limiter = Arc::new(RateLimiter::new(settings.global_speed_limit_kbps));
    let bridge_token = Arc::new(RwLock::new(settings.bridge_token.clone()));
    let bridge = runtime.block_on(bridge::start(
        settings.bridge_port,
        Arc::clone(&bridge_token),
        bridge_commands,
    ));
    let bridge_status = bridge.status.clone();
    let mut state = EngineState {
        store,
        settings,
        data_dir: config.data_dir,
        client,
        runtime,
        events: event_tx,
        snapshots,
        active: HashMap::new(),
        limiter,
        bridge: Some(bridge),
        bridge_status,
        bridge_token,
        generation: 0,
        speeds: HashMap::new(),
        progress: HashMap::new(),
        pending_segments: HashMap::new(),
        last_persist: HashMap::new(),
        message: None,
        pending_captures: HashMap::new(),
        next_capture_key: 0,
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
            Command::BridgeAdd { request, reply } => self.handle_bridge_add(request, reply),
            Command::CaptureFolderDecision { key, path } => {
                self.capture_folder_decision(key, path);
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
        let normalized_url = url.to_string();

        match self.store.active_by_url(&normalized_url) {
            Ok(Some(existing)) => {
                return Some(format!("Already in LajuFile: {}", existing.file_name));
            }
            Ok(None) => {}
            Err(error) => return Some(format!("Add failed: {error}")),
        }

        let user_agent = request
            .user_agent
            .clone()
            .unwrap_or_else(|| DEFAULT_USER_AGENT.to_string());
        let probe_request = ProbeRequest {
            url,
            options: RequestOptions {
                user_agent: Some(user_agent),
                referrer: request.referrer.clone(),
                cookie: request.cookie.clone(),
            },
            timeout: Duration::from_secs(20),
        };
        let client = self.client.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let result = probe(&client, &probe_request).await;
            let _ = events.send(Event::Probe {
                request: AddRequest {
                    url: normalized_url,
                    ..request
                },
                result: Box::new(result),
            });
        });
        None
    }

    fn handle_bridge_add(
        &mut self,
        request: BridgeAddRequest,
        reply: tokio::sync::oneshot::Sender<BridgeAddOutcome>,
    ) {
        let url = match Url::parse(&request.url) {
            Ok(url) => url,
            Err(error) => {
                let _ = reply.send(BridgeAddOutcome::Invalid(format!("invalid URL: {error}")));
                return;
            }
        };
        let normalized_url = url.to_string();

        match self.store.active_by_url(&normalized_url) {
            Ok(Some(existing)) => {
                let _ = reply.send(BridgeAddOutcome::Conflict { id: existing.id });
                return;
            }
            Ok(None) => {}
            Err(error) => {
                let _ = reply.send(BridgeAddOutcome::Internal(error.to_string()));
                return;
            }
        }

        let add = AddRequest {
            url: normalized_url,
            file_name: request.filename.clone(),
            save_dir: None,
            start_now: true,
            connections: None,
            referrer: request.referrer.clone(),
            user_agent: request.user_agent.clone(),
            cookie: request.cookie.clone(),
        };
        if self.settings.ask_where_to_save {
            let key = self.next_capture_key;
            self.next_capture_key = self.next_capture_key.wrapping_add(1);
            self.pending_captures
                .insert(key, PendingCapture { request, reply });
            self.message = Some("Waiting for a save folder".to_string());
            return;
        }
        self.spawn_bridge_probe(url, add, reply);
    }

    fn spawn_bridge_probe(
        &mut self,
        url: Url,
        add: AddRequest,
        reply: tokio::sync::oneshot::Sender<BridgeAddOutcome>,
    ) {
        let probe_request = ProbeRequest {
            url,
            options: RequestOptions {
                user_agent: add
                    .user_agent
                    .clone()
                    .or_else(|| Some(DEFAULT_USER_AGENT.to_string())),
                referrer: add.referrer.clone(),
                cookie: add.cookie.clone(),
            },
            timeout: Duration::from_secs(3),
        };
        let client = self.client.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let result = probe(&client, &probe_request).await;
            let _ = events.send(Event::BridgeProbe {
                request: add,
                reply,
                result: Box::new(result),
            });
        });
    }

    fn capture_folder_decision(&mut self, key: u64, path: Option<PathBuf>) {
        let Some(pending) = self.pending_captures.remove(&key) else {
            return;
        };
        match path {
            Some(path) => {
                let Ok(url) = Url::parse(&pending.request.url) else {
                    let _ = pending
                        .reply
                        .send(BridgeAddOutcome::Invalid("invalid URL".to_string()));
                    return;
                };
                let add = AddRequest {
                    url: pending.request.url.clone(),
                    file_name: pending.request.filename.clone(),
                    save_dir: Some(path),
                    start_now: true,
                    connections: None,
                    referrer: pending.request.referrer.clone(),
                    user_agent: pending.request.user_agent.clone(),
                    cookie: pending.request.cookie.clone(),
                };
                self.spawn_bridge_probe(url, add, pending.reply);
            }
            None => {
                let _ = pending.reply.send(BridgeAddOutcome::Canceled);
            }
        }
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
        match self.insert_from_probe(request, result) {
            Ok(inserted) => format!("Added {}", inserted.file_name),
            Err(InsertError::Duplicate(existing)) => {
                format!("Already in LajuFile: {}", existing.file_name)
            }
            Err(InsertError::Failed(error)) => format!("Add failed: {error}"),
        }
    }

    fn insert_from_probe(
        &mut self,
        request: AddRequest,
        result: ProbeResult,
    ) -> Result<Inserted, InsertError> {
        match self.store.active_by_url(&request.url) {
            Ok(Some(existing)) => return Err(InsertError::Duplicate(Box::new(existing))),
            Ok(None) => {}
            Err(error) => return Err(InsertError::Failed(error.to_string())),
        }

        let dir = request
            .save_dir
            .clone()
            .unwrap_or_else(|| self.settings.download_dir.clone());
        if let Err(error) = ensure_dir(&dir) {
            return Err(InsertError::Failed(error.to_string()));
        }

        let base_name = resolve_file_name(
            request.file_name.as_deref(),
            result.content_disposition.as_deref(),
            &result.final_url,
        );
        let mut final_path = dedupe_path(&dir, &base_name);
        let temp_path = loop {
            let candidate = temp_path_for(&final_path);
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(_) => break candidate,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    final_path = dedupe_path(&dir, &base_name);
                }
                Err(error) => return Err(InsertError::Failed(error.to_string())),
            }
        };
        let reserved_temp = temp_path.clone();
        let file_name = final_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or(base_name);
        let connections = request
            .connections
            .unwrap_or(self.settings.max_connections)
            .clamp(1, 32);
        let segments = plan_segments(result.size.unwrap_or(0), result.ranged, connections);
        let size = result.size;

        let cookie_enc = match request
            .cookie
            .as_deref()
            .map(str::trim)
            .filter(|cookie| !cookie.is_empty())
        {
            Some(cookie) => match protect_cookie(cookie) {
                Ok(encrypted) => Some(encrypted),
                Err(error) => {
                    let _ = std::fs::remove_file(&reserved_temp);
                    return Err(InsertError::Failed(format!("cookie: {error}")));
                }
            },
            None => None,
        };
        let referrer = request.referrer.clone();
        let user_agent = request
            .user_agent
            .clone()
            .or_else(|| Some(DEFAULT_USER_AGENT.to_string()));

        let queue_order = match self.store.next_queue_order() {
            Ok(order) => order,
            Err(error) => {
                let _ = std::fs::remove_file(&reserved_temp);
                return Err(InsertError::Failed(error.to_string()));
            }
        };
        let new = NewDownload {
            url: request.url,
            final_url: Some(result.final_url.to_string()),
            file_name: file_name.clone(),
            save_path: final_path,
            temp_path,
            size,
            status: DownloadStatus::Queued,
            category: Category::from_file_name(&file_name),
            connections,
            ranged: result.ranged,
            etag: result.etag,
            last_modified: result.last_modified,
            cookie_enc,
            referrer,
            user_agent,
            queue_order,
            created_at: now_millis(),
            segments,
        };

        match self.store.insert_download(&new) {
            Ok(id) => {
                self.pump_queue();
                Ok(Inserted {
                    id,
                    file_name,
                    size,
                })
            }
            Err(error) => {
                let _ = std::fs::remove_file(&reserved_temp);
                Err(InsertError::Failed(error.to_string()))
            }
        }
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Probe { request, result } => {
                self.message = Some(self.handle_probe(request, *result));
            }
            Event::BridgeProbe {
                request,
                reply,
                result,
            } => {
                let outcome = match *result {
                    Err(error) => BridgeAddOutcome::ProbeFailed(error.to_string()),
                    Ok(result) => match self.insert_from_probe(request, result) {
                        Ok(inserted) => BridgeAddOutcome::Created {
                            id: inserted.id,
                            filename: inserted.file_name,
                            size: inserted.size,
                        },
                        Err(InsertError::Duplicate(existing)) => {
                            BridgeAddOutcome::Conflict { id: existing.id }
                        }
                        Err(InsertError::Failed(error)) => BridgeAddOutcome::Internal(error),
                    },
                };
                let _ = reply.send(outcome);
            }
            Event::Progress {
                id,
                generation,
                done,
            } => {
                if self.active.get(&id).map(|active| active.generation) != Some(generation) {
                    return;
                }
                self.record_speed(id, done);
                self.pending_segments.insert((id, 0), done);
                let should_persist = self
                    .last_persist
                    .get(&id)
                    .is_none_or(|at| at.elapsed() >= Duration::from_secs(1));
                if should_persist {
                    self.flush_segment_progress(id);
                    self.last_persist.insert(id, Instant::now());
                }
            }
            Event::SegmentProgress {
                id,
                generation,
                idx,
                segment_done,
                total,
            } => {
                if self.active.get(&id).map(|active| active.generation) != Some(generation) {
                    return;
                }
                self.record_speed(id, total);
                self.pending_segments.insert((id, idx), segment_done);
                let should_persist = self
                    .last_persist
                    .get(&id)
                    .is_none_or(|at| at.elapsed() >= Duration::from_secs(1));
                if should_persist {
                    self.flush_segment_progress(id);
                    self.last_persist.insert(id, Instant::now());
                }
            }
            Event::Fallback {
                id,
                generation,
                reason,
            } => {
                if self.active.get(&id).map(|active| active.generation) != Some(generation) {
                    return;
                }
                self.active.remove(&id);
                self.clear_pending(id);
                self.reset_to_single(id, &reason);
                self.speeds.remove(&id);
                self.progress.remove(&id);
                self.last_persist.remove(&id);
                self.pump_queue();
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
                self.flush_segment_progress(id);
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
                self.clear_pending(id);
                self.speeds.remove(&id);
                self.progress.remove(&id);
                self.last_persist.remove(&id);
                self.pump_queue();
            }
        }
    }

    fn record_speed(&mut self, id: i64, done: u64) {
        let now = Instant::now();
        if let Some((previous, previous_time)) = self.progress.get(&id) {
            let seconds = now.duration_since(*previous_time).as_secs_f64();
            if seconds >= 0.1 && done >= *previous {
                self.speeds
                    .insert(id, ((done - *previous) as f64 / seconds) as u64);
            }
        }
        self.progress.insert(id, (done, now));
    }

    fn flush_segment_progress(&mut self, id: i64) {
        let updates: Vec<(u32, u64)> = self
            .pending_segments
            .iter()
            .filter(|((download_id, _), _)| *download_id == id)
            .map(|((_, idx), done)| (*idx, *done))
            .collect();
        if updates.is_empty() {
            return;
        }
        if self.store.update_progress_batch(id, &updates).is_ok() {
            for (idx, _) in updates {
                self.pending_segments.remove(&(id, idx));
            }
        }
    }

    fn clear_pending(&mut self, id: i64) {
        self.pending_segments
            .retain(|(download_id, _), _| *download_id != id);
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

        let cookie = match download.cookie_enc.as_deref() {
            Some(encrypted) => match unprotect_cookie(encrypted) {
                Ok(cookie) => Some(cookie),
                Err(error) => {
                    let _ = self.store.update_status(
                        download.id,
                        DownloadStatus::Error,
                        Some(&format!("cookie: {error}")),
                    );
                    return false;
                }
            },
            None => None,
        };

        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.clear_pending(download.id);
        if self
            .store
            .update_status(download.id, DownloadStatus::Downloading, None)
            .is_err()
        {
            return false;
        }

        let options = RequestOptions {
            user_agent: download
                .user_agent
                .clone()
                .or_else(|| Some(DEFAULT_USER_AGENT.to_string())),
            referrer: download.referrer.clone(),
            cookie,
        };
        let id = download.id;
        let events = self.events.clone();
        let limiter = self.limiter.clone();
        let segments = self.store.segments(id).unwrap_or_default();
        let use_segmented =
            download.ranged && download.size.is_some_and(|size| size > 0) && !segments.is_empty();
        let handle = if use_segmented {
            let spec = SegmentedSpec {
                url,
                temp_path: download.temp_path.clone(),
                final_path: download.save_path.clone(),
                size: download.size.unwrap_or(0),
                segments,
                options,
                if_range: download
                    .etag
                    .clone()
                    .or_else(|| download.last_modified.clone()),
                limiter,
                retry_delays: vec![
                    Duration::from_secs(2),
                    Duration::from_secs(5),
                    Duration::from_secs(10),
                ],
            };
            let client = self.client.clone();
            self.runtime.spawn(async move {
                let progress_events = events.clone();
                let result = download_segmented(&client, &spec, move |idx, segment_done, total| {
                    let _ = progress_events.send(Event::SegmentProgress {
                        id,
                        generation,
                        idx,
                        segment_done,
                        total,
                    });
                })
                .await;
                match result {
                    Ok(outcome) => {
                        let _ = events.send(Event::Finished {
                            id,
                            generation,
                            result: Ok(outcome),
                        });
                    }
                    Err(SegmentedError::RangeIgnored) => {
                        let _ = events.send(Event::Fallback {
                            id,
                            generation,
                            reason: "server ignored byte ranges".to_string(),
                        });
                    }
                    Err(SegmentedError::RestartRequired) => {
                        let _ = events.send(Event::Fallback {
                            id,
                            generation,
                            reason: "resume validation failed".to_string(),
                        });
                    }
                    Err(error) => {
                        let _ = events.send(Event::Finished {
                            id,
                            generation,
                            result: Err(segmented_error(error)),
                        });
                    }
                }
            })
        } else {
            let spec = DownloadSpec {
                url,
                temp_path: download.temp_path.clone(),
                final_path: download.save_path.clone(),
                expected_size: download.size,
                options,
            };
            let client = self.client.clone();
            self.runtime.spawn(async move {
                let result = download_single_with_limiter(&client, &spec, Some(limiter), |done| {
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
            })
        };
        self.active
            .insert(id, ActiveDownload { generation, handle });
        true
    }

    fn reset_to_single(&mut self, id: i64, reason: &str) {
        let Ok(download) = self.store.get_download(id) else {
            return;
        };
        let segments = plan_segments(download.size.unwrap_or(0), false, 1);
        if self.store.replace_segments(id, &segments, 1, false).is_ok() {
            let _ = self.store.update_status(id, DownloadStatus::Queued, None);
            self.message = Some(format!("Falling back to single stream: {reason}"));
        }
    }

    fn pump_queue(&mut self) {
        let limit = self.settings.max_concurrent.max(1) as usize;
        let mut queued: Vec<Download> = match self.store.list_downloads() {
            Ok(downloads) => downloads
                .into_iter()
                .filter(|download| {
                    download.status == DownloadStatus::Queued
                        && !self.active.contains_key(&download.id)
                })
                .collect(),
            Err(_) => return,
        };
        queued.sort_by_key(|download| download.queue_order);
        for download in queued {
            if self.active.len() >= limit {
                break;
            }
            // A row that fails before starting (bad cookie, invalid URL) must
            // not block the rest of the queue.
            self.start_download(&download);
        }
    }

    fn pause(&mut self, id: i64) {
        self.pause_with_pump(id, true);
    }

    fn pause_with_pump(&mut self, id: i64, pump: bool) {
        self.flush_segment_progress(id);
        if let Some(active) = self.active.remove(&id) {
            active.handle.abort();
            self.speeds.remove(&id);
            self.progress.remove(&id);
            self.last_persist.remove(&id);
        }
        self.clear_pending(id);
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
            active.handle.abort();
            let _ = self.runtime.block_on(active.handle);
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
        self.clear_pending(id);
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
        settings.max_connections = settings.max_connections.clamp(1, 32);
        settings.global_speed_limit_kbps = settings.global_speed_limit_kbps.min(1_000_000);
        settings.bridge_port = settings.bridge_port.clamp(1024, 65_535);
        if !valid_bridge_token(&settings.bridge_token) {
            settings.bridge_token = generate_token();
        }
        match settings.save(&self.data_dir) {
            Ok(()) => {
                self.limiter.set_kbps(settings.global_speed_limit_kbps);
                if let Ok(mut token) = self.bridge_token.write() {
                    *token = settings.bridge_token.clone();
                }
                self.settings = settings;
                self.pump_queue();
            }
            Err(error) => {
                self.message = Some(format!("Settings save failed: {error}"));
            }
        }
    }

    fn shutdown(&mut self) {
        if let Some(bridge) = self.bridge.take() {
            bridge.shutdown();
        }
        for (_, pending) in self.pending_captures.drain() {
            let _ = pending.reply.send(BridgeAddOutcome::Canceled);
        }
        let active: Vec<i64> = self.active.keys().copied().collect();
        for id in active {
            self.flush_segment_progress(id);
            if let Some(download) = self.active.remove(&id) {
                download.handle.abort();
                let _ = self.store.update_status(id, DownloadStatus::Paused, None);
            }
            self.clear_pending(id);
        }
    }

    fn publish(&mut self) {
        let Ok(downloads) = self.store.list_downloads() else {
            return;
        };
        let snapshot = Snapshot {
            downloads,
            settings: self.settings.clone(),
            speeds: self.speeds.clone(),
            message: self.message.clone(),
            bridge: self.bridge_status.clone(),
            pending_capture: self.pending_captures.iter().next().map(|(key, pending)| {
                CapturePrompt {
                    key: *key,
                    url: pending.request.url.clone(),
                }
            }),
        };
        if let Ok(mut slot) = self.snapshots.lock() {
            *slot = Some(snapshot);
        }
    }
}

fn segmented_error(error: SegmentedError) -> DownloadError {
    match error {
        SegmentedError::Download(error) => error,
        SegmentedError::InvalidSegments(message) => {
            DownloadError::Io(std::io::Error::other(message))
        }
        SegmentedError::RangeIgnored | SegmentedError::RestartRequired => {
            DownloadError::Io(std::io::Error::other("segmented fallback was not handled"))
        }
    }
}
