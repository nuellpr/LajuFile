use egui::{
    Align, Align2, Color32, FontId, Layout, Margin, Rect, RichText, Sense, Stroke, Ui, Vec2,
};
use lajufile_core::settings::{Settings, Theme};

use crate::bridge::{BridgeState, BridgeStatus};

use super::super::Action;
use super::{
    SettingsPalette, badge_pill, card_kicker, draw_h_divider, figma_button, mask_token,
    modal_header, settings_card, toggle_row_interactive,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserKind {
    Chrome,
    Edge,
    Brave,
    OperaVivaldi,
    Firefox,
}

pub(super) fn quick_pair_payload(settings: &Settings) -> String {
    serde_json::json!({
        "port": settings.bridge_port,
        "token": settings.bridge_token,
    })
    .to_string()
}

fn paint_browser_emblem(ui: &mut Ui, id: &str, sp: &SettingsPalette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
    let (bg, fg, glyph) = match id {
        "chrome" => (Color32::from_rgb(0x1D, 0x4E, 0xD8), Color32::WHITE, "C"),
        "edge" => (Color32::from_rgb(0x02, 0x84, 0xC7), Color32::WHITE, "E"),
        "brave" => (Color32::from_rgb(0xEA, 0x58, 0x0C), Color32::WHITE, "B"),
        "opera" | "vivaldi" => (Color32::from_rgb(0xDC, 0x26, 0x26), Color32::WHITE, "O"),
        "firefox" => (Color32::from_rgb(0xEA, 0x58, 0x0C), Color32::WHITE, "F"),
        _ => (sp.nav_bg, sp.text, "W"),
    };
    ui.painter().circle_filled(rect.center(), 14.0, bg);
    ui.painter().circle_stroke(
        rect.center(),
        14.0,
        Stroke::new(1.0, sp.border.gamma_multiply(0.8)),
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(13.0),
        fg,
    );
}

fn paint_capture_hero_badge(ui: &mut Ui, sp: &SettingsPalette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(48.0), Sense::hover());
    let c = rect.center();
    ui.painter()
        .circle_filled(c, 22.0, sp.accent.gamma_multiply(0.16));
    ui.painter()
        .circle_stroke(c, 22.0, Stroke::new(1.5, sp.accent.gamma_multiply(0.65)));
    ui.painter()
        .circle_stroke(c, 13.0, Stroke::new(1.4, sp.accent));
    ui.painter().line_segment(
        [egui::pos2(c.x - 13.0, c.y), egui::pos2(c.x + 13.0, c.y)],
        Stroke::new(1.1, sp.accent.gamma_multiply(0.75)),
    );
    ui.painter().line_segment(
        [egui::pos2(c.x, c.y - 13.0), egui::pos2(c.x, c.y + 13.0)],
        Stroke::new(1.1, sp.accent.gamma_multiply(0.75)),
    );
    let badge_center = egui::pos2(rect.right() - 9.0, rect.bottom() - 9.0);
    ui.painter().circle_filled(badge_center, 8.5, sp.green);
    ui.painter().line(
        vec![
            egui::pos2(badge_center.x, badge_center.y - 4.0),
            egui::pos2(badge_center.x, badge_center.y + 3.0),
        ],
        Stroke::new(1.8, Color32::WHITE),
    );
    ui.painter().line(
        vec![
            egui::pos2(badge_center.x - 3.0, badge_center.y + 0.5),
            egui::pos2(badge_center.x, badge_center.y + 3.5),
            egui::pos2(badge_center.x + 3.0, badge_center.y + 0.5),
        ],
        Stroke::new(1.8, Color32::WHITE),
    );
}

