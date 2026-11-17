use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use lajufile_core::cookie::{protect_cookie, unprotect_cookie};
use lajufile_core::download::{
    DownloadError, DownloadOutcome, DownloadSpec, download_single_with_limiter,
};
use lajufile_core::http::{DEFAULT_USER_AGENT, RequestOptions, build_client};
use lajufile_core::naming::{dedupe_path, resolve_file_name, sanitize_file_name, temp_path_for};
use lajufile_core::paths::ensure_dir;
use lajufile_core::probe::{ProbeError, ProbeRequest, ProbeResult, probe};
use lajufile_core::segment::plan_segments;
use lajufile_core::segmented::{SegmentedError, SegmentedSpec, download_segmented};
use lajufile_core::settings::{DuplicatePolicy, Settings, generate_token};
use lajufile_core::speed::RateLimiter;
use lajufile_core::store::{
    Category, Download, DownloadStatus, NewDownload, Segment, Store, StoreError, now_millis,
};
use lajufile_core::video::{self, DEFAULT_SELECTOR, VideoListing};
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
    WakeWindow,
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

fn validate_http_url(raw: &str) -> Result<Url, String> {
    let url = Url::parse(raw.trim()).map_err(|error| format!("invalid URL: {error}"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("only http:// and https:// URLs are supported".to_string());
    }
    Ok(url)
}

fn is_public_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let oct = v4.octets();
            let shared_cgnat = oct[0] == 100 && (oct[1] & 0xc0) == 64;
            let benchmark = oct[0] == 198 && (oct[1] & 0xfe) == 18;
            let reserved = oct[0] == 0 || oct[0] >= 240;
            !v4.is_private()
                && !v4.is_loopback()
                && !v4.is_link_local()
                && !v4.is_broadcast()
                && !v4.is_documentation()
                && !v4.is_unspecified()
                && !v4.is_multicast()
                && !shared_cgnat
                && !benchmark
                && !reserved
        }
        std::net::IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_public_ip(std::net::IpAddr::V4(mapped));
            }
            let seg = v6.segments();
            let ula = (seg[0] & 0xfe00) == 0xfc00;
            let link_local = (seg[0] & 0xffc0) == 0xfe80;
            let site_local = (seg[0] & 0xffc0) == 0xfec0;
            !v6.is_loopback()
                && !v6.is_unspecified()
                && !v6.is_multicast()
                && !ula
                && !link_local
                && !site_local
        }
    }
}

fn validate_public_page_url(url: &Url) -> Result<(), String> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Only http:// or https:// web page URLs are supported".to_string());
    }
    let Some(host) = url.host() else {
        return Err("Web page URL is missing a host".to_string());
    };
    match host {
        url::Host::Ipv4(v4) => {
            if !is_public_ip(std::net::IpAddr::V4(v4)) {
                return Err("Refusing to scan private or local IP addresses".to_string());
            }
        }
        url::Host::Ipv6(v6) => {
            if !is_public_ip(std::net::IpAddr::V6(v6)) {
                return Err("Refusing to scan private or local IP addresses".to_string());
            }
        }
        url::Host::Domain(domain) => {
            let lower = domain.trim_end_matches('.').to_ascii_lowercase();
            if lower == "localhost"
                || lower.ends_with(".localhost")
                || lower.ends_with(".local")
                || lower.ends_with(".internal")
            {
                return Err("Refusing to scan localhost or internal domains".to_string());
            }
        }
    }
    Ok(())
}

/// Reserves a deduplicated save path and empty `.lajufile` sentinel so queued rows
/// cannot collide before download writes begin.
fn reserve_download_paths(dir: &Path, base_name: &str) -> Result<(PathBuf, PathBuf), String> {
    let mut final_path = dedupe_path(dir, base_name);
    loop {
        let candidate = temp_path_for(&final_path);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(_) => return Ok((final_path, candidate)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                final_path = dedupe_path(dir, base_name);
            }
            Err(error) => return Err(error.to_string()),
        }
    }
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
    video: Option<VideoRun>,
}

