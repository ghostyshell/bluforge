//! TV / anime episode planning.
//!
//! TV discs are authored one of two ways: several episode-sized titles, or one
//! "play all" title whose *clips* are the episodes (One Punch Man S1 D1 is the
//! latter: 8 clips of 24:21). Either way each clip is one episode, so the clip
//! boundaries are the split points.
//!
//! Episode titles come from TMDB (scraped, no key), falling back to MAL via
//! Jikan when it is reachable.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::disc::Clip;
use crate::CoreError;

/// TMDB's edge 403s *browser* User-Agents coming from this client; a plain
/// token works (same trick as core::metadata). MAL/Jikan want a browser UA.
const TMDB_UA: &str = "BluForge/0.1";
const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/157 Safari/537.36";

/// Episodes shorter than this are menus / logos, not episodes.
const MIN_EPISODE_SECS: f64 = 300.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    pub season: u32,
    pub number: u32,
    pub title: String,
    /// Episode still from TMDB, when it could be fetched.
    pub still: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Series {
    pub name: String,
    pub year: Option<String>,
    pub tmdb_id: Option<u64>,
    pub mal_id: Option<u64>,
    /// Series poster (used for a per-episode thumb when there is no still).
    pub poster: Option<String>,
    pub episodes: Vec<Episode>,
}

impl Series {
    pub fn episode(&self, number: u32) -> Option<&Episode> {
        self.episodes.iter().find(|e| e.number == number)
    }
}

/// Split "ONE_PUNCH_MAN_S1_D1" into ("One Punch Man", season, disc).
///
/// Disc labels are usually `SERIES_S<season>_D<disc>` or a plain title; the
/// season/disc tokens are recognised and removed, and the rest becomes the
/// search name.
pub fn parse_label(label: &str) -> (String, Option<u32>, Option<u32>) {
    let mut words: Vec<String> = Vec::new();
    let (mut season, mut disc) = (None, None);
    let mut want_season = false;
    let mut want_disc = false;
    for tok in label.split(['_', '-', '.', ' ']).filter(|t| !t.is_empty()) {
        let up = tok.to_ascii_uppercase();
        let num = |s: &str| -> Option<u32> {
            let d: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
            if d.is_empty() {
                None
            } else {
                d.parse().ok()
            }
        };
        // "SEASON 3" / "DISC 2": the word and the number are separate tokens.
        if up == "SEASON" || up == "VOL" || up == "VOLUME" || up == "PT" {
            want_season = true;
            continue;
        }
        if up == "DISC" || up == "DISK" {
            want_disc = true;
            continue;
        }
        if want_season {
            if let Some(n) = num(&up) {
                season = Some(n);
                want_season = false;
                continue;
            }
            want_season = false;
        }
        if want_disc {
            if let Some(n) = num(&up) {
                disc = Some(n);
                want_disc = false;
                continue;
            }
            want_disc = false;
        }
        // Compact "S1" / "D1" (and "S01D02" style is split on the letters here).
        if up.starts_with('S') && num(&up).is_some() && up.len() <= 4 && season.is_none() {
            season = num(&up);
            continue;
        }
        if up.starts_with('D') && num(&up).is_some() && up.len() <= 4 && disc.is_none() {
            disc = num(&up);
            continue;
        }
        words.push(tok.to_string());
    }
    let name = words
        .iter()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    (name, season, disc)
}

