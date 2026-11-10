//! Disc menu documents.
//!
//! A *menu document* is a parsed, editable description of a disc's title menu:
//! the navigation entries from `BDMV/index.bdmv` (first playback, top menu and
//! the per-title movie/BD-J objects) plus the playable titles libbluray reports
//! (playlist, duration, chapter count, streams).
//!
//! Documents can be edited, saved as named templates under
//! `~/Library/Application Support/keepsake-atelier/menus/`, and applied to a different
//! disc. Applying transfers the editable parts (title names, the main-feature
//! flag, chapter names) by matching playlists and durations; the target disc's
//! own navigation objects are left alone, because those are disc-specific.
//!
//! Parsing only reads files, it never talks to the drive.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::CoreError;

/// Disc menu document schema version.
pub const MENU_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NavType {
    /// No navigation entry (object type 0).
    #[default]
    None,
    /// HDMV movie object (object type 1).
    Hdmv,
    /// BD-J object (object type 2).
    Bdj,
    /// Anything else; kept so an unusual disc is inspectable.
    Unknown,
}

impl NavType {
    fn from_bits(b: u8) -> Self {
        match b >> 6 {
            0 => NavType::None,
            1 => NavType::Hdmv,
            2 => NavType::Bdj,
            _ => NavType::Unknown,
        }
    }
}

/// One navigation entry: `first_playback`, `top_menu`, or a title entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct NavTarget {
    pub kind: NavType,
    /// HDMV movie object id, when the entry is HDMV.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<u16>,
    /// BD-J object file name (`00001`), when the entry is BD-J.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// The navigation table from `index.bdmv`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct NavIndex {
    pub first_playback: NavTarget,
    pub top_menu: NavTarget,
    pub titles: Vec<NavTarget>,
}

/// One playable title, with the editable fields a template carries.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MenuTitle {
    /// Title index as `keepsake-atelier disc` / `--title` use it.
    pub index: u32,
    pub playlist: u32,
    pub duration_secs: f64,
    pub chapters: u32,
    /// Editable display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Editable: this title is the main feature.
    #[serde(default)]
    pub main: bool,
    /// Editable chapter labels, aligned to the disc's chapter marks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chapter_names: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_streams: Option<u32>,
}

/// A parsed disc menu.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiscMenu {
    #[serde(default = "menu_version")]
    pub version: u32,
    /// Disc label as libbluray reports it.
    #[serde(default)]
    pub label: String,
    /// `SHA-1(AACS/Unit_Key_RO.inf)` when the disc is AACS.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disc_id: Option<String>,
    /// Where it was parsed from, for reference only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Saved template this document was applied from, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_from: Option<String>,
    /// Set when part of the disc could not be read (eg. titles need a key).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub navigation: Option<NavIndex>,
    #[serde(default)]
    pub titles: Vec<MenuTitle>,
}

fn menu_version() -> u32 {
    MENU_VERSION
}

impl DiscMenu {
    /// Title with this index, if present.
    pub fn title(&self, index: u32) -> Option<&MenuTitle> {
        self.titles.iter().find(|t| t.index == index)
    }

    fn title_mut(&mut self, index: u32) -> Result<&mut MenuTitle, CoreError> {
        self.titles
            .iter_mut()
            .find(|t| t.index == index)
            .ok_or_else(|| CoreError::Config(format!("no title {index} in this menu")))
    }

    /// Rename a title.
    pub fn set_name(&mut self, index: u32, name: &str) -> Result<(), CoreError> {
        self.title_mut(index)?.name = Some(name.trim().to_string());
        Ok(())
    }

    /// Make `index` the single main feature.
    pub fn set_main(&mut self, index: u32) -> Result<(), CoreError> {
        if !self.titles.iter().any(|t| t.index == index) {
            return Err(CoreError::Config(format!("no title {index} in this menu")));
        }
        for t in &mut self.titles {
            t.main = t.index == index;
        }
        Ok(())
    }

    /// Set a 1-based chapter label, growing the list if needed.
    pub fn set_chapter(&mut self, index: u32, chapter: usize, name: &str) -> Result<(), CoreError> {
        if chapter == 0 {
            return Err(CoreError::Config("chapter numbers are 1-based".into()));
        }
        let t = self.title_mut(index)?;
        while t.chapter_names.len() < chapter {
            t.chapter_names
                .push(format!("Chapter {}", t.chapter_names.len() + 1));
        }
        t.chapter_names[chapter - 1] = name.trim().to_string();
        Ok(())
    }
}

