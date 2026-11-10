//! Burn a Blu-ray ISO to an optical writer.
//!
//! Uses `growisofs` (dvd+rw-tools) when available, falling back to `cdrecord`.
//! `--dry-run` prints the command instead of writing; the CLI requires `--yes`
//! to actually write. Cancelling cannot undo what has already been written, so
//! the UI/CLI warns about that before starting.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::CoreError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurnRequest {
    /// The `.iso` to write (e.g. produced by `author --out disc.iso`).
    pub image: PathBuf,
    /// Raw device node, e.g. `/dev/rdisk4`.
    pub device: PathBuf,
    /// Write speed (e.g. `4` for 4x). Omit for the drive's default.
    #[serde(default)]
    pub speed: Option<u32>,
    /// Print the command instead of writing.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BurnProgress {
    pub percent: f64,
    pub message: String,
    pub done: bool,
}

fn in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// The writer front-end that will be used.
pub fn tool() -> Option<PathBuf> {
    in_path("growisofs").or_else(|| in_path("cdrecord"))
}

/// Validate the parts that do not require hardware (used by dry runs).
fn check_image(req: &BurnRequest) -> Result<(), CoreError> {
    if !req.image.is_file() {
        return Err(CoreError::Disc(format!(
            "image not found: {}",
            req.image.display()
        )));
    }
    if std::fs::metadata(&req.image).map(|m| m.len()).unwrap_or(0) == 0 {
        return Err(CoreError::Disc("image is empty".into()));
    }
    let dev = req.device.to_string_lossy();
    if !dev.starts_with("/dev/") {
        return Err(CoreError::Disc(format!(
            "device must be a /dev/rdiskN node, got {dev}"
        )));
    }
    Ok(())
}

/// Validate everything before doing anything destructive.
pub fn check(req: &BurnRequest) -> Result<(), CoreError> {
    check_image(req)?;
    let dev = req.device.to_string_lossy();
    if !Path::new(&req.device).exists() {
        return Err(CoreError::Disc(format!("device not present: {dev}")));
    }
    if let Some(mp) = crate::device::mount_point_for_device(&dev) {
        return Err(CoreError::Disc(format!(
            "{dev} is mounted at {mp}; unmount the disc before burning"
        )));
    }
    Ok(())
}

fn command_with(
    growisofs: Option<&Path>,
    cdrecord: Option<&Path>,
    req: &BurnRequest,
) -> Result<(PathBuf, Vec<String>), CoreError> {
    if let Some(g) = growisofs {
        let mut args = vec![
            "-Z".to_string(),
            format!("{}={}", req.device.display(), req.image.display()),
        ];
        if let Some(s) = req.speed {
            args.push(format!("-speed={s}"));
        }
        return Ok((g.to_path_buf(), args));
    }
    if let Some(c) = cdrecord {
        let mut args = vec!["-v".to_string(), format!("dev={}", req.device.display())];
        if let Some(s) = req.speed {
            args.push(format!("speed={s}"));
        }
        args.push(req.image.display().to_string());
        return Ok((c.to_path_buf(), args));
    }
    Err(CoreError::Disc(
        "no writer front-end: install growisofs (brew install dvd+rw-tools) or cdrecord".into(),
    ))
}

/// The command that would be run.
pub fn command(req: &BurnRequest) -> Result<(PathBuf, Vec<String>), CoreError> {
    command_with(
        in_path("growisofs").as_deref(),
        in_path("cdrecord").as_deref(),
        req,
    )
}

fn parse_percent(line: &str) -> Option<f64> {
    let i = line.find('%')?;
    let start = line[..i]
        .rfind(|c: char| !(c.is_ascii_digit() || c == '.'))
        .map(|j| j + 1)
        .unwrap_or(0);
    line[start..i].trim().parse::<f64>().ok()
}

/// Burn the image. With `dry_run`, only reports the command.
pub fn run<F>(req: &BurnRequest, cancel: &AtomicBool, mut on_progress: F) -> Result<(), CoreError>
where
    F: FnMut(BurnProgress),
{
    if req.dry_run {
        check_image(req)?;
    } else {
        check(req)?;
    }
    let (bin, args) = command(req)?;
    if req.dry_run {
        on_progress(BurnProgress {
            percent: 0.0,
            message: format!("would run: {} {}", bin.display(), args.join(" ")),
            done: true,
        });
        return Ok(());
    }

    let mut child = Command::new(&bin)
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| CoreError::Disc(format!("{}: {e}", bin.display())))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| CoreError::Disc("writer stderr".into()))?;

    let mut last = -1.0f64;
    let mut tail: Vec<String> = Vec::new();
    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
        }
        if let Some(p) = parse_percent(&line) {
            if (p - last).abs() >= 0.5 {
                last = p;
                on_progress(BurnProgress {
                    percent: p,
                    message: "writing".into(),
                    done: false,
                });
            }
        }
        tail.push(line);
        if tail.len() > 12 {
            tail.remove(0);
        }
    }
    let status = child
        .wait()
        .map_err(|e| CoreError::Disc(format!("writer: {e}")))?;
    if cancel.load(Ordering::Relaxed) {
        return Err(CoreError::Disc(
            "burn cancelled (the disc may be partially written)".into(),
        ));
    }
    if !status.success() {
        return Err(CoreError::Disc(format!(
            "writer failed: {}",
            tail.last().map(String::as_str).unwrap_or("see log")
        )));
    }
    on_progress(BurnProgress {
        percent: 100.0,
        message: "burned".into(),
        done: true,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> BurnRequest {
        BurnRequest {
            image: PathBuf::from("/tmp/disc.iso"),
            device: PathBuf::from("/dev/rdisk9"),
            speed: Some(4),
            dry_run: true,
        }
    }

    #[test]
    fn builds_growisofs_and_cdrecord_commands() {
        let (bin, args) = command_with(Some(Path::new("/bin/growisofs")), None, &req()).unwrap();
        assert_eq!(bin, PathBuf::from("/bin/growisofs"));
        assert_eq!(args, vec!["-Z", "/dev/rdisk9=/tmp/disc.iso", "-speed=4"]);

        let (bin, args) = command_with(None, Some(Path::new("/bin/cdrecord")), &req()).unwrap();
        assert_eq!(bin, PathBuf::from("/bin/cdrecord"));
        assert_eq!(
            args,
            vec!["-v", "dev=/dev/rdisk9", "speed=4", "/tmp/disc.iso"]
        );

        assert!(command_with(None, None, &req()).is_err());
    }

    #[test]
    fn parses_growisofs_progress() {
        assert_eq!(
            parse_percent("  12.5% done, estimate finish ..."),
            Some(12.5)
        );
        assert_eq!(parse_percent("100.0% done"), Some(100.0));
        assert_eq!(parse_percent("no percent here"), None);
    }

    #[test]
    fn rejects_a_missing_image() {
        let mut r = req();
        r.image = PathBuf::from("/tmp/definitely-not-there.iso");
        assert!(check_image(&r).is_err());
    }
}
