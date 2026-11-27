use std::sync::{Arc, Mutex};

use tray_icon::menu::{IsMenuItem, Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayEvent {
    Show,
    Add,
    PauseAll,
    ResumeAll,
    Exit,
}

pub struct Tray {
    icon: TrayIcon,
    events: Arc<Mutex<Vec<TrayEvent>>>,
}

pub fn create(icon: &egui::IconData, ctx: &egui::Context, lang: &str) -> Option<Tray> {
    let icon = Icon::from_rgba(icon.rgba.clone(), icon.width, icon.height).ok()?;
    let menu = Menu::new();
    let t = |en: &str| crate::i18n::translate(lang, en).into_owned();
    let show = MenuItem::with_id("show", t("Show LajuFile"), true, None);
    let add = MenuItem::with_id("add", t("Add URL..."), true, None);
    let pause = MenuItem::with_id("pause-all", t("Stop all"), true, None);
    let resume = MenuItem::with_id("resume-all", t("Resume all"), true, None);
    let exit = MenuItem::with_id("exit", t("Exit"), true, None);
    let items: [&dyn IsMenuItem; 5] = [&show, &add, &pause, &resume, &exit];
    menu.append_items(&items).ok()?;

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("LajuFile")
        .with_icon(icon)
        .with_menu_on_left_click(false)
        .build()
        .ok()?;

    let events = Arc::new(Mutex::new(Vec::new()));

    let queue = events.clone();
    let repaint = ctx.clone();
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        let clicked = matches!(
            event,
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            }
        );
        if clicked && let Ok(mut queue) = queue.lock() {
            queue.push(TrayEvent::Show);
            repaint.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            repaint.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            repaint.send_viewport_cmd(egui::ViewportCommand::Focus);
            repaint.request_repaint();
        }
    }));

    let queue = events.clone();
    let repaint = ctx.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let action = match event.id.as_ref() {
            "show" => Some(TrayEvent::Show),
            "add" => Some(TrayEvent::Add),
            "pause-all" => Some(TrayEvent::PauseAll),
            "resume-all" => Some(TrayEvent::ResumeAll),
            "exit" => Some(TrayEvent::Exit),
            _ => None,
        };
        if let Some(action) = action
            && let Ok(mut queue) = queue.lock()
        {
            queue.push(action);
            if matches!(action, TrayEvent::Show | TrayEvent::Add) {
                repaint.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                repaint.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                repaint.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            repaint.request_repaint();
        }
    }));

    Some(Tray { icon: tray, events })
}

impl Tray {
    pub fn poll(&self) -> Vec<TrayEvent> {
        let mut events = self.events.lock().expect("tray event queue");
        std::mem::take(&mut *events)
    }

    pub fn set_tooltip(&self, text: &str) {
        let _ = self.icon.set_tooltip(Some(text));
    }
}

pub fn should_hide_on_close(close_to_tray: bool, tray_available: bool) -> bool {
    close_to_tray && tray_available
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_hides_only_when_tray_exists() {
        assert!(should_hide_on_close(true, true));
        assert!(!should_hide_on_close(false, true));
        assert!(!should_hide_on_close(true, false));
    }
}
