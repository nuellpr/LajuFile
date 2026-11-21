mod actions;
mod helpers;

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
use crate::ui::download_info::{DownloadInfoChoice, DownloadInfoDraft};
use crate::ui::folder_picker::{FolderChoice, FolderPicker};
use crate::ui::format::format_speed;
use crate::ui::theme::apply_theme;
use crate::ui::{self, Action, Filter, SortOrder, extract_urls_from_text};
use crate::updater;
use helpers::*;

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
    pub(crate) startup_update_checked: bool,
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
    pub(crate) hf_files: Option<
        tokio::sync::oneshot::Receiver<
            Result<Vec<crate::engine::huggingface::HfModelFile>, String>,
        >,
    >,
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
    pub(crate) clipboard: Option<arboard::Clipboard>,
    pub(crate) last_clipboard: Option<String>,
    pub(crate) clipboard_initialized: bool,
    pub(crate) last_clipboard_poll: Instant,
    pub(crate) last_wake_seq: u64,
    pub(crate) sleep_guard: crate::platform::SleepGuard,
    pub(crate) was_downloading: bool,
    pub(crate) startup_trimmed: bool,
}

impl LajuFileApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let data_dir = paths::data_dir();
        let (settings, settings_warn) = load_settings_with_backup(&data_dir);
        if settings.autostart {
            sync_autostart(true);
        }
        let theme = settings.theme.clone();
        let ui_scale_pct = settings.ui_scale_pct.clamp(80, 150);
        let language = settings.language.clone();
        crate::i18n::install_system_fonts_for_language(&cc.egui_ctx, &language);
        crate::i18n::set_ctx_lang(&cc.egui_ctx, &language);
        apply_theme(&cc.egui_ctx, &theme);
        apply_scale(&cc.egui_ctx, ui_scale_pct);

        let icon = eframe::icon_data::from_png_bytes(include_bytes!(
            "../../../../assets/icons/icon-32.png"
        ))
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
        let toast = if engine.is_none() {
            Some(("Engine failed to start".to_string(), Instant::now()))
        } else {
            settings_warn.map(|msg| (msg, Instant::now()))
        };

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
            startup_update_checked: false,
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
            hf_files: None,
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
            clipboard: None,
            last_clipboard: None,
            clipboard_initialized: false,
            last_clipboard_poll: Instant::now(),
            last_wake_seq: 0,
            sleep_guard: crate::platform::SleepGuard::new(),
            was_downloading: false,
            startup_trimmed: false,
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
        if snapshot.exit_requested && !self.allow_exit {
            self.allow_exit = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let any_downloading = snapshot
            .downloads
            .iter()
            .any(|download| download.status == DownloadStatus::Downloading);
        self.sleep_guard.sync(any_downloading);
        if (self.was_downloading && !any_downloading) || (!self.startup_trimmed && !any_downloading)
        {
            if self.speed_history.is_empty() {
                self.speed_history.shrink_to_fit();
            }
            self.startup_trimmed = true;
            crate::platform::trim_process_memory();
        }
        self.was_downloading = any_downloading;

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
            crate::i18n::install_system_fonts_for_language(ctx, &self.language);
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
        if !self.startup_update_checked {
            self.startup_update_checked = true;
            if matches!(self.update_state, UpdateUiState::Idle)
                && should_auto_check_updates(
                    snapshot.settings.auto_check_updates,
                    snapshot.settings.last_update_check_ms,
                    lajufile_core::store::now_millis(),
                )
            {
                self.start_update_check();
            }
        }
        let pending = snapshot.pending_capture.clone();
        self.snapshot = Some(snapshot);
        match pending {
            Some(prompt) if self.dismissed_capture != Some(prompt.key) => {
                let mut refreshed_cat = None;
                if let Some((active_key, draft)) = &mut self.capture_info {
                    if *active_key == prompt.key {
                        if draft.size.is_none() && prompt.size.is_some() {
                            draft.size = prompt.size;
                        }
                        if draft.ranged.is_none() && prompt.ranged.is_some() {
                            draft.ranged = prompt.ranged;
                        }
                        if !draft.user_edited_name
                            && !prompt.file_name.is_empty()
                            && draft.file_name != prompt.file_name
                        {
                            draft.file_name = prompt.file_name.clone();
                            draft.category = Category::from_file_name(&draft.file_name);
                            if !draft.user_edited_folder {
                                refreshed_cat = Some(draft.category);
                            }
                        }
                    } else {
                        self.capture_info = None;
                    }
                }
                if let Some(cat) = refreshed_cat {
                    let folder = self.default_folder_for(cat);
                    if let Some((_, draft)) = &mut self.capture_info {
                        draft.folder = folder.display().to_string();
                    }
                }
                if self.capture_info.is_none() {
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
                            user_edited_name: false,
                            user_edited_folder: false,
                        },
                    ));
                }
            }
            Some(_) => {}
            None => {
                self.capture_info = None;
                self.dismissed_capture = None;
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

    fn poll_hf_files(&mut self) {
        let received = self.hf_files.as_mut().map(|rx| rx.try_recv());
        match received {
            Some(Ok(Ok(files))) => {
                self.hf_files = None;
                if let Some(first) = files.first() {
                    self.add_draft.file_name = first.file_name.clone();
                    if let Some(sha) = &first.sha256 {
                        self.add_draft.expected_sha256 = sha.clone();
                    }
                    self.add_draft.hf_selected = Some(0);
                }
                let count = files.len();
                self.add_draft.hf_files = files;
                self.toast = Some((
                    format!("Loaded {count} HuggingFace model file(s)"),
                    Instant::now(),
                ));
            }
            Some(Ok(Err(error))) => {
                self.hf_files = None;
                self.toast = Some((error, Instant::now()));
            }
            Some(Err(tokio::sync::oneshot::error::TryRecvError::Closed)) => self.hf_files = None,
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
                    .get(category.label())
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

    fn configured_proxy(&self) -> Option<String> {
        self.snapshot
            .as_ref()
            .map(|s| s.settings.proxy_url.trim().to_string())
            .filter(|p| !p.is_empty())
    }

    fn start_update_check(&mut self) {
        self.update_state = UpdateUiState::Checking;
        let (tx, rx) = mpsc::channel();
        self.update_rx = Some(rx);
        updater::spawn_check(tx, self.configured_proxy());
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
                    self.send(Command::RecordUpdateCheck(
                        lajufile_core::store::now_millis(),
                    ));
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
                        draft.user_edited_folder = true;
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
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        let current = match self.clipboard.as_mut() {
            Some(cb) => match cb.get_text() {
                Ok(text) => Some(text),
                Err(_) => {
                    self.clipboard = None;
                    None
                }
            },
            None => None,
        };
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
                if remember
                    && !folder.as_os_str().is_empty()
                    && let Some(snapshot) = &self.snapshot
                {
                    let mut settings = snapshot.settings.clone();
                    settings
                        .category_dirs
                        .insert(draft.category.label().to_string(), folder);
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

    fn intercept_close(&mut self, ctx: &egui::Context) -> bool {
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
            self.clipboard = None;
            crate::platform::trim_process_memory();
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
        self.poll_hf_files();
        self.poll_page_links();
        self.poll_checksum();
        self.poll_updater(ctx);
        self.update_tray_tooltip();
        let has_active_downloads = self.snapshot.as_ref().is_some_and(|s| {
            s.downloads.iter().any(|d| {
                matches!(
                    d.status,
                    DownloadStatus::Downloading | DownloadStatus::Queued
                )
            })
        });
        let is_busy = has_active_downloads
            || self.bridge_test.is_some()
            || self.formats.is_some()
            || self.hf_files.is_some()
            || self.page_links.is_some()
            || self.checksum_rx.is_some()
            || self.update_rx.is_some()
            || self.capture_info.is_some()
            || self.toast.is_some();
        let repaint_after = if is_busy {
            Duration::from_millis(250)
        } else {
            Duration::from_millis(1000)
        };
        ctx.request_repaint_after(repaint_after);
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

        if let Some((pending_action, deadline_ms)) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.pending_queue_action)
        {
            let remaining_s =
                ((deadline_ms - lajufile_core::store::now_millis()).max(0) + 999) / 1000;
            let verb = match pending_action {
                lajufile_core::settings::QueueCompleteAction::ExitApp => "Closing LajuFile",
                lajufile_core::settings::QueueCompleteAction::Sleep => "Sleeping computer",
                lajufile_core::settings::QueueCompleteAction::Shutdown => "Shutting down computer",
                lajufile_core::settings::QueueCompleteAction::None => "",
            };
            if !verb.is_empty() {
                egui::Window::new("Scheduled Action")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 18.0))
                    .show(&ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "All downloads finished — {verb} in {remaining_s}s"
                                ))
                                .strong(),
                            );
                            if ui.button("Cancel").clicked() {
                                self.handle_actions(&ctx, vec![Action::CancelQueueComplete]);
                            }
                        });
                    });
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        }
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
