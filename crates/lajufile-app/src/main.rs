#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use lajufile_app::app::LajuFileApp;

fn main() -> eframe::Result {
    let Some(_instance_guard) = lajufile_app::platform::SingleInstanceGuard::acquire() else {
        return Ok(());
    };
    lajufile_app::platform::ensure_writable_cwd();

    let icon =
        eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icons/icon-32.png"))
            .expect("valid app icon");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 640.0])
            .with_position([20.0, 20.0])
            .with_min_inner_size([1024.0, 640.0])
            .with_icon(icon),
        ..Default::default()
    };

    eframe::run_native(
        "LajuFile",
        options,
        Box::new(|cc| Ok(Box::new(LajuFileApp::new(cc)))),
    )
}
