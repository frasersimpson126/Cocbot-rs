use crate::{
    config::{AppConfig, LootPriority, PixelRegion, Village},
    battle::BattleState,
    loot::{LootTracker, LootDelta, ResourceSnapshot},
    backend::{BackendEvent, FarmingBackend},
    ocr::OcrEngine,
    strategy::ArmyStrategy,
};
use eframe::egui;
use std::{path::PathBuf, sync::mpsc::{self, Receiver, Sender, TryRecvError}};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Overview,
    Farming,
    Settings,
    Logs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CalibrationTarget { StorageHud, OpponentLoot, TroopBar, BattleControls }
impl CalibrationTarget {
    const ALL: [Self; 4] = [Self::StorageHud, Self::OpponentLoot, Self::TroopBar, Self::BattleControls];
    fn label(self) -> &'static str { match self {
        Self::StorageHud => "Home storage HUD", Self::OpponentLoot => "Opponent loot counters",
        Self::TroopBar => "Troop deployment bar", Self::BattleControls => "Battle controls",
    }}
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
    target_status: String,
    automation_ready: bool,
    readiness_status: String,
    backend_loot: Option<(LootDelta, LootDelta)>,
    backend_elapsed: Option<std::time::Duration>,
    current_storage: Option<ResourceSnapshot>,
    screenshot_texture: Option<egui::TextureHandle>,
    screenshot_dimensions: Option<(u32, u32)>,
    calibration_target: CalibrationTarget,
    selection_start: Option<egui::Pos2>,
    tesseract_status: String,
    event_tx: Sender<BackendEvent>,
    event_rx: Receiver<BackendEvent>,
}

