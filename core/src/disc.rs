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

fn res_label(format: u8) -> Option<String> {
    // bd_video_format_e
    Some(
        match format {
            1 => "480p",
            2 => "576p",
            3 => "720p",
            4 => "1080p",
            5 => "2160p",
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
    /// Open a disc at `path` (a mounted volume, an ISO, or a `/dev/rdiskX`).
    /// `keydb` points at a KEYDB.cfg consumed by libaacs.
    pub fn open(path: &Path, keydb: Option<&Path>) -> Result<Disc, CoreError> {
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
