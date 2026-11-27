use std::path::PathBuf;

use egui::{
    Align, Align2, Color32, FontId, Layout, Margin, Rect, RichText, Sense, Stroke, Ui, Vec2,
};
use lajufile_core::settings::{DEFAULT_CAPTURE_EXTENSIONS, DuplicatePolicy, Settings, Theme};

use crate::bridge::{BridgeState, BridgeStatus};

use super::super::Action;
use super::browser_lang::quick_pair_payload;
use super::{
    SettingsPalette, card_kicker, draw_h_divider, figma_button, mask_token, modal_header,
    paint_folder_icon, segmented_options, setting_row, settings_card, stepper_widget, toggle_row,
    toggle_row_interactive,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    General,
    Downloads,
    BrowserCapture,
    Appearance,
    About,
}

pub fn settings_dialog(
    ctx: &egui::Context,
    open: &mut bool,
    settings: &mut Settings,
    bridge: Option<&BridgeStatus>,
) -> Option<Action> {
    let sp = SettingsPalette::from_dark(matches!(settings.theme, Theme::Dark));
    let tab_id = egui::Id::unique("lajufile_settings_tab");
    let mut active_tab = ctx.data_mut(|data| {
        data.get_temp::<SettingsTab>(tab_id)
            .unwrap_or(SettingsTab::Downloads)
    });
    let mut action = None;

    let style = ctx.style_of(if matches!(settings.theme, Theme::Dark) {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    let frame = egui::Frame::window(&style)
        .fill(sp.dialog_bg)
        .stroke(Stroke::new(1.0, sp.border))
        .corner_radius(14)
        .inner_margin(Margin::ZERO);

    egui::Window::new("Settings")
        .id(egui::Id::unique("lajufile_settings_modal"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .frame(frame)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_width(820.0);
            ui.spacing_mut().item_spacing = Vec2::ZERO;

            if modal_header(
                ui,
                &crate::i18n::t_ctx(ctx, "Settings"),
                "Manage downloads, performance, and browser capture.",
                &sp,
            ) {
                *open = false;
            }

            draw_h_divider(ui, sp.border);

            // Body: Left nav (188px) + right scrollable content (631px)
            let body_h = 436.0;
            ui.horizontal(|ui| {
                ui.set_height(body_h);
                ui.allocate_ui_with_layout(
                    Vec2::new(188.0, body_h),
                    Layout::top_down(Align::Min),
                    |ui| {
                        egui::Frame::new()
                            .fill(sp.nav_bg)
                            .inner_margin(Margin::symmetric(10, 14))
                            .show(ui, |ui| {
                                ui.set_width(168.0);
                                ui.set_height(body_h - 28.0);
                                ui.vertical(|ui| {
                                    ui.set_width(168.0);
                                    ui.set_height(body_h - 28.0);
                                    let bridge_dot = bridge.map(|status| match status.state {
                                        BridgeState::Bound => sp.green,
                                        BridgeState::Error => sp.red,
                                        BridgeState::Starting => sp.amber,
                                    });
                                    for (tab, label, dot) in [
                                        (SettingsTab::General, "General", None),
                                        (SettingsTab::Downloads, "Downloads", None),
                                        (
                                            SettingsTab::BrowserCapture,
                                            "Browser capture",
                                            bridge_dot,
                                        ),
                                        (SettingsTab::Appearance, "Appearance", None),
                                        (SettingsTab::About, "About", None),
                                    ] {
                                        if settings_nav_item(
                                            ui,
                                            tab,
                                            &crate::i18n::t_ctx(ctx, label),
                                            active_tab == tab,
                                            dot,
                                            &sp,
                                        ) {
                                            active_tab = tab;
                                        }
                                        ui.add_space(4.0);
                                    }
                                    let footer_pad = (ui.available_height() - 16.0).max(8.0);
                                    ui.add_space(footer_pad);
                                    ui.label(
                                        RichText::new(concat!(
                                            "LajuFile ",
                                            env!("CARGO_PKG_VERSION")
                                        ))
                                        .size(10.0)
                                        .color(sp.faint),
                                    );
                                });
                            });
                    },
                );

                let v_div = ui
                    .allocate_exact_size(Vec2::new(1.0, body_h), Sense::hover())
                    .0;
                ui.painter().rect_filled(v_div, 0.0, sp.border);

                ui.allocate_ui_with_layout(
                    Vec2::new(ui.available_width(), body_h),
                    Layout::top_down(Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("lajufile_settings_scroll")
                            .max_height(body_h)
                            .show(ui, |ui| {
                                egui::Frame::new()
                                    .fill(sp.dialog_bg)
                                    .inner_margin(Margin::symmetric(24, 20))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        draw_settings_tab(
                                            ui,
                                            active_tab,
                                            settings,
                                            bridge,
                                            &sp,
                                            &mut action,
                                        );
                                    });
                            });
                    },
                );
            });

            draw_h_divider(ui, sp.border);

            // Footer bar (Figma 9:455 - 9:458)
            let proxy_err = lajufile_core::http::validate_proxy_url(&settings.proxy_url).err();
            let schedule_err = if settings.schedule_enabled
                && (lajufile_core::settings::parse_hhmm(&settings.schedule_start).is_none()
                    || lajufile_core::settings::parse_hhmm(&settings.schedule_stop).is_none())
            {
                Some("Schedule times must use HH:MM (24-hour format).".to_string())
            } else {
                None
            };
            let validation_err = proxy_err.or(schedule_err);
            egui::Frame::new()
                .fill(sp.dialog_bg)
                .inner_margin(Margin::symmetric(24, 12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        if let Some(err) = &validation_err {
                            ui.label(RichText::new(err).size(10.5).color(sp.red));
                        } else {
                            ui.label(
                                RichText::new("Click Done to save and apply changes")
                                    .size(10.5)
                                    .color(sp.faint),
                            );
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if figma_button(
                                ui,
                                &crate::i18n::t_ctx(ctx, "Done"),
                                90.0,
                                34.0,
                                true,
                                &sp,
                            ) && validation_err.is_none()
                            {
                                action = Some(Action::SaveSettings(settings.clone()));
                                *open = false;
                            }
                            ui.add_space(8.0);
                            if figma_button(
                                ui,
                                &crate::i18n::t_ctx(ctx, "Cancel"),
                                84.0,
                                34.0,
                                false,
                                &sp,
                            ) {
                                *open = false;
                            }
                        });
                    });
                });
        });

    ctx.data_mut(|data| data.insert_temp(tab_id, active_tab));
    action
}

