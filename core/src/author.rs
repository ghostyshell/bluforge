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
    /// Split the inputs across discs of at most this many GB (BD-25 = 25,
    /// BD-50 = 50). Whole inputs stay together, so the result is a set of disc
    /// folders rather than a re-encoded movie cut in half.
    #[serde(default)]
    pub split_gb: Option<f64>,
    /// Add an authored BD-J title menu as first play and top menu. Experimental:
    /// it is verified with libbluray, not on a hardware player.
    #[serde(default)]
    pub menu: bool,
    /// Background image for the authored menu (`BDMV/JAR/menu.png`).
    #[serde(default)]
    pub menu_background: Option<PathBuf>,
    /// Saved menu document whose title names feed the authored menu.
    #[serde(default)]
    pub menu_template: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthorProgress {
    pub percent: f64,
    pub message: String,
    pub done: bool,
}

/// Locate tsMuxeR: explicit override, the copy the app manages, then PATH.
pub fn tsmuxer_path() -> Option<PathBuf> {
    if let Some(p) = crate::env_compat("TSMUXER") {
        if Path::new(&p).is_file() {
            return Some(PathBuf::from(p));
        }
    }
    if let Some(p) = crate::binaries::helper("tsMuxeR") {
        return Some(p);
    }
    // GUI launches can inherit a minimal PATH; keep the usual Homebrew spots.
    ["/opt/homebrew/bin/tsMuxeR", "/usr/local/bin/tsMuxeR"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
}

fn ffprobe() -> String {
    if let Some(p) = crate::env_compat("FFPROBE") {
        return p;
    }
    crate::binaries::helper_str("ffprobe").unwrap_or_else(|| "ffprobe".to_string())
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
                line = format!(
                    "{codec_tag}, \"{path}\", insertSEI, contSPS, track={track}, lang={lang}"
                );
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
        .unwrap_or_else(|| "KEEPSAKE".to_string())
        .to_uppercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .take(32)
        .collect::<String>();

    let mut s =
        String::from("MUXOPT --blu-ray --no-pcr-on-video-pid --new-audio-pes --vbr --vbv-len=500");
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
    for i in &req.inputs {
        if !i.is_file() {
            return Err(CoreError::Disc(format!("missing input: {}", i.display())));
        }
    }
    let bin = tsmuxer_path().ok_or_else(|| {
        CoreError::Disc(
            "tsMuxeR is not installed (run `keepsake-atelier tsmuxer --install`, or set KEEPSAKE_TSMUXER)"
                .into(),
        )
    })?;

    if let Some(limit) = req.split_gb.filter(|g| *g > 0.0) {
        if req.inputs.len() > 1 {
            return author_split(req, &bin, limit, &mut on_progress);
        }
    }

    author_group(req, &bin, &req.dest, &mut on_progress)?;
    on_progress(AuthorProgress {
        percent: 100.0,
        message: format!("authored {}", req.dest.display()),
        done: true,
    });
    Ok(req.dest.clone())
}

/// A display name for one input.
fn title_name(input: &Path) -> String {
    input
        .file_stem()
        .map(|s| s.to_string_lossy().replace(['_', '.'], " "))
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Title".into())
}

/// Build the menu spec for `playlists`, taking names from the inputs or a
/// saved menu document.
fn menu_spec(
    req: &AuthorRequest,
    playlists: &[u32],
) -> Result<Option<crate::bdmv::MenuSpec>, CoreError> {
    if !req.menu {
        return Ok(None);
    }
    let jar = crate::bdjo::jar_path().ok_or_else(|| {
        CoreError::Disc(
            "menu JAR not found; run scripts/build-menu.sh or set KEEPSAKE_MENU_JAR".into(),
        )
    })?;
    if let Some(bg) = &req.menu_background {
        if !bg.is_file() {
            return Err(CoreError::Disc(format!(
                "menu background not found: {}",
                bg.display()
            )));
        }
    }
    let mut names: Vec<String> = req.inputs.iter().map(|p| title_name(p)).collect();
    if let Some(path) = &req.menu_template {
        let doc = crate::menu::load(&crate::menu::store_dir(), &path.to_string_lossy())?;
        for (i, name) in names.iter_mut().enumerate() {
            if let Some(n) = doc.titles.get(i).and_then(|t| t.name.as_ref()) {
                if !n.trim().is_empty() {
                    *name = n.trim().to_string();
                }
            }
        }
    }
    Ok(Some(crate::bdmv::MenuSpec {
        heading: "Keepsake Atelier".into(),
        label: req.label.clone().unwrap_or_else(|| "KEEPSAKE".into()),
        entries: names.into_iter().zip(playlists.iter().copied()).collect(),
        jar,
        background: req.menu_background.clone(),
    }))
}

/// Build a native UDF 2.50 image containing an already-staged disc tree.
fn iso_from_tree(tree: &Path, dest: &Path, label: &str) -> Result<(), CoreError> {
    if crate::udf::available() && !crate::udf_portable::forced() {
        let payload = crate::udf::tree_size(&tree.join("BDMV"));
        let size = crate::udf::image_size(payload);
        crate::udf::with_image(dest, size, label, |mount| {
            crate::bdmv::export_tree(tree, mount)
        })
    } else {
        // Linux and Windows have no newfs_udf. Package the staged tree with
        // the portable UDF 2.50 writer instead.
        crate::udf_portable::create_from_tree(tree, dest, label)
    }
}

/// Author `req.inputs` into one disc at `dest`: a single title, or one title per
/// input (merged by `bdmv::merge`).
fn author_group<F>(
    req: &AuthorRequest,
    bin: &Path,
    dest: &Path,
    on_progress: &mut F,
) -> Result<(), CoreError>
where
    F: FnMut(AuthorProgress),
{
    if req.inputs.len() == 1 {
        if req.menu {
            // tsMuxeR writes a bare title; stage it, add the menu, then export
            // the folder or build the UDF image ourselves.
            let menu = menu_spec(req, &[0])?.expect("menu requested");
            let stage = std::env::temp_dir().join(format!(
                "keepsake-menu-{}-{}",
                std::process::id(),
                dest.file_name()
                    .map(|n| n.to_string_lossy().replace(['/', ' '], "_"))
                    .unwrap_or_default()
            ));
            let _ = std::fs::remove_dir_all(&stage);
            let sub = AuthorRequest {
                inputs: req.inputs.clone(),
                dest: stage.clone(),
                label: req.label.clone(),
                chapters: req.chapters,
                auto_chapter_minutes: req.auto_chapter_minutes,
                split_gb: None,
                menu: false,
                menu_background: None,
                menu_template: None,
            };
            author_one(&sub, bin, &stage, on_progress)?;
            crate::bdmv::install_menu(&stage, &menu, 1)?;
            let result = if dest
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("iso"))
            {
                iso_from_tree(
                    &stage,
                    dest,
                    &req.label.clone().unwrap_or_else(|| "KEEPSAKE".into()),
                )
            } else {
                crate::bdmv::export_tree(&stage, dest)
            };
            let _ = std::fs::remove_dir_all(&stage);
            return result;
        }
        return author_one(req, bin, dest, on_progress);
    }
    let want_iso = dest
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("iso"));

    let n = req.inputs.len();
    let menu = menu_spec(req, &(0..n as u32).collect::<Vec<_>>())?;
    let mut parts = Vec::with_capacity(n);
    for (i, input) in req.inputs.iter().enumerate() {
        let temp =
            std::env::temp_dir().join(format!("keepsake-author-{}-{}", std::process::id(), i));
        let _ = std::fs::remove_dir_all(&temp);
        let sub = AuthorRequest {
            inputs: vec![input.clone()],
            dest: temp.clone(),
            label: req.label.clone(),
            chapters: req.chapters,
            auto_chapter_minutes: req.auto_chapter_minutes,
            split_gb: None,
            menu: false,
            menu_background: None,
            menu_template: None,
        };
        let base = 70.0 * i as f64 / n as f64;
        let span = 70.0 / n as f64;
        let mut progress = |p: AuthorProgress| {
            on_progress(AuthorProgress {
                percent: base + p.percent * span / 100.0,
                message: format!("title {} of {n}: {}", i + 1, p.message),
                done: false,
            });
        };
        author_one(&sub, bin, &temp, &mut progress)?;
        parts.push(temp);
    }

    if want_iso {
        // tsMuxeR writes only one playlist per image, so build the merged BDMV
        // into a tree first. macOS lays it into a native UDF image; elsewhere
        // the portable writer packages the finished tree.
        let label = req.label.clone().unwrap_or_else(|| "KEEPSAKE".into());
        if crate::udf::available() && !crate::udf_portable::forced() {
            let payload: u64 = parts
                .iter()
                .map(|p| crate::udf::tree_size(&p.join("BDMV")))
                .sum();
            let size = crate::udf::image_size(payload);
            on_progress(AuthorProgress {
                percent: 71.0,
                message: format!("creating UDF 2.50 image ({} MiB)", size / (1024 * 1024)),
                done: false,
            });
            crate::udf::with_image(dest, size, &label, |mount| {
                crate::bdmv::merge(&parts, mount, menu.as_ref(), |pct| {
                    on_progress(AuthorProgress {
                        percent: 74.0 + pct * 0.24,
                        message: "writing titles".into(),
                        done: false,
                    });
                })
            })?;
        } else {
            let stage = std::env::temp_dir().join(format!(
                "keepsake-udf-stage-{}-{}",
                std::process::id(),
                dest.file_name()
                    .map(|n| n.to_string_lossy().replace(['/', ' '], "_"))
                    .unwrap_or_default()
            ));
            let _ = std::fs::remove_dir_all(&stage);
            on_progress(AuthorProgress {
                percent: 71.0,
                message: "merging titles".into(),
                done: false,
            });
            crate::bdmv::merge(&parts, &stage, menu.as_ref(), |pct| {
                on_progress(AuthorProgress {
                    percent: 72.0 + pct * 0.22,
                    message: "merging titles".into(),
                    done: false,
                });
            })?;
            on_progress(AuthorProgress {
                percent: 95.0,
                message: "packaging UDF 2.50 image".into(),
                done: false,
            });
            let result = crate::udf_portable::create_from_tree(&stage, dest, &label);
            let _ = std::fs::remove_dir_all(&stage);
            result?;
        }
    } else {
        crate::bdmv::merge(&parts, dest, menu.as_ref(), |pct| {
            on_progress(AuthorProgress {
                percent: 70.0 + pct * 0.3,
                message: "merging titles".into(),
                done: false,
            });
        })?;
    }
    for p in &parts {
        let _ = std::fs::remove_dir_all(p);
    }
    Ok(())
}

