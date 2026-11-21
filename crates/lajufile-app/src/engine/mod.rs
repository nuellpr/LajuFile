mod actions;
pub mod extract;
pub mod huggingface;
mod intake;
mod speed;
mod ssrf;
mod video;

use speed::SpeedTracker;

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use lajufile_core::checksum::{checksums_match, normalize_checksum_input, sha256_file};
use lajufile_core::cookie::{protect_cookie, unprotect_cookie};
use lajufile_core::download::{
    DownloadError, DownloadOutcome, DownloadSpec, download_single_from_response_or_retry,
};
use lajufile_core::http::{
    DEFAULT_USER_AGENT, RequestOptions, build_client, build_client_with_proxy,
};
use lajufile_core::naming::{dedupe_path, resolve_file_name, sanitize_file_name, temp_path_for};
use lajufile_core::paths::ensure_dir;
use lajufile_core::probe::{
    ProbeError, ProbeRequest, ProbeResponse, ProbeResult, probe_with_response,
};
use lajufile_core::segment::plan_segments;
use lajufile_core::segmented::{SegmentedError, SegmentedSpec, download_segmented_with_splits};
use lajufile_core::settings::{
    DuplicatePolicy, QueueCompleteAction, Settings, generate_token, is_valid_token,
    is_within_schedule, parse_hhmm,
};
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
    MoveToTop(i64),
    ForceStart(i64),
    SetItemSpeedLimit {
        id: i64,
        kbps: u64,
    },
    CancelQueueComplete,
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
    LoadHfFiles {
        url: String,
        reply: tokio::sync::oneshot::Sender<Result<Vec<huggingface::HfModelFile>, String>>,
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
    /// Optional expected SHA-256 hex digest to auto-verify on completion.
    pub expected_sha256: Option<String>,
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
    pub pending_queue_action: Option<(QueueCompleteAction, i64)>,
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
        check_duplicate: bool,
        result: Box<Result<ProbeResponse, ProbeError>>,
    },
    BridgeProbe {
        request: AddRequest,
        reply: tokio::sync::oneshot::Sender<BridgeAddOutcome>,
        result: Box<Result<ProbeResponse, ProbeError>>,
    },
    CaptureProbe {
        key: u64,
        result: Box<Result<ProbeResponse, ProbeError>>,
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
    SegmentsSplit {
        id: i64,
        generation: u64,
        segments: Vec<Segment>,
    },
    ChecksumVerified {
        id: i64,
        status: String,
        error: Option<String>,
    },
    Extracted {
        message: String,
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
    item_limiter: Arc<RateLimiter>,
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
    progress: HashMap<i64, SpeedTracker>,
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
    prefetched_responses: HashMap<i64, reqwest::Response>,
    schedule_was_active: Option<bool>,
    last_schedule_check: Instant,
    had_completed_in_batch: bool,
    pending_queue_action: Option<(QueueCompleteAction, i64)>,
}

struct PendingCapture {
    request: BridgeAddRequest,
    reply: tokio::sync::oneshot::Sender<BridgeAddOutcome>,
    file_name: String,
    size: Option<u64>,
    probe: Option<ProbeResult>,
    probe_response: Option<reqwest::Response>,
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
        .worker_threads(2)
        .max_blocking_threads(4)
        .thread_stack_size(1024 * 1024)
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
                    pending_queue_action: None,
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
        prefetched_responses: HashMap::new(),
        schedule_was_active: None,
        last_schedule_check: Instant::now() - Duration::from_secs(10),
        had_completed_in_batch: false,
        pending_queue_action: None,
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
        state.tick_scheduler();
        state.tick_pending_queue_action();

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
        video::cleanup_stale_cookie_files(&std::env::temp_dir());
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
            Command::MoveToTop(id) => self.move_to_top(id),
            Command::ForceStart(id) => self.force_start(id),
            Command::SetItemSpeedLimit { id, kbps } => self.set_item_speed_limit(id, kbps),
            Command::CancelQueueComplete => {
                if self.pending_queue_action.take().is_some() {
                    self.message = Some("Scheduled post-download action canceled".to_string());
                    self.immediate_publish = true;
                }
            }
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
            Command::LoadHfFiles { url, reply } => self.load_hf_files(url, reply),
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

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Probe {
                request,
                check_duplicate,
                result,
            } => {
                self.in_flight_probes = self.in_flight_probes.saturating_sub(1);
                self.message = Some(self.handle_probe(request, check_duplicate, *result));
            }
            Event::BridgeProbe {
                request,
                reply,
                result,
            } => {
                self.in_flight_probes = self.in_flight_probes.saturating_sub(1);
                let outcome = match *result {
                    Err(error) => BridgeAddOutcome::ProbeFailed(error.to_string()),
                    Ok(probed) => {
                        match self.insert_from_probe(request, probed.result, probed.response, true)
                        {
                            Ok(inserted) => BridgeAddOutcome::Created {
                                id: inserted.id,
                                filename: inserted.file_name,
                                size: inserted.size,
                            },
                            Err(InsertError::Duplicate(existing)) => {
                                BridgeAddOutcome::Conflict { id: existing.id }
                            }
                            Err(InsertError::Failed(error)) => BridgeAddOutcome::Internal(error),
                        }
                    }
                };
                let _ = reply.send(outcome);
            }
            Event::CaptureProbe { key, result } => {
                self.in_flight_probes = self.in_flight_probes.saturating_sub(1);
                if let Ok(probed) = *result
                    && let Some(pending) = self.pending_captures.get_mut(&key)
                {
                    pending.file_name = resolve_file_name(
                        pending.request.filename.as_deref(),
                        probed.result.content_disposition.as_deref(),
                        &probed.result.final_url,
                    );
                    pending.size = probed.result.size;
                    pending.probe = Some(probed.result);
                    pending.probe_response = probed.response;
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
                    let size_changed = self
                        .cached_downloads
                        .iter_mut()
                        .find(|d| d.id == id)
                        .map(|download| {
                            if download.size != Some(size) {
                                download.size = Some(size);
                                true
                            } else {
                                false
                            }
                        })
                        .unwrap_or_else(|| {
                            self.cache_dirty = true;
                            true
                        });
                    if size_changed {
                        let _ = self.store.set_video_size(id, size);
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
            Event::SegmentsSplit {
                id,
                generation,
                segments,
            } => {
                if self.active.get(&id).map(|active| active.generation) != Some(generation) {
                    return;
                }
                if self.store.upsert_segments(id, &segments).is_ok() {
                    self.cached_segments.insert(id, segments);
                } else {
                    self.cache_dirty = true;
                }
            }
            Event::ChecksumVerified { id, status, error } => {
                let _ = self
                    .store
                    .update_checksum_result(id, &status, error.as_deref());
                if let Some(err) = error {
                    self.message = Some(err);
                } else if status == "verified"
                    && let Ok(row) = self.store.get_download(id)
                {
                    self.maybe_spawn_auto_extract(&row.save_path);
                }
                self.cache_dirty = true;
                self.immediate_publish = true;
            }
            Event::Extracted { message } => {
                self.message = Some(message);
                self.immediate_publish = true;
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
                        self.had_completed_in_batch = true;
                        self.cache_dirty = true;
                        if let Ok(row) = self.store.get_download(id)
                            && let Some(expected) = row.expected_sha256
                        {
                            let path = outcome.final_path;
                            let events = self.events.clone();
                            thread::spawn(move || {
                                let (status, error) = match sha256_file(&path) {
                                    Ok(actual) if checksums_match(&expected, &actual) => {
                                        ("verified".to_string(), None)
                                    }
                                    Ok(actual) => (
                                        "mismatch".to_string(),
                                        Some(format!(
                                            "SHA-256 mismatch (got {}…)",
                                            &actual[..actual.len().min(12)]
                                        )),
                                    ),
                                    Err(err) => (
                                        "mismatch".to_string(),
                                        Some(format!("SHA-256 verification failed: {err}")),
                                    ),
                                };
                                let _ = events.send(Event::ChecksumVerified { id, status, error });
                            });
                        } else {
                            self.maybe_spawn_auto_extract(&outcome.final_path);
                        }
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
                self.check_queue_complete();
            }
        }
    }

    fn maybe_spawn_auto_extract(&self, path: &Path) {
        if !self.settings.auto_extract_archives || !extract::is_supported_archive(path) {
            return;
        }
        let archive = path.to_path_buf();
        let events = self.events.clone();
        thread::spawn(move || {
            let name = archive
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "archive".to_string());
            let message = match extract::extract_archive_safely(&archive) {
                Ok(dest) => format!(
                    "Extracted {name} → {}",
                    dest.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                ),
                Err(err) => format!("Auto-extract skipped for {name}: {err}"),
            };
            let _ = events.send(Event::Extracted { message });
        });
    }

    fn tick_scheduler(&mut self) {
        if !self.settings.schedule_enabled {
            self.schedule_was_active = None;
            return;
        }
        if self.last_schedule_check.elapsed() < Duration::from_secs(5) {
            return;
        }
        self.last_schedule_check = Instant::now();
        let (Some(start), Some(stop)) = (
            parse_hhmm(&self.settings.schedule_start),
            parse_hhmm(&self.settings.schedule_stop),
        ) else {
            return;
        };
        let Some(now_mins) = crate::platform::local_minutes_since_midnight() else {
            return;
        };
        let in_window = is_within_schedule(now_mins, start, stop);
        match self.schedule_was_active {
            None => {
                self.schedule_was_active = Some(in_window);
                if !in_window && !self.active.is_empty() {
                    self.pause_all();
                    self.message = Some("Paused outside scheduled window".to_string());
                    self.dirty = true;
                }
            }
            Some(false) if in_window => {
                self.schedule_was_active = Some(true);
                self.resume_all();
                self.message = Some("Scheduler started downloads".to_string());
                self.dirty = true;
            }
            Some(true) if !in_window => {
                self.schedule_was_active = Some(false);
                self.pause_all();
                self.message = Some("Scheduler paused downloads".to_string());
                self.dirty = true;
            }
            _ => {}
        }
    }

    pub(super) fn check_queue_complete(&mut self) {
        let action = self.settings.on_queue_complete;
        if action == QueueCompleteAction::None
            || !self.had_completed_in_batch
            || !self.active.is_empty()
            || self.pending_queue_action.is_some()
        {
            return;
        }
        let still_queued = self
            .store
            .list_downloads()
            .map(|rows| {
                rows.iter().any(|d| {
                    matches!(
                        d.status,
                        DownloadStatus::Queued | DownloadStatus::Downloading
                    )
                })
            })
            .unwrap_or(true);
        if still_queued {
            return;
        }
        self.had_completed_in_batch = false;
        // One-shot safety: reset persisted action so the next launch never auto-shuts down.
        self.settings.on_queue_complete = QueueCompleteAction::None;
        let _ = self.settings.save(&self.data_dir);
        self.pending_queue_action = Some((action, now_millis() + 10_000));
        self.immediate_publish = true;
    }

    fn tick_pending_queue_action(&mut self) {
        let Some((action, deadline_ms)) = self.pending_queue_action else {
            return;
        };
        self.dirty = true;
        if now_millis() < deadline_ms {
            return;
        }
        self.pending_queue_action = None;
        self.immediate_publish = true;
        match action {
            QueueCompleteAction::None => {}
            QueueCompleteAction::ExitApp => self.request_exit(),
            QueueCompleteAction::Sleep | QueueCompleteAction::Shutdown => {
                crate::platform::trigger_power_action(action);
            }
        }
    }

    fn record_speed(&mut self, id: i64, done: u64) {
        let now = Instant::now();
        let previous_speed = self.speeds.get(&id).copied();
        let tracker = self
            .progress
            .entry(id)
            .or_insert_with(|| SpeedTracker::new(now, done));
        if let Some(speed) = tracker.record_at(now, done, previous_speed) {
            self.speeds.insert(id, speed);
        }
    }

    fn decay_stalled_speeds(&mut self) -> bool {
        let now = Instant::now();
        let mut changed = false;
        for (&id, tracker) in &mut self.progress {
            if let Some(speed) = self.speeds.get_mut(&id)
                && tracker.decay_at(now, speed)
            {
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
            pending_queue_action: self.pending_queue_action,
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

#[cfg(test)]
mod tests {
    use super::*;

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
            speed_limit_kbps: 0,
            expected_sha256: None,
            checksum_status: None,
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