fn draw_settings_tab(
    ui: &mut Ui,
    tab: SettingsTab,
    settings: &mut Settings,
    bridge: Option<&BridgeStatus>,
    sp: &SettingsPalette,
    action: &mut Option<Action>,
) {
    match tab {
        SettingsTab::Downloads => {
            tab_header(
                ui,
                "Downloads",
                "Choose where files go and how much bandwidth LajuFile uses.",
                sp,
            );
            draw_save_location_card(ui, settings, sp, action);
            ui.add_space(12.0);
            draw_category_folders_card(ui, settings, sp, action);
            ui.add_space(12.0);
            draw_performance_card(ui, settings, sp);
            ui.add_space(12.0);
            draw_browser_capture_card(ui, settings, bridge, sp, action);
            ui.add_space(12.0);
            draw_behavior_card(ui, settings, sp);
        }
        SettingsTab::General => {
            tab_header(
                ui,
                "General",
                "Startup behavior, tray integration, and folder defaults.",
                sp,
            );
            draw_save_location_card(ui, settings, sp, action);
            ui.add_space(12.0);
            draw_behavior_card(ui, settings, sp);
        }
        SettingsTab::BrowserCapture => {
            tab_header(
                ui,
                "Browser capture",
                "Pair LajuFile with Chrome, Brave, Edge, Opera, or Vivaldi to intercept downloads.",
                sp,
            );
            draw_browser_capture_card(ui, settings, bridge, sp, action);
            ui.add_space(12.0);
            settings_card(ui, sp, |ui| {
                card_kicker(ui, "BROWSER WORKFLOW & SETUP", sp);
                ui.add_space(8.0);
                let mut three_step_flow = settings.ask_where_to_save
                    && settings.show_progress_on_start
                    && settings.show_complete_dialog;
                if toggle_row_interactive(
                    ui,
                    "Classic 3-step popup flow (Info -> Progress -> Complete)",
                    "settings_browser_3step",
                    &mut three_step_flow,
                    sp,
                ) {
                    settings.ask_where_to_save = three_step_flow;
                    settings.show_progress_on_start = three_step_flow;
                    settings.show_complete_dialog = true;
                }
                ui.add_space(10.0);
                for step in [
                    "1. Microsoft Edge: install LajuFile Capture directly from Microsoft Edge Add-ons (no Developer Mode).",
                    "2. Chrome / Brave / Opera / Vivaldi: open chrome://extensions, enable Developer mode, and Load unpacked.",
                    "3. Hold Alt while clicking any link to keep that download inside the browser.",
                    "4. Right-click any link -> Download with LajuFile to capture any file type.",
                ] {
                    ui.label(RichText::new(step).size(11.0).color(sp.muted));
                    ui.add_space(4.0);
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if figma_button(ui, "Setup Wizard...", 128.0, 32.0, true, sp) {
                        *action = Some(Action::OpenBrowserWizard);
                    }
                    ui.add_space(8.0);
                    if figma_button(ui, "Open extension folder", 156.0, 32.0, false, sp) {
                        *action = Some(Action::OpenExtensionFolder);
                    }
                });
            });
        }
        SettingsTab::Appearance => {
            tab_header(
                ui,
                "Appearance",
                "Customize light and dark interface themes.",
                sp,
            );
            settings_card(ui, sp, |ui| {
                card_kicker(ui, "THEME", sp);
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("Interface theme")
                                .size(12.0)
                                .strong()
                                .color(sp.text),
                        );
                        ui.label(
                            RichText::new("Applies across main window and dialogs.")
                                .size(10.0)
                                .color(sp.faint),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        segmented_theme(ui, &mut settings.theme, sp);
                    });
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    for (name, color) in [
                        ("Window", sp.field_bg),
                        ("Surface", sp.card_bg),
                        ("Accent", sp.accent),
                        ("Success", sp.green),
                    ] {
                        let (swatch, _) =
                            ui.allocate_exact_size(Vec2::new(72.0, 26.0), Sense::hover());
                        ui.painter().rect_filled(swatch, 6.0, color);
                        ui.painter().rect_stroke(
                            swatch,
                            6.0,
                            Stroke::new(1.0, sp.border),
                            egui::StrokeKind::Inside,
                        );
                        ui.painter().text(
                            swatch.center(),
                            Align2::CENTER_CENTER,
                            name,
                            FontId::proportional(9.5),
                            if color == sp.accent {
                                Color32::WHITE
                            } else {
                                sp.muted
                            },
                        );
                        ui.add_space(8.0);
                    }
                });
            });
            ui.add_space(12.0);
            settings_card(ui, sp, |ui| {
                card_kicker(ui, "INTERFACE SCALE", sp);
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Zoom scale")
                            .size(12.0)
                            .strong()
                            .color(sp.text),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        for pct in [150u32, 125, 115, 100, 90] {
                            let active = settings.ui_scale_pct == pct;
                            if figma_button(ui, &format!("{pct}%"), 50.0, 26.0, active, sp) {
                                settings.ui_scale_pct = pct;
                            }
                            ui.add_space(6.0);
                        }
                    });
                });
            });
            ui.add_space(12.0);
            settings_card(ui, sp, |ui| {
                card_kicker(ui, "LANGUAGE", sp);
                ui.add_space(10.0);
                let active_label = if settings.language == "system" {
                    let os_code = crate::i18n::detect_windows_language();
                    let native = crate::i18n::find_language(os_code)
                        .map(|i| i.native_name)
                        .unwrap_or("English");
                    if cfg!(windows) {
                        format!("Windows' language ({native})")
                    } else {
                        format!("System language ({native})")
                    }
                } else {
                    crate::i18n::find_language(&settings.language)
                        .map(|i| format!("{} · {}", i.native_name, i.english_name))
                        .unwrap_or_else(|| settings.language.clone())
                };
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("Interface language")
                                .size(12.0)
                                .strong()
                                .color(sp.text),
                        );
                        ui.label(RichText::new(active_label).size(10.5).color(sp.faint));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if figma_button(ui, "Choose language...", 136.0, 30.0, false, sp) {
                            *action = Some(Action::OpenLanguagePicker);
                        }
                    });
                });
            });
        }
        SettingsTab::About => {
            tab_header(
                ui,
                "About",
                "Video extraction tools and application information.",
                sp,
            );
            draw_video_tools_card(ui, sp, action);
            ui.add_space(12.0);
            settings_card(ui, sp, |ui| {
                card_kicker(ui, "APPLICATION", sp);
                ui.add_space(8.0);
                ui.label(
                    RichText::new(concat!("LajuFile v", env!("CARGO_PKG_VERSION")))
                        .size(13.0)
                        .strong()
                        .color(sp.text),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new(
                        "Segmented multi-connection download manager with browser capture and yt-dlp integration.",
                    )
                    .size(11.0)
                    .color(sp.muted),
                );
            });
        }
    }
}

