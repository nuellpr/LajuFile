use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use lajufile_core::checksum::sha256_file;
use lajufile_core::paths;
use lajufile_core::settings::{Settings, Theme, generate_token};
use lajufile_core::store::{Category, DownloadStatus};
use lajufile_core::video::VideoListing;

use crate::bridge;
use crate::engine::{AddRequest, Command, EngineConfig, EngineHandle, Snapshot};
use crate::tray::{self, Tray, TrayEvent};
use crate::ui::dialogs::{
    AddDraft, BatchDraft, BatchMode, PropertiesDraft, UpdateUiState,
    clipboard_download_url_with_rules,
};
use crate::ui::download_info::{DownloadInfoChoice, DownloadInfoDraft, category_label};
use crate::ui::folder_picker::{FolderChoice, FolderPicker};
use crate::ui::format::format_speed;
use crate::ui::theme::apply_theme;
use crate::ui::{self, Action, Filter, SortOrder, extract_urls_from_text};
use crate::updater;

#[derive(Debug, Clone)]
pub(crate) enum FolderTarget {
    Category(String),
    MoveDownload,
}

pub struct LajuFileApp {
    pub(crate) engine: Option<EngineHandle>,
    pub(crate) snapshot: Option<Arc<Snapshot>>,
    pub(crate) snapshot_version: u64,
    pub(crate) selected: Option<i64>,
    pub(crate) selected_ids: HashSet<i64>,
    pub(crate) filter: Filter,
    pub(crate) search_query: String,
    pub(crate) search_focus: bool,
    pub(crate) sort_order: SortOrder,
    pub(crate) show_speed_popover: bool,
    pub(crate) custom_speed_kbps: String,
    pub(crate) delete_confirm: Option<(Vec<i64>, bool)>,
    pub(crate) properties_id: Option<i64>,
    pub(crate) properties_draft: Option<PropertiesDraft>,
    pub(crate) checksum_rx: Option<(i64, mpsc::Receiver<Result<String, String>>)>,
    pub(crate) checksum_result: Option<(i64, Result<String, String>)>,
    pub(crate) show_add: bool,
    pub(crate) show_batch: bool,
    pub(crate) show_settings: bool,
    pub(crate) show_browser_wizard: bool,
    pub(crate) show_language_picker: bool,
    pub(crate) show_update_dialog: bool,
    pub(crate) update_state: UpdateUiState,
    pub(crate) update_rx: Option<mpsc::Receiver<updater::UpdateEvent>>,
    pub(crate) language: String,
    pub(crate) add_draft: AddDraft,
    pub(crate) batch_draft: BatchDraft,
    pub(crate) add_focus: bool,
    pub(crate) settings_draft: Settings,
    pub(crate) toast: Option<(String, Instant)>,
    pub(crate) last_message: Option<String>,
    pub(crate) theme: Theme,
    pub(crate) ui_scale_pct: u32,
    pub(crate) tray: Option<Tray>,
    pub(crate) allow_exit: bool,
    pub(crate) tray_tooltip: String,
    pub(crate) bridge_test: Option<mpsc::Receiver<Result<(), String>>>,
    pub(crate) formats: Option<tokio::sync::oneshot::Receiver<Result<VideoListing, String>>>,
    pub(crate) page_links: Option<tokio::sync::oneshot::Receiver<Result<Vec<String>, String>>>,
    pub(crate) folder_picker: Option<FolderPicker>,
    pub(crate) folder_picker_target: Option<FolderTarget>,
    pub(crate) pending_add: Option<AddRequest>,
    pub(crate) capture_info: Option<(u64, DownloadInfoDraft)>,
    pub(crate) dismissed_capture: Option<u64>,
    pub(crate) progress_id: Option<i64>,
    pub(crate) hidden_progress_id: Option<i64>,
    pub(crate) progress_details: bool,
    pub(crate) speed_history: HashMap<i64, Vec<u64>>,
    pub(crate) last_speed_sample: Instant,
    pub(crate) completed_id: Option<i64>,
    pub(crate) seen_done: Option<HashSet<i64>>,
    pub(crate) seen_downloading: Option<HashSet<i64>>,
    pub(crate) last_clipboard: Option<String>,
    pub(crate) clipboard_initialized: bool,
    pub(crate) last_clipboard_poll: Instant,
    pub(crate) last_wake_seq: u64,
    pub(crate) sleep_guard: crate::platform::SleepGuard,
}

