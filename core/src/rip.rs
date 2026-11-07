//! Rip pipeline. Decrypted MPEG-TS from libbluray is piped into ffmpeg, which
//! either remuxes (lossless) or re-encodes (CPU x264/x265 or GPU VideoToolbox).

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::disc::{Chapter, Disc, StreamInfo, TitleReader};
use crate::{metadata, template, CoreError};

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

/// Which audio tracks to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioSel {
    All,
    /// Only the track that would be the default (English, non-commentary).
    Main,
    English,
}

impl Default for AudioSel {
    fn default() -> Self {
        AudioSel::All
    }
}

/// Which subtitle tracks to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubSel {
    All,
    English,
    None,
}

impl Default for SubSel {
    fn default() -> Self {
        SubSel::All
    }
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
    /// Title index; `None` rips the disc's main title.
    #[serde(default)]
    pub title: Option<u32>,
    /// Exact output file, or a directory to render the output template into.
    /// `None` uses the configured output folder and template.
    #[serde(default)]
    pub output: Option<PathBuf>,
    pub mode: RipMode,
    /// Embed chapter marks, named from ChapterDB when a matching entry exists.
    #[serde(default = "yes")]
    pub chapters: bool,
    /// Save a TMDB poster next to the output and attach it to the file (MKV).
    #[serde(default = "yes")]
    pub poster: bool,
    /// Movie title used for metadata lookups (defaults to the disc name).
    #[serde(default)]
    pub movie: Option<String>,
    /// Audio tracks to keep.
    #[serde(default)]
    pub audio: AudioSel,
    /// Subtitle tracks to keep.
    #[serde(default)]
    pub subs: SubSel,
}

fn yes() -> bool {
    true
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
    /// Resolved output path, once known.
    #[serde(default)]
    pub dest: Option<PathBuf>,
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

/// ISO 639-2 language name, for track titles.
pub fn lang_name(code: &str) -> String {
    match code.to_ascii_lowercase().as_str() {
        "eng" | "en" => "English",
        "fre" | "fra" | "fr" => "French",
        "spa" | "es" => "Spanish",
        "ger" | "deu" | "de" => "German",
        "ita" | "it" => "Italian",
        "jpn" | "ja" => "Japanese",
        "por" | "pt" => "Portuguese",
        "rus" | "ru" => "Russian",
        "chi" | "zho" | "zh" | "yue" => "Chinese",
        "kor" | "ko" => "Korean",
        "dut" | "nld" | "nl" => "Dutch",
        "swe" | "sv" => "Swedish",
        "nor" | "no" => "Norwegian",
        "dan" | "da" => "Danish",
        "fin" | "fi" => "Finnish",
        "pol" | "pl" => "Polish",
        "cze" | "ces" | "cs" => "Czech",
        "hun" | "hu" => "Hungarian",
        "tur" | "tr" => "Turkish",
        "ara" | "ar" => "Arabic",
        "hin" | "hi" => "Hindi",
        "tha" | "th" => "Thai",
        "ukr" | "uk" => "Ukrainian",
        other => return other.to_uppercase(),
    }
    .to_string()
}

/// Language tag for the MKV `language` element (ISO 639-2).
fn lang_tag(code: &str) -> String {
    match code.to_ascii_lowercase().as_str() {
        "en" => "eng",
        "fr" => "fre",
        "es" => "spa",
        "de" => "ger",
        "it" => "ita",
        "ja" => "jpn",
        "pt" => "por",
        "ru" => "rus",
        "zh" => "chi",
        "ko" => "kor",
        "nl" => "dut",
        "sv" => "swe",
        "no" => "nor",
        "da" => "dan",
        "fi" => "fin",
        "pl" => "pol",
        "cs" => "cze",
        "hu" => "hun",
        "tr" => "tur",
        "ar" => "ara",
        "hi" => "hin",
        other => return other.to_string(),
    }
    .to_string()
}

pub fn audio_codec_label(coding_type: u8) -> &'static str {
    match coding_type {
        0x03 | 0x04 => "MPEG",
        0x80 => "LPCM",
        0x81 => "AC3",
        0x82 => "DTS",
        0x83 => "TrueHD",
        0x84 => "E-AC3",
        0x85 | 0x86 => "DTS-HD",
        0xa1 => "E-AC3",
        0xa2 => "DTS Express",
        _ => "Audio",
    }
}

