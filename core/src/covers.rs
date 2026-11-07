//! Cover art: search and download from public sources, plus HiResCovers.
//!
//! Sources:
//! * `tmdb`       - the movie poster (public, no key).
//! * `dvdcover`   - dvdcover.com's WordPress REST API (public).
//! * `hirescovers`- HiResCovers. Its 204k-item manifest is read locally
//!   (`$HIRESCOVERS_DIR/state/gallery_items.jsonl`), so *searching* needs no
//!   login; the full-size original is fetched through `secure_image.php` with a
//!   member cookie, and falls back to the public `normal_` preview when the
//!   cookie is missing or stale.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::CoreError;

const UA: &str = "BluForge/0.1";
const BROWSER_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/157 Safari/537.36";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverSource {
    Tmdb,
    DvdCover,
    Hirescovers,
}

impl CoverSource {
    pub fn all() -> [CoverSource; 3] {
        [
            CoverSource::Hirescovers,
            CoverSource::DvdCover,
            CoverSource::Tmdb,
        ]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            CoverSource::Tmdb => "tmdb",
            CoverSource::DvdCover => "dvdcover",
            CoverSource::Hirescovers => "hirescovers",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "tmdb" => Some(CoverSource::Tmdb),
            "dvdcover" | "dvd" => Some(CoverSource::DvdCover),
            "hirescovers" | "hc" => Some(CoverSource::Hirescovers),
            _ => None,
        }
    }
}

/// One search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverHit {
    pub source: String,
    /// Source-specific id (pid / post id / tmdb id).
    pub id: String,
    pub title: String,
    /// "cover" (front) or "disc" (label).
    pub kind: String,
    /// Format hint: dvd / bluray / bluray-4k / poster.
    pub format: String,
    pub preview_url: String,
    pub full_url: String,
    /// True when `full_url` needs the member cookie.
    pub needs_auth: bool,
    /// Extra context (contributor, dimensions...).
    pub note: String,
}

/// Per-host request decoration: hirescovers needs the member cookie + a gallery
/// referer; dvdcover 403s hotlinks without a browser UA + referer.
fn decorate(url: &str, cookie: Option<&str>) -> ureq::Request {
    let mut req = ureq::get(url);
    if url.contains("hirescovers.net") {
        req = req.set("User-Agent", UA);
        if let Some(c) = cookie {
            req = req.set("Cookie", c).set("Referer", "https://www.hirescovers.net/gallery/");
        }
    } else if url.contains("dvdcover.com") {
        req = req.set("User-Agent", BROWSER_UA).set("Referer", "https://dvdcover.com/");
    } else {
        req = req.set("User-Agent", UA);
    }
    req
}

fn get(url: &str, cookie: Option<&str>) -> Result<String, CoreError> {
    let req = decorate(url, cookie);
    let resp = req
        .timeout(Duration::from_secs(30))
        .call()
        .map_err(|e| CoreError::Disc(format!("GET {url}: {e}")))?;
    let mut body = String::new();
    resp.into_reader()
        .take(16 * 1024 * 1024)
        .read_to_string(&mut body)
        .map_err(|e| CoreError::Disc(format!("read {url}: {e}")))?;
    Ok(body)
}

/// Fetch bytes plus the content type.
/// Public wrapper over the byte fetch, for thumbnails.
pub fn fetch(url: &str, cookie: Option<&str>) -> Result<(Vec<u8>, String), CoreError> {
    get_bytes(url, cookie)
}

/// Filesystem-safe form of a title.
pub fn sanitize(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| {
            if "/\\:*?\"<>|".contains(c) {
                '-'
            } else {
                c
            }
        })
        .collect();
    let t = t.replace("&#038;", "&").replace("&amp;", "&");
    let t = t.trim().trim_matches('.').to_string();
    let mut t = t;
    while t.contains("  ") {
        t = t.replace("  ", " ");
    }
    t
}