fn tab_header(ui: &mut Ui, title: &str, subtitle: &str, sp: &SettingsPalette) {
    ui.label(RichText::new(title).size(17.0).strong().color(sp.text));
    ui.add_space(2.0);
    ui.label(RichText::new(subtitle).size(11.0).color(sp.muted));
    ui.add_space(14.0);
}

fn draw_save_location_card(
    ui: &mut Ui,
    settings: &mut Settings,
    sp: &SettingsPalette,
    action: &mut Option<Action>,
) {
    settings_card(ui, sp, |ui| {
        card_kicker(ui, "SAVE LOCATION", sp);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let field_w = (ui.available_width() - 122.0).max(200.0);
            egui::Frame::new()
                .fill(sp.field_bg)
                .stroke(Stroke::new(1.0, sp.border))
                .corner_radius(8)
                .inner_margin(Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_width(field_w - 20.0);
                    ui.set_height(22.0);
                    ui.horizontal_centered(|ui| {
                        let (icon_rect, _) =
                            ui.allocate_exact_size(Vec2::new(12.0, 10.0), Sense::hover());
                        paint_folder_icon(ui.painter(), icon_rect, sp.muted);
                        ui.add_space(8.0);
                        let mut folder = settings.download_dir.display().to_string();
                        if ui
                            .add_sized(
                                [ui.available_width(), 20.0],
                                egui::TextEdit::singleline(&mut folder)
                                    .frame(egui::Frame::NONE)
                                    .background_color(Color32::TRANSPARENT),
                            )
                            .changed()
                        {
                            settings.download_dir = PathBuf::from(folder.trim());
                        }
                    });
                });
            ui.add_space(10.0);
            if figma_button(ui, "Change", 112.0, 34.0, false, sp) {
                *action = Some(Action::OpenFolderPicker);
            }
        });
    });
}

