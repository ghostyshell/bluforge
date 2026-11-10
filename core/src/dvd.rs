//! DVD-Video (`VIDEO_TS`) reading and ripping.
//!
//! The bundled ffmpeg has the `dvdvideo` demuxer, so a title can be ripped with
//! `-f dvdvideo -title N` without libbluray. This module enumerates the titles
//! from `VIDEO_TS.IFO` (the VMG title search pointer table), probes durations,
//! and runs ffmpeg with progress/cancel.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::CoreError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DvdTitle {
    /// 1-based title number (what `-title` takes).
    pub number: u16,
    pub chapters: u16,
    pub angles: u8,
    pub title_set: u8,
    pub vts_title: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DvdInfo {
    pub root: String,
    pub label: String,
    pub titles: Vec<DvdTitle>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DvdRipRequest {
    /// Disc root (the directory containing `VIDEO_TS`) or the `VIDEO_TS` dir.
    pub source: PathBuf,
    /// 1-based title number.
    pub title: u16,
    /// Output file; MKV is the sane default (MPEG-2/AC-3/DTS copy).
    pub output: PathBuf,
    /// Include DVD subtitles (VobSub); off by default because some players/
    /// containers disagree about them.
    #[serde(default)]
    pub subtitles: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DvdProgress {
    pub percent: f64,
    pub out_time_ms: u64,
    pub total_ms: u64,
    pub message: String,
    pub done: bool,
}

fn be16(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(o)?, *b.get(o + 1)?]))
}
fn be32(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *b.get(o)?,
        *b.get(o + 1)?,
        *b.get(o + 2)?,
        *b.get(o + 3)?,
    ]))
}

/// The disc root (the directory containing `VIDEO_TS`) for a mount, a
/// `VIDEO_TS` directory, or the `VIDEO_TS.IFO` file.
pub fn root_of(path: &Path) -> Option<PathBuf> {
    // A raw device node: find the mounted volume first.
    let raw = path.to_string_lossy();
    if raw.starts_with("/dev/") {
        if let Some(mp) = crate::device::mount_point_for_device(&raw) {
            return root_of(Path::new(&mp));
        }
        return None;
    }
    if path.join("VIDEO_TS").join("VIDEO_TS.IFO").is_file() {
        return Some(path.to_path_buf());
    }
    if path.join("VIDEO_TS.IFO").is_file() {
        return path.parent().map(|p| p.to_path_buf());
    }
    if path
        .file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("VIDEO_TS.IFO"))
    {
        return path.parent()?.parent().map(|p| p.to_path_buf());
    }
    None
}

/// Parse `VIDEO_TS.IFO` (VMG) into its title list.
///
/// `tt_srpt` is a sector pointer at offset 0xC4; the table is
/// `u16 count, u16 zero, u32 last_byte` followed by 12-byte `title_info`
/// entries (libdvdread `dvdread/ifo_types.h`).
pub fn parse_ifo(data: &[u8]) -> Option<Vec<DvdTitle>> {
    if data.len() < 0xC8 || &data[0..12] != b"DVDVIDEO-VMG" {
        return None;
    }
    let sector = be32(data, 0xC4)? as usize;
    let tt = sector.checked_mul(2048)?;
    let count = be16(data, tt)? as usize;
    if count == 0 || tt + 8 + count * 12 > data.len() {
        return None;
    }
    let mut titles = Vec::with_capacity(count);
    for i in 0..count {
        let o = tt + 8 + i * 12;
        titles.push(DvdTitle {
            number: (i + 1) as u16,
            angles: *data.get(o + 1)?,
            chapters: be16(data, o + 2)?,
            title_set: *data.get(o + 6)?,
            vts_title: *data.get(o + 7)?,
            duration_secs: None,
        });
    }
    Some(titles)
}

fn ffprobe_duration(root: &Path, title: u16) -> Option<f64> {
    let ffprobe = crate::binaries::helper_str("ffprobe").unwrap_or_else(|| "ffprobe".into());
    let out = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-f",
            "dvdvideo",
            "-title",
            &title.to_string(),
            "-show_entries",
            "format=duration",
            "-of",
            "default=nw=1:nk=1",
        ])
        .arg(root)
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|d| *d > 0.0)
}

/// Read the disc's title list, probing each duration (best effort).
pub fn info(path: &Path) -> Result<DvdInfo, CoreError> {
    let root = root_of(path).ok_or_else(|| {
        CoreError::Disc(format!(
            "no VIDEO_TS/VIDEO_TS.IFO under {} (mount the DVD or pass the disc root)",
            path.display()
        ))
    })?;
    let ifo = std::fs::read(root.join("VIDEO_TS").join("VIDEO_TS.IFO"))
        .map_err(|e| CoreError::Disc(format!("VIDEO_TS.IFO: {e}")))?;
    let mut titles =
        parse_ifo(&ifo).ok_or_else(|| CoreError::Disc("VIDEO_TS.IFO does not parse".into()))?;
    for t in &mut titles {
        t.duration_secs = ffprobe_duration(&root, t.number);
    }
    let label = root
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "DVD".into());
    Ok(DvdInfo {
        root: root.display().to_string(),
        label,
        titles,
    })
}