/// A running `yt-dlp` child; the engine polls it for exit instead of awaiting it,
/// so pause and cancel can kill the process.
struct VideoRun {
    child: std::process::Child,
    save_path: PathBuf,
    last_error: Arc<Mutex<String>>,
    // Dropped automatically when the yt-dlp process exits, stops, or is aborted.
    _cookie_file: Option<TempCookieFile>,
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
    video_streams: HashMap<i64, VideoStreamTracker>,
    pending_segments: HashMap<(i64, u32), u64>,
    last_persist: HashMap<i64, Instant>,
    message: Option<String>,
    pending_captures: BTreeMap<u64, PendingCapture>,
    next_capture_key: u64,
    wake_seq: u64,
    immediate_publish: bool,
    wake_callback: Arc<RwLock<Option<WakeCallback>>>,
    dirty: bool,
}

struct PendingCapture {
    request: BridgeAddRequest,
    reply: tokio::sync::oneshot::Sender<BridgeAddOutcome>,
    file_name: String,
    size: Option<u64>,
    probe: Option<ProbeResult>,
}

#[derive(Default)]
struct VideoStreamTracker {
    seeded_size: Option<u64>,
    current_file: Option<String>,
    completed_bytes: u64,
    current_peak: u64,
    current_total: u64,
}

impl VideoStreamTracker {
    fn from_seeded(seeded_size: Option<u64>) -> Self {
        Self {
            seeded_size: seeded_size.filter(|s| *s > 0),
            ..Self::default()
        }
    }

