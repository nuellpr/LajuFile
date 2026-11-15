use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use lajufile_core::paths;
use lajufile_core::settings::{Settings, Theme, generate_token};
use lajufile_core::store::{Category, DownloadStatus};
use lajufile_core::video::VideoListing;

use crate::bridge;
use crate::engine::{AddRequest, Command, EngineConfig, EngineHandle, Snapshot};
use crate::tray::{self, Tray, TrayEvent};
use crate::ui::dialogs::{AddDraft, BatchDraft, clipboard_download_url};
use crate::ui::download_info::{DownloadInfoChoice, DownloadInfoDraft, category_label};
use crate::ui::folder_picker::{FolderChoice, FolderPicker};
use crate::ui::theme::apply_theme;
use crate::ui::{self, Action, Filter};

pub struct LajuFileApp {
    pub(crate) engine: Option<EngineHandle>,
    pub(crate) snapshot: Option<Snapshot>,
    pub(crate) selected: Option<i64>,
    pub(crate) filter: Filter,
    pub(crate) show_add: bool,
    pub(crate) show_batch: bool,
    pub(crate) show_settings: bool,
    pub(crate) add_draft: AddDraft,
    pub(crate) batch_draft: BatchDraft,
    pub(crate) add_focus: bool,
    pub(crate) settings_draft: Settings,
    pub(crate) toast: Option<(String, Instant)>,
    pub(crate) last_message: Option<String>,
    pub(crate) theme: Theme,
    pub(crate) tray: Option<Tray>,
    pub(crate) allow_exit: bool,
    pub(crate) tray_tooltip: String,
    pub(crate) bridge_test: Option<mpsc::Receiver<Result<(), String>>>,
    pub(crate) formats: Option<tokio::sync::oneshot::Receiver<Result<VideoListing, String>>>,
    pub(crate) folder_picker: Option<FolderPicker>,
    pub(crate) pending_add: Option<AddRequest>,
    pub(crate) capture_info: Option<(u64, DownloadInfoDraft)>,
    pub(crate) dismissed_capture: Option<u64>,
    pub(crate) progress_id: Option<i64>,
    pub(crate) progress_details: bool,
    pub(crate) completed_id: Option<i64>,
    pub(crate) seen_done: Option<HashSet<i64>>,
    pub(crate) last_clipboard: Option<String>,
    pub(crate) clipboard_initialized: bool,
    pub(crate) last_clipboard_poll: Instant,
}

