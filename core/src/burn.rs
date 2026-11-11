//! Burn a Blu-ray ISO to an optical writer.
//!
//! macOS uses the native `hdiutil burn`, Linux uses `growisofs` with a
//! `cdrecord` fallback, and Windows drives the built-in IMAPI2 stack through
//! PowerShell. `--dry-run` reports what would happen instead of writing; the
//! CLI requires `--yes` to actually write. Cancelling cannot undo what has
//! already been written, so the UI/CLI warns about that before starting.

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
    /// Raw device node for growisofs, e.g. `/dev/rdisk4`. Omit to use the
    /// native `hdiutil burn`, which picks the writer itself.
    #[serde(default)]
    pub device: Option<PathBuf>,
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
    #[cfg(target_os = "windows")]
    {
        // IMAPI2 is driven through PowerShell and ships with Windows.
        return in_path("powershell.exe").or_else(|| in_path("powershell"));
    }
    #[cfg(not(target_os = "windows"))]
    {
        in_path("growisofs").or_else(|| in_path("cdrecord"))
    }
}

/// A PowerShell script that writes the image with Windows' own IMAPI2 stack.
/// The placeholders are substituted by `windows_script`; keeping it as a raw
/// string avoids doubling every PowerShell brace for `format!`.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const WINDOWS_BURN_SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
$image = __IMAGE__
$want = __DEVICE__
$master = New-Object -ComObject IMAPI2.MsftDiscMaster2
if ($master.Count -eq 0) { throw 'no optical recorder found' }
$recorder = $null
for ($i = 0; $i -lt $master.Count; $i++) {
  $candidate = New-Object -ComObject IMAPI2.MsftDiscRecorder2
  $candidate.InitializeDiscRecorder($master.Item($i))
  $hit = $false
  if ($want) {
    foreach ($p in @($candidate.VolumePathNames)) {
      if ($p -and $p.StartsWith($want.TrimEnd(':'))) { $hit = $true }
    }
  }
  if (-not $want -or $hit) { $recorder = $candidate; break }
}
if ($null -eq $recorder) { throw "no recorder matches $want" }
$format = New-Object -ComObject IMAPI2.MsftDiscFormat2Data
if (-not $format.IsCurrentMediaSupported($recorder)) { throw 'the inserted media is not writable by this drive' }
$format.Recorder = $recorder
$format.ClientName = 'Keepsake Atelier'
$format.ForceMediaToBeClosed = $true
if ($format.MediaPhysicallyWriteProtected) { throw 'the disc is write protected' }
__SPEED__
$stream = New-Object -ComObject ADODB.Stream
$stream.Type = 1
$stream.Open()
$stream.LoadFromFile($image)
try { $format.Write($stream) } finally { $stream.Close() }
Write-Output 'BURN-OK'
"#;

fn ps_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn windows_script(req: &BurnRequest) -> String {
    let image = ps_quote(&req.image.display().to_string());
    let device = req
        .device
        .as_ref()
        .map(|d| ps_quote(&d.display().to_string()))
        .unwrap_or_else(|| "''".into());
    let speed = req
        .speed
        .map(|s| format!("$format.SetWriteSpeed({s})"))
        .unwrap_or_default();
    WINDOWS_BURN_SCRIPT
        .replace("__IMAGE__", &image)
        .replace("__DEVICE__", &device)
        .replace("__SPEED__", &speed)
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
    if let Some(device) = &req.device {
        let dev = device.to_string_lossy();
        #[cfg(target_os = "windows")]
        let shaped =
            dev.len() >= 2 && dev.as_bytes()[1] == b':' && dev.as_bytes()[0].is_ascii_alphabetic();
        #[cfg(not(target_os = "windows"))]
        let shaped = dev.starts_with("/dev/");
        if !shaped {
            return Err(CoreError::Disc(if cfg!(target_os = "windows") {
                format!("device must be a drive letter, e.g. D:, got {dev}")
            } else {
                format!("device must be a /dev/rdiskN node, got {dev}")
            }));
        }
    }
    Ok(())
}

