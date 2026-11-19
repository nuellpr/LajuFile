mod ssrf;
mod video;

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use lajufile_core::cookie::{protect_cookie, unprotect_cookie};
use lajufile_core::download::{
    DownloadError, DownloadOutcome, DownloadSpec, download_single_with_retry,
};
use lajufile_core::http::{
    DEFAULT_USER_AGENT, RequestOptions, build_client, build_client_with_proxy,
};
use lajufile_core::naming::{dedupe_path, resolve_file_name, sanitize_file_name, temp_path_for};
use lajufile_core::paths::ensure_dir;
use lajufile_core::probe::{ProbeError, ProbeRequest, ProbeResult, probe};
use lajufile_core::segment::plan_segments;
use lajufile_core::segmented::{SegmentedError, SegmentedSpec, download_segmented};
use lajufile_core::settings::{DuplicatePolicy, Settings, generate_token, is_valid_token};
use lajufile_core::speed::RateLimiter;
use lajufile_core::store::{
    Category, Download, DownloadStatus, NewDownload, Segment, Store, StoreError, now_millis,
};
use lajufile_core::video::{DEFAULT_SELECTOR, VideoListing, is_audio_only_selector};
use tokio::runtime::Runtime;
use url::Url;

use crate::bridge::{BridgeAddOutcome, BridgeAddRequest};

use crate::bridge::{self, BridgeServer, BridgeStatus};
use crate::tools;

pub struct EngineConfig {
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub settings: Settings,
}

#[derive(Default)]
struct SnapshotSlot {
    version: u64,
    snapshot: Option<Arc<Snapshot>>,
}

pub type WakeCallback = Arc<dyn Fn() + Send + Sync>;

pub struct EngineHandle {
    commands: Sender<Command>,
    snapshots: Arc<Mutex<SnapshotSlot>>,
    wake_callback: Arc<RwLock<Option<WakeCallback>>>,
    join: Option<JoinHandle<()>>,
}

impl EngineHandle {
    pub fn start(config: EngineConfig) -> Result<Self, EngineError> {
        let store = Store::open(&config.db_path)?;
        let (command_tx, command_rx) = mpsc::channel();
        let bridge_commands = command_tx.clone();
        let snapshots = Arc::new(Mutex::new(SnapshotSlot::default()));
        let engine_snapshots = Arc::clone(&snapshots);
        let wake_callback: Arc<RwLock<Option<WakeCallback>>> = Arc::new(RwLock::new(None));
        let engine_wake = Arc::clone(&wake_callback);

        let join = thread::Builder::new()
            .name("lajufile-engine".to_string())
            .spawn(move || {
                engine_loop(
                    store,
                    config,
                    command_rx,
                    bridge_commands,
                    engine_snapshots,
                    engine_wake,
                )
            })?;

        Ok(Self {
            commands: command_tx,
            snapshots,
            wake_callback,
            join: Some(join),
        })
    }

    pub fn set_wake_callback(&self, callback: WakeCallback) {
        if let Ok(mut guard) = self.wake_callback.write() {
            *guard = Some(callback);
        }
    }

    pub fn send(&self, command: Command) -> Result<(), EngineError> {
        self.commands
            .send(command)
            .map_err(|_| EngineError::Stopped)
    }

    pub fn latest_snapshot_if_changed(&self, seen_version: &mut u64) -> Option<Arc<Snapshot>> {
        let guard = self.snapshots.lock().ok()?;
        if guard.version > *seen_version
            && let Some(snapshot) = &guard.snapshot
        {
            *seen_version = guard.version;
            return Some(Arc::clone(snapshot));
        }
        None
    }