fn draw_category_folders_card(
    ui: &mut Ui,
    settings: &mut Settings,
    sp: &SettingsPalette,
    action: &mut Option<Action>,
) {
    settings_card(ui, sp, |ui| {
        card_kicker(ui, "CATEGORY FOLDERS", sp);
        ui.add_space(8.0);
        for cat in ["Video", "Music", "Documents", "Programs", "Compressed"] {
            let custom = settings.category_dirs.get(cat).cloned();
            ui.horizontal(|ui| {
                ui.set_width(ui.available_width());
                ui.allocate_ui_with_layout(
                    Vec2::new(96.0, 26.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.label(RichText::new(cat).size(11.5).strong().color(sp.text));
                    },
                );
                let display = custom
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "Default save folder".to_string());
                let text_color = if custom.is_some() { sp.text } else { sp.faint };
                let btn_reserve = if custom.is_some() { 132.0 } else { 70.0 };
                let path_w = (ui.available_width() - btn_reserve).max(100.0);
                egui::Frame::new()
                    .fill(sp.field_bg)
                    .stroke(Stroke::new(1.0, sp.border))
                    .corner_radius(6)
                    .inner_margin(Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.set_width(path_w);
                        ui.label(RichText::new(display).size(11.0).color(text_color));
                    });
                ui.add_space(6.0);
                if figma_button(ui, "Pick", 54.0, 26.0, false, sp) {
                    *action = Some(Action::BrowseCategoryDir(cat.to_string()));
                }
                if custom.is_some() {
                    ui.add_space(6.0);
                    if figma_button(ui, "Reset", 54.0, 26.0, false, sp) {
                        settings.category_dirs.remove(cat);
                    }
                }
            });
            ui.add_space(6.0);
        }
    });
}