impl LajuFileApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::i18n::install_system_fonts(&cc.egui_ctx);
        let data_dir = paths::data_dir();
        let settings = Settings::load_or_create(&data_dir).unwrap_or_default();
        if settings.autostart {
            sync_autostart(true);
        }
        let theme = settings.theme.clone();
        let ui_scale_pct = settings.ui_scale_pct.clamp(80, 150);
        let language = settings.language.clone();
        crate::i18n::set_ctx_lang(&cc.egui_ctx, &language);
        apply_theme(&cc.egui_ctx, &theme);
        apply_scale(&cc.egui_ctx, ui_scale_pct);

        let icon =
            eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icons/icon-32.png"))
                .ok();
        let tray = icon
            .as_ref()
            .and_then(|icon| tray::create(icon, &cc.egui_ctx));

        let add_draft = AddDraft::for_settings(&settings);
        let batch_draft = BatchDraft::for_settings(&settings);
        let custom_speed_kbps = settings.global_speed_limit_kbps.to_string();
        let settings_draft = settings.clone();
        let engine = EngineHandle::start(EngineConfig {
            data_dir: data_dir.clone(),
            db_path: paths::db_path(&data_dir),
            settings,
        })
        .ok();
        if let Some(engine) = &engine {
            let wake_ctx = cc.egui_ctx.clone();
            engine.set_wake_callback(Arc::new(move || {
                wake_ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                wake_ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                wake_ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                wake_ctx.request_repaint();
            }));
        }
        let toast = engine
            .is_none()
            .then(|| ("Engine failed to start".to_string(), Instant::now()));

        Self {
            engine,
            snapshot: None,
            snapshot_version: 0,
            selected: None,
            selected_ids: HashSet::new(),
            filter: Filter::All,
            search_query: String::new(),
            search_focus: false,
            sort_order: SortOrder::Newest,
            show_speed_popover: false,
            custom_speed_kbps,
            delete_confirm: None,
            properties_id: None,
            properties_draft: None,
            checksum_rx: None,
            checksum_result: None,
            show_add: false,
            show_batch: false,
            show_settings: false,
            show_browser_wizard: false,
            show_language_picker: false,
            show_update_dialog: false,
            update_state: UpdateUiState::Idle,
            update_rx: None,
            language,
            add_draft,
            batch_draft,
            add_focus: false,
            settings_draft,
            toast,
            last_message: None,
            theme,
            ui_scale_pct,
            tray,
            allow_exit: false,
            tray_tooltip: String::new(),
            bridge_test: None,
            formats: None,
            page_links: None,
            folder_picker: None,
            folder_picker_target: None,
            pending_add: None,
            capture_info: None,
            dismissed_capture: None,
            progress_id: None,
            hidden_progress_id: None,
            progress_details: true,
            speed_history: HashMap::new(),
            last_speed_sample: Instant::now(),
            completed_id: None,
            seen_done: None,
            seen_downloading: None,
            last_clipboard: None,
            clipboard_initialized: false,
            last_clipboard_poll: Instant::now(),
            last_wake_seq: 0,
            sleep_guard: crate::platform::SleepGuard::new(),
        }
    }

    fn fresh_add_draft(&self) -> AddDraft {
        self.snapshot
            .as_ref()
            .map(|snapshot| AddDraft::for_settings(&snapshot.settings))
            .unwrap_or_default()
    }

    fn fresh_batch_draft(&self) -> BatchDraft {
        self.snapshot
            .as_ref()
            .map(|snapshot| BatchDraft::for_settings(&snapshot.settings))
            .unwrap_or_default()
    }

    fn poll_engine(&mut self, ctx: &egui::Context) {
        let Some(engine) = &self.engine else {
            return;
        };
        let Some(snapshot) = engine.latest_snapshot_if_changed(&mut self.snapshot_version) else {
            return;
        };

        if snapshot.wake_seq > self.last_wake_seq {
            self.last_wake_seq = snapshot.wake_seq;
            self.show_window(ctx);
        }
        let any_downloading = snapshot
            .downloads
            .iter()
            .any(|download| download.status == DownloadStatus::Downloading);
        self.sleep_guard.sync(any_downloading);

        if let Some(message) = snapshot.message.as_ref()
            && self.last_message.as_deref() != Some(message.as_str())
        {
            self.toast = Some((message.clone(), Instant::now()));
            self.last_message = Some(message.clone());
        }

        if snapshot.settings.theme != self.theme {
            self.theme = snapshot.settings.theme.clone();
            apply_theme(ctx, &self.theme);
        }
        let next_scale = snapshot.settings.ui_scale_pct.clamp(80, 150);
        if next_scale != self.ui_scale_pct {
            self.ui_scale_pct = next_scale;
            apply_scale(ctx, next_scale);
        }
        if snapshot.settings.language != self.language {
            self.language = snapshot.settings.language.clone();
            crate::i18n::set_ctx_lang(ctx, &self.language);
        }
        let done_now: HashSet<i64> = snapshot
            .downloads
            .iter()
            .filter(|download| download.status == DownloadStatus::Done)
            .map(|download| download.id)
            .collect();
        if let Some(seen) = &mut self.seen_done {
            for id in &done_now {
                if !seen.contains(id) {
                    if self.progress_id == Some(*id) {
                        self.progress_id = None;
                    }
                    if self.hidden_progress_id == Some(*id) {
                        self.hidden_progress_id = None;
                    }
                    if snapshot.settings.show_complete_dialog {
                        self.completed_id = Some(*id);
                    }
                }
            }
            *seen = done_now;
        } else {
            self.seen_done = Some(done_now);
        }
        let downloading_now: HashSet<i64> = snapshot
            .downloads
            .iter()
            .filter(|download| download.status == DownloadStatus::Downloading)
            .map(|download| download.id)
            .collect();
        if let Some(seen) = &mut self.seen_downloading {
            if snapshot.settings.show_progress_on_start
                && self.progress_id.is_none()
                && self.capture_info.is_none()
                && let Some(&started_id) = downloading_now.iter().find(|id| !seen.contains(id))
            {
                self.progress_id = Some(started_id);
            }
            *seen = downloading_now;
        } else {
            self.seen_downloading = Some(downloading_now);
        }
        if self.last_speed_sample.elapsed() >= Duration::from_millis(500) {
            self.last_speed_sample = Instant::now();
            for download in &snapshot.downloads {
                if download.status == DownloadStatus::Downloading {
                    let entry = self.speed_history.entry(download.id).or_default();
                    if entry.len() >= 60 {
                        entry.remove(0);
                    }
                    entry.push(snapshot.speeds.get(&download.id).copied().unwrap_or(0));
                }
            }
            self.speed_history.retain(|id, _| {
                self.progress_id == Some(*id)
                    || snapshot.downloads.iter().any(|d| {
                        d.id == *id
                            && matches!(
                                d.status,
                                DownloadStatus::Downloading
                                    | DownloadStatus::Paused
                                    | DownloadStatus::Queued
                            )
                    })
            });
        }

        self.settings_draft.browser_paired |= snapshot.settings.browser_paired;
        let pending = snapshot.pending_capture.clone();
        self.snapshot = Some(snapshot);
        match pending {
            Some(prompt) if self.dismissed_capture != Some(prompt.key) => {
                if let Some((active_key, draft)) = &mut self.capture_info
                    && *active_key == prompt.key
                {
                    if draft.size.is_none() && prompt.size.is_some() {
                        draft.size = prompt.size;
                    }
                    if draft.ranged.is_none() && prompt.ranged.is_some() {
                        draft.ranged = prompt.ranged;
                    }
                } else if self.capture_info.is_none() {
                    let category = Category::from_file_name(&prompt.file_name);
                    let folder = self.default_folder_for(category);
                    self.show_window(ctx);
                    self.capture_info = Some((
                        prompt.key,
                        crate::ui::download_info::DownloadInfoDraft {
                            url: prompt.url,
                            file_name: prompt.file_name,
                            size: prompt.size,
                            ranged: prompt.ranged,
                            category,
                            folder: folder.display().to_string(),
                            remember: false,
                        },
                    ));
                }
            }
            Some(_) => {}
            None => {
                if self.capture_info.is_none() {
                    self.dismissed_capture = None;
                }
            }
        }
    }

    fn poll_bridge_test(&mut self) {
        let received = self.bridge_test.as_ref().map(|rx| rx.try_recv());
        match received {
            Some(Ok(Ok(()))) => {
                self.toast = Some(("Bridge connected".to_string(), Instant::now()));
                self.bridge_test = None;
            }
            Some(Ok(Err(error))) => {
                self.toast = Some((format!("Bridge check failed: {error}"), Instant::now()));
                self.bridge_test = None;
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => self.bridge_test = None,
            _ => {}
        }
    }

    fn poll_formats(&mut self) {
        let received = self.formats.as_mut().map(|rx| rx.try_recv());
        match received {
            Some(Ok(Ok(listing))) => {
                if self.add_draft.file_name.trim().is_empty() {
                    self.add_draft.file_name = format!("{}.mp4", listing.title);
                }
                self.add_draft.formats = listing.choices;
                self.formats = None;
            }
            Some(Ok(Err(error))) => {
                self.toast = Some((format!("Qualities failed: {error}"), Instant::now()));
                self.formats = None;
            }
            Some(Err(tokio::sync::oneshot::error::TryRecvError::Closed)) => self.formats = None,
            _ => {}
        }
    }

    fn poll_page_links(&mut self) {
        let received = self.page_links.as_mut().map(|rx| rx.try_recv());
        match received {
            Some(Ok(Ok(links))) => {
                self.page_links = None;
                if links.is_empty() {
                    self.toast = Some((
                        "No downloadable links found on that page".to_string(),
                        Instant::now(),
                    ));
                } else {
                    let added = links.join("\n");
                    if self.batch_draft.urls_text.trim().is_empty() {
                        self.batch_draft.urls_text = added;
                    } else {
                        self.batch_draft.urls_text.push('\n');
                        self.batch_draft.urls_text.push_str(&added);
                    }
                    self.toast = Some((
                        format!("Extracted {} links from web page", links.len()),
                        Instant::now(),
                    ));
                }
            }
            Some(Ok(Err(error))) => {
                self.page_links = None;
                self.toast = Some((error, Instant::now()));
            }
            Some(Err(tokio::sync::oneshot::error::TryRecvError::Closed)) => {
                self.page_links = None;
            }
            _ => {}
        }
    }

    fn poll_tray(&mut self, ctx: &egui::Context) {
        let events = self.tray.as_ref().map(Tray::poll).unwrap_or_default();
        for event in events {
            match event {
                TrayEvent::Show => self.show_window(ctx),
                TrayEvent::Add => {
                    self.show_window(ctx);
                    self.add_draft = self.fresh_add_draft();
                    self.add_focus = true;
                    self.show_add = true;
                }
                TrayEvent::PauseAll => self.send(Command::PauseAll),
                TrayEvent::ResumeAll => self.send(Command::ResumeAll),
                TrayEvent::Exit => {
                    self.allow_exit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    fn show_window(&self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        crate::platform::focus_window_by_title("LajuFile");
    }

    fn update_tray_tooltip(&mut self) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let active = snapshot
            .downloads
            .iter()
            .filter(|download| {
                matches!(
                    download.status,
                    DownloadStatus::Queued | DownloadStatus::Downloading
                )
            })
            .count();
        let tooltip = format!("LajuFile — {active} active");
        if tooltip != self.tray_tooltip {
            if let Some(tray) = &self.tray {
                tray.set_tooltip(&tooltip);
            }
            self.tray_tooltip = tooltip;
        }
    }

    fn default_save_dir(&self) -> PathBuf {
        if !self.add_draft.folder.trim().is_empty() {
            return PathBuf::from(self.add_draft.folder.trim());
        }
        self.snapshot
            .as_ref()
            .map(|snapshot| snapshot.settings.download_dir.clone())
            .unwrap_or_else(paths::default_download_dir)
    }

    fn default_folder_for(&self, category: Category) -> PathBuf {
        self.snapshot
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .settings
                    .category_dirs
                    .get(crate::ui::download_info::category_label(category))
                    .cloned()
                    .unwrap_or_else(|| snapshot.settings.download_dir.clone())
            })
            .unwrap_or_else(paths::default_download_dir)
    }

    fn open_folder_picker(&mut self) {
        self.pending_add = None;
        self.folder_picker_target = None;
        let start_dir = if self.show_settings
            && !self.show_add
            && !self.settings_draft.download_dir.as_os_str().is_empty()
        {
            self.settings_draft.download_dir.clone()
        } else {
            self.default_save_dir()
        };
        self.folder_picker = Some(FolderPicker::new(start_dir));
    }

    fn open_properties(&mut self, id: i64) {
        self.properties_id = Some(id);
        let download = self
            .snapshot
            .as_ref()
            .and_then(|s| s.downloads.iter().find(|d| d.id == id));
        self.properties_draft = download.map(PropertiesDraft::for_download);
        if self
            .checksum_result
            .as_ref()
            .is_some_and(|(cid, _)| *cid != id)
        {
            self.checksum_result = None;
        }
        if self.checksum_rx.as_ref().is_some_and(|(cid, _)| *cid != id) {
            self.checksum_rx = None;
        }
    }

    fn start_update_check(&mut self) {
        self.update_state = UpdateUiState::Checking;
        let (tx, rx) = mpsc::channel();
        self.update_rx = Some(rx);
        updater::spawn_check(tx);
    }

    fn poll_updater(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.update_rx else {
            return;
        };
        let mut clear_rx = false;
        while let Ok(event) = rx.try_recv() {
            match event {
                updater::UpdateEvent::Checked(Ok(check)) => {
                    self.update_state = if check.has_update {
                        UpdateUiState::Available(check.latest)
                    } else {
                        UpdateUiState::UpToDate(check)
                    };
                    clear_rx = true;
                }
                updater::UpdateEvent::Checked(Err(err)) => {
                    self.update_state = UpdateUiState::Failed(err);
                    clear_rx = true;
                }
                updater::UpdateEvent::DownloadProgress { downloaded, total } => {
                    if let UpdateUiState::Downloading {
                        downloaded: slot_dl,
                        total: slot_total,
                        ..
                    } = &mut self.update_state
                    {
                        *slot_dl = downloaded;
                        if total.is_some() {
                            *slot_total = total;
                        }
                    }
                    ctx.request_repaint_after(Duration::from_millis(60));
                }
                updater::UpdateEvent::ReadyToApply(Ok(path)) => {
                    clear_rx = true;
                    match updater::launch_installer_and_update(&path) {
                        Ok(true) => {
                            self.allow_exit = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        Ok(false) => {
                            self.show_update_dialog = false;
                            self.toast = Some((
                                "Update installer opened — follow the installer window."
                                    .to_string(),
                                Instant::now(),
                            ));
                        }
                        Err(err) => {
                            self.update_state = UpdateUiState::Failed(err);
                        }
                    }
                }
                updater::UpdateEvent::ReadyToApply(Err(err)) => {
                    self.update_state = UpdateUiState::Failed(err);
                    clear_rx = true;
                }
            }
        }
        if clear_rx {
            self.update_rx = None;
        }
    }

    fn poll_checksum(&mut self) {
        let Some((id, rx)) = &self.checksum_rx else {
            return;
        };
        let id = *id;
        match rx.try_recv() {
            Ok(result) => {
                self.checksum_result = Some((id, result));
                self.checksum_rx = None;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.checksum_result = Some((id, Err("Worker disconnected".to_string())));
                self.checksum_rx = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }

    fn handle_add(&mut self, request: AddRequest) {
        let prompt = self
            .snapshot
            .as_ref()
            .map(|snapshot| should_prompt_for_folder(&snapshot.settings, true))
            .unwrap_or(false);
        if prompt && request.save_dir.is_none() {
            self.pending_add = Some(request);
            self.folder_picker = Some(FolderPicker::new(self.default_save_dir()));
            return;
        }
        self.send(Command::Add(request));
    }

    fn poll_folder_picker(&mut self, ctx: &egui::Context) {
        let Some(mut picker) = self.folder_picker.take() else {
            return;
        };
        let dark = matches!(self.theme, Theme::Dark);
        match picker.show(ctx, dark) {
            Some(FolderChoice::Select(path)) => match self.folder_picker_target.take() {
                Some(FolderTarget::Category(cat)) => {
                    self.settings_draft.category_dirs.insert(cat, path);
                }
                Some(FolderTarget::MoveDownload) => {
                    if let Some(draft) = &mut self.properties_draft {
                        draft.folder = path.display().to_string();
                    }
                }
                None => {
                    if let Some((_, draft)) = &mut self.capture_info {
                        draft.folder = path.display().to_string();
                    } else if self.show_batch {
                        self.batch_draft.folder = path.display().to_string();
                    } else if self.show_settings && !self.show_add && self.pending_add.is_none() {
                        self.settings_draft.download_dir = path;
                    } else {
                        self.add_draft.folder = path.display().to_string();
                        if let Some(mut request) = self.pending_add.take() {
                            request.save_dir = Some(path);
                            self.send(Command::Add(request));
                            self.show_add = false;
                        }
                    }
                }
            },
            Some(FolderChoice::Cancel) => {
                self.pending_add = None;
                self.folder_picker_target = None;
            }
            None => self.folder_picker = Some(picker),
        }
    }

    fn poll_clipboard(&mut self, ctx: &egui::Context) {
        let enabled = self
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.settings.clipboard_monitor)
            .unwrap_or(true);
        if !enabled {
            self.clipboard_initialized = false;
            return;
        }
        if self.last_clipboard_poll.elapsed() < Duration::from_millis(800) {
            return;
        }
        self.last_clipboard_poll = Instant::now();
        let current = arboard::Clipboard::new()
            .ok()
            .and_then(|mut cb| cb.get_text().ok());
        if !self.clipboard_initialized {
            self.clipboard_initialized = true;
            self.last_clipboard = current;
            return;
        }
        if current == self.last_clipboard {
            return;
        }
        self.last_clipboard = current.clone();
        if self.show_add || self.show_batch || self.capture_info.is_some() {
            return;
        }
        let Some(text) = current else {
            return;
        };
        let url = match self.snapshot.as_ref().map(|snapshot| &snapshot.settings) {
            Some(settings) => clipboard_download_url_with_rules(&text, settings),
            None => clipboard_download_url_with_rules(&text, &Settings::default()),
        };
        let Some(url) = url else {
            return;
        };
        let already_active = self.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.downloads.iter().any(|download| {
                download.url == url
                    && matches!(
                        download.status,
                        DownloadStatus::Queued
                            | DownloadStatus::Downloading
                            | DownloadStatus::Paused
                    )
            })
        });
        if already_active {
            return;
        }
        self.show_window(ctx);
        self.add_draft = self.fresh_add_draft();
        self.add_draft.url = url;
        self.add_focus = true;
        self.show_add = true;
    }

    fn poll_capture_info(&mut self, ctx: &egui::Context) {
        let Some((key, mut draft)) = self.capture_info.take() else {
            return;
        };
        let dark = matches!(
            self.snapshot
                .as_ref()
                .map(|s| &s.settings.theme)
                .unwrap_or(&self.theme),
            Theme::Dark
        );
        match crate::ui::download_info::download_info_dialog(ctx, &mut draft, dark) {
            Some(DownloadInfoChoice::Browse) => {
                self.folder_picker = Some(FolderPicker::new(PathBuf::from(draft.folder.trim())));
                self.capture_info = Some((key, draft));
            }
            Some(DownloadInfoChoice::Start {
                folder,
                file_name,
                remember,
                start_now,
            }) => {
                self.dismissed_capture = Some(key);
                self.send(Command::CaptureDecision {
                    key,
                    folder: folder.clone(),
                    file_name: Some(file_name),
                    start_now,
                });
                if remember && let Some(snapshot) = &self.snapshot {
                    let mut settings = snapshot.settings.clone();
                    settings
                        .category_dirs
                        .insert(category_label(draft.category).to_string(), folder);
                    self.send(Command::UpdateSettings(settings));
                }
            }
            Some(DownloadInfoChoice::Cancel) => {
                self.dismissed_capture = Some(key);
                self.send(Command::CaptureFolderDecision { key, path: None });
            }
            None => self.capture_info = Some((key, draft)),
        }
    }

    fn handle_actions(&mut self, ctx: &egui::Context, actions: Vec<Action>) {
        for action in actions {
            match action {
                Action::Select(id) => {
                    self.selected = Some(id);
                    self.selected_ids.clear();
                    self.selected_ids.insert(id);
                }
                Action::SelectRow { id, ctrl, shift } => {
                    let visible_ids: Vec<i64> = self
                        .snapshot
                        .as_ref()
                        .map(|snapshot| {
                            ui::main_window::visible_downloads(self, &snapshot.downloads)
                                .into_iter()
                                .map(|d| d.id)
                                .collect()
                        })
                        .unwrap_or_default();
                    apply_row_click(
                        &visible_ids,
                        &mut self.selected,
                        &mut self.selected_ids,
                        id,
                        ctrl,
                        shift,
                    );
                }
                Action::SelectAll => {
                    let visible_ids: Vec<i64> = self
                        .snapshot
                        .as_ref()
                        .map(|snapshot| {
                            ui::main_window::visible_downloads(self, &snapshot.downloads)
                                .into_iter()
                                .map(|d| d.id)
                                .collect()
                        })
                        .unwrap_or_default();
                    self.selected_ids = visible_ids.iter().copied().collect();
                    if self.selected.is_none()
                        || !self
                            .selected
                            .is_some_and(|id| self.selected_ids.contains(&id))
                    {
                        self.selected = visible_ids.first().copied();
                    }
                }
                Action::Pause(id) => self.send(Command::Pause(id)),
                Action::Resume(id) => self.send(Command::Resume(id)),
                Action::UpdateUrlAndResume { id, url } => {
                    self.send(Command::UpdateUrlAndResume { id, url });
                }
                Action::MoveRename { id, file_name, dir } => {
                    self.send(Command::MoveRename { id, file_name, dir });
                    self.properties_id = None;
                }
                Action::BrowseMoveDir(id) => {
                    let start = self
                        .path_for(id)
                        .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
                        .unwrap_or_else(|| self.default_save_dir());
                    self.folder_picker_target = Some(FolderTarget::MoveDownload);
                    self.folder_picker = Some(FolderPicker::new(start));
                }
                Action::BrowseCategoryDir(cat) => {
                    let start = self
                        .settings_draft
                        .category_dirs
                        .get(&cat)
                        .cloned()
                        .unwrap_or_else(|| self.settings_draft.download_dir.clone());
                    self.folder_picker_target = Some(FolderTarget::Category(cat));
                    self.folder_picker = Some(FolderPicker::new(start));
                }
                Action::VerifyChecksum(id) => {
                    if let Some(path) = self.path_for(id) {
                        let (tx, rx) = mpsc::channel();
                        std::thread::spawn(move || {
                            let res = sha256_file(&path).map_err(|e| e.to_string());
                            let _ = tx.send(res);
                        });
                        self.checksum_rx = Some((id, rx));
                        self.checksum_result = None;
                    }
                }
                Action::CopyChecksum(hash) => {
                    self.last_clipboard = Some(hash.clone());
                    ctx.copy_text(hash);
                    self.toast = Some(("SHA-256 copied".to_string(), Instant::now()));
                }
                Action::Remove(id) => self.remove_download(id, false),
                Action::ConfirmRemove(ids, delete_file) => {
                    if !ids.is_empty() {
                        self.delete_confirm = Some((ids, delete_file));
                    }
                }
                Action::RemoveWithFile { ids, delete_file } => {
                    for id in ids {
                        self.remove_download(id, delete_file);
                    }
                }
                Action::RemoveCompleted => {
                    self.send(Command::RemoveCompleted);
                    self.completed_id = None;
                }
                Action::ShowProgress(id) => {
                    self.progress_id = Some(id);
                    if self.hidden_progress_id == Some(id) {
                        self.hidden_progress_id = None;
                    }
                }
                Action::HideProgress(id) => {
                    if self.progress_id == Some(id) {
                        self.progress_id = None;
                    }
                    self.hidden_progress_id = Some(id);
                }
                Action::OpenUpdateChecker => {
                    self.show_update_dialog = true;
                    if matches!(
                        self.update_state,
                        UpdateUiState::Idle | UpdateUiState::UpToDate(_) | UpdateUiState::Failed(_)
                    ) {
                        self.start_update_check();
                    }
                }
                Action::CheckUpdatesNow => {
                    self.start_update_check();
                }
                Action::StartSelfUpdate { url, file_name } => {
                    if let UpdateUiState::Available(info) = &self.update_state {
                        let checksums_url = info.checksums_url.clone();
                        self.update_state = UpdateUiState::Downloading {
                            info: info.clone(),
                            downloaded: 0,
                            total: info.asset_size,
                        };
                        let (tx, rx) = mpsc::channel();
                        self.update_rx = Some(rx);
                        updater::spawn_download(url, file_name, checksums_url, tx);
                    }
                }
                Action::OpenProperties(id) => {
                    self.open_properties(id);
                }
                Action::Redownload(id) => {
                    if let Some(download) = self
                        .snapshot
                        .as_ref()
                        .and_then(|s| s.downloads.iter().find(|d| d.id == id))
                        .cloned()
                    {
                        self.send(Command::Add(AddRequest {
                            url: download.url,
                            file_name: Some(download.file_name),
                            save_dir: download.save_path.parent().map(|p| p.to_path_buf()),
                            start_now: true,
                            connections: Some(download.connections.max(1)),
                            referrer: download.referrer,
                            user_agent: download.user_agent,
                            cookie: None,
                            video_format: download.video_format,
                            video_size: download.size,
                        }));
                        self.toast = Some(("Redownload queued".to_string(), Instant::now()));
                    }
                }
                Action::PauseAll => self.send(Command::PauseAll),
                Action::ResumeAll => self.send(Command::ResumeAll),
                Action::Exit => {
                    self.allow_exit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Action::Open(id) => {
                    if let Some(path) = self.path_for(id) {
                        open_file(&path);
                    }
                }
                Action::OpenWith(id) => {
                    if let Some(path) = self.path_for(id) {
                        open_with(&path);
                    }
                }
                Action::OpenFolder(id) => {
                    if let Some(path) = self.path_for(id) {
                        open_folder(&path);
                    }
                }
                Action::CopyUrl(url) => {
                    self.last_clipboard = Some(url.clone());
                    ctx.copy_text(url);
                    self.toast = Some(("URL copied".to_string(), Instant::now()));
                }
                Action::CopyPath(path) => {
                    let text = path.display().to_string();
                    self.last_clipboard = Some(text.clone());
                    ctx.copy_text(text);
                    self.toast = Some(("File path copied".to_string(), Instant::now()));
                }
                Action::FocusSearch => {
                    self.search_focus = true;
                }
                Action::ExportLinks => {
                    self.export_links(ctx);
                }
                Action::OpenAdd => {
                    self.add_draft = self.fresh_add_draft();
                    self.add_focus = true;
                    self.show_add = true;
                }
                Action::OpenBatch => {
                    self.batch_draft = self.fresh_batch_draft();
                    self.show_batch = true;
                }
                Action::OpenSettings => {
                    if let Some(snapshot) = &self.snapshot {
                        self.settings_draft = snapshot.settings.clone();
                    }
                    self.show_settings = true;
                }
                Action::OpenBrowserWizard => {
                    if !self.show_settings
                        && let Some(snapshot) = &self.snapshot
                    {
                        self.settings_draft = snapshot.settings.clone();
                    }
                    self.show_settings = false;
                    self.show_browser_wizard = true;
                }
                Action::OpenExtensionFolder => {
                    if let Some(dir) = crate::tools::extension_dir() {
                        open_file(&dir);
                    } else {
                        self.toast = Some((
                            "Extension folder not found next to LajuFile".to_string(),
                            Instant::now(),
                        ));
                    }
                }
                Action::LaunchBrowser {
                    browser_name,
                    extensions_url,
                } => {
                    if let Some(browser) = crate::tools::detect_installed_browsers()
                        .into_iter()
                        .find(|b| b.name == browser_name)
                        && let Some(exe) = browser.executable
                    {
                        let _ = std::process::Command::new(exe).spawn();
                    }
                    self.toast = Some((
                        format!("Quick-Pair JSON copied — open {extensions_url} in {browser_name}"),
                        Instant::now(),
                    ));
                }
                Action::OpenLanguagePicker => {
                    self.show_language_picker = true;
                }
                Action::SetLanguage(code) => {
                    self.language = code.clone();
                    self.settings_draft.language = code.clone();
                    crate::i18n::set_ctx_lang(ctx, &code);
                    if let Some(snapshot) = &self.snapshot {
                        let mut settings = snapshot.settings.clone();
                        settings.language = code;
                        self.send(Command::UpdateSettings(settings));
                    }
                    let eff = crate::i18n::effective_code(&self.language);
                    let name = crate::i18n::find_language(&eff)
                        .map(|info| info.native_name)
                        .unwrap_or("English");
                    self.toast = Some((format!("Language: {name}"), Instant::now()));
                }
                Action::Add(request) => self.handle_add(request),
                Action::AddBatch(requests) => {
                    let count = requests.len();
                    for request in requests {
                        self.send(Command::Add(request));
                    }
                    if count > 0 {
                        self.toast =
                            Some((format!("Queued {count} batch downloads"), Instant::now()));
                    }
                }
                Action::LoadFormats(url) => {
                    let (reply, rx) = tokio::sync::oneshot::channel();
                    self.send(Command::VideoFormats {
                        url,
                        cookie: None,
                        reply,
                    });
                    self.formats = Some(rx);
                }
                Action::GrabPageLinks(url) => {
                    let (reply, rx) = tokio::sync::oneshot::channel();
                    self.send(Command::GrabPageLinks { url, reply });
                    self.page_links = Some(rx);
                }
                Action::OpenFolderPicker => self.open_folder_picker(),
                Action::SaveSettings(mut settings) => {
                    normalize_settings(&mut settings);
                    sync_autostart(settings.autostart);
                    self.settings_draft = settings.clone();
                    self.theme = settings.theme.clone();
                    self.ui_scale_pct = settings.ui_scale_pct;
                    apply_theme(ctx, &self.theme);
                    apply_scale(ctx, self.ui_scale_pct);
                    self.send(Command::UpdateSettings(settings));
                }
                Action::SetScale(pct) => {
                    let clamped = pct.clamp(80, 150);
                    self.ui_scale_pct = clamped;
                    self.settings_draft.ui_scale_pct = clamped;
                    apply_scale(ctx, clamped);
                    if let Some(snapshot) = &self.snapshot {
                        let mut settings = snapshot.settings.clone();
                        settings.ui_scale_pct = clamped;
                        self.send(Command::UpdateSettings(settings));
                    }
                    self.toast = Some((format!("Interface size: {clamped}%"), Instant::now()));
                }
                Action::SetSpeedLimit(kbps) => {
                    if let Some(snapshot) = &self.snapshot {
                        let mut settings = snapshot.settings.clone();
                        settings.global_speed_limit_kbps = kbps;
                        normalize_settings(&mut settings);
                        let applied = settings.global_speed_limit_kbps;
                        self.settings_draft.global_speed_limit_kbps = applied;
                        self.custom_speed_kbps = applied.to_string();
                        self.send(Command::UpdateSettings(settings));
                        let msg = if applied == 0 {
                            "Speed limit: Unlimited".to_string()
                        } else {
                            format!("Speed limit: {}", format_speed(Some(applied * 1024)))
                        };
                        self.toast = Some((msg, Instant::now()));
                    }
                }
                Action::SetTheme(theme) => {
                    self.theme = theme.clone();
                    apply_theme(ctx, &theme);
                    if let Some(snapshot) = &self.snapshot {
                        let mut settings = snapshot.settings.clone();
                        settings.theme = theme;
                        self.send(Command::UpdateSettings(settings));
                    }
                }
                Action::TestBridge(port) => {
                    let (tx, rx) = mpsc::channel();
                    std::thread::spawn(move || {
                        let _ = tx.send(bridge::health_check(port, Duration::from_secs(2)));
                    });
                    self.bridge_test = Some(rx);
                }
                Action::RegenerateToken => {
                    self.settings_draft.bridge_token = generate_token();
                }
                Action::UpdateTools => self.send(Command::UpdateTools),
            }
        }
    }

    fn send(&self, command: Command) {
        if let Some(engine) = &self.engine {
            let _ = engine.send(command);
        }
    }

    fn path_for(&self, id: i64) -> Option<std::path::PathBuf> {
        self.snapshot
            .as_ref()?
            .downloads
            .iter()
            .find(|download| download.id == id)
            .map(|download| download.save_path.clone())
    }

    fn remove_download(&mut self, id: i64, delete_file: bool) {
        self.send(Command::RemoveWithFile { id, delete_file });
        self.selected_ids.remove(&id);
        if self.selected == Some(id) {
            self.selected = self.selected_ids.iter().next().copied();
        }
        if self.progress_id == Some(id) {
            self.progress_id = None;
        }
        if self.completed_id == Some(id) {
            self.completed_id = None;
        }
        if self.properties_id == Some(id) {
            self.properties_id = None;
            self.properties_draft = None;
        }
        self.speed_history.remove(&id);
    }

    fn export_links(&mut self, ctx: &egui::Context) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let visible = ui::main_window::visible_downloads(self, &snapshot.downloads);
        let use_selected = self.selected_ids.len() > 1;
        let mut urls = Vec::new();
        let mut seen = HashSet::new();
        for d in visible {
            if use_selected && !self.selected_ids.contains(&d.id) {
                continue;
            }
            if seen.insert(d.url.clone()) {
                urls.push(d.url.clone());
            }
        }
        if urls.is_empty() {
            self.toast = Some(("No links to export".to_string(), Instant::now()));
            return;
        }
        let content = urls.join("\n");
        let dir = self.default_save_dir();
        let _ = paths::ensure_dir(&dir);
        let out_path = dir.join("lajufile-links.txt");
        self.last_clipboard = Some(content.clone());
        ctx.copy_text(content.clone());
        match std::fs::write(&out_path, content) {
            Ok(()) => {
                self.toast = Some((
                    format!("Exported {} links to {}", urls.len(), out_path.display()),
                    Instant::now(),
                ));
            }
            Err(err) => {
                self.toast = Some((
                    format!("Copied {} links (write failed: {err})", urls.len()),
                    Instant::now(),
                ));
            }
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|input| input.raw.dropped_files.clone());
        if dropped.is_empty() {
            return;
        }
        const MAX_DROP_BYTES: u64 = 512 * 1024;
        let mut collected = Vec::new();
        for file in dropped {
            let path = file.path();
            if !path.as_os_str().is_empty() {
                let ext = path
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_ascii_lowercase());
                let allowed_ext = matches!(ext.as_deref(), Some("url" | "txt" | "ef2" | "md"));
                let size_ok = std::fs::metadata(path)
                    .map(|m| m.len() <= MAX_DROP_BYTES)
                    .unwrap_or(false);
                if allowed_ext
                    && size_ok
                    && let Ok(text) = std::fs::read_to_string(path)
                {
                    collected.extend(extract_urls_from_text(&text));
                }
            } else if let Ok(bytes) = file.bytes()
                && bytes.len() <= MAX_DROP_BYTES as usize
                && let Ok(text) = std::str::from_utf8(&bytes)
            {
                collected.extend(extract_urls_from_text(text));
            }
        }
        let mut unique = Vec::new();
        let mut seen = HashSet::new();
        for url in collected {
            if seen.insert(url.clone()) {
                unique.push(url);
            }
        }
        match unique.len() {
            0 => {
                self.toast = Some((
                    "Drop a .url shortcut or .txt link list with HTTP/HTTPS URLs".to_string(),
                    Instant::now(),
                ));
            }
            1 => {
                self.add_draft = self.fresh_add_draft();
                self.add_draft.url = unique.into_iter().next().unwrap_or_default();
                self.add_focus = true;
                self.show_add = true;
            }
            _ => {
                self.batch_draft = self.fresh_batch_draft();
                self.batch_draft.mode = BatchMode::UrlList;
                self.batch_draft.urls_text = unique.join("\n");
                self.show_batch = true;
            }
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            if self.delete_confirm.is_some() {
                self.delete_confirm = None;
                return;
            }
            if self.properties_id.is_some() {
                self.properties_id = None;
                return;
            }
            if self.show_speed_popover {
                self.show_speed_popover = false;
                return;
            }
            if !self.search_query.is_empty() {
                self.search_query.clear();
                return;
            }
        }

        let blocking_modal = self.show_add
            || self.show_batch
            || self.show_settings
            || self.delete_confirm.is_some()
            || self.properties_id.is_some()
            || self.capture_info.is_some()
            || self.folder_picker.is_some();
        if blocking_modal {
            return;
        }

        let mut actions = Vec::new();
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::F)) {
            actions.push(Action::FocusSearch);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::N)) {
            actions.push(Action::OpenAdd);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::B)) {
            actions.push(Action::OpenBatch);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Comma)) {
            actions.push(Action::OpenSettings);
        }

        if !ctx.egui_wants_keyboard_input() {
            let visible_ids: Vec<i64> = self
                .snapshot
                .as_ref()
                .map(|snapshot| {
                    ui::main_window::visible_downloads(self, &snapshot.downloads)
                        .into_iter()
                        .map(|d| d.id)
                        .collect()
                })
                .unwrap_or_default();

            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::A)) {
                actions.push(Action::SelectAll);
            } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown))
            {
                if let Some(next_id) = step_selection(&visible_ids, self.selected, 1) {
                    self.selected = Some(next_id);
                    self.selected_ids.clear();
                    self.selected_ids.insert(next_id);
                }
            } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp))
                && let Some(prev_id) = step_selection(&visible_ids, self.selected, -1)
            {
                self.selected = Some(prev_id);
                self.selected_ids.clear();
                self.selected_ids.insert(prev_id);
            }

            let target_ids: Vec<i64> = if !self.selected_ids.is_empty() {
                visible_ids
                    .iter()
                    .copied()
                    .filter(|id| self.selected_ids.contains(id))
                    .collect()
            } else {
                self.selected.into_iter().collect()
            };

            if !target_ids.is_empty()
                && let Some(snapshot) = &self.snapshot
            {
                let anchor_id = self.selected.unwrap_or(target_ids[0]);
                if ctx.input_mut(|i| {
                    i.consume_key(egui::Modifiers::ALT, egui::Key::Enter)
                        || i.consume_key(egui::Modifiers::COMMAND, egui::Key::M)
                }) {
                    actions.push(Action::OpenProperties(anchor_id));
                } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
                    && let Some(anchor) = snapshot.downloads.iter().find(|d| d.id == anchor_id)
                {
                    if anchor.status == DownloadStatus::Done {
                        actions.push(Action::Open(anchor_id));
                    } else {
                        actions.push(Action::ShowProgress(anchor_id));
                    }
                } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space))
                {
                    let any_active = target_ids.iter().any(|id| {
                        snapshot.downloads.iter().any(|d| {
                            d.id == *id
                                && matches!(
                                    d.status,
                                    DownloadStatus::Downloading | DownloadStatus::Queued
                                )
                        })
                    });
                    for id in target_ids {
                        if any_active {
                            actions.push(Action::Pause(id));
                        } else {
                            actions.push(Action::Resume(id));
                        }
                    }
                } else if ctx
                    .input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::Delete))
                {
                    actions.push(Action::ConfirmRemove(target_ids, true));
                } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete))
                {
                    actions.push(Action::ConfirmRemove(target_ids, false));
                } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::C)) {
                    let urls: Vec<String> = target_ids
                        .iter()
                        .filter_map(|id| snapshot.downloads.iter().find(|d| d.id == *id))
                        .map(|d| d.url.clone())
                        .collect();
                    if !urls.is_empty() {
                        actions.push(Action::CopyUrl(urls.join("\n")));
                    }
                }
            }
        }

        if !actions.is_empty() {
            self.handle_actions(ctx, actions);
        }
    }

    fn intercept_close(&self, ctx: &egui::Context) -> bool {
        let close_to_tray = self
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.settings.close_to_tray)
            .unwrap_or(true);
        if ctx.input(|input| input.viewport().close_requested())
            && !self.allow_exit
            && tray::should_hide_on_close(close_to_tray, self.tray.is_some())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            ctx.request_repaint_after(Duration::from_millis(250));
            return true;
        }
        false
    }
}