    fn record(
        &mut self,
        done: u64,
        total: Option<u64>,
        filename: Option<&str>,
    ) -> (u64, Option<u64>) {
        if let Some(fname) = filename {
            if self.current_file.as_deref() != Some(fname) {
                if self.current_file.is_some() {
                    let finished = self.current_total.max(self.current_peak);
                    self.completed_bytes = self.completed_bytes.saturating_add(finished);
                    self.current_peak = 0;
                    self.current_total = 0;
                }
                self.current_file = Some(fname.to_string());
            }
        } else if self.current_peak > 0 && done < self.current_peak / 2 {
            // Filename omitted by yt-dlp; treat a sharp drop back toward zero as a new stream.
            let finished = self.current_total.max(self.current_peak);
            self.completed_bytes = self.completed_bytes.saturating_add(finished);
            self.current_peak = 0;
            self.current_total = 0;
        }
        self.current_peak = self.current_peak.max(done);
        if let Some(t) = total
            && t > 0
        {
            self.current_total = t;
        }
        let cumulative_done = self.completed_bytes.saturating_add(done);
        let observed_total = self
            .completed_bytes
            .saturating_add(self.current_total.max(done));
        let expected_size = if self.current_total > 0 {
            Some(
                self.seeded_size
                    .map_or(observed_total, |seeded| seeded.max(observed_total)),
            )
        } else {
            self.seeded_size
        };
        (cumulative_done, expected_size)
    }
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
    let client = match build_client(Duration::from_secs(10), Duration::from_secs(30)) {
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
    if !valid_bridge_token(&settings.bridge_token) {
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
        message: None,
        pending_captures: BTreeMap::new(),
        next_capture_key: 0,
        wake_seq: 0,
        immediate_publish: false,
        wake_callback,
        dirty: false,
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

        if state.immediate_publish
            || (state.dirty && last_publish.elapsed() >= Duration::from_millis(250))
        {
            state.decay_stalled_speeds();
            state.publish();
            state.immediate_publish = false;
            state.dirty = false;
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
            Command::WakeWindow => self.trigger_wake(),
            Command::Shutdown => {
                self.shutdown();
                return true;
            }
        }
        false
    }

    fn trigger_wake(&mut self) {
        self.wake_seq = self.wake_seq.saturating_add(1);
        self.immediate_publish = true;
        if let Ok(guard) = self.wake_callback.read()
            && let Some(cb) = guard.as_ref()
        {
            cb();
        }
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
        let url = match validate_http_url(&request.url) {
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
        let url = match validate_http_url(&request.url) {
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
                    .unwrap_or_else(|| video_file_name(&add.url))
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
                size: None,
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
            url: pending.request.url,
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
        let dir = request
            .save_dir
            .clone()
            .unwrap_or_else(|| self.settings.download_dir.clone());
        if let Err(error) = ensure_dir(&dir) {
            return Err(InsertError::Failed(error.to_string()));
        }

        let base_name = sanitize_file_name(
            &request
                .file_name
                .clone()
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| video_file_name(&request.url)),
        );
        let base_name = if base_name.to_ascii_lowercase().ends_with(".mp4") {
            base_name
        } else {
            format!("{base_name}.mp4")
        };
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
        let new = NewDownload {
            url: request.url.clone(),
            final_url: Some(request.url),
            file_name: file_name.clone(),
            temp_path,
            save_path,
            size: request.video_size.filter(|s| *s > 0),
            status,
            category: Category::Video,
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
                self.pump_queue();
                Ok(Inserted {
                    id,
                    file_name,
                    size: None,
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

    fn spawn_tools_install(&mut self) {
        if self.tools_installing {
            self.message = Some("Downloading tools...".to_string());
            return;
        }
        let dir = tools::tools_dir();
        self.tools_installing = true;
        self.message = Some("Downloading tools...".to_string());
        let commands = self.commands.clone();
        thread::spawn(move || {
            let result = tools::install_tools(&dir);
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
        if let Err(error) = validate_http_url(&url) {
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
        thread::spawn(move || {
            let _ = reply.send(ytdlp_listing(&dir, &url, cookie.as_deref()));
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
        if let Err(reason) = validate_public_page_url(&parsed) {
            let _ = reply.send(Err(reason));
            return;
        }
        self.runtime.spawn(async move {
            let _ = reply.send(fetch_page_links(parsed).await);
        });
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
            Event::CaptureProbe { key, result } => {
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

    fn decay_stalled_speeds(&mut self) {
        let now = Instant::now();
        for (&id, (_, at)) in &self.progress {
            if now.duration_since(*at) >= Duration::from_secs(2)
                && let Some(speed) = self.speeds.get_mut(&id)
                && *speed > 0
            {
                *speed = if *speed < 1024 { 0 } else { *speed / 2 };
            }
        }
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
        if download.kind == "video" {
            return self.spawn_video(download, generation);
        }
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

    fn spawn_video(&mut self, download: &Download, generation: u64) -> bool {
        let dir = tools::tools_dir();
        if !tools::tools_ready(&dir) {
            // No download here: a test (or a fresh profile) must not hit the network.
            let _ = self.store.update_status(
                download.id,
                DownloadStatus::Error,
                Some("the video tools are not installed, install them in Settings"),
            );
            return false;
        }
        let cookie_text = match download.cookie_enc.as_deref() {
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
        let cookie_file = cookie_text
            .as_deref()
            .and_then(TempCookieFile::from_netscape);
        let js_runtime = tools::js_runtime_arg(&dir);
        let selector = download.video_format.as_deref().unwrap_or(DEFAULT_SELECTOR);
        let mut command = std::process::Command::new(tools::tool_path(&dir, tools::YTDLP));
        command
            .args(video_args(
                selector,
                &download.save_path,
                &download.url,
                &dir,
                self.settings.video_subtitles,
                self.settings.video_chapters,
                js_runtime.as_deref(),
                cookie_file.as_ref().map(TempCookieFile::path),
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
                return false;
            }
        };
        let id = download.id;
        self.video_streams
            .insert(id, VideoStreamTracker::from_seeded(download.size));
        if let Some(stdout) = child.stdout.take() {
            let events = self.events.clone();
            thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if let Some(parsed) = parse_progress(&line) {
                        if is_auxiliary_media_file(parsed.filename.as_deref()) {
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
            });
        }
        let last_error = Arc::new(Mutex::new(String::new()));
        if let Some(stderr) = child.stderr.take() {
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
            });
        }
        self.active.insert(
            id,
            ActiveDownload {
                generation,
                handle: None,
                video: Some(VideoRun {
                    child,
                    save_path: download.save_path.clone(),
                    last_error,
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
            let Some(run) = active.video.take() else {
                continue;
            };
            self.flush_segment_progress(id);
            if success {
                // The row reserved its temp name before the run; a finished video leaves it empty.
                let _ = std::fs::remove_file(temp_path_for(&run.save_path));
                let actual_bytes = std::fs::metadata(&run.save_path).ok().map(|m| m.len());
                let _ = self.store.mark_video_done(id, actual_bytes, now_millis());
            } else {
                let raw = run
                    .last_error
                    .lock()
                    .map(|line| line.clone())
                    .unwrap_or_default();
                let message = format_ytdlp_error(&raw);
                let _ = self
                    .store
                    .update_status(id, DownloadStatus::Error, Some(&message));
                let _ = std::fs::remove_file(&run.save_path);
                remove_parts(&run.save_path);
            }
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
            stop_video(&mut active);
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
        self.message = Some(format!("Moved to {}", dest.display()));
    }

    fn remove(&mut self, id: i64, delete_file: bool) {
        if let Some(mut active) = self.active.remove(&id) {
            stop_video(&mut active);
            if let Some(handle) = active.handle.take() {
                handle.abort();
                let _ = self.runtime.block_on(handle);
            }
        }
        let paths = self
            .store
            .get_download(id)
            .ok()
            .map(|download| (download.temp_path, download.save_path));
        if self.store.delete_download(id).is_ok()
            && let Some((temp_path, save_path)) = paths
        {
            let _ = std::fs::remove_file(&temp_path);
            remove_parts(&save_path);
            if delete_file {
                let _ = std::fs::remove_file(&save_path);
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
            let _ = self.store.delete_download(id);
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
            let _ = self.store.update_status(id, DownloadStatus::Queued, None);
        }
        self.pump_queue();
    }

    fn update_settings(&mut self, mut settings: Settings) {
        settings.max_concurrent = settings.max_concurrent.clamp(1, 10);
        settings.max_connections = settings.max_connections.clamp(1, 32);
        settings.global_speed_limit_kbps = settings.global_speed_limit_kbps.min(1_000_000);
        settings.bridge_port = settings.bridge_port.clamp(1024, 65_535);
        settings.browser_paired |= self.settings.browser_paired;
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
        for (_, pending) in std::mem::take(&mut self.pending_captures) {
            let _ = pending.reply.send(BridgeAddOutcome::Canceled);
        }
        let active: Vec<i64> = self.active.keys().copied().collect();
        for id in active {
            self.flush_segment_progress(id);
            if let Some(mut download) = self.active.remove(&id) {
                stop_video(&mut download);
                if let Some(handle) = download.handle.take() {
                    handle.abort();
                }
                let _ = self.store.update_status(id, DownloadStatus::Paused, None);
            }
            self.clear_pending(id);
        }
    }

    fn publish(&mut self) {
        let Ok(downloads) = self.store.list_downloads() else {
            return;
        };
        let mut segments = self.store.all_segments().unwrap_or_default();
        for ((download_id, idx), done) in &self.pending_segments {
            if let Some(list) = segments.get_mut(download_id)
                && let Some(segment) = list.iter_mut().find(|seg| seg.idx == *idx)
            {
                segment.done = *done;
            }
        }
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
        };
        if let Ok(mut slot) = self.snapshots.lock() {
            slot.version = slot.version.saturating_add(1);
            slot.snapshot = Some(Arc::new(snapshot));
        }
    }
}

/// File name for a video link that came without one: the YouTube id, else the last path segment.
fn video_file_name(url: &str) -> String {
    let id = Url::parse(url)
        .ok()
        .and_then(|url| {
            url.query_pairs()
                .find(|(key, _)| key == "v")
                .map(|(_, value)| value.into_owned())
                .or_else(|| {
                    url.path_segments()
                        .and_then(|mut segments| segments.next_back().map(str::to_string))
                })
        })
        .unwrap_or_default();
    let safe: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let safe = safe.trim_matches('-');
    if safe.is_empty() {
        "video.mp4".to_string()
    } else {
        format!("{safe}.mp4")
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

/// Short-lived Netscape cookie file for `yt-dlp --cookies`, deleted automatically on drop.
struct TempCookieFile {
    path: PathBuf,
}

impl TempCookieFile {
    fn from_netscape(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        if !trimmed.starts_with("# Netscape HTTP Cookie File") {
            return None;
        }
        let has_entries = trimmed
            .lines()
            .any(|line| !line.is_empty() && !line.starts_with('#') && line.contains('\t'));
        if !has_entries {
            return None;
        }

        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let pid = std::process::id();
        let dir = std::env::temp_dir();
        for _ in 0..8 {
            let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = dir.join(format!("lajufile-yt-{pid}-{seq}.cookies"));
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            match opts.open(&path) {
                Ok(mut file) => {
                    use std::io::Write;
                    if file.write_all(trimmed.as_bytes()).is_err() || file.write_all(b"\n").is_err()
                    {
                        drop(file);
                        let _ = std::fs::remove_file(&path);
                        return None;
                    }
                    return Some(Self { path });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return None,
            }
        }
        None
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempCookieFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn format_ytdlp_error(stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.contains("Sign in to confirm you're not a bot") {
        return "yt-dlp: YouTube requires a signed-in browser session on this network. Log in to YouTube in your browser and retry from the LajuFile extension (paired with this app).".to_string();
    }
    if let Some(err_line) = trimmed
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with("ERROR:"))
    {
        return format!("yt-dlp: {}", err_line.trim());
    }
    if trimmed.is_empty() {
        "yt-dlp failed".to_string()
    } else {
        format!("yt-dlp: {trimmed}")
    }
}

/// `yt-dlp -J`, killed if it stalls: a hung run must not outlive the request.
fn ytdlp_listing(dir: &Path, url: &str, cookie: Option<&str>) -> Result<VideoListing, String> {
    let cookie_file = cookie.and_then(TempCookieFile::from_netscape);
    let js_runtime = tools::js_runtime_arg(dir);
    let mut command = std::process::Command::new(tools::tool_path(dir, tools::YTDLP));
    command.args(["--no-playlist", "-J"]);
    if let Some(runtime) = js_runtime.as_deref() {
        command.args(["--js-runtimes", runtime]);
    }
    if let Some(cookies) = cookie_file.as_ref() {
        command.arg("--cookies").arg(cookies.path());
    }
    command
        .args(["--", url])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    tools::no_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("yt-dlp: {error}"))?;
    let stdout = read_all(child.stdout.take());
    let stderr = read_all(child.stderr.take());
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                tools::kill_tree(child.id());
                let _ = child.wait();
                return Err("yt-dlp did not answer in time".to_string());
            }
            Err(error) => return Err(format!("yt-dlp: {error}")),
        }
    };
    let output = stdout.join().unwrap_or_default();
    if !status.success() {
        let error = String::from_utf8_lossy(&stderr.join().unwrap_or_default()).to_string();
        return Err(format_ytdlp_error(&error));
    }
    let json: serde_json::Value =
        serde_json::from_slice(&output).map_err(|error| format!("yt-dlp: {error}"))?;
    let listing = video::listing(&json);
    if listing.choices.is_empty() {
        return Err("this link has no downloadable formats".to_string());
    }
    Ok(listing)
}

fn read_all<R: Read + Send + 'static>(reader: Option<R>) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut buffer = Vec::new();
        if let Some(mut reader) = reader {
            let _ = reader.read_to_end(&mut buffer);
        }
        buffer
    })
}

async fn verify_public_dns(url: &Url) -> Result<(), String> {
    validate_public_page_url(url)?;
    let host = url
        .host_str()
        .ok_or_else(|| "Web page URL is missing a host".to_string())?;
    let port = url.port_or_known_default().unwrap_or(80);
    let addrs = tokio::net::lookup_host((host, port))
        .await
        .map_err(|error| format!("Resolve failed: {error}"))?;
    let mut saw_addr = false;
    for addr in addrs {
        saw_addr = true;
        if !is_public_ip(addr.ip()) {
            return Err("Refusing to scan private or local network address".to_string());
        }
    }
    if !saw_addr {
        return Err("Could not resolve host".to_string());
    }
    Ok(())
}

/// Fetches up to 2 MB of HTML from one public web page and extracts unique link targets.
async fn fetch_page_links(url: Url) -> Result<Vec<String>, String> {
    const MAX_PAGE_BYTES: usize = 2 * 1024 * 1024;
    verify_public_dns(&url).await?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                return attempt.stop();
            }
            if let Err(msg) = validate_public_page_url(attempt.url()) {
                return attempt.error(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    msg,
                ));
            }
            attempt.follow()
        }))
        .build()
        .map_err(|error| format!("Client failed: {error}"))?;
    let mut response = client
        .get(url)
        .header(reqwest::header::USER_AGENT, DEFAULT_USER_AGENT)
        .send()
        .await
        .map_err(|error| format!("Fetch failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Fetch failed: {error}"))?;
    verify_public_dns(response.url()).await?;
    if let Some(content_type) = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
    {
        let lower = content_type.to_ascii_lowercase();
        if !lower.contains("html") && !lower.contains("text/plain") {
            return Err(format!("Page is not HTML ({content_type})"));
        }
    }
    let final_url = response.url().clone();
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("Read failed: {error}"))?
    {
        let remaining = MAX_PAGE_BYTES.saturating_sub(bytes.len());
        if remaining == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        if bytes.len() >= MAX_PAGE_BYTES {
            break;
        }
    }
    let html = String::from_utf8_lossy(&bytes);
    Ok(crate::ui::extract_links_from_html(
        final_url.as_str(),
        &html,
    ))
}

/// yt-dlp arguments for one video job: merge to mp4 next to the target path.
#[allow(clippy::too_many_arguments)]
fn video_args(
    selector: &str,
    save_path: &Path,
    url: &str,
    tools_dir: &Path,
    subtitles: bool,
    embed_chapters: bool,
    js_runtime: Option<&str>,
    cookie_file: Option<&Path>,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "--no-playlist".into(),
        "--newline".into(),
        "--merge-output-format".into(),
        "mp4".into(),
        // Prefer H.264 MP4 + AAC M4A and transcode non-AAC audio so Windows players have sound.
        "-S".into(),
        "ext:mp4:m4a,vcodec:h264,acodec:aac".into(),
        "--ppa".into(),
        "Merger+ffmpeg_o1:-c:a aac".into(),
        "--ffmpeg-location".into(),
        tools_dir.as_os_str().to_owned(),
    ];
    if let Some(runtime) = js_runtime {
        args.push("--js-runtimes".into());
        args.push(runtime.into());
    }
    if let Some(cookies) = cookie_file {
        args.push("--cookies".into());
        args.push(cookies.as_os_str().to_owned());
    }
    if subtitles {
        args.extend([
            "--write-subs".into(),
            "--write-auto-subs".into(),
            "--sub-format".into(),
            "srt/best".into(),
            "--embed-subs".into(),
        ]);
    }
    if embed_chapters {
        args.push("--embed-chapters".into());
    }
    args.extend([
        // Speed up fragmented/DASH streams and work around single-stream server throttling.
        "-N".into(),
        "8".into(),
        "--http-chunk-size".into(),
        "10M".into(),
        // Machine-readable progress, one line per update:
        // "LF <downloaded> <total> <estimate> <filename>", either total may be NA.
        "--progress-template".into(),
        "download:LF %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s %(progress.filename)s".into(),
        "-f".into(),
        selector.into(),
        "-o".into(),
        save_path.as_os_str().to_owned(),
        url.into(),
    ]);
    args
}

fn compute_smoothed_speed(previous: Option<u64>, instant: u64) -> u64 {
    match previous {
        Some(prev) if prev > 0 => ((prev as f64 * 0.6) + (instant as f64 * 0.4)).round() as u64,
        _ => instant,
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ParsedVideoProgress {
    done: u64,
    total: Option<u64>,
    filename: Option<String>,
}

fn is_auxiliary_media_file(filename: Option<&str>) -> bool {
    let Some(name) = filename else {
        return false;
    };
    let lower = name.trim().to_ascii_lowercase();
    let base = lower.strip_suffix(".part").unwrap_or(&lower);
    [
        ".vtt",
        ".srt",
        ".ass",
        ".ssa",
        ".ttml",
        ".lrc",
        ".srv1",
        ".srv2",
        ".srv3",
        ".json3",
        ".description",
        ".info.json",
        ".webp",
        ".jpg",
        ".jpeg",
        ".png",
        ".ytdl",
    ]
    .iter()
    .any(|ext| base.ends_with(ext))
}

fn parse_progress(line: &str) -> Option<ParsedVideoProgress> {
    let rest = line.strip_prefix("LF ")?;
    let mut parts = rest.splitn(4, ' ');
    let done = parts.next()?.parse().ok()?;
    let total_opt = parts.next().and_then(|text| text.parse().ok());
    let estimate_opt = parts.next().and_then(|text| text.parse().ok());
    let filename = parts
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "NA")
        .map(str::to_string);
    Some(ParsedVideoProgress {
        done,
        total: total_opt.or(estimate_opt),
        filename,
    })
}

fn stop_video(active: &mut ActiveDownload) {
    if let Some(mut run) = active.video.take() {
        // yt-dlp's ffmpeg child survives a plain kill and would finish the merge.
        tools::kill_tree(run.child.id());
        let _ = run.child.wait();
        remove_parts(&run.save_path);
    }
}

/// Drops the temp files yt-dlp leaves behind: `name.part`, `name.part-Frag<N>`,
/// `name.ytdl`, `name.f137.mp4.part-Frag<N>` and `name.temp.mp4`.
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

fn remove_parts(save_path: &Path) {
    let (Some(dir), Some(name)) = (
        save_path.parent(),
        save_path.file_name().and_then(|name| name.to_str()),
    ) else {
        return;
    };
    let stem = save_path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.to_string());
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().into_owned();
        let ours = file_name.starts_with(name) || file_name.starts_with(&format!("{stem}."));
        let temp = file_name.ends_with(".part")
            || file_name.contains(".part-Frag")
            || file_name.ends_with(".ytdl")
            || file_name.ends_with(".temp.mp4")
            || file_name.ends_with(".lajufile");
        if ours && temp {
            let _ = std::fs::remove_file(entry.path());
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
    fn progress_lines_carry_downloaded_bytes_and_total() {
        assert_eq!(
            parse_progress("LF 1234 5678 5678"),
            Some(ParsedVideoProgress {
                done: 1234,
                total: Some(5678),
                filename: None,
            })
        );
        assert_eq!(
            parse_progress("LF 99 NA 4321 C:/dl/clip with spaces.f137.mp4"),
            Some(ParsedVideoProgress {
                done: 99,
                total: Some(4321),
                filename: Some("C:/dl/clip with spaces.f137.mp4".to_string()),
            })
        );
        assert_eq!(
            parse_progress("LF 99 NA NA"),
            Some(ParsedVideoProgress {
                done: 99,
                total: None,
                filename: None,
            })
        );
        assert_eq!(parse_progress("[youtube] Extracting URL"), None);
        assert_eq!(parse_progress(""), None);
    }

    #[test]
    fn video_stream_tracker_skips_subtitles_and_accumulates_video_and_audio() {
        assert!(is_auxiliary_media_file(Some("clip.en.vtt")));
        assert!(is_auxiliary_media_file(Some("clip.id.srt.part")));
        assert!(!is_auxiliary_media_file(Some("clip.f137.mp4")));

        let mut tracker = VideoStreamTracker::from_seeded(Some(150_000_000));
        let (done1, sz1) = tracker.record(20_000_000, Some(135_000_000), Some("clip.f137.mp4"));
        assert_eq!(done1, 20_000_000);
        assert_eq!(sz1, Some(150_000_000));

        let (done2, _) = tracker.record(135_000_000, Some(135_000_000), Some("clip.f137.mp4"));
        assert_eq!(done2, 135_000_000);

        let (done3, sz3) = tracker.record(5_000_000, Some(18_000_000), Some("clip.f140.m4a"));
        assert_eq!(done3, 140_000_000);
        assert_eq!(sz3, Some(153_000_000));
    }

    #[test]
    fn video_args_select_quality_and_target() {
        let args = video_args(
            "bv*[height<=360]+ba",
            Path::new("C:/dl/clip.mp4"),
            "https://youtu.be/x",
            Path::new("C:/tools"),
            false,
            false,
            Some("node:/usr/bin/node"),
            Some(Path::new("C:/tmp/cookies.txt")),
        );
        let args: Vec<String> = args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();

        assert_eq!(
            args[args.iter().position(|a| a == "-f").unwrap() + 1],
            "bv*[height<=360]+ba"
        );
        assert_eq!(
            args[args.iter().position(|a| a == "-o").unwrap() + 1],
            "C:/dl/clip.mp4"
        );
        assert_eq!(
            args[args
                .iter()
                .position(|a| a == "--merge-output-format")
                .unwrap()
                + 1],
            "mp4"
        );
        assert_eq!(
            args[args.iter().position(|a| a == "-S").unwrap() + 1],
            "ext:mp4:m4a,vcodec:h264,acodec:aac"
        );
        assert_eq!(
            args[args.iter().position(|a| a == "--ppa").unwrap() + 1],
            "Merger+ffmpeg_o1:-c:a aac"
        );
        assert_eq!(
            args[args.iter().position(|a| a == "--js-runtimes").unwrap() + 1],
            "node:/usr/bin/node"
        );
        assert_eq!(
            args[args.iter().position(|a| a == "--cookies").unwrap() + 1],
            "C:/tmp/cookies.txt"
        );
        assert!(!args.iter().any(|arg| arg == "--embed-subs"));
        assert!(!args.iter().any(|arg| arg == "--embed-chapters"));
        assert!(args.iter().any(|arg| arg == "--http-chunk-size"));
        assert!(args.iter().any(|arg| arg == "--progress-template"));
        assert_eq!(args.last().unwrap(), "https://youtu.be/x");
    }

    #[test]
    fn smoothed_speed_blends_samples_without_wild_spikes() {
        assert_eq!(compute_smoothed_speed(None, 10_000_000), 10_000_000);
        assert_eq!(
            compute_smoothed_speed(Some(10_000_000), 20_000_000),
            14_000_000
        );
    }

    #[test]
    fn video_args_include_subtitles_and_chapters_when_enabled() {
        let args: Vec<String> = video_args(
            "best",
            Path::new("C:/dl/clip.mp4"),
            "https://youtu.be/x",
            Path::new("C:/tools"),
            true,
            true,
            None,
            None,
        )
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
        for flag in [
            "--write-subs",
            "--write-auto-subs",
            "--embed-subs",
            "--embed-chapters",
        ] {
            assert!(args.iter().any(|arg| arg == flag), "missing {flag}");
        }
    }

    #[test]
    fn video_file_name_uses_the_youtube_id() {
        assert_eq!(
            video_file_name("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            "dQw4w9WgXcQ.mp4"
        );
        assert_eq!(video_file_name("https://youtu.be/abc123"), "abc123.mp4");
        assert_eq!(video_file_name("not a url"), "video.mp4");
    }

    #[test]
    fn remove_parts_keeps_the_target_file() {
        let dir = tempfile::tempdir().unwrap();
        let save = dir.path().join("clip.mp4");
        std::fs::write(&save, b"x").unwrap();
        let leftovers = [
            "clip.mp4.part",
            "clip.mp4.f137.mp4.part",
            "clip.mp4.part-Frag12",
            "clip.mp4.ytdl",
            "clip.mp4.temp.mp4",
            "clip.mp4.lajufile",
        ];
        for name in leftovers {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        std::fs::write(dir.path().join("other.mp4.part"), b"x").unwrap();
        std::fs::write(dir.path().join("clip.mp4"), b"final").unwrap();

        remove_parts(&save);

        assert_eq!(std::fs::read(&save).unwrap(), b"final");
        assert!(dir.path().join("other.mp4.part").exists());
        for name in leftovers {
            assert!(!dir.path().join(name).exists(), "{name} should be removed");
        }
    }

    #[test]
    fn ssrf_guard_blocks_loopback_private_and_metadata_urls() {
        for bad in [
            "http://127.0.0.1:53841/rules",
            "http://localhost:8080/",
            "http://sub.localhost/test",
            "http://169.254.169.254/latest/meta-data/",
            "http://10.0.0.1/",
            "http://192.168.1.1/",
            "http://172.16.0.5/",
            "http://100.64.0.1/",
            "http://[::1]/",
            "http://[::ffff:127.0.0.1]/",
            "file:///C:/Windows/System32/drivers/etc/hosts",
        ] {
            let parsed = Url::parse(bad).unwrap();
            assert!(
                validate_public_page_url(&parsed).is_err(),
                "{bad} should be blocked"
            );
        }
        let ok = Url::parse("https://example.com/downloads").unwrap();
        assert!(validate_public_page_url(&ok).is_ok());
    }

    #[test]
    fn validate_http_url_rejects_non_http_schemes() {
        assert!(validate_http_url("https://example.com/a.zip").is_ok());
        assert!(validate_http_url("http://example.com/a.zip").is_ok());
        assert!(validate_http_url("file:///tmp/a.zip").is_err());
        assert!(validate_http_url("javascript:alert(1)").is_err());
    }

    #[test]
    fn temp_cookie_file_writes_netscape_cookies_and_cleans_up_on_drop() {
        assert!(TempCookieFile::from_netscape("SID=abc; HSID=xyz").is_none());
        assert!(TempCookieFile::from_netscape("# Netscape HTTP Cookie File\n").is_none());

        let netscape =
            "# Netscape HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\t1900000000\tSID\tvalue123\n";
        let path = {
            let file = TempCookieFile::from_netscape(netscape).expect("valid cookie file");
            let saved = std::fs::read_to_string(file.path()).unwrap();
            assert!(saved.contains("SID\tvalue123"));
            file.path().to_path_buf()
        };
        assert!(!path.exists(), "cookie file must be deleted on drop");
    }

    #[test]
    fn format_ytdlp_error_clarifies_bot_check_and_picks_error_line() {
        let bot = "WARNING: foo\nERROR: [youtube] xyz: Sign in to confirm you're not a bot.\nAlso see wiki";
        assert!(format_ytdlp_error(bot).contains("signed-in browser session"));

        let other = "WARNING: minor\nERROR: [youtube] xyz: Video unavailable";
        assert_eq!(
            format_ytdlp_error(other),
            "yt-dlp: ERROR: [youtube] xyz: Video unavailable"
        );
    }
}
