use crate::{
    config::{AppConfig, LootPriority, Village},
    battle::BattleState,
    loot::{LootTracker, LootDelta},
    backend::{BackendEvent, FarmingBackend},
    ocr::OcrEngine,
    strategy::ArmyStrategy,
};
use eframe::egui;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

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
    backend: Option<FarmingBackend>,
    backend_status: String,
    battle_status: String,
    raid_status: String,
    backend_loot: Option<(LootDelta, LootDelta)>,
    backend_elapsed: Option<std::time::Duration>,
    event_tx: Sender<BackendEvent>,
    event_rx: Receiver<BackendEvent>,
}

impl CocbotApp {
    pub fn new(cc: &eframe::CreationContext<'_>, config: AppConfig) -> Self {
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        cc.egui_ctx.set_style(style);
        let (event_tx, event_rx) = mpsc::channel();
        Self {
            config,
            page: Page::Overview,
            logs: vec!["Cocbot-rs initialised. Game automation backend is not connected yet.".into()],
            loot: LootTracker::new(),
            backend: None,
            backend_status: "Not connected".into(),
            battle_status: "Not available".into(),
            raid_status: "Idle".into(),
            backend_loot: None,
            backend_elapsed: None,
            event_tx,
            event_rx,
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

}

impl eframe::App for CocbotApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll worker events regularly so live OCR/loot updates render without user input.
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        loop {
            match self.event_rx.try_recv() {
                Ok(event) => match event {
                    BackendEvent::Connected { width, height } => {
                        self.backend_status = format!("Connected — {width}×{height}");
                        self.logs.push(self.backend_status.clone());
                    }
                    BackendEvent::RaidUpdated { phase, command } => {
                        self.raid_status = format!("{phase:?} — pending: {command}");
                        self.logs.push(format!("Raid coordinator: {}", self.raid_status));
                    }
                    BackendEvent::BattleUpdated { state, confidence } => {
                        self.battle_status = format!("{} ({:.0}% confidence)", battle_state_label(state), confidence * 100.0);
                        self.logs.push(format!("Battle screen: {}", battle_state_label(state)));
                    }
                    BackendEvent::LootUpdated { total, per_hour, elapsed } => {
                        self.backend_loot = Some((total, per_hour));
                        self.backend_elapsed = Some(elapsed);
                    }
                    BackendEvent::Status(message) => self.logs.push(message),
                    BackendEvent::Error(message) => {
                        self.backend_status = format!("Error: {message}");
                        self.logs.push(self.backend_status.clone());
                    }
                    BackendEvent::Stopped => {
                        self.backend_status = "Monitoring stopped".into();
                        self.logs.push("Game monitoring stopped.".into());
                        let _ = self.backend.take();
                    }
                },
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Cocbot-rs");
                ui.label(egui::RichText::new("FARMING AUTOMATION").small().weak());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(self.backend_status.to_uppercase()).color(if self.backend.is_some() { egui::Color32::LIGHT_GREEN } else { egui::Color32::YELLOW }));
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
                ui.label("Live game capture and HUD OCR status is shown in the Farming page.");
                ui.separator();
                let summary = self.loot.summary();
                let (total, per_hour) = self.backend_loot.unwrap_or((summary.total, summary.per_hour));
                ui.label("Session loot");
                Self::resource_row(ui, "Gold", total.gold, per_hour.gold);
                Self::resource_row(ui, "Elixir", total.elixir, per_hour.elixir);
                Self::resource_row(ui, "Dark elixir", total.dark_elixir, per_hour.dark_elixir);
                ui.add_space(12.0);
                let elapsed = self.backend_elapsed.unwrap_or(summary.elapsed);
                ui.label(format!("Elapsed tracking time: {:.0} min", elapsed.as_secs_f64() / 60.0));
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
                ui.add_space(10.0);
                ui.collapsing("Target requirements", |ui| {
                    ui.label("A target must meet all three resource minimums and the weighted score/confidence thresholds.");
                    ui.add(egui::Slider::new(&mut self.config.minimum_gold, 100_000..=2_000_000)
                        .step_by(50_000.0).text("Minimum gold"));
                    ui.add(egui::Slider::new(&mut self.config.minimum_elixir, 100_000..=2_000_000)
                        .step_by(50_000.0).text("Minimum elixir"));
                    ui.add(egui::Slider::new(&mut self.config.minimum_dark_elixir, 0..=20_000)
                        .step_by(500.0).text("Minimum dark elixir"));
                    ui.add(egui::Slider::new(&mut self.config.minimum_score_percent, 0..=100)
                        .text("Minimum loot score (%)"));
                    ui.add(egui::Slider::new(&mut self.config.minimum_confidence_percent, 50..=100)
                        .text("Minimum OCR confidence (%)"));
                });
                ui.add_space(12.0);
                ui.label(format!("Selected strategy is intended for: {}", self.config.strategy.village_label()));
                ui.label(format!("Recognised battle screen: {}", self.battle_status));
                ui.label(format!("Raid coordinator: {}", self.raid_status));
                ui.label("Farming is designed to continue until manually stopped; there is no run timer.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if self.backend.is_none() {
                        if ui.button("Connect game & monitor loot").clicked() {
                            let executable = if self.config.tesseract_path.trim().is_empty() {
                                "tesseract".to_owned()
                            } else {
                                self.config.tesseract_path.trim().to_owned()
                            };
                            match FarmingBackend::start(OcrEngine::new(executable), self.config.clone(), self.event_tx.clone()) {
                                Ok(backend) => {
                                    self.backend = Some(backend);
                                    self.backend_status = "Connecting…".into();
                                    self.logs.push("Starting game capture and HUD monitoring.".into());
                                }
                                Err(error) => {
                                    self.backend_status = format!("Error: {error}");
                                    self.logs.push(self.backend_status.clone());
                                }
                            }
                        }
                    } else if ui.button("Stop monitoring").clicked()
                        && let Some(mut backend) = self.backend.take()
                    {
                        backend.stop();
                    }
                });
                ui.label("Live capture feeds HUD OCR, battle-state recognition and the raid coordinator. Planned actions are displayed but not sent. Screen templates, opponent loot crops and troop icons still require calibration; automated input remains disabled.");
                ui.add_enabled(false, egui::Button::new("Start farming — attack engine pending"));
                if ui.button("Save farming settings").clicked() {
                    match self.config.save() {
                        Ok(()) => self.logs.push("Farming settings saved.".into()),
                        Err(error) => self.logs.push(format!("Settings save failed: {error}")),
                    }
                }
            }
            Page::Settings => {
                ui.heading("Settings");
                ui.label("Tesseract executable path (optional; required for live loot OCR)");
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

fn battle_state_label(state: BattleState) -> &'static str {
    match state {
        BattleState::HomeVillage => "Home Village",
        BattleState::BuilderBase => "Builder Base",
        BattleState::AttackMenu => "Attack menu",
        BattleState::Matchmaking => "Matchmaking",
        BattleState::OpponentFound => "Opponent found",
        BattleState::HomeBattle => "Home Village battle",
        BattleState::BuilderBattle => "Builder Base battle",
        BattleState::BattleEnding => "Battle ending",
        BattleState::BattleWon => "Battle won",
        BattleState::BattleLost => "Battle lost",
        BattleState::ReturningHome => "Returning home",
        BattleState::ConnectionError => "Connection error — automation must pause",
        BattleState::Unknown => "Unknown — templates required",
    }
}
