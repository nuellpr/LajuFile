use std::time::{Duration, Instant};

use lajufile_core::paths;
use lajufile_core::settings::{Settings, Theme};
use lajufile_core::store::DownloadStatus;

use crate::engine::{Command, EngineConfig, EngineHandle, Snapshot};
use crate::tray::{self, Tray, TrayEvent};
use crate::ui::dialogs::AddDraft;
use crate::ui::theme::apply_theme;
use crate::ui::{self, Action, Filter};

pub struct LajuFileApp {
    pub(crate) engine: Option<EngineHandle>,
    pub(crate) snapshot: Option<Snapshot>,
    pub(crate) selected: Option<i64>,
    pub(crate) filter: Filter,
    pub(crate) show_add: bool,
    pub(crate) show_settings: bool,
    pub(crate) add_draft: AddDraft,
    pub(crate) add_focus: bool,
    pub(crate) settings_draft: Settings,
    pub(crate) toast: Option<(String, Instant)>,
    pub(crate) last_message: Option<String>,
    pub(crate) theme: Theme,
    pub(crate) tray: Option<Tray>,
    pub(crate) allow_exit: bool,
    pub(crate) tray_tooltip: String,
}

impl LajuFileApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let data_dir = paths::data_dir();
        let settings = Settings::load_or_create(&data_dir).unwrap_or_default();
        let theme = settings.theme.clone();
        apply_theme(&cc.egui_ctx, &theme);

        let icon =
            eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icons/icon-32.png"))
                .ok();
        let tray = icon
            .as_ref()
            .and_then(|icon| tray::create(icon, &cc.egui_ctx));

        let add_draft = AddDraft::for_settings(&settings);
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
            show_settings: false,
            add_draft,
            add_focus: false,
            settings_draft,
            toast,
            last_message: None,
            theme,
            tray,
            allow_exit: false,
            tray_tooltip: String::new(),
        }
    }

    fn fresh_add_draft(&self) -> AddDraft {
        self.snapshot
            .as_ref()
            .map(|snapshot| AddDraft::for_settings(&snapshot.settings))
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
        self.snapshot = Some(snapshot);
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
                }
                Action::PauseAll => self.send(Command::PauseAll),
                Action::ResumeAll => self.send(Command::ResumeAll),
                Action::Exit => {
                    self.allow_exit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Action::Open(id) => {
                    if let Some(path) = self.path_for(id) {
                        let _ = std::process::Command::new("explorer").arg(path).spawn();
                    }
                }
                Action::OpenFolder(id) => {
                    if let Some(path) = self.path_for(id) {
                        let _ = std::process::Command::new("explorer")
                            .arg(format!("/select,{}", path.display()))
                            .spawn();
                    }
                }
                Action::CopyUrl(url) => ctx.copy_text(url),
                Action::OpenAdd => {
                    self.add_draft = self.fresh_add_draft();
                    self.add_focus = true;
                    self.show_add = true;
                }
                Action::OpenSettings => {
                    if let Some(snapshot) = &self.snapshot {
                        self.settings_draft = snapshot.settings.clone();
                    }
                    self.show_settings = true;
                }
                Action::Add(request) => self.send(Command::Add(request)),
                Action::SaveSettings(mut settings) => {
                    normalize_settings(&mut settings);
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
}

impl eframe::App for LajuFileApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_tray(ctx);
        self.poll_engine(ctx);
        self.update_tray_tooltip();
        ctx.request_repaint_after(Duration::from_millis(250));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

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
        if self.show_settings
            && let Some(action) = ui::dialogs::settings_dialog(
                &ctx,
                &mut self.show_settings,
                &mut self.settings_draft,
            )
        {
            self.handle_actions(&ctx, vec![action]);
        }
    }
}

impl Drop for LajuFileApp {
    fn drop(&mut self) {
        if let Some(engine) = self.engine.take() {
            engine.shutdown();
        }
    }
}

fn normalize_settings(settings: &mut Settings) {
    if settings.download_dir.as_os_str().is_empty() {
        settings.download_dir = paths::default_download_dir();
    }
    settings.max_concurrent = settings.max_concurrent.clamp(1, 10);
    settings.max_connections = settings.max_connections.clamp(1, 32);
    settings.global_speed_limit_kbps = settings.global_speed_limit_kbps.min(1_000_000);
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
            connections: 8,
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
}