/// ffmpeg arguments for one title (tested without running).
pub fn build_args(root: &Path, title: u16, output: &Path, subtitles: bool) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "-hide_banner".into(),
        "-nostdin".into(),
        "-y".into(),
        "-f".into(),
        "dvdvideo".into(),
        "-title".into(),
        title.to_string(),
        "-i".into(),
        root.display().to_string(),
        "-map".into(),
        "0:v:0".into(),
        "-map".into(),
        "0:a?".into(),
    ];
    if subtitles {
        a.push("-map".into());
        a.push("0:s?".into());
    }
    a.push("-c".into());
    a.push("copy".into());
    a.push("-progress".into());
    a.push("pipe:1".into());
    a.push("-nostats".into());
    a.push(output.display().to_string());
    a
}

/// Rip one title. `cancel` stops ffmpeg and removes the partial file.
pub fn rip<F>(
    req: &DvdRipRequest,
    cancel: &AtomicBool,
    mut on_progress: F,
) -> Result<PathBuf, CoreError>
where
    F: FnMut(DvdProgress),
{
    let root = root_of(&req.source)
        .ok_or_else(|| CoreError::Disc(format!("no VIDEO_TS under {}", req.source.display())))?;
    let total_ms = ffprobe_duration(&root, req.title)
        .map(|d| (d * 1000.0) as u64)
        .unwrap_or(0);
    if let Some(dir) = req.output.parent() {
        std::fs::create_dir_all(dir)?;
    }

    let args = build_args(&root, req.title, &req.output, req.subtitles);
    let mut child = Command::new(crate::rip::ffmpeg_path())
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| CoreError::Disc(format!("ffmpeg: {e}")))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CoreError::Disc("ffmpeg stdout".into()))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| CoreError::Disc("ffmpeg stderr".into()))?;
    let err_thread = std::thread::spawn(move || {
        use std::io::Read;
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });

    let mut last = 0u64;
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k {
            "out_time_us" | "out_time_ms" => {
                if let Ok(us) = v.trim().parse::<u64>() {
                    let ms = us / 1000;
                    if ms >= last {
                        last = ms;
                        let percent = if total_ms > 0 {
                            (ms as f64 / total_ms as f64 * 100.0).clamp(0.0, 100.0)
                        } else {
                            0.0
                        };
                        on_progress(DvdProgress {
                            percent,
                            out_time_ms: ms,
                            total_ms,
                            message: "copying".into(),
                            done: false,
                        });
                    }
                }
            }
            "progress" if v.trim() == "end" => {
                on_progress(DvdProgress {
                    percent: 100.0,
                    out_time_ms: last,
                    total_ms,
                    message: "done".into(),
                    done: true,
                });
            }
            _ => {}
        }
    }
    let status = child
        .wait()
        .map_err(|e| CoreError::Disc(format!("ffmpeg: {e}")))?;
    let err = err_thread.join().unwrap_or_default();
    if cancel.load(Ordering::Relaxed) {
        let _ = std::fs::remove_file(&req.output);
        return Err(CoreError::Disc("DVD rip cancelled".into()));
    }
    if !status.success() {
        return Err(CoreError::Disc(format!(
            "ffmpeg failed: {}",
            err.lines().last().unwrap_or("see log")
        )));
    }
    Ok(req.output.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal VMG IFO with two titles (tt_srpt in sector 1).
    fn ifo() -> Vec<u8> {
        let mut d = vec![0u8; 2048 * 2];
        d[0..12].copy_from_slice(b"DVDVIDEO-VMG");
        d[0xC4..0xC8].copy_from_slice(&1u32.to_be_bytes());
        let tt = 2048;
        d[tt..tt + 2].copy_from_slice(&2u16.to_be_bytes()); // count
        d[tt + 4..tt + 8].copy_from_slice(&(8 + 24u32).to_be_bytes()); // last_byte
                                                                       // title 1: angles 1, 5 chapters, VTS 1, title 1
        d[tt + 8 + 1] = 1;
        d[tt + 8 + 2..tt + 8 + 4].copy_from_slice(&5u16.to_be_bytes());
        d[tt + 8 + 6] = 1;
        d[tt + 8 + 7] = 1;
        // title 2: angles 2, 3 chapters, VTS 1, title 2
        d[tt + 20 + 1] = 2;
        d[tt + 20 + 2..tt + 20 + 4].copy_from_slice(&3u16.to_be_bytes());
        d[tt + 20 + 6] = 1;
        d[tt + 20 + 7] = 2;
        d
    }

    #[test]
    fn parses_titles_from_vmg_ifo() {
        let t = parse_ifo(&ifo()).expect("parsed");
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].number, 1);
        assert_eq!(t[0].chapters, 5);
        assert_eq!(t[1].angles, 2);
        assert_eq!(t[1].chapters, 3);
        assert_eq!(t[1].vts_title, 2);
    }

    #[test]
    fn rejects_other_files() {
        assert!(parse_ifo(b"not an ifo").is_none());
    }

    #[test]
    fn builds_a_dvdvideo_copy_command() {
        let a = build_args(Path::new("/Volumes/DVD"), 2, Path::new("/tmp/t.mkv"), true);
        let s = a.join(" ");
        assert!(s.starts_with("-hide_banner -nostdin -y -f dvdvideo -title 2 -i /Volumes/DVD"));
        assert!(s.contains("-map 0:v:0 -map 0:a? -map 0:s? -c copy"));
        assert!(s.ends_with("/tmp/t.mkv"));
    }
}
