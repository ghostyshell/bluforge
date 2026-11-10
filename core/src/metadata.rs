//! External metadata lookups used to enrich a remux:
//!
//! * chapter **names** from ChapterDB.org (chapterdb.plex.tv),
//! * a **poster** from TMDB (themoviedb.org).
//!
//! Both are used keylessly by scraping the public sites, so there is nothing to
//! configure. Failures are never fatal - a remux is still produced without the
//! extra metadata.

use std::time::Duration;

use crate::CoreError;

// Note: TMDB's edge returns 403 for *browser* User-Agents coming from a
// non-browser client; a plain product UA works.
const UA: &str = "KeepsakeAtelier/0.1";
const MAX_BODY: u64 = 8 * 1024 * 1024;

fn get(url: &str) -> Result<String, CoreError> {
    let resp = ureq::get(url)
        .set("User-Agent", UA)
        .timeout(Duration::from_secs(30))
        .call()
        .map_err(|e| CoreError::Disc(format!("GET {url}: {e}")))?;
    let mut body = String::new();
    use std::io::Read;
    resp.into_reader()
        .take(MAX_BODY)
        .read_to_string(&mut body)
        .map_err(|e| CoreError::Disc(format!("read {url}: {e}")))?;
    Ok(body)
}

/// Download a URL to bytes.
pub fn download(url: &str) -> Result<Vec<u8>, CoreError> {
    let resp = ureq::get(url)
        .set("User-Agent", UA)
        .timeout(Duration::from_secs(60))
        .call()
        .map_err(|e| CoreError::Disc(format!("GET {url}: {e}")))?;
    let mut body = Vec::new();
    use std::io::Read;
    resp.into_reader()
        .take(32 * 1024 * 1024)
        .read_to_end(&mut body)
        .map_err(|e| CoreError::Disc(format!("read {url}: {e}")))?;
    Ok(body)
}

/// Lowercase, drop everything that is not a letter or digit.
fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut depth = 0usize;
    for c in html.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

/// `hh:mm.ss` (ChapterDB's odd duration format) -> seconds.
fn parse_cdb_duration(s: &str) -> Option<f64> {
    let mut parts = s.split(':');
    let h: f64 = parts.next()?.trim().parse().ok()?;
    let rest = parts.next()?.trim();
    let mut sub = rest.split('.');
    let m: f64 = sub.next()?.parse().ok()?;
    let sec: f64 = sub.next().unwrap_or("0").parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + sec)
}

#[derive(Debug)]
struct CdbHit {
    id: u64,
    title: String,
    chapters: usize,
    kind: String,
    runtime: Option<f64>,
    named: bool,
}

impl CdbHit {
    fn score(&self, title: &str, runtime: f64, chapters: usize) -> i32 {
        let mut s = 0;
        if self.chapters == chapters && chapters > 0 {
            s += 120;
        }
        if self.kind.eq_ignore_ascii_case("blu-ray") {
            s += 60;
        } else if self.kind.is_empty() || self.kind.eq_ignore_ascii_case("unknown") {
            s += 0;
        } else {
            s -= 20;
        }
        if let Some(rt) = self.runtime {
            let d = (rt - runtime).abs();
            s += if d < 30.0 {
                80
            } else if d < 90.0 {
                40
            } else if d < 300.0 {
                10
            } else {
                -40
            };
        }
        if self.named {
            s += 25;
        }
        let a = norm(title);
        let b = norm(&self.title);
        if a == b {
            s += 60;
        } else if !b.is_empty() && (a.starts_with(&b) || b.starts_with(&a)) {
            s += 30;
        }
        s
    }
}

fn search_chapterdb(title: &str) -> Result<Vec<CdbHit>, CoreError> {
    let q: String = title
        .chars()
        .map(|c| if c == ' ' { '+' } else { c })
        .collect();
    let html = get(&format!(
        "https://chapterdb.plex.tv/grid?Criteria.Title={q}"
    ))?;

    let mut hits = Vec::new();
    let mut rest = html.as_str();
    while let Some(pos) = rest.find("href=\"/browse/") {
        let after = &rest[pos + "href=\"/browse/".len()..];
        let id_end = after.find('"').unwrap_or(0);
        let id: u64 = match after[..id_end].parse() {
            Ok(v) => v,
            Err(_) => {
                rest = &after[id_end..];
                continue;
            }
        };
        let row_start = rest[..pos].rfind("<tr").unwrap_or(0);
        let row_end = rest[pos..]
            .find("</tr>")
            .map(|p| pos + p)
            .unwrap_or(rest.len());
        let row = &rest[row_start..row_end.max(pos)];

        // Title is the link text of the /browse/<id> anchor.
        let link_tail = &row[row.find("/browse/").unwrap_or(0)..];
        let anchor_text = link_tail
            .find('>')
            .map(|p| {
                let t = &link_tail[p + 1..];
                let end = t.find('<').unwrap_or(t.len());
                strip_tags(&t[..end]).trim().to_string()
            })
            .unwrap_or_default();

        let text = strip_tags(row);
        let mut chapters = 0usize;
        let mut runtime = None;
        let mut kind = String::new();
        for cell in text.split(['|', '\n']) {
            let c = cell.trim();
            if c.is_empty() {
                continue;
            }
            if let Ok(n) = c.parse::<usize>() {
                if chapters == 0 {
                    chapters = n;
                }
                continue;
            }
            if let Some(rt) = parse_cdb_duration(c) {
                runtime = Some(rt);
                continue;
            }
            if ["blu-ray", "dvd", "hd-dvd", "unknown"]
                .iter()
                .any(|k| c.eq_ignore_ascii_case(k))
            {
                kind = c.to_string();
            }
        }

        hits.push(CdbHit {
            id,
            title: anchor_text,
            chapters,
            kind,
            runtime,
            named: row.contains("Names detected"),
        });
        rest = &rest[row_end.min(rest.len())..];
    }
    Ok(hits)
}