    pub fn latest_snapshot(&self) -> Option<Snapshot> {
        self.snapshots
            .lock()
            .ok()
            .and_then(|guard| guard.snapshot.as_ref().map(|snapshot| (**snapshot).clone()))
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
    CaptureDecision {
        key: u64,
        folder: PathBuf,
        file_name: Option<String>,
        start_now: bool,
    },
    Pause(i64),
    Resume(i64),
    Redownload(i64),
    UpdateUrlAndResume {
        id: i64,
        url: String,
    },
    MoveRename {
        id: i64,
        file_name: String,
        dir: Option<PathBuf>,
    },
    Remove(i64),
    RemoveWithFile {
        id: i64,
        delete_file: bool,
    },
    RemoveCompleted,
    PauseAll,
    ResumeAll,
    UpdateSettings(Settings),
    GetRules {
        reply: tokio::sync::oneshot::Sender<(String, String)>,
    },
    MarkBrowserPaired,
    GrabPageLinks {
        url: String,
        reply: tokio::sync::oneshot::Sender<Result<Vec<String>, String>>,
    },
    VideoFormats {
        url: String,
        cookie: Option<String>,
        reply: tokio::sync::oneshot::Sender<Result<VideoListing, String>>,
    },
    UpdateTools,
    ToolsInstalled(Result<(), String>),
    RecordUpdateCheck(i64),
    WakeWindow,
    RequestExit,
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
    /// yt-dlp selector; `Some` means the add is queued as a video job, not probed.
    pub video_format: Option<String>,
    /// Optional estimated byte size selected from video formats.
    pub video_size: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct CapturePrompt {
    pub key: u64,
    pub url: String,
    pub file_name: String,
    pub size: Option<u64>,
    pub ranged: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub downloads: Vec<Download>,
    pub segments: HashMap<i64, Vec<Segment>>,
    pub settings: Settings,
    pub speeds: HashMap<i64, u64>,
    pub message: Option<String>,
    pub bridge: BridgeStatus,
    pub pending_capture: Option<CapturePrompt>,
    pub wake_seq: u64,
    pub exit_requested: bool,
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

/// Reserves a deduplicated save path and empty `.lajufile` sentinel so queued rows
/// cannot collide before download writes begin.
fn reserve_download_paths(dir: &Path, base_name: &str) -> Result<(PathBuf, PathBuf), String> {
    for _ in 0..128 {
        let final_path = dedupe_path(dir, base_name);
        let candidate = temp_path_for(&final_path);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(_) => return Ok((final_path, candidate)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("Could not reserve a unique download filename".to_string())
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
    CaptureProbe {
        key: u64,
        result: Box<Result<ProbeResult, ProbeError>>,
    },
    Progress {
        id: i64,
        generation: u64,
        done: u64,
    },
    VideoProgress {
        id: i64,
        generation: u64,
        done: u64,
        total: Option<u64>,
        filename: Option<String>,
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
    handle: Option<tokio::task::JoinHandle<()>>,
    video: Option<video::VideoRun>,
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

enum DuplicateAction {
    Proceed,
    Handled {
        id: i64,
        file_name: String,
        size: Option<u64>,
        message: String,
    },
    Rejected(Box<Download>),
}

struct EngineState {
    store: Store,
    settings: Settings,
    data_dir: PathBuf,
    client: reqwest::Client,
    runtime: Runtime,
    events: Sender<Event>,
    commands: Sender<Command>,
    tools_installing: bool,
    snapshots: Arc<Mutex<SnapshotSlot>>,
    active: HashMap<i64, ActiveDownload>,
    limiter: Arc<RateLimiter>,
    bridge: Option<BridgeServer>,
    bridge_status: BridgeStatus,
    bridge_token: Arc<RwLock<String>>,
    generation: u64,
    speeds: HashMap<i64, u64>,
    progress: HashMap<i64, (u64, Instant)>,
    video_streams: HashMap<i64, video::VideoStreamTracker>,
    pending_segments: HashMap<(i64, u32), u64>,
    last_persist: HashMap<i64, Instant>,
    message: Option<String>,
    pending_captures: BTreeMap<u64, PendingCapture>,
    next_capture_key: u64,
    in_flight_probes: usize,
    wake_seq: u64,
    exit_requested: bool,
    immediate_publish: bool,
    wake_callback: Arc<RwLock<Option<WakeCallback>>>,
    dirty: bool,
    cached_downloads: Vec<Download>,
    cached_segments: HashMap<i64, Vec<Segment>>,
    cache_dirty: bool,
}

struct PendingCapture {
    request: BridgeAddRequest,
    reply: tokio::sync::oneshot::Sender<BridgeAddOutcome>,
    file_name: String,
    size: Option<u64>,
    probe: Option<ProbeResult>,
}

fn engine_loop(
    store: Store,
    config: EngineConfig,
    commands: Receiver<Command>,
    bridge_commands: Sender<Command>,
    snapshots: Arc<Mutex<SnapshotSlot>>,
    wake_callback: Arc<RwLock<Option<WakeCallback>>>,
) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("build engine runtime");
    let proxy_trimmed = config.settings.proxy_url.trim().to_string();
    let (client, mut startup_message) = if proxy_trimmed.is_empty() {
        (
            build_client(Duration::from_secs(10), Duration::from_secs(30)),
            None,
        )
    } else {
        match build_client_with_proxy(
            Duration::from_secs(10),
            Duration::from_secs(30),
            Some(&proxy_trimmed),
        ) {
            Ok(client) => (Ok(client), None),
            Err(err) => (
                build_client(Duration::from_secs(10), Duration::from_secs(30)),
                Some(format!("Proxy ignored ({err})")),
            ),
        }
    };
    let client = match client {
        Ok(client) => client,
        Err(error) => {
            if let Ok(mut slot) = snapshots.lock() {
                slot.version = slot.version.saturating_add(1);
                slot.snapshot = Some(Arc::new(Snapshot {
                    downloads: store.list_downloads().unwrap_or_default(),
                    segments: HashMap::new(),
                    settings: config.settings.clone(),
                    speeds: HashMap::new(),
                    message: Some(format!("Engine failed: {error}")),
                    bridge: BridgeStatus::starting(config.settings.bridge_port),
                    pending_capture: None,
                    wake_seq: 0,
                    exit_requested: false,
                }));
            }
            return;
        }
    };

    let (event_tx, event_rx) = mpsc::channel();
    let mut settings = config.settings;
    settings.max_concurrent = settings.max_concurrent.clamp(1, 10);
    settings.max_connections = settings.max_connections.clamp(1, 32);
    settings.global_speed_limit_kbps = settings.global_speed_limit_kbps.min(1_000_000);
    if !is_valid_token(&settings.bridge_token) {
        settings.bridge_token = generate_token();
        let _ = settings.save(&config.data_dir);
    }
    let limiter = Arc::new(RateLimiter::new(settings.global_speed_limit_kbps));
    let bridge_token = Arc::new(RwLock::new(settings.bridge_token.clone()));
    let engine_commands = bridge_commands.clone();
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
        commands: engine_commands,
        tools_installing: false,
        snapshots,
        active: HashMap::new(),
        limiter,
        bridge: Some(bridge),
        bridge_status,
        bridge_token,
        generation: 0,
        speeds: HashMap::new(),
        progress: HashMap::new(),
        video_streams: HashMap::new(),
        pending_segments: HashMap::new(),
        last_persist: HashMap::new(),
        message: startup_message.take(),
        pending_captures: BTreeMap::new(),
        next_capture_key: 0,
        in_flight_probes: 0,
        wake_seq: 0,
        exit_requested: false,
        immediate_publish: false,
        wake_callback,
        dirty: false,
        cached_downloads: Vec::new(),
        cached_segments: HashMap::new(),
        cache_dirty: true,
    };

    state.normalize_startup();
    state.pump_queue();
    state.publish();
    state.dirty = false;
    let mut last_publish = Instant::now();

    loop {
        while let Ok(command) = commands.try_recv() {
            let mutates = !matches!(command, Command::GetRules { .. });
            if state.handle_command(command) {
                state.publish();
                return;
            }
            if mutates {
                state.dirty = true;
            }
        }

        while let Ok(event) = event_rx.try_recv() {
            state.handle_event(event);
            state.dirty = true;
        }

        if state.poll_videos() || !state.active.is_empty() {
            state.dirty = true;
        }

        let captures_before = state.pending_captures.len();
        state
            .pending_captures
            .retain(|_, pending| !pending.reply.is_closed());
        if state.pending_captures.len() != captures_before {
            state.dirty = true;
        }

        if !state.speeds.is_empty() && state.decay_stalled_speeds() {
            state.dirty = true;
        }

        if state.immediate_publish
            || (state.dirty && last_publish.elapsed() >= Duration::from_millis(250))
        {
            state.publish();
            state.immediate_publish = false;
            state.dirty = false;
            last_publish = Instant::now();
        }

        let wait_timeout = if !state.active.is_empty()
            || state.in_flight_probes > 0
            || !state.pending_captures.is_empty()
            || state.dirty
        {
            Duration::from_millis(25)
        } else {
            Duration::from_millis(500)
        };
        match commands.recv_timeout(wait_timeout) {
            Ok(command) => {
                let mutates = !matches!(command, Command::GetRules { .. });
                if state.handle_command(command) {
                    state.publish();
                    return;
                }
                if mutates {
                    state.dirty = true;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                state.shutdown();
                state.publish();
                return;
            }
        }
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
                self.cache_dirty = true;
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
            Command::CaptureDecision {
                key,
                folder,
                file_name,
                start_now,
            } => {
                self.capture_decision(key, folder, file_name, start_now);
            }
            Command::Pause(id) => self.pause(id),
            Command::Resume(id) => self.resume(id),
            Command::Redownload(id) => self.redownload(id),
            Command::UpdateUrlAndResume { id, url } => self.update_url_and_resume(id, url),
            Command::MoveRename { id, file_name, dir } => self.move_rename(id, file_name, dir),
            Command::Remove(id) => self.remove(id, false),
            Command::RemoveWithFile { id, delete_file } => self.remove(id, delete_file),
            Command::RemoveCompleted => self.remove_completed(),
            Command::PauseAll => self.pause_all(),
            Command::ResumeAll => self.resume_all(),
            Command::UpdateSettings(settings) => self.update_settings(settings),
            Command::GetRules { reply } => {
                let _ = reply.send((
                    self.settings.excluded_domains.clone(),
                    self.settings.captured_extensions.clone(),
                ));
            }
            Command::MarkBrowserPaired => {
                if !self.settings.browser_paired {
                    self.settings.browser_paired = true;
                    let _ = self.settings.save(&self.data_dir);
                }
            }
            Command::GrabPageLinks { url, reply } => self.grab_page_links(url, reply),
            Command::VideoFormats { url, cookie, reply } => {
                self.video_formats(url, cookie, reply);
            }
            Command::UpdateTools => self.update_tools(),
            Command::ToolsInstalled(result) => {
                self.tools_installing = false;
                self.message = Some(match result {
                    Ok(()) => "Tools ready".to_string(),
                    Err(error) => format!("Tools failed: {error}"),
                });
                self.pump_queue();
            }
            Command::RecordUpdateCheck(ts) => {
                self.settings.last_update_check_ms = self.settings.last_update_check_ms.max(ts);
                let _ = self.settings.save(&self.data_dir);
            }
            Command::WakeWindow => self.trigger_wake(),
            Command::RequestExit => self.request_exit(),
            Command::Shutdown => {
                self.shutdown();
                return true;
            }
        }
        false
    }

    fn notify_wake_callback(&self) {
        if let Ok(guard) = self.wake_callback.read()
            && let Some(cb) = guard.as_ref()
        {
            cb();
        }
    }

    fn trigger_wake(&mut self) {
        self.wake_seq = self.wake_seq.saturating_add(1);
        self.immediate_publish = true;
        self.notify_wake_callback();
    }

    fn request_exit(&mut self) {
        self.pause_all();
        self.exit_requested = true;
        self.immediate_publish = true;
        self.notify_wake_callback();
    }

    fn check_duplicate_url(&mut self, url: &str) -> Result<DuplicateAction, StoreError> {
        if self.settings.duplicate_policy == DuplicatePolicy::RenameNumbered {
            return Ok(DuplicateAction::Proceed);
        }
        let Some(existing) = self.store.active_by_url(url)? else {
            return Ok(DuplicateAction::Proceed);
        };
        if self.settings.duplicate_policy == DuplicatePolicy::ResumeExisting {
            let message = if existing.status == DownloadStatus::Paused {
                self.resume(existing.id);
                format!("Resumed {}", existing.file_name)
            } else {
                format!("Already active: {}", existing.file_name)
            };
            return Ok(DuplicateAction::Handled {
                id: existing.id,
                file_name: existing.file_name,
                size: existing.size,
                message,
            });
        }
        Ok(DuplicateAction::Rejected(Box::new(existing)))
    }

    fn handle_add(&mut self, request: AddRequest) -> Option<String> {
        let url = match ssrf::validate_http_url(&request.url) {
            Ok(url) => url,
            Err(error) => return Some(format!("Add failed: {error}")),
        };
        let normalized_url = url.to_string();

        match self.check_duplicate_url(&normalized_url) {
            Ok(DuplicateAction::Proceed) => {}
            Ok(DuplicateAction::Handled { message, .. }) => return Some(message),
            Ok(DuplicateAction::Rejected(existing)) => {
                return Some(format!("Already in LajuFile: {}", existing.file_name));
            }
            Err(error) => return Some(format!("Add failed: {error}")),
        }

        if request.video_format.is_some() {
            let request = AddRequest {
                url: normalized_url,
                ..request
            };
            return Some(match self.insert_video(request) {
                Ok(inserted) => format!("Added {}", inserted.file_name),
                Err(InsertError::Duplicate(existing)) => {
                    format!("Already in LajuFile: {}", existing.file_name)
                }
                Err(InsertError::Failed(error)) => format!("Add failed: {error}"),
            });
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
        self.in_flight_probes += 1;
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
        let url = match ssrf::validate_http_url(&request.url) {
            Ok(url) => url,
            Err(error) => {
                let _ = reply.send(BridgeAddOutcome::Invalid(error));
                return;
            }
        };
        let normalized_url = url.to_string();

        match self.check_duplicate_url(&normalized_url) {
            Ok(DuplicateAction::Proceed) => {}
            Ok(DuplicateAction::Handled {
                id,
                file_name,
                size,
                ..
            }) => {
                let _ = reply.send(BridgeAddOutcome::Created {
                    id,
                    filename: file_name,
                    size,
                });
                return;
            }
            Ok(DuplicateAction::Rejected(existing)) => {
                let _ = reply.send(BridgeAddOutcome::Conflict { id: existing.id });
                return;
            }
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
            video_format: request.video_format.clone(),
            video_size: request.size,
        };
        if self.settings.ask_where_to_save {
            let key = self.next_capture_key;
            self.next_capture_key = self.next_capture_key.wrapping_add(1);
            let file_name = if request.video_format.is_some() {
                request
                    .filename
                    .clone()
                    .unwrap_or_else(|| video::video_file_name(&add.url))
            } else {
                resolve_file_name(request.filename.as_deref(), None, &url)
            };
            self.pending_captures.insert(
                key,
                PendingCapture {
                    request,
                    reply,
                    file_name,
                    size: None,
                    probe: None,
                },
            );
            self.message = Some("Waiting for a save folder".to_string());
            self.trigger_wake();
            if add.video_format.is_none() {
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
                self.in_flight_probes += 1;
                self.runtime.spawn(async move {
                    let result = probe(&client, &probe_request).await;
                    let _ = events.send(Event::CaptureProbe {
                        key,
                        result: Box::new(result),
                    });
                });
            }
            return;
        }
        if add.video_format.is_some() {
            self.finish_video_add(add, reply);
            return;
        }
        self.spawn_bridge_probe(url, add, reply);
    }

    /// Replies to a `/video` request: the row exists or the caller gets the reason.
    fn finish_video_add(
        &mut self,
        add: AddRequest,
        reply: tokio::sync::oneshot::Sender<BridgeAddOutcome>,
    ) {
        if !tools::tools_ready(&tools::tools_dir()) {
            self.ensure_tools();
            let _ = reply.send(BridgeAddOutcome::ProbeFailed(
                "the video tools are downloading, try again in a minute".to_string(),
            ));
            return;
        }
        let _ = reply.send(match self.insert_video(add) {
            Ok(inserted) => BridgeAddOutcome::Created {
                id: inserted.id,
                filename: inserted.file_name,
                size: inserted.size,
            },
            Err(InsertError::Duplicate(existing)) => BridgeAddOutcome::Conflict { id: existing.id },
            Err(InsertError::Failed(error)) => BridgeAddOutcome::ProbeFailed(error),
        });
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
        self.in_flight_probes += 1;
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
        match path {
            Some(folder) => self.capture_decision(key, folder, None, true),
            None => {
                if let Some(pending) = self.pending_captures.remove(&key) {
                    let _ = pending.reply.send(BridgeAddOutcome::Canceled);
                }
            }
        }
    }

    fn capture_decision(
        &mut self,
        key: u64,
        folder: PathBuf,
        file_name: Option<String>,
        start_now: bool,
    ) {
        let Some(pending) = self.pending_captures.remove(&key) else {
            return;
        };
        let Ok(url) = Url::parse(&pending.request.url) else {
            let _ = pending
                .reply
                .send(BridgeAddOutcome::Invalid("invalid URL".to_string()));
            return;
        };
        let chosen_name = file_name
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .or(pending.request.filename);
        let add = AddRequest {
            url: url.to_string(),
            file_name: chosen_name,
            save_dir: Some(folder),
            start_now,
            connections: None,
            referrer: pending.request.referrer,
            user_agent: pending.request.user_agent,
            cookie: pending.request.cookie,
            video_format: pending.request.video_format,
            video_size: pending.request.size,
        };
        if add.video_format.is_some() {
            self.finish_video_add(add, pending.reply);
            return;
        }
        if let Some(probe_result) = pending.probe {
            let outcome = match self.insert_from_probe(add, probe_result) {
                Ok(inserted) => BridgeAddOutcome::Created {
                    id: inserted.id,
                    filename: inserted.file_name,
                    size: inserted.size,
                },
                Err(InsertError::Duplicate(existing)) => {
                    BridgeAddOutcome::Conflict { id: existing.id }
                }
                Err(InsertError::Failed(error)) => BridgeAddOutcome::Internal(error),
            };
            let _ = pending.reply.send(outcome);
            return;
        }
        self.spawn_bridge_probe(url, add, pending.reply);
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

    fn resolve_save_dir(&self, category: Category, requested: Option<&Path>) -> PathBuf {
        if let Some(dir) = requested {
            return dir.to_path_buf();
        }
        if let Some(dir) = self.settings.category_dirs.get(category.label())
            && !dir.as_os_str().is_empty()
        {
            return dir.clone();
        }
        self.settings.download_dir.clone()
    }

    fn insert_from_probe(
        &mut self,
        request: AddRequest,
        result: ProbeResult,
    ) -> Result<Inserted, InsertError> {
        if self.settings.duplicate_policy != DuplicatePolicy::RenameNumbered {
            match self.store.active_by_url(&request.url) {
                Ok(Some(existing)) => return Err(InsertError::Duplicate(Box::new(existing))),
                Ok(None) => {}
                Err(error) => return Err(InsertError::Failed(error.to_string())),
            }
        }

        let base_name = resolve_file_name(
            request.file_name.as_deref(),
            result.content_disposition.as_deref(),
            &result.final_url,
        );
        let category = Category::from_file_name(&base_name);
        let dir = self.resolve_save_dir(category, request.save_dir.as_deref());
        if let Err(error) = ensure_dir(&dir) {
            return Err(InsertError::Failed(error.to_string()));
        }

        let (final_path, temp_path) =
            reserve_download_paths(&dir, &base_name).map_err(InsertError::Failed)?;
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

        let cookie_enc = match encrypt_cookie(request.cookie.as_deref()) {
            Ok(encrypted) => encrypted,
            Err(error) => {
                let _ = std::fs::remove_file(&reserved_temp);
                return Err(InsertError::Failed(error));
            }
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
        let status = if request.start_now {
            DownloadStatus::Queued
        } else {
            DownloadStatus::Paused
        };
        let new = NewDownload {
            url: request.url,
            final_url: Some(result.final_url.to_string()),
            file_name: file_name.clone(),
            save_path: final_path,
            temp_path,
            size,
            status,
            category: Category::from_file_name(&file_name),
            kind: "http".to_string(),
            video_format: None,
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
                self.cache_dirty = true;
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

    /// Queues a yt-dlp job: no probe, no temp file, no parallel ranges.
    fn insert_video(&mut self, request: AddRequest) -> Result<Inserted, InsertError> {
        if self.settings.duplicate_policy != DuplicatePolicy::RenameNumbered {
            match self.store.active_by_url(&request.url) {
                Ok(Some(existing)) => return Err(InsertError::Duplicate(Box::new(existing))),
                Ok(None) => {}
                Err(error) => return Err(InsertError::Failed(error.to_string())),
            }
        }

        let audio_only = request
            .video_format
            .as_deref()
            .is_some_and(is_audio_only_selector);
        let category = if audio_only {
            Category::Music
        } else {
            Category::Video
        };
        let dir = self.resolve_save_dir(category, request.save_dir.as_deref());
        if let Err(error) = ensure_dir(&dir) {
            return Err(InsertError::Failed(error.to_string()));
        }

        let base_name = video::resolve_video_file_name(
            request.file_name.as_deref(),
            &request.url,
            request.video_format.as_deref(),
        );
        // Reserve the temp name so two jobs queued before either file exists cannot
        // collide on one save path.
        let (save_path, temp_path) =
            reserve_download_paths(&dir, &base_name).map_err(InsertError::Failed)?;
        let reserved_temp = temp_path.clone();
        let file_name = save_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or(base_name);
        let cookie_enc = match encrypt_cookie(request.cookie.as_deref()) {
            Ok(encrypted) => encrypted,
            Err(error) => {
                let _ = std::fs::remove_file(&reserved_temp);
                return Err(InsertError::Failed(error));
            }
        };
        let queue_order = match self.store.next_queue_order() {
            Ok(order) => order,
            Err(error) => {
                let _ = std::fs::remove_file(&reserved_temp);
                return Err(InsertError::Failed(error.to_string()));
            }
        };
        let status = if request.start_now {
            DownloadStatus::Queued
        } else {
            DownloadStatus::Paused
        };
        let size = request.video_size.filter(|s| *s > 0);
        let new = NewDownload {
            url: request.url.clone(),
            final_url: Some(request.url),
            file_name: file_name.clone(),
            temp_path,
            save_path,
            size,
            status,
            category,
            kind: "video".to_string(),
            video_format: request.video_format,
            connections: 1,
            ranged: false,
            etag: None,
            last_modified: None,
            cookie_enc,
            referrer: request.referrer,
            user_agent: request.user_agent,
            queue_order,
            created_at: now_millis(),
            segments: vec![Segment {
                idx: 0,
                start: 0,
                end: 0,
                done: 0,
            }],
        };

        match self.store.insert_download(&new) {
            Ok(id) => {
                self.cache_dirty = true;
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

    fn update_tools(&mut self) {
        self.spawn_tools_install();
    }

    /// Downloads missing video tools once; video jobs work after it lands.
    fn ensure_tools(&mut self) {
        let dir = tools::tools_dir();
        if tools::tools_ready(&dir) {
            return;
        }
        self.spawn_tools_install();
    }

    fn configured_proxy(&self) -> Option<String> {
        let trimmed = self.settings.proxy_url.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    }

    fn spawn_tools_install(&mut self) {
        if self.tools_installing {
            self.message = Some("Downloading tools...".to_string());
            return;
        }
        let dir = tools::tools_dir();
        let proxy = self.configured_proxy();
        self.tools_installing = true;
        self.message = Some("Downloading tools...".to_string());
        let commands = self.commands.clone();
        thread::spawn(move || {
            let result = tools::install_tools_with_proxy(&dir, proxy.as_deref());
            let _ = commands.send(Command::ToolsInstalled(result));
        });
    }

    /// Runs `yt-dlp -J` off the engine loop; the reply carries the title and qualities.
    fn video_formats(
        &mut self,
        url: String,
        cookie: Option<String>,
        reply: tokio::sync::oneshot::Sender<Result<VideoListing, String>>,
    ) {
        if let Err(error) = ssrf::validate_http_url(&url) {
            let _ = reply.send(Err(error));
            return;
        }
        let dir = tools::tools_dir();
        if !tools::tools_ready(&dir) {
            self.ensure_tools();
            let _ = reply.send(Err(
                "the video tools are downloading, try again in a minute".to_string(),
            ));
            return;
        }
        let proxy = self.configured_proxy();
        thread::spawn(move || {
            let _ = reply.send(video::ytdlp_listing(
                &dir,
                &url,
                cookie.as_deref(),
                proxy.as_deref(),
            ));
        });
    }

    fn grab_page_links(
        &self,
        raw_url: String,
        reply: tokio::sync::oneshot::Sender<Result<Vec<String>, String>>,
    ) {
        let parsed = match Url::parse(raw_url.trim()) {
            Ok(url) => url,
            Err(_) => {
                let _ = reply.send(Err(
                    "Enter a valid http:// or https:// web page URL".to_string()
                ));
                return;
            }
        };
        if let Err(reason) = ssrf::validate_public_page_url(&parsed) {
            let _ = reply.send(Err(reason));
            return;
        }
        let proxy = (!self.settings.proxy_url.trim().is_empty())
            .then(|| self.settings.proxy_url.trim().to_string());
        self.runtime.spawn(async move {
            let _ = reply.send(ssrf::fetch_page_links(parsed, proxy).await);
        });
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Probe { request, result } => {
                self.in_flight_probes = self.in_flight_probes.saturating_sub(1);
                self.message = Some(self.handle_probe(request, *result));
            }
            Event::BridgeProbe {
                request,
                reply,
                result,
            } => {
                self.in_flight_probes = self.in_flight_probes.saturating_sub(1);
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
            Event::CaptureProbe { key, result } => {
                self.in_flight_probes = self.in_flight_probes.saturating_sub(1);
                if let Ok(result) = *result
                    && let Some(pending) = self.pending_captures.get_mut(&key)
                {
                    pending.file_name = resolve_file_name(
                        pending.request.filename.as_deref(),
                        result.content_disposition.as_deref(),
                        &result.final_url,
                    );
                    pending.size = result.size;
                    pending.probe = Some(result);
                }
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
            Event::VideoProgress {
                id,
                generation,
                done,
                total,
                filename,
            } => {
                if self.active.get(&id).map(|active| active.generation) != Some(generation) {
                    return;
                }
                let (cumulative_done, expected_size) = {
                    let tracker = self.video_streams.entry(id).or_default();
                    tracker.record(done, total, filename.as_deref())
                };
                if let Some(size) = expected_size
                    && size > 0
                {
                    let _ = self.store.set_video_size(id, size);
                    if let Some(download) = self.cached_downloads.iter_mut().find(|d| d.id == id) {
                        download.size = Some(size);
                    } else {
                        self.cache_dirty = true;
                    }
                }
                self.record_speed(id, cumulative_done);
                self.pending_segments.insert((id, 0), cumulative_done);
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
                    Ok(outcome) => {
                        let _ = self.store.set_size(id, outcome.bytes);
                        let _ = self.store.mark_done(id, now_millis());
                        self.cache_dirty = true;
                    }
                    Err(error) => {
                        let _ = self.store.update_status(
                            id,
                            DownloadStatus::Error,
                            Some(&error.to_string()),
                        );
                        self.cache_dirty = true;
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
        match self.progress.get(&id).copied() {
            None => {
                self.progress.insert(id, (done, now));
            }
            Some((previous, _)) if done < previous => {
                self.progress.insert(id, (done, now));
            }
            Some((previous, previous_time)) => {
                let seconds = now.duration_since(previous_time).as_secs_f64();
                if seconds >= 0.5 {
                    let instant = ((done - previous) as f64 / seconds).round() as u64;
                    let smoothed = compute_smoothed_speed(self.speeds.get(&id).copied(), instant);
                    self.speeds.insert(id, smoothed);
                    self.progress.insert(id, (done, now));
                }
            }
        }
    }

    fn decay_stalled_speeds(&mut self) -> bool {
        let now = Instant::now();
        let mut changed = false;
        for (&id, (_, at)) in &self.progress {
            if now.duration_since(*at) >= Duration::from_secs(2)
                && let Some(speed) = self.speeds.get_mut(&id)
                && *speed > 0
            {
                *speed = if *speed < 1024 { 0 } else { *speed / 2 };
                changed = true;
            }
        }
        changed
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
        if let Ok(total) = self.store.update_progress_batch(id, &updates) {
            for (idx, done) in updates {
                self.pending_segments.remove(&(id, idx));
                if let Some(list) = self.cached_segments.get_mut(&id)
                    && let Some(segment) = list.iter_mut().find(|seg| seg.idx == idx)
                {
                    segment.done = done;
                } else {
                    self.cache_dirty = true;
                }
            }
            if let Some(download) = self.cached_downloads.iter_mut().find(|d| d.id == id) {
                download.downloaded = total;
            } else {
                self.cache_dirty = true;
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
                self.cache_dirty = true;
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
                    self.cache_dirty = true;
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
        self.cache_dirty = true;

        if download.kind == "video" {
            return self.spawn_video(download, generation, cookie);
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
            let retry_delays = [
                Duration::from_secs(2),
                Duration::from_secs(5),
                Duration::from_secs(10),
            ];
            self.runtime.spawn(async move {
                let result = download_single_with_retry(
                    &client,
                    &spec,
                    Some(limiter),
                    &retry_delays,
                    |done| {
                        let _ = events.send(Event::Progress {
                            id,
                            generation,
                            done,
                        });
                    },
                )
                .await;
                let _ = events.send(Event::Finished {
                    id,
                    generation,
                    result,
                });
            })
        };
        self.active.insert(
            id,
            ActiveDownload {
                generation,
                handle: Some(handle),
                video: None,
            },
        );
        true
    }

    fn spawn_video(
        &mut self,
        download: &Download,
        generation: u64,
        cookie_text: Option<String>,
    ) -> bool {
        let dir = tools::tools_dir();
        if !tools::tools_ready(&dir) {
            // No download here: a test (or a fresh profile) must not hit the network.
            let _ = self.store.update_status(
                download.id,
                DownloadStatus::Error,
                Some("the video tools are not installed, install them in Settings"),
            );
            self.cache_dirty = true;
            return false;
        }
        let cookie_file = cookie_text
            .as_deref()
            .and_then(video::TempCookieFile::from_netscape);
        let js_runtime = tools::js_runtime_arg(&dir);
        let selector = download.video_format.as_deref().unwrap_or(DEFAULT_SELECTOR);
        let work_dir = video::ytdlp_work_dir(download.save_path.parent());
        let proxy = self.configured_proxy();
        let mut command = std::process::Command::new(tools::tool_path(&dir, tools::YTDLP));
        command
            .current_dir(work_dir)
            .args(video::video_args(
                selector,
                &download.save_path,
                &download.url,
                &dir,
                self.settings.video_subtitles,
                self.settings.video_chapters,
                js_runtime.as_deref(),
                cookie_file.as_ref().map(video::TempCookieFile::path),
                proxy.as_deref(),
                self.settings.global_speed_limit_kbps,
            ))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        tools::no_window(&mut command);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = self.store.update_status(
                    download.id,
                    DownloadStatus::Error,
                    Some(&format!("yt-dlp: {error}")),
                );
                self.cache_dirty = true;
                return false;
            }
        };
        let id = download.id;
        self.video_streams
            .insert(id, video::VideoStreamTracker::from_seeded(download.size));
        let stdout_join = child.stdout.take().map(|stdout| {
            let events = self.events.clone();
            thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if let Some(parsed) = video::parse_progress(&line) {
                        if video::is_auxiliary_media_file(parsed.filename.as_deref()) {
                            continue;
                        }
                        let _ = events.send(Event::VideoProgress {
                            id,
                            generation,
                            done: parsed.done,
                            total: parsed.total,
                            filename: parsed.filename,
                        });
                    }
                }
            })
        });
        let last_error = Arc::new(Mutex::new(String::new()));
        let stderr_join = child.stderr.take().map(|stderr| {
            let slot = Arc::clone(&last_error);
            thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    if let Ok(mut slot) = slot.lock() {
                        if !slot.is_empty() {
                            slot.push('\n');
                        }
                        if slot.len() < 4096 {
                            slot.push_str(line);
                        }
                    }
                }
            })
        });
        self.active.insert(
            id,
            ActiveDownload {
                generation,
                handle: None,
                video: Some(video::VideoRun {
                    child,
                    save_path: download.save_path.clone(),
                    last_error,
                    stdout_join,
                    stderr_join,
                    _cookie_file: cookie_file,
                }),
            },
        );
        true
    }

    fn poll_videos(&mut self) -> bool {
        let exited: Vec<(i64, bool)> = self
            .active
            .iter_mut()
            .filter_map(|(id, active)| {
                let status = active.video.as_mut()?.child.try_wait().ok()??;
                Some((*id, status.success()))
            })
            .collect();
        let changed = !exited.is_empty();
        for (id, success) in exited {
            let Some(mut active) = self.active.remove(&id) else {
                continue;
            };
            let Some(mut run) = active.video.take() else {
                continue;
            };
            if let Some(join) = run.stdout_join.take() {
                let _ = join.join();
            }
            if let Some(join) = run.stderr_join.take() {
                let _ = join.join();
            }
            self.flush_segment_progress(id);
            if success {
                // The row reserved its temp name before the run; a finished video leaves it empty.
                let _ = std::fs::remove_file(temp_path_for(&run.save_path));
                if let Some(actual_path) = video::resolve_video_output(&run.save_path) {
                    if actual_path != run.save_path
                        && let Some(actual_name) = actual_path.file_name().and_then(|n| n.to_str())
                    {
                        let actual_temp = temp_path_for(&actual_path);
                        let _ =
                            self.store
                                .update_location(id, actual_name, &actual_path, &actual_temp);
                    }
                    let actual_bytes = std::fs::metadata(&actual_path).ok().map(|m| m.len());
                    let _ = self.store.mark_video_done(id, actual_bytes, now_millis());
                } else {
                    let _ = self.store.update_status(
                        id,
                        DownloadStatus::Error,
                        Some("yt-dlp exited without producing an output file"),
                    );
                    video::remove_parts(&run.save_path);
                }
            } else {
                let raw = run
                    .last_error
                    .lock()
                    .map(|line| line.clone())
                    .unwrap_or_default();
                let message = video::format_ytdlp_error(&raw);
                let _ = self
                    .store
                    .update_status(id, DownloadStatus::Error, Some(&message));
                let _ = std::fs::remove_file(&run.save_path);
                video::remove_parts(&run.save_path);
            }
            self.cache_dirty = true;
            self.video_streams.remove(&id);
            self.speeds.remove(&id);
            self.progress.remove(&id);
            self.last_persist.remove(&id);
            self.clear_pending(id);
            self.pump_queue();
        }
        changed
    }

    fn reset_to_single(&mut self, id: i64, reason: &str) {
        let Ok(download) = self.store.get_download(id) else {
            return;
        };
        let segments = plan_segments(download.size.unwrap_or(0), false, 1);
        if self.store.replace_segments(id, &segments, 1, false).is_ok() {
            let _ = self.store.update_status(id, DownloadStatus::Queued, None);
            self.cache_dirty = true;
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
        if let Some(mut active) = self.active.remove(&id) {
            video::stop_video(&mut active.video);
            if let Some(handle) = active.handle.take() {
                handle.abort();
            }
            self.video_streams.remove(&id);
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
                self.cache_dirty = true;
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
                self.cache_dirty = true;
            }
        }
        self.pump_queue();
    }

    fn redownload(&mut self, id: i64) {
        let Ok(download) = self.store.get_download(id) else {
            return;
        };
        let cookie = match download.cookie_enc.as_deref() {
            Some(encrypted) => match unprotect_cookie(encrypted) {
                Ok(cookie) => Some(cookie),
                Err(error) => {
                    self.message = Some(format!("Redownload failed: cookie {error}"));
                    return;
                }
            },
            None => None,
        };
        let request = AddRequest {
            url: download.url,
            file_name: Some(download.file_name),
            save_dir: download.save_path.parent().map(Path::to_path_buf),
            start_now: true,
            connections: Some(download.connections.max(1)),
            referrer: download.referrer,
            user_agent: download.user_agent,
            cookie,
            video_format: download.video_format,
            video_size: download.size,
        };
        if let Some(text) = self.handle_add(request) {
            self.message = Some(text);
        }
    }

    fn update_url_and_resume(&mut self, id: i64, raw_url: String) {
        let Ok(parsed) = Url::parse(raw_url.trim()) else {
            self.message = Some("Invalid URL".to_string());
            return;
        };
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            self.message = Some("Only HTTP/HTTPS URLs are supported".to_string());
            return;
        }
        let Ok(download) = self.store.get_download(id) else {
            return;
        };
        if !matches!(
            download.status,
            DownloadStatus::Paused | DownloadStatus::Error
        ) {
            return;
        }
        if let Err(error) = self.store.update_url(id, parsed.as_str()) {
            self.message = Some(format!("Update URL failed: {error}"));
            return;
        }
        self.cache_dirty = true;
        self.message = Some("URL updated, resuming download".to_string());
        self.resume(id);
    }

    fn move_rename(&mut self, id: i64, raw_name: String, target_dir: Option<PathBuf>) {
        let Ok(download) = self.store.get_download(id) else {
            return;
        };
        if download.status != DownloadStatus::Done {
            self.message = Some("Only completed downloads can be moved or renamed".to_string());
            return;
        }
        if !download.save_path.exists() {
            self.message = Some("Source file no longer exists on disk".to_string());
            return;
        }
        let clean_name = sanitize_file_name(raw_name.trim());
        let dir = target_dir
            .filter(|p| !p.as_os_str().is_empty())
            .or_else(|| download.save_path.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| self.settings.download_dir.clone());
        if let Err(error) = ensure_dir(&dir) {
            self.message = Some(format!("Create folder failed: {error}"));
            return;
        }
        let dest = dir.join(&clean_name);
        if dest == download.save_path {
            return;
        }
        if dest.exists() {
            self.message = Some("Destination file already exists".to_string());
            return;
        }
        if let Err(error) = move_file_safe(&download.save_path, &dest) {
            self.message = Some(format!("Move failed: {error}"));
            return;
        }
        let temp = temp_path_for(&dest);
        if let Err(error) = self.store.update_location(id, &clean_name, &dest, &temp) {
            self.message = Some(format!("Update record failed: {error}"));
            return;
        }
        self.cache_dirty = true;
        self.message = Some(format!("Moved to {}", dest.display()));
    }

    fn remove(&mut self, id: i64, delete_file: bool) {
        if let Some(mut active) = self.active.remove(&id) {
            video::stop_video(&mut active.video);
            if let Some(handle) = active.handle.take() {
                handle.abort();
                let _ = self.runtime.block_on(handle);
            }
        }
        let details = self
            .store
            .get_download(id)
            .ok()
            .map(|download| (download.temp_path, download.save_path, download.kind));
        if self.store.delete_download(id).is_ok() {
            self.cache_dirty = true;
            if let Some((temp_path, save_path, kind)) = details {
                let _ = std::fs::remove_file(&temp_path);
                if kind == "video" {
                    video::remove_parts(&save_path);
                }
                if delete_file {
                    let _ = std::fs::remove_file(&save_path);
                }
            }
        }
        self.video_streams.remove(&id);
        self.speeds.remove(&id);
        self.progress.remove(&id);
        self.last_persist.remove(&id);
        self.clear_pending(id);
        self.pump_queue();
    }

    fn remove_completed(&mut self) {
        let completed: Vec<i64> = self
            .store
            .list_downloads()
            .unwrap_or_default()
            .into_iter()
            .filter(|download| download.status == DownloadStatus::Done)
            .map(|download| download.id)
            .collect();
        for id in completed {
            if self.store.delete_download(id).is_ok() {
                self.cache_dirty = true;
            }
        }
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
            if self
                .store
                .update_status(id, DownloadStatus::Queued, None)
                .is_ok()
            {
                self.cache_dirty = true;
            }
        }
        self.pump_queue();
    }

    fn update_settings(&mut self, mut settings: Settings) {
        settings.max_concurrent = settings.max_concurrent.clamp(1, 10);
        settings.max_connections = settings.max_connections.clamp(1, 32);
        settings.global_speed_limit_kbps = settings.global_speed_limit_kbps.min(1_000_000);
        settings.bridge_port = settings.bridge_port.clamp(1024, 65_535);
        settings.proxy_url = settings.proxy_url.trim().to_string();
        settings.last_update_check_ms = settings
            .last_update_check_ms
            .max(self.settings.last_update_check_ms);
        settings.browser_paired |= self.settings.browser_paired;
        if !is_valid_token(&settings.bridge_token) {
            settings.bridge_token = generate_token();
        }
        if settings.proxy_url != self.settings.proxy_url {
            match build_client_with_proxy(
                Duration::from_secs(10),
                Duration::from_secs(30),
                (!settings.proxy_url.is_empty()).then_some(settings.proxy_url.as_str()),
            ) {
                Ok(client) => self.client = client,
                Err(error) => {
                    self.message = Some(format!("Settings save failed: {error}"));
                    return;
                }
            }
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
        for (_, pending) in std::mem::take(&mut self.pending_captures) {
            let _ = pending.reply.send(BridgeAddOutcome::Canceled);
        }
        let active: Vec<i64> = self.active.keys().copied().collect();
        for id in active {
            self.flush_segment_progress(id);
            if let Some(mut download) = self.active.remove(&id) {
                video::stop_video(&mut download.video);
                if let Some(handle) = download.handle.take() {
                    handle.abort();
                }
                let _ = self.store.update_status(id, DownloadStatus::Paused, None);
                self.cache_dirty = true;
            }
            self.clear_pending(id);
        }
    }

    fn publish(&mut self) {
        if self.cache_dirty {
            let Ok(downloads) = self.store.list_downloads() else {
                return;
            };
            self.cached_downloads = downloads;
            self.cached_segments = self.store.all_segments().unwrap_or_default();
            self.cache_dirty = false;
        }
        let mut downloads = self.cached_downloads.clone();
        let mut segments = self.cached_segments.clone();
        apply_pending_segments_overlay(&mut downloads, &mut segments, &self.pending_segments);
        let snapshot = Snapshot {
            downloads,
            segments,
            settings: self.settings.clone(),
            speeds: self.speeds.clone(),
            message: self.message.clone(),
            bridge: self.bridge_status.clone(),
            pending_capture: self.pending_captures.iter().next().map(|(key, pending)| {
                CapturePrompt {
                    key: *key,
                    url: pending.request.url.clone(),
                    file_name: pending.file_name.clone(),
                    size: pending.size,
                    ranged: pending.probe.as_ref().map(|probe| probe.ranged),
                }
            }),
            wake_seq: self.wake_seq,
            exit_requested: self.exit_requested,
        };
        if let Ok(mut slot) = self.snapshots.lock() {
            slot.version = slot.version.saturating_add(1);
            slot.snapshot = Some(Arc::new(snapshot));
        }
    }
}

fn apply_pending_segments_overlay(
    downloads: &mut [Download],
    segments: &mut HashMap<i64, Vec<Segment>>,
    pending_segments: &HashMap<(i64, u32), u64>,
) {
    if pending_segments.is_empty() {
        return;
    }
    let mut touched_ids = Vec::new();
    for ((download_id, idx), done) in pending_segments {
        if let Some(list) = segments.get_mut(download_id)
            && let Some(segment) = list.iter_mut().find(|seg| seg.idx == *idx)
        {
            segment.done = *done;
            if !touched_ids.contains(download_id) {
                touched_ids.push(*download_id);
            }
        }
    }
    for id in touched_ids {
        if let Some(list) = segments.get(&id)
            && let Some(download) = downloads.iter_mut().find(|d| d.id == id)
        {
            download.downloaded = list.iter().map(|seg| seg.done).sum();
        }
    }
}

fn encrypt_cookie(raw: Option<&str>) -> Result<Option<Vec<u8>>, String> {
    match raw.map(str::trim).filter(|cookie| !cookie.is_empty()) {
        Some(cookie) => protect_cookie(cookie)
            .map(Some)
            .map_err(|error| format!("cookie: {error}")),
        None => Ok(None),
    }
}

fn compute_smoothed_speed(previous: Option<u64>, instant: u64) -> u64 {
    match previous {
        Some(prev) if prev > 0 => ((prev as f64 * 0.6) + (instant as f64 * 0.4)).round() as u64,
        _ => instant,
    }
}

fn move_file_safe(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(rename_err) => {
            if std::fs::copy(from, to).is_ok() {
                if let Err(rm_err) = std::fs::remove_file(from) {
                    let _ = std::fs::remove_file(to);
                    return Err(rm_err);
                }
                Ok(())
            } else {
                Err(rename_err)
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoothed_speed_blends_samples_without_wild_spikes() {
        assert_eq!(compute_smoothed_speed(None, 10_000_000), 10_000_000);
        assert_eq!(
            compute_smoothed_speed(Some(10_000_000), 20_000_000),
            14_000_000
        );
    }

    #[test]
    fn apply_pending_segments_overlay_updates_segments_and_downloaded_total() {
        let mut downloads = vec![Download {
            id: 7,
            url: "https://example.com/archive.zip".to_string(),
            final_url: None,
            file_name: "archive.zip".to_string(),
            save_path: PathBuf::from("archive.zip"),
            temp_path: PathBuf::from("archive.zip.lajufile"),
            size: Some(1_000),
            downloaded: 100,
            status: DownloadStatus::Downloading,
            category: Category::Compressed,
            kind: "http".to_string(),
            video_format: None,
            connections: 2,
            ranged: true,
            etag: None,
            last_modified: None,
            cookie_enc: None,
            referrer: None,
            user_agent: None,
            error: None,
            queue_order: 1,
            created_at: 0,
            completed_at: None,
        }];
        let mut segments = HashMap::from([(
            7,
            vec![
                Segment {
                    idx: 0,
                    start: 0,
                    end: 499,
                    done: 100,
                },
                Segment {
                    idx: 1,
                    start: 500,
                    end: 999,
                    done: 0,
                },
            ],
        )]);
        let pending = HashMap::from([((7, 0), 250), ((7, 1), 150), ((99, 0), 50)]);

        apply_pending_segments_overlay(&mut downloads, &mut segments, &pending);

        assert_eq!(segments[&7][0].done, 250);
        assert_eq!(segments[&7][1].done, 150);
        assert_eq!(downloads[0].downloaded, 400);
    }
}