impl eframe::App for LajuFileApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_tray(ctx);
        self.intercept_close(ctx);
        self.poll_engine(ctx);
        self.poll_clipboard(ctx);
        self.poll_bridge_test();
        self.poll_formats();
        self.poll_page_links();
        self.poll_checksum();
        self.poll_updater(ctx);
        self.update_tray_tooltip();
        ctx.request_repaint_after(Duration::from_millis(250));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        crate::i18n::set_ctx_lang(&ctx, &self.language);

        if self.intercept_close(&ctx) {
            return;
        }

        self.handle_shortcuts(&ctx);
        self.handle_dropped_files(&ctx);

        let dark = matches!(
            self.snapshot
                .as_ref()
                .map(|s| &s.settings.theme)
                .unwrap_or(&self.theme),
            Theme::Dark
        );

        let actions = egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(ui, |ui| ui::main_window::draw(self, ui))
            .inner;
        self.handle_actions(&ctx, actions);

        if self.show_add
            && let Some(action) = ui::dialogs::add_dialog(
                &ctx,
                &mut self.show_add,
                &mut self.add_draft,
                &mut self.add_focus,
            )
        {
            self.handle_actions(&ctx, vec![action]);
        }
        if self.show_batch
            && let Some(action) = ui::dialogs::batch_dialog(
                &ctx,
                &mut self.show_batch,
                &mut self.batch_draft,
                self.page_links.is_some(),
            )
        {
            self.handle_actions(&ctx, vec![action]);
        }
        if self.show_settings {
            let bridge = self.snapshot.as_ref().map(|snapshot| &snapshot.bridge);
            if let Some(action) = ui::dialogs::settings_dialog(
                &ctx,
                &mut self.show_settings,
                &mut self.settings_draft,
                bridge,
            ) {
                self.handle_actions(&ctx, vec![action]);
            }
        }
        if self.show_browser_wizard {
            let bridge = self.snapshot.as_ref().map(|snapshot| &snapshot.bridge);
            if let Some(action) = ui::dialogs::browser_setup_dialog(
                &ctx,
                &mut self.show_browser_wizard,
                &mut self.settings_draft,
                bridge,
            ) {
                self.handle_actions(&ctx, vec![action]);
            }
        }
        if self.show_language_picker
            && let Some(action) = ui::dialogs::language_dialog(
                &ctx,
                &mut self.show_language_picker,
                &self.language,
                dark,
            )
        {
            self.handle_actions(&ctx, vec![action]);
        }
        if self.show_update_dialog
            && let Some(action) = ui::dialogs::update_dialog(
                &ctx,
                &mut self.show_update_dialog,
                &self.update_state,
                dark,
            )
        {
            self.handle_actions(&ctx, vec![action]);
        }

        if let Some(id) = self.progress_id {
            let entry = self.snapshot.as_ref().and_then(|snapshot| {
                let download = snapshot.downloads.iter().find(|d| d.id == id)?.clone();
                let segments = snapshot.segments.get(&id).cloned().unwrap_or_default();
                let speed = snapshot.speeds.get(&id).copied();
                Some((download, segments, speed))
            });
            if let Some((download, segments, speed)) = entry {
                let history = self
                    .speed_history
                    .get(&id)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let (keep_open, action) = ui::dialogs::progress_dialog(
                    &ctx,
                    &download,
                    &segments,
                    speed,
                    history,
                    &mut self.progress_details,
                    dark,
                );
                if let Some(action) = action {
                    self.handle_actions(&ctx, vec![action]);
                }
                if !keep_open {
                    self.progress_id = None;
                }
            } else {
                self.progress_id = None;
            }
        }

        if let Some(id) = self.properties_id {
            let download = self
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.downloads.iter().find(|d| d.id == id).cloned());
            if let Some(download) = download {
                let draft = self
                    .properties_draft
                    .get_or_insert_with(|| PropertiesDraft::for_download(&download));
                if draft.download_id != id {
                    *draft = PropertiesDraft::for_download(&download);
                }
                let checksum_busy = self.checksum_rx.as_ref().is_some_and(|(cid, _)| *cid == id);
                let checksum_res = self
                    .checksum_result
                    .as_ref()
                    .and_then(|(cid, r)| (*cid == id).then_some(r));
                let (keep_open, action) = ui::dialogs::properties_dialog(
                    &ctx,
                    &download,
                    draft,
                    checksum_busy,
                    checksum_res,
                    dark,
                );
                if let Some(action) = action {
                    self.handle_actions(&ctx, vec![action]);
                }
                if !keep_open {
                    self.properties_id = None;
                    self.properties_draft = None;
                }
            } else {
                self.properties_id = None;
                self.properties_draft = None;
            }
        }

        if let Some((ids, mut delete_file)) = self.delete_confirm.clone() {
            let targets: Vec<_> = self
                .snapshot
                .as_ref()
                .map(|snapshot| {
                    snapshot
                        .downloads
                        .iter()
                        .filter(|d| ids.contains(&d.id))
                        .collect()
                })
                .unwrap_or_default();
            if !targets.is_empty() {
                match ui::dialogs::delete_confirm_dialog(&ctx, &targets, &mut delete_file, dark) {
                    Some(true) => {
                        self.delete_confirm = None;
                        self.handle_actions(
                            &ctx,
                            vec![Action::RemoveWithFile { ids, delete_file }],
                        );
                    }
                    Some(false) => self.delete_confirm = None,
                    None => self.delete_confirm = Some((ids, delete_file)),
                }
            } else {
                self.delete_confirm = None;
            }
        }

        if let Some(id) = self.completed_id {
            let download = self
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.downloads.iter().find(|d| d.id == id).cloned());
            if let Some(download) = download {
                let (keep_open, action) = ui::dialogs::complete_dialog(&ctx, &download, dark);
                if let Some(action) = action {
                    self.handle_actions(&ctx, vec![action]);
                }
                if !keep_open {
                    self.completed_id = None;
                }
            } else {
                self.completed_id = None;
            }
        }

        self.poll_capture_info(&ctx);
        self.poll_folder_picker(&ctx);
    }
}

