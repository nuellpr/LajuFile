pub struct LajuFileApp;

impl LajuFileApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::light());
        Self
    }
}

impl eframe::App for LajuFileApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.heading("LajuFile");
    }
}