fn get_bytes(url: &str, cookie: Option<&str>) -> Result<(Vec<u8>, String), CoreError> {
    let req = decorate(url, cookie);
    let resp = req
        .timeout(Duration::from_secs(120))
        .call()
        .map_err(|e| CoreError::Disc(format!("GET {url}: {e}")))?;
    let ctype = resp.content_type().to_string();
    let mut body = Vec::new();
    resp.into_reader()
        .take(128 * 1024 * 1024)
        .read_to_end(&mut body)
        .map_err(|e| CoreError::Disc(format!("read {url}: {e}")))?;
    Ok((body, ctype))
}

fn jstr(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key)?.as_str().map(|s| s.to_string())
}

/// dvdcover stores originals that 403 on hotlink; the `single-free` render is
/// the free, fetchable one (falls back through `medium` to the original).
fn dvdcover_free_url(media: &serde_json::Value) -> Option<String> {
    let sizes = media.get("media_details")?.get("sizes")?;
    for key in ["single-free", "medium", "large", "full"] {
        if let Some(u) = sizes.get(key).and_then(|s| jstr(s, "source_url")) {
            return Some(u);
        }
    }
    jstr(media, "source_url")
}

// ---------------------------------------------------------------- tmdb

fn search_tmdb(query: &str, limit: usize) -> Result<Vec<CoverHit>, CoreError> {
    let Some(m) = crate::metadata::lookup_movie(query, 0.0)? else {
        return Ok(vec![]);
    };
    let Some(w500) = m.poster_url.clone() else {
        return Ok(vec![]);
    };
    let title = match &m.year {
        Some(y) => format!("{} ({y})", m.title),
        None => m.title.clone(),
    };
    Ok(vec![CoverHit {
        source: CoverSource::Tmdb.as_str().into(),
        id: m.tmdb_id.to_string(),
        title,
        kind: "cover".into(),
        format: "poster".into(),
        preview_url: w500.clone(),
        full_url: w500.replace("/w500/", "/original/"),
        needs_auth: false,
        note: "TMDB poster".into(),
    }])
    .map(|v: Vec<CoverHit>| v.into_iter().take(limit).collect())
}

// ------------------------------------------------------------ dvdcover

fn search_dvdcover(query: &str, limit: usize) -> Result<Vec<CoverHit>, CoreError> {
    // Their search degrades with extra words ("gladiator ii" ranks Pompeii
    // first); searching the head word and ranking locally works far better.
    let head = query.split_whitespace().next().unwrap_or(query);
    let q: String = head
        .chars()
        .map(|c| if c == ' ' { '+' } else { c })
        .collect();
    let body = get(
        &format!("https://dvdcover.com/wp-json/wp/v2/posts?search={q}&per_page={}&_fields=id,title,featured_media", (limit * 3).min(60)),
        None,
    )?;
    let posts: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap_or_default();
    let ids: Vec<u64> = posts
        .iter()
        .filter_map(|p| p.get("featured_media")?.as_u64())
        .filter(|i| *i > 0)
        .collect();
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let inc = ids
        .iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let media = get(
        &format!("https://dvdcover.com/wp-json/wp/v2/media?include={inc}&per_page=100&_fields=id,source_url,media_details"),
        None,
    )?;
    let medias: Vec<serde_json::Value> = serde_json::from_str(&media).unwrap_or_default();
    let mut by_id = std::collections::HashMap::new();
    for m in &medias {
        if let (Some(id), Some(url)) = (m.get("id").and_then(|v| v.as_u64()), dvdcover_free_url(m)) {
            by_id.insert(id, url);
        }
    }
    let mut out = Vec::new();
    for p in &posts {
        let Some(fm) = p.get("featured_media").and_then(|v| v.as_u64()) else {
            continue;
        };
        let Some(url) = by_id.get(&fm) else { continue };
        let title = p
            .get("title")
            .and_then(|t| jstr(t, "rendered"))
            .unwrap_or_else(|| query.to_string());
        let low = title.to_ascii_lowercase();
        let kind = if low.contains("label") || low.contains("disc") {
            "disc"
        } else {
            "cover"
        };
        let format = if low.contains("4k") || low.contains("uhd") {
            "bluray-4k"
        } else if low.contains("blu-ray") || low.contains("bluray") {
            "bluray"
        } else {
            "dvd"
        };
        out.push(CoverHit {
            source: CoverSource::DvdCover.as_str().into(),
            id: p.get("id").and_then(|v| v.as_u64()).unwrap_or(0).to_string(),
            title,
            kind: kind.into(),
            format: format.into(),
            preview_url: url.clone(),
            full_url: url.clone(),
            needs_auth: false,
            note: "dvdcover.com".into(),
        });
        if out.len() >= limit * 3 {
            break;
        }
    }
    // dvdcover's search is loose (it also matches post bodies), so rank by how
    // much of the query the *title* contains, weighting long words higher so
    // "gladiator ii" beats "pompeii".
    let words: Vec<String> = query
        .split_whitespace()
        .map(|w| w.to_ascii_lowercase())
        .collect();
    let mut scored: Vec<(usize, CoverHit)> = out
        .into_iter()
        .map(|h| {
            let t = h.title.to_ascii_lowercase();
            let score: usize = words
                .iter()
                .filter(|w| t.contains(w.as_str()))
                .map(|w| w.len() * w.len())
                .sum();
            (score, h)
        })
        .filter(|(s, _)| *s > 0)
        .collect();
    scored.sort_by_key(|(s, h)| (std::cmp::Reverse(*s), h.title.len()));
    let out: Vec<CoverHit> = scored.into_iter().map(|(_, h)| h).take(limit).collect();
    Ok(out)
}

