//! Managed tsMuxeR installation.
//!
//! Authoring needs tsMuxeR, which is not part of any package here. `install`
//! downloads the pinned upstream release for this platform, verifies its
//! SHA-256, extracts the single CLI binary into the data directory and checks
//! that it runs. The download is Apache-2.0; nothing is redistributed with the
//! app.

use std::io::{Cursor, Read};
use std::path::PathBuf;

use sha2::{Digest, Sha256};

use crate::CoreError;

/// Upstream release pinned by version and per-asset digest.
const VERSION: &str = "2.7.0";

struct Asset {
    url: &'static str,
    sha256: &'static str,
    /// Name inside the zip.
    entry: &'static str,
}

/// The release asset for this OS/architecture.
///
/// Upstream publishes an arm64 macOS build only, so an Intel Mac gets a clear
/// error instead of a binary that cannot run.
fn asset() -> Option<Asset> {
    let url = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Asset {
            url: "https://github.com/justdan96/tsMuxer/releases/download/2.7.0/tsMuxer-2.7.0-mac.zip",
            sha256: "bf90b608321f491b955fd9432000a8370c048e9c306e3a5833dfd75e5fa530e1",
            entry: "tsMuxeR",
        },
        ("linux", "x86_64") => Asset {
            url: "https://github.com/justdan96/tsMuxer/releases/download/2.7.0/tsMuxer-2.7.0-linux.zip",
            sha256: "ceaaa181ab70e201685b1e45260d337d1cbdb0aba1408d4fbc47e232a7e4c987",
            entry: "tsMuxeR",
        },
        ("windows", "x86_64") => Asset {
            url: "https://github.com/justdan96/tsMuxer/releases/download/2.7.0/tsMuxer-2.7.0-win64.zip",
            sha256: "e3cf117d2c6f01332188123641c63fe38ab9731ae7aa23645f6f9a261a7a301c",
            entry: "tsMuxeR.exe",
        },
        _ => return None,
    };
    Some(url)
}

/// The binary name used in the data directory.
pub fn file_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "tsMuxeR.exe"
    } else {
        "tsMuxeR"
    }
}

/// Where `install` puts the managed copy.
pub fn managed_path() -> PathBuf {
    crate::config_dir().join("bin").join(file_name())
}

/// Download and install the pinned tsMuxeR into the data directory.
pub fn install() -> Result<PathBuf, CoreError> {
    let asset =
        asset().ok_or_else(|| CoreError::Config(format!(
            "no managed tsMuxeR {VERSION} build for {} {}; install tsMuxeR yourself and put it on PATH",
            std::env::consts::OS,
            std::env::consts::ARCH
        )))?;

    let response = ureq::get(asset.url)
        .timeout(std::time::Duration::from_secs(600))
        .call()
        .map_err(|e| CoreError::Config(format!("download tsMuxeR: {e}")))?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(64 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|e| CoreError::Config(format!("read tsMuxeR download: {e}")))?;

    let digest = Sha256::digest(&bytes);
    let actual: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    if actual != asset.sha256 {
        return Err(CoreError::Config(format!(
            "tsMuxeR download failed its SHA-256 check (got {actual})"
        )));
    }

    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| CoreError::Config(format!("open tsMuxeR zip: {e}")))?;
    let mut entry = archive
        .by_name(asset.entry)
        .map_err(|e| CoreError::Config(format!("tsMuxeR zip entry {}: {e}", asset.entry)))?;
    let mut binary = Vec::with_capacity(entry.size() as usize);
    entry
        .read_to_end(&mut binary)
        .map_err(|e| CoreError::Config(format!("extract tsMuxeR: {e}")))?;

    let dest = managed_path();
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = dest.with_extension("part");
    std::fs::write(&tmp, &binary)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&tmp, &dest)?;

    // A binary that cannot run is worse than no install: fail here so doctor
    // and the author step never hand a broken path to the muxer.
    let out = std::process::Command::new(&dest)
        .arg("--version")
        .output()
        .map_err(|e| CoreError::Config(format!("run tsMuxeR: {e}")))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() || !text.contains("tsMuxeR") {
        let _ = std::fs::remove_file(&dest);
        return Err(CoreError::Config(format!(
            "the downloaded tsMuxeR did not run: {}",
            text.trim()
        )));
    }
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_managed_binary_is_named_for_the_platform() {
        let name = file_name();
        if cfg!(windows) {
            assert_eq!(name, "tsMuxeR.exe");
        } else {
            assert_eq!(name, "tsMuxeR");
        }
    }

    #[test]
    fn the_managed_path_sits_under_the_data_bin_dir() {
        let path = managed_path();
        assert!(path.ends_with(format!("bin/{name}", name = file_name())));
    }

    #[test]
    fn the_asset_matches_the_platform() {
        // Every release platform we build for must resolve, except Intel macOS.
        if cfg!(target_os = "macos") && cfg!(target_arch = "x86_64") {
            assert!(asset().is_none());
        } else if cfg!(any(
            target_os = "macos",
            target_os = "linux",
            target_os = "windows"
        )) {
            assert!(asset().is_some());
        }
    }
}
