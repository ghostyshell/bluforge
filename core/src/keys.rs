//! AACS decryption-key database (`KEYDB.cfg`) management.
//!
//! BluForge keeps a local copy of the community key database and refreshes it
//! from the network on demand (and on launch). The database is consumed by
//! libaacs at rip time; here we only own fetch / store / parse.

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
    ("https://raw.githubusercontent.com/lwjohnst86/KEYDB/master/KEYDB.cfg", false),
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