// --------------------------------------------------------- hirescovers

pub fn manifest_path(dir: &Path) -> PathBuf {
    dir.join("state/gallery_items.jsonl")
}

/// Search the local HiResCovers manifest (204k items) - no network, no login.
pub fn search_hirescovers(manifest: &Path, query: &str, limit: usize) -> Result<Vec<CoverHit>, CoreError> {
    if !manifest.is_file() {
        return Ok(vec![]);
    }
    let words: Vec<String> = query
        .split_whitespace()
        .map(|w| w.to_ascii_lowercase())
        .collect();
    if words.is_empty() {
        return Ok(vec![]);
    }
    let f = std::fs::File::open(manifest)?;
    let mut out: Vec<CoverHit> = Vec::new();
    for line in BufReader::new(f).lines().map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let title = jstr(&v, "title").unwrap_or_default();
        let caption = jstr(&v, "caption").unwrap_or_default();
        let hay = format!("{} {}", title, caption).to_ascii_lowercase();
        if !words.iter().all(|w| hay.contains(w.as_str())) {
            continue;
        }
        let Some(pid) = v.get("pid").and_then(|p| p.as_u64()) else {
            continue;
        };
        let Some(normal) = jstr(&v, "normal") else {
            continue;
        };
        let cl = caption.to_ascii_lowercase();
        let kind = if cl.contains("label") || cl.contains("disc") {
            "disc"
        } else {
            "cover"
        };
        let format = if cl.contains("4k") || cl.contains("uhd") {
            "bluray-4k"
        } else if cl.contains("blu-ray") || cl.contains("bluray") {
            "bluray"
        } else if cl.contains("3d") {
            "bluray-3d"
        } else {
            "dvd"
        };
        out.push(CoverHit {
            source: CoverSource::Hirescovers.as_str().into(),
            id: pid.to_string(),
            title: if title.is_empty() { query.to_string() } else { title },
            kind: kind.into(),
            format: format.into(),
            preview_url: normal,
            full_url: format!("https://www.hirescovers.net/gallery/secure_image.php?pid={pid}"),
            needs_auth: true,
            note: caption,
        });
        if out.len() >= limit * 4 {
            out.sort_by(|a, b| a.format.cmp(&b.format).then(a.kind.cmp(&b.kind)));
            out.truncate(limit);
        }
    }
    out.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    out.truncate(limit);
    Ok(out)
}