fn slugify(s: &str) -> String {
    s.to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct DiscRecord {
    first: u32,
    count: u32,
}

fn memory_path() -> std::path::PathBuf {
    crate::config_dir().join("tv_discs.json")
}

fn load_memory() -> std::collections::HashMap<String, DiscRecord> {
    std::fs::read(memory_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Where does this disc start? Remembered mapping first, then the previous disc.
fn disc_memory_start(slug: &str, season: u32, disc: Option<u32>) -> Option<u32> {
    let mem = load_memory();
    let prefix = format!("{slug}|s{season}|");
    if let Some(d) = disc {
        if let Some(r) = mem.get(&format!("{prefix}{d}")) {
            return Some(r.first);
        }
    }
    // Anything earlier in the same season tells us where this disc should start.
    let mut start: Option<u32> = None;
    for (k, r) in &mem {
        let Some(rest) = k.strip_prefix(&prefix) else { continue };
        let Ok(other) = rest.parse::<u32>() else { continue };
        if disc.map(|d| other < d).unwrap_or(true) {
            let next = r.first + r.count;
            start = Some(start.map(|s| s.max(next)).unwrap_or(next));
        }
    }
    start
}

/// Record that this disc holds episodes `first..first+count` so the next disc
/// numbers itself correctly.
pub fn remember_disc(series: &str, season: u32, disc: Option<u32>, first: u32, count: u32) {
    let Some(d) = disc else { return };
    let mut mem = load_memory();
    mem.insert(
        format!("{}|s{}|{}", slugify(series), season, d),
        DiscRecord { first, count },
    );
    if let Some(dir) = memory_path().parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_vec_pretty(&mem) {
        let _ = std::fs::write(memory_path(), json);
    }
}

/// Keep only plausibly-episode-sized clips, in order. A trailing 0.5s fragment
/// (common at the end of a play-all title) is dropped, as are menus.
pub fn episode_clips(clips: &[Clip]) -> Vec<Clip> {
    let mut durs: Vec<f64> = clips.iter().map(|c| c.duration_secs).collect();
    if durs.is_empty() {
        return Vec::new();
    }
    durs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = durs[durs.len() / 2];
    clips
        .iter()
        .filter(|c| {
            c.duration_secs >= MIN_EPISODE_SECS && (median <= 0.0 || c.duration_secs >= median * 0.5)
        })
        .cloned()
        .collect()
}

fn get(url: &str) -> Result<String, CoreError> {
    let ua = if url.contains("themoviedb.org") { TMDB_UA } else { UA };
    let resp = ureq::get(url)
        .set("User-Agent", ua)
        .set("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .set("Accept-Language", "en-US,en;q=0.9")
        .timeout(Duration::from_secs(30))
        .call()
        .map_err(|e| CoreError::Disc(format!("GET {url}: {e}")))?;
    let mut body = String::new();
    use std::io::Read;
    resp.into_reader()
        .take(16 * 1024 * 1024)
        .read_to_string(&mut body)
        .map_err(|e| CoreError::Disc(e.to_string()))?;
    Ok(body)
}

fn unescape(s: &str) -> String {
    s.replace("&#0?39;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

/// Find the TMDB id for a TV show.
pub fn tmdb_tv_id(query: &str) -> Result<Option<u64>, CoreError> {
    let q = query.replace(' ', "+");
    let html = get(&format!("https://www.themoviedb.org/search/tv?query={q}"))?;
    let mut rest = html.as_str();
    while let Some(p) = rest.find("/tv/") {
        let after = &rest[p + 4..];
        let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            return Ok(digits.parse().ok());
        }
        rest = after;
    }
    Ok(None)
}

/// Episodes of one season from the TMDB season page.
pub fn tmdb_season(tv_id: u64, season: u32) -> Result<Vec<Episode>, CoreError> {
    let html = get(&format!(
        "https://www.themoviedb.org/tv/{tv_id}/season/{season}"
    ))?;
    let mut out = Vec::new();
    for part in html.split("data-episode-number=\"").skip(1) {
        let num: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
        let Ok(number) = num.parse::<u32>() else {
            continue;
        };
        // title attribute: "Show: Season 1 (2015): Episode 3 - The Obsessive Scientist"
        let title = match (part.find("title=\""), part.find("\">")) {
            (Some(t), _) => {
                let v = &part[t + 7..];
                let end = v.find('"').unwrap_or(0);
                let t = &v[..end];
                match t.rsplit_once(" - ") {
                    Some((_, ep_title)) => ep_title.trim().to_string(),
                    None => t.to_string(),
                }
            }
            _ => String::new(),
        };
        if number > 0 && !out.iter().any(|e: &Episode| e.number == number) {
            out.push(Episode {
                season,
                number,
                title: unescape(&title),
                still: None,
            });
        }
    }
    out.sort_by_key(|e| e.number);
    Ok(out)
}

/// The episode still from the episode page, best effort.
pub fn tmdb_episode_still(tv_id: u64, season: u32, episode: u32) -> Option<String> {
    let html = get(&format!(
        "https://www.themoviedb.org/tv/{tv_id}/season/{season}/episode/{episode}"
    ))
    .ok()?;
    // Prefer a still (w300/w500); skip the poster-sized w220.
    let mut best = None;
    let mut rest = html.as_str();
    while let Some(p) = rest.find("https://image.tmdb.org/t/p/") {
        let v = &rest[p..];
        let end = v.find(".jpg").map(|e| e + 4)?;
        let url = &v[..end];
        if url.contains("/w780/") || url.contains("/w500/") {
            best = Some(url.to_string());
            break;
        }
        if best.is_none() && !url.contains("/w220") && !url.contains("/w45") {
            best = Some(url.to_string());
        }
        rest = &v[end..];
    }
    best
}

/// The series poster (w500), best effort.
pub fn tmdb_poster(tv_id: u64, season: u32) -> Option<String> {
    let html = get(&format!(
        "https://www.themoviedb.org/tv/{tv_id}/season/{season}"
    ))
    .ok()?;
    let rest = html.as_str();
    let p = rest.find("https://image.tmdb.org/t/p/w500")?;
    let v = &rest[p..];
    let end = v.find(".jpg").map(|e| e + 4)?;
    Some(v[..end].to_string())
}

/// MAL fallback (Jikan) - used when TMDB has nothing or is unreachable.
fn jikan_series(query: &str, season: u32) -> Result<Option<Series>, CoreError> {
    let q = query.replace(' ', "%20");
    let body = get(&format!(
        "https://api.jikan.moe/v4/anime?q={q}&type=tv&limit=5"
    ))?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| CoreError::Disc(e.to_string()))?;
    let Some(first) = v.get("data").and_then(|d| d.as_array()).and_then(|a| a.first()) else {
        return Ok(None);
    };
    let mal_id = first.get("mal_id").and_then(|m| m.as_u64());
    let name = first
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or(query)
        .to_string();
    let poster = first
        .get("images")
        .and_then(|i| i.get("jpg"))
        .and_then(|j| j.get("large_image_url"))
        .and_then(|u| u.as_str())
        .map(|s| s.to_string());
    let mut episodes = Vec::new();
    if let Some(id) = mal_id {
        if let Ok(b) = get(&format!("https://api.jikan.moe/v4/anime/{id}/episodes")) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&b) {
                if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
                    for (i, e) in arr.iter().enumerate() {
                        episodes.push(Episode {
                            season,
                            number: (i + 1) as u32,
                            title: e
                                .get("title")
                                .and_then(|t| t.as_str())
                                .unwrap_or("")
                                .to_string(),
                            still: None,
                        });
                    }
                }
            }
        }
    }
    Ok(Some(Series {
        name,
        year: first.get("year").and_then(|y| y.as_u64()).map(|y| y.to_string()),
        tmdb_id: None,
        mal_id,
        poster,
        episodes,
    }))
}

/// Look a series up: TMDB first, MAL (Jikan) as fallback.
pub fn lookup_series(query: &str, season: u32) -> Result<Option<Series>, CoreError> {
    if let Ok(Some(id)) = tmdb_tv_id(query) {
        let mut episodes = tmdb_season(id, season).unwrap_or_default();
        let name = tmdb_series_name(id).unwrap_or_else(|| query.to_string());
        let poster = tmdb_poster(id, season);
        if !episodes.is_empty() || poster.is_some() {
            // Stills are per-episode pages; fetch only for the ones present.
            for e in episodes.iter_mut() {
                e.still = tmdb_episode_still(id, season, e.number);
            }
            return Ok(Some(Series {
                name,
                year: None,
                tmdb_id: Some(id),
                mal_id: None,
                poster,
                episodes,
            }));
        }
    }
    jikan_series(query, season)
}

fn tmdb_series_name(tv_id: u64) -> Option<String> {
    let html = get(&format!("https://www.themoviedb.org/tv/{tv_id}")).ok()?;
    let p = html.find("<title>")?;
    let v = &html[p + 7..];
    let end = v.find('<').unwrap_or(0);
    let t = v[..end].split('(').next().unwrap_or("").trim().to_string();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

/// One episode to rip: which title, where in it, and its identity.
#[derive(Debug, Clone, Serialize)]
pub struct PlanItem {
    pub title_index: u32,
    /// In/out within `title_index` (seconds).
    pub start_secs: f64,
    pub duration_secs: f64,
    pub episode: Episode,
}

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub series: String,
    pub season: u32,
    pub disc: Option<u32>,
    pub tmdb_id: Option<u64>,
    pub mal_id: Option<u64>,
    pub poster: Option<String>,
    pub items: Vec<PlanItem>,
}

/// Work out what episodes a disc holds.
///
/// Handles both authorings: one "play all" title whose clips are the episodes,
/// or several episode-sized titles. `series` overrides the label-derived name.
pub fn plan(
    disc: &crate::Disc,
    info: &crate::DiscInfo,
    series: Option<&str>,
    start_override: Option<u32>,
) -> Result<Plan, CoreError> {
    let (label_name, season_from_label, disc_no) = parse_label(&info.name);
    let season = season_from_label.unwrap_or(1);
    let name = series
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or(label_name);

    // Candidate episode lists, best (most episodes) first.
    let mut best: Vec<PlanItem> = Vec::new();
    let mut candidates: Vec<(u32, Vec<Clip>)> = Vec::new();
    for t in &info.titles {
        let clips = episode_clips(&disc.clips(t.index).unwrap_or_default());
        if clips.len() >= 2 {
            candidates.push((t.index, clips));
        }
    }
    candidates.sort_by_key(|(_, c)| std::cmp::Reverse(c.len()));
    if let Some((idx, clips)) = candidates.first() {
        let mut acc = 0.0;
        for c in clips {
            best.push(PlanItem {
                title_index: *idx,
                start_secs: acc,
                duration_secs: c.duration_secs,
                episode: Episode {
                    season,
                    number: 0,
                    title: String::new(),
                    still: None,
                },
            });
            acc += c.duration_secs;
        }
    } else {
        // Multi-title authoring: each episode-sized title is one episode.
        let mut ts: Vec<&crate::disc::Title> = info
            .titles
            .iter()
            .filter(|t| t.duration_secs >= MIN_EPISODE_SECS && t.audio_streams > 0)
            .collect();
        ts.sort_by_key(|t| t.playlist);
        for t in ts {
            best.push(PlanItem {
                title_index: t.index,
                start_secs: 0.0,
                duration_secs: t.duration_secs,
                episode: Episode {
                    season,
                    number: 0,
                    title: String::new(),
                    still: None,
                },
            });
        }
    }
    if best.is_empty() {
        return Err(CoreError::Disc("no episode-sized titles or clips found".into()));
    }

    // Number them. Discs are not always evenly split (this show is 8 + 4), so a
    // remembered mapping wins over arithmetic: an explicit override, then what we
    // recorded for this disc, then the end of the previous disc, then 1.
    let slug = slugify(&name);
    let start = start_override
        .or_else(|| disc_memory_start(&slug, season, disc_no))
        .unwrap_or(1);
    for (i, item) in best.iter_mut().enumerate() {
        item.episode.number = start + i as u32;
    }

    let found = lookup_series(&name, season).unwrap_or(None);
    if let Some(s) = &found {
        for item in best.iter_mut() {
            if let Some(e) = s.episode(item.episode.number) {
                item.episode.title = e.title.clone();
                item.episode.still = e.still.clone();
            }
        }
    }
    for item in best.iter_mut() {
        if item.episode.title.is_empty() {
            item.episode.title = format!("Episode {}", item.episode.number);
        }
    }

    Ok(Plan {
        series: found.as_ref().map(|s| s.name.clone()).unwrap_or(name),
        season,
        disc: disc_no,
        tmdb_id: found.as_ref().and_then(|s| s.tmdb_id),
        mal_id: found.as_ref().and_then(|s| s.mal_id),
        poster: found.as_ref().and_then(|s| s.poster.clone()),
        items: best,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_disc_label() {
        assert_eq!(
            parse_label("ONE_PUNCH_MAN_S1_D1"),
            ("One Punch Man".into(), Some(1), Some(1))
        );
        let (title, season, disc) = parse_label("STAR_TREK_TNG_SEASON_3_DISC_2");
        assert_eq!(title, "Star Trek Tng");
        assert_eq!((season, disc), (Some(3), Some(2)));
        assert_eq!(parse_label("BAND_OF_BROTHERS").1, None);
    }

    #[test]
    fn keeps_episode_sized_clips_only() {
        let mk = |d: f64| Clip {
            in_secs: 0.0,
            out_secs: d,
            duration_secs: d,
        };
        let clips = vec![
            mk(1461.0),
            mk(1461.0),
            mk(1461.0),
            mk(0.5),
            mk(30.0),
        ];
        let eps = episode_clips(&clips);
        assert_eq!(eps.len(), 3);
        assert!(eps.iter().all(|c| c.duration_secs > 300.0));
    }
}
