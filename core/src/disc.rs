//! Blu-ray disc inspection & decrypted title streaming via libbluray.

use std::ffi::{CStr, CString};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::CoreError;

pub const VERSION: &str = "0.1";

#[cfg(have_bluray)]
mod ffi {
    use libc::{c_char, c_int, c_void};

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct BfTitle {
        pub index: u32,
        pub playlist: u32,
        pub duration_90k: u64,
        pub chapter_count: u32,
        pub clip_count: u32,
        pub video_format: u8,
        pub video_rate: u8,
        pub dynamic_range: u8,
        pub audio_count: u8,
        pub is_main: c_int,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct BfChapter {
        pub start_secs: f64,
        pub duration_90k: u64,
        pub name: [c_char; 128],
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct BfStream {
        pub kind: c_int,
        pub lang: [c_char; 4],
        pub coding_type: u8,
        pub format: u8,
        pub rate: u8,
        pub pid: u16,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct BfClip {
        pub start_90k: u64,
        pub duration_90k: u64,
    }

    extern "C" {
        pub fn bf_open(path: *const c_char, keyfile: *const c_char) -> *mut c_void;
        pub fn bf_close(d: *mut c_void);
        pub fn bf_title_count(d: *mut c_void) -> c_int;
        pub fn bf_main_title(d: *mut c_void) -> c_int;
        pub fn bf_titles(d: *mut c_void, out: *mut BfTitle, max: c_int) -> c_int;
        pub fn bf_disc_name(d: *mut c_void) -> *const c_char;
        pub fn bf_has_aacs(d: *mut c_void) -> c_int;
        pub fn bf_aacs_ready(d: *mut c_void) -> c_int;
        pub fn bf_aacs_error(d: *mut c_void) -> c_int;
        pub fn bf_select_title(d: *mut c_void, title: u32) -> c_int;
        pub fn bf_read(d: *mut c_void, buf: *mut u8, len: c_int) -> c_int;
        pub fn bf_seek(d: *mut c_void, ticks: u64);
        pub fn bf_version() -> *const c_char;
        pub fn bf_export_decrypted(
            d: *mut c_void,
            dest: *const c_char,
            out_files: *mut libc::c_long,
        ) -> c_int;
        pub fn bf_title_chapters(
            d: *mut c_void,
            title: u32,
            out: *mut BfChapter,
            max: c_int,
        ) -> c_int;
        pub fn bf_title_clips(d: *mut c_void, title: u32, out: *mut BfClip, max: c_int) -> c_int;
        pub fn bf_title_streams(
            d: *mut c_void,
            title: u32,
            out: *mut BfStream,
            max: c_int,
        ) -> c_int;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Title {
    pub index: u32,
    pub playlist: u32,
    pub duration_secs: f64,
    pub chapters: u32,
    pub is_main: bool,
    pub resolution: Option<String>,
    pub dynamic_range: Option<String>,
    pub audio_streams: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscInfo {
    pub name: String,
    pub path: String,
    pub aacs: bool,
    pub aacs_ready: bool,
    pub aacs_error: i32,
    pub titles: Vec<Title>,
    pub main_title: Option<u32>,
    pub library: String,
    /// Codec/language summary of the main title, for output-template previews.
    #[serde(default)]
    pub video_codec: Option<String>,
    #[serde(default)]
    pub audio_codec: Option<String>,
    #[serde(default)]
    pub audio_lang: Option<String>,
}

/// One chapter entry mark on a title.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chapter {
    pub start_secs: f64,
    /// Name authored on the disc, when present (usually absent).
    pub name: Option<String>,
}

/// One clip of a title. Chained clips are separate clips, which is how a
/// "play all" TV/anime disc stores its episodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clip {
    /// Offset within the title, seconds.
    pub in_secs: f64,
    pub out_secs: f64,
    pub duration_secs: f64,
}

/// A title stream, used to label exported tracks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamInfo {
    /// "video" | "audio" | "subtitle"
    pub kind: String,
    /// ISO 639-2 code when the disc tags the stream.
    pub lang: Option<String>,
    /// BD coding type (e.g. TrueHD / DTS-HD / PG).
    pub coding_type: u8,
    /// BD format code.
    pub format: u8,
    pub rate: u8,
    /// mpeg-ts PID.
    #[serde(default)]
    pub pid: u16,
}

fn res_label(format: u8) -> Option<String> {
    // bd_video_format_e
    Some(
        match format {
            1 => "480i",
            2 => "576i",
            3 => "480p",
            4 => "1080i",
            5 => "720p",
            6 => "1080p",
            7 => "576p",
            8 => "2160p",
            _ => return None,
        }
        .to_string(),
    )
}

fn dr_label(dr: u8) -> Option<String> {
    match dr {
        1 => Some("HDR10".into()),
        2 => Some("Dolby Vision".into()),
        _ => None,
    }
}

/// AACS/BD+ decrypt backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecryptBackend {
    /// Native Homebrew libaacs (no MakeMKV involved).
    Aacs,
    /// MakeMKV's libmmbd (`libaacs`/`libbdplus` ABI compatible).
    Mmbd,
}

impl DecryptBackend {
    pub fn label(self) -> &'static str {
        match self {
            DecryptBackend::Aacs => "libaacs (native)",
            DecryptBackend::Mmbd => "libmmbd (MakeMKV)",
        }
    }
}

/// An open Blu-ray disc. Cheap to keep open; `send` because libbluray is
/// accessed under our own locking.
pub struct Disc {
    #[cfg(have_bluray)]
    handle: *mut libc::c_void,
}

unsafe impl Send for Disc {}

impl Disc {
    /// Path to Homebrew's libaacs, when installed.
    fn libaacs_path() -> Option<&'static str> {
        ["/opt/homebrew/lib/libaacs", "/usr/local/lib/libaacs"]
            .into_iter()
            .find(|c| Path::new(&format!("{c}.dylib")).is_file())
    }