/// Log in to HiResCovers (phpBB) and return a cookie header.
pub fn login_hirescovers(user: &str, pass: &str) -> Result<String, CoreError> {
    let jar = std::cell::RefCell::new(Vec::<(String, String)>::new());
    let cookie_of = |jar: &std::cell::RefCell<Vec<(String, String)>>| -> String {
        jar.borrow()
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; ")
    };

    let fetch = |url: &str, data: Option<String>| -> Result<(u16, String, String), CoreError> {
        let ck = cookie_of(&jar);
        let mut req = ureq::get(url).set("User-Agent", BROWSER_UA);
        if !ck.is_empty() {
            req = req.set("Cookie", &ck);
        }
        let resp = if let Some(body) = data {
            req.set("Content-Type", "application/x-www-form-urlencoded")
                .send_string(&body)
        } else {
            req.call()
        }
        .map_err(|e| CoreError::Disc(format!("hirescovers login: {e}")))?;
        for c in resp.all("set-cookie") {
            if let Some((kv, _)) = c.split_once(';') {
                if let Some((k, v)) = kv.split_once('=') {
                    let k = k.trim().to_string();
                    let v = v.trim().to_string();
                    let mut j = jar.borrow_mut();
                    if let Some(e) = j.iter_mut().find(|(a, _)| *a == k) {
                        e.1 = v;
                    } else {
                        j.push((k, v));
                    }
                }
            }
        }
        let status = resp.status();
        let mut body = String::new();
        resp.into_reader()
            .take(4 * 1024 * 1024)
            .read_to_string(&mut body)
            .ok();
        Ok((status, body, cookie_of(&jar)))
    };

    let (_s, page, _) = fetch("https://www.hirescovers.net/forum/ucp.php?mode=login", None)?;
    let hidden = |name: &str| -> String {
        let needle = format!("name=\"{name}\"");
        match page.find(&needle) {
            Some(p) => {
                let tail = &page[p..];
                match (tail.find("value=\""), tail.find('>')) {
                    (Some(v), Some(end)) if v < end => {
                        let v = &tail[v + 7..];
                        v[..v.find('"').unwrap_or(0)].to_string()
                    }
                    _ => String::new(),
                }
            }
            None => String::new(),
        }
    };
    let form_token = hidden("form_token");
    let creation_time = hidden("creation_time");
    let sid = hidden("sid");
    if form_token.is_empty() {
        tracing::warn!("hirescovers: no form_token on the login page");
    }

    let body = format!(
        "username={}&password={}&login=Login&autologin=1&redirect=index.php&creation_time={}&form_token={}&sid={}",
        urlencode(user),
        urlencode(pass),
        urlencode(&creation_time),
        urlencode(&form_token),
        urlencode(&sid)
    );
    let (_s, page2, cookie) = fetch("https://www.hirescovers.net/forum/ucp.php?mode=login", Some(body))?;
    if page2.contains("mode=logout") {
        tracing::info!("hirescovers: logged in");
    } else {
        tracing::warn!("hirescovers: login did not confirm (check the credentials)");
    }
    Ok(cookie)
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'*' => {
                (b as char).to_string()
            }
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

// ----------------------------------------------------------- dispatch

/// Search one source.
pub fn search(
    source: CoverSource,
    query: &str,
    limit: usize,
    hirescovers_dir: &Path,
) -> Result<Vec<CoverHit>, CoreError> {
    match source {
        CoverSource::Tmdb => search_tmdb(query, limit),
        CoverSource::DvdCover => search_dvdcover(query, limit),
        CoverSource::Hirescovers => {
            search_hirescovers(&manifest_path(hirescovers_dir), query, limit)
        }
    }
}

/// Re-resolve a hit from a `source id` pair (used by `covers get`).
pub fn by_id(source: CoverSource, id: &str, hirescovers_dir: &Path) -> Result<Option<CoverHit>, CoreError> {
    match source {
        CoverSource::Hirescovers => {
            let manifest = manifest_path(hirescovers_dir);
            if !manifest.is_file() {
                return Ok(None);
            }
            let f = std::fs::File::open(manifest)?;
            for line in BufReader::new(f).lines().map_while(Result::ok) {
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
                    continue;
                };
                if v.get("pid").and_then(|p| p.as_u64()).map(|p| p.to_string()).as_deref()
                    == Some(id)
                {
                    let caption = jstr(&v, "caption").unwrap_or_default();
                    let cl = caption.to_ascii_lowercase();
                    return Ok(Some(CoverHit {
                        source: CoverSource::Hirescovers.as_str().into(),
                        id: id.to_string(),
                        title: jstr(&v, "title").unwrap_or_default(),
                        kind: if cl.contains("label") || cl.contains("disc") {
                            "disc".into()
                        } else {
                            "cover".into()
                        },
                        format: if cl.contains("4k") {
                            "bluray-4k".into()
                        } else {
                            "dvd".into()
                        },
                        preview_url: jstr(&v, "normal").unwrap_or_default(),
                        full_url: format!(
                            "https://www.hirescovers.net/gallery/secure_image.php?pid={id}"
                        ),
                        needs_auth: true,
                        note: caption,
                    }));
                }
            }
            Ok(None)
        }
        CoverSource::DvdCover => {
            let body = get(
                &format!("https://dvdcover.com/wp-json/wp/v2/posts/{id}?_fields=id,title,featured_media"),
                None,
            )?;
            let post: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
            let Some(fm) = post.get("featured_media").and_then(|v| v.as_u64()) else {
                return Ok(None);
            };
            let m = get(
                &format!("https://dvdcover.com/wp-json/wp/v2/media/{fm}?_fields=source_url,media_details"),
                None,
            )?;
            let media: serde_json::Value = serde_json::from_str(&m).unwrap_or_default();
            let Some(url) = dvdcover_free_url(&media) else {
                return Ok(None);
            };
            Ok(Some(CoverHit {
                source: CoverSource::DvdCover.as_str().into(),
                id: id.to_string(),
                title: post
                    .get("title")
                    .and_then(|t| jstr(t, "rendered"))
                    .unwrap_or_default(),
                kind: "cover".into(),
                format: "dvd".into(),
                preview_url: url.clone(),
                full_url: url,
                needs_auth: false,
                note: "dvdcover.com".into(),
            }))
        }
        CoverSource::Tmdb => {
            let body = get(&format!("https://www.themoviedb.org/movie/{id}"), None)?;
            let mut poster = None;
            let mut rest = body.as_str();
            while let Some(p) = rest.find("https://image.tmdb.org/t/p/w500/") {
                let v = &rest[p..];
                let end = v.find(".jpg").map(|e| e + 4).unwrap_or(0);
                if end > 0 {
                    poster = Some(v[..end].to_string());
                    break;
                }
                rest = &rest[p + 10..];
            }
            let Some(url) = poster else { return Ok(None) };
            let title = body
                .find("<title>")
                .map(|p| {
                    let t = &body[p + 7..];
                    let end = t.find('<').unwrap_or(0);
                    t[..end].split('@').next().unwrap_or("").trim().to_string()
                })
                .unwrap_or_default();
            Ok(Some(CoverHit {
                source: CoverSource::Tmdb.as_str().into(),
                id: id.to_string(),
                title,
                kind: "cover".into(),
                format: "poster".into(),
                preview_url: url.clone(),
                full_url: url.replace("/w500/", "/original/"),
                needs_auth: false,
                note: "TMDB poster".into(),
            }))
        }
    }
}

