#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use lajufile_app::app::LajuFileApp;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::System::Console::AttachConsole(
                windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
            );
        }
        let code = match lajufile_app::cli::parse_args(args) {
            Ok(Some(cmd)) => lajufile_app::cli::run_cli(cmd),
            Ok(None) => 0,
            Err(err) => {
                eprintln!("{err}");
                2
            }
        };
        std::process::exit(code);
    }

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
        depth_buffer: 0,
        stencil_buffer: 0,
        multisampling: 0,
        ..Default::default()
    };

    eframe::run_native(
        "LajuFile",
        options,
        Box::new(|cc| Ok(Box::new(LajuFileApp::new(cc)))),
    )
}
