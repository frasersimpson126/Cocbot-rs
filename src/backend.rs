use crate::{
    automation::AutomationEngine,
    battle::BattleState,
    config::AppConfig,
    deployment::{DeploymentPlanner, Viewport},
    executor::{DeploymentExecutor, RaidNavigationExecutor, ReturnHomeExecutor},
    loot::{LootTracker, ResourceSnapshot},
    ocr::{OcrEngine, OcrProfile},
    platform::{create_input, GameWindow},
    pipeline::BattlePipeline,
    raid::{RaidCoordinator, RaidPhase},
    target::{TargetEvaluator, TargetPolicy},
    templates::TemplatePackDiagnostics,
    vision::{SearchRegion, TemplateMatcher},
};
use anyhow::{Context, Result};
use image::{imageops, DynamicImage};
use std::{
    path::PathBuf,
    fs,
    sync::{atomic::{AtomicBool, Ordering}, mpsc::{self, Sender}, Arc},
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone)]
pub enum BackendEvent {
    Connected { width: u32, height: u32 },
    LootUpdated { total: crate::loot::LootDelta, per_hour: crate::loot::LootDelta, elapsed: Duration },
    BattleUpdated { state: BattleState, confidence: f32 },
    RaidUpdated { phase: RaidPhase, command: String },
    TargetUpdated(String),
    Ready { can_start: bool, reason: String },
    Status(String),
    Error(String),
    Stopped,
}

#[derive(Debug, Clone, Copy)]
pub enum BackendCommand {
    StartFarming,
    PauseFarming,
    ResumeFarming,
    StopFarming,
    SaveCalibrationScreenshot,
}

/// Owns capture, perception, user controls and guarded farming input execution.
pub struct FarmingBackend {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    commands: Sender<BackendCommand>,
}

