//! BluForge core: disc/optical device discovery, AACS key management, disc
//! inspection and the rip pipeline. Framework-agnostic so it can back a CLI,
//! a Tauri desktop app, or a headless service.

use std::path::PathBuf;

pub mod author;
pub mod bdjo;
pub mod bdmv;
pub mod bdplus;
pub mod binaries;
pub mod burn;
pub mod clone;
pub mod config;
pub mod covers;
pub mod device;
pub mod disc;
pub mod doctor;
pub mod dvd;
pub mod iso;
pub mod keys;
pub mod menu;
pub mod metadata;
pub mod native;
pub mod rip;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod scsi;
pub mod template;
pub mod tsmuxer;
pub mod tv;
pub mod udf;
pub mod udf_portable;
pub mod verify;

pub use clone::{CloneProgress, CloneRequest};
pub use config::AppConfig;
pub use covers::{CoverHit, CoverSource};
pub use disc::{Chapter, DecryptBackend, Disc, DiscInfo, StreamInfo, Title};
pub use dvd::{DvdInfo, DvdProgress, DvdRipRequest, DvdTitle};
pub use iso::{IsoFormat, IsoProgress, IsoRequest};
pub use menu::{DiscMenu, MenuTitle, NavIndex, NavTarget, NavType};
pub use metadata::{lookup_movie, Poster};
pub use rip::{Container, EncodeSettings, RipMode, RipProgress, RipRequest, VideoCodec};
pub use template::{render as render_template, Vars as TemplateVars};

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

/// Per-user config/state directory (`~/Library/Application Support/keepsake-atelier`
/// on macOS).
///
/// A pre-rename `bluforge` directory is moved into place once, so existing
/// keys, menus, queue and settings carry over.
pub fn config_dir() -> PathBuf {
    let dir = if let Some(proj) = directories::ProjectDirs::from("", "", "keepsake-atelier") {
        proj.config_dir().to_path_buf()
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home).join(".config/keepsake-atelier")
    };
    migrate_legacy_dir(&dir);
    dir
}

fn migrate_legacy_dir(new: &std::path::Path) {
    if new.exists() {
        return;
    }
    let Some(old) =
        directories::ProjectDirs::from("", "", "bluforge").map(|p| p.config_dir().to_path_buf())
    else {
        return;
    };
    if !old.exists() {
        return;
    }
    if let Some(parent) = new.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Both paths sit in the same parent directory, so this is an atomic rename.
    if std::fs::rename(&old, new).is_err() && !new.exists() {
        tracing::warn!(
            old = %old.display(),
            new = %new.display(),
            "could not move the pre-rename data directory; starting with a fresh one"
        );
    }
}

/// Look up a setting from the environment, accepting both the current
/// `KEEPSAKE_*` name and the pre-rename `BLUFORGE_*` name.
pub fn env_compat(name: &str) -> Option<String> {
    std::env::var(format!("KEEPSAKE_{name}"))
        .or_else(|_| std::env::var(format!("BLUFORGE_{name}")))
        .ok()
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