fn draw_performance_card(ui: &mut Ui, settings: &mut Settings, sp: &SettingsPalette) {
    settings_card(ui, sp, |ui| {
        card_kicker(ui, "PERFORMANCE", sp);
        ui.add_space(10.0);

        setting_row(
            ui,
            "Connections per download",
            "Parallel segments requested from the server.",
            sp,
            |ui| {
                stepper_widget(ui, &mut settings.max_connections, 1, 32, sp);
            },
        );
        ui.add_space(10.0);
        setting_row(
            ui,
            "Simultaneous downloads",
            "Files downloading at the same time.",
            sp,
            |ui| {
                stepper_widget(ui, &mut settings.max_concurrent, 1, 10, sp);
            },
        );
        ui.add_space(10.0);
        setting_row(ui, "Global speed limit", "0 means unlimited.", sp, |ui| {
            egui::Frame::new()
                .fill(sp.field_bg)
                .stroke(Stroke::new(1.0, sp.border))
                .corner_radius(8)
                .inner_margin(Margin::symmetric(10, 5))
                .show(ui, |ui| {
                    ui.set_width(130.0);
                    ui.set_height(20.0);
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut settings.global_speed_limit_kbps)
                                .range(0..=1_000_000)
                                .speed(64),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let hint = if settings.global_speed_limit_kbps == 0 {
                                "Unlimited"
                            } else {
                                "KB/s"
                            };
                            ui.label(RichText::new(hint).size(9.5).color(sp.faint));
                        });
                    });
                });
        });

        ui.add_space(12.0);
        card_kicker(ui, "PROXY URL (HTTP / HTTPS / SOCKS5)", sp);
        ui.add_space(6.0);
        egui::Frame::new()
            .fill(sp.field_bg)
            .stroke(Stroke::new(1.0, sp.border))
            .corner_radius(8)
            .inner_margin(Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_width((ui.available_width() - 22.0).max(160.0));
                ui.add_sized(
                    [ui.available_width(), 20.0],
                    egui::TextEdit::singleline(&mut settings.proxy_url)
                        .frame(egui::Frame::NONE)
                        .background_color(Color32::TRANSPARENT)
                        .hint_text("http://127.0.0.1:7890 or socks5://127.0.0.1:1080"),
                );
            });
        if let Err(err) = lajufile_core::http::validate_proxy_url(&settings.proxy_url) {
            ui.add_space(4.0);
            ui.label(RichText::new(err).size(10.0).color(sp.red));
        }
        ui.add_space(12.0);
        card_kicker(
            ui,
            "HUGGINGFACE ACCESS TOKEN (OPTIONAL, FOR GATED AI MODELS)",
            sp,
        );
        ui.add_space(6.0);
        egui::Frame::new()
            .fill(sp.field_bg)
            .stroke(Stroke::new(1.0, sp.border))
            .corner_radius(8)
            .inner_margin(Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_width((ui.available_width() - 22.0).max(160.0));
                ui.add_sized(
                    [ui.available_width(), 20.0],
                    egui::TextEdit::singleline(&mut settings.hf_token)
                        .password(true)
                        .frame(egui::Frame::NONE)
                        .background_color(Color32::TRANSPARENT)
                        .hint_text("hf_... (sent only to huggingface.co)"),
                );
            });
    });
}

