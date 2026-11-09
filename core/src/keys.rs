//! AACS decryption-key database (`KEYDB.cfg`) management.
//!
//! BluForge keeps a local copy of the community key database and refreshes it
//! from the network on demand (and on launch). The database is consumed by
//! libaacs at rip time; here we own fetch / store / parse, plus the small key
//! store used to retire MakeMKV: any disc you can key decrypts with libaacs
//! alone, offline, with no MakeMKV process.
//!
//! KEYDB line format (the fields libaacs understands):
//!
//! ```text
//! 0x<DISCID> = <name> | D | <date> | M | 0x<media-key> | I | 0x<volume-id> |
//!              V | 0x<volume-unique-key> | U | <n>-0x<unit-key>,... ; note
//! ```

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::CoreError;

/// Where the local copy lives.
pub fn keydb_path() -> PathBuf {
    crate::config_dir().join("KEYDB.cfg")
}

/// Mirrors, in preference order. Each entry is `(url, is_zip)`.
const MIRRORS: &[(&str, bool)] = &[
    ("http://fvonline-db.bplaced.net/export/keydb_eng.zip", true),
    (
        "https://raw.githubusercontent.com/lwjohnst86/KEYDB/master/KEYDB.cfg",
        false,
    ),
    ("http://fvonline-db.bplaced.net/export/keydb_eng.zip", true),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyDbStatus {
    pub path: PathBuf,
    pub exists: bool,
    pub entries: usize,
    pub bytes: u64,
    pub modified: Option<u64>,
}

pub fn status() -> KeyDbStatus {
    let path = keydb_path();
    let meta = fs::metadata(&path).ok();
    let entries = if meta.is_some() {
        fs::read_to_string(&path)
            .map(|t| {
                t.lines()
                    .filter(|l| l.starts_with("0x") && l.contains('='))
                    .count()
            })
            .unwrap_or(0)
    } else {
        0
    };
    KeyDbStatus {
        exists: meta.is_some(),
        bytes: meta.as_ref().map(|m| m.len()).unwrap_or(0),
        modified: meta
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs()),
        entries,
        path,
    }
}

/// Download the latest key database, replacing the local copy atomically.
pub fn update() -> Result<KeyDbStatus, CoreError> {
    let dir = crate::config_dir();
    fs::create_dir_all(&dir)?;
    let dest = keydb_path();

    let mut last_err: Option<CoreError> = None;
    for (url, is_zip) in MIRRORS {
        match fetch(url, *is_zip) {
            Ok(bytes) => {
                let tmp = dest.with_extension("cfg.tmp");
                fs::write(&tmp, &bytes)?;
                validate(&tmp)?;
                fs::rename(&tmp, &dest)?;
                tracing::info!(url, bytes = bytes.len(), "key database updated");
                return Ok(status());
            }
            Err(e) => {
                tracing::warn!(url, error = %e, "key mirror failed");
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| CoreError::Keys("no mirrors configured".into())))
}

fn fetch(url: &str, is_zip: bool) -> Result<Vec<u8>, CoreError> {
    let resp = ureq::get(url)
        .timeout(std::time::Duration::from_secs(60))
        .call()
        .map_err(|e| CoreError::Keys(format!("GET {url}: {e}")))?;
    let mut raw = Vec::new();
    resp.into_reader()
        .take(64 * 1024 * 1024)
        .read_to_end(&mut raw)?;

    if !is_zip {
        return Ok(raw);
    }
    let reader = std::io::Cursor::new(raw);
    let mut archive =
        zip::ZipArchive::new(reader).map_err(|e| CoreError::Keys(format!("zip: {e}")))?;
    for i in 0..archive.len() {
        let mut f = archive
            .by_index(i)
            .map_err(|e| CoreError::Keys(format!("zip entry: {e}")))?;
        let name = f.name().to_ascii_lowercase();
        if name.ends_with("keydb.cfg") {
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)?;
            return Ok(buf);
        }
    }
    Err(CoreError::Keys("no KEYDB.cfg inside archive".into()))
}

