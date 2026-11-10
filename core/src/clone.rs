//! 1:1 disc clone: copy the raw sectors of an optical disc into an ISO image.
//!
//! Unlike `iso::run` (which reads a decrypted, libbluray-interpreted stream and
//! writes a clean UDF image), this is a bit-for-bit sector copy. AACS/BD+
//! encryption is preserved, which is what "clone" means in this class of app:
//! the result is an archival image that only a decrypting player can play, and
//! it is the input a burner would need.
//!
//! The source is normally `/dev/rdiskN`. Any readable file works too, which is
//! how the unit test covers the copy loop without a drive.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::CoreError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneRequest {
    /// Raw device (`/dev/rdiskN`) or a readable file.
    pub source: PathBuf,
    /// Image to write. Existing files are overwritten.
    pub dest: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CloneProgress {
    pub percent: f64,
    pub bytes: u64,
    /// Zero when the media size cannot be determined (progress is then unknown).
    pub total: u64,
    pub message: String,
    pub done: bool,
}

/// Media size in bytes. Block/character devices report zero through `stat`, so
/// ask the driver; regular files just use their length.
#[cfg(target_os = "macos")]
fn media_size(file: &std::fs::File) -> Option<u64> {
    use std::os::unix::io::AsRawFd;

    // <sys/disk.h>: _IOR('d', 24, uint32_t), _IOR('d', 25, uint64_t)
    const DKIOCGETBLOCKSIZE: libc::c_ulong = 0x4004_6418;
    const DKIOCGETBLOCKCOUNT: libc::c_ulong = 0x4008_6419;

    let fd = file.as_raw_fd();
    let mut block_size: u32 = 0;
    let mut block_count: u64 = 0;
    unsafe {
        if libc::ioctl(fd, DKIOCGETBLOCKSIZE, &mut block_size) != 0 || block_size == 0 {
            return None;
        }
        if libc::ioctl(fd, DKIOCGETBLOCKCOUNT, &mut block_count) != 0 || block_count == 0 {
            return None;
        }
    }
    Some(block_size as u64 * block_count)
}

#[cfg(not(target_os = "macos"))]
fn media_size(_file: &std::fs::File) -> Option<u64> {
    None
}

/// Copy `req.source` to `req.dest` in 4 MiB chunks. Returns the byte count.
///
/// A read error stops the copy and reports the offset reached, because an
/// optical drive that drops off the bus mid-clone leaves a truncated image that
/// must not look like a success.
pub fn run<F>(req: &CloneRequest, cancel: &AtomicBool, mut progress: F) -> Result<u64, CoreError>
where
    F: FnMut(CloneProgress),
{
    let mut source = std::fs::File::open(&req.source)?;
    let meta = source.metadata()?;
    let total = if meta.is_file() {
        meta.len()
    } else {
        media_size(&source).unwrap_or(0)
    };

    // Do not pull gigabytes of disc data through the unified buffer cache.
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::io::AsRawFd;
        const F_NOCACHE: libc::c_int = 48;
        unsafe {
            libc::fcntl(source.as_raw_fd(), F_NOCACHE, 1);
        }
    }

    if let Some(parent) = req.dest.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let mut dest = std::fs::File::create(&req.dest)?;

    let mut buf = vec![0u8; 4 << 20];
    let mut copied: u64 = 0;
    let mut reported = f64::NEG_INFINITY;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = std::fs::remove_file(&req.dest);
            return Err(CoreError::Disc("clone cancelled".into()));
        }
        let n = match source.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                return Err(CoreError::Disc(format!(
                    "read failed after {copied} bytes: {e}"
                )))
            }
        };
        dest.write_all(&buf[..n])?;
        copied += n as u64;
        let percent = if total > 0 {
            (copied as f64 / total as f64 * 100.0).min(100.0)
        } else {
            0.0
        };
        if percent - reported >= 0.5 || total == 0 {
            reported = percent;
            progress(CloneProgress {
                percent,
                bytes: copied,
                total,
                message: format!("{} of {}", human(copied), human(total)),
                done: false,
            });
        }
    }
    dest.sync_all()?;
    progress(CloneProgress {
        percent: 100.0,
        bytes: copied,
        total: copied,
        message: format!("wrote {}", human(copied)),
        done: true,
    });
    Ok(copied)
}

fn human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{} {}", bytes, UNITS[i])
    } else {
        format!("{v:.2} {}", UNITS[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("keepsake-clone-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn copies_every_byte_and_reports_progress() {
        let dir = temp_dir("copy");
        let source = dir.join("source.bin");
        let dest = dir.join("out/image.iso");
        let data: Vec<u8> = (0..(3 * 1024 * 1024 + 123))
            .map(|i| (i % 251) as u8)
            .collect();
        std::fs::write(&source, &data).unwrap();

        let mut last = CloneProgress::default();
        let mut calls = 0;
        let copied = run(
            &CloneRequest {
                source: source.clone(),
                dest: dest.clone(),
            },
            &AtomicBool::new(false),
            |p| {
                calls += 1;
                last = p;
            },
        )
        .unwrap();

        assert_eq!(copied as usize, data.len());
        assert_eq!(std::fs::read(&dest).unwrap(), data);
        assert!(calls >= 1);
        assert!(last.done);
        assert_eq!(last.percent, 100.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancellation_removes_the_partial_image() {
        let dir = temp_dir("cancel");
        let source = dir.join("source.bin");
        let dest = dir.join("out.iso");
        std::fs::write(&source, vec![7u8; 1024]).unwrap();
        let cancel = AtomicBool::new(true);
        let err = run(
            &CloneRequest {
                source: source.clone(),
                dest: dest.clone(),
            },
            &cancel,
            |_| {},
        )
        .unwrap_err();
        assert!(err.to_string().contains("cancelled"));
        assert!(!dest.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