pub fn video_codec_label(coding_type: u8) -> &'static str {
    match coding_type {
        0x01 => "MPEG-1",
        0x02 => "MPEG-2",
        0x1b => "H.264",
        0x24 => "HEVC",
        0xea => "VC-1",
        _ => "Video",
    }
}

/// Channel hint from the BD audio format code (only where it is unambiguous).
fn audio_chans(format: u8) -> Option<&'static str> {
    match format {
        1 => Some("1.0"),
        3 => Some("2.0"),
        _ => None,
    }
}

/// TrueHD / DTS-HD carry an embedded lossy core, which ffmpeg exposes as an
/// *extra* audio stream of the same PID. That shifts every later ffmpeg audio
/// index, so mapping/labelling has to account for it.
fn has_core(coding_type: u8) -> bool {
    matches!(coding_type, 0x83 | 0x85 | 0x86)
}

fn core_codec_label(coding_type: u8) -> &'static str {
    match coding_type {
        0x83 => "AC3",
        _ => "DTS",
    }
}

/// Secondary coding types are the disc's commentary / picture-in-picture tracks.
fn is_secondary(coding_type: u8) -> bool {
    coding_type == 0xa1 || coding_type == 0xa2
}

/// Lossless / primary audio coding types.
fn is_primary_audio(coding_type: u8) -> bool {
    matches!(coding_type, 0x80 | 0x83 | 0x85 | 0x86)
}

/// Disc-derived context used to label and flag the exported tracks.
#[derive(Default, Clone)]
pub struct TrackCtx {
    /// Streams in disc order (video, then audio, then subtitle).
    pub streams: Vec<StreamInfo>,
    /// Index (within the audio streams) that should be the default track.
    pub default_audio: Option<usize>,
    /// Chapter names aligned to the disc's chapter marks, when known.
    pub chapter_names: Vec<String>,
    pub chapters_file: Option<PathBuf>,
    pub poster_file: Option<PathBuf>,
    /// Final output path (template already rendered).
    pub dest: PathBuf,
}

