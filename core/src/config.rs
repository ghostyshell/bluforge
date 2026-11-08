//! Persisted application settings.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::CoreError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_version")]
    pub version: u32,
    /// Output folder root; the output template is rendered beneath this.
    #[serde(default = "default_output_dir")]
    pub output_dir: PathBuf,
    /// Output path template (see `core::template`).
    #[serde(default = "default_template")]
    pub output_template: String,
    /// Output template used when ripping a disc as TV/anime episodes.
    #[serde(default = "default_tv_template")]
    pub tv_template: String,
    /// Subtitle tracks on TV rips: "all" keeps every track, "signs" keeps only
    /// the signs-&-songs track when there is English audio (else full English).
    #[serde(default = "default_tv_subs")]
    pub tv_subs: String,
    /// Refresh KEYDB.cfg automatically at launch.
    #[serde(default = "yes")]
    pub auto_update_keys: bool,
    /// Check GitHub releases for a newer BluForge at launch.
    #[serde(default = "yes")]
    pub auto_check_updates: bool,
    /// Last update check (unix seconds) to throttle.
    #[serde(default)]
    pub last_update_check: Option<u64>,
    /// Download cover art next to the rip when one is found.
    #[serde(default = "yes")]
    pub auto_cover: bool,
    /// Checkout of the hirescovers tooling; its manifests are read for search.
    #[serde(default = "default_hirescovers_dir")]
    pub hirescovers_dir: PathBuf,
    #[serde(default)]
    pub hirescovers_user: Option<String>,
    #[serde(default)]
    pub hirescovers_pass: Option<String>,
    /// Member cookie for hirescovers originals (falls back to the tooling's).
    #[serde(default)]
    pub hirescovers_cookie: Option<String>,
    /// Expose the app over MCP (localhost HTTP) so an agent can drive it.
    /// Off unless explicitly enabled; the app has no remote surface otherwise.
    #[serde(default)]
    pub mcp_enabled: bool,
    /// Port for the MCP server (localhost only).
    #[serde(default = "default_mcp_port")]
    pub mcp_port: u16,
}

pub fn default_mcp_port() -> u16 {
    8848
}

pub fn yes() -> bool {
    true
}

pub fn default_version() -> u32 {
    2
}

pub fn default_output_dir() -> PathBuf {
    directories::UserDirs::new()
        .and_then(|d| d.download_dir().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn default_hirescovers_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join("Code/hirescovers")
}

pub fn default_tv_subs() -> String {
    "all".to_string()
}

pub fn default_tv_template() -> String {
    "{series}/Season {season:02}/{series} - S{season:02}E{episode:02} - {episode_title}".to_string()
}

pub fn default_template() -> String {
    "{title} ({year})/{title} ({year}) - {resolution} {hdr}".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        let output_dir = directories::UserDirs::new()
            .and_then(|d| d.download_dir().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            version: 2,
            output_dir,
            output_template: default_template(),
            tv_template: default_tv_template(),
            tv_subs: default_tv_subs(),
            auto_update_keys: true,
            auto_check_updates: true,
            last_update_check: None,
            auto_cover: true,
            hirescovers_dir: default_hirescovers_dir(),
            hirescovers_user: None,
            hirescovers_pass: None,
            hirescovers_cookie: None,
            mcp_enabled: false,
            mcp_port: default_mcp_port(),
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
