use crate::config::AppConfig;
use eframe::egui;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page { Overview, Farming, Upgrades, Settings, Logs }

pub struct CocbotApp {
    config: AppConfig,
    page: Page,
    logs: Vec<String>,
    running: bool,
}
impl CocbotApp {
    pub fn new(cc: &eframe::CreationContext<'_>, config: AppConfig) -> Self {
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        cc.egui_ctx.set_style(style);
        Self { config, page: Page::Overview, logs: vec!["Cocbot-rs initialised.".into()], running: false }
    }
    fn nav(ui: &mut egui::Ui, current: &mut Page, page: Page, label: &str) {
        if ui.selectable_label(*current == page, label).clicked() { *current = page; }
    }
}
impl eframe::App for CocbotApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Cocbot-rs");
                ui.label(egui::RichText::new("RUST DESKTOP").small().weak());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(if self.running { "RUNNING" } else { "STOPPED" });
                });
            });
        });
        egui::SidePanel::left("nav").resizable(false).default_width(170.0).show(ctx, |ui| {
            ui.add_space(12.0);
            Self::nav(ui, &mut self.page, Page::Overview, "Overview");
            Self::nav(ui, &mut self.page, Page::Farming, "Farming");
            Self::nav(ui, &mut self.page, Page::Upgrades, "Upgrades");
            Self::nav(ui, &mut self.page, Page::Settings, "Settings");
            Self::nav(ui, &mut self.page, Page::Logs, "Logs");
        });
        egui::CentralPanel::default().show(ctx, |ui| match self.page {
            Page::Overview => {
                ui.heading("Village overview");
                ui.label("Live game recognition and status panels will be connected here.");
                ui.separator();
                ui.label("Planned status: builders, storages, laboratory, loot gained and loot/hour.");
                ui.label("Automation target: Google Play Games on Windows.");
            }
            Page::Farming => {
                ui.heading("Farming");
                ui.label("Home Village and Builder Base farming strategies.");
                ui.label("Army selection and deployment controls will be added with the game-control backend.");
                ui.horizontal(|ui| {
                    if ui.button("Start").clicked() { self.running = true; self.logs.push("Start requested; game backend is not implemented yet.".into()); }
                    if ui.button("Stop").clicked() { self.running = false; self.logs.push("Stop requested.".into()); }
                });
            }
            Page::Upgrades => {
                ui.heading("Village upgrades");
                ui.label("Maxer and Rusher upgrade policies, verified purchases, and wall batching.");
                ui.label("Upgrade execution is not implemented yet.");
            }
            Page::Settings => {
                ui.heading("Settings");
                ui.add(egui::Slider::new(&mut self.config.reserve_builders, 0..=5).text("Reserve builders"));
                ui.add(egui::Slider::new(&mut self.config.idle_recheck_seconds, 30..=900).text("Idle recheck interval (seconds)"));
                ui.label("Tesseract executable path");
                ui.text_edit_singleline(&mut self.config.tesseract_path);
                if ui.button("Save settings").clicked() {
                    match self.config.save() {
                        Ok(()) => self.logs.push("Settings saved.".into()),
                        Err(e) => self.logs.push(format!("Settings save failed: {e}")),
                    }
                }
            }
            Page::Logs => {
                ui.heading("Logs");
                egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                    for line in &self.logs { ui.monospace(line); }
                });
            }
        });
    }
}