impl CocbotApp {
    pub fn new(cc: &eframe::CreationContext<'_>, config: AppConfig) -> Self {
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.spacing.item_spacing = egui::vec2(12.0, 12.0);
        style.spacing.button_padding = egui::vec2(18.0, 11.0);
        style.spacing.interact_size.y = 42.0;
        style.spacing.window_margin = egui::Margin::same(16);
        style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(39, 48, 64);
        style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(55, 69, 91);
        style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(67, 103, 156);
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
            target_status: "No opponent target evaluated".into(),
            automation_ready: false,
            readiness_status: "Connect the game to inspect required templates and OCR calibration.".into(),
            backend_loot: None,
            backend_elapsed: None,
            current_storage: None,
            screenshot_texture: None,
            screenshot_dimensions: None,
            calibration_target: CalibrationTarget::StorageHud,
            selection_start: None,
            tesseract_status: "Not tested".into(),
            event_tx,
            event_rx,
        }
    }

    fn nav(ui: &mut egui::Ui, current: &mut Page, page: Page, label: &str) {
        if ui.selectable_label(*current == page, label).clicked() {
            *current = page;
        }
    }

    fn load_screenshot(&mut self, ctx: &egui::Context, path: PathBuf) {
        match image::open(path) {
            Ok(image) => {
                let rgba = image.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                self.screenshot_texture = Some(ctx.load_texture("calibration-screenshot", color, egui::TextureOptions::LINEAR));
                self.screenshot_dimensions = Some((rgba.width(), rgba.height()));
                self.selection_start = None;
            }
            Err(error) => self.logs.push(format!("Could not open calibration screenshot: {error}")),
        }
    }

    fn selected_region(&self) -> Option<PixelRegion> {
        match self.calibration_target {
            CalibrationTarget::StorageHud => self.config.hud_roi,
            CalibrationTarget::OpponentLoot => self.config.opponent_loot_roi,
            CalibrationTarget::TroopBar => self.config.troop_bar_roi,
            CalibrationTarget::BattleControls => self.config.battle_controls_roi,
        }
    }

    fn set_selected_region(&mut self, region: Option<PixelRegion>) {
        match self.calibration_target {
            CalibrationTarget::StorageHud => self.config.hud_roi = region,
            CalibrationTarget::OpponentLoot => { self.config.opponent_loot_roi = region; self.config.opponent_loot_roi_calibrated = false; },
            CalibrationTarget::TroopBar => self.config.troop_bar_roi = region,
            CalibrationTarget::BattleControls => self.config.battle_controls_roi = region,
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
                    BackendEvent::TargetUpdated(status) => self.target_status = status,
                    BackendEvent::Ready { can_start, reason } => {
                        self.automation_ready = can_start;
                        self.readiness_status = reason;
                    }
                    BackendEvent::BattleUpdated { state, confidence } => {
                        self.battle_status = format!("{} ({:.0}% confidence)", battle_state_label(state), confidence * 100.0);
                        self.logs.push(format!("Battle screen: {}", battle_state_label(state)));
                    }
                    BackendEvent::LootUpdated { total, per_hour, elapsed } => {
                        self.backend_loot = Some((total, per_hour));
                        self.backend_elapsed = Some(elapsed);
                    }
                    BackendEvent::StorageUpdated(snapshot) => self.current_storage = Some(snapshot),
                    BackendEvent::Status(message) => self.logs.push(message),
                    BackendEvent::Error(message) => {
                        self.backend_status = format!("Error: {message}");
                        self.logs.push(self.backend_status.clone());
                    }
                    BackendEvent::Stopped => {
                        self.backend_status = "Monitoring stopped".into();
                        self.automation_ready = false;
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
                ui.label("Live game capture, OCR and farming status.");
                ui.separator();
                ui.heading("Current storages");
                if let Some(storage) = self.current_storage {
                    ui.horizontal(|ui| {
                        ui.label(format!("Gold: {:}", storage.gold));
                        ui.label(format!("Elixir: {:}", storage.elixir));
                        ui.label(format!("Dark elixir: {:}", storage.dark_elixir));
                    });
                } else { ui.label("Waiting for OCR readings. Connect to the game to read storage values."); }
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
                ui.collapsing("Opponent loot OCR region", |ui| {
                    ui.label("Set a crop around the three opponent resource counters. Values are percentages of the full captured game frame; leave width/height at 0 to keep opponent OCR disabled.");
                    ui.add(egui::Slider::new(&mut self.config.opponent_loot_x_percent, 0..=100).text("Left (%)"));
                    ui.add(egui::Slider::new(&mut self.config.opponent_loot_y_percent, 0..=100).text("Top (%)"));
                    ui.add(egui::Slider::new(&mut self.config.opponent_loot_width_percent, 0..=100).text("Width (%)"));
                    ui.add(egui::Slider::new(&mut self.config.opponent_loot_height_percent, 0..=100).text("Height (%)"));
                    let valid = self.config.opponent_loot_width_percent > 0
                        && self.config.opponent_loot_height_percent > 0
                        && self.config.opponent_loot_x_percent as u16 + self.config.opponent_loot_width_percent as u16 <= 100
                        && self.config.opponent_loot_y_percent as u16 + self.config.opponent_loot_height_percent as u16 <= 100;
                    ui.label(if valid { "Crop configured; compare OCR output with real opponent loot before trusting it." } else { "Crop disabled or outside the frame." });
                    ui.checkbox(&mut self.config.opponent_loot_roi_calibrated, "I verified this crop against several real opponent screens");
                });
                ui.checkbox(&mut self.config.template_pack_calibrated, "I verified all required templates against live game screens");
                ui.label("Save settings and reconnect after changing the crop or calibration confirmations.");
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
                ui.label(format!("Target evaluation: {}", self.target_status));
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
                ui.label("Start farming is enabled only when required screen/troop templates and the opponent-loot crop pass startup checks. Actions are checked against the current stable screen before input.");
                if self.backend.is_some() {
                    ui.horizontal(|ui| {
                        if ui.add_enabled(self.automation_ready, egui::Button::new("Start farming")).clicked()
                            && let Some(backend) = self.backend.as_ref()
                            && let Err(error) = backend.start_farming()
                        {
                            self.logs.push(format!("Start command failed: {error}"));
                        }
                        if ui.button("Pause").clicked()
                            && let Some(backend) = self.backend.as_ref()
                            && let Err(error) = backend.pause_farming()
                        {
                            self.logs.push(format!("Pause command failed: {error}"));
                        }
                        if ui.button("Resume").clicked()
                            && let Some(backend) = self.backend.as_ref()
                            && let Err(error) = backend.resume_farming()
                        {
                            self.logs.push(format!("Resume command failed: {error}"));
                        }
                        if ui.button("Stop farming").clicked()
                            && let Some(backend) = self.backend.as_ref()
                            && let Err(error) = backend.stop_farming()
                        {
                            self.logs.push(format!("Stop command failed: {error}"));
                        }
                        if ui.button("Save calibration screenshot").clicked()
                            && let Some(backend) = self.backend.as_ref()
                            && let Err(error) = backend.save_calibration_screenshot()
                        {
                            self.logs.push(format!("Screenshot command failed: {error}"));
                        }
                    });
                    if !self.automation_ready {
                        ui.label(format!("Farming locked: {}", self.readiness_status));
                    }
                }
                if ui.button("Save farming settings").clicked() {
                    match self.config.save() {
                        Ok(()) => self.logs.push("Farming settings saved.".into()),
                        Err(error) => self.logs.push(format!("Settings save failed: {error}")),
                    }
                }
            }
            Page::Settings => {
                ui.heading("Settings");
                ui.heading("Tesseract OCR");
                ui.horizontal(|ui| {
                    ui.label("Executable path");
                    ui.text_edit_singleline(&mut self.config.tesseract_path);
                    if ui.button("Test connection").clicked() {
                        let path = if self.config.tesseract_path.trim().is_empty() { "tesseract".to_owned() } else { self.config.tesseract_path.trim().to_owned() };
                        self.tesseract_status = match OcrEngine::new(path).test_connection() {
                            Ok(message) => format!("Connected: {message}"),
                            Err(error) => format!("Failed: {error}"),
                        };
                        self.logs.push(self.tesseract_status.clone());
                    }
                });
                ui.label(&self.tesseract_status);
                ui.separator();
                ui.heading("Screen area calibration");
                ui.label("Capture the game, select an area by dragging over the screenshot, then save the settings.");
                if ui.button("New screenshot").clicked() {
                    match FarmingBackend::capture_screenshot_now() {
                        Ok(path) => self.load_screenshot(ctx, path),
                        Err(error) => self.logs.push(format!("Screenshot capture failed: {error}")),
                    }
                }
                ui.horizontal(|ui| {
                    ui.label("Area");
                    egui::ComboBox::from_id_salt("calibration_target")
                        .selected_text(self.calibration_target.label())
                        .show_ui(ui, |ui| {
                            for target in CalibrationTarget::ALL { ui.selectable_value(&mut self.calibration_target, target, target.label()); }
                        });
                    if ui.button("Clear selection").clicked() { self.set_selected_region(None); }
                });
                if let Some(region) = self.selected_region() {
                    ui.label(format!("x={}  y={}  width={}  height={}", region.x, region.y, region.width, region.height));
                } else { ui.label("No saved area for this item."); }
                if let (Some(texture), Some((width, height))) = (&self.screenshot_texture, self.screenshot_dimensions) {
                    let available = ui.available_size();
                    let scale = (available.x / width as f32).min(500.0 / height as f32).min(1.0);
                    let size = egui::vec2(width as f32 * scale, height as f32 * scale);
                    let response = ui.add(egui::Image::new((texture.id(), size)).sense(egui::Sense::click_and_drag()));
                    if response.drag_started() { self.selection_start = response.interact_pointer_pos(); }
                    if response.drag_stopped() {
                        if let (Some(a), Some(b)) = (self.selection_start.take(), response.interact_pointer_pos()) {
                            let rect = egui::Rect::from_two_pos(a, b).intersect(response.rect);
                            let x = ((rect.left() - response.rect.left()) / response.rect.width() * width as f32).round().clamp(0.0, width as f32) as u32;
                            let y = ((rect.top() - response.rect.top()) / response.rect.height() * height as f32).round().clamp(0.0, height as f32) as u32;
                            let right = ((rect.right() - response.rect.left()) / response.rect.width() * width as f32).round().clamp(0.0, width as f32) as u32;
                            let bottom = ((rect.bottom() - response.rect.top()) / response.rect.height() * height as f32).round().clamp(0.0, height as f32) as u32;
                            if right > x && bottom > y { self.set_selected_region(Some(PixelRegion { x, y, width: right - x, height: bottom - y })); }
                        }
                    }
                    if let Some(region) = self.selected_region() {
                        if region.is_valid_for(width, height) {
                            let rect = egui::Rect::from_min_max(
                                egui::pos2(response.rect.left() + region.x as f32 / width as f32 * response.rect.width(), response.rect.top() + region.y as f32 / height as f32 * response.rect.height()),
                                egui::pos2(response.rect.left() + (region.x + region.width) as f32 / width as f32 * response.rect.width(), response.rect.top() + (region.y + region.height) as f32 / height as f32 * response.rect.height()),
                            );
                            ui.painter().rect_filled(rect, 0.0, egui::Color32::from_rgba_unmultiplied(40, 150, 255, 55));
                            ui.painter().rect_stroke(rect, 0.0, egui::Stroke::new(2.0, egui::Color32::LIGHT_BLUE), egui::StrokeKind::Inside);
                        }
                    }
                    ui.label(format!("Screenshot: {width} × {height} pixels"));
                } else { ui.label("No screenshot loaded. Keep the game visible and press New screenshot."); }
                ui.separator();
                if ui.button("Save settings and coordinates").clicked() {
                    match self.config.save() {
                        Ok(()) => self.logs.push("Settings and pixel coordinates saved.".into()),
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
