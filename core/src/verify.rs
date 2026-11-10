//! BDMV verification.
//!
//! Reads the navigation files directly (no libbluray, no decryption) and checks
//! that every title's movie object plays a playlist that exists, and that every
//! clip a playlist references is present. Useful before burning an authored
//! disc, and for spotting a broken tree on a pressed one.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::menu::{self, NavType};
use crate::CoreError;

#[derive(Debug, Clone, Serialize, Default)]
pub struct VerifyTitle {
    pub index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playlist: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clips: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapters: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct VerifyReport {
    pub path: String,
    pub first_play: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_play_bdj: Option<String>,
    pub top_menu: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_menu_bdj: Option<String>,
    pub titles: Vec<VerifyTitle>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub ok: bool,
}

const INDIRECT: &str =
    "some titles have no immediate PlayPL (playlist chosen at runtime, or empty object)";

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

/// One HDMV movie object: its command count and the first playlist it plays.
#[derive(Debug, Clone, Serialize, Default)]
pub struct MovieObject {
    pub commands: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playlist: Option<u32>,
}

/// Parse `MovieObject.bdmv` into objects (see libbluray `hdmv/mobj_parse.c`).
pub fn parse_movie_objects(data: &[u8]) -> Option<Vec<MovieObject>> {
    if data.len() < 0x32 || &data[0..4] != b"MOBJ" {
        return None;
    }
    let count = be16(data, 0x30)? as usize;
    let mut pos = 0x32;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if pos + 4 > data.len() {
            return None;
        }
        let commands = be16(data, pos + 2)?;
        pos += 4;
        let mut playlist = None;
        for _ in 0..commands {
            if pos + 12 > data.len() {
                return None;
            }
            let c = &data[pos..pos + 12];
            pos += 12;
            let op_cnt = c[0] >> 5;
            let grp = (c[0] >> 3) & 0x03;
            let sub = c[0] & 0x07;
            let imm1 = (c[1] >> 7) & 1;
            let branch = c[1] & 0x0f;
            // BRANCH / PLAY / {PlayPL, PlayPL_PI, PlayPL_PM}
            if op_cnt > 0 && grp == 0 && sub == 2 && branch <= 2 && imm1 == 1 {
                if playlist.is_none() {
                    playlist = be32(c, 4);
                }
            }
        }
        out.push(MovieObject { commands, playlist });
    }
    Some(out)
}

#[derive(Debug, Clone, Default)]
pub struct Playlist {
    pub clips: Vec<String>,
    pub duration_secs: f64,
    pub chapters: u32,
}

/// Parse an `.mpls`: its play items (clips, in/out times) and mark count.
pub fn parse_mpls(data: &[u8]) -> Option<Playlist> {
    if data.len() < 0x2c || &data[0..4] != b"MPLS" {
        return None;
    }
    let list_pos = be32(data, 0x08)? as usize;
    let mark_pos = be32(data, 0x0c)? as usize;
    let mut pl = Playlist::default();

    if list_pos + 10 > data.len() {
        return None;
    }
    let count = be16(data, list_pos + 6)? as usize;
    let mut pos = list_pos + 10;
    let mut total_90k: u64 = 0;
    for _ in 0..count {
        let len = be16(data, pos)? as usize;
        if len < 18 || pos + 2 + len > data.len() {
            return None;
        }
        let clip = data.get(pos + 2..pos + 7)?;
        if clip.iter().all(|c| c.is_ascii_digit()) {
            pl.clips.push(String::from_utf8_lossy(clip).into_owned());
        }
        // length(2) + clip(5) + codec(4) + 3 bytes flags/stc + in(4) + out(4)
        let in_time = be32(data, pos + 2 + 5 + 4 + 3)? as u64;
        let out_time = be32(data, pos + 2 + 5 + 4 + 3 + 4)? as u64;
        total_90k += out_time.saturating_sub(in_time);
        pos += 2 + len;
    }
    if count > 0 {
        pl.duration_secs = total_90k as f64 / 45_000.0;
    }
    if mark_pos + 6 <= data.len() {
        pl.chapters = be16(data, mark_pos + 4).unwrap_or(0) as u32;
    }
    Some(pl)
}