pub fn browser_setup_dialog(
    ctx: &egui::Context,
    open: &mut bool,
    settings: &mut Settings,
    bridge: Option<&BridgeStatus>,
) -> Option<Action> {
    let sp = SettingsPalette::from_dark(matches!(settings.theme, Theme::Dark));
    let browser_id = egui::Id::unique("lajufile_browser_wizard_kind");
    let manual_id = egui::Id::unique("lajufile_browser_wizard_manual");
    let detected_id = egui::Id::unique("lajufile_detected_browsers");

    let mut selected_browser = ctx.data_mut(|d| {
        d.get_temp::<BrowserKind>(browser_id)
            .unwrap_or(BrowserKind::Chrome)
    });
    let mut manual_mode = ctx.data_mut(|d| d.get_temp::<bool>(manual_id).unwrap_or(false));
    let detected = ctx.data_mut(|d| {
        if let Some(list) = d.get_temp::<Vec<crate::tools::DetectedBrowser>>(detected_id) {
            list
        } else {
            let list = crate::tools::detect_installed_browsers();
            d.insert_temp(detected_id, list.clone());
            list
        }
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

    egui::Window::new("Add LajuFile to your browser")
        .id(egui::Id::unique("lajufile_browser_setup_wizard"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .frame(frame)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_width(580.0);
            ui.spacing_mut().item_spacing = Vec2::ZERO;

            if modal_header(
                ui,
                &crate::i18n::t_ctx(ctx, "Add LajuFile to your browser"),
                "Browser capture, PDF/media links, and YouTube download panel.",
                &sp,
            ) {
                *open = false;
            }
            draw_h_divider(ui, sp.border);

            egui::Frame::new()
                .fill(sp.dialog_bg)
                .inner_margin(Margin::symmetric(24, 18))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());

                    // Hero row (matching screenshot hierarchy with sharper dark/light card polish)
                    settings_card(ui, &sp, |ui| {
                        ui.horizontal(|ui| {
                            paint_capture_hero_badge(ui, &sp);
                            ui.add_space(14.0);
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(crate::i18n::t_ctx(
                                            ctx,
                                            "Catch downloads from your browser",
                                        ))
                                        .size(15.0)
                                        .strong()
                                        .color(sp.text),
                                    );
                                    ui.add_space(8.0);
                                    if settings.browser_paired {
                                        badge_pill(ui, "Connected", sp.green, &sp);
                                    } else if bridge.is_some_and(|s| s.state == BridgeState::Error) {
                                        badge_pill(ui, "Bridge offline", sp.red, &sp);
                                    } else {
                                        badge_pill(ui, "Not connected", sp.amber, &sp);
                                    }
                                });
                                ui.add_space(3.0);
                                ui.label(
                                    RichText::new(
                                        "Add the extension: files you download in your browser come here, PDFs/media are captured, and videos get a Download button.",
                                    )
                                    .size(11.0)
                                    .color(sp.muted),
                                );
                            });
                        });
                    });

                    ui.add_space(12.0);

                    if !manual_mode {
                        settings_card(ui, &sp, |ui| {
                            card_kicker(ui, "DETECTED BROWSERS ON THIS PC", &sp);
                            ui.add_space(8.0);
                            for (idx, browser) in detected.iter().enumerate() {
                                if idx > 0 {
                                    ui.add_space(8.0);
                                }
                                egui::Frame::new()
                                    .fill(sp.field_bg)
                                    .stroke(Stroke::new(1.0, sp.border))
                                    .corner_radius(9)
                                    .inner_margin(Margin::symmetric(12, 9))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            paint_browser_emblem(ui, browser.id, &sp);
                                            ui.add_space(10.0);
                                            ui.vertical(|ui| {
                                                ui.label(
                                                    RichText::new(browser.name)
                                                        .size(13.0)
                                                        .strong()
                                                        .color(sp.text),
                                                );
                                                let subtitle = if browser.is_firefox {
                                                    "Requires signed AMO add-on (Firefox Release)"
                                                } else if browser.executable.is_some() {
                                                    "Chromium MV3 · Ready for Quick-Pair"
                                                } else {
                                                    "Chromium MV3 compatible"
                                                };
                                                ui.label(
                                                    RichText::new(subtitle)
                                                        .size(10.0)
                                                        .color(sp.faint),
                                                );
                                            });
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    let primary = idx == 0 && !browser.is_firefox;
                                                    if figma_button(
                                                        ui,
                                                        "Add extension",
                                                        118.0,
                                                        31.0,
                                                        primary,
                                                        &sp,
                                                    ) {
                                                        selected_browser = match browser.id {
                                                            "edge" => BrowserKind::Edge,
                                                            "brave" => BrowserKind::Brave,
                                                            "opera" | "vivaldi" => {
                                                                BrowserKind::OperaVivaldi
                                                            }
                                                            "firefox" => BrowserKind::Firefox,
                                                            _ => BrowserKind::Chrome,
                                                        };
                                                        if !browser.is_firefox {
                                                            ui.ctx().copy_text(
                                                                quick_pair_payload(settings),
                                                            );
                                                            action = Some(Action::LaunchBrowser {
                                                                browser_name: browser
                                                                    .name
                                                                    .to_string(),
                                                                extensions_url: browser
                                                                    .extensions_url
                                                                    .to_string(),
                                                            });
                                                        }
                                                        manual_mode = true;
                                                    }
                                                },
                                            );
                                        });
                                    });
                            }
                            ui.add_space(10.0);
                            let note = if settings.browser_paired {
                                "Once added and paired, downloads flow straight to LajuFile automatically."
                            } else {
                                "Once it is added, this window updates automatically. Note: installing without Developer Mode requires Store publication (Chrome Web Store / Edge Add-ons / AMO); until then, Add extension copies Quick-Pair and opens install steps."
                            };
                            ui.label(RichText::new(note).size(10.5).color(sp.faint));
                        });
                    } else {
                        ui.horizontal(|ui| {
                            for (kind, label, width) in [
                                (BrowserKind::Chrome, "Chrome", 74.0),
                                (BrowserKind::Edge, "Edge", 64.0),
                                (BrowserKind::Brave, "Brave", 68.0),
                                (BrowserKind::OperaVivaldi, "Opera / Vivaldi", 110.0),
                                (BrowserKind::Firefox, "Firefox", 74.0),
                            ] {
                                if figma_button(
                                    ui,
                                    label,
                                    width,
                                    28.0,
                                    selected_browser == kind,
                                    &sp,
                                ) {
                                    selected_browser = kind;
                                }
                                ui.add_space(6.0);
                            }
                        });
                        ui.add_space(10.0);

                        if selected_browser == BrowserKind::Firefox {
                            settings_card(ui, &sp, |ui| {
                                card_kicker(ui, "FIREFOX RELEASE LIMITATION", &sp);
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(
                                        "Release Firefox refuses permanent unsigned local add-ons unless signed via Mozilla Add-ons (AMO). Run extension/package.ps1 to generate lajufile-extension-firefox.zip for AMO upload, or use Chrome/Edge/Brave/Opera/Vivaldi locally right now.",
                                    )
                                    .size(11.0)
                                    .color(sp.muted),
                                );
                            });
                        } else {
                            let ext_page_url = match selected_browser {
                                BrowserKind::Edge => "edge://extensions",
                                BrowserKind::Brave => "brave://extensions",
                                BrowserKind::OperaVivaldi => "opera://extensions",
                                BrowserKind::Chrome | BrowserKind::Firefox => "chrome://extensions",
                            };
                            let ext_folder = crate::tools::extension_dir()
                                .map(|p| p.display().to_string())
                                .unwrap_or_else(|| "extension".to_string());

                            settings_card(ui, &sp, |ui| {
                                card_kicker(
                                    ui,
                                    "1. OPEN EXTENSIONS & ENABLE DEVELOPER MODE (UNTIL STORE PUBLISH)",
                                    &sp,
                                );
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    egui::Frame::new()
                                        .fill(sp.field_bg)
                                        .stroke(Stroke::new(1.0, sp.border))
                                        .corner_radius(8)
                                        .inner_margin(Margin::symmetric(10, 6))
                                        .show(ui, |ui| {
                                            ui.set_width(
                                                (ui.available_width() - 110.0).max(150.0),
                                            );
                                            ui.label(
                                                RichText::new(ext_page_url)
                                                    .monospace()
                                                    .size(11.0)
                                                    .color(sp.text),
                                            );
                                        });
                                    ui.add_space(8.0);
                                    if figma_button(ui, "Copy URL", 86.0, 30.0, false, &sp) {
                                        ui.ctx().copy_text(ext_page_url.to_string());
                                    }
                                });

                                ui.add_space(10.0);
                                card_kicker(ui, "2. CLICK 'LOAD UNPACKED' & PICK EXTENSION FOLDER", &sp);
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    egui::Frame::new()
                                        .fill(sp.field_bg)
                                        .stroke(Stroke::new(1.0, sp.border))
                                        .corner_radius(8)
                                        .inner_margin(Margin::symmetric(10, 6))
                                        .show(ui, |ui| {
                                            ui.set_width(
                                                (ui.available_width() - 196.0).max(110.0),
                                            );
                                            ui.label(
                                                RichText::new(&ext_folder)
                                                    .monospace()
                                                    .size(10.0)
                                                    .color(sp.text),
                                            );
                                        });
                                    ui.add_space(8.0);
                                    if figma_button(ui, "Copy path", 80.0, 30.0, false, &sp) {
                                        ui.ctx().copy_text(ext_folder.clone());
                                    }
                                    ui.add_space(8.0);
                                    if figma_button(ui, "Open folder", 92.0, 30.0, false, &sp) {
                                        action = Some(Action::OpenExtensionFolder);
                                    }
                                });

                                ui.add_space(10.0);
                                card_kicker(ui, "3. PASTE QUICK-PAIR IN POPUP OR OPTIONS", &sp);
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "Port {} · Token {}",
                                            settings.bridge_port,
                                            mask_token(&settings.bridge_token)
                                        ))
                                        .monospace()
                                        .size(10.5)
                                        .color(sp.muted),
                                    );
                                    ui.with_layout(
                                        Layout::right_to_left(Align::Center),
                                        |ui| {
                                            if figma_button(
                                                ui,
                                                "Copy Quick-Pair",
                                                122.0,
                                                30.0,
                                                true,
                                                &sp,
                                            ) {
                                                ui.ctx().copy_text(quick_pair_payload(settings));
                                            }
                                        },
                                    );
                                });
                            });
                        }

                        ui.add_space(10.0);
                        settings_card(ui, &sp, |ui| {
                            let mut three_step_flow = settings.ask_where_to_save
                                && settings.show_progress_on_start
                                && settings.show_complete_dialog;
                            if toggle_row_interactive(
                                ui,
                                "Classic 3-step popup flow (File Info -> Progress -> Complete)",
                                "wizard_3step_flow",
                                &mut three_step_flow,
                                &sp,
                            ) {
                                settings.ask_where_to_save = three_step_flow;
                                settings.show_progress_on_start = three_step_flow;
                                settings.show_complete_dialog = true;
                                action = Some(Action::SaveSettings(settings.clone()));
                            }
                        });
                    }
                });

            draw_h_divider(ui, sp.border);
            egui::Frame::new()
                .fill(sp.dialog_bg)
                .inner_margin(Margin::symmetric(24, 12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let toggle_label = if manual_mode {
                            "<- Back to browsers"
                        } else {
                            "Install it by hand"
                        };
                        let resp = ui
                            .add(
                                egui::Label::new(
                                    RichText::new(toggle_label).size(12.0).color(sp.accent),
                                )
                                .sense(Sense::click()),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if resp.clicked() {
                            manual_mode = !manual_mode;
                        }

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if figma_button(
                                ui,
                                &crate::i18n::t_ctx(ctx, "Close"),
                                84.0,
                                32.0,
                                false,
                                &sp,
                            ) {
                                *open = false;
                            }
                            ui.add_space(8.0);
                            if figma_button(ui, "Don't ask again", 122.0, 32.0, false, &sp) {
                                settings.suppress_browser_wizard = true;
                                action = Some(Action::SaveSettings(settings.clone()));
                                *open = false;
                            }
                        });
                    });
                });
        });

    ctx.data_mut(|d| {
        d.insert_temp(browser_id, selected_browser);
        d.insert_temp(manual_id, manual_mode);
    });
    action
}

