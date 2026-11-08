//! Blu-ray authoring: turn finished MKVs back into a playable BDMV / ISO.
//!
//! The heavy lifting is done by `tsMuxeR` (it re-muxes without re-encoding and
//! writes a proper UDF 2.50 image with PLAYList/CLIPINF/STREAM/index.bdmv). This
//! module owns the meta-file generation, track mapping, chapter placement and
//! progress reporting, and keeps the tool lookup in one place.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::CoreError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorRequest {
    /// Input files in playback order (MKV / MP4 / M2TS).
    pub inputs: Vec<PathBuf>,
    /// Output `.iso` file, or a directory for an unpacked BDMV tree.
    pub dest: PathBuf,
    /// Volume label (uppercased, max 32 chars).
    #[serde(default)]
    pub label: Option<String>,
    /// Carry the inputs' chapter marks (episode starts become chapters).
    #[serde(default = "crate::config::yes")]
    pub chapters: bool,
    /// Force a chapter every N minutes instead of using the inputs' marks.
    #[serde(default)]
    pub auto_chapter_minutes: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthorProgress {
    pub percent: f64,
    pub message: String,
    pub done: bool,
}

/// Locate tsMuxeR: explicit override, the copy the app manages, then PATH.
pub fn tsmuxer_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("BLUFORGE_TSMUXER") {
        if Path::new(&p).is_file() {
            return Some(PathBuf::from(p));
        }
    }
    let managed = crate::config_dir().join("bin/tsMuxeR");
    if managed.is_file() {
        return Some(managed);
    }
    for c in ["/opt/homebrew/bin/tsMuxeR", "/usr/local/bin/tsMuxeR"] {
        if Path::new(c).is_file() {
            return Some(PathBuf::from(c));
        }
    }
    None
}

fn ffprobe() -> String {
    for c in ["/opt/homebrew/bin/ffprobe", "/usr/local/bin/ffprobe"] {
        if Path::new(c).is_file() {
            return c.to_string();
        }
    }
    "ffprobe".to_string()
}

struct Probe {
    duration: f64,
    tracks: Vec<TrackLine>,
    chapters: Vec<f64>,
}

struct TrackLine {
    line: String,
}

fn probe(input: &Path) -> Result<Probe, CoreError> {
    let out = Command::new(ffprobe())
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_streams",
            "-show_chapters",
            "-show_format",
        ])
        .arg(input)
        .output()
        .map_err(|e| CoreError::Disc(format!("ffprobe: {e}")))?;
    if !out.status.success() {
        return Err(CoreError::Disc(format!(
            "ffprobe failed for {}",
            input.display()
        )));
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| CoreError::Disc(format!("ffprobe json: {e}")))?;

    let path = input
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let mut tracks = Vec::new();
    if let Some(sts) = v["streams"].as_array() {
        for st in sts {
            let kind = st["codec_type"].as_str().unwrap_or("");
            let codec = st["codec_name"].as_str().unwrap_or("");
            let idx = st["index"].as_u64().unwrap_or(0);
            let track = idx + 1;
            let lang = st["tags"]["language"]
                .as_str()
                .unwrap_or("und")
                .to_ascii_lowercase();
            let title = st["tags"]["title"].as_str().unwrap_or("");
            let attached = st["disposition"]["attached_pic"].as_u64().unwrap_or(0) == 1;
            if attached {
                continue;
            }
            let codec_tag = match (kind, codec) {
                ("video", "h264") => "V_MPEG4/ISO/AVC",
                ("video", "hevc") => "V_MPEG4/ISO/HEVC",
                ("video", "mpeg2video") => "V_MPEG2",
                ("video", "vc1") => "V_VC1",
                ("audio", "dts") => "A_DTS",
                ("audio", "ac3") => "A_AC3",
                ("audio", "eac3") => "A_AC3",
                ("audio", "truehd") => "A_TRUEHD",
                ("audio", "aac") => "A_AAC",
                ("audio", "flac") => "A_FLAC",
                ("audio", "pcm_bluray") | ("audio", "pcm_s16le") | ("audio", "pcm_s24le") => {
                    "A_LPCM"
                }
                ("subtitle", "hdmv_pgs_subtitle") => "S_HDMV/PGS",
                ("subtitle", "subrip") => "S_TEXT/UTF8",
                ("subtitle", "ass") | ("subtitle", "ssa") => "S_TEXT/ASS",
                _ => continue,
            };
            let mut line = format!("{codec_tag}, \"{path}\", track={track}, lang={lang}");
            if kind == "video" && codec == "h264" {
                line = format!("{codec_tag}, \"{path}\", insertSEI, contSPS, track={track}, lang={lang}");
            }
            if !title.is_empty() {
                line.push_str(&format!(
                    ", text={}",
                    title.replace(',', " ").replace('"', "")
                ));
            }
            tracks.push(TrackLine { line });
        }
    }
    if !tracks.iter().any(|t| t.line.starts_with("V_")) {
        return Err(CoreError::Disc(format!(
            "{} has no video stream tsMuxeR can carry",
            input.display()
        )));
    }

    let duration = v["format"]["duration"]
        .as_str()
        .and_then(|d| d.parse::<f64>().ok())
        .unwrap_or(0.0);
    let chapters = v["chapters"]
        .as_array()
        .map(|cs| {
            cs.iter()
                .filter_map(|c| c["start_time"].as_str().and_then(|t| t.parse::<f64>().ok()))
                .collect()
        })
        .unwrap_or_default();

    Ok(Probe {
        duration,
        tracks,
        chapters,
    })
}