impl Drop for LajuFileApp {
    fn drop(&mut self) {
        self.sleep_guard.sync(false);
        if let Some(engine) = self.engine.take() {
            engine.shutdown();
        }
    }
}

fn open_file(path: &std::path::Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

fn open_with(path: &std::path::Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("rundll32.exe")
        .args([
            std::ffi::OsStr::new("shell32.dll,OpenAs_RunDLL"),
            path.as_os_str(),
        ])
        .spawn();
    #[cfg(not(windows))]
    open_file(path);
}

fn open_folder(path: &std::path::Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer")
        .arg(format!("/select,{}", path.display()))
        .spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open")
        .args(["-R", &path.to_string_lossy()])
        .spawn();
    #[cfg(all(not(windows), not(target_os = "macos")))]
    if let Some(parent) = path.parent() {
        let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
    }
}

fn sync_autostart(enabled: bool) {
    #[cfg(windows)]
    {
        const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
        let mut command = std::process::Command::new("reg.exe");
        crate::tools::no_window(&mut command);
        if enabled {
            let Ok(exe) = std::env::current_exe() else {
                return;
            };
            let quoted = format!("\"{}\"", exe.display());
            command.args([
                "add", RUN_KEY, "/v", "LajuFile", "/t", "REG_SZ", "/d", &quoted, "/f",
            ]);
        } else {
            command.args(["delete", RUN_KEY, "/v", "LajuFile", "/f"]);
        }
        let _ = command.status();
    }
    #[cfg(target_os = "macos")]
    {
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
            return;
        };
        let agents_dir = home.join("Library/LaunchAgents");
        let plist = agents_dir.join("io.lajufile.app.plist");
        if enabled {
            let Ok(exe) = std::env::current_exe() else {
                return;
            };
            let _ = std::fs::create_dir_all(&agents_dir);
            let escaped = exe
                .to_string_lossy()
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            let body = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>io.lajufile.app</string><key>ProgramArguments</key><array><string>{escaped}</string></array><key>RunAtLoad</key><true/></dict></plist>\n"
            );
            let _ = std::fs::write(plist, body);
        } else {
            let _ = std::fs::remove_file(plist);
        }
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
            return;
        };
        let autostart_dir = home.join(".config/autostart");
        let desktop_file = autostart_dir.join("lajufile.desktop");
        if enabled {
            let Ok(exe) = std::env::current_exe() else {
                return;
            };
            let _ = std::fs::create_dir_all(&autostart_dir);
            let content = format!(
                "[Desktop Entry]\nType=Application\nName=LajuFile\nExec=\"{}\"\nTerminal=false\n",
                exe.display()
            );
            let _ = std::fs::write(desktop_file, content);
        } else {
            let _ = std::fs::remove_file(desktop_file);
        }
    }
}

