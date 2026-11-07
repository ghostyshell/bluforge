//! BluForge core: disc/optical device discovery, AACS key management, disc
//! inspection and the rip pipeline. Framework-agnostic so it can back a CLI,
//! a Tauri desktop app, or a headless service.

use std::path::PathBuf;

pub mod config;
pub mod device;
pub mod disc;
pub mod iso;
pub mod keys;
pub mod metadata;
pub mod rip;
pub mod template;

pub use config::AppConfig;
pub use disc::{Chapter, Disc, DiscInfo, StreamInfo, Title};
pub use iso::{IsoFormat, IsoProgress, IsoRequest};
pub use metadata::{lookup_movie, Poster};
pub use template::{render as render_template, Vars as TemplateVars};
pub use rip::{Container, EncodeSettings, RipMode, RipProgress, RipRequest, VideoCodec};

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("keys: {0}")]
    Keys(String),
    #[error("device: {0}")]
    Device(String),
    #[error("disc: {0}")]
    Disc(String),
    #[error("config: {0}")]
    Config(String),
}

/// Per-user config/state directory (`~/Library/Application Support/bluforge`
/// on macOS).
pub fn config_dir() -> PathBuf {
    if let Some(proj) = directories::ProjectDirs::from("", "", "bluforge") {
        proj.config_dir().to_path_buf()
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home).join(".config/bluforge")
    }
}

/// Everything the UI needs to render a first screen.
#[derive(Debug, Default, serde::Serialize)]
pub struct CoreSnapshot {
    pub version: String,
    pub devices: Vec<device::BlockDevice>,
    pub optical: Vec<device::BlockDevice>,
    pub keys: Option<keys::KeyDbStatus>,
}

pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

pub fn snapshot() -> CoreSnapshot {
    CoreSnapshot {
        version: version(),
        devices: device::list(),
        optical: device::list_optical(),
        keys: Some(keys::status()),
    }
}
