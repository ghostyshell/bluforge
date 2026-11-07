//! Output path templating.
//!
//! A template is rendered against a set of variables built from the disc and
//! from TMDB, then sanitised. `{title} ({year})` style; `/` in the template
//! creates sub-folders. Unknown placeholders are kept verbatim so a typo is
//! visible rather than silently dropped.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// Placeholders offered in the UI: `(name, description)`.
pub const PLACEHOLDERS: &[(&str, &str)] = &[
    ("title", "movie title (TMDB when matched, else the disc label)"),
    ("year", "release year (TMDB)"),
    ("tmdb", "TMDB id"),
    ("disc", "disc label"),
    ("resolution", "e.g. 2160p"),
    ("hdr", "HDR10 / Dolby Vision, empty when SDR"),
    ("video_codec", "HEVC / H.264"),
    ("audio_lang", "language of the default audio track"),
    ("audio_codec", "codec of the default audio track"),
    ("runtime", "main title runtime, h:mm"),
    ("chapters", "chapter count"),
    ("playlist", "playlist number"),
    ("index", "title index"),
    ("ext", "container extension"),
];

/// Values available to a template.
#[derive(Debug, Default, Clone)]
pub struct Vars(BTreeMap<String, String>);

impl Vars {
    pub fn set(&mut self, k: &str, v: impl Into<String>) {
        let v = v.into();
        if !v.trim().is_empty() {
            self.0.insert(k.to_string(), v);
        }
    }

    pub fn get(&self, k: &str) -> Option<&str> {
        self.0.get(k).map(|s| s.as_str())
    }

    /// Variables for a TV episode.
    pub fn tv(series: &str, season: u32, episode: u32, title: &str, disc: Option<u32>) -> Vars {
        let mut v = Vars::default();
        v.set("series", series);
        v.set("season", season.to_string());
        v.set("episode", episode.to_string());
        v.set("episode_title", title);
        v.set("episode_title_clean", title);
        if let Some(d) = disc {
            v.set("disc", d.to_string());
        }
        v
    }

    /// Build the standard variable set.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        disc: &str,
        title: &str,
        year: Option<&str>,
        tmdb_id: Option<u64>,
        resolution: Option<&str>,
        hdr: Option<&str>,
        video_codec: Option<&str>,
        audio_lang: Option<&str>,
        audio_codec: Option<&str>,
        runtime_secs: f64,
        chapters: usize,
        playlist: u32,
        index: u32,
        ext: &str,
    ) -> Vars {
        let mut v = Vars::default();
        v.set("title", title);
        v.set("disc", disc);
        if let Some(y) = year {
            v.set("year", y);
        }
        if let Some(id) = tmdb_id {
            v.set("tmdb", id.to_string());
        }
        if let Some(r) = resolution {
            v.set("resolution", r);
        }
        if let Some(h) = hdr {
            v.set("hdr", h);
        }
        if let Some(c) = video_codec {
            v.set("video_codec", c);
        }
        if let Some(a) = audio_lang {
            v.set("audio_lang", a);
        }
        if let Some(a) = audio_codec {
            v.set("audio_codec", a);
        }
        if runtime_secs > 0.0 {
            let secs = runtime_secs.round() as u64;
            v.set(
                "runtime",
                format!("{}h{:02}m", secs / 3600, (secs % 3600) / 60),
            );
        }
        if chapters > 0 {
            v.set("chapters", chapters.to_string());
        }
        v.set("playlist", format!("{playlist:05}"));
        v.set("index", index.to_string());
        v.set("ext", ext);
        v
    }
}

fn strip_bad(s: &str) -> String {
    s.chars()
        .map(|c| if "/\\:*?\"<>|".contains(c) { '-' } else { c })
        .collect()
}

/// `{episode:02}` -> padded to width 2 with zeros (space-padded otherwise).
fn pad(val: &str, flags: &str) -> String {
    let width: usize = flags.trim_start_matches('0').parse().unwrap_or(0);
    if width == 0 || val.len() >= width {
        return val.to_string();
    }
    let fill = if flags.starts_with('0') && val.chars().all(|c| c.is_ascii_digit()) {
        '0'
    } else {
        ' '
    };
    format!("{}{}", fill.to_string().repeat(width - val.len()), val)
}

/// Collapse artefacts left by empty variables (`Title ()`, ` - `, double spaces).
fn tidy(seg: &str) -> String {
    let mut s = strip_bad(seg);
    loop {
        let before = s.clone();
        s = s.replace("()", "");
        s = s.replace("[]", "");
        s = s.replace("{}", "");
        s = s.replace(" -  - ", " - ");
        s = s.replace("--", "-");
        while s.contains("  ") {
            s = s.replace("  ", " ");
        }
        s = s.replace(" -/", "/").replace("/- ", "/");
        s = s.trim_matches(|c: char| c == ' ' || c == '-' || c == '.' || c == '_').to_string();
        if s == before {
            break;
        }
    }
    s
}

/// Render `template` against `vars`, appending `.ext` if the last segment has
/// no extension. Empty template falls back to `fallback`.
pub fn render(template: &str, vars: &Vars, ext: &str, fallback: &str) -> PathBuf {
    let tpl = if template.trim().is_empty() { fallback } else { template };

    // Substitute {name} with values.
    let mut out = String::with_capacity(tpl.len() + 16);
    let mut rest = tpl;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let raw = &after[..close];
                let (key, flags) = match raw.split_once(':') {
                    Some((k, f)) => (k, Some(f)),
                    None => (raw, None),
                };
                match vars.get(key).map(|v| match flags {
                    Some(f) => pad(v, f),
                    None => v.to_string(),
                }) {
                    Some(v) => out.push_str(&v),
                    None => {
                        out.push('{');
                        out.push_str(raw);
                        out.push('}');
                    }
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push_str(&rest[open..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);

    // Sanitise per path segment and drop empty ones.
    let mut parts: Vec<String> = out
        .split('/')
        .map(tidy)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        parts.push(tidy(fallback));
    }
    if let Some(last) = parts.last_mut() {
        let has_ext = last
            .rsplit_once('.')
            .map(|(_, e)| !e.is_empty() && e.len() <= 5 && !e.contains(' '))
            .unwrap_or(false);
        if !has_ext {
            last.push('.');
            last.push_str(ext);
        }
    }
    PathBuf::from(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> Vars {
        let mut v = Vars::default();
        v.set("title", "Gladiator II");
        v.set("year", "2024");
        v.set("resolution", "2160p");
        v.set("hdr", "HDR10");
        v
    }

    #[test]
    fn renders_and_extends() {
        let p = render("{title} ({year})/{title} ({year}) - {resolution} {hdr}", &vars(), "mkv", "");
        assert_eq!(
            p,
            PathBuf::from("Gladiator II (2024)/Gladiator II (2024) - 2160p HDR10.mkv")
        );
    }

    #[test]
    fn drops_empty_vars_and_keeps_unknown() {
        let mut v = vars();
        v.0.remove("hdr");
        let p = render("{title} ({year}) - {resolution}", &v, "mkv", "");
        assert_eq!(p, PathBuf::from("Gladiator II (2024) - 2160p.mkv"));
        let p = render("{title} - {bogus}", &v, "mkv", "");
        assert_eq!(p, PathBuf::from("Gladiator II - {bogus}.mkv"));
    }

    #[test]
    fn falls_back_when_empty() {
        let p = render("", &vars(), "mkv", "{title}");
        assert_eq!(p, PathBuf::from("Gladiator II.mkv"));
    }
}
