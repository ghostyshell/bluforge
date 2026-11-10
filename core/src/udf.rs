//! UDF 2.50 image creation and mounting with macOS's own tools.
//!
//! Blu-ray needs UDF 2.50 with a metadata partition. `newfs_udf` (built into
//! macOS) can create exactly that in a plain file, and `hdiutil` mounts it
//! read-write so the BDMV tree can be laid down. No third-party UDF tool is
//! involved.
//!
//! This is what makes a multi-title ISO possible: tsMuxeR only writes one
//! playlist per image, so we author the merged BDMV into a mounted UDF 2.50
//! image instead.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::CoreError;

const NEWFS_UDF: &str = "/sbin/newfs_udf";
const HDIUTIL: &str = "/usr/bin/hdiutil";

/// Both native tools are present on this system.
pub fn available() -> bool {
    Path::new(NEWFS_UDF).is_file() && Path::new(HDIUTIL).is_file()
}

/// Uppercase, alphanumeric/underscore volume label, at most 32 characters.
pub fn label(s: &str) -> String {
    let out: String = s
        .to_uppercase()
        .chars()
        .filter_map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                Some(c)
            } else if c.is_whitespace() {
                Some('_')
            } else {
                None
            }
        })
        .take(32)
        .collect();
    if out.is_empty() {
        "KEEPSAKE".into()
    } else {
        out
    }
}

/// Total byte size of a directory tree.
pub fn tree_size(path: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(path) else {
        return 0;
    };
    let mut total = 0;
    for e in rd.flatten() {
        let p = e.path();
        match std::fs::metadata(&p) {
            Ok(m) if m.is_dir() => total += tree_size(&p),
            Ok(m) => total += m.len(),
            Err(_) => {}
        }
    }
    total
}

/// Image size: the payload plus 5% and 16 MiB of filesystem overhead, rounded
/// up to a MiB and never below 16 MiB.
pub fn image_size(payload: u64) -> u64 {
    const MIB: u64 = 1024 * 1024;
    let want = payload + payload / 20 + 16 * MIB;
    want.max(16 * MIB).div_ceil(MIB) * MIB
}

/// Create and format a UDF 2.50 image at `image` (sparse, `size` bytes).
pub fn create(image: &Path, size: u64, volume: &str) -> Result<(), CoreError> {
    if !available() {
        return Err(CoreError::Disc(
            "macOS newfs_udf/hdiutil are missing; cannot write a UDF 2.50 image".into(),
        ));
    }
    if let Some(dir) = image.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    // Sparse: only the filesystem metadata is written until files are copied in.
    let f = std::fs::File::create(image)?;
    f.set_len(size)?;
    drop(f);

    let out = Command::new(NEWFS_UDF)
        .args(["-r", "2.50", "-m", "blk", "-t", "ow", "-b", "2048", "-v"])
        .arg(label(volume))
        .arg(image)
        .output()
        .map_err(|e| CoreError::Disc(format!("newfs_udf: {e}")))?;
    if !out.status.success() {
        let _ = std::fs::remove_file(image);
        return Err(CoreError::Disc(format!(
            "newfs_udf failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}

/// Mount the image read-write and return the mount point.
pub fn mount(image: &Path) -> Result<PathBuf, CoreError> {
    let out = Command::new(HDIUTIL)
        .args(["attach", "-nobrowse", "-readwrite"])
        .arg(image)
        .output()
        .map_err(|e| CoreError::Disc(format!("hdiutil: {e}")))?;
    let text = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() {
        return Err(CoreError::Disc(format!(
            "hdiutil attach failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    for line in text.lines() {
        if let Some(pos) = line.find("/Volumes/") {
            let mp = line[pos..].trim().to_string();
            if Path::new(&mp).is_dir() {
                return Ok(PathBuf::from(mp));
            }
        }
    }
    Err(CoreError::Disc(format!(
        "hdiutil mounted the image but no /Volumes path was reported: {}",
        text.trim()
    )))
}

/// Unmount a previously attached image.
pub fn unmount(mount: &Path) -> Result<(), CoreError> {
    let out = Command::new(HDIUTIL)
        .arg("detach")
        .arg(mount)
        .output()
        .map_err(|e| CoreError::Disc(format!("hdiutil detach: {e}")))?;
    if !out.status.success() {
        return Err(CoreError::Disc(format!(
            "hdiutil detach failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}

/// Create, format, mount, run `fill` against the mount point, unmount, and
/// clean up on failure.
pub fn with_image<T, F>(image: &Path, size: u64, volume: &str, fill: F) -> Result<T, CoreError>
where
    F: FnOnce(&Path) -> Result<T, CoreError>,
{
    create(image, size, volume)?;
    let mount = match mount(image) {
        Ok(m) => m,
        Err(e) => {
            let _ = std::fs::remove_file(image);
            return Err(e);
        }
    };
    let result = fill(&mount);
    let detach = unmount(&mount);
    match result {
        Ok(v) => {
            detach?;
            Ok(v)
        }
        Err(e) => {
            let _ = detach;
            let _ = std::fs::remove_file(image);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_sizes_are_sane() {
        assert_eq!(label("my disc!"), "MY_DISC");
        assert_eq!(label(""), "KEEPSAKE");
        assert_eq!(label(&"x".repeat(40)).len(), 32);
        assert!(image_size(0) >= 16 * 1024 * 1024);
        let big = 10 * 1024 * 1024 * 1024u64;
        assert!(image_size(big) > big);
        assert_eq!(image_size(big) % (1024 * 1024), 0);
    }

    #[test]
    fn tree_size_adds_files() {
        let dir = std::env::temp_dir().join(format!("keepsake-udf-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("BDMV")).unwrap();
        std::fs::write(dir.join("BDMV/index.bdmv"), vec![0u8; 100]).unwrap();
        std::fs::write(dir.join("BDMV/MovieObject.bdmv"), vec![0u8; 50]).unwrap();
        assert_eq!(tree_size(&dir), 150);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