/// Fetch chapter names for a movie from ChapterDB.
///
/// `chapters` is the disc's own chapter-mark count and `runtime` its main-title
/// duration; both are used to pick the right entry out of the search results.
/// When no entry matches exactly the best-scoring nearby entry is used, since
/// the disc's own chapter times are kept either way. Returns `None` when
/// nothing plausible is found.
pub fn chapter_names(
    movie: &str,
    runtime_secs: f64,
    chapters: usize,
) -> Result<Option<Vec<String>>, CoreError> {
    let mut hits = search_chapterdb(movie)?;

    // Sequels and subtitled titles are often absent even though an entry for the
    // base name exists; retry with trailing words dropped.
    if hits.iter().all(|h| !h.named) {
        let toks: Vec<&str> = movie.split_whitespace().collect();
        if toks.len() > 1 {
            for n in (1..toks.len()).rev() {
                let q = toks[..n].join(" ");
                let more = search_chapterdb(&q)?;
                if more.iter().any(|h| h.named) {
                    tracing::info!(query = %q, "chapterdb: retried without trailing words");
                    hits.extend(more);
                    break;
                }
            }
        }
    }

    let named = hits.iter().filter(|h| h.named).max_by_key(|h| {
        let exact = h.chapters == chapters && chapters > 0;
        let mut s = h.score(movie, runtime_secs, chapters);
        if exact {
            s += 100;
        } else if let Some(rt) = h.runtime {
            // A different cut only aligns if the runtime is close.
            if (rt - runtime_secs).abs() > 300.0 {
                s -= 80;
            }
        } else {
            s -= 40;
        }
        s
    });

    let Some(best) = named else {
        tracing::info!(%movie, "chapterdb: no entry with names");
        return Ok(None);
    };
    if best.chapters != chapters {
        tracing::info!(
            hit = best.id,
            entry = %best.title,
            entry_chapters = best.chapters,
            disc_chapters = chapters,
            "chapterdb: entry is a different cut - names only, disc times kept"
        );
    }

    let xml = get(&format!("https://chapterdb.plex.tv/browse/{}.xml", best.id))?;
    let mut names = Vec::new();
    let mut rest = xml.as_str();
    while let Some(pos) = rest.find("<chapter ") {
        let tag_end = rest[pos..].find('>').map(|p| pos + p).unwrap_or(rest.len());
        let tag = &rest[pos..tag_end];
        if let Some(p) = tag.find("name=\"") {
            let v = &tag[p + 6..];
            let end = v.find('"').unwrap_or(v.len());
            names.push(v[..end].to_string());
        }
        rest = &rest[tag_end.min(rest.len())..];
    }
    tracing::info!(hit = best.id, title = %best.title, found = names.len(), "chapterdb: chapter names");
    if names.is_empty() {
        Ok(None)
    } else {
        Ok(Some(names))
    }
}

/// A movie identity resolved on TMDB.
#[derive(Debug, Clone)]
pub struct MovieRef {
    pub title: String,
    pub year: Option<String>,
    pub tmdb_id: u64,
    pub poster_url: Option<String>,
    pub runtime_secs: Option<f64>,
}