    /// Path to MakeMKV's libmmbd, when installed.
    fn libmmbd_path() -> Option<&'static str> {
        [
            "/Applications/MakeMKV.app/Contents/lib/libmmbd_new",
            "/Applications/MakeMKV.app/Contents/lib/libmmbd_old",
            "/usr/local/lib/libmmbd",
            "/opt/homebrew/lib/libmmbd",
        ]
        .into_iter()
        .find(|c| Path::new(&format!("{c}.dylib")).is_file())
    }

    /// Disc id of `source`, read from its `AACS/Unit_Key_RO.inf` (following a
    /// `/dev/rdiskN` node to its mount point).
    pub fn disc_id_for_source(source: &Path) -> Option<String> {
        if let Some(id) = crate::keys::disc_id_for_path(source) {
            return Some(id);
        }
        if let Some(s) = source.to_str() {
            if s.starts_with("/dev/") {
                if let Some(mp) = crate::device::mount_point_for_device(s) {
                    return crate::keys::disc_id_for_path(Path::new(&mp));
                }
            }
        }
        None
    }

    fn disc_is_keyed(source: &Path) -> bool {
        Self::disc_id_for_source(source)
            .map(|id| crate::keys::has(&id).unwrap_or(false))
            .unwrap_or(false)
    }

    /// Pick the backend for a rip/export. A keyed disc uses libaacs. An unkeyed
    /// disc first tries to derive the key from the drive itself (native: the
    /// drive's LibreDrive VID read plus the disc's own MKB); if that fails it
    /// falls back to MakeMKV's `discatt.dat` and finally to libmmbd.
    pub fn plan_backend(source: &Path) -> DecryptBackend {
        let backend = Self::resolve_backend_for(Some(source));
        if backend != DecryptBackend::Aacs || Self::disc_is_keyed(source) {
            return backend;
        }
        let mount = Self::aacs_mount_path(source);
        match crate::native::derive_and_store(&mount) {
            Ok((k, added)) => {
                if added {
                    tracing::info!(id = %k.id, "derived disc key natively");
                }
                return DecryptBackend::Aacs;
            }
            Err(e) => tracing::info!("native disc key derivation failed ({e})"),
        }
        if Self::libmmbd_path().is_none() {
            // libaacs can still decrypt if the VID is cached or the drive
            // authenticates on its own; only the extra oracle is unavailable.
            return DecryptBackend::Aacs;
        }
        tracing::info!("trying MakeMKV discatt (VID)");
        match crate::native::from_makemkv_and_store(&mount) {
            Ok((k, added)) => {
                if added {
                    tracing::info!(id = %k.id, "derived disc key via MakeMKV VID");
                }
                DecryptBackend::Aacs
            }
            Err(e2) => {
                tracing::warn!("MakeMKV VID path failed ({e2}); falling back to libmmbd");
                DecryptBackend::Mmbd
            }
        }
    }

    /// Which decrypt backend to use. Native libaacs is preferred; MakeMKV's
    /// libmmbd is only the fallback for a disc we cannot key yet (its id absent
    /// from KEYDB) or when libaacs is not installed. `KEEPSAKE_DECRYPT=aacs`
    /// forces libaacs, `=mmbd` forces libmmbd.
    pub fn resolve_backend_for(source: Option<&Path>) -> DecryptBackend {
        match crate::env_compat("DECRYPT")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "aacs" | "native" | "libaacs" => return DecryptBackend::Aacs,
            "mmbd" | "makemkv" | "libmmbd" => return DecryptBackend::Mmbd,
            _ => {}
        }
        if Self::libaacs_path().is_none() {
            return DecryptBackend::Mmbd;
        }
        if Self::libmmbd_path().is_none() {
            return DecryptBackend::Aacs;
        }
        // Prefer libaacs even for a disc that is not in KEYDB: it can
        // authenticate to the drive and derive the key itself.
        let _ = source;
        DecryptBackend::Aacs
    }

    /// True when `v` is a library path we resolve ourselves, as opposed to one
    /// the user exported by hand.
    fn is_our_lib(v: &str) -> bool {
        const AACS: [&str; 2] = ["/opt/homebrew/lib/libaacs", "/usr/local/lib/libaacs"];
        const MMBD: [&str; 4] = [
            "/Applications/MakeMKV.app/Contents/lib/libmmbd_new",
            "/Applications/MakeMKV.app/Contents/lib/libmmbd_old",
            "/usr/local/lib/libmmbd",
            "/opt/homebrew/lib/libmmbd",
        ];
        const BDPLUS: [&str; 2] = ["/opt/homebrew/lib/libbdplus", "/usr/local/lib/libbdplus"];
        AACS.iter()
            .chain(MMBD.iter())
            .chain(BDPLUS.iter())
            .any(|c| *c == v)
    }

    fn user_set(var: &str) -> bool {
        std::env::var(var)
            .map(|v| !Self::is_our_lib(&v))
            .unwrap_or(false)
    }

    fn apply_env(backend: DecryptBackend) {
        use std::env;

        // Overwrite a path we set earlier (so a backend switch takes effect),
        // but never a path the user exported themselves.
        if !Self::user_set("LIBAACS_PATH") {
            match backend {
                DecryptBackend::Aacs => {
                    if let Some(c) = Self::libaacs_path().or_else(Self::libmmbd_path) {
                        env::set_var("LIBAACS_PATH", c);
                    }
                }
                DecryptBackend::Mmbd => {
                    if let Some(c) = Self::libmmbd_path() {
                        env::set_var("LIBAACS_PATH", c);
                    }
                }
            }
        }

        // libaacs reads KEYDB.cfg from its own config dir, not ours. Point it at
        // the database we maintain so keyed discs decrypt with no proprietary
        // library. BD+ still prefers libmmbd (Homebrew's libbdplus ships no BD+
        // VM config), falling back to Homebrew libbdplus.
        if backend == DecryptBackend::Aacs {
            Self::link_keydb_for_libaacs(&crate::keys::keydb_path());
        }
        if !Self::user_set("LIBBDPLUS_PATH") {
            if let Some(c) = Self::libmmbd_path() {
                env::set_var("LIBBDPLUS_PATH", c);
            } else {
                for c in ["/opt/homebrew/lib/libbdplus", "/usr/local/lib/libbdplus"] {
                    if Path::new(&format!("{c}.dylib")).is_file() {
                        env::set_var("LIBBDPLUS_PATH", c);
                        break;
                    }
                }
            }
        }
    }

    /// Point libbluray at the AACS / BD+ libraries before the first `bd_open`.
    ///
    /// libbluray `dlopen`s these *by name* and appends `.dylib`; Homebrew's
    /// `/opt/homebrew/lib` is not on dyld's search path, so we resolve real
    /// paths and export them. Native libaacs is the default; MakeMKV's libmmbd
    /// (ABI compatible, and the only thing that handles BD+ on this stack) is
    /// used only as a fallback or when forced. Export `LIBAACS_PATH` /
    /// `LIBBDPLUS_PATH` to override either choice.
    pub fn init_env() {
        Self::apply_env(Self::resolve_backend_for(None));
    }

    /// As `init_env`, but selecting the backend from `source` (a keyed disc
    /// uses native libaacs; an unkeyed one falls back to libmmbd).
    pub fn init_env_for(source: &Path) {
        Self::apply_env(Self::resolve_backend_for(Some(source)));
    }

    /// Which AACS / BD+ implementation libbluray will use, for diagnostics.
    pub fn decryption_backend() -> (Option<String>, Option<String>) {
        (
            std::env::var("LIBAACS_PATH").ok(),
            std::env::var("LIBBDPLUS_PATH").ok(),
        )
    }

    /// Point libaacs at our KEYDB.cfg (it only looks inside its own config dir).
    fn link_keydb_for_libaacs(keydb: &Path) {
        use std::path::PathBuf;
        if !keydb.is_file() {
            return;
        }
        let dir = if cfg!(target_os = "macos") {
            std::env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join("Library/Preferences/aacs"))
        } else {
            std::env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join(".config/aacs"))
        };
        let Some(dir) = dir else { return };
        let link = dir.join("KEYDB.cfg");
        if std::fs::read_link(&link).ok().as_deref() == Some(keydb) {
            return;
        }
        if link.exists() {
            // Do not clobber a user-managed database.
            return;
        }
        if std::fs::create_dir_all(&dir).is_ok() {
            #[cfg(unix)]
            let _ = std::os::unix::fs::symlink(keydb, &link);
        }
    }

    /// For the libaacs backend: a *mounted* path to the volume. libaacs resolves
    /// the BSD device from statfs and then unmounts/remounts it itself, so it
    /// must be handed a mounted path, not `/dev/rdiskN`. Mounts it if needed.
    pub fn aacs_mount_path(path: &Path) -> PathBuf {
        let s = path.to_string_lossy();
        if !s.starts_with("/dev/") {
            return path.to_path_buf();
        }
        let Some(bsd) = bsd_name_for(path) else {
            return path.to_path_buf();
        };
        let cands = [s.to_string(), format!("/dev/{bsd}"), format!("/dev/r{bsd}")];
        if let Some(mp) = cands
            .iter()
            .find_map(|c| crate::device::mount_point_for_device(c))
        {
            return PathBuf::from(mp);
        }
        let _ = std::process::Command::new("diskutil")
            .arg("mount")
            .arg(format!("/dev/{bsd}"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        std::thread::sleep(std::time::Duration::from_millis(500));
        cands
            .iter()
            .find_map(|c| crate::device::mount_point_for_device(c))
            .map(PathBuf::from)
            .unwrap_or_else(|| path.to_path_buf())
    }

    /// Open a disc at `path` (a mounted volume, an ISO, or a `/dev/rdiskX`).
    /// `keydb` points at a KEYDB.cfg consumed by libaacs.
    pub fn open(path: &Path, keydb: Option<&Path>) -> Result<Disc, CoreError> {
        let backend = Self::resolve_backend_for(Some(path));
        Self::open_with_backend(path, keydb, backend)
    }

    /// As `open`, with an explicit backend. Used when the caller already chose
    /// (eg. after unmounting, where auto-detection can no longer see the mount).
    pub fn open_with_backend(
        path: &Path,
        keydb: Option<&Path>,
        backend: DecryptBackend,
    ) -> Result<Disc, CoreError> {
        Self::apply_env(backend);
        let open_path = if backend == DecryptBackend::Aacs {
            Self::aacs_mount_path(path)
        } else {
            path.to_path_buf()
        };
        #[cfg(have_bluray)]
        {
            let cpath = CString::new(open_path.to_string_lossy().as_bytes())
                .map_err(|e| CoreError::Disc(e.to_string()))?;
            let ckey = match keydb {
                Some(k) => Some(
                    CString::new(k.to_string_lossy().as_bytes())
                        .map_err(|e| CoreError::Disc(e.to_string()))?,
                ),
                None => None,
            };
            let handle = unsafe {
                ffi::bf_open(
                    cpath.as_ptr(),
                    ckey.as_ref()
                        .map(|c| c.as_ptr())
                        .unwrap_or(std::ptr::null()),
                )
            };
            if handle.is_null() {
                return Err(CoreError::Disc(format!(
                    "libbluray could not open {}",
                    path.display()
                )));
            }
            Ok(Disc { handle })
        }
        #[cfg(not(have_bluray))]
        {
            let _ = (path, keydb);
            Err(CoreError::Disc("built without libbluray".into()))
        }
    }

    pub fn info(&self) -> Result<DiscInfo, CoreError> {
        #[cfg(have_bluray)]
        unsafe {
            let name = CStr::from_ptr(ffi::bf_disc_name(self.handle))
                .to_string_lossy()
                .into_owned();
            let count = ffi::bf_title_count(self.handle).max(0);
            let main = ffi::bf_main_title(self.handle);
            let mut raw = vec![
                ffi::BfTitle {
                    index: 0,
                    playlist: 0,
                    duration_90k: 0,
                    chapter_count: 0,
                    clip_count: 0,
                    video_format: 0,
                    video_rate: 0,
                    dynamic_range: 0,
                    audio_count: 0,
                    is_main: 0,
                };
                count as usize
            ];
            let n = ffi::bf_titles(self.handle, raw.as_mut_ptr(), count);
            raw.truncate(n.max(0) as usize);
            let titles: Vec<Title> = raw
                .into_iter()
                .map(|t| Title {
                    index: t.index,
                    playlist: t.playlist,
                    duration_secs: t.duration_90k as f64 / 90000.0,
                    chapters: t.chapter_count,
                    is_main: t.is_main != 0,
                    resolution: res_label(t.video_format),
                    dynamic_range: dr_label(t.dynamic_range),
                    audio_streams: t.audio_count as u32,
                })
                .collect();
            let main_t = if main >= 0 { Some(main as u32) } else { None };
            let streams = main_t
                .and_then(|m| self.streams(m).ok())
                .unwrap_or_default();
            let vid = streams.iter().find(|s| s.kind == "video");
            let aud = streams.iter().find(|s| s.kind == "audio");
            Ok(DiscInfo {
                video_codec: vid.map(|s| crate::rip::video_codec_label(s.coding_type).to_string()),
                audio_codec: aud.map(|s| crate::rip::audio_codec_label(s.coding_type).to_string()),
                audio_lang: aud
                    .and_then(|s| s.lang.as_deref())
                    .map(crate::rip::lang_name),
                name,
                path: String::new(),
                aacs: ffi::bf_has_aacs(self.handle) != 0,
                aacs_ready: ffi::bf_aacs_ready(self.handle) != 0,
                aacs_error: ffi::bf_aacs_error(self.handle),
                main_title: if main >= 0 { Some(main as u32) } else { None },
                titles,
                library: library_version(),
            })
        }
        #[cfg(not(have_bluray))]
        {
            Err(CoreError::Disc("built without libbluray".into()))
        }
    }

    /// Chapter entry marks of `title`. Times are authoritative (from the
    /// playlist); names are only present when the disc authors them.
    pub fn chapters(&self, title: u32) -> Result<Vec<Chapter>, CoreError> {
        #[cfg(have_bluray)]
        unsafe {
            let empty = ffi::BfChapter {
                start_secs: 0.0,
                duration_90k: 0,
                name: [0; 128],
            };
            let mut buf = vec![empty; 512];
            let n = ffi::bf_title_chapters(
                self.handle,
                title,
                buf.as_mut_ptr(),
                buf.len() as libc::c_int,
            );
            if n < 0 {
                return Err(CoreError::Disc("cannot read chapter marks".into()));
            }
            Ok(buf[..n as usize]
                .iter()
                .map(|c| {
                    let bytes: Vec<u8> = c
                        .name
                        .iter()
                        .take_while(|ch| **ch != 0)
                        .map(|ch| *ch as u8)
                        .collect();
                    let name = String::from_utf8_lossy(&bytes).trim().to_string();
                    Chapter {
                        start_secs: c.start_secs,
                        name: if name.is_empty() { None } else { Some(name) },
                    }
                })
                .collect())
        }
        #[cfg(not(have_bluray))]
        {
            let _ = title;
            Err(CoreError::Disc("built without libbluray".into()))
        }
    }

    /// Clip boundaries of `title`. On a "play all" TV/anime disc each clip is
    /// one episode, which is the only reliable episode delimiter.
    pub fn clips(&self, title: u32) -> Result<Vec<Clip>, CoreError> {
        #[cfg(have_bluray)]
        unsafe {
            let empty = ffi::BfClip {
                start_90k: 0,
                duration_90k: 0,
            };
            let mut buf = vec![empty; 512];
            let n = ffi::bf_title_clips(
                self.handle,
                title,
                buf.as_mut_ptr(),
                buf.len() as libc::c_int,
            );
            if n < 0 {
                return Err(CoreError::Disc("cannot read clip info".into()));
            }
            let first = buf.first().map(|c| c.start_90k).unwrap_or(0);
            Ok(buf[..n as usize]
                .iter()
                .map(|c| Clip {
                    in_secs: c.start_90k.saturating_sub(first) as f64 / 90000.0,
                    out_secs: c.start_90k.saturating_sub(first) as f64 / 90000.0
                        + c.duration_90k as f64 / 90000.0,
                    duration_secs: c.duration_90k as f64 / 90000.0,
                })
                .collect())
        }
        #[cfg(not(have_bluray))]
        {
            let _ = title;
            Err(CoreError::Disc("built without libbluray".into()))
        }
    }

    /// Video / audio / PG-subtitle streams of `title`, in disc order.
    pub fn streams(&self, title: u32) -> Result<Vec<StreamInfo>, CoreError> {
        #[cfg(have_bluray)]
        unsafe {
            let empty = ffi::BfStream {
                kind: 0,
                lang: [0; 4],
                coding_type: 0,
                format: 0,
                rate: 0,
                pid: 0,
            };
            let mut buf = vec![empty; 64];
            let n = ffi::bf_title_streams(
                self.handle,
                title,
                buf.as_mut_ptr(),
                buf.len() as libc::c_int,
            );
            if n < 0 {
                return Err(CoreError::Disc("cannot read stream info".into()));
            }
            Ok(buf[..n as usize]
                .iter()
                .map(|s| {
                    let lang: String = s
                        .lang
                        .iter()
                        .take_while(|c| **c != 0)
                        .map(|c| *c as u8 as char)
                        .collect();
                    StreamInfo {
                        kind: match s.kind {
                            0 => "video",
                            1 => "audio",
                            _ => "subtitle",
                        }
                        .into(),
                        lang: if lang.trim().is_empty() {
                            None
                        } else {
                            Some(lang)
                        },
                        coding_type: s.coding_type,
                        format: s.format,
                        rate: s.rate,
                        pid: s.pid,
                    }
                })
                .collect())
        }
        #[cfg(not(have_bluray))]
        {
            let _ = title;
            Err(CoreError::Disc("built without libbluray".into()))
        }
    }

    pub fn select(&self, title: u32) -> Result<(), CoreError> {
        #[cfg(have_bluray)]
        unsafe {
            if ffi::bf_select_title(self.handle, title) == 0 {
                return Err(CoreError::Disc(format!("cannot select title {title}")));
            }
            Ok(())
        }
        #[cfg(not(have_bluray))]
        {
            let _ = title;
            Err(CoreError::Disc("built without libbluray".into()))
        }
    }

    /// Seek the selected title to `secs` into its own timeline (90 kHz units in
    /// libbluray). Used to jump straight to an episode instead of reading the
    /// whole play-all title from the start.
    pub fn seek(&self, secs: f64) -> Result<(), CoreError> {
        #[cfg(have_bluray)]
        {
            if self.handle.is_null() || secs <= 0.0 {
                return Ok(());
            }
            let ticks = (secs * 90_000.0).round() as u64;
            unsafe { ffi::bf_seek(self.handle, ticks) };
            Ok(())
        }
        #[cfg(not(have_bluray))]
        {
            let _ = secs;
            Err(CoreError::Disc("built without libbluray".into()))
        }
    }

    /// Export the full disc tree to `dest` with all encrypted streams decrypted
    /// (for building a decrypted ISO / backup folder). Returns the file count.
    pub fn export_decrypted(&self, dest: &Path) -> Result<u64, CoreError> {
        #[cfg(have_bluray)]
        unsafe {
            let cdest = CString::new(dest.to_string_lossy().as_bytes())
                .map_err(|e| CoreError::Disc(e.to_string()))?;
            let mut files: libc::c_long = 0;
            let rc = ffi::bf_export_decrypted(self.handle, cdest.as_ptr(), &mut files);
            if rc != 0 {
                return Err(CoreError::Disc("failed to export decrypted tree".into()));
            }
            Ok(files.max(0) as u64)
        }
        #[cfg(not(have_bluray))]
        {
            let _ = dest;
            Err(CoreError::Disc("built without libbluray".into()))
        }
    }

    /// Read decrypted MPEG-TS bytes from the currently selected title.
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, CoreError> {
        #[cfg(have_bluray)]
        unsafe {
            let n = ffi::bf_read(self.handle, buf.as_mut_ptr(), buf.len() as libc::c_int);
            if n < 0 {
                Err(CoreError::Disc("libbluray read error".into()))
            } else {
                Ok(n as usize)
            }
        }
        #[cfg(not(have_bluray))]
        {
            let _ = buf;
            Err(CoreError::Disc("built without libbluray".into()))
        }
    }
}

impl Drop for Disc {
    fn drop(&mut self) {
        #[cfg(have_bluray)]
        unsafe {
            ffi::bf_close(self.handle);
        }
    }
}

pub fn library_version() -> String {
    #[cfg(have_bluray)]
    unsafe {
        CStr::from_ptr(ffi::bf_version())
            .to_string_lossy()
            .into_owned()
    }
    #[cfg(not(have_bluray))]
    {
        "none".into()
    }
}

/// Read the 16-byte AACS volume ID straight from the drive on macOS.
///
/// libaacs needs it to derive the volume-unique key, but its own macOS MMC layer
/// cannot find the drive on this stack, so we ask the drive ourselves. A
/// LibreDrive drive hands the decrypted VID over a vendor READ BUFFER with no
/// authentication at all; otherwise fall back to the standard unauthenticated
/// READ DISC STRUCTURE (0x80), which a revoked-certificate drive rejects.
#[cfg(target_os = "macos")]
pub fn aacs_volume_id(device: &Path) -> Option<[u8; 16]> {
    extern "C" {
        fn bf_aacs_volume_id(
            name: *const std::os::raw::c_char,
            out: *mut u8,
        ) -> std::os::raw::c_int;
        fn bf_aacs_vid_libredrive(
            name: *const std::os::raw::c_char,
            out: *mut u8,
        ) -> std::os::raw::c_int;
    }
    let name = device
        .file_name()?
        .to_string_lossy()
        .trim_start_matches('r')
        .to_string();
    let cname = std::ffi::CString::new(name).ok()?;
    let mut out = [0u8; 16];
    let rc = unsafe { bf_aacs_vid_libredrive(cname.as_ptr(), out.as_mut_ptr()) };
    if rc == 0 {
        return Some(out);
    }
    let rc = unsafe { bf_aacs_volume_id(cname.as_ptr(), out.as_mut_ptr()) };
    if rc == 0 {
        Some(out)
    } else {
        tracing::warn!(rc, "aacs: could not read the volume id");
        None
    }
}

#[cfg(not(target_os = "macos"))]
pub fn aacs_volume_id(_device: &Path) -> Option<[u8; 16]> {
    None
}

/// Seed libaacs's volume-ID cache: `~/Library/Caches/aacs/vid/<sha1(Unit_Key_RO.inf)>`.
pub fn aacs_cache_volume_id(disc_root: &Path, vid: &[u8; 16]) -> bool {
    let ukf = disc_root.join("AACS").join("Unit_Key_RO.inf");
    let Ok(data) = std::fs::read(&ukf) else {
        tracing::warn!(path = %ukf.display(), "aacs: no unit key file, cannot compute the disc id");
        return false;
    };
    aacs_cache_volume_id_bytes(&data, vid)
}

/// Cache by unit-key-file content (the disc id is SHA-1 of that file).
pub fn aacs_cache_volume_id_bytes(unit_key_file: &[u8], vid: &[u8; 16]) -> bool {
    use sha1::{Digest, Sha1};
    let id = format!("{:x}", Sha1::digest(unit_key_file));
    let dir = if cfg!(target_os = "macos") {
        std::env::var("HOME")
            .ok()
            .map(|h| std::path::PathBuf::from(h).join("Library/Caches/aacs/vid"))
    } else {
        std::env::var("HOME")
            .ok()
            .map(|h| std::path::PathBuf::from(h).join(".cache/aacs/vid"))
    };
    let Some(dir) = dir else { return false };
    if std::fs::create_dir_all(&dir).is_err() {
        return false;
    }
    match std::fs::write(dir.join(&id), vid) {
        Ok(()) => {
            tracing::info!(disc_id = %id, "aacs: cached the volume id for libaacs");
            true
        }
        Err(e) => {
            tracing::warn!(%e, "aacs: cannot write the volume id cache");
            false
        }
    }
}

/// Unmount the volume so the drive can be opened exclusively for a SCSI command.
pub fn aacs_unmount(device: &Path) -> bool {
    std::process::Command::new("diskutil")
        .arg("unmountDisk")
        .arg("force")
        .arg(device)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Whole-disk BSD name behind `source`: a `/dev/diskN` / `/dev/rdiskN` node, or
/// the device backing a mounted optical volume.
pub fn bsd_name_for(source: &Path) -> Option<String> {
    let s = source.to_string_lossy();
    if let Some(rest) = s.strip_prefix("/dev/") {
        let rest = rest.strip_prefix('r').unwrap_or(rest);
        return if rest.starts_with("disk") {
            Some(rest.to_string())
        } else {
            None
        };
    }
    crate::device::list_optical()
        .into_iter()
        .find(|d| d.mount_point.as_deref() == Some(s.as_ref()))
        .map(|d| d.bsd_name)
}

/// The raw whole-disk node for an optical `source`, or `None` when `source` is
/// a plain file/folder (ISO, BDMV dir). A mounted volume hands back the still
/// AACS-encrypted bytes, so the device is required.
pub fn raw_device_for(source: &Path) -> Option<PathBuf> {
    let s = source.to_string_lossy();
    if s.starts_with("/dev/rdisk") {
        return Some(source.to_path_buf());
    }
    bsd_name_for(source).map(|b| PathBuf::from(format!("/dev/r{b}")))
}

/// Prepare a `source` for reading: prefer the raw device. `unmount` is best
/// unmount the volume first (native libaacs cannot decrypt while the volume is
/// mounted). Returns the source to open and, if it needs restoring, the device
/// to remount afterwards.
pub fn prepare_source(source: &Path, unmount: bool) -> (PathBuf, Option<PathBuf>) {
    let Some(raw) = raw_device_for(source) else {
        return (source.to_path_buf(), None);
    };
    if unmount {
        if let Some(bsd) = bsd_name_for(source) {
            let _ = aacs_unmount(&PathBuf::from(format!("/dev/{bsd}")));
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
    }
    tracing::info!(source = %source.display(), raw = %raw.display(), unmount, "prepare source for decrypt");
    (raw.clone(), if unmount { Some(raw) } else { None })
}

/// Best-effort remount of a device returned by `prepare_source`.
pub fn remount(device: &Path) {
    if let Some(bsd) = bsd_name_for(device) {
        let _ = std::process::Command::new("diskutil")
            .arg("mountDisk")
            .arg(format!("/dev/{bsd}"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

/// Remounts a device on drop, so a rip that unmounted a volume puts it back.
pub struct RemountGuard(pub Option<PathBuf>);

impl Drop for RemountGuard {
    fn drop(&mut self) {
        if let Some(d) = &self.0 {
            remount(d);
        }
    }
}

pub struct TitleReader<'a> {
    disc: &'a Disc,
    buf: Vec<u8>,
    pos: usize,
    filled: usize,
    done: bool,
}

impl<'a> TitleReader<'a> {
    pub fn new(disc: &'a Disc) -> Self {
        Self {
            disc,
            buf: vec![0u8; 6144 * 64],
            pos: 0,
            filled: 0,
            done: false,
        }
    }
}

impl Read for TitleReader<'_> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.filled {
            if self.done {
                return Ok(0);
            }
            let n = self
                .disc
                .read(&mut self.buf)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            if n == 0 {
                self.done = true;
                return Ok(0);
            }
            self.pos = 0;
            self.filled = n;
        }
        let n = (self.filled - self.pos).min(out.len());
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}