fn human_time(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

/// Decide which streams to map, and the `-metadata`/`-disposition` arguments for
/// them. Output stream indices are counted separately from input ones because
/// filtering shifts them.
fn plan_tracks(ctx: &TrackCtx, audio: AudioSel, subs: SubSel) -> (Vec<String>, Vec<String>) {
    let mut maps: Vec<String> = Vec::new();
    let mut meta: Vec<String> = Vec::new();

    let primary_lang = ctx
        .default_audio
        .and_then(|i| ctx.streams.iter().filter(|s| s.kind == "audio").nth(i))
        .and_then(|s| s.lang.clone())
        .unwrap_or_default();
    let is_eng = |l: &str| matches!(l, "eng" | "en");

    // Keep at least one audio track even if the filter matches nothing.
    let any_eng = ctx
        .streams
        .iter()
        .any(|s| s.kind == "audio" && s.lang.as_deref().map(is_eng).unwrap_or(false));
    let audio_sel = if audio == AudioSel::English && !any_eng {
        AudioSel::Main
    } else {
        audio
    };

    let (mut v, mut ai, mut si) = (0usize, 0usize, 0usize);
    let (mut out_v, mut out_a, mut out_s) = (0usize, 0usize, 0usize);
    let mut cores_before = 0usize;
    for st in &ctx.streams {
        let lang = st.lang.clone().unwrap_or_default();
        match st.kind.as_str() {
            "video" => {
                let res = match st.format {
                    1 => "480i",
                    2 => "576i",
                    3 => "480p",
                    4 => "1080i",
                    5 => "720p",
                    6 => "1080p",
                    7 => "576p",
                    8 => "2160p",
                    _ => "video",
                };
                let label = format!("{res} {}", video_codec_label(st.coding_type));
                maps.push(format!("0:v:{v}"));
                if out_v == 0 {
                    meta.extend([
                        format!("-metadata:s:v:{out_v}"),
                        format!("title={}", label.trim()),
                        format!("-metadata:s:v:{out_v}"),
                        "language=und".into(),
                        format!("-disposition:v:{out_v}"),
                        "default".into(),
                    ]);
                }
                out_v += 1;
                v += 1;
            }
            "audio" => {
                let ff = ai + cores_before;
                let keep = match audio_sel {
                    AudioSel::All => true,
                    AudioSel::Main => ctx.default_audio == Some(ai),
                    AudioSel::English => is_eng(&lang),
                };
                if keep {
                    maps.push(format!("0:a:{ff}"));
                    if !lang.is_empty() {
                        let mut label =
                            format!("{} {}", lang_name(&lang), audio_codec_label(st.coding_type));
                        if let Some(ch) = audio_chans(st.format) {
                            label.push(' ');
                            label.push_str(ch);
                        }
                        if is_secondary(st.coding_type)
                            || (audio_chans(st.format) == Some("2.0")
                                && !is_primary_audio(st.coding_type)
                                && !primary_lang.is_empty()
                                && lang == primary_lang
                                && ctx.default_audio != Some(ai))
                        {
                            label.push_str(" (Commentary)");
                        }
                        meta.extend([
                            format!("-metadata:s:a:{out_a}"),
                            format!("language={}", lang_tag(&lang)),
                            format!("-metadata:s:a:{out_a}"),
                            format!("title={label}"),
                        ]);
                    }
                    meta.extend([
                        format!("-disposition:a:{out_a}"),
                        if ctx.default_audio == Some(ai) { "default".into() } else { "0".into() },
                    ]);
                    out_a += 1;
                    // With `all`, also keep the embedded lossy core.
                    if has_core(st.coding_type) {
                        maps.push(format!("0:a:{}", ff + 1));
                        if !lang.is_empty() {
                            meta.extend([
                                format!("-metadata:s:a:{out_a}"),
                                format!("language={}", lang_tag(&lang)),
                                format!("-metadata:s:a:{out_a}"),
                                format!(
                                    "title={} {} (core)",
                                    lang_name(&lang),
                                    core_codec_label(st.coding_type)
                                ),
                            ]);
                        }
                        meta.extend([format!("-disposition:a:{out_a}"), "0".into()]);
                        out_a += 1;
                    }
                }
                if has_core(st.coding_type) {
                    cores_before += 1;
                }
                ai += 1;
            }
            _ => {
                let keep = match subs {
                    SubSel::All => true,
                    SubSel::English => is_eng(&lang),
                    SubSel::None => false,
                };
                if keep {
                    maps.push(format!("0:s:{si}"));
                    if !lang.is_empty() {
                        meta.extend([
                            format!("-metadata:s:s:{out_s}"),
                            format!("language={}", lang_tag(&lang)),
                            format!("-metadata:s:s:{out_s}"),
                            format!("title={} (subtitles)", lang_name(&lang)),
                        ]);
                    }
                    // Subtitles stay off by default.
                    meta.extend([format!("-disposition:s:{out_s}"), "0".into()]);
                    out_s += 1;
                }
                si += 1;
            }
        }
    }

    (maps, meta)
}

fn map_args(maps: &[String]) -> Vec<String> {
    let mut v = Vec::with_capacity(maps.len() * 2);
    for m in maps {
        v.push("-map".into());
        v.push(m.clone());
    }
    v
}

fn build_args(req: &RipRequest, ctx: &TrackCtx) -> Vec<String> {
    let mkv = matches!(
        &req.mode,
        RipMode::Remux { container: Container::Mkv }
            | RipMode::Encode { container: Container::Mkv, .. }
    );

    let mut a: Vec<String> = vec![
        "-hide_banner".into(),
        "-nostdin".into(),
        "-i".into(),
        "pipe:0".into(),
    ];
    if let Some(cf) = &ctx.chapters_file {
        a.extend(["-f".into(), "ffmetadata".into(), "-i".into(), cf.to_string_lossy().into_owned()]);
    }

    let (mut maps, meta) = plan_tracks(ctx, req.audio, req.subs);
    // MP4 cannot carry PGS subtitles.
    if !mkv {
        maps.retain(|m| !m.starts_with("0:s:"));
    }

    match &req.mode {
        RipMode::Remux { container } => match container {
            Container::Mkv => {
                a.extend(map_args(&maps));
                a.extend(["-c".into(), "copy".into()]);
            }
            Container::Mp4 => {
                a.extend(map_args(&maps));
                a.extend(["-c".into(), "copy".into()]);
            }
        },
        RipMode::Encode { settings, .. } => {
            a.push("-map".into());
            a.push("0:v:0".into());
            // audio/subtitle maps from the planner (skip its duplicate video map)
            for m in maps.iter().filter(|m| !m.starts_with("0:v:")) {
                a.push("-map".into());
                a.push(m.clone());
            }
            a.extend(codec_args(settings));
            if settings.aac_audio {
                a.extend(["-c:a".into(), "aac".into(), "-b:a".into(), "640k".into()]);
            } else {
                a.extend(["-c:a".into(), "copy".into()]);
            }
            if mkv && maps.iter().any(|m| m.starts_with("0:s:")) {
                a.extend(["-c:s".into(), "copy".into()]);
            }
        }
    }

    if mkv {
        if ctx.chapters_file.is_some() {
            a.extend(["-map_chapters".into(), "1".into()]);
        }
        a.extend(meta);
        if let Some(poster) = &ctx.poster_file {
            a.extend([
                "-attach".into(),
                poster.to_string_lossy().into_owned(),
                "-metadata:s:t".into(),
                "mimetype=image/jpeg".into(),
                "-metadata:s:t".into(),
                "filename=cover.jpg".into(),
            ]);
        }
    }

    a.extend(["-progress".into(), "pipe:1".into(), "-nostats".into()]);
    a.push(ctx.dest.to_string_lossy().into_owned());
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

/// Tidy a disc name into something a metadata site will match.
fn clean_movie_name(s: &str, fallback: &str) -> String {
    let mut t = s.replace('_', " ").trim().to_string();
    for suf in [" - Blu-ray", " (Blu-ray)", " Blu-ray", " - BluRay"] {
        if let Some(stripped) = t.strip_suffix(suf) {
            t = stripped.trim().to_string();
        }
    }
    while t.contains("  ") {
        t = t.replace("  ", " ");
    }
    if t.is_empty() {
        fallback.to_string()
    } else {
        t
    }
}

/// The audio stream that should be the default: English and not commentary.
fn pick_default_audio(streams: &[StreamInfo]) -> Option<usize> {
    let audios: Vec<&StreamInfo> = streams.iter().filter(|s| s.kind == "audio").collect();
    if let Some(i) = audios.iter().position(|s| {
        matches!(s.lang.as_deref(), Some("eng") | Some("en")) && !is_secondary(s.coding_type)
    }) {
        return Some(i);
    }
    if let Some(i) = audios
        .iter()
        .position(|s| !is_secondary(s.coding_type) && s.lang.is_some())
    {
        return Some(i);
    }
    audios.iter().position(|s| s.lang.is_some())
}

fn ffmeta_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' | '=' | ';' | '#' => {
                o.push('\\');
                o.push(c);
            }
            '\n' => o.push(' '),
            _ => o.push(c),
        }
    }
    o
}

