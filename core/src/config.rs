//! Persisted application settings.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::CoreError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub version: u32,
    /// Output folder root; per-job subfolders are created beneath this.
    pub output_dir: PathBuf,
    /// Refresh KEYDB.cfg automatically at launch.
    pub auto_update_keys: bool,
    /// Check GitHub releases for a newer BluForge at launch.
    pub auto_check_updates: bool,
    /// Last update check (unix seconds) to throttle.
    pub last_update_check: Option<u64>,
}

impl Default for AppConfig {
    fn default() -> Self {
        let output_dir = directories::UserDirs::new()
            .and_then(|d| d.video_dir().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."))
            .join("BluForge");
        Self {
            version: 1,
            output_dir,
            auto_update_keys: true,
            auto_check_updates: true,
            last_update_check: None,
        }
    }
}

pub fn config_file() -> PathBuf {
    crate::config_dir().join("config.json")
}

impl AppConfig {
    pub fn load() -> Self {
        match fs::read_to_string(config_file()) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<(), CoreError> {
        let dir = crate::config_dir();
        fs::create_dir_all(&dir)?;
        let s = serde_json::to_string_pretty(self).map_err(|e| CoreError::Config(e.to_string()))?;
        fs::write(config_file(), s)?;
        Ok(())
    }
}