fn draw_browser_capture_card(
    ui: &mut Ui,
    settings: &mut Settings,
    bridge: Option<&BridgeStatus>,
    sp: &SettingsPalette,
    action: &mut Option<Action>,
) {
    settings_card(ui, sp, |ui| {
        let (status_label, dot_color) = match bridge {
            Some(status) if status.state == BridgeState::Bound => ("Connected", sp.green),
            Some(status) if status.state == BridgeState::Error => ("Bridge unavailable", sp.red),
            _ => ("Starting bridge", sp.amber),
        };
        let bound_port = bridge
            .filter(|status| status.port != 0)
            .map(|status| status.port)
            .unwrap_or(settings.bridge_port);

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                    ui.painter()
                        .circle_filled(dot_rect.center(), 3.5, dot_color);
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(status_label)
                            .size(12.0)
                            .strong()
                            .color(dot_color),
                    );
                });
                ui.add_space(2.0);
                ui.label(
                    RichText::new(format!("Browser extension · 127.0.0.1:{bound_port}"))
                        .size(10.5)
                        .color(sp.muted),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if figma_button(ui, "Test connection", 118.0, 30.0, false, sp) {
                    *action = Some(Action::TestBridge(bound_port));
                }
                ui.add_space(8.0);
                let pair_label = if settings.browser_paired {
                    "Extension paired"
                } else {
                    "Setup Wizard"
                };
                if figma_button(ui, pair_label, 118.0, 30.0, !settings.browser_paired, sp) {
                    *action = Some(Action::OpenBrowserWizard);
                }
            });
        });

        ui.add_space(10.0);
        setting_row(
            ui,
            "Bridge port",
            "Port changes apply after restart.",
            sp,
            |ui| {
                egui::Frame::new()
                    .fill(sp.field_bg)
                    .stroke(Stroke::new(1.0, sp.border))
                    .corner_radius(8)
                    .inner_margin(Margin::symmetric(10, 4))
                    .show(ui, |ui| {
                        ui.set_width(88.0);
                        ui.set_height(20.0);
                        ui.add(
                            egui::DragValue::new(&mut settings.bridge_port)
                                .range(1024..=65_535)
                                .speed(1),
                        );
                    });
            },
        );

        ui.add_space(12.0);
        card_kicker(ui, "ACCESS TOKEN", sp);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let field_w = (ui.available_width() - 310.0).max(120.0);
            let display = if settings.bridge_token.len() >= 8 {
                format!(
                    "lf_••••••••••••••••••••{}",
                    &settings.bridge_token[settings.bridge_token.len() - 4..]
                )
            } else {
                mask_token(&settings.bridge_token)
            };
            egui::Frame::new()
                .fill(sp.field_bg)
                .stroke(Stroke::new(1.0, sp.border))
                .corner_radius(8)
                .inner_margin(Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_width(field_w);
                    ui.set_height(20.0);
                    ui.horizontal_centered(|ui| {
                        ui.label(RichText::new(display).monospace().size(10.5).color(sp.text));
                    });
                });
            ui.add_space(8.0);
            if figma_button(ui, "Copy", 58.0, 34.0, false, sp) {
                ui.ctx().copy_text(settings.bridge_token.clone());
            }
            ui.add_space(8.0);
            if figma_button(ui, "Quick-Pair", 92.0, 34.0, false, sp) {
                ui.ctx().copy_text(quick_pair_payload(settings, bridge));
            }
            ui.add_space(8.0);
            if figma_button(ui, "Regenerate", 104.0, 34.0, false, sp) {
                *action = Some(Action::RegenerateToken);
            }
        });
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "Paste this port and token into the LajuFile browser extension options (Chrome, Brave, Edge).",
            )
            .size(10.0)
            .color(sp.faint),
        );

        ui.add_space(12.0);
        card_kicker(ui, "CAPTURED EXTENSIONS (CLIPBOARD & BROWSER)", sp);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let field_w = (ui.available_width() - 96.0).max(160.0);
            egui::Frame::new()
                .fill(sp.field_bg)
                .stroke(Stroke::new(1.0, sp.border))
                .corner_radius(8)
                .inner_margin(Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_width(field_w);
                    ui.add_sized(
                        [ui.available_width(), 20.0],
                        egui::TextEdit::singleline(&mut settings.captured_extensions)
                            .frame(egui::Frame::NONE)
                            .background_color(Color32::TRANSPARENT)
                            .hint_text(DEFAULT_CAPTURE_EXTENSIONS),
                    );
                });
            ui.add_space(8.0);
            if figma_button(ui, "Reset", 64.0, 32.0, false, sp) {
                settings.captured_extensions = DEFAULT_CAPTURE_EXTENSIONS.to_string();
            }
        });

        ui.add_space(12.0);
        card_kicker(ui, "EXCLUDED DOMAINS", sp);
        ui.add_space(6.0);
        egui::Frame::new()
            .fill(sp.field_bg)
            .stroke(Stroke::new(1.0, sp.border))
            .corner_radius(8)
            .inner_margin(Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_width((ui.available_width() - 22.0).max(160.0));
                ui.add_sized(
                    [ui.available_width(), 20.0],
                    egui::TextEdit::singleline(&mut settings.excluded_domains)
                        .frame(egui::Frame::NONE)
                        .background_color(Color32::TRANSPARENT)
                        .hint_text("example.com, *.internal.net"),
                );
            });
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Domains skipped by clipboard monitoring and automatic browser capture (comma or space separated).",
            )
            .size(10.0)
            .color(sp.faint),
        );
    });
}