impl FarmingBackend {
    pub fn start(ocr: OcrEngine, config: AppConfig, events: Sender<BackendEvent>) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let (commands, command_rx) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("cocbot-backend".into())
            .spawn(move || {
                let window = match GameWindow::find() {
                    Ok(window) => window,
                    Err(error) => {
                        let _ = events.send(BackendEvent::Error(error.to_string()));
                        let _ = events.send(BackendEvent::Stopped);
                        return;
                    }
                };
                let (width, height) = window.dimensions();
                let _ = events.send(BackendEvent::Connected { width, height });

                // Opponent loot OCR is opt-in: only a user-calibrated crop is passed
                // to the pipeline. A missing or invalid crop keeps target decisions disabled.
                let opponent_roi = opponent_loot_region(width, height, &config);
                if opponent_roi.is_some() {
                    let _ = events.send(BackendEvent::Status(
                        "Opponent loot OCR enabled with the saved, user-calibrated crop. Verify readings against the live opponent screen before enabling input.".into()
                    ));
                } else {
                    let _ = events.send(BackendEvent::Status(
                        "Opponent loot OCR disabled: set a valid crop in Farming > Opponent loot OCR region.".into()
                    ));
                }
                let troop_template = match config.strategy {
                    crate::strategy::ArmyStrategy::SneakyGoblin => "sneaky_goblin.png",
                    crate::strategy::ArmyStrategy::Valkyrie => "valkyrie.png",
                    crate::strategy::ArmyStrategy::SuperMinion => "super_minion.png",
                    crate::strategy::ArmyStrategy::ElectroDragon => "electro_dragon.png",
                    crate::strategy::ArmyStrategy::SuperDragon => "super_dragon.png",
                    crate::strategy::ArmyStrategy::BuilderBaseBabyDragon => "baby_dragon.png",
                };
                let templates = template_directory(width, height);
                let diagnostics = TemplatePackDiagnostics::inspect(&templates, config.strategy);
                for message in diagnostics.messages() {
                    let _ = events.send(BackendEvent::Status(message));
                }
                let ocr_available = ocr.is_available();
                if !ocr_available {
                    let _ = events.send(BackendEvent::Status(format!("Tesseract OCR is unavailable at '{}'; configure its executable path before farming.", ocr.executable())));
                }
                let can_start = diagnostics.is_ready()
                    && opponent_roi.is_some()
                    && ocr_available
                    && config.template_pack_calibrated
                    && config.opponent_loot_roi_calibrated;
                let readiness_reason = if !diagnostics.is_ready() {
                    "Required screen or selected troop templates are missing/invalid.".to_owned()
                } else if !ocr_available {
                    format!("Tesseract OCR is unavailable at '{}'.", ocr.executable())
                } else if opponent_roi.is_none() {
                    "Configure a valid opponent-loot OCR crop before starting.".to_owned()
                } else if !config.template_pack_calibrated {
                    "Confirm that every required screen/troop template has been checked against live game screens.".to_owned()
                } else if !config.opponent_loot_roi_calibrated {
                    "Confirm that opponent-loot OCR matches real values across multiple opponent screens.".to_owned()
                } else {
                    "Template pack and opponent-loot OCR calibration have been confirmed.".to_owned()
                };
                let _ = events.send(BackendEvent::Ready { can_start, reason: readiness_reason.clone() });
                if !diagnostics.is_ready() {
                    let _ = events.send(BackendEvent::Status(
                        "Automation remains locked: required screen and troop templates are missing or invalid.".into()
                    ));
                }
                let executors = FarmingExecutors {
                    navigation: RaidNavigationExecutor::new(TemplateMatcher::new(templates.clone())),
                    return_home: ReturnHomeExecutor::new(TemplateMatcher::new(templates.clone())),
                    deployment: DeploymentExecutor::default(),
                };
                let mut input = None;
                let pipeline = BattlePipeline::new(
                    templates,
                    ocr.clone(),
                    opponent_roi.unwrap_or(SearchRegion { x: 0, y: 0, width: 1, height: 1 }),
                    vec![troop_template.to_owned()],
                );
                let pipeline = if opponent_roi.is_some() {
                    pipeline
                } else {
                    pipeline.without_opponent_loot_ocr()
                };
                let pipeline = pipeline.with_stable_samples(2);

                let policy = TargetPolicy {
                    minimum_gold: config.minimum_gold,
                    minimum_elixir: config.minimum_elixir,
                    minimum_dark_elixir: config.minimum_dark_elixir,
                    minimum_score_percent: config.minimum_score_percent,
                    minimum_confidence_percent: config.minimum_confidence_percent,
                };
                let evaluator = TargetEvaluator::new(policy, config.loot_priority);
                let raid = match RaidCoordinator::new(
                    config.village,
                    config.strategy,
                    evaluator,
                    DeploymentPlanner::default(),
                ) {
                    Ok(raid) => raid,
                    Err(error) => {
                        let _ = events.send(BackendEvent::Error(format!("Invalid farming configuration: {error:?}")));
                        let _ = events.send(BackendEvent::Stopped);
                        return;
                    }
                };
                let mut automation = AutomationEngine::new(pipeline, raid);

                let mut tracker = LootTracker::new();
                let mut previous_read: Option<ResourceSnapshot> = None;
                let mut stable_count = 0_u8;
                let mut last_ocr_error: Option<String> = None;
                let mut last_capture_error: Option<String> = None;
                let mut capture_error_frames = 0_u8;
                let mut resize_warning_active = false;
                let mut last_battle_state: Option<BattleState> = None;
                let mut unknown_state_frames = 0_u8;
                let mut last_raid_status: Option<(RaidPhase, String)> = None;
                let mut last_target_status: Option<String> = None;
                let mut pending_control: Option<BackendCommand> = None;
                let mut pending_screenshot = false;

                while !worker_stop.load(Ordering::Relaxed) {
                    // Pause/stop must still be honoured if capture temporarily fails.
                    while let Ok(control) = command_rx.try_recv() {
                        match control {
                            BackendCommand::PauseFarming => {
                                automation.pause();
                                pending_control = None;
                                let _ = events.send(BackendEvent::Status("Farming paused.".into()));
                            }
                            BackendCommand::StopFarming => {
                                automation.stop();
                                pending_control = None;
                                let _ = events.send(BackendEvent::Status("Farming session stopped.".into()));
                            }
                            BackendCommand::SaveCalibrationScreenshot => pending_screenshot = true,
                            start_or_resume @ (BackendCommand::StartFarming | BackendCommand::ResumeFarming) => {
                                pending_control = Some(start_or_resume);
                            }
                        }
                    }
                    let frame = match window.capture() {
                        Ok(frame) => {
                            last_capture_error = None;
                            capture_error_frames = 0;
                            frame
                        }
                        Err(error) => {
                            let message = error.to_string();
                            capture_error_frames = capture_error_frames.saturating_add(1);
                            if last_capture_error.as_deref() != Some(message.as_str()) {
                                let _ = events.send(BackendEvent::Status(format!("Screen capture failed: {message}")));
                                last_capture_error = Some(message);
                            }
                            if capture_error_frames >= 3
                                && !matches!(automation.phase(), RaidPhase::Idle | RaidPhase::Paused)
                            {
                                automation.pause();
                                let _ = events.send(BackendEvent::Status("Farming paused after repeated capture failures; restore the game window and explicitly resume.".into()));
                            }
                            sleep_interruptibly(&worker_stop, Duration::from_millis(250));
                            continue;
                        }
                    };

                    if frame.width() != width || frame.height() != height {
                        let message = format!(
                            "Game window resized from {width}x{height} to {}x{}; farming paused until monitoring is restarted and the ROI is recalibrated.",
                            frame.width(), frame.height()
                        );
                        if !resize_warning_active {
                            let _ = events.send(BackendEvent::Status(message));
                            resize_warning_active = true;
                        }
                        automation.pause();
                        sleep_interruptibly(&worker_stop, Duration::from_millis(250));
                        continue;
                    }
                    resize_warning_active = false;

                    match automation.process_frame(&frame, Viewport { width, height }) {
                        Ok(analysis) => {
                            let state = analysis.perception.stable_state;
                            if state == BattleState::Unknown {
                                unknown_state_frames = unknown_state_frames.saturating_add(1);
                                if unknown_state_frames >= 3
                                    && !matches!(automation.phase(), RaidPhase::Idle | RaidPhase::Paused)
                                {
                                    automation.pause();
                                    let _ = events.send(BackendEvent::Status("Farming paused: three consecutive frames were not recognised. Resolve the screen and explicitly resume.".into()));
                                }
                            } else {
                                unknown_state_frames = 0;
                            }
                            if last_battle_state != Some(state) {
                                let _ = events.send(BackendEvent::BattleUpdated {
                                    state,
                                    confidence: if state == BattleState::Unknown {
                                        0.0
                                    } else {
                                        analysis.perception.detection.confidence
                                    },
                                });
                                last_battle_state = Some(state);
                            }

                            // Commands are executed below only after explicit Start and only
                            // while startup asset checks permit farming input.
                            let command = format!("{:?}", analysis.command);
                            let status = (automation.phase(), command);
                            if last_raid_status.as_ref() != Some(&status) {
                                let _ = events.send(BackendEvent::RaidUpdated {
                                    phase: status.0,
                                    command: status.1.clone(),
                                });
                                last_raid_status = Some(status);
                            }

                            let target_status = if state == BattleState::OpponentFound {
                                if let Some(candidate) = analysis.perception.opponent_loot {
                                    automation.last_evaluation().map(|evaluation| {
                                        format!(
                                            "Opponent loot: {} gold / {} elixir / {} dark elixir | OCR {:.0}% | score {}% | {}{}",
                                            candidate.loot.gold,
                                            candidate.loot.elixir,
                                            candidate.loot.dark_elixir,
                                            candidate.confidence * 100.0,
                                            evaluation.score_percent,
                                            if evaluation.accepted { "ACCEPTED" } else { "REJECTED" },
                                            evaluation.rejection.map(|reason| format!(": {reason:?}")).unwrap_or_default()
                                        )
                                    })
                                } else {
                                    Some("Opponent loot OCR: waiting for a valid three-line read".to_owned())
                                }
                            } else {
                                None
                            };
                            if target_status != last_target_status {
                                let status = target_status.clone()
                                    .unwrap_or_else(|| "No opponent target evaluated".to_owned());
                                let _ = events.send(BackendEvent::TargetUpdated(status));
                                last_target_status = target_status;
                            }

                            let mut command = analysis.command.clone();
                            if let Some(control) = pending_control.take() {
                                match control {
                                    BackendCommand::StartFarming => {
                                        if !can_start {
                                            let _ = events.send(BackendEvent::Status(format!("Start refused: {readiness_reason}")));
                                        } else if !matches!(state, BattleState::HomeVillage | BattleState::BuilderBase) {
                                            let _ = events.send(BackendEvent::Status("Start refused: wait until the selected village home screen is stably recognised.".into()));
                                        } else {
                                            if input.is_none() {
                                                match create_input() {
                                                    Ok(created) => input = Some(created),
                                                    Err(error) => { let _ = events.send(BackendEvent::Error(format!("Desktop input unavailable: {error}"))); }
                                                }
                                            }
                                            if input.is_some() {
                                                match automation.start(state) {
                                                    Ok(start_command) => {
                                                        command = start_command;
                                                        let _ = events.send(BackendEvent::Status("Farming session started.".into()));
                                                    }
                                                    Err(error) => { let _ = events.send(BackendEvent::Status(format!("Could not start farming: {error:?}"))); }
                                                }
                                            }
                                        }
                                    }
                                    BackendCommand::ResumeFarming => {
                                        if matches!(state, BattleState::Unknown | BattleState::ConnectionError) {
                                            let _ = events.send(BackendEvent::Status("Resume refused: wait for a stable, recognised screen and resolve any connection error.".into()));
                                        } else {
                                            if input.is_none() {
                                                match create_input() {
                                                    Ok(created) => input = Some(created),
                                                    Err(error) => { let _ = events.send(BackendEvent::Error(format!("Desktop input unavailable: {error}"))); }
                                                }
                                            }
                                            if input.is_some() {
                                                command = automation.resume(state);
                                                let _ = events.send(BackendEvent::Status("Farming resume requested.".into()));
                                            }
                                        }
                                    }
                                    BackendCommand::PauseFarming | BackendCommand::StopFarming => {}
                                    BackendCommand::SaveCalibrationScreenshot => pending_screenshot = true,
                                }
                            }
                            while let Ok(control) = command_rx.try_recv() {
                                match control {
                                    BackendCommand::StartFarming => {
                                        if !can_start {
                                            let _ = events.send(BackendEvent::Status(format!("Start refused: {readiness_reason}")));
                                        } else if !matches!(state, BattleState::HomeVillage | BattleState::BuilderBase) {
                                            let _ = events.send(BackendEvent::Status("Start refused: wait until the selected village home screen is stably recognised.".into()));
                                        } else {
                                            if input.is_none() {
                                                match create_input() {
                                                    Ok(created) => input = Some(created),
                                                    Err(error) => { let _ = events.send(BackendEvent::Error(format!("Desktop input unavailable: {error}"))); }
                                                }
                                            }
                                            if input.is_some() {
                                                match automation.start(state) {
                                                    Ok(start_command) => {
                                                        command = start_command;
                                                        let _ = events.send(BackendEvent::Status("Farming session started.".into()));
                                                    }
                                                    Err(error) => { let _ = events.send(BackendEvent::Status(format!("Could not start farming: {error:?}"))); }
                                                }
                                            }
                                        }
                                    }
                                    BackendCommand::PauseFarming => {
                                        automation.pause();
                                        command = crate::raid::RaidCommand::WaitForStableFrame;
                                        let _ = events.send(BackendEvent::Status("Farming paused.".into()));
                                    }
                                    BackendCommand::ResumeFarming => {
                                        if matches!(state, BattleState::Unknown | BattleState::ConnectionError) {
                                            let _ = events.send(BackendEvent::Status("Resume refused: wait for a stable, recognised screen and resolve any connection error.".into()));
                                        } else {
                                            if input.is_none() {
                                                match create_input() {
                                                    Ok(created) => input = Some(created),
                                                    Err(error) => { let _ = events.send(BackendEvent::Error(format!("Desktop input unavailable: {error}"))); }
                                                }
                                            }
                                            if input.is_some() {
                                                command = automation.resume(state);
                                                let _ = events.send(BackendEvent::Status("Farming resume requested.".into()));
                                            }
                                        }
                                    }
                                    BackendCommand::StopFarming => {
                                        command = automation.stop();
                                        let _ = events.send(BackendEvent::Status("Farming session stopped.".into()));
                                    }
                                    BackendCommand::SaveCalibrationScreenshot => pending_screenshot = true,
                                }
                            }

                            if pending_screenshot {
                                pending_screenshot = false;
                                match save_calibration_screenshot(&frame) {
                                    Ok(path) => { let _ = events.send(BackendEvent::Status(format!("Calibration screenshot saved: {}", path.display()))); }
                                    Err(error) => { let _ = events.send(BackendEvent::Error(format!("Could not save calibration screenshot: {error}"))); }
                                }
                            }

                            if let Some(input_device) = input.as_mut()
                                && automation.phase() != RaidPhase::Idle
                                && !matches!(&command,
                                    &crate::raid::RaidCommand::None
                                    | &crate::raid::RaidCommand::WaitForStableFrame
                                    | &crate::raid::RaidCommand::RaidFinished)
                                && let Err(error) = execute_raid_command(
                                    input_device, &frame, config.village, state, &command,
                                    &executors, &mut automation,
                                )
                            {
                                let _ = events.send(BackendEvent::Error(format!("Action refused or failed: {error}")));
                                if matches!(&command, crate::raid::RaidCommand::Deploy(_)) {
                                    automation.stop();
                                    let _ = events.send(BackendEvent::Status("Deployment failed after input may have started; farming stopped to prevent duplicate troop deployment.".into()));
                                } else {
                                    automation.pause();
                                    let _ = events.send(BackendEvent::Status("Farming paused after an input/navigation error. Inspect the game before resuming.".into()));
                                }
                            }
                        }
                        Err(error) => {
                            let _ = events.send(BackendEvent::Status(format!("Battle automation analysis skipped: {error}")));
                        }
                    }

                    match read_hud_from_frame(&frame, &ocr) {
                        Ok(Some(snapshot)) => {
                            last_ocr_error = None;
                            if previous_read == Some(snapshot) {
                                stable_count = stable_count.saturating_add(1);
                            } else {
                                previous_read = Some(snapshot);
                                stable_count = 1;
                            }
                            if stable_count >= 2 {
                                tracker.observe(snapshot);
                                let summary = tracker.summary();
                                let _ = events.send(BackendEvent::LootUpdated {
                                    total: summary.total,
                                    per_hour: summary.per_hour,
                                    elapsed: summary.elapsed,
                                });
                                stable_count = 0;
                            }
                        }
                        Ok(None) => {
                            last_ocr_error = None;
                            stable_count = 0;
                            previous_read = None;
                        }
                        Err(error) => {
                            let message = error.to_string();
                            if last_ocr_error.as_deref() != Some(message.as_str()) {
                                let _ = events.send(BackendEvent::Status(format!("OCR read skipped: {message}")));
                                last_ocr_error = Some(message);
                            }
                        }
                    }

                    sleep_interruptibly(&worker_stop, Duration::from_millis(500));
                }
                let _ = events.send(BackendEvent::Stopped);
            })?;

