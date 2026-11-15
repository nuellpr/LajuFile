use std::collections::HashMap;
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
use lajufile_core::settings::{Settings, generate_token};
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
    VideoFormats {
        url: String,
        reply: tokio::sync::oneshot::Sender<Result<VideoListing, String>>,
    },
    UpdateTools,
    ToolsInstalled(Result<(), String>),
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
    VideoProgress {
        id: i64,
        generation: u64,
        done: u64,
        total: Option<u64>,
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
    commands: Sender<Command>,
    tools_installing: bool,
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

        state.poll_videos();

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
            Command::VideoFormats { url, reply } => self.video_formats(url, reply),
            Command::UpdateTools => self.update_tools(),
            Command::ToolsInstalled(result) => {
                self.tools_installing = false;
                self.message = Some(match result {
                    Ok(()) => "Tools ready".to_string(),
                    Err(error) => format!("Tools failed: {error}"),
                });
                self.pump_queue();
            }
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
            video_format: request.video_format.clone(),
        };
        if self.settings.ask_where_to_save {
            let key = self.next_capture_key;
            self.next_capture_key = self.next_capture_key.wrapping_add(1);
            self.pending_captures
                .insert(key, PendingCapture { request, reply });
            self.message = Some("Waiting for a save folder".to_string());
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
                    video_format: pending.request.video_format.clone(),
                };
                if add.video_format.is_some() {
                    self.finish_video_add(add, pending.reply);
                    return;
                }
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
        let mut save_path = dedupe_path(&dir, &base_name);
        let temp_path = loop {
            let candidate = temp_path_for(&save_path);
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(_) => break candidate,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    save_path = dedupe_path(&dir, &base_name);
                }
                Err(error) => return Err(InsertError::Failed(error.to_string())),
            }
        };
        let file_name = save_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or(base_name);
        let queue_order = match self.store.next_queue_order() {
            Ok(order) => order,
            Err(error) => return Err(InsertError::Failed(error.to_string())),
        };
        let new = NewDownload {
            url: request.url.clone(),
            final_url: Some(request.url),
            file_name: file_name.clone(),
            temp_path,
            save_path,
            size: None,
            status: DownloadStatus::Queued,
            category: Category::Video,
            kind: "video".to_string(),
            video_format: request.video_format,
            connections: 1,
            ranged: false,
            etag: None,
            last_modified: None,
            cookie_enc: None,
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
            Err(error) => Err(InsertError::Failed(error.to_string())),
        }
    }

    fn update_tools(&mut self) {
        if tools::tools_ready(&tools::tools_dir()) {
            self.message = Some("Tools ready".to_string());
            return;
        }
        self.ensure_tools();
    }

    /// Downloads yt-dlp + ffmpeg once; video jobs work after it lands.
    fn ensure_tools(&mut self) {
        if self.tools_installing {
            self.message = Some("Downloading tools...".to_string());
            return;
        }
        let dir = tools::tools_dir();
        if tools::tools_ready(&dir) {
            return;
        }
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
        reply: tokio::sync::oneshot::Sender<Result<VideoListing, String>>,
    ) {
        let dir = tools::tools_dir();
        if !tools::tools_ready(&dir) {
            self.ensure_tools();
            let _ = reply.send(Err(
                "the video tools are downloading, try again in a minute".to_string(),
            ));
            return;
        }
        thread::spawn(move || {
            let _ = reply.send(ytdlp_listing(&dir, &url));
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
            } => {
                if self.active.get(&id).map(|active| active.generation) != Some(generation) {
                    return;
                }
                if let Some(total) = total
                    && total > 0
                {
                    let _ = self.store.set_size(id, total);
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
        let selector = download.video_format.as_deref().unwrap_or(DEFAULT_SELECTOR);
        let mut command = std::process::Command::new(tools::tool_path(&dir, tools::YTDLP));
        command
            .args(video_args(
                selector,
                &download.save_path,
                &download.url,
                &dir,
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
        if let Some(stdout) = child.stdout.take() {
            let events = self.events.clone();
            thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if let Some((done, total)) = parse_progress(&line) {
                        let _ = events.send(Event::VideoProgress {
                            id,
                            generation,
                            done,
                            total,
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
                    let line = line.trim().to_string();
                    if line.is_empty() {
                        continue;
                    }
                    if let Ok(mut slot) = slot.lock() {
                        *slot = line;
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
                }),
            },
        );
        true
    }

    fn poll_videos(&mut self) {
        let exited: Vec<(i64, bool)> = self
            .active
            .iter_mut()
            .filter_map(|(id, active)| {
                let status = active.video.as_mut()?.child.try_wait().ok()??;
                Some((*id, status.success()))
            })
            .collect();
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
                let _ = self.store.mark_done(id, now_millis());
            } else {
                let message = run
                    .last_error
                    .lock()
                    .map(|line| line.clone())
                    .unwrap_or_default();
                let message = if message.is_empty() {
                    "yt-dlp failed".to_string()
                } else {
                    message
                };
                let _ = self
                    .store
                    .update_status(id, DownloadStatus::Error, Some(&message));
                let _ = std::fs::remove_file(&run.save_path);
                remove_parts(&run.save_path);
            }
            self.speeds.remove(&id);
            self.progress.remove(&id);
            self.last_persist.remove(&id);
            self.clear_pending(id);
            self.pump_queue();
        }
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
        if let Some(mut active) = self.active.remove(&id) {
            stop_video(&mut active);
            if let Some(handle) = active.handle.take() {
                handle.abort();
                let _ = self.runtime.block_on(handle);
            }
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

/// `yt-dlp -J`, killed if it stalls: a hung run must not outlive the request.
fn ytdlp_listing(dir: &Path, url: &str) -> Result<VideoListing, String> {
    let mut command = std::process::Command::new(tools::tool_path(dir, tools::YTDLP));
    command
        .args(["--no-playlist", "-J", "--", url])
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
        let error = String::from_utf8_lossy(&stderr.join().unwrap_or_default())
            .trim()
            .to_string();
        return Err(format!("yt-dlp: {error}"));
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

/// yt-dlp arguments for one video job: merge to mp4 next to the target path.
fn video_args(selector: &str, save_path: &Path, url: &str, tools_dir: &Path) -> Vec<OsString> {
    vec![
        "--no-playlist".into(),
        "--newline".into(),
        "--merge-output-format".into(),
        "mp4".into(),
        "--ffmpeg-location".into(),
        tools_dir.as_os_str().to_owned(),
        // Machine-readable progress, one line per update:
        // "LF <downloaded> <total> <estimate>", either total may be NA.
        "--progress-template".into(),
        "download:LF %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s".into(),
        "-f".into(),
        selector.into(),
        "-o".into(),
        save_path.as_os_str().to_owned(),
        url.into(),
    ]
}

fn parse_progress(line: &str) -> Option<(u64, Option<u64>)> {
    let mut fields = line.strip_prefix("LF ")?.split_whitespace();
    let done = fields.next()?.parse().ok()?;
    let total = fields
        .next()
        .and_then(|text| text.parse().ok())
        .or_else(|| fields.next().and_then(|text| text.parse().ok()));
    Some((done, total))
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
            Some((1234, Some(5678)))
        );
        assert_eq!(parse_progress("LF 99 NA 4321"), Some((99, Some(4321))));
        assert_eq!(parse_progress("LF 99 NA NA"), Some((99, None)));
        assert_eq!(parse_progress("[youtube] Extracting URL"), None);
        assert_eq!(parse_progress(""), None);
    }

    #[test]
    fn video_args_select_quality_and_target() {
        let args = video_args(
            "bv*[height<=360]+ba",
            Path::new("C:/dl/clip.mp4"),
            "https://youtu.be/x",
            Path::new("C:/tools"),
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
        assert!(args.iter().any(|arg| arg == "--progress-template"));
        assert_eq!(args.last().unwrap(), "https://youtu.be/x");
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
}
