#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::ExitCode;

use egui::{Align, Color32, CornerRadius, FontId, Layout, Margin, RichText, Stroke, Vec2};
use lajufile_app::installer::{
    InstallOptions, build_setup_executable, default_install_dir, launch_installed_path,
    perform_install, perform_uninstall,
};

const LICENSE_TEXT: &str = include_str!("../../../../LICENSE");

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    License,
    Options,
    Done,
}

struct SetupApp {
    uninstall_mode: bool,
    step: Step,
    accepted: bool,
    install_dir: String,
    desktop_shortcut: bool,
    start_menu_shortcut: bool,
    launch_after: bool,
    error: Option<String>,
}

impl SetupApp {
    fn new(uninstall_mode: bool, install_dir: PathBuf) -> Self {
        Self {
            uninstall_mode,
            step: Step::License,
            accepted: false,
            install_dir: install_dir.display().to_string(),
            desktop_shortcut: true,
            start_menu_shortcut: true,
            launch_after: true,
            error: None,
        }
    }
}

impl eframe::App for SetupApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let bg = Color32::from_rgb(0x0D, 0x11, 0x17);
        let card = Color32::from_rgb(0x16, 0x1B, 0x24);
        let border = Color32::from_rgb(0x28, 0x32, 0x42);
        let accent = Color32::from_rgb(0x3B, 0x82, 0xF6);
        let text = Color32::from_rgb(0xEB, 0xF0, 0xF7);
        let muted = Color32::from_rgb(0x94, 0xA3, 0xB8);

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(bg).inner_margin(Margin::same(22)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("LajuFile Setup")
                            .size(21.0)
                            .strong()
                            .color(text),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                                .size(12.0)
                                .color(accent),
                        );
                    });
                });
                ui.add_space(4.0);
                ui.label(
                    RichText::new(if self.uninstall_mode {
                        "Uninstall LajuFile Download Manager from this computer."
                    } else {
                        "High-speed multi-connection download manager & browser capture suite."
                    })
                    .size(12.0)
                    .color(muted),
                );
                ui.add_space(14.0);

                egui::Frame::new()
                    .fill(card)
                    .stroke(Stroke::new(1.0, border))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(16))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        if self.uninstall_mode {
                            self.draw_uninstall_body(ui, text, muted);
                        } else {
                            match self.step {
                                Step::License => self.draw_license(ui, text, muted),
                                Step::Options => self.draw_options(ui, text, muted),
                                Step::Done => self.draw_done(ui, text, muted),
                            }
                        }
                    });

                if let Some(err) = &self.error {
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(err)
                            .size(11.5)
                            .color(Color32::from_rgb(0xF8, 0x71, 0x71)),
                    );
                }

                ui.with_layout(Layout::bottom_up(Align::RIGHT), |ui| {
                    ui.horizontal(|ui| {
                        if self.uninstall_mode {
                            self.draw_uninstall_buttons(ui, &ctx, accent);
                        } else {
                            self.draw_install_buttons(ui, &ctx, accent);
                        }
                    });
                });
            });
    }
}

