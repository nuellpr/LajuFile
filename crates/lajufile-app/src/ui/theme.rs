use lajufile_core::settings::Theme;

pub fn apply_theme(ctx: &egui::Context, theme: &Theme) {
    let mut visuals = match theme {
        Theme::Light => egui::Visuals::light(),
        Theme::Dark => egui::Visuals::dark(),
    };

    let (window, panel, accent, text, muted, border) = match theme {
        Theme::Light => (
            egui::Color32::from_rgb(0xF7, 0xF8, 0xFA),
            egui::Color32::WHITE,
            egui::Color32::from_rgb(0x0B, 0x6B, 0xCB),
            egui::Color32::from_rgb(0x18, 0x21, 0x2B),
            egui::Color32::from_rgb(0x66, 0x70, 0x85),
            egui::Color32::from_rgb(0xE1, 0xE7, 0xEF),
        ),
        Theme::Dark => (
            egui::Color32::from_rgb(0x0E, 0x13, 0x19),
            egui::Color32::from_rgb(0x14, 0x1A, 0x21),
            egui::Color32::from_rgb(0x4C, 0x9A, 0xFF),
            egui::Color32::from_rgb(0xE7, 0xED, 0xF5),
            egui::Color32::from_rgb(0x94, 0xA3, 0xB8),
            egui::Color32::from_rgb(0x26, 0x30, 0x3B),
        ),
    };

    visuals.panel_fill = window;
    visuals.window_fill = panel;
    visuals.extreme_bg_color = window;
    visuals.faint_bg_color = panel;
    visuals.override_text_color = Some(text);
    visuals.window_stroke = egui::Stroke::new(1.0, border);
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, border);
    visuals.widgets.inactive.bg_fill = panel;
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, muted);
    visuals.widgets.hovered.bg_fill = accent.gamma_multiply(0.18);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, text);
    visuals.widgets.active.bg_fill = accent;
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, text);
    visuals.selection.bg_fill = accent.gamma_multiply(0.35);
    visuals.selection.stroke = egui::Stroke::new(1.0, accent);
    visuals.hyperlink_color = accent;
    ctx.set_visuals(visuals);
}