pub fn language_dialog(
    ctx: &egui::Context,
    open: &mut bool,
    current_setting: &str,
    dark: bool,
) -> Option<Action> {
    let sp = SettingsPalette::from_dark(dark);
    let search_id = egui::Id::unique("lajufile_lang_search");
    let sel_id = egui::Id::unique("lajufile_lang_selected");
    let mut query = ctx.data_mut(|d| d.get_temp::<String>(search_id).unwrap_or_default());
    let mut selected = ctx.data_mut(|d| {
        d.get_temp::<String>(sel_id)
            .unwrap_or_else(|| current_setting.to_string())
    });
    let mut action = None;

    let style = ctx.style_of(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    let frame = egui::Frame::window(&style)
        .fill(sp.dialog_bg)
        .stroke(Stroke::new(1.0, sp.border))
        .corner_radius(14)
        .inner_margin(Margin::ZERO);

    egui::Window::new("Choose language")
        .id(egui::Id::unique("lajufile_language_picker"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .frame(frame)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_width(540.0);
            ui.spacing_mut().item_spacing = Vec2::ZERO;

            if modal_header(
                ui,
                &crate::i18n::t_ctx(ctx, "Choose language"),
                "Switches the interface immediately without restarting. Supports RTL mirroring & custom i18n/*.tsv catalogs.",
                &sp,
            ) {
                *open = false;
            }
            draw_h_divider(ui, sp.border);

            egui::Frame::new()
                .fill(sp.dialog_bg)
                .inner_margin(Margin::symmetric(24, 16))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::Frame::new()
                        .fill(sp.field_bg)
                        .stroke(Stroke::new(1.0, sp.border))
                        .corner_radius(8)
                        .inner_margin(Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_width((ui.available_width() - 20.0).max(180.0));
                            ui.add_sized(
                                [ui.available_width(), 22.0],
                                egui::TextEdit::singleline(&mut query)
                                    .frame(egui::Frame::NONE)
                                    .background_color(Color32::TRANSPARENT)
                                    .hint_text("Search language (name, native name, or code)..."),
                            );
                        });

                    ui.add_space(10.0);
                    let q = query.trim().to_ascii_lowercase();
                    let os_code = crate::i18n::detect_windows_language();
                    let os_label = crate::i18n::find_language(os_code)
                        .map(|info| info.native_name)
                        .unwrap_or("English");

                    egui::Frame::new()
                        .fill(sp.card_bg)
                        .stroke(Stroke::new(1.0, sp.border))
                        .corner_radius(10)
                        .inner_margin(Margin::symmetric(6, 6))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            egui::ScrollArea::vertical()
                                .id_salt("lajufile_lang_scroll")
                                .max_height(310.0)
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    if q.is_empty()
                                        || "windows".contains(&q)
                                        || "system".contains(&q)
                                        || "auto".contains(&q)
                                    {
                                        let is_sel = selected == "system";
                                        let (rect, resp) = ui.allocate_exact_size(
                                            Vec2::new(ui.available_width(), 34.0),
                                            Sense::click(),
                                        );
                                        let resp =
                                            resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                                        if is_sel || resp.hovered() {
                                            ui.painter().rect_filled(
                                                rect,
                                                7.0,
                                                if is_sel { sp.nav_selected } else { sp.field_bg },
                                            );
                                        }
                                        ui.painter().text(
                                            egui::pos2(rect.left() + 12.0, rect.center().y),
                                            Align2::LEFT_CENTER,
                                            format!(
                                                "{} ({os_label})",
                                                crate::i18n::t_ctx(ctx, "Windows' language")
                                            ),
                                            FontId::proportional(12.0),
                                            if is_sel { sp.accent } else { sp.text },
                                        );
                                        ui.painter().text(
                                            egui::pos2(rect.right() - 12.0, rect.center().y),
                                            Align2::RIGHT_CENTER,
                                            "Auto",
                                            FontId::proportional(10.5),
                                            sp.muted,
                                        );
                                        if resp.clicked() {
                                            selected = "system".to_string();
                                        }
                                        if resp.double_clicked() {
                                            action = Some(Action::SetLanguage("system".to_string()));
                                            *open = false;
                                        }
                                        ui.add_space(4.0);
                                        draw_h_divider(ui, sp.border);
                                        ui.add_space(4.0);
                                    }

                                    for info in crate::i18n::CATALOG {
                                        if !q.is_empty()
                                            && !info.english_name.to_ascii_lowercase().contains(&q)
                                            && !info.native_name.to_ascii_lowercase().contains(&q)
                                            && !info.code.to_ascii_lowercase().contains(&q)
                                        {
                                            continue;
                                        }
                                        let is_sel = selected.eq_ignore_ascii_case(info.code);
                                        let (rect, resp) = ui.allocate_exact_size(
                                            Vec2::new(ui.available_width(), 32.0),
                                            Sense::click(),
                                        );
                                        let resp =
                                            resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                                        if is_sel || resp.hovered() {
                                            ui.painter().rect_filled(
                                                rect,
                                                7.0,
                                                if is_sel { sp.nav_selected } else { sp.field_bg },
                                            );
                                        }
                                        let badge_rect = Rect::from_center_size(
                                            egui::pos2(rect.left() + 20.0, rect.center().y),
                                            Vec2::new(26.0, 18.0),
                                        );
                                        ui.painter().rect_filled(badge_rect, 4.0, sp.field_bg);
                                        ui.painter().rect_stroke(
                                            badge_rect,
                                            4.0,
                                            Stroke::new(1.0, sp.border),
                                            egui::StrokeKind::Inside,
                                        );
                                        ui.painter().text(
                                            badge_rect.center(),
                                            Align2::CENTER_CENTER,
                                            info.badge,
                                            FontId::proportional(9.0),
                                            sp.muted,
                                        );
                                        ui.painter().text(
                                            egui::pos2(rect.left() + 42.0, rect.center().y),
                                            Align2::LEFT_CENTER,
                                            format!(
                                                "{}  ·  {}",
                                                info.native_name, info.english_name
                                            ),
                                            FontId::proportional(11.5),
                                            if is_sel { sp.accent } else { sp.text },
                                        );
                                        let built_in = crate::i18n::has_builtin_catalog(info.code);
                                        let tag = match (info.rtl, built_in) {
                                            (true, true) => "RTL · Built-in",
                                            (true, false) => "RTL · TSV / EN",
                                            (false, true) => "Built-in",
                                            (false, false) => "TSV / EN",
                                        };
                                        ui.painter().text(
                                            egui::pos2(rect.right() - 10.0, rect.center().y),
                                            Align2::RIGHT_CENTER,
                                            tag,
                                            FontId::proportional(9.5),
                                            sp.faint,
                                        );
                                        if resp.clicked() {
                                            selected = info.code.to_string();
                                        }
                                        if resp.double_clicked() {
                                            action =
                                                Some(Action::SetLanguage(info.code.to_string()));
                                            *open = false;
                                        }
                                    }
                                });
                        });
                });

            draw_h_divider(ui, sp.border);
            egui::Frame::new()
                .fill(sp.dialog_bg)
                .inner_margin(Margin::symmetric(24, 12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if figma_button(
                            ui,
                            &crate::i18n::t_ctx(ctx, "Apply"),
                            92.0,
                            34.0,
                            true,
                            &sp,
                        ) {
                            action = Some(Action::SetLanguage(selected.clone()));
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

    ctx.data_mut(|d| {
        d.insert_temp(search_id, query);
        if *open {
            d.insert_temp(sel_id, selected);
        } else {
            d.remove::<String>(sel_id);
        }
    });
    action
}