        Ok(Self { stop, worker: Some(worker), commands })
    }

    pub fn start_farming(&self) -> Result<()> {
        self.commands.send(BackendCommand::StartFarming).map_err(|_| anyhow::anyhow!("Farming worker is not available"))
    }

    pub fn pause_farming(&self) -> Result<()> {
        self.commands.send(BackendCommand::PauseFarming).map_err(|_| anyhow::anyhow!("Farming worker is not available"))
    }

    pub fn resume_farming(&self) -> Result<()> {
        self.commands.send(BackendCommand::ResumeFarming).map_err(|_| anyhow::anyhow!("Farming worker is not available"))
    }

    pub fn stop_farming(&self) -> Result<()> {
        self.commands.send(BackendCommand::StopFarming).map_err(|_| anyhow::anyhow!("Farming worker is not available"))
    }

    pub fn save_calibration_screenshot(&self) -> Result<()> {
        self.commands.send(BackendCommand::SaveCalibrationScreenshot).map_err(|_| anyhow::anyhow!("Farming worker is not available"))
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for FarmingBackend {
    fn drop(&mut self) {
        self.stop();
    }
}


fn save_calibration_screenshot(frame: &DynamicImage) -> Result<PathBuf> {
    let directory = AppConfig::calibration_dir()?;
    fs::create_dir_all(&directory)?;
    let data_directory = directory.parent()
        .context("Could not resolve local application data directory")?;
    let aspect_directory = if frame.width() as f32 / frame.height().max(1) as f32 > 1.70 {
        "16_9"
    } else {
        "16_10"
    };
    fs::create_dir_all(data_directory.join("assets").join("templates").join(aspect_directory))?;
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let path = directory.join(format!("game-capture-{timestamp}.png"));
    frame.save(&path)?;
    Ok(path)
}

struct FarmingExecutors {
    navigation: RaidNavigationExecutor,
    return_home: ReturnHomeExecutor,
    deployment: DeploymentExecutor,
}

fn execute_raid_command(
    input: &mut enigo::Enigo,
    frame: &DynamicImage,
    village: crate::config::Village,
    state: BattleState,
    command: &crate::raid::RaidCommand,
    executors: &FarmingExecutors,
    automation: &mut AutomationEngine,
) -> Result<()> {
    // Re-resolve the window immediately before sending input so a moved or
    // minimised game window cannot leave stale desktop coordinates in use.
    let action_window = GameWindow::find()?;
    if action_window.dimensions() != (frame.width(), frame.height()) {
        anyhow::bail!("Game window dimensions changed during farming; reconnect and recalibrate before continuing");
    }
    match command {
        crate::raid::RaidCommand::OpenAttackMenu
        | crate::raid::RaidCommand::FindOpponent
        | crate::raid::RaidCommand::SkipOpponent
        | crate::raid::RaidCommand::AttackOpponent => {
            executors.navigation.execute(&action_window, input, frame, village, state, command)?;
            if matches!(command, crate::raid::RaidCommand::SkipOpponent) {
                automation.confirm_opponent_skipped()
                    .map_err(|error| anyhow::anyhow!("Could not confirm skipped opponent: {error:?}"))?;
            }
        }
        crate::raid::RaidCommand::Deploy(plan) => {
            let receipt = executors.deployment.execute(&action_window, input, state, plan)?;
            automation.confirm_deployment_started(state)
                .map_err(|error| anyhow::anyhow!("Could not confirm deployment: {error:?}"))?;
            tracing::info!("Deployment sent for {} ({} actions)", receipt.troop_name, receipt.actions_sent);
        }
        crate::raid::RaidCommand::ReturnToVillage => {
            executors.return_home.execute(&action_window, input, frame, village, state)?;
        }
        _ => {}
    }
    Ok(())
}

/// Resolve the user-writable app-data template pack first, then beside the executable,
/// and finally from the current working directory.
fn contains_template_images(directory: &std::path::Path) -> bool {
    fs::read_dir(directory).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry.path().extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
        })
    })
}