fn stamp(secs: f64) -> String {
    let ms = (secs.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        ms / 3_600_000,
        (ms / 60_000) % 60,
        (ms / 1000) % 60,
        ms % 1000
    )
}

/// Build the tsMuxeR meta file for the whole play-all title.
fn meta(req: &AuthorRequest) -> Result<(String, f64), CoreError> {
    let mut probes = Vec::new();
    let mut total = 0.0;
    for input in &req.inputs {
        let p = probe(input)?;
        total += p.duration;
        probes.push(p);
    }

    let mut chapter_marks: Vec<f64> = Vec::new();
    if let Some(minutes) = req.auto_chapter_minutes {
        let step = (minutes.max(1) as f64) * 60.0;
        let mut t = 0.0;
        while t < total {
            chapter_marks.push(t);
            t += step;
        }
    } else if req.chapters {
        let mut offset = 0.0;
        for p in &probes {
            for c in &p.chapters {
                chapter_marks.push(offset + c);
            }
            offset += p.duration;
        }
        // Every input starts a new chapter even if it carries no marks.
        let mut offset = 0.0;
        for p in &probes {
            if !chapter_marks.iter().any(|c| (c - offset).abs() < 0.5) {
                chapter_marks.push(offset);
            }
            offset += p.duration;
        }
        chapter_marks.sort_by(|a, b| a.partial_cmp(b).unwrap());
        chapter_marks.dedup_by(|a, b| (*a - *b).abs() < 0.5);
    }

    let label = req
        .label
        .clone()
        .unwrap_or_else(|| "BLUFORGE".to_string())
        .to_uppercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .take(32)
        .collect::<String>();

    let mut s = String::from(
        "MUXOPT --blu-ray --no-pcr-on-video-pid --new-audio-pes --vbr --vbv-len=500",
    );
    s.push_str(&format!(" --label={label}"));
    if !chapter_marks.is_empty() {
        let list = chapter_marks
            .iter()
            .map(|c| stamp(*c))
            .collect::<Vec<_>>()
            .join(";");
        s.push_str(&format!(" --custom-chapters={list}"));
    }
    s.push('\n');

    for (i, p) in probes.iter().enumerate() {
        if i > 0 {
            s.push('\n');
        }
        for t in &p.tracks {
            s.push_str(&t.line);
            s.push('\n');
        }
    }
    Ok((s, total))
}

/// Author the disc. `on_progress` is called as tsMuxeR reports percentages.
pub fn run<F>(req: &AuthorRequest, mut on_progress: F) -> Result<PathBuf, CoreError>
where
    F: FnMut(AuthorProgress),
{
    if req.inputs.is_empty() {
        return Err(CoreError::Disc("no input files".into()));
    }
    if req.inputs.len() > 1 {
        // tsMuxeR's CLI writes one title and concatenates inputs into a single
        // clip, which resets timestamps and leaves the playlist stopping after
        // the first file. Per-episode playlists need their own mpls/clpi, which
        // is the next step; one file per disc is what works today.
        return Err(CoreError::Disc(
            "authoring one file per disc for now: multi-episode discs need per-episode playlists (next iteration)".into(),
        ));
    }
    for i in &req.inputs {
        if !i.is_file() {
            return Err(CoreError::Disc(format!("missing input: {}", i.display())));
        }
    }
    let bin = tsmuxer_path().ok_or_else(|| {
        CoreError::Disc(
            "tsMuxeR is not installed (expected at ~/Library/Application Support/bluforge/bin/tsMuxeR)"
                .into(),
        )
    })?;

    let (meta_text, total) = meta(req)?;
    if let Some(dir) = req.dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let meta_path = std::env::temp_dir().join(format!("bluforge-author-{}.meta", std::process::id()));
    std::fs::write(&meta_path, &meta_text)?;

    on_progress(AuthorProgress {
        percent: 0.0,
        message: format!("authoring {} ({})", req.dest.display(), stamp(total)),
        done: false,
    });

    let mut child = Command::new(&bin)
        .arg(&meta_path)
        .arg(&req.dest)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| CoreError::Disc(format!("tsMuxeR: {e}")))?;

    let mut out = child
        .stdout
        .take()
        .ok_or_else(|| CoreError::Disc("tsMuxeR stdout".into()))?;
    let mut err = child
        .stderr
        .take()
        .ok_or_else(|| CoreError::Disc("tsMuxeR stderr".into()))?;
    // Drain stderr on a thread so a chatty stream cannot block the pipe.
    let err_thread = std::thread::spawn(move || {
        use std::io::Read;
        let mut s = String::new();
        let _ = err.read_to_string(&mut s);
        s
    });

    {
        use std::io::{BufRead, BufReader};
        for line in BufReader::new(&mut out).lines().map_while(Result::ok) {
            let l = line.trim_start_matches('\u{1b}').trim();
            let l = l.trim_start_matches('M').trim();
            if let Some(p) = l.strip_suffix("% complete") {
                if let Ok(v) = p.trim().parse::<f64>() {
                    on_progress(AuthorProgress {
                        percent: v,
                        message: "muxing".into(),
                        done: false,
                    });
                }
            }
        }
    }
    let status = child
        .wait()
        .map_err(|e| CoreError::Disc(format!("tsMuxeR: {e}")))?;
    let err_text = err_thread.join().unwrap_or_default();
    let _ = std::fs::remove_file(&meta_path);
    if !status.success() {
        return Err(CoreError::Disc(format!(
            "tsMuxeR failed: {}",
            err_text.lines().last().unwrap_or("see log")
        )));
    }

    on_progress(AuthorProgress {
        percent: 100.0,
        message: format!("authored {}", req.dest.display()),
        done: true,
    });
    Ok(req.dest.clone())
}