/// Validate everything before doing anything destructive.
pub fn check(req: &BurnRequest) -> Result<(), CoreError> {
    check_image(req)?;
    match &req.device {
        Some(device) => {
            let dev = device.to_string_lossy();
            #[cfg(not(target_os = "windows"))]
            {
                if !Path::new(device).exists() {
                    return Err(CoreError::Disc(format!("device not present: {dev}")));
                }
                if let Some(mp) = crate::device::mount_point_for_device(&dev) {
                    return Err(CoreError::Disc(format!(
                        "{dev} is mounted at {mp}; unmount the disc before burning"
                    )));
                }
            }
            // On Windows the letter is a volume, not a file handle. IMAPI2
            // validates the recorder and the inserted media when it starts.
            #[cfg(target_os = "windows")]
            let _ = &dev;
        }
        None => {
            if crate::device::list_optical().is_empty() {
                return Err(CoreError::Disc(
                    "no optical drive detected; connect the writer (or pass --device)".into(),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn command_with(
    growisofs: Option<&Path>,
    cdrecord: Option<&Path>,
    hdiutil: Option<&Path>,
    req: &BurnRequest,
) -> Result<(PathBuf, Vec<String>), CoreError> {
    if req.device.is_none() {
        let h = hdiutil.ok_or_else(|| CoreError::Disc("hdiutil not found".into()))?;
        let mut args = vec!["burn".to_string(), req.image.display().to_string()];
        if let Some(s) = req.speed {
            args.push("-speed".to_string());
            args.push(s.to_string());
        }
        return Ok((h.to_path_buf(), args));
    }
    if let Some(g) = growisofs {
        let mut args = vec![
            "-Z".to_string(),
            format!(
                "{}={}",
                req.device
                    .as_ref()
                    .map(|d| d.display().to_string())
                    .unwrap_or_default(),
                req.image.display()
            ),
        ];
        if let Some(s) = req.speed {
            args.push(format!("-speed={s}"));
        }
        return Ok((g.to_path_buf(), args));
    }
    if let Some(c) = cdrecord {
        let mut args = vec![
            "-v".to_string(),
            format!(
                "dev={}",
                req.device
                    .as_ref()
                    .map(|d| d.display().to_string())
                    .unwrap_or_default()
            ),
        ];
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
    #[cfg(target_os = "windows")]
    {
        let ps = in_path("powershell.exe")
            .or_else(|| in_path("powershell"))
            .ok_or_else(|| CoreError::Disc("powershell.exe not found".into()))?;
        return Ok((
            ps,
            vec![
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-ExecutionPolicy".into(),
                "Bypass".into(),
                "-Command".into(),
                windows_script(req),
            ],
        ));
    }
    #[cfg(not(target_os = "windows"))]
    {
        command_with(
            in_path("growisofs").as_deref(),
            in_path("cdrecord").as_deref(),
            Some(Path::new("/usr/bin/hdiutil")).filter(|p| p.is_file()),
            req,
        )
    }
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
        let message = if cfg!(target_os = "windows") {
            format!(
                "would write {} with IMAPI2 to {}",
                req.image.display(),
                req.device
                    .as_ref()
                    .map(|d| d.display().to_string())
                    .unwrap_or_else(|| "the first writer".into())
            )
        } else {
            format!("would run: {} {}", bin.display(), args.join(" "))
        };
        on_progress(BurnProgress {
            percent: 0.0,
            message,
            done: true,
        });
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    on_progress(BurnProgress {
        percent: 1.0,
        message: "preparing the writer".into(),
        done: false,
    });

    // IMAPI2 talks on stdout; hdiutil also uses stdout, growisofs stderr.
    let native = req.device.is_none() || cfg!(target_os = "windows");
    let mut command = Command::new(&bin);
    command.args(&args);
    // growisofs reports progress on stderr, hdiutil on stdout.
    if native {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
    } else {
        command.stdout(Stdio::null()).stderr(Stdio::piped());
    }
    let mut child = command
        .spawn()
        .map_err(|e| CoreError::Disc(format!("{}: {e}", bin.display())))?;
    let stream: Box<dyn std::io::Read> = if native {
        Box::new(
            child
                .stdout
                .take()
                .ok_or_else(|| CoreError::Disc("writer stdout".into()))?,
        )
    } else {
        Box::new(
            child
                .stderr
                .take()
                .ok_or_else(|| CoreError::Disc("writer stderr".into()))?,
        )
    };

    let mut last = -1.0f64;
    let mut tail: Vec<String> = Vec::new();
    for line in BufReader::new(stream).lines().map_while(Result::ok) {
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
            device: Some(PathBuf::from("/dev/rdisk9")),
            speed: Some(4),
            dry_run: true,
        }
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn builds_growisofs_and_cdrecord_commands() {
        let (bin, args) = command_with(
            Some(Path::new("/bin/growisofs")),
            None,
            Some(Path::new("/usr/bin/hdiutil")),
            &req(),
        )
        .unwrap();
        assert_eq!(bin, PathBuf::from("/bin/growisofs"));
        assert_eq!(args, vec!["-Z", "/dev/rdisk9=/tmp/disc.iso", "-speed=4"]);

        let (bin, args) = command_with(
            None,
            Some(Path::new("/bin/cdrecord")),
            Some(Path::new("/usr/bin/hdiutil")),
            &req(),
        )
        .unwrap();
        assert_eq!(bin, PathBuf::from("/bin/cdrecord"));
        assert_eq!(
            args,
            vec!["-v", "dev=/dev/rdisk9", "speed=4", "/tmp/disc.iso"]
        );

        assert!(command_with(None, None, None, &req()).is_err());

        let mut native = req();
        native.device = None;
        let (bin, args) = command_with(
            Some(Path::new("/bin/growisofs")),
            None,
            Some(Path::new("/usr/bin/hdiutil")),
            &native,
        )
        .unwrap();
        assert_eq!(bin, PathBuf::from("/usr/bin/hdiutil"));
        assert_eq!(args, vec!["burn", "/tmp/disc.iso", "-speed", "4"]);
    }

    #[test]
    fn builds_the_windows_imapi_script() {
        let mut r = req();
        r.device = Some(PathBuf::from("D:"));
        r.speed = Some(8);
        let script = windows_script(&r);
        assert!(script.contains("$image = '/tmp/disc.iso'"));
        assert!(script.contains("$want = 'D:'"));
        assert!(script.contains("SetWriteSpeed(8)"));
        assert!(script.contains("IMAPI2.MsftDiscFormat2Data"));
        assert!(script.contains("ForceMediaToBeClosed = $true"));
        assert!(script.contains("Write-Output 'BURN-OK'"));

        r.image = PathBuf::from("C:\\disc's.iso");
        let quoted = windows_script(&r);
        assert!(quoted.contains("$image = 'C:\\disc''s.iso'"));
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
