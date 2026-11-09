//! BD+ VM configuration for libbdplus.
//!
//! libbdplus ships no VM data, so a BD+ disc needs a `vm0/` config (memory map,
//! player AES/ECDSA keys, device discovery dumps) in its config directory.
//! Homebrew's libbdplus looks under `~/Library/Preferences/bdplus/vm0/` on
//! macOS. This fetches the public `bdplus-vm0.bz2` and unpacks it there, so BD+
//! discs come off MakeMKV too.

use std::io::Read;
use std::path::PathBuf;

use crate::CoreError;

const URL: &str =
    "https://web.archive.org/web/2016id_/http://www.labdv.com/aacs/libbdplus/bdplus-vm0.bz2";

/// libbdplus's per-user config root (`~/Library/Preferences` on macOS).
fn pref_root() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join("Library/Preferences")
}

/// The directory libbdplus probes for VM data.
pub fn config_dir() -> PathBuf {
    pref_root().join("bdplus/vm0")
}

/// True when the VM config is present (libbdplus needs `memory.map`).
pub fn installed() -> bool {
    config_dir().join("memory.map").is_file()
}

/// Download and unpack the BD+ VM data. Idempotent.
pub fn install() -> Result<PathBuf, CoreError> {
    let resp = ureq::get(URL)
        .timeout(std::time::Duration::from_secs(120))
        .call()
        .map_err(|e| CoreError::Config(format!("GET {URL}: {e}")))?;
    let mut body = Vec::new();
    resp.into_reader()
        .read_to_end(&mut body)
        .map_err(|e| CoreError::Config(e.to_string()))?;
    if !body.starts_with(b"BZh") {
        return Err(CoreError::Config(
            "unexpected BD+ VM download (not bzip2)".into(),
        ));
    }

    let tmp = std::env::temp_dir().join("bluforge-bdplus-vm0.bz2");
    std::fs::write(&tmp, &body)?;
    let status = std::process::Command::new("tar")
        .arg("-xjf")
        .arg(&tmp)
        .arg("-C")
        .arg(pref_root())
        .status()?;
    let _ = std::fs::remove_file(&tmp);
    if !status.success() {
        return Err(CoreError::Config("tar failed to unpack BD+ VM data".into()));
    }
    if !installed() {
        return Err(CoreError::Config(
            "BD+ VM data unpacked but memory.map is missing".into(),
        ));
    }
    Ok(config_dir())
}