/// Outcome of a download.
#[derive(Debug, Clone, Serialize)]
pub struct Saved {
    pub path: PathBuf,
    /// False when the full-size original needed a login and the public preview
    /// was saved instead.
    pub full_size: bool,
    pub bytes: u64,
}

fn looks_like_image(ctype: &str, body: &[u8]) -> bool {
    ctype.starts_with("image/")
        || body.starts_with(&[0xFF, 0xD8])
        || body.starts_with(&[0x89, b'P', b'N', b'G'])
}

/// Download a hit to `dest` (a file path). HiResCovers needs `cookie` for the
/// full-size original; without it (or on a stale cookie) the preview is used.
pub fn download(hit: &CoverHit, dest: &Path, cookie: Option<&str>) -> Result<Saved, CoreError> {
    let (mut body, mut ctype) = get_bytes(&hit.full_url, cookie)?;
    let mut full = looks_like_image(&ctype, &body);
    if !full {
        // secure_image.php served a login page: fall back to the public preview.
        let (b2, c2) = get_bytes(&hit.preview_url, cookie)?;
        if looks_like_image(&c2, &b2) {
            body = b2;
            ctype = c2;
            full = false;
        } else {
            full = false;
        }
    }
    let ext = if ctype.contains("png") || body.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else {
        "jpg"
    };
    let dest = dest.with_extension(ext);
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::File::create(&dest)?;
    f.write_all(&body)?;
    Ok(Saved {
        path: dest,
        full_size: full,
        bytes: body.len() as u64,
    })
}