fn draw_behavior_card(ui: &mut Ui, settings: &mut Settings, sp: &SettingsPalette) {
    settings_card(ui, sp, |ui| {
        card_kicker(ui, "BEHAVIOR", sp);
        ui.add_space(10.0);

        toggle_row(
            ui,
            "Close to tray",
            "settings_tray",
            &mut settings.close_to_tray,
            sp,
        );
        ui.add_space(8.0);
        toggle_row(
            ui,
            if cfg!(windows) {
                "Start LajuFile with Windows"
            } else {
                "Start LajuFile at login"
            },
            "settings_autostart",
            &mut settings.autostart,
            sp,
        );
        ui.add_space(8.0);
        toggle_row(
            ui,
            "Auto-resume on startup",
            "settings_autoresume",
            &mut settings.auto_resume,
            sp,
        );
        ui.add_space(8.0);
        toggle_row(
            ui,
            "Check for updates on startup",
            "settings_auto_updates",
            &mut settings.auto_check_updates,
            sp,
        );
        ui.add_space(8.0);
        toggle_row(
            ui,
            "Monitor clipboard for download links (Ctrl+C)",
            "settings_clipboard",
            &mut settings.clipboard_monitor,
            sp,
        );
        ui.add_space(8.0);
        toggle_row(
            ui,
            "Ask where to save each download",
            "settings_ask_folder",
            &mut settings.ask_where_to_save,
            sp,
        );
        ui.add_space(8.0);
        toggle_row(
            ui,
            "Show progress window when download starts",
            "settings_progress_start",
            &mut settings.show_progress_on_start,
            sp,
        );
        ui.add_space(8.0);
        toggle_row(
            ui,
            "Show dialog when download completes",
            "settings_complete_dialog",
            &mut settings.show_complete_dialog,
            sp,
        );
        ui.add_space(8.0);
        toggle_row(
            ui,
            "Download video subtitles when available",
            "settings_video_subs",
            &mut settings.video_subtitles,
            sp,
        );
        ui.add_space(8.0);
        toggle_row(
            ui,
            "Embed video chapters (yt-dlp)",
            "settings_video_chapters",
            &mut settings.video_chapters,
            sp,
        );
        ui.add_space(8.0);
        toggle_row(
            ui,
            "Auto-extract downloaded archives (.zip, .tar.gz) into subfolder",
            "settings_auto_extract",
            &mut settings.auto_extract_archives,
            sp,
        );
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Duplicate URLs").size(12.0).color(sp.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                segmented_duplicate_policy(ui, &mut settings.duplicate_policy, sp);
            });
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Theme").size(12.0).color(sp.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                segmented_theme(ui, &mut settings.theme, sp);
            });
        });
        ui.add_space(12.0);
        draw_h_divider(ui, sp.border);
        ui.add_space(10.0);
        card_kicker(ui, "SCHEDULER & QUEUE COMPLETION", sp);
        ui.add_space(8.0);
        toggle_row(
            ui,
            "Download only during scheduled hours (local time)",
            "settings_scheduler",
            &mut settings.schedule_enabled,
            sp,
        );
        if settings.schedule_enabled {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Start (HH:MM)").size(11.0).color(sp.muted));
                ui.add_space(6.0);
                egui::Frame::new()
                    .fill(sp.field_bg)
                    .stroke(Stroke::new(1.0, sp.border))
                    .corner_radius(6)
                    .inner_margin(Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.set_width(64.0);
                        ui.add(
                            egui::TextEdit::singleline(&mut settings.schedule_start)
                                .frame(egui::Frame::NONE)
                                .hint_text("00:00"),
                        );
                    });
                ui.add_space(14.0);
                ui.label(RichText::new("Stop (HH:MM)").size(11.0).color(sp.muted));
                ui.add_space(6.0);
                egui::Frame::new()
                    .fill(sp.field_bg)
                    .stroke(Stroke::new(1.0, sp.border))
                    .corner_radius(6)
                    .inner_margin(Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.set_width(64.0);
                        ui.add(
                            egui::TextEdit::singleline(&mut settings.schedule_stop)
                                .frame(egui::Frame::NONE)
                                .hint_text("06:00"),
                        );
                    });
            });
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("When all downloads finish (one-shot)")
                    .size(12.0)
                    .color(sp.text),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                segmented_options(
                    ui,
                    &mut settings.on_queue_complete,
                    "queue_complete_action",
                    &[
                        (
                            lajufile_core::settings::QueueCompleteAction::None,
                            "Off",
                            44.0,
                        ),
                        (
                            lajufile_core::settings::QueueCompleteAction::ExitApp,
                            "Exit",
                            48.0,
                        ),
                        (
                            lajufile_core::settings::QueueCompleteAction::Sleep,
                            "Sleep",
                            54.0,
                        ),
                        (
                            lajufile_core::settings::QueueCompleteAction::Shutdown,
                            "Shutdown",
                            74.0,
                        ),
                    ],
                    sp,
                );
            });
        });
    });
}

fn draw_video_tools_card(ui: &mut Ui, sp: &SettingsPalette, action: &mut Option<Action>) {
    let tools_dir = crate::tools::tools_dir();
    let ready = crate::tools::tools_ready(&tools_dir);
    settings_card(ui, sp, |ui| {
        card_kicker(ui, "VIDEO TOOLS", sp);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                    let color = if ready { sp.green } else { sp.amber };
                    ui.painter().circle_filled(dot_rect.center(), 3.5, color);
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(format!(
                            "yt-dlp and ffmpeg: {}",
                            if ready { "Installed" } else { "Not installed" }
                        ))
                        .size(12.0)
                        .strong()
                        .color(sp.text),
                    );
                });
                ui.add_space(2.0);
                ui.label(
                    RichText::new(tools_dir.display().to_string())
                        .size(10.0)
                        .color(sp.faint),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if figma_button(ui, "Install / Update", 124.0, 32.0, false, sp) {
                    *action = Some(Action::UpdateTools);
                }
            });
        });
    });
}

