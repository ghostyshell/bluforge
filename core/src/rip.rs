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
    /// Strip the Cinavia watermark (re-encodes the audio to FLAC).
    #[serde(default)]
    pub cinavia: bool,
}

impl Default for EncodeSettings {
    fn default() -> Self {
        Self {
            codec: VideoCodec::X265,
            quality: 20,
            preset: Some("medium".into()),
            ten_bit: true,
            aac_audio: false,
            cinavia: false,
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
    /// Download cover art beside the output when a source has one.
    #[serde(default = "yes")]
    pub covers: bool,
    /// Rip as TV: one file per episode, named SxxExx with the episode title.
    #[serde(default)]
    pub tv: bool,
    /// Series name override for episode lookups.
    #[serde(default)]
    pub series: Option<String>,
    /// Episode-number filter, eg. "1-3" or "1,2,5". Empty = all on the disc.
    #[serde(default)]
    pub episodes: Option<String>,
    /// First episode number on this disc (overrides the remembered mapping).
    #[serde(default)]
    pub start_episode: Option<u32>,
    /// Strip the Cinavia watermark from the kept audio. Audio is re-encoded
    /// because a copy cannot carry the filter.
    #[serde(default)]
    pub cinavia: bool,
    /// Codec for the stripped audio; `None` follows the source format.
    #[serde(default)]
    pub cinavia_codec: Option<crate::cinavia::Codec>,
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
    /// Cover files saved next to the output.
    #[serde(default)]
    pub covers: Vec<String>,
}

pub fn ffmpeg_path() -> PathBuf {
    if let Some(p) = crate::env_compat("FFMPEG") {
        return PathBuf::from(p);
    }
    crate::binaries::helper("ffmpeg").unwrap_or_else(|| PathBuf::from("ffmpeg"))
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

/// TrueHD carries an embedded AC-3 core that ffmpeg exposes as an *extra* audio
/// stream of the same PID. That shifts every later ffmpeg audio index, so
/// mapping/labelling has to account for it.
///
/// DTS-HD (0x85/0x86) is different: its lossy core travels inside the same
/// stream, and ffmpeg exposes it as a single audio stream. Adding a core map
/// there makes ffmpeg fail with `Stream map '0:a:N' matches no streams`.
fn has_core(coding_type: u8) -> bool {
    coding_type == 0x83
}

fn core_codec_label(_coding_type: u8) -> &'static str {
    "AC3"
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

/// Rip a disc as TV: one ffmpeg pass with the segment muxer, cutting at the
/// episode (clip) boundaries, then rename to `SxxExx - Title`.

/// Split a play-all title into one raw `.m2ts` per clip (episode), without
/// relying on timestamps: the clips restart their PTS at zero, so both `-t` and
/// the segment muxer cut in the wrong place. Clip boundaries come from a PTS
/// reset or from the planned clip duration, whichever comes first.
fn split_clips<F>(
    req: &RipRequest,
    wanted: &[&crate::tv::PlanItem],
    plan: &crate::tv::Plan,
    tmp_dir: &std::path::Path,
    backend: crate::disc::DecryptBackend,
    prog: &mut RipProgress,
    on_progress: &mut F,
) -> Result<Vec<std::path::PathBuf>, CoreError>
where
    F: FnMut(RipProgress),
{
    use std::io::Write as _;

    let title = wanted[0].title_index;
    let first_idx = plan
        .items
        .iter()
        .position(|i| (i.start_secs - wanted[0].start_secs).abs() < 0.001)
        .unwrap_or(0);
    let last_wanted = first_idx + wanted.len() - 1;

    let disc = crate::disc::Disc::open_with_backend(&req.source, req.keydb.as_deref(), backend)?;
    let _ = disc.info()?;
    disc.select(title)?;
    // When the wanted range does not start at the first clip, jump to it: the
    // splitter only writes wanted clips anyway, so there is no reason to read
    // the earlier ones.
    let start_secs = wanted[0].start_secs;
    if start_secs > 0.5 {
        let _ = disc.seek(start_secs);
    }
    let video_pids: Vec<u16> = disc
        .streams(title)
        .unwrap_or_default()
        .iter()
        .filter(|s| s.kind == "video")
        .map(|s| s.pid)
        .collect();

    let mut reader = TitleReader::new(&disc);
    let mut buf = vec![0u8; 1 << 20];
    let mut stride = 0usize;
    let mut sync = 0usize;
    let mut skip = 0usize;
    let mut clip = if start_secs > 0.5 { first_idx } else { 0 };
    let mut base: Option<u64> = None;
    let mut last_pts = 0u64;
    let mut current: Option<(std::fs::File, Vec<u8>)> = None;
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    let mut read_total = 0u64;

    let expected = |i: usize| plan.items.get(i).map(|x| x.duration_secs).unwrap_or(0.0);
    let wanted_clip = |i: usize| i >= first_idx && i <= last_wanted;

    'outer: loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => return Err(CoreError::Disc(format!("disc read: {e}"))),
        };
        read_total += n as u64;

        if stride == 0 {
            let limit = n.min(8192);
            for off in 0..limit.saturating_sub(384) {
                if buf[off] != 0x47 {
                    continue;
                }
                if buf.get(off + 188) == Some(&0x47) && buf.get(off + 376) == Some(&0x47) {
                    stride = 188;
                    sync = off;
                    break;
                }
                if buf.get(off + 192) == Some(&0x47) && buf.get(off + 384) == Some(&0x47) {
                    stride = 192;
                    // `off` is the TS sync byte; the 4-byte m2ts timestamp header
                    // sits immediately before it, so the packet starts at off-4.
                    let start = off.saturating_sub(4);
                    skip = start;
                    sync = off - start;
                    break;
                }
            }
            if stride == 0 {
                stride = 192;
                sync = 4;
            }
            tracing::info!(stride, sync, skip, "tv split: packet alignment");
        }

        let mut i = skip;
        skip = 0;
        while i + stride <= n {
            let p = i + sync;
            if buf[p] != 0x47 {
                i += stride;
                continue;
            }
            let pid = (((buf[p + 1] & 0x1f) as u16) << 8) | buf[p + 2] as u16;
            if pid != 0x1fff {
                let is_video = video_pids.is_empty() || video_pids.contains(&pid);
                if is_video && buf[p + 1] & 0x40 != 0 {
                    let afc = (buf[p + 3] >> 4) & 3;
                    if afc != 2 {
                        let mut off = p + 4;
                        if afc == 3 {
                            off += 1 + buf[p + 4] as usize;
                        }
                        if off + 14 <= i + stride
                            && buf[off] == 0
                            && buf[off + 1] == 0
                            && buf[off + 2] == 1
                            && buf[off + 7] & 0x80 != 0
                        {
                            let b = &buf[off + 9..off + 14];
                            let pts = (((b[0] as u64) & 0x0e) << 29)
                                | ((b[1] as u64) << 22)
                                | (((b[2] as u64) & 0xfe) << 14)
                                | ((b[3] as u64) << 7)
                                | (((b[4] as u64) & 0xfe) >> 1);
                            // The last wanted clip runs to the end of the title:
                            // splitting there would only truncate it.
                            let mut next = false;
                            if clip >= last_wanted {
                                next = false;
                            } else if let Some(b0) = base {
                                if last_pts > pts && last_pts - pts > 90_000 {
                                    next = true; // timestamps restarted: new clip
                                } else if (pts.saturating_sub(b0) as f64) / 90_000.0
                                    >= expected(clip) + 0.5
                                {
                                    next = true; // planned clip length reached
                                }
                            }
                            if next {
                                if let Some((mut f, b)) = current.take() {
                                    let _ = f.write_all(&b);
                                }
                                clip += 1;
                                base = Some(pts);
                                if clip > last_wanted {
                                    break 'outer;
                                }
                                if wanted_clip(clip) {
                                    let path =
                                        tmp_dir.join(format!("clip{:03}.m2ts", clip - first_idx));
                                    let f = std::fs::File::create(&path)?;
                                    paths.push(path);
                                    current = Some((f, Vec::with_capacity(1 << 20)));
                                }
                            } else if base.is_none() {
                                base = Some(pts);
                            }
                            last_pts = pts;
                        }
                    }
                }
                if wanted_clip(clip) {
                    if current.is_none() && clip >= first_idx {
                        let path = tmp_dir.join(format!("clip{:03}.m2ts", clip - first_idx));
                        let f = std::fs::File::create(&path)?;
                        paths.push(path);
                        current = Some((f, Vec::with_capacity(1 << 20)));
                    }
                    if let Some((_, b)) = current.as_mut() {
                        b.extend_from_slice(&buf[i..i + stride]);
                    }
                }
            }
            i += stride;
        }
        if let Some((_, b)) = current.as_mut() {
            if b.len() >= (1 << 20) {
                if let Some((f, b)) = current.as_mut() {
                    f.write_all(b)?;
                    b.clear();
                }
            }
        }
        let pct = if last_wanted > first_idx {
            ((clip - first_idx) as f64 / (last_wanted - first_idx) as f64 * 100.0).min(99.0)
        } else {
            50.0
        };
        if (pct - prog.percent).abs() >= 1.0 {
            prog.percent = pct;
            prog.message = format!("episode {} of {}", clip - first_idx + 1, wanted.len());
            on_progress(prog.clone());
        }
    }

    if let Some((mut f, b)) = current.take() {
        f.write_all(&b)?;
        f.flush().ok();
    }
    tracing::info!(
        clips = paths.len(),
        read_bytes = read_total,
        "tv split: done"
    );
    if paths.is_empty() {
        return Err(CoreError::Disc(
            "no clips split out of the title (disc read failed?)".into(),
        ));
    }
    Ok(paths)
}

fn run_tv<F>(
    plan: &crate::tv::Plan,
    req: &RipRequest,
    cfg: &crate::AppConfig,
    ctx: &mut TrackCtx,
    backend: crate::disc::DecryptBackend,
    _cancel: &std::sync::atomic::AtomicBool,
    on_progress: &mut F,
) -> Result<(), CoreError>
where
    F: FnMut(RipProgress),
{
    // Only the requested episodes.
    let wanted: Vec<&crate::tv::PlanItem> = match &req.episodes {
        Some(spec) if !spec.trim().is_empty() => plan
            .items
            .iter()
            .filter(|i| episode_selected(spec, i.episode.number))
            .collect(),
        _ => plan.items.iter().collect(),
    };
    if wanted.is_empty() {
        return Err(CoreError::Disc("no episodes matched the filter".into()));
    }

    // Output paths: Series/Season NN/Series - SxxExx - Title.mkv
    let safe = |s: &str| crate::covers::sanitize(s);
    let dir = cfg
        .output_dir
        .join(safe(&plan.series))
        .join(format!("Season {:02}", plan.season));
    std::fs::create_dir_all(&dir)?;

    // Subtitle/audio policy: keep both English subtitle tracks, but default the
    // "signs & songs" one (assumed to be the first) when there is English
    // audio, otherwise default the full track for Japanese audio.
    // Which English subtitle track is signs-&-songs? Nothing on the disc says,
    // so count events: the sparse one is signs.
    let signs_pid = probe_sub_events(
        &req.source,
        req.keydb.as_deref(),
        wanted[0].title_index,
        200 * 1024 * 1024,
        backend,
    )
    .unwrap_or_default()
    .into_iter()
    .filter(|(pid, _)| {
        ctx.streams
            .iter()
            .any(|s| s.pid == *pid && s.lang.as_deref() == Some("eng"))
    })
    .min_by_key(|(_, c)| *c)
    .map(|(pid, _)| pid);
    tracing::info!(?signs_pid, "signs-&-songs subtitle track");

    let mut maps = Vec::new();
    let mut meta = Vec::new();
    plan_tracks_tv(ctx, cfg.tv_subs == "signs", signs_pid, &mut maps, &mut meta);

    let total: f64 = wanted.iter().map(|i| i.duration_secs).sum();
    let mut prog = RipProgress {
        total_ms: (total * 1000.0) as u64,
        message: format!("ripping {} episodes", wanted.len()),
        ..Default::default()
    };
    on_progress(prog.clone());

    // Segment boundaries, relative to the first episode being ripped.
    let _first_start = wanted.first().map(|i| i.start_secs).unwrap_or(0.0);
    let mut times = Vec::new();
    let mut acc = 0.0;
    for it in &wanted {
        acc += it.duration_secs;
        times.push(format!("{:.3}", acc));
    }
    times.pop(); // no boundary after the last

    let tmp_dir = std::env::temp_dir().join(format!("keepsake-tv-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp_dir);
    // Chapters of the whole title; sliced per episode below. (Read before the
    // split so the marks cover the title even if the split runs long.)
    let marks = crate::disc::Disc::open_with_backend(&req.source, req.keydb.as_deref(), backend)
        .and_then(|d| {
            let _ = d.info()?;
            d.chapters(wanted[0].title_index)
        })
        .unwrap_or_default();

    // Disc 2's play-all title concatenates clips whose timestamps restart at
    // zero, so segmenting by timestamp splits in the wrong place (and produces
    // files whose duration has nothing to do with their length). Split on the
    // clip boundaries instead: one raw .m2ts per wanted episode.
    let segs = split_clips(
        req,
        &wanted,
        plan,
        &tmp_dir,
        backend,
        &mut prog,
        on_progress,
    )?;

    // Anime (Japanese audio present) gets OP/ED/preview chapters labelled.
    let anime = ctx
        .streams
        .iter()
        .any(|s| s.kind == "audio" && s.lang.as_deref() == Some("jpn"));
    // Per episode: name it from the TV template, write its own chapters, attach
    // its still, and remux (local, so it is quick).
    let cookie = crate::covers::cookie(cfg);
    let mut saved = Vec::new();
    for (i, it) in wanted.iter().enumerate() {
        let Some(from) = segs.get(i).cloned() else {
            continue;
        };
        let vars = crate::template::Vars::tv(
            &plan.series,
            it.episode.season,
            it.episode.number,
            &it.episode.title,
            plan.disc,
        );
        let fallback = format!(
            "{} - S{:02}E{:02} - {}",
            plan.series, it.episode.season, it.episode.number, it.episode.title
        );
        let rel = crate::template::render(&cfg.tv_template, &vars, "mkv", &fallback);
        let dest = cfg.output_dir.join(&rel);
        if let Some(d) = dest.parent() {
            std::fs::create_dir_all(d)?;
        }

        // This episode's chapters, offset to its own start.
        let ch: Vec<crate::disc::Chapter> = marks
            .iter()
            .filter(|c| {
                c.start_secs >= it.start_secs - 1.0
                    && c.start_secs < it.start_secs + it.duration_secs - 0.5
            })
            .map(|c| crate::disc::Chapter {
                start_secs: c.start_secs - it.start_secs,
                name: c.name.clone(),
            })
            .collect();
        let mut ch = ch;
        if anime {
            label_anime_chapters(&mut ch, it.duration_secs);
        }
        if !ch.is_empty() {
            tracing::info!(
                ep = it.episode.number,
                marks = %ch.iter().map(|c| format!("{:.0}:{}", c.start_secs, c.name.clone().unwrap_or_default())).collect::<Vec<_>>().join(" | "),
                "episode chapters"
            );
        }
        let meta_path = tmp_dir.join(format!("ep{i}.ffmeta"));
        let have_ch = !ch.is_empty();
        if have_ch {
            write_chapters(&ch, &[], (it.duration_secs * 1000.0) as u64, &meta_path)?;
        }

        // Still, for the cover attachment (and the sidecar).
        let still_path = tmp_dir.join(format!("ep{i}.jpg"));
        let still_url = it.episode.still.clone().or_else(|| plan.poster.clone());
        let have_still = still_url
            .as_deref()
            .map(|u| crate::covers::fetch(u, cookie.as_deref()).is_ok())
            .unwrap_or(false)
            && std::fs::write(
                &still_path,
                crate::covers::fetch(still_url.as_deref().unwrap(), cookie.as_deref())
                    .map(|(b, _)| b)
                    .unwrap_or_default(),
            )
            .is_ok();

        let mut a2: Vec<String> = vec![
            "-hide_banner".into(),
            "-nostdin".into(),
            "-y".into(),
            "-i".into(),
            from.to_string_lossy().into_owned(),
        ];
        if have_ch {
            a2.extend([
                "-f".into(),
                "ffmetadata".into(),
                "-i".into(),
                meta_path.to_string_lossy().into_owned(),
                "-map".into(),
                "0".into(),
                "-map_chapters".into(),
                "1".into(),
            ]);
        } else {
            a2.extend(["-map".into(), "0".into()]);
        }
        a2.extend(["-c".into(), "copy".into()]);
        // Hard bound: whatever the segment muxer wrote, the episode is this long.
        a2.extend(["-t".into(), format!("{:.3}", it.duration_secs)]);
        if have_still {
            a2.extend([
                "-attach".into(),
                still_path.to_string_lossy().into_owned(),
                "-metadata:s:t".into(),
                "mimetype=image/jpeg".into(),
                "-metadata:s:t".into(),
                "filename=cover.jpg".into(),
            ]);
        }
        a2.push(dest.to_string_lossy().into_owned());
        let st = Command::new(ffmpeg_path())
            .args(&a2)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| CoreError::Disc(format!("remux episode: {e}")))?;
        if !st.success() {
            tracing::warn!(ep = it.episode.number, "episode remux failed");
        }
        let _ = std::fs::remove_file(&from);
        tracing::info!(
            path = %dest.display(),
            chapters = ch.len(),
            still = have_still,
            "episode written"
        );
        prog.covers.push(dest.display().to_string());
        saved.push(dest.clone());

        // Sidecar thumb next to the episode, plus a season poster.
        if req.covers && cfg.auto_cover && have_still {
            let thumb = dest.with_extension("jpg");
            let _ = std::fs::copy(&still_path, &thumb);
        }
    }
    if req.covers && cfg.auto_cover {
        if let Some(p) = &plan.poster {
            if let Ok((bytes, _)) = crate::covers::fetch(p, cookie.as_deref()) {
                let dir = saved
                    .first()
                    .and_then(|s| s.parent().map(|p| p.to_path_buf()))
                    .unwrap_or_default();
                let _ = std::fs::write(dir.join("poster.jpg"), &bytes);
            }
        }
    }

    // Remember the numbering so the next disc continues from here, even when the
    // discs hold different episode counts.
    if let Some(first) = wanted.first() {
        crate::tv::remember_disc(
            &plan.series,
            plan.season,
            plan.disc,
            first.episode.number,
            wanted.len() as u32,
        );
    }

    prog.done = true;
    prog.percent = 100.0;
    prog.message = format!("{} episodes", saved.len());
    prog.dest = saved.first().map(|p| p.clone());
    on_progress(prog);
    Ok(())
}

/// Count subtitle *event starts* per pid over the first `budget` bytes of a
/// title. The signs-&-songs track carries far fewer than the full track, so the
/// count tells them apart without any disc metadata (there is none).
pub fn probe_sub_events(
    source: &std::path::Path,
    keydb: Option<&std::path::Path>,
    title: u32,
    budget: u64,
    backend: crate::disc::DecryptBackend,
) -> Result<Vec<(u16, u64)>, CoreError> {
    let disc = crate::disc::Disc::open_with_backend(source, keydb, backend)?;
    let info = disc.info()?;
    let subs: Vec<u16> = disc
        .streams(title)?
        .into_iter()
        .filter(|s| s.kind == "subtitle")
        .map(|s| s.pid)
        .collect();
    if subs.is_empty() {
        return Ok(Vec::new());
    }
    let _ = info;
    disc.select(title)?;
    let mut reader = TitleReader::new(&disc);
    let mut buf = vec![0u8; 1 << 20];
    let mut stride = 0usize;
    let mut read = 0u64;
    let mut counts: std::collections::HashMap<u16, u64> = subs.iter().map(|p| (*p, 0)).collect();
    while read < budget {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        read += n as u64;
        if stride == 0 {
            stride = if n >= 5 && buf[4] == 0x47 { 192 } else { 188 };
        }
        let sync = if stride == 192 { 4 } else { 0 };
        let mut i = 0usize;
        while i + stride <= n {
            if buf[i + sync] == 0x47 {
                let pid = (((buf[i + sync + 1] & 0x1f) as u16) << 8) | buf[i + sync + 2] as u16;
                if buf[i + sync + 1] & 0x40 != 0 {
                    if let Some(c) = counts.get_mut(&pid) {
                        *c += 1;
                    }
                }
            }
            i += stride;
        }
    }
    let mut out: Vec<(u16, u64)> = counts.into_iter().collect();
    out.sort_by_key(|(_, c)| *c);
    tracing::info!(?out, budget, "subtitle event probe");
    Ok(out)
}

/// Anime episodes have a fixed shape: a ~90s opening early, an ending in the
/// last third, and a short next-episode preview at the very end. The disc's
/// chapter marks land on those transitions, so label them. Only used when the
/// disc carries Japanese audio (an anime sign).
pub fn label_anime_chapters(ch: &mut [crate::disc::Chapter], duration: f64) {
    if ch.len() < 3 || duration < 600.0 {
        return;
    }
    // (index, gap to the next chapter, or to the end)
    let gaps: Vec<(usize, f64)> = (0..ch.len())
        .map(|i| {
            let end = ch.get(i + 1).map(|c| c.start_secs).unwrap_or(duration);
            (i, end - ch[i].start_secs)
        })
        .collect();

    // Opening: a distinct ~90s OP chapter when the disc has one. Otherwise the
    // first chapter carries a cold open plus the OP (2:35-4:00 on One Punch Man),
    // so label that whole block instead of leaving it as "Chapter 01".
    // Prefer a dedicated OP chapter after the cold open; a ~2 minute chapter at
    // the very start is more likely the cold open itself.
    let op = gaps
        .iter()
        .skip(1)
        .find(|(i, g)| (75.0..=125.0).contains(g) && ch[*i].start_secs <= duration * 0.35)
        .map(|(i, _)| *i)
        .or_else(|| {
            let g = gaps[0].1;
            (ch[0].start_secs <= 5.0 && (75.0..=420.0).contains(&g)).then_some(0)
        });
    if let Some(i) = op {
        ch[i].name = Some("Opening".into());
    }
    // Ending: the first ~90s block in the last third (the creditless ED is 91.2s
    // on every episode of this disc). Taking the first rather than the last keeps
    // an adjacent preview block from stealing the label.
    let ending = gaps
        .iter()
        .find(|(i, g)| (70.0..=125.0).contains(g) && ch[*i].start_secs >= duration * 0.6)
        .map(|(i, _)| *i);
    if let Some(i) = ending {
        if ch[i].name.is_none() {
            ch[i].name = Some("Ending".into());
        }
    }
    // Preview: the block right after the ending, else a short trailing block.
    let preview = match ending {
        Some(e) if e + 1 < ch.len() && (8.0..=100.0).contains(&gaps[e + 1].1) => Some(e + 1),
        _ => gaps
            .iter()
            .rev()
            .find(|(i, g)| (8.0..=75.0).contains(g) && ch[*i].start_secs >= duration * 0.75)
            .map(|(i, _)| *i),
    };
    if let Some(i) = preview {
        if ch[i].name.is_none() {
            ch[i].name = Some("Preview (next episode)".into());
        }
    }
}

/// "1-3" or "1,2,5" or "4".
fn episode_selected(spec: &str, n: u32) -> bool {
    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((a, b)) = part.split_once('-') {
            let (Ok(a), Ok(b)) = (a.trim().parse::<u32>(), b.trim().parse::<u32>()) else {
                continue;
            };
            if n >= a && n <= b {
                return true;
            }
        } else if let Ok(v) = part.parse::<u32>() {
            if v == n {
                return true;
            }
        }
    }
    false
}

/// Track maps/metadata for a TV rip: all video, English audio first (default),
/// both English subtitle tracks with the signs one default when the audio is
/// English, otherwise the full track.
fn plan_tracks_tv(
    ctx: &TrackCtx,
    signs_only: bool,
    signs_pid: Option<u16>,
    maps: &mut Vec<String>,
    meta: &mut Vec<String>,
) {
    let has_eng_audio = ctx
        .streams
        .iter()
        .any(|s| s.kind == "audio" && s.lang.as_deref() == Some("eng"));
    let eng_subs: Vec<usize> = ctx
        .streams
        .iter()
        .enumerate()
        .filter(|(_, s)| s.kind == "subtitle" && s.lang.as_deref() == Some("eng"))
        .map(|(i, _)| i)
        .collect();
    // The probe picks the signs-&-songs track (fewest events); fall back to the
    // first English track when we have no probe.
    let signs_idx = signs_pid
        .and_then(|pid| ctx.streams.iter().position(|s| s.pid == pid))
        .filter(|i| eng_subs.contains(i));
    let default_sub = if has_eng_audio {
        signs_idx.or_else(|| eng_subs.first().copied())
    } else {
        eng_subs.last().copied()
    };
    let default_audio = ctx.streams.iter().position(|s| {
        s.kind == "audio" && s.lang.as_deref() == Some(if has_eng_audio { "eng" } else { "jpn" })
    });

    let (mut ai, mut si, mut out_a, mut out_s) = (0usize, 0usize, 0usize, 0usize);
    for (idx, st) in ctx.streams.iter().enumerate() {
        match st.kind.as_str() {
            "video" => {
                let res = match st.format {
                    8 => "2160p",
                    6 => "1080p",
                    5 => "720p",
                    4 => "1080i",
                    _ => "video",
                };
                maps.push(format!(
                    "0:v:{}",
                    maps.iter().filter(|m| m.starts_with("0:v:")).count()
                ));
                if out_s + out_a == 0 && !maps.is_empty() {
                    // label the first video
                }
                let _ = res;
                meta.extend([
                    format!("-metadata:s:v:0"),
                    format!("title={} {}", res, video_codec_label(st.coding_type)),
                    "-disposition:v:0".into(),
                    "default".into(),
                ]);
            }
            "audio" => {
                let keep = st.lang.as_deref() == Some("eng")
                    || st.lang.as_deref() == Some("jpn")
                    || st.lang.is_none();
                if keep {
                    maps.push(format!("0:a:{}", ai));
                    let lang = st.lang.clone().unwrap_or_default();
                    if !lang.is_empty() {
                        meta.extend([
                            format!("-metadata:s:a:{out_a}"),
                            format!("language={}", lang_tag(&lang)),
                            format!("-metadata:s:a:{out_a}"),
                            format!(
                                "title={} {}",
                                lang_name(&lang),
                                audio_codec_label(st.coding_type)
                            ),
                        ]);
                    }
                    meta.extend([
                        format!("-disposition:a:{out_a}"),
                        if default_audio == Some(idx) {
                            "default".into()
                        } else {
                            "0".into()
                        },
                    ]);
                    out_a += 1;
                }
                ai += 1;
            }
            "subtitle" => {
                // "signs" mode keeps just the default track on an English-audio
                // disc, so the file carries no redundant full subtitles.
                let signs_only_skip =
                    signs_only && has_eng_audio && signs_idx.is_some() && signs_idx != Some(idx);
                let keep = st.lang.as_deref() == Some("eng") && !signs_only_skip;
                if keep {
                    maps.push(format!("0:s:{}", si));
                    let lang = st.lang.clone().unwrap_or_default();
                    let is_default = Some(idx) == default_sub;
                    let label = if Some(idx) == signs_idx {
                        "signs & songs"
                    } else {
                        "full"
                    };
                    meta.extend([
                        format!("-metadata:s:s:{out_s}"),
                        format!("language={}", lang_tag(&lang)),
                        format!("-metadata:s:s:{out_s}"),
                        format!("title={} ({})", lang_name(&lang), label),
                        format!("-disposition:s:{out_s}"),
                        if is_default {
                            "default".into()
                        } else {
                            "0".into()
                        },
                    ]);
                    out_s += 1;
                }
                si += 1;
            }
            _ => {
                si += 1;
            }
        }
    }
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
                        if ctx.default_audio == Some(ai) {
                            "default".into()
                        } else {
                            "0".into()
                        },
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

/// Pump the selected title into an ffmpeg stdin. libbluray emits 192-byte
/// m2ts packets with null-packet (0x1fff) padding; ffmpeg misdetects those as
/// a phantom stream, so they are stripped on the way through.
fn feed_title(
    disc: crate::disc::Disc,
    mut stdin: std::process::ChildStdin,
) -> std::thread::JoinHandle<Result<(), CoreError>> {
    std::thread::spawn(move || -> Result<(), CoreError> {
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
                stride = if n >= 5 && buf[4] == 0x47 { 192 } else { 188 };
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
    })
}

fn build_args(req: &RipRequest, ctx: &TrackCtx, cinavia: &[PreparedAudio]) -> Vec<String> {
    let mkv = matches!(
        &req.mode,
        RipMode::Remux {
            container: Container::Mkv
        } | RipMode::Encode {
            container: Container::Mkv,
            ..
        }
    );

    let mut a: Vec<String> = vec![
        "-hide_banner".into(),
        "-nostdin".into(),
        "-y".into(),
        // The disc timeline starts at a non-zero PTS (600s on this title), so
        // -t only behaves as a duration together with these two flags.
        "-copyts".into(),
        "-start_at_zero".into(),
        "-i".into(),
        "pipe:0".into(),
    ];
    if let Some(cf) = &ctx.chapters_file {
        a.extend([
            "-f".into(),
            "ffmetadata".into(),
            "-i".into(),
            cf.to_string_lossy().into_owned(),
        ]);
    }

    let (mut maps, meta) = plan_tracks(ctx, req.audio, req.subs);
    // MP4 cannot carry PGS subtitles.
    if !mkv {
        maps.retain(|m| !m.starts_with("0:s:"));
    }

    // Cinavia: each kept audio track is pre-filtered into its own input, so the
    // map is re-pointed at that input and the audio gets re-encoded to the
    // codec chosen for it (normally the source format).
    let mut encoders: Vec<Vec<String>> = Vec::new();
    if !cinavia.is_empty() {
        let first = a.iter().filter(|x| x.as_str() == "-i").count();
        for (n, prep) in cinavia.iter().enumerate() {
            // dca and truehd only take a 5.1(side) layout; a decoded WAV carries
            // no layout, and 7.1 has to be folded down anyway.
            if prep.target.channels >= 8 {
                a.push("-channel_layout".into());
                a.push("7.1".into());
            } else if prep.target.channels >= 6 {
                a.push("-channel_layout".into());
                a.push("5.1(side)".into());
            }
            a.push("-i".into());
            a.push(prep.wav.to_string_lossy().into_owned());
            let _ = n;
        }
        for m in maps.iter_mut() {
            if let Some(idx) = m.strip_prefix("0:a:").and_then(|x| x.parse::<usize>().ok()) {
                if let Some(pos) = cinavia.iter().position(|p| p.track == idx) {
                    *m = format!("{}:a:0", first + pos);
                    let out_index = encoders.len();
                    let mut args = crate::cinavia::encode_args(&cinavia[pos].target);
                    // encode_args always opens with a plain `-c:a`; index it to
                    // this output stream.
                    if let Some(first) = args.first_mut() {
                        *first = format!("-c:a:{out_index}");
                    }
                    encoders.push(args);
                }
            }
        }
    }

    match &req.mode {
        RipMode::Remux { container } => match container {
            Container::Mkv => {
                a.extend(map_args(&maps));
                if encoders.is_empty() {
                    a.extend(["-c".into(), "copy".into()]);
                } else {
                    a.extend(["-c:v".into(), "copy".into()]);
                    for e in &encoders {
                        a.extend(e.iter().cloned());
                    }
                    a.extend(["-c:s".into(), "copy".into()]);
                }
            }
            Container::Mp4 => {
                a.extend(map_args(&maps));
                if encoders.is_empty() {
                    a.extend(["-c".into(), "copy".into()]);
                } else {
                    a.extend(["-c:v".into(), "copy".into()]);
                    for e in &encoders {
                        a.extend(e.iter().cloned());
                    }
                }
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
            if !encoders.is_empty() {
                for e in &encoders {
                    a.extend(e.iter().cloned());
                }
            } else if settings.aac_audio {
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
/// Split a trailing `(1967)` or `[1967]` from a menu title.
fn split_year(s: &str) -> (&str, Option<String>) {
    let t = s.trim();
    for (open, close) in [('(', ')'), ('[', ']')] {
        if t.ends_with(close) {
            if let Some(i) = t.rfind(open) {
                let inner = &t[i + 1..t.len() - 1];
                if inner.len() == 4 && inner.chars().all(|c| c.is_ascii_digit()) {
                    return (t[..i].trim(), Some(inner.to_string()));
                }
            }
        }
    }
    (t, None)
}

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

/// One filtered track waiting to be muxed, with the encoder chosen for it.
struct PreparedAudio {
    track: usize,
    wav: PathBuf,
    target: crate::cinavia::Target,
}

/// Decode every kept audio track, measure and strip the Cinavia watermark, and
/// return the processed WAVs with their audio indices. The caller muxes them in
/// place of the originals.
#[allow(clippy::too_many_arguments)]
fn prepare_cinavia(
    source: &std::path::Path,
    keydb: Option<&std::path::Path>,
    backend: crate::disc::DecryptBackend,
    title: u32,
    req: &RipRequest,
    ctx: &TrackCtx,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<(PathBuf, Vec<PreparedAudio>), CoreError> {
    let (maps, _) = plan_tracks(ctx, req.audio, req.subs);
    let tracks: Vec<usize> = maps
        .iter()
        .filter_map(|m| m.strip_prefix("0:a:").and_then(|x| x.parse::<usize>().ok()))
        .collect();
    if tracks.is_empty() {
        return Ok((PathBuf::new(), Vec::new()));
    }

    let dir = std::env::temp_dir().join(format!("keepsake-cinavia-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;

    // One read: decode every kept track to float WAV.
    let raws: Vec<PathBuf> = tracks
        .iter()
        .map(|n| dir.join(format!("raw-{n}.wav")))
        .collect();
    let mut cmd = Command::new(ffmpeg_path());
    cmd.args([
        "-hide_banner",
        "-nostdin",
        "-y",
        "-copyts",
        "-start_at_zero",
        "-i",
        "pipe:0",
    ]);
    for (n, raw) in raws.iter().enumerate() {
        cmd.args(["-map", &format!("0:a:{}", tracks[n])]);
        cmd.args(["-vn", "-sn", "-dn", "-acodec", "pcm_f32le"]);
        cmd.arg(raw);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    let mut child = cmd
        .spawn()
        .map_err(|e| CoreError::Disc(format!("cinavia ffmpeg: {e}")))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| CoreError::Disc("cinavia stdin".into()))?;

    let pre = Disc::open_with_backend(source, keydb, backend)?;
    // Enumerate titles on the fresh handle before selecting; libbluray keeps
    // that per handle.
    let _ = pre.info()?;
    pre.select(title)?;
    let feed = feed_title(pre, stdin);
    let status = child.wait()?;
    let _ = feed.join();
    if !status.success() {
        return Err(CoreError::Disc("cinavia: audio decode failed".into()));
    }

    let mut out = Vec::new();
    for (n, raw) in raws.iter().enumerate() {
        let probe = crate::cinavia::probe(raw, 0).map_err(CoreError::Disc)?;
        let channels = probe.channels;
        let report =
            crate::cinavia::analyze(raw, 0, channels, Some(120)).map_err(CoreError::Disc)?;
        if report.verdict == crate::cinavia::Verdict::Clean {
            tracing::info!(track = tracks[n], "cinavia: track reads clean");
        }
        let mut spec = crate::cinavia::BandSpec::default_for(
            report.band.unwrap_or(crate::cinavia::DEFAULT_BAND),
        );
        spec.depth_db = report.depth_db;
        let dst = dir.join(format!("clean-{n}.wav"));
        crate::cinavia::strip(raw, 0, &dst, spec, channels, cancel, |_| {})
            .map_err(CoreError::Disc)?;

        // The source coding type is on the disc stream, not on the decoded WAV.
        let coding = ctx
            .streams
            .iter()
            .filter(|st| st.kind == "audio")
            .nth(tracks[n])
            .map(|st| st.coding_type)
            .unwrap_or(0x80);
        let (source_family, hd) = crate::cinavia::Codec::from_bd(coding);
        let mkv = !matches!(
            &req.mode,
            RipMode::Remux {
                container: Container::Mp4
            } | RipMode::Encode {
                container: Container::Mp4,
                ..
            }
        );
        let target = crate::cinavia::target_for(
            req.cinavia_codec.unwrap_or(crate::cinavia::Codec::Auto),
            source_family,
            hd,
            None,
            channels,
            mkv,
        )
        .map_err(CoreError::Disc)?;
        if let Some(note) = &target.note {
            tracing::info!(track = tracks[n], %note, "cinavia: encoder choice");
        }
        out.push(PreparedAudio {
            track: tracks[n],
            wav: dst,
            target,
        });
    }
    Ok((dir, out))
}

/// Run the rip. `on_progress` is called on the calling thread for each update.
/// `cancel` is polled between progress lines; when set the ffmpeg child is killed.
pub fn run<F>(
    req_in: &RipRequest,
    cancel: &std::sync::atomic::AtomicBool,
    mut on_progress: F,
) -> Result<(), CoreError>
where
    F: FnMut(RipProgress),
{
    let _awake = if crate::config::AppConfig::load().prevent_sleep {
        Some(crate::sleep_guard::prevent())
    } else {
        None
    };
    if let Some(dir) = req_in.output.as_ref().and_then(|p| p.parent()) {
        std::fs::create_dir_all(dir)?;
    }
    // Native libaacs cannot decrypt while the volume is mounted, so open the raw
    // device and unmount it for the rip; libmmbd only needs the raw device.
    let backend = Disc::plan_backend(&req_in.source);
    let disc_id = Disc::disc_id_for_source(&req_in.source);
    let keyed = disc_id
        .as_deref()
        .map(|i| crate::keys::has(i).unwrap_or(false))
        .unwrap_or(false);
    let (source, remount) = crate::disc::prepare_source(&req_in.source, false);
    let _remount = crate::disc::RemountGuard(remount);
    let mut owned = req_in.clone();
    owned.source = source;
    let req: &RipRequest = &owned;

    // Either the request flag or the encode setting asks for a strip; the app
    // sets the setting because it renders next to the other audio options.
    let want_cinavia =
        req.cinavia || matches!(&req.mode, RipMode::Encode { settings, .. } if settings.cinavia);

    let disc = Disc::open_with_backend(&req.source, req.keydb.as_deref(), backend)?;
    let info = disc.info()?;
    let (aacs_lib, _) = Disc::decryption_backend();
    tracing::info!(
        backend = backend.label(),
        aacs_lib = aacs_lib.as_deref().unwrap_or("(none)"),
        disc_id = ?disc_id,
        keyed,
        "decrypt backend"
    );
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
    // A saved menu document for this disc supplies the title name and chapter
    // labels, so editing a template actually changes the next rip.
    let menu_doc = disc_id
        .as_deref()
        .and_then(|id| crate::menu::find_by_disc_id(&crate::menu::store_dir(), id))
        .map(|(_, doc)| doc);
    let menu_title = menu_doc
        .as_ref()
        .and_then(|d| d.titles.iter().find(|t| t.index == title));
    let menu_name = menu_title
        .and_then(|t| t.name.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let (menu_base, menu_year) = match menu_name {
        Some(n) => {
            let (base, year) = split_year(n);
            (base.to_string(), year)
        }
        None => (String::new(), None),
    };
    if menu_doc.is_some() {
        tracing::info!(title, name = ?menu_name, "using saved menu template");
    }
    let query_src = req
        .movie
        .as_deref()
        .or((!menu_base.is_empty()).then_some(menu_base.as_str()))
        .unwrap_or(&info.name);
    let movie_query = clean_movie_name(query_src, "Blu-ray");

    let mut ctx = TrackCtx {
        streams: disc.streams(title).unwrap_or_default(),
        ..Default::default()
    };
    for s in &ctx.streams {
        tracing::info!(kind = %s.kind, lang = ?s.lang, pid = s.pid, coding_type = s.coding_type, format = s.format, "stream");
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
        .or_else(|| req.movie.clone())
        .or_else(|| (!menu_base.is_empty()).then(|| menu_base.clone()))
        .unwrap_or_else(|| movie_query.clone());
    let year = tmdb.as_ref().and_then(|m| m.year.clone()).or(menu_year);

    let marks = disc.chapters(title).unwrap_or_default();
    let audio_stream = ctx
        .default_audio
        .and_then(|i| ctx.streams.iter().filter(|s| s.kind == "audio").nth(i));
    let video_stream = ctx.streams.iter().find(|s| s.kind == "video");
    let vars = template::Vars::build(
        &disc_name,
        &movie,
        year.as_deref(),
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
    // A path that is an existing dir, ends in `/`, or has no extension is a
    // folder to render the template into; anything else is the exact file.
    ctx.dest = match &req.output {
        Some(p) if p.is_dir() || p.to_string_lossy().ends_with('/') || p.extension().is_none() => {
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

    // TV / anime: one file per episode, from the title's clips.
    if req.tv {
        match crate::tv::plan(&disc, &info, req.series.as_deref(), req.start_episode) {
            Ok(plan) if plan.items.len() >= 2 => {
                return run_tv(
                    &plan,
                    req,
                    &cfg,
                    &mut ctx,
                    backend,
                    cancel,
                    &mut on_progress,
                );
            }
            Ok(plan) => tracing::info!(
                found = plan.items.len(),
                "tv: not enough episodes, ripping as one title"
            ),
            Err(e) => tracing::warn!(error = %e, "tv: episode planning failed"),
        }
    }

    let marker = ctx.dest.with_extension("chapters.ffmeta");
    if req.chapters && !marks.is_empty() {
        let names = match menu_title
            .filter(|t| !t.chapter_names.is_empty() && t.chapter_names.len() == marks.len())
        {
            Some(t) => t.chapter_names.clone(),
            None => metadata::chapter_names(&movie, runtime_secs, marks.len())
                .unwrap_or(None)
                .unwrap_or_default(),
        };
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

    // Cinavia: pre-filter each kept audio track into a temp WAV. This reads the
    // disc once more (audio only), so it is opt-in.
    let mut cinavia_temps: Vec<PreparedAudio> = Vec::new();
    let mut cinavia_dir: Option<PathBuf> = None;
    if want_cinavia {
        match prepare_cinavia(
            &req.source,
            req.keydb.as_deref(),
            backend,
            title,
            req,
            &ctx,
            cancel,
        ) {
            Ok((dir, temps)) => {
                cinavia_dir = Some(dir);
                cinavia_temps = temps;
            }
            Err(e) => {
                tracing::warn!(error = %e, "cinavia: pre-pass failed, keeping the original audio");
                on_progress(RipProgress {
                    total_ms,
                    message: format!("cinavia: {e}"),
                    dest: Some(ctx.dest.clone()),
                    ..Default::default()
                });
            }
        }
    }

    let args = build_args(req, &ctx, &cinavia_temps);
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

    let feed = feed_title(disc, stdin);

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
    if let Some(dir) = &cinavia_dir {
        let _ = std::fs::remove_dir_all(dir);
    }

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
    if req.covers && cfg.auto_cover {
        let found = crate::covers::auto_covers(&movie, &ctx.dest, &cfg);
        prog.covers = found.iter().map(|s| s.path.display().to_string()).collect();
        if found.is_empty() {
            tracing::info!("no cover art found");
        }
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
            lang: if lang.is_empty() {
                None
            } else {
                Some(lang.into())
            },
            coding_type: ct,
            format: fmt,
            rate: 0,
            pid: 0,
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
    fn splits_a_trailing_year_from_a_menu_title() {
        assert_eq!(
            split_year("The Jungle Book (1967)"),
            ("The Jungle Book", Some("1967".into()))
        );
        assert_eq!(split_year("Movie [2001]"), ("Movie", Some("2001".into())));
        assert_eq!(split_year("No Year"), ("No Year", None));
        assert_eq!(
            split_year("  Se7en (1995)  "),
            ("Se7en", Some("1995".into()))
        );
        assert_eq!(split_year("Year (99)"), ("Year (99)", None));
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
    fn dts_hd_is_one_stream_without_a_core_map() {
        // ffmpeg exposes DTS-HD MA as a single audio stream (its core is inside
        // it), unlike TrueHD. A phantom core map makes ffmpeg fail at open.
        let ctx = TrackCtx {
            streams: vec![
                st("video", "", 0x24, 8),
                st("audio", "eng", 0x86, 6),
                st("audio", "eng", 0x81, 6),
            ],
            default_audio: Some(0),
            ..Default::default()
        };
        let (maps, meta) = plan_tracks(&ctx, AudioSel::All, SubSel::None);
        assert_eq!(maps, vec!["0:v:0", "0:a:0", "0:a:1"]);
        assert!(meta.contains(&"title=English DTS-HD".to_string()));
        assert!(!meta.iter().any(|m| m.contains("(core)")));
    }

    #[test]
    fn english_keeps_truehd_plus_core_and_english_ac3() {
        let (maps, meta) = plan_tracks(&sample(), AudioSel::English, SubSel::None);
        assert_eq!(maps, vec!["0:v:0", "0:v:1", "0:a:0", "0:a:1", "0:a:2"]);
        assert!(!meta.contains(&"title=French AC3".to_string()));
        assert!(!meta.iter().any(|m| m.starts_with("-metadata:s:s:")));
    }

    fn chapters_at(starts: &[f64]) -> Vec<crate::disc::Chapter> {
        starts
            .iter()
            .map(|s| crate::disc::Chapter {
                start_secs: *s,
                name: None,
            })
            .collect()
    }

    fn name_at(ch: &[crate::disc::Chapter], i: usize) -> Option<String> {
        ch.get(i).and_then(|c| c.name.clone())
    }

    #[test]
    fn anime_labels_a_standalone_opening_chapter() {
        // One Punch Man E01: 0, 99 (OP end), 891, 1331 (ED), 1420 (preview)
        let mut ch = chapters_at(&[0.0, 99.0, 891.0, 1331.0, 1420.0]);
        label_anime_chapters(&mut ch, 1461.0);
        assert_eq!(name_at(&ch, 0).as_deref(), Some("Opening"));
        assert_eq!(name_at(&ch, 3).as_deref(), Some("Ending"));
        assert_eq!(name_at(&ch, 4).as_deref(), Some("Preview (next episode)"));
    }

    #[test]
    fn anime_labels_a_cold_open_plus_opening_block() {
        // E03/E04: the first chapter runs 2:35-4:00 (cold open + OP).
        let mut ch = chapters_at(&[0.0, 239.0, 749.0, 1321.0, 1411.0]);
        label_anime_chapters(&mut ch, 1461.0);
        assert_eq!(name_at(&ch, 0).as_deref(), Some("Opening"));
        assert_eq!(name_at(&ch, 3).as_deref(), Some("Ending"));
        assert_eq!(name_at(&ch, 4).as_deref(), Some("Preview (next episode)"));

        let mut ch = chapters_at(&[0.0, 155.0, 711.0, 1330.0, 1420.0]);
        label_anime_chapters(&mut ch, 1461.0);
        assert_eq!(name_at(&ch, 0).as_deref(), Some("Opening"));
    }

    #[test]
    fn anime_prefers_a_separate_op_over_a_cold_open() {
        // A 2:00 cold open followed by its own 90s OP chapter.
        let mut ch = chapters_at(&[0.0, 120.0, 210.0, 900.0, 1331.0, 1420.0]);
        label_anime_chapters(&mut ch, 1461.0);
        assert_eq!(name_at(&ch, 0), None);
        assert_eq!(name_at(&ch, 1).as_deref(), Some("Opening"));
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
        assert_eq!(
            maps,
            vec!["0:v:0", "0:v:1", "0:a:0", "0:a:1", "0:s:0", "0:s:1"]
        );
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
        assert_eq!(
            meta.iter()
                .filter(|m| m.starts_with("-metadata:s:v:"))
                .count(),
            2
        );
    }
}