impl LajuFileApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let data_dir = paths::data_dir();
        let settings = Settings::load_or_create(&data_dir).unwrap_or_default();
        if settings.autostart {
            sync_autostart(true);
        }
        let theme = settings.theme.clone();
        apply_theme(&cc.egui_ctx, &theme);

        let icon =
            eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icons/icon-32.png"))
                .ok();
        let tray = icon
            .as_ref()
            .and_then(|icon| tray::create(icon, &cc.egui_ctx));

        let add_draft = AddDraft::for_settings(&settings);
        let batch_draft = BatchDraft::for_settings(&settings);
        let settings_draft = settings.clone();
        let engine = EngineHandle::start(EngineConfig {
            data_dir: data_dir.clone(),
            db_path: paths::db_path(&data_dir),
            settings,
        })
        .ok();
        let toast = engine
            .is_none()
            .then(|| ("Engine failed to start".to_string(), Instant::now()));

        Self {
            engine,
            snapshot: None,
            selected: None,
            filter: Filter::All,
            show_add: false,
            show_batch: false,
            show_settings: false,
            add_draft,
            batch_draft,
            add_focus: false,
            settings_draft,
            toast,
            last_message: None,
            theme,
            tray,
            allow_exit: false,
            tray_tooltip: String::new(),
            bridge_test: None,
            formats: None,
            folder_picker: None,
            pending_add: None,
            capture_info: None,
            dismissed_capture: None,
            progress_id: None,
            progress_details: true,
            completed_id: None,
            seen_done: None,
            last_clipboard: None,
            clipboard_initialized: false,
            last_clipboard_poll: Instant::now(),
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
        let Some(snapshot) = engine.latest_snapshot() else {
            return;
        };

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
                    self.completed_id = Some(*id);
                }
            }
            *seen = done_now;
        } else {
            self.seen_done = Some(done_now);
        }
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
        self.folder_picker = Some(FolderPicker::new(self.default_save_dir()));
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
        match picker.show(ctx) {
            Some(FolderChoice::Select(path)) => {
                if let Some((_, draft)) = &mut self.capture_info {
                    draft.folder = path.display().to_string();
                } else if self.show_batch {
                    self.batch_draft.folder = path.display().to_string();
                } else {
                    self.add_draft.folder = path.display().to_string();
                    if let Some(mut request) = self.pending_add.take() {
                        request.save_dir = Some(path);
                        self.send(Command::Add(request));
                        self.show_add = false;
                    }
                }
            }
            Some(FolderChoice::Cancel) => self.pending_add = None,
            None => self.folder_picker = Some(picker),
        }
    }

    fn poll_clipboard(&mut self, ctx: &egui::Context) {
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
        let enabled = self
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.settings.clipboard_monitor)
            .unwrap_or(true);
        if !enabled || self.show_add || self.show_batch || self.capture_info.is_some() {
            return;
        }
        let Some(text) = current else {
            return;
        };
        let Some(url) = clipboard_download_url(&text) else {
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
        match crate::ui::download_info::download_info_dialog(ctx, &mut draft) {
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
                Action::Select(id) => self.selected = Some(id),
                Action::Pause(id) => self.send(Command::Pause(id)),
                Action::Resume(id) => self.send(Command::Resume(id)),
                Action::Remove(id) => {
                    self.send(Command::Remove(id));
                    if self.selected == Some(id) {
                        self.selected = None;
                    }
                    if self.progress_id == Some(id) {
                        self.progress_id = None;
                    }
                    if self.completed_id == Some(id) {
                        self.completed_id = None;
                    }
                }
                Action::RemoveCompleted => {
                    self.send(Command::RemoveCompleted);
                    self.completed_id = None;
                }
                Action::ShowProgress(id) => {
                    self.progress_id = Some(id);
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
                Action::OpenFolder(id) => {
                    if let Some(path) = self.path_for(id) {
                        open_folder(&path);
                    }
                }
                Action::CopyUrl(url) => {
                    self.last_clipboard = Some(url.clone());
                    ctx.copy_text(url);
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
                    self.send(Command::VideoFormats { url, reply });
                    self.formats = Some(rx);
                }
                Action::OpenFolderPicker => self.open_folder_picker(),
                Action::SaveSettings(mut settings) => {
                    normalize_settings(&mut settings);
                    sync_autostart(settings.autostart);
                    self.settings_draft = settings.clone();
                    self.theme = settings.theme.clone();
                    apply_theme(ctx, &self.theme);
                    self.send(Command::UpdateSettings(settings));
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
        self.update_tray_tooltip();
        ctx.request_repaint_after(Duration::from_millis(250));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        if self.intercept_close(&ctx) {
            return;
        }

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
            && let Some(action) =
                ui::dialogs::batch_dialog(&ctx, &mut self.show_batch, &mut self.batch_draft)
        {
            self.handle_actions(&ctx, vec![action]);
        }
        if self.show_settings
            && let Some(action) = ui::dialogs::settings_dialog(
                &ctx,
                &mut self.show_settings,
                &mut self.settings_draft,
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
                let (keep_open, action) = ui::dialogs::progress_dialog(
                    &ctx,
                    &download,
                    &segments,
                    speed,
                    &mut self.progress_details,
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

        if let Some(id) = self.completed_id {
            let download = self
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.downloads.iter().find(|d| d.id == id).cloned());
            if let Some(download) = download {
                let (keep_open, action) = ui::dialogs::complete_dialog(&ctx, &download);
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
        if let Some(engine) = self.engine.take() {
            engine.shutdown();
        }
    }
}

fn open_file(path: &std::path::Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(not(windows))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

fn open_folder(path: &std::path::Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer")
        .arg(format!("/select,{}", path.display()))
        .spawn();
    #[cfg(not(windows))]
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
    #[cfg(not(windows))]
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
}
