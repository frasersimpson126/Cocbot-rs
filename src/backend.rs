use crate::{
    loot::{LootTracker, ResourceSnapshot},
    ocr::{OcrEngine, OcrProfile},
    platform::{create_input, GameWindow},
};
use anyhow::Result;
use image::{imageops, DynamicImage};
use std::{
    sync::{atomic::{AtomicBool, Ordering}, mpsc::Sender, Arc},
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Debug, Clone)]
pub enum BackendEvent {
    Connected { width: u32, height: u32 },
    LootUpdated { total: crate::loot::LootDelta, per_hour: crate::loot::LootDelta },
    Status(String),
    Error(String),
    Stopped,
}

/// Owns the long-running backend worker. Stop is signal-based and does not impose a
/// session duration; the worker exits only when the user requests it or an error occurs.
pub struct FarmingBackend {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl FarmingBackend {
    pub fn start(ocr: OcrEngine, events: Sender<BackendEvent>) -> Result<Self> {
        let window = GameWindow::find()?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("cocbot-backend".into())
            .spawn(move || {
                let (width, height) = window.dimensions();
                let _ = events.send(BackendEvent::Connected { width, height });
                let _input = match create_input() {
                    Ok(input) => input,
                    Err(error) => {
                        let _ = events.send(BackendEvent::Error(error.to_string()));
                        let _ = events.send(BackendEvent::Stopped);
                        return;
                    }
                };

                let mut tracker = LootTracker::new();
                let mut previous_read: Option<ResourceSnapshot> = None;
                let mut stable_count = 0_u8;

                while !worker_stop.load(Ordering::Relaxed) {
                    match read_hud(&window, &ocr) {
                        Ok(Some(snapshot)) => {
                            if previous_read == Some(snapshot) {
                                stable_count = stable_count.saturating_add(1);
                            } else {
                                previous_read = Some(snapshot);
                                stable_count = 1;
                            }
                            // Require consecutive matching reads before updating totals.
                            if stable_count >= 2 {
                                tracker.observe(snapshot);
                                let summary = tracker.summary();
                                let _ = events.send(BackendEvent::LootUpdated {
                                    total: summary.total,
                                    per_hour: summary.per_hour,
                                });
                                stable_count = 0;
                            }
                        }
                        Ok(None) => {
                            stable_count = 0;
                            previous_read = None;
                        }
                        Err(error) => {
                            let _ = events.send(BackendEvent::Status(format!("OCR read skipped: {error}")));
                        }
                    }

                    for _ in 0..10 {
                        if worker_stop.load(Ordering::Relaxed) { break; }
                        thread::sleep(Duration::from_millis(100));
                    }
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

/// Crops the top-right HUD before OCR. Restricting OCR to this ROI is substantially
/// faster and more reliable than running recognition over the full game screenshot.
fn read_hud(window: &GameWindow, ocr: &OcrEngine) -> Result<Option<ResourceSnapshot>> {
    let frame = window.capture()?;
    let (width, height) = (frame.width(), frame.height());
    let left = width * 72 / 100;
    let top = height * 3 / 100;
    let roi_width = width.saturating_sub(left);
    let roi_height = height * 27 / 100;
    if roi_width == 0 || roi_height == 0 { return Ok(None); }
    let roi = imageops::crop_imm(&frame, left, top, roi_width, roi_height).to_image();
    let result = ocr.recognise(&DynamicImage::ImageRgba8(roi), OcrProfile::HudNumbers)?;
    let Some([gold, elixir, dark_elixir]) = OcrEngine::parse_hud_triplet(&result.text) else {
        return Ok(None);
    };
    Ok(Some(ResourceSnapshot { gold, elixir, dark_elixir }))
}