/// Write an ffmetadata chapters file: disc times, ChapterDB names, generic
/// fallback names, and any names authored on the disc.
fn write_chapters(
    marks: &[Chapter],
    names: &[String],
    title_end_ms: u64,
    path: &Path,
) -> Result<(), CoreError> {
    let mut s = String::from(";FFMETADATA1\n");
    for (i, m) in marks.iter().enumerate() {
        let start = (m.start_secs * 1000.0).round().max(0.0) as u64;
        let mut end = marks
            .get(i + 1)
            .map(|n| (n.start_secs * 1000.0).round() as u64)
            .unwrap_or(title_end_ms);
        if end <= start {
            end = start + 1;
        }
        let name = names
            .get(i)
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .or_else(|| m.name.clone())
            .unwrap_or_else(|| format!("Chapter {:02}", i + 1));
        s.push_str(&format!(
            "[CHAPTER]\nTIMEBASE=1/1000\nSTART={start}\nEND={end}\ntitle={}\n",
            ffmeta_escape(&name)
        ));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, s)?;
    Ok(())
}

/// Run the rip. `on_progress` is called on the calling thread for each update.
/// `cancel` is polled between progress lines; when set the ffmpeg child is killed.
pub fn run<F>(req: &RipRequest, cancel: &std::sync::atomic::AtomicBool, mut on_progress: F) -> Result<(), CoreError>
where
    F: FnMut(RipProgress),
{
    if let Some(dir) = req.output.as_ref().and_then(|p| p.parent()) {
        std::fs::create_dir_all(dir)?;
    }
    let disc = Disc::open(&req.source, req.keydb.as_deref())?;
    let info = disc.info()?;
    let title = req.title.or(info.main_title).unwrap_or(0);
    tracing::info!(title, main = ?info.main_title, "ripping title");
    let title_info = info.titles.iter().find(|t| t.index == title);
    let total_ms = title_info
        .map(|t| (t.duration_secs * 1000.0) as u64)
        .unwrap_or(0);
    let runtime_secs = total_ms as f64 / 1000.0;

    let cfg = crate::AppConfig::load();
    let ext = match &req.mode {
        RipMode::Remux { container } | RipMode::Encode { container, .. } => {
            if matches!(container, Container::Mp4) {
                "mp4"
            } else {
                "mkv"
            }
        }
    };
    let disc_name = clean_movie_name(&info.name, "Blu-ray");
    let movie_query = clean_movie_name(req.movie.as_deref().unwrap_or(&info.name), "Blu-ray");

    let mut ctx = TrackCtx {
        streams: disc.streams(title).unwrap_or_default(),
        ..Default::default()
    };
    for s in &ctx.streams {
        tracing::info!(kind = %s.kind, lang = ?s.lang, coding_type = s.coding_type, format = s.format, "stream");
    }
    ctx.default_audio = pick_default_audio(&ctx.streams);

    // TMDB is needed for the poster and/or for template placeholders.
    let tpl = cfg.output_template.clone();
    let wants_tmdb = req.poster || ["{title", "{year", "{tmdb"].iter().any(|k| tpl.contains(k));
    let tmdb = if wants_tmdb {
        match metadata::lookup_movie(&movie_query, runtime_secs) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "tmdb lookup failed");
                None
            }
        }
    } else {
        None
    };
    let movie = tmdb
        .as_ref()
        .map(|m| m.title.clone())
        .unwrap_or_else(|| movie_query.clone());

    let marks = disc.chapters(title).unwrap_or_default();
    let audio_stream = ctx
        .default_audio
        .and_then(|i| ctx.streams.iter().filter(|s| s.kind == "audio").nth(i));
    let video_stream = ctx.streams.iter().find(|s| s.kind == "video");
    let vars = template::Vars::build(
        &disc_name,
        &movie,
        tmdb.as_ref().and_then(|m| m.year.as_deref()),
        tmdb.as_ref().map(|m| m.tmdb_id),
        title_info.and_then(|t| t.resolution.as_deref()),
        title_info.and_then(|t| t.dynamic_range.as_deref()),
        video_stream.map(|s| video_codec_label(s.coding_type)),
        audio_stream
            .and_then(|s| s.lang.as_deref())
            .map(lang_name)
            .as_deref(),
        audio_stream.map(|s| audio_codec_label(s.coding_type)),
        runtime_secs,
        marks.len(),
        title_info.map(|t| t.playlist).unwrap_or(0),
        title,
        ext,
    );
    // `None` (or a directory) means "use the configured folder + template".
    ctx.dest = match &req.output {
        Some(p) if p.is_dir() || p.to_string_lossy().ends_with('/') => {
            p.join(template::render(&tpl, &vars, ext, "{title} - {resolution}"))
        }
        Some(p) => p.clone(),
        None => cfg
            .output_dir
            .join(template::render(&tpl, &vars, ext, "{title} - {resolution}")),
    };
    if let Some(dir) = ctx.dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    tracing::info!(dest = %ctx.dest.display(), "output");

    let marker = ctx.dest.with_extension("chapters.ffmeta");
    if req.chapters && !marks.is_empty() {
        let names = metadata::chapter_names(&movie, runtime_secs, marks.len())
            .unwrap_or(None)
            .unwrap_or_default();
        tracing::info!(marks = marks.len(), names = names.len(), %movie, "chapters");
        write_chapters(&marks, &names, total_ms, &marker)?;
        ctx.chapters_file = Some(marker.clone());
    }

    if req.poster {
        if let Some(m) = &tmdb {
            if let Some(url) = &m.poster_url {
                match metadata::download(url) {
                    Ok(bytes) => {
                        let dest = ctx.dest.with_extension("poster.jpg");
                        std::fs::write(&dest, &bytes)?;
                        tracing::info!(?dest, tmdb = m.tmdb_id, year = ?m.year, "poster saved");
                        ctx.poster_file = Some(dest);
                    }
                    Err(e) => tracing::warn!(error = %e, "poster download failed"),
                }
            }
        } else {
            tracing::info!("no tmdb match - no poster");
        }
    }

    disc.select(title)?;

    let args = build_args(req, &ctx);
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
        dest: Some(ctx.dest.clone()),
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
    // A drive read error mid-stream makes libbluray end early; ffmpeg then still
    // exits 0, so compare what we got against the title duration.
    let short = total_ms > 0 && prog.out_time_ms + 2_000 < total_ms;
    if short {
        tracing::warn!(
            got_ms = prog.out_time_ms,
            want_ms = total_ms,
            "output is shorter than the title - disc read error?"
        );
    }
    if let Some(cf) = &ctx.chapters_file {
        let _ = std::fs::remove_file(cf);
    }
    prog.done = true;
    prog.percent = 100.0;
    prog.message = if short {
        format!(
            "truncated: {} of {}",
            human_time(prog.out_time_ms),
            human_time(total_ms)
        )
    } else {
        "done".into()
    };
    on_progress(prog);
    Ok(())
}