fn template_directory(width: u32, height: u32) -> PathBuf {
    let aspect_directory = if width as f32 / height.max(1) as f32 > 1.70 {
        "16_9"
    } else {
        "16_10"
    };
    if let Ok(calibration_directory) = AppConfig::calibration_dir()
        && let Some(data_directory) = calibration_directory.parent()
    {
        let candidate = data_directory.join("assets").join("templates").join(aspect_directory);
        if contains_template_images(&candidate) {
            return candidate;
        }
    }
    if let Ok(executable) = std::env::current_exe()
        && let Some(directory) = executable.parent()
    {
        let candidate = directory.join("assets").join("templates").join(aspect_directory);
        if contains_template_images(&candidate) {
            return candidate;
        }
    }
    PathBuf::from("assets").join("templates").join(aspect_directory)
}

fn sleep_interruptibly(stop: &AtomicBool, duration: Duration) {
    let interval = Duration::from_millis(50);
    let mut remaining = duration;
    while !remaining.is_zero() && !stop.load(Ordering::Relaxed) {
        let sleep = remaining.min(interval);
        thread::sleep(sleep);
        remaining = remaining.saturating_sub(sleep);
    }
}


/// Converts a user-calibrated percentage crop into captured-frame pixels.
/// A zero-sized crop or coordinates extending beyond the frame disables OCR.
fn opponent_loot_region(width: u32, height: u32, config: &AppConfig) -> Option<SearchRegion> {
    let x = config.opponent_loot_x_percent as u16;
    let y = config.opponent_loot_y_percent as u16;
    let w = config.opponent_loot_width_percent as u16;
    let h = config.opponent_loot_height_percent as u16;
    if width == 0 || height == 0 || w == 0 || h == 0 || x + w > 100 || y + h > 100 {
        return None;
    }
    let pixel_x = width.saturating_mul(x as u32) / 100;
    let pixel_y = height.saturating_mul(y as u32) / 100;
    let pixel_width = width.saturating_mul(w as u32) / 100;
    let pixel_height = height.saturating_mul(h as u32) / 100;
    if pixel_width == 0 || pixel_height == 0 {
        return None;
    }
    Some(SearchRegion { x: pixel_x, y: pixel_y, width: pixel_width, height: pixel_height })
}