/// Parse a `N:Text` chapter label (also accepts `N=Text`).
pub fn parse_chapter_spec(spec: &str) -> Result<(usize, String), CoreError> {
    let (n, text) = spec
        .split_once(':')
        .or_else(|| spec.split_once('='))
        .ok_or_else(|| CoreError::Config(format!("chapter label must be N:Text, got {spec:?}")))?;
    let n: usize = n
        .trim()
        .parse()
        .map_err(|_| CoreError::Config(format!("chapter number must be numeric, got {n:?}")))?;
    if n == 0 {
        return Err(CoreError::Config("chapter numbers are 1-based".into()));
    }
    Ok((n, text.trim().to_string()))
}

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

/// Parse one navigation entry.
///
/// `index.bdmv` stores 12-byte entries on the discs seen so far: a type byte
/// (top two bits), flags, a 16-bit object id at offset 6, and — for BD-J — the
/// five ASCII digits of the BD-J object name in the same slot. Older/simpler
/// layouts put the id right after the type byte, so that is the fallback.
fn parse_nav(entry: &[u8]) -> NavTarget {
    if entry.is_empty() {
        return NavTarget::default();
    }
    let kind = NavType::from_bits(entry[0]);
    let mut out = NavTarget {
        kind,
        object: None,
        name: None,
    };
    if entry.len() >= 11 && kind == NavType::Bdj {
        let digits = &entry[6..11];
        if digits.iter().all(|c| c.is_ascii_digit()) {
            out.name = Some(String::from_utf8_lossy(digits).into_owned());
            return out;
        }
    }
    if let Some(id) = be16(entry, 6).filter(|id| *id != 0) {
        out.object = Some(id);
    } else if let Some(id) = be16(entry, 1).filter(|id| *id != 0) {
        out.object = Some(id);
    }
    out
}

/// Parse the navigation table out of an `index.bdmv`.
///
/// The entry stride is self-describing: the block is
/// `u32 length | first | top | u16 count | count * entry`, so we accept the
/// first stride whose arithmetic covers the block exactly.
pub fn parse_index(data: &[u8]) -> Option<NavIndex> {
    if data.len() < 12 || &data[0..4] != b"INDX" {
        return None;
    }
    let start = be32(data, 8)? as usize;
    let body = data.get(start..)?;
    let total = (be32(body, 0)? as usize).checked_add(4)?;
    if total > body.len() {
        return None;
    }
    let body = &body[..total];

    for esize in [12usize, 6, 8] {
        let count_at = 4 + 2 * esize;
        let Some(count) = be16(body, count_at).map(usize::from) else {
            continue;
        };
        if count_at + 2 + count * esize != body.len() {
            continue;
        }
        let first = parse_nav(&body[4..4 + esize]);
        let top = parse_nav(&body[4 + esize..4 + 2 * esize]);
        let mut titles = Vec::with_capacity(count);
        let mut p = count_at + 2;
        for _ in 0..count {
            titles.push(parse_nav(&body[p..p + esize]));
            p += esize;
        }
        return Some(NavIndex {
            first_playback: first,
            top_menu: top,
            titles,
        });
    }
    None
}

/// The BDMV directory for a mounted volume or a BDMV folder.
pub fn bdmv_dir(source: &Path) -> Option<PathBuf> {
    if source.join("index.bdmv").is_file() {
        return Some(source.to_path_buf());
    }
    let candidate = source.join("BDMV");
    candidate.join("index.bdmv").is_file().then_some(candidate)
}