/// Convenience: does the disc at `path` look like a Blu-ray?
pub fn is_bluray(path: &Path) -> bool {
    path.join("BDMV").is_dir() || path.join("bdmv").is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(kind: &str, lang: &str, ct: u8, fmt: u8) -> StreamInfo {
        StreamInfo {
            kind: kind.into(),
            lang: if lang.is_empty() { None } else { Some(lang.into()) },
            coding_type: ct,
            format: fmt,
            rate: 0,
        }
    }

    /// 2160p + 1080p video, eng TrueHD (with core) + eng AC3 + fra AC3,
    /// eng + fra subtitles.
    fn sample() -> TrackCtx {
        TrackCtx {
            streams: vec![
                st("video", "", 0x24, 8),
                st("video", "", 0x24, 6),
                st("audio", "eng", 0x83, 6),
                st("audio", "eng", 0x81, 6),
                st("audio", "fra", 0x81, 6),
                st("subtitle", "eng", 0x90, 0),
                st("subtitle", "fra", 0x90, 0),
            ],
            default_audio: Some(0),
            ..Default::default()
        }
    }

    /// The disposition following `spec` in the flat meta list.
    fn dispo<'a>(meta: &'a [String], spec: &str) -> &'a str {
        let i = meta.iter().position(|m| m == spec).expect("spec present");
        &meta[i + 1]
    }

    #[test]
    fn truehd_core_shifts_following_audio_indices() {
        let (maps, meta) = plan_tracks(&sample(), AudioSel::All, SubSel::None);
        // eng TrueHD a:0, its embedded core a:1, eng AC3 a:2, fra AC3 a:3
        assert_eq!(
            maps,
            vec!["0:v:0", "0:v:1", "0:a:0", "0:a:1", "0:a:2", "0:a:3"]
        );
        assert_eq!(dispo(&meta, "-disposition:a:0"), "default");
        assert_eq!(dispo(&meta, "-disposition:a:1"), "0");
        // only the first audio is default
        assert_eq!(meta.iter().filter(|m| *m == "default").count(), 2); // video + audio
        assert!(meta.contains(&"title=English TrueHD".to_string()));
        assert!(meta.contains(&"title=English AC3 (core)".to_string()));
    }

    #[test]
    fn english_keeps_truehd_plus_core_and_english_ac3() {
        let (maps, meta) = plan_tracks(&sample(), AudioSel::English, SubSel::None);
        assert_eq!(maps, vec!["0:v:0", "0:v:1", "0:a:0", "0:a:1", "0:a:2"]);
        assert!(!meta.contains(&"title=French AC3".to_string()));
        assert!(!meta.iter().any(|m| m.starts_with("-metadata:s:s:")));
    }

    #[test]
    fn main_audio_is_the_default_only() {
        let (maps, meta) = plan_tracks(&sample(), AudioSel::Main, SubSel::None);
        assert_eq!(maps, vec!["0:v:0", "0:v:1", "0:a:0", "0:a:1"]);
        assert!(meta.contains(&"title=English TrueHD".to_string()));
        assert!(!meta.contains(&"title=English AC3".to_string()));
    }

    #[test]
    fn subtitles_are_mapped_and_never_default() {
        let (maps, meta) = plan_tracks(&sample(), AudioSel::Main, SubSel::All);
        assert_eq!(maps, vec!["0:v:0", "0:v:1", "0:a:0", "0:a:1", "0:s:0", "0:s:1"]);
        assert_eq!(dispo(&meta, "-disposition:s:0"), "0");
        assert_eq!(dispo(&meta, "-disposition:s:1"), "0");
        assert!(meta.contains(&"title=English (subtitles)".to_string()));
    }

    #[test]
    fn english_subs_only() {
        let (maps, _) = plan_tracks(&sample(), AudioSel::Main, SubSel::English);
        assert_eq!(maps, vec!["0:v:0", "0:v:1", "0:a:0", "0:a:1", "0:s:0"]);
    }

    #[test]
    fn no_subs() {
        let (maps, meta) = plan_tracks(&sample(), AudioSel::Main, SubSel::None);
        assert!(!maps.iter().any(|m| m.starts_with("0:s:")));
        assert!(!meta.iter().any(|m| m.starts_with("-metadata:s:s:")));
    }

    #[test]
    fn video_is_labelled_once_and_default() {
        let (_, meta) = plan_tracks(&sample(), AudioSel::Main, SubSel::None);
        assert!(meta.contains(&"title=2160p HEVC".to_string()));
        assert_eq!(meta.iter().filter(|m| m.starts_with("-metadata:s:v:")).count(), 2);
    }
}
