use crate::strategy::ArmyStrategy;
use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Village {
    #[default]
    Home,
    BuilderBase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LootPriority {
    Gold,
    Elixir,
    DarkElixir,
    #[default]
    Balanced,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub village: Village,
    pub strategy: ArmyStrategy,
    pub loot_priority: LootPriority,
    pub minimum_gold: u64,
    pub minimum_elixir: u64,
    pub minimum_dark_elixir: u64,
    pub minimum_score_percent: u8,
    pub minimum_confidence_percent: u8,
    /// Opponent loot crop as percentages of the captured frame; zero size disables it.
    pub opponent_loot_x_percent: u8,
    pub opponent_loot_y_percent: u8,
    pub opponent_loot_width_percent: u8,
    pub opponent_loot_height_percent: u8,
    pub template_pack_calibrated: bool,
    pub opponent_loot_roi_calibrated: bool,
    pub tesseract_path: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            village: Village::Home,
            strategy: ArmyStrategy::SneakyGoblin,
            loot_priority: LootPriority::Balanced,
            minimum_gold: 300_000,
            minimum_elixir: 300_000,
            minimum_dark_elixir: 2_000,
            minimum_score_percent: 45,
            minimum_confidence_percent: 85,
            opponent_loot_x_percent: 0,
            opponent_loot_y_percent: 0,
            opponent_loot_width_percent: 0,
            opponent_loot_height_percent: 0,
            template_pack_calibrated: false,
            opponent_loot_roi_calibrated: false,
            tesseract_path: String::new(),
        }
    }
}

impl AppConfig {
    fn path() -> Result<PathBuf> {
        let dirs = ProjectDirs::from("uk", "FraserSimpson", "Cocbot-rs")
            .context("Could not resolve config directory")?;
        Ok(dirs.config_dir().join("config.toml"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            let config = Self::default();
            config.save()?;
            return Ok(config);
        }
        let raw = fs::read_to_string(path)?;
        toml::from_str(&raw).context("Invalid config.toml")
    }

    pub fn calibration_dir() -> Result<PathBuf> {
        let dirs = ProjectDirs::from("uk", "FraserSimpson", "Cocbot-rs")
            .context("Could not resolve application data directory")?;
        Ok(dirs.data_local_dir().join("calibration"))
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, toml::to_string_pretty(self)?)?;
        fs::rename(tmp, path)?;
        Ok(())
    }
}