/// Parse a disc into a menu document.
///
/// `keydb` is only needed to enumerate titles of a disc libaacs can decrypt;
/// the navigation table is read directly from the files and does not need it.
pub fn parse(source: &Path, keydb: Option<&Path>) -> Result<DiscMenu, CoreError> {
    let source = source.to_path_buf();
    let bdmv = bdmv_dir(&source).ok_or_else(|| {
        CoreError::Disc(format!(
            "no BDMV/index.bdmv under {} (mount the disc or point at a BDMV folder)",
            source.display()
        ))
    })?;

    let mut doc = DiscMenu {
        version: MENU_VERSION,
        source: Some(source.display().to_string()),
        ..Default::default()
    };

    if let Ok(data) = std::fs::read(bdmv.join("index.bdmv")) {
        doc.navigation = parse_index(&data);
    }

    match crate::disc::Disc::open(&source, keydb) {
        Ok(disc) => {
            let info = disc.info()?;
            doc.label = info.name.clone();
            doc.disc_id = crate::keys::disc_id_for_path(&source);
            for t in &info.titles {
                let mut names = Vec::new();
                if let Ok(chapters) = disc.chapters(t.index) {
                    if t.chapters as usize == chapters.len() {
                        names = chapters
                            .iter()
                            .enumerate()
                            .map(|(i, c)| {
                                c.name
                                    .clone()
                                    .unwrap_or_else(|| format!("Chapter {}", i + 1))
                            })
                            .collect();
                    }
                }
                doc.titles.push(MenuTitle {
                    index: t.index,
                    playlist: t.playlist,
                    duration_secs: t.duration_secs,
                    chapters: t.chapters,
                    name: None,
                    main: t.is_main,
                    chapter_names: names,
                    resolution: t.resolution.clone(),
                    audio_streams: Some(t.audio_streams),
                });
            }
        }
        Err(e) => {
            doc.note = Some(format!(
                "titles unavailable ({e}); navigation was read from index.bdmv"
            ));
        }
    }

    if doc.label.is_empty() {
        doc.label = source
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
    }

    Ok(doc)
}

// ---------------------------------------------------------------------------
// Template store
// ---------------------------------------------------------------------------

/// Directory holding saved menu templates.
pub fn store_dir() -> PathBuf {
    crate::config_dir().join("menus")
}

/// Turn a user-supplied name into a safe file stem.
pub fn slug(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ') {
            out.push(c);
        } else if !c.is_whitespace() {
            out.push('-');
        }
    }
    let out = out.trim().trim_matches('.').to_string();
    if out.is_empty() {
        "menu".into()
    } else {
        out
    }
}

/// Save a document under `name` and return its path.
pub fn save(dir: &Path, name: &str, doc: &DiscMenu) -> Result<PathBuf, CoreError> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.json", slug(name)));
    let text = serde_json::to_string_pretty(doc)
        .map_err(|e| CoreError::Config(format!("serialise menu: {e}")))?;
    std::fs::write(&path, text + "\n")?;
    Ok(path)
}

/// Load a document by template name or by explicit path.
pub fn load(dir: &Path, name_or_path: &str) -> Result<DiscMenu, CoreError> {
    let direct = Path::new(name_or_path);
    let path = if direct.extension().is_some_and(|e| e == "json") && direct.is_file() {
        direct.to_path_buf()
    } else {
        dir.join(format!("{}.json", slug(name_or_path)))
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|e| CoreError::Config(format!("{}: {e}", path.display())))?;
    serde_json::from_str(&text).map_err(|e| CoreError::Config(format!("{}: {e}", path.display())))
}

/// The saved template whose document was parsed from this disc id.
pub fn find_by_disc_id(dir: &Path, disc_id: &str) -> Option<(String, DiscMenu)> {
    for (name, _) in list(dir) {
        if let Ok(doc) = load(dir, &name) {
            if doc.disc_id.as_deref() == Some(disc_id) {
                return Some((name, doc));
            }
        }
    }
    None
}

