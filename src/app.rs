use crate::{
    config::{AppConfig, LootPriority, Village},
    loot::{LootDelta, LootTracker},
    strategy::ArmyStrategy,
};
use eframe::egui;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Overview,
    Farming,
    Settings,
    Logs,
}

pub struct CocbotApp {
    config: AppConfig,
    page: Page,
    logs: Vec<String>,
    loot: LootTracker,
}

impl CocbotApp {
    pub fn new(cc: &eframe::CreationContext<'_>, config: AppConfig) -> Self {
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        cc.egui_ctx.set_style(style);
        Self {
            config,
            page: Page::Overview,
            logs: vec!["Cocbot-rs initialised. Game automation backend is not connected yet.".into()],
            loot: LootTracker::new(),
        }
    }

    fn nav(ui: &mut egui::Ui, current: &mut Page, page: Page, label: &str) {
        if ui.selectable_label(*current == page, label).clicked() {
            *current = page;
        }
    }

    fn resource_row(ui: &mut egui::Ui, label: &str, total: u64, rate: u64) {
        ui.horizontal(|ui| {
            ui.label(label);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.monospace(format!("{rate}/h"));
                ui.add_space(18.0);
                ui.monospace(total.to_string());
            });
        });
    }

    fn format_loot(ui: &mut egui::Ui, loot: LootDelta) {
        ui.label(format!(
            "Gold: {}    Elixir: {}    Dark elixir: {}",
            loot.gold, loot.elixir, loot.dark_elixir
        ));
    }
}

impl eframe::App for CocbotApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Cocbot-rs");
                ui.label(egui::RichText::new("FARMING AUTOMATION").small().weak());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new("BACKEND NOT CONNECTED").color(egui::Color32::YELLOW));
                });
            });
        });

        egui::SidePanel::left("nav")
            .resizable(false)
            .default_width(170.0)
            .show(ctx, |ui| {
                ui.add_space(12.0);
                Self::nav(ui, &mut self.page, Page::Overview, "Overview");
                Self::nav(ui, &mut self.page, Page::Farming, "Farming");
                Self::nav(ui, &mut self.page, Page::Settings, "Settings");
                Self::nav(ui, &mut self.page, Page::Logs, "Logs");
            });

        egui::CentralPanel::default().show(ctx, |ui| match self.page {
            Page::Overview => {
                ui.heading("Farming overview");
                ui.label("Live village recognition will appear here once game capture is implemented.");
                ui.separator();
                let summary = self.loot.summary();
                ui.label("Session loot");
                Self::resource_row(ui, "Gold", summary.total.gold, summary.per_hour.gold);
                Self::resource_row(ui, "Elixir", summary.total.elixir, summary.per_hour.elixir);
                Self::resource_row(ui, "Dark elixir", summary.total.dark_elixir, summary.per_hour.dark_elixir);
                ui.add_space(12.0);
                ui.label(format!("Elapsed tracking time: {:.0} min", summary.elapsed.as_secs_f64() / 60.0));
                ui.label("Automation target: Clash of Clans in Google Play Games for Windows.");
            }
            Page::Farming => {
                ui.heading("Farming");
                let builder_base = self.config.village == Village::BuilderBase;
                if self.config.strategy.is_builder_base() != builder_base {
                    self.config.strategy = if builder_base { ArmyStrategy::BuilderBaseBabyDragon } else { ArmyStrategy::SneakyGoblin };
                }
                egui::Grid::new("farming_settings").num_columns(2).spacing([16.0, 12.0]).show(ui, |ui| {
                    ui.label("Village");
                    egui::ComboBox::from_id_salt("village")
                        .selected_text(match self.config.village {
                            Village::Home => "Home Village",
                            Village::BuilderBase => "Builder Base",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.config.village, Village::Home, "Home Village");
                            ui.selectable_value(&mut self.config.village, Village::BuilderBase, "Builder Base");
                        });
                    ui.end_row();

                    ui.label("Army strategy");
                    egui::ComboBox::from_id_salt("army_strategy")
                        .selected_text(self.config.strategy.label())
                        .show_ui(ui, |ui| {
                            for strategy in ArmyStrategy::ALL {
                                if strategy.is_builder_base() == builder_base {
                                    ui.selectable_value(&mut self.config.strategy, strategy, strategy.label());
                                }
                            }
                        });
                    ui.end_row();

                    ui.label("Loot priority");
                    egui::ComboBox::from_id_salt("loot_priority")
                        .selected_text(match self.config.loot_priority {
                            LootPriority::Gold => "Gold",
                            LootPriority::Elixir => "Elixir",
                            LootPriority::DarkElixir => "Dark elixir",
                            LootPriority::Balanced => "Balanced",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.config.loot_priority, LootPriority::Gold, "Gold");
                            ui.selectable_value(&mut self.config.loot_priority, LootPriority::Elixir, "Elixir");
                            ui.selectable_value(&mut self.config.loot_priority, LootPriority::DarkElixir, "Dark elixir");
                            ui.selectable_value(&mut self.config.loot_priority, LootPriority::Balanced, "Balanced");
                        });
                    ui.end_row();
                });
                ui.add_space(12.0);
                ui.label(format!("Selected strategy is intended for: {}", self.config.strategy.village_label()));
                ui.label("Farming is designed to continue until manually stopped; there is no run timer.");
                ui.add_space(8.0);
                ui.add_enabled(false, egui::Button::new("Start farming — game backend not implemented"));
                ui.label("Start is disabled until window capture, recognition, and safe input are implemented.");
                if ui.button("Save farming settings").clicked() {
                    match self.config.save() {
                        Ok(()) => self.logs.push("Farming settings saved.".into()),
                        Err(error) => self.logs.push(format!("Settings save failed: {error}")),
                    }
                }
            }
            Page::Settings => {
                ui.heading("Settings");
                ui.label("Tesseract executable path (optional; OCR integration is pending)");
                ui.text_edit_singleline(&mut self.config.tesseract_path);
                if ui.button("Save settings").clicked() {
                    match self.config.save() {
                        Ok(()) => self.logs.push("Settings saved.".into()),
                        Err(error) => self.logs.push(format!("Settings save failed: {error}")),
                    }
                }
            }
            Page::Logs => {
                ui.heading("Logs");
                egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                    for line in &self.logs {
                        ui.monospace(line);
                    }
                });
            }
        });
    }
}