/// Crops the top-right HUD before OCR. Restricting OCR to this ROI is substantially
/// faster and more reliable than running recognition over the full game screenshot.
fn read_hud_from_frame(frame: &DynamicImage, ocr: &OcrEngine) -> Result<Option<ResourceSnapshot>> {
    let (width, height) = (frame.width(), frame.height());
    let is_widescreen = width as f32 / height as f32 > 1.70;
    let (reference_width, reference_height, roi_ref_width, roi_ref_height) = if is_widescreen {
        (2560.0_f32, 1440.0_f32, 550.0_f32, 350.0_f32)
    } else {
        (2560.0_f32, 1600.0_f32, 600.0_f32, 400.0_f32)
    };
    let roi_width = ((width as f32 * roi_ref_width / reference_width).round() as u32).min(width);
    let roi_height = ((height as f32 * roi_ref_height / reference_height).round() as u32).min(height);
    let left = width.saturating_sub(roi_width);
    if roi_width == 0 || roi_height == 0 {
        return Ok(None);
    }
    let roi = imageops::crop_imm(frame, left, 0, roi_width, roi_height).to_image();
    let result = ocr.recognise(&DynamicImage::ImageRgba8(roi), OcrProfile::HudNumbers)?;
    let Some([gold, elixir, dark_elixir]) = OcrEngine::parse_hud_triplet(&result.text) else {
        return Ok(None);
    };
    Ok(Some(ResourceSnapshot { gold, elixir, dark_elixir }))
}

#[cfg(test)]
mod opponent_roi_tests {
    use super::*;

    #[test]
    fn opponent_crop_scales_from_percentages_to_frame_pixels() {
        let config = AppConfig {
            opponent_loot_x_percent: 10,
            opponent_loot_y_percent: 20,
            opponent_loot_width_percent: 30,
            opponent_loot_height_percent: 15,
            ..AppConfig::default()
        };
        let region = opponent_loot_region(1920, 1080, &config).unwrap();
        assert_eq!((region.x, region.y, region.width, region.height), (192, 216, 576, 162));
    }

    #[test]
    fn opponent_crop_is_disabled_until_a_nonzero_valid_size_is_set() {
        let config = AppConfig::default();
        assert!(opponent_loot_region(1920, 1080, &config).is_none());
    }

    #[test]
    fn opponent_crop_outside_frame_is_rejected() {
        let config = AppConfig {
            opponent_loot_x_percent: 90,
            opponent_loot_width_percent: 20,
            opponent_loot_height_percent: 10,
            ..AppConfig::default()
        };
        assert!(opponent_loot_region(1920, 1080, &config).is_none());
    }
}
