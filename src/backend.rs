use crate::{
    automation::AutomationEngine,
    battle::BattleState,
    config::{AppConfig, LootPriority},
    deployment::{DeploymentPlanner, Viewport},
    loot::{LootTracker, ResourceSnapshot},
    ocr::{OcrEngine, OcrProfile},
    platform::GameWindow,
    pipeline::BattlePipeline,
    raid::{RaidCoordinator, RaidPhase},
    target::{TargetEvaluator, TargetPolicy},
    vision::SearchRegion,
};
use anyhow::Result;
use image::{imageops, DynamicImage};
use std::{
    path::PathBuf,
    sync::{atomic::{AtomicBool, Ordering}, mpsc::Sender, Arc},
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Debug, Clone)]
pub enum BackendEvent {
    Connected { width: u32, height: u32 },
    LootUpdated { total: crate::loot::LootDelta, per_hour: crate::loot::LootDelta, elapsed: Duration },
    BattleUpdated { state: BattleState, confidence: f32 },
    RaidUpdated { phase: RaidPhase, command: String },
    Status(String),
    Error(String),
    Stopped,
}

/// Owns the long-running capture and perception worker. Automated input is deliberately
/// not invoked here: screen templates and loot coordinates must be calibrated first.
pub struct FarmingBackend {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl FarmingBackend {
    pub fn start(ocr: OcrEngine, config: AppConfig, events: Sender<BackendEvent>) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
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

                // No opponent ROI or troop templates are configured yet. This connects
                // live frames to state recognition without treating guessed coordinates
                // or absent assets as evidence for an attack.
                let pipeline = BattlePipeline::new(
                    template_directory(width, height),
                    ocr.clone(),
                    SearchRegion { x: 0, y: 0, width: 1, height: 1 },
                    Vec::new(),
                )
                .without_opponent_loot_ocr()
                .with_stable_samples(2);

                let evaluator = TargetEvaluator::new(TargetPolicy::default(), config.loot_priority);
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
                let mut last_battle_state: Option<BattleState> = None;
                let mut last_raid_status: Option<(RaidPhase, String)> = None;

                while !worker_stop.load(Ordering::Relaxed) {
                    let frame = match window.capture() {
                        Ok(frame) => {
                            last_capture_error = None;
                            frame
                        }
                        Err(error) => {
                            let message = error.to_string();
                            if last_capture_error.as_deref() != Some(message.as_str()) {
                                let _ = events.send(BackendEvent::Status(format!("Screen capture failed: {message}")));
                                last_capture_error = Some(message);
                            }
                            sleep_interruptibly(&worker_stop, Duration::from_millis(250));
                            continue;
                        }
                    };

                    match automation.process_frame(&frame, Viewport { width, height }) {
                        Ok(analysis) => {
                            let state = analysis.perception.stable_state;
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

                            // The coordinator is live, but commands are only surfaced to
                            // the UI. No mouse input is sent until templates and coordinates
                            // have been validated against real game screenshots.
                            let command = format!("{:?}", analysis.command);
                            let status = (automation.phase(), command);
                            if last_raid_status.as_ref() != Some(&status) {
                                let _ = events.send(BackendEvent::RaidUpdated {
                                    phase: status.0,
                                    command: status.1.clone(),
                                });
                                last_raid_status = Some(status);
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

        Ok(Self { stop, worker: Some(worker) })
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


/// Resolve the aspect-specific template pack beside the executable first, then from
/// the current working directory. This keeps packaged builds independent of launch cwd.
fn template_directory(width: u32, height: u32) -> PathBuf {
    let aspect_directory = if width as f32 / height.max(1) as f32 > 1.70 {
        "16_9"
    } else {
        "16_10"
    };
    if let Ok(executable) = std::env::current_exe()
        && let Some(directory) = executable.parent()
    {
        let candidate = directory.join("assets").join("templates").join(aspect_directory);
        if candidate.is_dir() {
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
