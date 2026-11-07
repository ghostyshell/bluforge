//! Rip pipeline. Decrypted MPEG-TS from libbluray is piped into ffmpeg, which
//! either remuxes (lossless) or re-encodes (CPU x264/x265 or GPU VideoToolbox).

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::disc::{Disc, TitleReader};
use crate::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoCodec {
    X264,
    X265,
    HevcVideotoolbox,
    H264Videotoolbox,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncodeSettings {
    pub codec: VideoCodec,
    /// Constant-quality value (CRF for x264/x265, `-q:v` for VideoToolbox, 1..=51).
    pub quality: u32,
    /// x264/x265 preset.
    pub preset: Option<String>,
    /// 10-bit encode (software encoders only).
    pub ten_bit: bool,
    /// Optional audio re-encode to AAC (default: copy).
    pub aac_audio: bool,
}

impl Default for EncodeSettings {
    fn default() -> Self {
        Self {
            codec: VideoCodec::X265,
            quality: 20,
            preset: Some("medium".into()),
            ten_bit: true,
            aac_audio: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Container {
    Mkv,
    Mp4,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RipMode {
    /// Lossless remux, all streams preserved.
    Remux { container: Container },
    /// Re-encode video with the given settings.
    Encode {
        container: Container,
        settings: EncodeSettings,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RipRequest {
    /// Disc source: mounted volume, ISO, or device node.
    pub source: PathBuf,
    pub keydb: Option<PathBuf>,
    pub title: u32,
    pub output: PathBuf,
    pub mode: RipMode,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RipProgress {
    pub out_time_ms: u64,
    pub total_ms: u64,
    pub frames: u64,
    pub fps: f64,
    pub speed: String,
    pub done: bool,
    pub percent: f64,
    pub message: String,
}

pub fn ffmpeg_path() -> PathBuf {
    std::env::var_os("BLUFORGE_FFMPEG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

fn codec_args(s: &EncodeSettings) -> Vec<String> {
    let mut v = Vec::new();
    match s.codec {
        VideoCodec::X264 => {
            v.push("-c:v".into());
            v.push("libx264".into());
            v.push("-crf".into());
            v.push(s.quality.to_string());
            if let Some(p) = &s.preset {
                v.push("-preset".into());
                v.push(p.clone());
            }
            v.push("-pix_fmt".into());
            v.push(if s.ten_bit { "yuv420p10le" } else { "yuv420p" }.into());
        }
        VideoCodec::X265 => {
            v.push("-c:v".into());
            v.push("libx265".into());
            v.push("-crf".into());
            v.push(s.quality.to_string());
            if let Some(p) = &s.preset {
                v.push("-preset".into());
                v.push(p.clone());
            }
            v.push("-pix_fmt".into());
            v.push(if s.ten_bit { "yuv420p10le" } else { "yuv420p" }.into());
            v.push("-tag:v".into());
            v.push("hvc1".into());
        }
        VideoCodec::HevcVideotoolbox => {
            v.push("-c:v".into());
            v.push("hevc_videotoolbox".into());
            v.push("-q:v".into());
            v.push(s.quality.to_string());
            v.push("-tag:v".into());
            v.push("hvc1".into());
        }
        VideoCodec::H264Videotoolbox => {
            v.push("-c:v".into());
            v.push("h264_videotoolbox".into());
            v.push("-q:v".into());
            v.push(s.quality.to_string());
        }
    }
    v
}

fn build_args(req: &RipRequest) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "-hide_banner".into(),
        "-nostdin".into(),
        "-i".into(),
        "pipe:0".into(),
    ];
    match &req.mode {
        RipMode::Remux { container } => {
            match container {
                Container::Mkv => {
                    a.extend(["-map", "0", "-c", "copy", "-map_chapters", "0"].map(String::from));
                }
                Container::Mp4 => {
                    a.extend(["-map", "0:v", "-map", "0:a", "-c", "copy"].map(String::from));
                }
            }
        }
        RipMode::Encode { container, settings } => {
            a.extend(["-map".into(), "0:v:0".into(), "-map".into(), "0:a?".into()]);
            a.extend(codec_args(settings));
            if settings.aac_audio {
                a.extend(["-c:a".into(), "aac".into(), "-b:a".into(), "640k".into()]);
            } else {
                a.extend(["-c:a".into(), "copy".into()]);
            }
            if matches!(container, Container::Mp4) {
                // drop incompatible subtitle streams for mp4
                a.extend(["-sn".into()]);
            }
        }
    }
    a.extend(["-progress".into(), "pipe:1".into(), "-nostats".into()]);
    a.push(req.output.to_string_lossy().into_owned());
    a
}

fn parse_progress(line: &str, p: &mut RipProgress) {
    if let Some((k, v)) = line.split_once('=') {
        match k.trim() {
            "out_time_us" | "out_time_ms" => {
                if let Ok(us) = v.trim().parse::<u64>() {
                    p.out_time_ms = us / 1000;
                }
            }
            "frame" => {
                if let Ok(f) = v.trim().parse::<u64>() {
                    p.frames = f;
                }
            }
            "fps" => {
                if let Ok(f) = v.trim().parse::<f64>() {
                    p.fps = f;
                }
            }
            "speed" => p.speed = v.trim().to_string(),
            "progress" => {
                if v.trim() == "end" {
                    p.done = true;
                }
            }
            _ => {}
        }
    }
    if p.total_ms > 0 {
        p.percent = (p.out_time_ms as f64 / p.total_ms as f64 * 100.0).clamp(0.0, 100.0);
    }
}

/// Run the rip. `on_progress` is called on the calling thread for each update.
/// `cancel` is polled between progress lines; when set the ffmpeg child is killed.
pub fn run<F>(req: &RipRequest, cancel: &std::sync::atomic::AtomicBool, mut on_progress: F) -> Result<(), CoreError>
where
    F: FnMut(RipProgress),
{
    if let Some(dir) = req.output.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let disc = Disc::open(&req.source, req.keydb.as_deref())?;
    let info = disc.info()?;
    let total_ms = info
        .titles
        .iter()
        .find(|t| t.index == req.title)
        .map(|t| (t.duration_secs * 1000.0) as u64)
        .unwrap_or(0);
    disc.select(req.title)?;

    let args = build_args(req);
    tracing::info!(?args, "spawning ffmpeg");
    let mut child = Command::new(ffmpeg_path())
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| CoreError::Disc(format!("spawn ffmpeg: {e}")))?;

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");

    // Feed disc -> ffmpeg stdin. libbluray emits 192-byte m2ts packets with
    // null-packet (0x1fff) padding; ffmpeg misdetects those as a phantom
    // stream, so we strip them on the way through.
    let feed = std::thread::spawn(move || -> Result<(), CoreError> {
        let mut reader = TitleReader::new(&disc);
        let mut buf = vec![0u8; 1 << 20];
        let mut out: Vec<u8> = Vec::with_capacity(1 << 20);
        let mut stride = 0usize;
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            if stride == 0 {
                stride = if n >= 5 && buf[4] == 0x47 {
                    192
                } else {
                    188
                };
            }
            out.clear();
            let sync = if stride == 192 { 4 } else { 0 };
            let mut i = 0;
            while i + stride <= n {
                if buf[i + sync] == 0x47 {
                    let pid = (((buf[i + sync + 1] & 0x1f) as u16) << 8) | buf[i + sync + 2] as u16;
                    if pid != 0x1fff {
                        out.extend_from_slice(&buf[i..i + stride]);
                    }
                } else {
                    out.extend_from_slice(&buf[i..i + stride]);
                }
                i += stride;
            }
            if i < n {
                out.extend_from_slice(&buf[i..n]);
            }
            if stdin.write_all(&out).is_err() {
                break;
            }
        }
        let _ = stdin.flush();
        drop(stdin);
        Ok(())
    });

    let mut prog = RipProgress {
        total_ms,
        message: "ripping".into(),
        ..Default::default()
    };
    on_progress(prog.clone());

    let reader = BufReader::new(stdout);
    for line in reader.lines().map_while(Result::ok) {
        parse_progress(&line, &mut prog);
        on_progress(prog.clone());
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = child.kill();
            break;
        }
    }

    let status = child.wait()?;
    let _ = feed.join();

    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        prog.message = "cancelled".into();
        on_progress(prog);
        return Ok(());
    }
    if !status.success() {
        return Err(CoreError::Disc(format!("ffmpeg exited with {status}")));
    }
    prog.done = true;
    prog.percent = 100.0;
    prog.message = "done".into();
    on_progress(prog);
    Ok(())
}

/// Convenience: does the disc at `path` look like a Blu-ray?
pub fn is_bluray(path: &Path) -> bool {
    path.join("BDMV").is_dir() || path.join("bdmv").is_dir()
}