impl SetupApp {
    fn draw_license(&mut self, ui: &mut egui::Ui, text: Color32, muted: Color32) {
        ui.label(
            RichText::new("License & Terms and Conditions")
                .size(13.5)
                .strong()
                .color(text),
        );
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .max_height(210.0)
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(LICENSE_TEXT)
                            .font(FontId::monospace(10.5))
                            .color(muted),
                    )
                    .wrap(),
                );
            });
        ui.add_space(10.0);
        ui.checkbox(
            &mut self.accepted,
            "I accept the MIT License and Terms and Conditions",
        );
    }

    fn draw_options(&mut self, ui: &mut egui::Ui, text: Color32, muted: Color32) {
        ui.label(
            RichText::new("Installation Folder & Shortcuts")
                .size(13.5)
                .strong()
                .color(text),
        );
        ui.add_space(8.0);
        ui.label(RichText::new("Destination folder:").size(11.5).color(muted));
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.install_dir)
                .desired_width(f32::INFINITY)
                .margin(Vec2::new(10.0, 8.0)),
        );
        ui.add_space(12.0);
        let app_menu_label = if cfg!(windows) {
            "Create Start Menu shortcut"
        } else {
            "Create Applications Launcher entry"
        };
        ui.checkbox(&mut self.start_menu_shortcut, app_menu_label);
        ui.checkbox(&mut self.desktop_shortcut, "Create Desktop shortcut");
        ui.checkbox(
            &mut self.launch_after,
            "Launch LajuFile after setup finishes",
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new("Includes LajuFile Desktop, Chrome/Edge/Brave/Vivaldi/Opera/Firefox Extension package, and built-in uninstaller.")
                .size(11.0)
                .color(muted),
        );
    }

    fn draw_done(&self, ui: &mut egui::Ui, text: Color32, muted: Color32) {
        ui.label(
            RichText::new("LajuFile has been installed successfully!")
                .size(15.0)
                .strong()
                .color(Color32::from_rgb(0x34, 0xD3, 0x99)),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new(format!("Installed to: {}", self.install_dir))
                .size(11.5)
                .color(text),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("Browser extension files are located inside the extension subfolder and can also be paired directly from inside LajuFile.")
                .size(11.0)
                .color(muted),
        );
    }

    fn draw_uninstall_body(&self, ui: &mut egui::Ui, text: Color32, muted: Color32) {
        if self.step == Step::Done {
            ui.label(
                RichText::new("LajuFile has been uninstalled.")
                    .size(14.5)
                    .strong()
                    .color(text),
            );
            return;
        }
        ui.label(
            RichText::new(format!("Remove LajuFile from:\n{}", self.install_dir))
                .size(12.5)
                .color(text),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new("Desktop/Start Menu shortcuts and Windows Uninstall entries will be removed. Your downloaded files remain untouched.")
                .size(11.0)
                .color(muted),
        );
    }

    fn draw_install_buttons(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, accent: Color32) {
        match self.step {
            Step::License => {
                if ui.button("Cancel").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                ui.add_space(8.0);
                if ui
                    .add_enabled(self.accepted, egui::Button::new("Next ->").fill(accent))
                    .clicked()
                {
                    self.step = Step::Options;
                }
            }
            Step::Options => {
                if ui.button("Cancel").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                ui.add_space(6.0);
                if ui.button("<- Back").clicked() {
                    self.step = Step::License;
                }
                ui.add_space(8.0);
                if ui.add(egui::Button::new("Install").fill(accent)).clicked() {
                    let target = PathBuf::from(self.install_dir.trim());
                    match perform_install(&InstallOptions {
                        install_dir: target,
                        desktop_shortcut: self.desktop_shortcut,
                        start_menu_shortcut: self.start_menu_shortcut,
                    }) {
                        Ok(()) => {
                            self.error = None;
                            self.step = Step::Done;
                        }
                        Err(err) => self.error = Some(err),
                    }
                }
            }
            Step::Done => {
                if ui.add(egui::Button::new("Finish").fill(accent)).clicked() {
                    if self.launch_after {
                        let exe = launch_installed_path(&PathBuf::from(self.install_dir.trim()));
                        let _ = std::process::Command::new(exe).spawn();
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    fn draw_uninstall_buttons(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, accent: Color32) {
        if self.step == Step::Done {
            if ui.add(egui::Button::new("Close").fill(accent)).clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }
        if ui.button("Cancel").clicked() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ui.add_space(8.0);
        if ui
            .add(egui::Button::new("Uninstall").fill(Color32::from_rgb(0xDC, 0x26, 0x26)))
            .clicked()
        {
            match perform_uninstall(&PathBuf::from(self.install_dir.trim())) {
                Ok(()) => {
                    self.error = None;
                    self.step = Step::Done;
                }
                Err(err) => self.error = Some(err),
            }
        }
    }
}

fn parse_arg_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].clone())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--pack") {
        let Some(stub) = parse_arg_value(&args, "--stub") else {
            eprintln!("missing --stub");
            return ExitCode::from(2);
        };
        let Some(app_exe) = parse_arg_value(&args, "--app") else {
            eprintln!("missing --app");
            return ExitCode::from(2);
        };
        let Some(ext_dir) = parse_arg_value(&args, "--ext") else {
            eprintln!("missing --ext");
            return ExitCode::from(2);
        };
        let Some(license) = parse_arg_value(&args, "--license") else {
            eprintln!("missing --license");
            return ExitCode::from(2);
        };
        let Some(readme) = parse_arg_value(&args, "--readme") else {
            eprintln!("missing --readme");
            return ExitCode::from(2);
        };
        let Some(out_exe) = parse_arg_value(&args, "--out") else {
            eprintln!("missing --out");
            return ExitCode::from(2);
        };
        return match build_setup_executable(
            &PathBuf::from(stub),
            &PathBuf::from(app_exe),
            &PathBuf::from(ext_dir),
            &PathBuf::from(license),
            &PathBuf::from(readme),
            &PathBuf::from(out_exe),
        ) {
            Ok(count) => {
                println!("Packed {count} files into installer.");
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("Pack failed: {err}");
                ExitCode::FAILURE
            }
        };
    }

    let uninstall_mode = args.iter().any(|a| a == "--uninstall");
    let silent = args.iter().any(|a| a == "--silent");
    let relaunch = args.iter().any(|a| a == "--relaunch");
    let install_dir = parse_arg_value(&args, "--dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if uninstall_mode
                && let Ok(exe) = std::env::current_exe()
                && let Some(parent) = exe.parent()
            {
                return parent.to_path_buf();
            }
            default_install_dir()
        });

    if silent {
        if relaunch {
            std::thread::sleep(std::time::Duration::from_millis(350));
        }
        let result = if uninstall_mode {
            perform_uninstall(&install_dir)
        } else {
            perform_install(&InstallOptions {
                install_dir: install_dir.clone(),
                desktop_shortcut: !relaunch,
                start_menu_shortcut: true,
            })
        };
        return match result {
            Ok(()) => {
                if relaunch && !uninstall_mode {
                    let exe = launch_installed_path(&install_dir);
                    let _ = std::process::Command::new(exe).spawn();
                }
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("{err}");
                ExitCode::FAILURE
            }
        };
    }

    let icon =
        eframe::icon_data::from_png_bytes(include_bytes!("../../../../assets/icons/icon-32.png"))
            .ok();
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([560.0, 440.0])
        .with_resizable(false);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let title = if uninstall_mode {
        "LajuFile Uninstall"
    } else {
        "LajuFile Setup"
    };
    if eframe::run_native(
        title,
        options,
        Box::new(move |_cc| Ok(Box::new(SetupApp::new(uninstall_mode, install_dir)))),
    )
    .is_err()
    {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