/// Greedy grouping of whole inputs into discs of at most `limit` bytes.
fn group_sizes(sizes: &[u64], limit: u64) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut total: u64 = 0;
    for (i, size) in sizes.iter().enumerate() {
        if !cur.is_empty() && total + size > limit {
            groups.push(std::mem::take(&mut cur));
            total = 0;
        }
        cur.push(i);
        total += size;
    }
    if !cur.is_empty() {
        groups.push(cur);
    }
    groups
}

/// Split the inputs across `limit_gb` discs, one folder per disc.
fn author_split<F>(
    req: &AuthorRequest,
    bin: &Path,
    limit_gb: f64,
    on_progress: &mut F,
) -> Result<PathBuf, CoreError>
where
    F: FnMut(AuthorProgress),
{
    // Whole inputs stay together: an episode cannot span two discs without
    // re-encoding, so measure the files and pack them greedily.
    let sizes: Vec<u64> = req
        .inputs
        .iter()
        .map(|i| std::fs::metadata(i).map(|m| m.len()).unwrap_or(0))
        .collect();
    let limit = (limit_gb * 1_000_000_000.0) as u64;
    let groups = group_sizes(&sizes, limit);
    let n = groups.len().max(1);
    let want_iso = req
        .dest
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("iso"));
    let base_dir = if want_iso {
        req.dest.with_extension("")
    } else {
        req.dest.clone()
    };
    std::fs::create_dir_all(&base_dir)?;

    for (gi, group) in groups.iter().enumerate() {
        let inputs: Vec<PathBuf> = group.iter().map(|i| req.inputs[*i].clone()).collect();
        let dest = if want_iso {
            base_dir.join(format!("Disc{}.iso", gi + 1))
        } else {
            base_dir.join(format!("Disc{}", gi + 1))
        };
        let sub = AuthorRequest {
            inputs,
            dest: dest.clone(),
            label: req.label.clone(),
            chapters: req.chapters,
            auto_chapter_minutes: req.auto_chapter_minutes,
            split_gb: None,
            menu: req.menu,
            menu_background: req.menu_background.clone(),
            menu_template: req.menu_template.clone(),
        };
        let base = 100.0 * gi as f64 / n as f64;
        let span = 100.0 / n as f64;
        let mut progress = |p: AuthorProgress| {
            on_progress(AuthorProgress {
                percent: base + p.percent * span / 100.0,
                message: format!("disc {} of {n}: {}", gi + 1, p.message),
                done: false,
            });
        };
        author_group(&sub, bin, &dest, &mut progress)?;
    }
    on_progress(AuthorProgress {
        percent: 100.0,
        message: format!("authored {n} discs under {}", base_dir.display()),
        done: true,
    });
    Ok(base_dir)
}

/// Run tsMuxeR for one title into `dest`.
fn author_one<F>(
    req: &AuthorRequest,
    bin: &Path,
    dest: &Path,
    on_progress: &mut F,
) -> Result<(), CoreError>
where
    F: FnMut(AuthorProgress),
{
    let (meta_text, total) = meta(req)?;
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let stamp_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let meta_path = std::env::temp_dir().join(format!(
        "keepsake-author-{}-{stamp_ns}.meta",
        std::process::id()
    ));
    std::fs::write(&meta_path, &meta_text)?;

    on_progress(AuthorProgress {
        percent: 0.0,
        message: format!("authoring {} ({})", dest.display(), stamp(total)),
        done: false,
    });

    let mut child = Command::new(bin)
        .arg(&meta_path)
        .arg(dest)
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_whole_inputs_by_size() {
        assert_eq!(
            group_sizes(&[10, 10, 10, 5], 25),
            vec![vec![0, 1], vec![2, 3]]
        );
        // a single over-size input still gets its own disc
        assert_eq!(group_sizes(&[30, 5], 25), vec![vec![0], vec![1]]);
        assert!(group_sizes(&[], 25).is_empty());
    }
}
