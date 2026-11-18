mod app;
pub mod config;
pub mod backend;
pub mod platform;
pub mod loot;
pub mod ocr;
pub mod strategy;
pub mod vision;
pub mod battle;
pub mod target;
pub mod deployment;
pub mod raid;

use anyhow::Result;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = config::AppConfig::load()?;
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([900.0, 620.0])
            .with_title("Cocbot-rs"),
        ..Default::default()
    };
    eframe::run_native(
        "Cocbot-rs",
        options,
        Box::new(move |cc| Ok(Box::new(app::CocbotApp::new(cc, config)))),
    )
    .map_err(|error| anyhow::anyhow!("GUI failed: {error}"))
}