fn segmented_theme(ui: &mut Ui, theme: &mut Theme, sp: &SettingsPalette) {
    segmented_options(
        ui,
        theme,
        "lajufile_theme_seg",
        &[(Theme::Light, "Light", 64.0), (Theme::Dark, "Dark", 64.0)],
        sp,
    );
}

fn segmented_duplicate_policy(ui: &mut Ui, policy: &mut DuplicatePolicy, sp: &SettingsPalette) {
    segmented_options(
        ui,
        policy,
        "lajufile_dup_seg",
        &[
            (DuplicatePolicy::Reject, "Skip", 52.0),
            (DuplicatePolicy::ResumeExisting, "Resume", 62.0),
            (DuplicatePolicy::RenameNumbered, "Rename (#)", 76.0),
        ],
        sp,
    );
}

fn settings_nav_item(
    ui: &mut Ui,
    tab: SettingsTab,
    label: &str,
    selected: bool,
    status_dot: Option<Color32>,
    sp: &SettingsPalette,
) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 36.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let fill = if selected {
        sp.nav_selected
    } else if response.hovered() {
        sp.border.gamma_multiply(0.35)
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 8.0, fill);
    if selected {
        ui.painter().rect_filled(
            Rect::from_min_size(
                egui::pos2(rect.left() - 10.0, rect.top() + 8.0),
                Vec2::new(3.0, 20.0),
            ),
            2.0,
            sp.accent,
        );
    }
    let icon_rect = Rect::from_center_size(
        egui::pos2(rect.left() + 14.0, rect.center().y),
        Vec2::splat(11.0),
    );
    let fg = if selected { sp.accent } else { sp.muted };
    paint_settings_nav_icon(ui.painter(), icon_rect, tab, fg);
    ui.painter().text(
        egui::pos2(rect.left() + 26.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(11.5),
        fg,
    );
    if let Some(dot) = status_dot {
        ui.painter()
            .circle_filled(egui::pos2(rect.right() - 10.0, rect.center().y), 3.0, dot);
    }
    response.clicked()
}

fn paint_settings_nav_icon(painter: &egui::Painter, rect: Rect, tab: SettingsTab, color: Color32) {
    let c = rect.center();
    let pt = |x: f32, y: f32| {
        egui::pos2(
            rect.left() + (x / 12.0) * rect.width(),
            rect.top() + (y / 12.0) * rect.height(),
        )
    };
    match tab {
        SettingsTab::General => {
            painter.circle_stroke(c, 2.3, Stroke::new(1.2, color));
            for step in 0..6 {
                let angle = step as f32 * (std::f32::consts::TAU / 6.0);
                let dir = Vec2::new(angle.cos(), angle.sin());
                painter.line_segment([c + dir * 2.7, c + dir * 4.5], Stroke::new(1.2, color));
            }
        }
        SettingsTab::Downloads => {
            painter.line_segment([pt(6.0, 1.5), pt(6.0, 8.0)], Stroke::new(1.3, color));
            painter.line(
                vec![pt(3.5, 5.5), pt(6.0, 8.0), pt(8.5, 5.5)],
                Stroke::new(1.3, color),
            );
            painter.line_segment([pt(2.0, 10.2), pt(10.0, 10.2)], Stroke::new(1.3, color));
        }
        SettingsTab::BrowserCapture => {
            painter.circle_stroke(c, 4.5, Stroke::new(1.2, color));
            painter.line_segment([pt(1.6, 6.0), pt(10.4, 6.0)], Stroke::new(1.0, color));
            painter.line_segment([pt(6.0, 1.6), pt(6.0, 10.4)], Stroke::new(1.0, color));
        }
        SettingsTab::Appearance => {
            painter.circle_stroke(c, 4.3, Stroke::new(1.2, color));
            painter.circle_filled(egui::pos2(c.x - 1.3, c.y), 1.6, color);
        }
        SettingsTab::About => {
            painter.circle_stroke(c, 4.5, Stroke::new(1.2, color));
            painter.circle_filled(pt(6.0, 3.8), 0.8, color);
            painter.line_segment([pt(6.0, 5.5), pt(6.0, 9.0)], Stroke::new(1.3, color));
        }
    }
}
