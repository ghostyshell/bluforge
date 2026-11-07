//! Decrypted ISO / backup-folder output.
//!
//! The disc tree is exported with every encrypted stream decrypted (via
//! libbluray's VFS) and then wrapped in a UDF image. The `AACS`/`BDSVM`
//! directories are dropped so a re-opened image is treated as plaintext.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::disc::Disc;
use crate::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IsoFormat {
    /// UDF image file.
    Iso,
    /// Plain decrypted folder (BDMV tree), MakeMKV-backup style.
    Folder,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsoRequest {
    pub source: PathBuf,
    pub keydb: Option<PathBuf>,
    /// Output `.iso` file (Iso) or directory (Folder).
    pub dest: PathBuf,
    pub format: IsoFormat,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IsoProgress {
    pub stage: String,
    pub percent: f64,
    pub files: u64,
    pub message: String,
}

fn sanitize_label(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' })
        .collect();
    let trimmed = cleaned.trim_matches('_');
    if trimmed.is_empty() {
        "BLURAY".into()
    } else {
        trimmed.chars().take(32).collect()
    }
}

fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let cand = dir.join(bin);
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

/// Build a UDF image from a directory, preferring macOS `hdiutil` (it handles
/// files >4 GB) and falling back to `mkisofs`/`genisoimage`.
///
/// The fallback needs `-iso-level 3`: without it `mkisofs` *silently skips*
/// every file >=4 GB ("too large ... - ignoring"), which produced an image
/// missing the main movie.
fn build_iso(dir: &Path, dest: &Path, label: &str) -> Result<(), CoreError> {
    let _ = std::fs::remove_file(dest);
    let mut last_error: Option<String> = None;

    if which("hdiutil").is_some() {
        let out = Command::new("hdiutil")
            .args(["makehybrid", "-udf", "-udf-volume-name", label, "-iso-volume-name", label])
            .arg("-o")
            .arg(dest)
            .arg(dir)
            .output()
            .map_err(|e| CoreError::Disc(format!("hdiutil: {e}")))?;
        if out.status.success() {
            return Ok(());
        }
        last_error = Some(format!(
            "hdiutil failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }

    if let Some(mk) = which("mkisofs").or_else(|| which("genisoimage")) {
        let out = Command::new(mk)
            .arg("-udf")
            .arg("-iso-level")
            .arg("3")
            .arg("-V")
            .arg(label)
            .arg("-o")
            .arg(dest)
            .arg(dir)
            .output()
            .map_err(|e| CoreError::Disc(format!("mkisofs: {e}")))?;
        if out.status.success() {
            return Ok(());
        }
        last_error = Some(format!(
            "mkisofs failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }

    Err(CoreError::Disc(
        last_error.unwrap_or_else(|| "no ISO builder found (install cdrtools for mkisofs)".into()),
    ))
}

/// The device node backing a mount point, if any.
fn device_for_mount(mp: &str) -> Option<String> {
    let out = Command::new("mount").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        // "/dev/disk5 on /Volumes/DISC (udf, ...)"
        let mut it = line.split_whitespace();
        let dev = it.next().unwrap_or("");
        if it.next() != Some("on") {
            continue;
        }
        if it.next().unwrap_or("") == mp && dev.starts_with("/dev/disk") {
            return Some(dev.to_string());
        }
    }
    None
}

/// Pick the source used for the decrypted tree walk.
///
/// The device is required: libbluray's decryption layer and MakeMKV's `libmmbd`
/// both need it, whereas a *mounted* volume is served by libbluray's overlay and
/// hands back the raw (still AACS-encrypted) bytes. So a mounted volume - or a
/// `/dev/diskN` node - is mapped to its raw device. BD root enumeration on a
/// device is handled by the root probe in the shim.
fn export_source(source: &Path) -> Result<PathBuf, CoreError> {
    let s = source.to_string_lossy();
    if s.starts_with("/dev/rdisk") {
        return Ok(source.to_path_buf());
    }
    let dev = if s.starts_with("/dev/disk") {
        Some(s.to_string())
    } else {
        device_for_mount(&s)
    };
    match dev {
        Some(d) => {
            let raw = d.replacen("/dev/disk", "/dev/rdisk", 1);
            eprintln!("bf_export: {} -> {raw}", source.display());
            Ok(PathBuf::from(raw))
        }
        None => Ok(source.to_path_buf()),
    }
}

/// Run a decrypted ISO/folder export. `on_progress` is called from this thread.
pub fn run<F>(req: &IsoRequest, mut on_progress: F) -> Result<(), CoreError>
where
    F: FnMut(IsoProgress),
{
    let source = export_source(&req.source)?;
    let disc = Disc::open(&source, req.keydb.as_deref())?;
    let info = disc.info()?;
    let label = req
        .label
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            crate::device::source_name(&req.source.to_string_lossy(), &info.name)
        });
    let label = sanitize_label(&label);

    let (work, temp) = match req.format {
        IsoFormat::Folder => (req.dest.clone(), false),
        IsoFormat::Iso => {
            let parent = req.dest.parent().unwrap_or(Path::new("."));
            std::fs::create_dir_all(parent)?;
            (parent.join(format!(".{}.bluforge-tmp", label)), true)
        }
    };
    if work.exists() {
        std::fs::remove_dir_all(&work).ok();
    }
    std::fs::create_dir_all(&work)?;

    on_progress(IsoProgress {
        stage: "exporting".into(),
        percent: 2.0,
        files: 0,
        message: "reading and decrypting streams".into(),
    });
    let files = disc.export_decrypted(&work)?;
    on_progress(IsoProgress {
        stage: "exporting".into(),
        percent: 70.0,
        files,
        message: format!("{files} files exported"),
    });

    if req.format == IsoFormat::Iso {
        on_progress(IsoProgress {
            stage: "building".into(),
            percent: 80.0,
            files,
            message: format!("writing UDF image {label}"),
        });
        build_iso(&work, &req.dest, &label)?;
        if temp {
            std::fs::remove_dir_all(&work).ok();
        }
    }

    on_progress(IsoProgress {
        stage: "done".into(),
        percent: 100.0,
        files,
        message: "done".into(),
    });
    Ok(())
}
