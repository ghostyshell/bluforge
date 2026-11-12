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
    /// Rip a disc automatically when it is inserted, or when a new file appears
    /// in `watch_dir`.
    #[serde(default)]
    pub auto_rip: bool,
    /// `main` rips the main feature; `all` rips every title at least
    /// `auto_rip_min_secs` long.
    #[serde(default = "default_auto_rip_mode")]
    pub auto_rip_mode: String,
    /// Shortest title `auto_rip_mode = "all"` will rip, in seconds.
    #[serde(default = "default_auto_rip_min_secs")]
    pub auto_rip_min_secs: u32,
    /// Eject the drive after an automatic rip finishes.
    #[serde(default)]
    pub auto_rip_eject: bool,
    /// Folder watched for new ISOs and BDMV folders (empty disables it).
    #[serde(default)]
    pub watch_dir: Option<PathBuf>,
    /// Show a desktop notification when a job finishes or fails.
    #[serde(default = "yes")]
    pub notify_native: bool,
    /// POST `{"title","body"}` here on completion or failure (Slack/Discord
    /// compatible when the service accepts a generic JSON body).
    #[serde(default)]
    pub notify_webhook: Option<String>,
    /// Keep the machine awake while a rip is running.
    #[serde(default = "yes")]
    pub prevent_sleep: bool,
}

pub fn default_auto_rip_mode() -> String {
    "main".into()
}

pub fn default_auto_rip_min_secs() -> u32 {
    600
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
            auto_rip: false,
            auto_rip_mode: default_auto_rip_mode(),
            auto_rip_min_secs: default_auto_rip_min_secs(),
            auto_rip_eject: false,
            watch_dir: None,
            notify_native: true,
            notify_webhook: None,
            prevent_sleep: true,
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
