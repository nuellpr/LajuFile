use std::collections::HashSet;
use std::path::Path;

use lajufile_core::paths;
use lajufile_core::settings::{Settings, generate_token, is_valid_token};

pub(super) fn open_file(path: &std::path::Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

pub(super) fn open_with(path: &std::path::Path) {
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

pub(super) fn open_folder(path: &std::path::Path) {
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

pub(super) fn sync_autostart(enabled: bool) {
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
        let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
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
        let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
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

pub(super) fn apply_scale(ctx: &egui::Context, pct: u32) {
    let factor = (pct.clamp(80, 150) as f32) / 100.0;
    ctx.set_zoom_factor(factor);
}

pub(super) fn apply_row_click(
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

pub(super) fn step_selection(ids: &[i64], current: Option<i64>, delta: isize) -> Option<i64> {
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

pub(super) fn should_prompt_for_folder(settings: &Settings, manual: bool) -> bool {
    manual && settings.ask_where_to_save
}

pub(super) const AUTO_UPDATE_INTERVAL_MS: i64 = 24 * 60 * 60 * 1000;

pub(super) fn should_auto_check_updates(enabled: bool, last_check_ms: i64, now_ms: i64) -> bool {
    enabled
        && (last_check_ms <= 0 || now_ms.saturating_sub(last_check_ms) >= AUTO_UPDATE_INTERVAL_MS)
}

pub(super) fn load_settings_with_backup(dir: &Path) -> (Settings, Option<String>) {
    match Settings::load_or_create(dir) {
        Ok(settings) => (settings, None),
        Err(err) => {
            let path = paths::settings_path(dir);
            let backup = dir.join("settings.json.bak");
            let msg = if path.exists() && std::fs::rename(&path, &backup).is_ok() {
                format!(
                    "Settings reset to defaults (corrupt file backed up to settings.json.bak: {err})"
                )
            } else {
                format!("Settings could not be loaded ({err}); using defaults")
            };
            (Settings::default(), Some(msg))
        }
    }
}

pub(super) fn normalize_settings(settings: &mut Settings) {
    if settings.download_dir.as_os_str().is_empty() {
        settings.download_dir = paths::default_download_dir();
    }
    settings.max_concurrent = settings.max_concurrent.clamp(1, 10);
    settings.max_connections = settings.max_connections.clamp(1, 32);
    settings.global_speed_limit_kbps = settings.global_speed_limit_kbps.min(1_000_000);
    settings.ui_scale_pct = settings.ui_scale_pct.clamp(80, 150);
    settings.bridge_port = settings.bridge_port.clamp(1024, 65_535);
    settings.proxy_url = settings.proxy_url.trim().to_string();
    if !is_valid_token(&settings.bridge_token) {
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
            proxy_url: " http://127.0.0.1:7890 ".into(),
            ..Settings::default()
        };
        normalize_settings(&mut settings);
        assert_eq!(settings.max_concurrent, 1);
        assert_eq!(settings.max_connections, 32);
        assert_eq!(settings.global_speed_limit_kbps, 1_000_000);
        assert_eq!(settings.proxy_url, "http://127.0.0.1:7890");
    }

    #[test]
    fn should_auto_check_updates_respects_flag_and_24h_cooldown() {
        let now = 2_000_000_000_000i64;
        assert!(!should_auto_check_updates(false, 0, now));
        assert!(should_auto_check_updates(true, 0, now));
        assert!(!should_auto_check_updates(true, now - 60_000, now));
        assert!(should_auto_check_updates(
            true,
            now - 24 * 60 * 60 * 1000 - 1,
            now
        ));
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

    #[test]
    fn load_settings_with_backup_preserves_corrupt_file() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = paths::settings_path(dir.path());
        std::fs::write(&settings_path, "{ not valid json").unwrap();

        let (settings, warning) = load_settings_with_backup(dir.path());
        assert_eq!(
            settings.max_connections,
            Settings::default().max_connections
        );
        assert!(
            warning
                .as_deref()
                .is_some_and(|w| w.contains("settings.json.bak"))
        );
        let backup = dir.path().join("settings.json.bak");
        assert!(backup.exists());
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            "{ not valid json"
        );
    }
}