fn validate(path: &Path) -> Result<(), CoreError> {
    let text = fs::read_to_string(path)?;
    let entries = text.lines().filter(|l| l.starts_with("0x")).count();
    if entries < 100 {
        return Err(CoreError::Keys(format!(
            "suspiciously small key database ({entries} entries)"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Entry parsing / writing
// ---------------------------------------------------------------------------

/// One parsed KEYDB entry. `id` is the 40-hex disc id (lowercase).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct KeyEntry {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub media_key: Option<String>,
    #[serde(default)]
    pub volume_id: Option<String>,
    #[serde(default)]
    pub vuk: Option<String>,
    #[serde(default)]
    pub unit_keys: Vec<String>,
    #[serde(default)]
    pub note: Option<String>,
}

/// Normalise a disc id or key: drop `0x`, lowercase, require hex.
fn hexnorm(s: &str) -> Option<String> {
    let t = s.trim();
    let t = t
        .strip_prefix("0x")
        .or_else(|| t.strip_prefix("0X"))
        .unwrap_or(t);
    let t: String = t.trim().to_ascii_lowercase();
    if t.is_empty() || !t.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(t)
}

fn normalize_id(s: &str) -> Option<String> {
    let id = hexnorm(s)?;
    if id.len() == 40 {
        Some(id)
    } else {
        None
    }
}

/// Parse one `0x<id> = ...` line. Returns `None` for comments/blank lines.
pub fn parse_line(line: &str) -> Option<KeyEntry> {
    let line = line.trim();
    if !line.starts_with("0x") {
        return None;
    }
    let (body, note) = match line.split_once(';') {
        Some((b, n)) => (b, Some(n.trim().to_string())),
        None => (line, None),
    };
    let (idpart, rest) = body.split_once('=')?;
    let id = normalize_id(idpart)?;
    let mut fields = rest.split('|').map(|s| s.trim());
    let name = fields.next().map(str::to_string).filter(|s| !s.is_empty());
    let mut e = KeyEntry {
        id,
        name,
        ..Default::default()
    };
    let parts: Vec<&str> = fields.collect();
    let mut i = 0;
    while i + 1 < parts.len() {
        let (tag, val) = (parts[i], parts[i + 1]);
        match tag {
            "M" => e.media_key = hexnorm(val),
            "I" => e.volume_id = hexnorm(val),
            "V" => e.vuk = hexnorm(val),
            "U" => e.unit_keys = parse_units(val),
            _ => {}
        }
        i += 2;
    }
    e.note = note.filter(|n| !n.is_empty());
    Some(e)
}

/// `U` value: `<count>-0x<key>[,0x<key>...]`.
fn parse_units(val: &str) -> Vec<String> {
    let keys = match val.split_once('-') {
        Some((_, k)) => k,
        None => val,
    };
    keys.split(',')
        .filter_map(hexnorm)
        .filter(|k| !k.is_empty())
        .collect()
}

/// Render an entry back to a KEYDB line. `date` is `YYYY-MM-DD`.
pub fn entry_to_line(e: &KeyEntry, date: &str) -> String {
    let mut s = format!(
        "0x{} = {}",
        e.id.to_ascii_uppercase(),
        e.name.clone().unwrap_or_default()
    );
    s.push_str(&format!(" | D | {date}"));
    if let Some(mk) = &e.media_key {
        s.push_str(&format!(" | M | 0x{}", mk.to_ascii_uppercase()));
    }
    if let Some(vid) = &e.volume_id {
        s.push_str(&format!(" | I | 0x{}", vid.to_ascii_uppercase()));
    }
    if let Some(vuk) = &e.vuk {
        s.push_str(&format!(" | V | 0x{}", vuk.to_ascii_uppercase()));
    }
    if !e.unit_keys.is_empty() {
        let keys: Vec<String> = e
            .unit_keys
            .iter()
            .map(|k| format!("0x{}", k.to_ascii_uppercase()))
            .collect();
        s.push_str(&format!(" | U | {}-{}", e.unit_keys.len(), keys.join(",")));
    }
    if let Some(n) = &e.note {
        s.push_str(&format!(" ; {n}"));
    }
    s
}

fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    civil_from_days((secs / 86_400) as i64)
}

/// Days since 1970-01-01 -> `YYYY-MM-DD` (Howard Hinnant's civil algorithm).
fn civil_from_days(z: i64) -> String {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

// ---------------------------------------------------------------------------
// CRUD over a KEYDB file
// ---------------------------------------------------------------------------

/// Parse every entry in a KEYDB file.
pub fn parse_file(path: &Path) -> Result<Vec<KeyEntry>, CoreError> {
    let text = fs::read_to_string(path)?;
    Ok(text.lines().filter_map(parse_line).collect())
}

/// True when the database holds an entry for `disc_id`.
pub fn has_at(path: &Path, disc_id: &str) -> Result<bool, CoreError> {
    let Some(id) = normalize_id(disc_id) else {
        return Ok(false);
    };
    let text = fs::read_to_string(path)?;
    Ok(text.lines().filter_map(parse_line).any(|e| e.id == id))
}

pub fn has(disc_id: &str) -> Result<bool, CoreError> {
    has_at(&keydb_path(), disc_id)
}

pub fn lookup_at(path: &Path, disc_id: &str) -> Result<Option<KeyEntry>, CoreError> {
    let Some(id) = normalize_id(disc_id) else {
        return Ok(None);
    };
    Ok(parse_file(path)?.into_iter().find(|e| e.id == id))
}

pub fn lookup(disc_id: &str) -> Result<Option<KeyEntry>, CoreError> {
    lookup_at(&keydb_path(), disc_id)
}

fn atomic_write(path: &Path, content: &str) -> Result<(), CoreError> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;
    let tmp = path.with_extension("cfg.tmp");
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Insert or replace one entry. Returns `true` when an existing line was
/// replaced, `false` when a new line was appended.
pub fn upsert_at(path: &Path, entry: &KeyEntry) -> Result<bool, CoreError> {
    let date = today();
    let new_line = entry_to_line(entry, &date);
    let text = if path.is_file() {
        fs::read_to_string(path)?
    } else {
        String::new()
    };
    let mut replaced = false;
    let mut out = String::with_capacity(text.len() + new_line.len() + 2);
    for line in text.lines() {
        if !replaced {
            if let Some(e) = parse_line(line) {
                if e.id == entry.id {
                    out.push_str(&new_line);
                    out.push('\n');
                    replaced = true;
                    continue;
                }
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    if !replaced {
        out.push_str(&new_line);
        out.push('\n');
    }
    atomic_write(path, &out)?;
    Ok(replaced)
}

/// Remove the entry for `disc_id`. Returns `true` when a line was removed.
pub fn remove_at(path: &Path, disc_id: &str) -> Result<bool, CoreError> {
    let Some(id) = normalize_id(disc_id) else {
        return Ok(false);
    };
    let text = fs::read_to_string(path)?;
    let mut removed = false;
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        if parse_line(line).map(|e| e.id == id).unwrap_or(false) {
            removed = true;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    if removed {
        atomic_write(path, &out)?;
    }
    Ok(removed)
}

/// Merge entries from another KEYDB file, skipping ids already present.
/// Returns `(added, skipped)`.
pub fn import_at(path: &Path, other: &Path) -> Result<(usize, usize), CoreError> {
    let existing: std::collections::HashSet<String> =
        parse_file(path).map(|v| v.into_iter().map(|e| e.id).collect())?;
    let src = fs::read_to_string(other)?;
    let date = today();
    let mut added = 0usize;
    let mut skipped = 0usize;
    let mut out = if path.is_file() {
        fs::read_to_string(path)?
    } else {
        String::new()
    };
    for line in src.lines() {
        let Some(e) = parse_line(line) else { continue };
        if existing.contains(&e.id) {
            skipped += 1;
            continue;
        }
        out.push_str(&entry_to_line(&e, &date));
        out.push('\n');
        added += 1;
    }
    if added > 0 {
        atomic_write(path, &out)?;
    }
    Ok((added, skipped))
}

/// List entries, newest-first order is not tracked so file order is kept.
pub fn list_at(path: &Path, limit: Option<usize>) -> Result<Vec<KeyEntry>, CoreError> {
    let mut all = parse_file(path)?;
    if let Some(n) = limit {
        all.truncate(n);
    }
    Ok(all)
}

// ---------------------------------------------------------------------------
// Disc id
// ---------------------------------------------------------------------------

/// The AACS disc id: SHA-1 of `AACS/Unit_Key_RO.inf`, lowercase hex.
pub fn disc_id_of(unit_key_file: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    format!("{:x}", Sha1::digest(unit_key_file))
}

/// Disc id for a mounted disc / BDMV folder / its parent, else `None`.
pub fn disc_id_for_path(p: &Path) -> Option<String> {
    let candidates = [
        p.join("AACS").join("Unit_Key_RO.inf"),
        p.join("..").join("AACS").join("Unit_Key_RO.inf"),
    ];
    for c in candidates {
        if let Ok(data) = fs::read(&c) {
            return Some(disc_id_of(&data));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempFile(PathBuf);
    impl TempFile {
        fn new(tag: &str) -> Self {
            let mut p = std::env::temp_dir();
            let n = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            p.push(format!("bluforge-keys-{tag}-{n}.cfg"));
            let _ = fs::remove_file(&p);
            TempFile(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    const ID: &str = "20377624994e9eb5480028798e71ea00ed788f6b";

    #[test]
    fn disc_id_is_sha1_hex() {
        assert_eq!(disc_id_of(b"x").len(), 40);
        assert_ne!(disc_id_of(b"a"), disc_id_of(b"b"));
    }

    #[test]
    fn parse_line_reads_fields_and_note() {
        let line = format!(
            "0x{} = HOUSE_OF_THE_DRAGON | D | 2022-09-28 | M | 0xAA | I | 0xBB | V | 0xCC | U | 1-0xDD ; MKBv77/BEE",
            ID.to_uppercase()
        );
        let e = parse_line(&line).expect("parses");
        assert_eq!(e.id, ID);
        assert_eq!(e.name.as_deref(), Some("HOUSE_OF_THE_DRAGON"));
        assert_eq!(e.media_key.as_deref(), Some("aa"));
        assert_eq!(e.volume_id.as_deref(), Some("bb"));
        assert_eq!(e.vuk.as_deref(), Some("cc"));
        assert_eq!(e.unit_keys, vec!["dd".to_string()]);
        assert_eq!(e.note.as_deref(), Some("MKBv77/BEE"));
    }

    #[test]
    fn parse_line_ignores_non_entries() {
        assert!(parse_line("; a comment").is_none());
        assert!(parse_line("| DK | DEVICE_KEY 0x1234").is_none());
        assert!(parse_line("0xNOPE = x").is_none());
    }

    #[test]
    fn round_trips_through_line() {
        let e = KeyEntry {
            id: ID.to_string(),
            name: Some("Test Disc".into()),
            volume_id: Some("aa".into()),
            vuk: Some("bb".into()),
            unit_keys: vec!["cc".into(), "dd".into()],
            ..Default::default()
        };
        let line = entry_to_line(&e, "2026-11-09");
        let back = parse_line(&line).unwrap();
        assert_eq!(back.vuk.as_deref(), Some("bb"));
        assert_eq!(back.unit_keys.len(), 2);
        assert_eq!(back.name.as_deref(), Some("Test Disc"));
    }

    #[test]
    fn upsert_lookup_remove_round_trip() {
        let f = TempFile::new("crud");
        fs::write(
            f.path(),
            "0x0000000000000000000000000000000000000001 = OTHER | V | 0x11\n",
        )
        .unwrap();
        let e = KeyEntry {
            id: ID.to_string(),
            name: Some("HoD".into()),
            vuk: Some("2ac02bc117595a9142a2cc88d6391815".into()),
            ..Default::default()
        };
        assert!(!upsert_at(f.path(), &e).unwrap()); // appended
        assert!(has_at(f.path(), ID).unwrap());
        assert_eq!(
            lookup_at(f.path(), ID).unwrap().unwrap().vuk.as_deref(),
            Some("2ac02bc117595a9142a2cc88d6391815")
        );
        // replace, not duplicate
        assert!(upsert_at(f.path(), &e).unwrap());
        assert_eq!(parse_file(f.path()).unwrap().len(), 2);
        assert!(remove_at(f.path(), ID).unwrap());
        assert!(!has_at(f.path(), ID).unwrap());
        assert_eq!(parse_file(f.path()).unwrap().len(), 1);
    }

    #[test]
    fn import_skips_existing() {
        let dst = TempFile::new("dst");
        let src = TempFile::new("src");
        fs::write(
            src.path(),
            format!(
                "0x{ID} = NEW | V | 0x22\n0x0000000000000000000000000000000000000002 = TWO | V | 0x33\n"
            ),
        )
        .unwrap();
        fs::write(dst.path(), format!("0x{ID} = OLD | V | 0x11\n")).unwrap();
        let (added, skipped) = import_at(dst.path(), src.path()).unwrap();
        assert_eq!((added, skipped), (1, 1));
        assert_eq!(
            lookup_at(dst.path(), ID).unwrap().unwrap().name.as_deref(),
            Some("OLD")
        );
        assert_eq!(parse_file(dst.path()).unwrap().len(), 2);
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), "1970-01-01");
        assert_eq!(civil_from_days(19_000), "2022-01-08");
    }
}
