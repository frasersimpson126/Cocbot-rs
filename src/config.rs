use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub reserve_builders: u8,
    pub idle_recheck_seconds: u64,
    pub tesseract_path: String,
}
impl Default for AppConfig {
    fn default() -> Self {
        Self { reserve_builders: 1, idle_recheck_seconds: 300, tesseract_path: String::new() }
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
    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, toml::to_string_pretty(self)?)?;
        fs::rename(tmp, path)?;
        Ok(())
    }
}