fn apply_scale(ctx: &egui::Context, pct: u32) {
    let factor = (pct.clamp(80, 150) as f32) / 100.0;
    ctx.set_zoom_factor(factor);
}

fn apply_row_click(
    visible_ids: &[i64],
    anchor: &mut Option<i64>,
    selected_ids: &mut HashSet<i64>,
    id: i64,
    ctrl: bool,
    shift: bool,
) {
    if shift {
        let start_pos = anchor
            .and_then(|a| visible_ids.iter().position(|&v| v == a))
            .unwrap_or(0);
        if let Some(end_pos) = visible_ids.iter().position(|&v| v == id) {
            if !ctrl {
                selected_ids.clear();
            }
            let (lo, hi) = if start_pos <= end_pos {
                (start_pos, end_pos)
            } else {
                (end_pos, start_pos)
            };
            for &row_id in &visible_ids[lo..=hi] {
                selected_ids.insert(row_id);
            }
            if anchor.is_none() {
                *anchor = Some(id);
            }
            return;
        }
    }
    if ctrl {
        if !selected_ids.remove(&id) {
            selected_ids.insert(id);
            *anchor = Some(id);
        } else if *anchor == Some(id) {
            *anchor = selected_ids.iter().next().copied();
        }
    } else {
        selected_ids.clear();
        selected_ids.insert(id);
        *anchor = Some(id);
    }
}