fn clip_files(bdmv: &Path, clip: &str) -> (PathBuf, PathBuf) {
    (
        bdmv.join("CLIPINF").join(format!("{clip}.clpi")),
        bdmv.join("STREAM").join(format!("{clip}.m2ts")),
    )
}

/// Verify a BDMV folder (or a disc root containing `BDMV`).
pub fn run(source: &Path) -> Result<VerifyReport, CoreError> {
    let bdmv = menu::bdmv_dir(source).ok_or_else(|| {
        CoreError::Disc(format!(
            "no BDMV/index.bdmv under {} (mount the disc or pass a BDMV folder)",
            source.display()
        ))
    })?;

    let mut report = VerifyReport {
        path: bdmv.display().to_string(),
        ..Default::default()
    };

    let index = std::fs::read(bdmv.join("index.bdmv"))
        .map_err(|e| CoreError::Disc(format!("BDMV/index.bdmv: {e}")))?;
    let nav = menu::parse_index(&index)
        .ok_or_else(|| CoreError::Disc("index.bdmv does not parse".into()))?;

    let objects = std::fs::read(bdmv.join("MovieObject.bdmv"))
        .ok()
        .and_then(|d| parse_movie_objects(&d))
        .unwrap_or_default();
    if objects.is_empty() {
        report
            .errors
            .push("MovieObject.bdmv is missing or has no objects".into());
    }

    let check_object =
        |id: u16, report: &mut VerifyReport, what: &str, need_playlist: bool| -> Option<u32> {
            match objects.get(id as usize) {
                None => {
                    report.errors.push(format!(
                        "{what} references movie object {id}, which does not exist"
                    ));
                    None
                }
                Some(o) => {
                    if need_playlist && o.playlist.is_none() {
                        // Pressed discs often pick the playlist from a GPR, or use
                        // an empty object as a hidden-title placeholder. Only an
                        // authored disc that lost its PlayPL matters, and that is
                        // covered by the writer's tests.
                        report.warnings.push(INDIRECT.to_string());
                    }
                    o.playlist
                }
            }
        };

    // A BD-J menu is a real object: its BDJO and the application JAR must be
    // on disc, or the player has nothing to start.
    let check_bdj =
        |target: &menu::NavTarget, report: &mut VerifyReport, what: &str| match &target.name {
            Some(name) => {
                let path = bdmv.join("BDJO").join(format!("{name}.bdjo"));
                if !path.is_file() {
                    report
                        .errors
                        .push(format!("{what}: missing BDJO/{name}.bdjo"));
                }
            }
            None => report
                .errors
                .push(format!("{what}: BD-J entry has no object name")),
        };

    report.first_play = match nav.first_playback.kind {
        NavType::Hdmv => nav.first_playback.object,
        NavType::None => None,
        NavType::Bdj => {
            check_bdj(&nav.first_playback, &mut report, "first playback");
            report.first_play_bdj = nav.first_playback.name.clone();
            None
        }
        NavType::Unknown => {
            report
                .warnings
                .push("first playback is an unknown object type".into());
            None
        }
    };
    report.top_menu = match nav.top_menu.kind {
        NavType::Hdmv => nav.top_menu.object,
        NavType::None => None,
        NavType::Bdj => {
            check_bdj(&nav.top_menu, &mut report, "top menu");
            report.top_menu_bdj = nav.top_menu.name.clone();
            None
        }
        NavType::Unknown => {
            report
                .warnings
                .push("top menu is an unknown object type".into());
            None
        }
    };

    // If an authored menu is present, its rows must point at real playlists.
    let menu_txt = bdmv.join("JAR/menu.txt");
    if menu_txt.is_file() {
        let mut rows = 0;
        if let Ok(text) = std::fs::read_to_string(&menu_txt) {
            for (n, line) in text.lines().enumerate() {
                let Some((_, tail)) = line.trim().rsplit_once('|') else {
                    continue;
                };
                let Ok(pl) = tail.trim().parse::<u32>() else {
                    continue;
                };
                rows += 1;
                let path = bdmv.join("PLAYLIST").join(format!("{pl:05}.mpls"));
                if !path.is_file() {
                    report.errors.push(format!(
                        "menu.txt line {}: playlist {pl:05}.mpls is missing",
                        n + 1
                    ));
                }
            }
        }
        if !bdmv.join("JAR").is_dir()
            || std::fs::read_dir(bdmv.join("JAR"))
                .map(|d| {
                    d.flatten()
                        .all(|e| e.path().extension().is_none_or(|x| x != "jar"))
                })
                .unwrap_or(true)
        {
            report
                .errors
                .push("BDMV/JAR has no application JAR for the menu".into());
        }
        if rows == 0 {
            report.warnings.push("menu.txt has no playable rows".into());
        }
    }
    if let Some(id) = report.first_play {
        check_object(id, &mut report, "first playback", false);
    }
    if let Some(id) = report.top_menu {
        check_object(id, &mut report, "top menu", false);
    }

    for (i, entry) in nav.titles.iter().enumerate() {
        let mut title = VerifyTitle {
            index: i,
            object: entry.object,
            ..Default::default()
        };
        if entry.kind != NavType::Hdmv {
            report.warnings.push(format!(
                "title #{i} is a {} object; not checked",
                match entry.kind {
                    NavType::Bdj => "BD-J",
                    NavType::None => "empty",
                    _ => "unknown",
                }
            ));
            report.titles.push(title);
            continue;
        }
        let Some(object_id) = entry.object else {
            title.errors.push("title has no movie object id".into());
            report.titles.push(title);
            continue;
        };
        title.playlist = check_object(object_id, &mut report, &format!("title #{i}"), true);
        let Some(pl_id) = title.playlist else {
            report.titles.push(title);
            continue;
        };
        let path = bdmv.join("PLAYLIST").join(format!("{pl_id:05}.mpls"));
        match std::fs::read(&path) {
            Err(e) => title.errors.push(format!("playlist {pl_id:05}.mpls: {e}")),
            Ok(data) => match parse_mpls(&data) {
                None => title
                    .errors
                    .push(format!("playlist {pl_id:05}.mpls does not parse")),
                Some(pl) => {
                    title.clips = pl.clips.clone();
                    title.duration_secs = Some(pl.duration_secs);
                    title.chapters = Some(pl.chapters);
                    for clip in &pl.clips {
                        let (clpi, m2ts) = clip_files(&bdmv, clip);
                        if !clpi.is_file() {
                            title
                                .errors
                                .push(format!("clip {clip}: missing CLIPINF/{clip}.clpi"));
                        }
                        if !m2ts.is_file() {
                            title
                                .errors
                                .push(format!("clip {clip}: missing STREAM/{clip}.m2ts"));
                        }
                    }
                }
            },
        }
        report.titles.push(title);
    }

    for t in &report.titles {
        for e in &t.errors {
            report.errors.push(format!("title #{}: {e}", t.index));
        }
    }
    // Collapse the per-title "not statically resolvable" notes into one.
    let mut seen = std::collections::BTreeSet::new();
    report
        .warnings
        .retain(|w| *w != INDIRECT || seen.insert(w.clone()));
    report.ok = report.errors.is_empty();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_disc(dir: &Path, clip: &str, with_clip: bool, dur: u32) {
        let bdmv = dir.join("BDMV");
        for d in ["PLAYLIST", "CLIPINF", "STREAM"] {
            std::fs::create_dir_all(bdmv.join(d)).unwrap();
        }
        std::fs::write(
            bdmv.join("index.bdmv"),
            crate::bdmv::write_index(1, 2, &[0]),
        )
        .unwrap();
        std::fs::write(
            bdmv.join("MovieObject.bdmv"),
            crate::bdmv::write_movie_objects(1),
        )
        .unwrap();

        // One playlist, one play item, one mark.
        let mut m = vec![0u8; 0x2c];
        m[0..4].copy_from_slice(b"MPLS");
        m[4..8].copy_from_slice(b"0200");
        let list_pos = 0x2c;
        let mark_pos = list_pos + 4 + 6 + 30;
        m[0x08..0x0c].copy_from_slice(&(list_pos as u32).to_be_bytes());
        m[0x0c..0x10].copy_from_slice(&(mark_pos as u32).to_be_bytes());
        // playlist
        m.extend_from_slice(&30u32.to_be_bytes()); // length
        m.extend_from_slice(&0u16.to_be_bytes()); // reserved
        m.extend_from_slice(&1u16.to_be_bytes()); // play items
        m.extend_from_slice(&0u16.to_be_bytes()); // sub paths
        m.extend_from_slice(&28u16.to_be_bytes()); // play item length
        m.extend_from_slice(clip.as_bytes());
        m.extend_from_slice(b"M2TS");
        m.extend_from_slice(&[0, 0, 0]); // flags + stc id
        m.extend_from_slice(&0u32.to_be_bytes()); // in
        m.extend_from_slice(&dur.to_be_bytes()); // out
        m.extend_from_slice(&[0; 4]); // uo mask
        m.extend_from_slice(&[0, 0, 0, 0]); // random access + still mode + still time
                                            // marks
        m.extend_from_slice(&16u32.to_be_bytes()); // length
        m.extend_from_slice(&1u16.to_be_bytes()); // count
        m.extend_from_slice(&[0, 1]); // reserved + mark type
        m.extend_from_slice(&0u16.to_be_bytes()); // play item ref
        m.extend_from_slice(&0u32.to_be_bytes()); // time
        m.extend_from_slice(&0xffffu16.to_be_bytes()); // pid
        m.extend_from_slice(&0u32.to_be_bytes()); // duration
        std::fs::write(bdmv.join("PLAYLIST/00000.mpls"), &m).unwrap();

        if with_clip {
            std::fs::write(bdmv.join(format!("CLIPINF/{clip}.clpi")), b"x").unwrap();
            std::fs::write(bdmv.join(format!("STREAM/{clip}.m2ts")), b"x").unwrap();
        }
    }

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("keepsake-verify-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn accepts_a_consistent_tree() {
        let dir = temp("ok");
        build_disc(&dir, "00000", true, 135_000); // 3 s
        let r = run(&dir).unwrap();
        assert!(r.ok, "{:?}", r.errors);
        assert_eq!(r.first_play, Some(1));
        assert_eq!(r.top_menu, Some(2));
        assert_eq!(r.titles.len(), 1);
        assert_eq!(r.titles[0].playlist, Some(0));
        assert_eq!(r.titles[0].clips, vec!["00000".to_string()]);
        assert!((r.titles[0].duration_secs.unwrap() - 3.0).abs() < 0.01);
        assert_eq!(r.titles[0].chapters, Some(1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reports_a_missing_clip() {
        let dir = temp("missing");
        build_disc(&dir, "00000", false, 45_000);
        let r = run(&dir).unwrap();
        assert!(!r.ok);
        assert!(r.errors.iter().any(|e| e.contains("CLIPINF/00000.clpi")));
        assert!(r.errors.iter().any(|e| e.contains("STREAM/00000.m2ts")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_movie_objects_and_playlists() {
        let objs = parse_movie_objects(&crate::bdmv::write_movie_objects(2)).unwrap();
        assert_eq!(objs.len(), 4);
        assert_eq!(objs[0].playlist, Some(0));
        assert_eq!(objs[1].playlist, Some(1));
        assert_eq!(objs[2].playlist, None); // JumpTitle first play
    }
}