/// Saved templates as `(name, path)`, sorted by name.
pub fn list(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(String, PathBuf)> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .map(|e| {
            let name = e
                .path()
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            (name, e.path())
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Delete a saved template.
pub fn remove(dir: &Path, name: &str) -> Result<PathBuf, CoreError> {
    let path = dir.join(format!("{}.json", slug(name)));
    std::fs::remove_file(&path)
        .map_err(|e| CoreError::Config(format!("{}: {e}", path.display())))?;
    Ok(path)
}

// ---------------------------------------------------------------------------
// Applying a template to another disc
// ---------------------------------------------------------------------------

/// Find the target title that corresponds to `want`.
///
/// Playlist id first (stable across editions of the same disc), then duration
/// within 1 second, then the same position when both discs have the same title
/// count. `used` prevents two template entries claiming one target title.
fn match_title(template: &DiscMenu, target: &DiscMenu, i: usize, used: &[bool]) -> Option<usize> {
    let want = template.titles.get(i)?;
    let free = |j: usize| !used.get(j).copied().unwrap_or(true);
    if want.playlist != 0 {
        if let Some(j) = (0..target.titles.len())
            .find(|j| free(*j) && target.titles[*j].playlist == want.playlist)
        {
            return Some(j);
        }
    }
    if want.duration_secs > 0.0 {
        if let Some(j) = (0..target.titles.len()).find(|j| {
            free(*j) && (target.titles[*j].duration_secs - want.duration_secs).abs() <= 1.0
        }) {
            return Some(j);
        }
    }
    if template.titles.len() == target.titles.len() && free(i) {
        return Some(i);
    }
    None
}

/// Apply a template's editable fields to another disc's menu document.
///
/// The target keeps its own navigation table, playlists and durations; only
/// names, the main-feature flag and chapter names are carried over.
pub fn apply(template: &DiscMenu, target: &DiscMenu, template_name: &str) -> DiscMenu {
    let mut out = target.clone();
    out.applied_from = Some(template_name.to_string());
    out.note = None;

    let mut used = vec![false; target.titles.len()];
    let mut matched_main = None;
    for i in 0..template.titles.len() {
        let Some(j) = match_title(template, target, i, &used) else {
            continue;
        };
        used[j] = true;
        let want = &template.titles[i];
        if want.main {
            matched_main = Some(j);
        }
        let dst = &mut out.titles[j];
        if want.name.is_some() {
            dst.name = want.name.clone();
        }
        if !want.chapter_names.is_empty() && want.chapter_names.len() == dst.chapter_names.len() {
            dst.chapter_names = want.chapter_names.clone();
        }
    }

    // Exactly one main feature, as the template intends.
    if let Some(j) = matched_main {
        for (k, t) in out.titles.iter_mut().enumerate() {
            t.main = k == j;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `Indexes()` block from The Jungle Book (Diamond Edition), trimmed to
    /// the real bytes: 2 navigation entries, 49 titles, 12-byte stride.
    fn jungle_index() -> Vec<u8> {
        let start = 0x4eu32;
        let mut v = Vec::new();
        v.extend_from_slice(b"INDX0200");
        v.extend_from_slice(&start.to_be_bytes()); // IndexesStartAddress
        v.extend_from_slice(&0u32.to_be_bytes());
        v.extend_from_slice(&[0u8; 24]);
        // The real file carries a vendor "Provider Name" block between the
        // header and Indexes(); pad to the same 0x4e offset.
        v.resize(start as usize, 0);
        // Indexes(): length, then first play / top menu / count / entries.
        let mut body = Vec::new();
        body.extend_from_slice(&[0u8; 4]); // length patched below
        body.extend_from_slice(&[0x80, 0, 0, 0, 0x80, 0, b'6', b'5', b'5', b'3', b'5', 0]);
        body.extend_from_slice(&[0x80, 0, 0, 0, 0xc0, 0, b'0', b'0', b'0', b'0', b'0', 0]);
        body.extend_from_slice(&3u16.to_be_bytes());
        body.extend_from_slice(&[0xa0, 0, 0, 0, 0x80, 0, b'0', b'0', b'0', b'0', b'1', 0]);
        body.extend_from_slice(&[0x60, 0, 0, 0, 0x00, 0, 0x00, 0x01, 0, 0, 0, 0]);
        body.extend_from_slice(&[0x60, 0, 0, 0, 0x00, 0, 0x00, 0x02, 0, 0, 0, 0]);
        let len = (body.len() - 4) as u32;
        body[..4].copy_from_slice(&len.to_be_bytes());
        v.extend_from_slice(&body);
        v
    }

    #[test]
    fn parses_real_index_layout() {
        let nav = parse_index(&jungle_index()).expect("parsed");
        assert_eq!(nav.first_playback.kind, NavType::Bdj);
        assert_eq!(nav.first_playback.name.as_deref(), Some("65535"));
        assert_eq!(nav.top_menu.kind, NavType::Bdj);
        assert_eq!(nav.top_menu.name.as_deref(), Some("00000"));
        assert_eq!(nav.titles.len(), 3);
        assert_eq!(nav.titles[0].name.as_deref(), Some("00001"));
        assert_eq!(nav.titles[1].kind, NavType::Hdmv);
        assert_eq!(nav.titles[1].object, Some(1));
        assert_eq!(nav.titles[2].object, Some(2));
    }

    #[test]
    fn rejects_short_or_foreign_data() {
        assert!(parse_index(b"INDX0200").is_none());
        assert!(parse_index(b"NOPE0200\0\0\0\0").is_none());
    }

    fn doc(label: &str, titles: &[(u32, f64)]) -> DiscMenu {
        DiscMenu {
            version: MENU_VERSION,
            label: label.into(),
            titles: titles
                .iter()
                .enumerate()
                .map(|(i, (playlist, duration))| MenuTitle {
                    index: i as u32,
                    playlist: *playlist,
                    duration_secs: *duration,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn applies_by_playlist_then_duration() {
        let mut tpl = doc("A", &[(800, 4707.0), (801, 100.0)]);
        tpl.titles[0].name = Some("Main Feature".into());
        tpl.titles[0].main = true;
        tpl.titles[1].name = Some("Trailer".into());

        // Same disc, different title order: playlist match has to win.
        let target = doc("B", &[(801, 100.0), (800, 4707.0)]);
        let out = apply(&tpl, &target, "jungle");
        assert_eq!(out.applied_from.as_deref(), Some("jungle"));
        let main = out.titles.iter().find(|t| t.playlist == 800).unwrap();
        assert_eq!(main.name.as_deref(), Some("Main Feature"));
        assert!(main.main);
        let trailer = out.titles.iter().find(|t| t.playlist == 801).unwrap();
        assert_eq!(trailer.name.as_deref(), Some("Trailer"));
        assert!(!trailer.main);
    }

    #[test]
    fn applies_chapter_names_only_when_counts_agree() {
        let mut tpl = doc("A", &[(800, 4707.0)]);
        tpl.titles[0].chapter_names = vec!["One".into(), "Two".into()];
        let mut target = doc("B", &[(800, 4707.0)]);
        target.titles[0].chapter_names = vec!["x".into(), "y".into()];
        let out = apply(&tpl, &target, "a");
        assert_eq!(out.titles[0].chapter_names, vec!["One", "Two"]);

        target.titles[0].chapter_names = vec!["x".into()];
        let out = apply(&tpl, &target, "a");
        assert_eq!(out.titles[0].chapter_names, vec!["x"]);
    }

    #[test]
    fn edits_names_main_and_chapters() {
        let mut d = doc("A", &[(800, 10.0), (801, 20.0)]);
        d.set_name(1, "  Trailer  ").unwrap();
        d.set_main(0).unwrap();
        assert_eq!(d.title(1).unwrap().name.as_deref(), Some("Trailer"));
        assert!(d.title(0).unwrap().main);
        assert!(!d.title(1).unwrap().main);

        d.set_chapter(1, 2, " Second ").unwrap();
        assert_eq!(
            d.title(1).unwrap().chapter_names,
            vec!["Chapter 1", "Second"]
        );

        assert_eq!(parse_chapter_spec("2:Hello").unwrap(), (2, "Hello".into()));
        assert_eq!(parse_chapter_spec("3=Bye").unwrap(), (3, "Bye".into()));
        assert!(parse_chapter_spec("0:x").is_err());
        assert!(parse_chapter_spec("nope").is_err());
        assert!(d.set_main(9).is_err());
    }

    #[test]
    fn finds_a_template_by_disc_id() {
        let dir = std::env::temp_dir().join(format!("keepsake-menu-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut one = doc("A", &[(800, 10.0)]);
        one.disc_id = Some("abc".into());
        save(&dir, "one", &one).unwrap();
        let mut two = doc("B", &[]);
        two.disc_id = Some("def".into());
        save(&dir, "two", &two).unwrap();

        assert_eq!(find_by_disc_id(&dir, "abc").expect("found").0, "one");
        assert!(find_by_disc_id(&dir, "zzz").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn store_round_trip_and_slug() {
        let dir = std::env::temp_dir().join(format!("keepsake-menu-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let d = doc("My Disc", &[(1, 2.0)]);
        let p = save(&dir, "My Disc: 1967", &d).expect("saved");
        assert!(p.ends_with("My Disc- 1967.json"));
        let back = load(&dir, "My Disc: 1967").expect("loaded");
        assert_eq!(back.label, "My Disc");
        assert_eq!(list(&dir).len(), 1);
        remove(&dir, "My Disc: 1967").expect("removed");
        assert!(list(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