fn step_selection(ids: &[i64], current: Option<i64>, delta: isize) -> Option<i64> {
    if ids.is_empty() {
        return None;
    }
    let Some(curr_id) = current else {
        return if delta >= 0 {
            ids.first().copied()
        } else {
            ids.last().copied()
        };
    };
    let Some(pos) = ids.iter().position(|&id| id == curr_id) else {
        return ids.first().copied();
    };
    let next = (pos as isize + delta).clamp(0, ids.len() as isize - 1) as usize;
    ids.get(next).copied()
}

fn should_prompt_for_folder(settings: &Settings, manual: bool) -> bool {
    manual && settings.ask_where_to_save
}

fn normalize_settings(settings: &mut Settings) {
    if settings.download_dir.as_os_str().is_empty() {
        settings.download_dir = paths::default_download_dir();
    }
    settings.max_concurrent = settings.max_concurrent.clamp(1, 10);
    settings.max_connections = settings.max_connections.clamp(1, 32);
    settings.global_speed_limit_kbps = settings.global_speed_limit_kbps.min(1_000_000);
    settings.ui_scale_pct = settings.ui_scale_pct.clamp(80, 150);
    settings.bridge_port = settings.bridge_port.clamp(1024, 65_535);
    if settings.bridge_token.len() != 64
        || !settings
            .bridge_token
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        settings.bridge_token = generate_token();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::dialogs::AddDraft;

    #[test]
    fn add_draft_trims_empty_values() {
        let draft = AddDraft {
            url: " https://example.com/a.bin ".into(),
            file_name: "  ".into(),
            folder: " ".into(),
            start_now: true,
            ..AddDraft::default()
        };
        let request = draft.into_request();
        assert_eq!(request.url, "https://example.com/a.bin");
        assert_eq!(request.file_name, None);
        assert_eq!(request.save_dir, None);
    }

    #[test]
    fn normalize_settings_clamps_limits() {
        let mut settings = Settings {
            max_concurrent: 0,
            max_connections: 99,
            global_speed_limit_kbps: 2_000_000,
            ..Settings::default()
        };
        normalize_settings(&mut settings);
        assert_eq!(settings.max_concurrent, 1);
        assert_eq!(settings.max_connections, 32);
        assert_eq!(settings.global_speed_limit_kbps, 1_000_000);
    }

    #[test]
    fn normalize_settings_raises_zero_connections() {
        let mut settings = Settings {
            max_connections: 0,
            ..Settings::default()
        };
        normalize_settings(&mut settings);
        assert_eq!(settings.max_connections, 1);
    }

    #[test]
    fn normalize_settings_clamps_bridge_port() {
        let mut settings = Settings {
            bridge_port: 0,
            ..Settings::default()
        };
        normalize_settings(&mut settings);
        assert_eq!(settings.bridge_port, 1024);
        settings.bridge_port = 65_535;
        normalize_settings(&mut settings);
        assert_eq!(settings.bridge_port, 65_535);
    }

    #[test]
    fn ask_prompt_only_when_enabled_and_manual() {
        let mut settings = Settings {
            ask_where_to_save: true,
            ..Settings::default()
        };
        assert!(should_prompt_for_folder(&settings, true));
        assert!(!should_prompt_for_folder(&settings, false));
        settings.ask_where_to_save = false;
        assert!(!should_prompt_for_folder(&settings, true));
    }

    #[test]
    fn step_selection_moves_safely_within_visible_ids() {
        assert_eq!(step_selection(&[], None, 1), None);
        assert_eq!(step_selection(&[10, 20, 30], None, 1), Some(10));
        assert_eq!(step_selection(&[10, 20, 30], None, -1), Some(30));
        assert_eq!(step_selection(&[10, 20, 30], Some(10), 1), Some(20));
        assert_eq!(step_selection(&[10, 20, 30], Some(30), 1), Some(30));
        assert_eq!(step_selection(&[10, 20, 30], Some(20), -1), Some(10));
        assert_eq!(step_selection(&[10, 20, 30], Some(99), 1), Some(10));
    }

    #[test]
    fn apply_row_click_handles_plain_ctrl_and_shift_range() {
        let visible = [1, 2, 3, 4, 5];
        let mut anchor = None;
        let mut set = HashSet::new();

        apply_row_click(&visible, &mut anchor, &mut set, 2, false, false);
        assert_eq!(anchor, Some(2));
        assert_eq!(set, HashSet::from([2]));

        apply_row_click(&visible, &mut anchor, &mut set, 4, false, true);
        assert_eq!(set, HashSet::from([2, 3, 4]));

        apply_row_click(&visible, &mut anchor, &mut set, 1, true, false);
        assert_eq!(set, HashSet::from([1, 2, 3, 4]));

        apply_row_click(&visible, &mut anchor, &mut set, 3, true, false);
        assert_eq!(set, HashSet::from([1, 2, 4]));
    }
}
