use lajufile_app::app::LajuFileApp;

fn main() -> eframe::Result {
    let icon =
        eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icons/icon-32.png"))
            .expect("valid app icon");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
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
