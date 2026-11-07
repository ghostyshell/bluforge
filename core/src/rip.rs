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

    match &req.mode {
        RipMode::Remux { container } => match container {
            Container::Mkv => a.extend(["-map", "0", "-c", "copy"].map(String::from)),
            Container::Mp4 => {
                a.extend(["-map", "0:v", "-map", "0:a", "-c", "copy"].map(String::from))
            }
        },
        RipMode::Encode { container, settings } => {
            a.extend(["-map".into(), "0:v:0".into(), "-map".into(), "0:a?".into()]);
            if matches!(container, Container::Mkv) {
                a.extend(["-map".into(), "0:s?".into()]);
            }
            a.extend(codec_args(settings));
            if settings.aac_audio {
                a.extend(["-c:a".into(), "aac".into(), "-b:a".into(), "640k".into()]);
            } else {
                a.extend(["-c:a".into(), "copy".into()]);
            }
            if matches!(container, Container::Mkv) {
                a.extend(["-c:s".into(), "copy".into()]);
            } else {
                a.extend(["-sn".into()]);
            }
        }
    }

    if mkv {
        if ctx.chapters_file.is_some() {
            a.extend(["-map_chapters".into(), "1".into()]);
        }
        // Per-stream language + title, and default flags. Subtitles stay
        // disabled by default (no `default` disposition); the chosen audio
        // track - English, non-commentary - is the only default audio.
        // Language of the default track, used to spot same-language commentary.
        let primary_lang = ctx
            .default_audio
            .and_then(|i| {
                ctx.streams
                    .iter()
                    .filter(|s| s.kind == "audio")
                    .nth(i)
            })
            .and_then(|s| s.lang.clone())
            .unwrap_or_default();
        let (mut v, mut au, mut s) = (0usize, 0usize, 0usize);
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
                    a.extend([
                        format!("-metadata:s:v:{v}"),
                        format!("title={}", label.trim()),
                        format!("-metadata:s:v:{v}"),
                        "language=und".into(),
                        format!("-disposition:v:{v}"),
                        "default".into(),
                    ]);
                    v += 1;
                }
                "audio" => {
                    if lang.is_empty() {
                        au += 1;
                        continue;
                    }
                    let mut label = format!("{} {}", lang_name(&lang), audio_codec_label(st.coding_type));
                    if let Some(ch) = audio_chans(st.format) {
                        label.push(' ');
                        label.push_str(ch);
                    }
                    if is_secondary(st.coding_type)
                        || (audio_chans(st.format) == Some("2.0")
                            && !is_primary_audio(st.coding_type)
                            && !primary_lang.is_empty()
                            && lang == primary_lang
                            && ctx.default_audio != Some(au))
                    {
                        label.push_str(" (Commentary)");
                    }
                    a.extend([
                        format!("-metadata:s:a:{au}"),
                        format!("language={}", lang_tag(&lang)),
                        format!("-metadata:s:a:{au}"),
                        format!("title={label}"),
                        format!("-disposition:a:{au}"),
                        if ctx.default_audio == Some(au) { "default".into() } else { "0".into() },
                    ]);
                    au += 1;
                }
                _ => {
                    if lang.is_empty() {
                        s += 1;
                        continue;
                    }
                    a.extend([
                        format!("-metadata:s:s:{s}"),
                        format!("language={}", lang_tag(&lang)),
                        format!("-metadata:s:s:{s}"),
                        format!("title={} (subtitles)", lang_name(&lang)),
                        format!("-disposition:s:{s}"),
                        "0".into(),
                    ]);
                    s += 1;
                }
            }
        }
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
    if let Some(cf) = &ctx.chapters_file {
        let _ = std::fs::remove_file(cf);
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