/// The HiResCovers member cookie: the configured one, else whatever the
/// hirescovers tooling last refreshed into its state directory.
pub fn cookie(cfg: &crate::AppConfig) -> Option<String> {
    if let Some(c) = &cfg.hirescovers_cookie {
        if !c.trim().is_empty() {
            return Some(c.trim().to_string());
        }
    }
    for name in ["cookie_free.txt", "cookie_header.txt"] {
        let p = cfg.hirescovers_dir.join("state").join(name);
        if let Ok(c) = std::fs::read_to_string(&p) {
            if !c.trim().is_empty() {
                return Some(c.trim().to_string());
            }
        }
    }
    None
}

/// Download the best front cover (and disc label, when one exists) next to a
/// rip. Prefers a real case cover over the TMDB poster, and a blu-ray/4k entry
/// over a dvd one. Never fails the rip: a source error just moves on.
pub fn auto_covers(movie: &str, dest: &Path, cfg: &crate::AppConfig) -> Vec<Saved> {
    let cookie = cookie(cfg);
    let parent = dest.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let stem = dest
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "cover".into());
    // Try the full name, then with trailing words dropped ("Dune Part Two" ->
    // "Dune Part" -> "Dune") so a sequel still finds a cover.
    let toks: Vec<&str> = movie.split_whitespace().collect();
    let mut queries: Vec<String> = vec![movie.to_string()];
    for n in (1..toks.len()).rev() {
        queries.push(toks[..n].join(" "));
    }

    let mut saved = Vec::new();
    for want in ["cover", "disc"] {
        for source in CoverSource::all() {
            let mut hits = Vec::new();
            for q in &queries {
                hits = match search(source, q, 16, &cfg.hirescovers_dir) {
                    Ok(h) => h,
                    Err(_) => Vec::new(),
                };
                if hits.iter().any(|h| h.kind == want) {
                    break;
                }
            }
            let hits = hits;
            let pick = hits
                .iter()
                .find(|h| h.kind == want && h.format != "dvd")
                .or_else(|| hits.iter().find(|h| h.kind == want));
            if let Some(hit) = pick {
                let target = parent.join(format!("{stem} - {want}"));
                match download(hit, &target, cookie.as_deref()) {
                    Ok(s) => {
                        tracing::info!(
                            path = %s.path.display(),
                            source = %hit.source,
                            full = s.full_size,
                            "cover saved"
                        );
                        saved.push(s);
                        break;
                    }
                    Err(e) => tracing::debug!(error = %e, "cover download failed"),
                }
            }
        }
    }
    saved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencode_basics() {
        assert_eq!(urlencode("a b"), "a+b");
        assert_eq!(urlencode("p@ss/w"), "p%40ss%2Fw");
    }

    #[test]
    fn image_sniffing() {
        assert!(looks_like_image("text/html", &[0xFF, 0xD8, 0x11]));
        assert!(looks_like_image("image/jpeg", b""));
        assert!(!looks_like_image("text/html", b"<html>"));
    }

    #[test]
    fn source_roundtrip() {
        for s in CoverSource::all() {
            assert_eq!(CoverSource::parse(s.as_str()), Some(s));
        }
        assert_eq!(CoverSource::parse("nope"), None);
    }
}
