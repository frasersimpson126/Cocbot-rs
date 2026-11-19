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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PixelRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelRegion {
    pub fn is_valid_for(self, frame_width: u32, frame_height: u32) -> bool {
        self.width > 0 && self.height > 0
            && self.x.checked_add(self.width).is_some_and(|right| right <= frame_width)
            && self.y.checked_add(self.height).is_some_and(|bottom| bottom <= frame_height)
    }
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
    /// Exact screenshot-pixel crops selected in Settings.
    pub hud_roi: Option<PixelRegion>,
    pub opponent_loot_roi: Option<PixelRegion>,
    pub troop_bar_roi: Option<PixelRegion>,
    pub battle_controls_roi: Option<PixelRegion>,
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
            hud_roi: None,
            opponent_loot_roi: None,
            troop_bar_roi: None,
            battle_controls_roi: None,
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


#[cfg(test)]
mod pixel_region_tests {
    use super::*;

    #[test]
    fn pixel_region_accepts_only_non_empty_regions_inside_frame() {
        let region = PixelRegion { x: 100, y: 50, width: 200, height: 80 };
        assert!(region.is_valid_for(1280, 720));
        assert!(!region.is_valid_for(250, 720));
        assert!(!PixelRegion { width: 0, ..region }.is_valid_for(1280, 720));
        assert!(!PixelRegion { y: 700, ..region }.is_valid_for(1280, 720));
    }
    #[test]
    fn pixel_calibration_regions_round_trip_through_toml() {
        let config = AppConfig {
            hud_roi: Some(PixelRegion { x: 1200, y: 20, width: 300, height: 180 }),
            opponent_loot_roi: Some(PixelRegion { x: 80, y: 200, width: 500, height: 140 }),
            ..AppConfig::default()
        };
        let encoded = toml::to_string(&config).unwrap();
        let decoded: AppConfig = toml::from_str(&encoded).unwrap();
        assert_eq!(decoded.hud_roi, config.hud_roi);
        assert_eq!(decoded.opponent_loot_roi, config.opponent_loot_roi);
    }

    #[test]
    fn legacy_config_without_pixel_regions_uses_defaults() {
        let decoded: AppConfig = toml::from_str("").unwrap();
        assert_eq!(decoded.hud_roi, None);
        assert_eq!(decoded.opponent_loot_roi, None);
    }

}
