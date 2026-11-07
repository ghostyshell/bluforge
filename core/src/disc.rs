//! Blu-ray disc inspection & decrypted title streaming via libbluray.

use std::ffi::{CStr, CString};
use std::io::Read;
use std::path::Path;

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
        pub fn bf_version() -> *const c_char;
        pub fn bf_export_decrypted(d: *mut c_void, dest: *const c_char, out_files: *mut libc::c_long) -> c_int;
        pub fn bf_title_chapters(d: *mut c_void, title: u32, out: *mut BfChapter, max: c_int) -> c_int;
        pub fn bf_title_streams(d: *mut c_void, title: u32, out: *mut BfStream, max: c_int) -> c_int;
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
}

/// One chapter entry mark on a title.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chapter {
    pub start_secs: f64,
    /// Name authored on the disc, when present (usually absent).
    pub name: Option<String>,
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

/// An open Blu-ray disc. Cheap to keep open; `send` because libbluray is
/// accessed under our own locking.
pub struct Disc {
    #[cfg(have_bluray)]
    handle: *mut libc::c_void,
}

unsafe impl Send for Disc {}

impl Disc {
    /// Point libbluray at the AACS / BD+ libraries before the first `bd_open`.
    ///
    /// libbluray `dlopen`s these *by name* and appends `.dylib`; Homebrew's
    /// `/opt/homebrew/lib` is not on dyld's search path, so we resolve real
    /// paths and export them. When MakeMKV is installed its `libmmbd` is
    /// preferred for **both** AACS and BD+: it is libaacs/libbdplus ABI
    /// compatible, does BD+ natively, and handles LibreDrive plus AACS bus
    /// encryption (which plain libaacs cannot do on drives that enforce
    /// revocation). Homebrew's libbdplus also ships without a BD+ VM config,
    /// so libmmbd additionally covers that. Export `LIBAACS_PATH` /
    /// `LIBBDPLUS_PATH` to override either choice.
    pub fn init_env() {
        use std::env;
        use std::path::Path;

        let have = |p: &str| Path::new(p).is_file();

        let mmbd = [
            "/Applications/MakeMKV.app/Contents/lib/libmmbd_new",
            "/Applications/MakeMKV.app/Contents/lib/libmmbd_old",
            "/usr/local/lib/libmmbd",
            "/opt/homebrew/lib/libmmbd",
        ]
        .into_iter()
        .find(|c| have(&format!("{c}.dylib")));

        if env::var_os("LIBAACS_PATH").is_none() {
            match mmbd {
                Some(c) => env::set_var("LIBAACS_PATH", c),
                None => {
                    for c in ["/opt/homebrew/lib/libaacs", "/usr/local/lib/libaacs"] {
                        if have(&format!("{c}.dylib")) {
                            env::set_var("LIBAACS_PATH", c);
                            break;
                        }
                    }
                }
            }
        }

        if env::var_os("LIBBDPLUS_PATH").is_none() {
            match mmbd {
                Some(c) => env::set_var("LIBBDPLUS_PATH", c),
                None => {
                    for c in ["/opt/homebrew/lib/libbdplus", "/usr/local/lib/libbdplus"] {
                        if have(&format!("{c}.dylib")) {
                            env::set_var("LIBBDPLUS_PATH", c);
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Which AACS / BD+ implementation libbluray will use, for diagnostics.
    pub fn decryption_backend() -> (Option<String>, Option<String>) {
        (
            std::env::var("LIBAACS_PATH").ok(),
            std::env::var("LIBBDPLUS_PATH").ok(),
        )
    }

    /// Open a disc at `path` (a mounted volume, an ISO, or a `/dev/rdiskX`).
    /// `keydb` points at a KEYDB.cfg consumed by libaacs.
    pub fn open(path: &Path, keydb: Option<&Path>) -> Result<Disc, CoreError> {
        Self::init_env();
        #[cfg(have_bluray)]
        {
            let cpath = CString::new(path.to_string_lossy().as_bytes())
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
                    ckey.as_ref().map(|c| c.as_ptr()).unwrap_or(std::ptr::null()),
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
            Ok(DiscInfo {
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
            let empty = ffi::BfChapter { start_secs: 0.0, duration_90k: 0, name: [0; 128] };
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

    /// Video / audio / PG-subtitle streams of `title`, in disc order.
    pub fn streams(&self, title: u32) -> Result<Vec<StreamInfo>, CoreError> {
        #[cfg(have_bluray)]
        unsafe {
            let empty = ffi::BfStream { kind: 0, lang: [0; 4], coding_type: 0, format: 0, rate: 0 };
            let mut buf = vec![empty; 64];
            let n =
                ffi::bf_title_streams(self.handle, title, buf.as_mut_ptr(), buf.len() as libc::c_int);
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
                        lang: if lang.trim().is_empty() { None } else { Some(lang) },
                        coding_type: s.coding_type,
                        format: s.format,
                        rate: s.rate,
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

    pub fn select(&self, title: u32) -> Result<(), CoreError> {        #[cfg(have_bluray)]
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
        CStr::from_ptr(ffi::bf_version()).to_string_lossy().into_owned()
    }
    #[cfg(not(have_bluray))]
    {
        "none".into()
    }
}

/// `Read` adapter over a selected title (yields decrypted MPEG-TS).
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