/// Look up a movie on TMDB. `hint_runtime` (seconds) breaks ties between cuts.
pub fn lookup_movie(movie: &str, hint_runtime: f64) -> Result<Option<MovieRef>, CoreError> {
    if movie.trim().is_empty() {
        return Ok(None);
    }
    let q: String = movie
        .chars()
        .map(|c| if c == ' ' { '+' } else { c })
        .collect();
    let html = get(&format!("https://www.themoviedb.org/search?query={q}"))?;

    // Candidate ids from /movie/<id>-<slug> links, in page order.
    let mut cands: Vec<(u64, String)> = Vec::new();
    let mut rest = html.as_str();
    while let Some(pos) = rest.find("/movie/") {
        let after = &rest[pos + "/movie/".len()..];
        let idend = after.find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
        if idend == 0 {
            rest = &after[idend.max(1)..];
            continue;
        }
        let id: u64 = after[..idend].parse().unwrap_or(0);
        let tail = &after[idend..];
        let slug = if let Some(s) = tail.strip_prefix('-') {
            let end = s
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
                .unwrap_or(s.len());
            s[..end].to_string()
        } else {
            String::new()
        };
        if id > 0 && !cands.iter().any(|(i, _)| *i == id) {
            cands.push((id, slug));
        }
        rest = &after[idend..];
    }
    if cands.is_empty() {
        return Ok(None);
    }

    let want = norm(movie);
    let toks: Vec<String> = movie
        .split_whitespace()
        .map(|t| norm(t))
        .filter(|t| !t.is_empty())
        .collect();
    let score = |slug: &str| -> i32 {
        let s = norm(slug);
        if s == want {
            return 1000;
        }
        let mut v = 0;
        if !s.is_empty() && (s.starts_with(&want) || want.starts_with(&s)) {
            v += 300;
        }
        for t in &toks {
            if s.contains(t.as_str()) {
                v += 40;
            }
        }
        v
    };
    let (id, slug) = cands.iter().max_by_key(|(_, s)| score(s)).cloned().unwrap();
    tracing::info!(tmdb_id = id, %slug, "tmdb: matched");

    let page = get(&format!("https://www.themoviedb.org/movie/{id}"))?;

    // <title>Name (Year) @ The Movie Database (TMDB)
    let mut name = String::new();
    let mut year = None;
    if let Some(p) = page.find("<title>") {
        let v = &page[p + 7..];
        let end = v.find('<').unwrap_or(v.len());
        let t = v[..end].trim();
        let t = t.split('@').next().unwrap_or(t).trim();
        if let Some(op) = t.find(" (") {
            name = t[..op].trim().to_string();
            year = t[op + 2..].split(')').next().map(|s| s.trim().to_string());
        } else {
            name = t.to_string();
        }
    }

    // Runtime, when the page shows one ("2h 28m").
    let mut runtime_secs = None;
    for marker in ["class=\"runtime\"", "\"runtime\""] {
        if let Some(p) = page.find(marker) {
            let win = &page[p..page.len().min(p + 400)];
            if let (Some(hp), Some(mp)) = (win.find('h'), win.find('m')) {
                if hp < mp {
                    let h = win[..hp]
                        .chars()
                        .rev()
                        .take_while(|c| c.is_ascii_digit())
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect::<String>();
                    let m = win[hp..mp]
                        .chars()
                        .filter(|c| c.is_ascii_digit())
                        .collect::<String>();
                    if let (Ok(h), Ok(m)) = (h.parse::<f64>(), m.parse::<f64>()) {
                        runtime_secs = Some(h * 3600.0 + m * 60.0);
                        break;
                    }
                }
            }
        }
    }
    let _ = hint_runtime;

    // Poster: prefer a w500 variant.
    let mut poster_url = None;
    let mut rest = page.as_str();
    while let Some(pos) = rest.find("https://image.tmdb.org/t/p/") {
        let v = &rest[pos..];
        let end = v
            .find(|c: char| !(c.is_ascii_alphanumeric() || "/._:-".contains(c)))
            .unwrap_or(v.len());
        let url = &v[..end];
        if url.ends_with(".jpg") {
            if url.contains("/w500/") {
                poster_url = Some(url.to_string());
                break;
            }
            if poster_url.is_none() {
                poster_url = Some(url.to_string());
            }
        }
        rest = &rest[pos + 22..];
    }

    Ok(Some(MovieRef {
        title: if name.is_empty() {
            movie.to_string()
        } else {
            name
        },
        year,
        tmdb_id: id,
        poster_url,
        runtime_secs,
    }))
}

/// A poster plus the identity it was matched to.
#[derive(Debug, Clone)]
pub struct Poster {
    pub bytes: Vec<u8>,
    pub movie: String,
    pub year: Option<String>,
    pub tmdb_id: u64,
}

/// Look up a movie poster on TMDB.
pub fn poster(movie: &str, runtime_secs: f64) -> Result<Option<Poster>, CoreError> {
    let Some(m) = lookup_movie(movie, runtime_secs)? else {
        return Ok(None);
    };
    let Some(url) = m.poster_url else {
        return Ok(None);
    };
    let bytes = download(&url)?;
    Ok(Some(Poster {
        bytes,
        movie: m.title,
        year: m.year,
        tmdb_id: m.tmdb_id,
    }))
}
