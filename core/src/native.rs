//! Native AACS key recovery: authenticate to the drive and derive a disc's key
//! from its own MKB, with no MakeMKV and no pre-existing KEYDB entry.
//!
//! The heavy lifting (AES/AES-G3/ECDSA, the MKB record parser, the MMC auth
//! sequence) lives in `aacs_native.c`, which links libaacs's internal helpers.
//! Here we supply the host certificate and device keys from `KEYDB.cfg`, read
//! the disc's MKB, and turn the result into a KEYDB entry.

use std::path::Path;

use crate::keys::{self, KeyEntry};
use crate::CoreError;

#[repr(C)]
struct BfDk {
    key: [u8; 16],
    node: u32,
    uv: u32,
    u_shift: u8,
}

extern "C" {
    fn bf_native_key(
        mount: *const libc::c_char,
        host_priv: *const u8,
        host_cert: *const u8,
        mkb: *const u8,
        mkb_len: usize,
        dks: *const BfDk,
        ndks: i32,
        out_vid: *mut u8,
        out_mk: *mut u8,
        out_vuk: *mut u8,
    ) -> i32;
}

/// A key recovered from the drive/disc itself.
#[derive(Debug, Clone, serde::Serialize)]
pub struct NativeKey {
    pub id: String,
    pub name: String,
    pub vid: String,
    pub media_key: String,
    pub vuk: String,
}

fn hexbytes(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    let t = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    let t: String = t.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if t.is_empty() || t.len() % 2 != 0 {
        return None;
    }
    (0..t.len() / 2)
        .map(|i| u8::from_str_radix(&t[i * 2..i * 2 + 2], 16).ok())
        .collect()
}

fn hexstr(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Value of `KEY` in a `| TAG | KEY 0x.. | ...` KEYDB header line.
fn field(line: &str, key: &str) -> Option<String> {
    let toks: Vec<&str> = line
        .split(|c: char| c == '|' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .collect();
    toks.iter()
        .position(|t| *t == key)
        .and_then(|i| toks.get(i + 1).map(|s| s.to_string()))
}

fn as_u32(s: &str) -> Option<u32> {
    let s = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    u32::from_str_radix(s, 16).ok()
}

struct Header {
    host_priv: Vec<u8>,
    host_cert: Vec<u8>,
    dks: Vec<BfDk>,
}

/// Host certificate and device keys from the KEYDB header (`| HC |`, `| DK |`).
fn read_header(keydb: &Path) -> Result<Header, CoreError> {
    let text = std::fs::read_to_string(keydb)
        .map_err(|e| CoreError::Keys(format!("{}: {e}", keydb.display())))?;

    let mut host_priv = None;
    let mut host_cert = None;
    let mut dks = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("| HC") {
            if let Some(v) = field(line, "HOST_PRIV_KEY") {
                host_priv = hexbytes(&v);
            }
            if let Some(v) = field(line, "HOST_CERT") {
                host_cert = hexbytes(&v);
            }
        } else if line.starts_with("| DK") {
            let (Some(k), Some(n), Some(uv), Some(sh)) = (
                field(line, "DEVICE_KEY").and_then(|v| hexbytes(&v)),
                field(line, "DEVICE_NODE").and_then(|v| as_u32(&v)),
                field(line, "KEY_UV").and_then(|v| as_u32(&v)),
                field(line, "KEY_U_MASK_SHIFT").and_then(|v| as_u32(&v)),
            ) else {
                continue;
            };
            if k.len() != 16 {
                continue;
            }
            let mut key = [0u8; 16];
            key.copy_from_slice(&k);
            dks.push(BfDk {
                key,
                node: n,
                uv,
                u_shift: sh as u8,
            });
        }
    }

    let host_priv =
        host_priv.ok_or_else(|| CoreError::Keys("KEYDB has no host certificate".into()))?;
    let host_cert =
        host_cert.ok_or_else(|| CoreError::Keys("KEYDB has no host certificate".into()))?;
    if dks.is_empty() {
        return Err(CoreError::Keys("KEYDB has no device keys".into()));
    }
    Ok(Header {
        host_priv,
        host_cert,
        dks,
    })
}

fn read_mkb(mount: &Path) -> Result<Vec<u8>, CoreError> {
    for rel in ["AACS/MKB_RO.inf", "AACS/DUPLICATE/MKB_RO.inf"] {
        let p = mount.join(rel);
        if let Ok(b) = std::fs::read(&p) {
            return Ok(b);
        }
    }
    Err(CoreError::Disc(format!(
        "no AACS/MKB_RO.inf under {}",
        mount.display()
    )))
}

/// Derive the disc key at `mount` (a mounted volume) natively.
pub fn derive(mount: &Path) -> Result<NativeKey, CoreError> {
    let keydb = keys::keydb_path();
    let header = read_header(&keydb)?;
    let mkb = read_mkb(mount)?;

    let id = keys::disc_id_for_path(mount).ok_or_else(|| {
        CoreError::Disc(format!("no AACS/Unit_Key_RO.inf under {}", mount.display()))
    })?;

    let bsd = crate::disc::bsd_name_for(mount).ok_or_else(|| {
        CoreError::Disc(format!(
            "cannot resolve the optical device for {}",
            mount.display()
        ))
    })?;
    let cpath =
        std::ffi::CString::new(bsd.as_bytes()).map_err(|e| CoreError::Disc(e.to_string()))?;

    // The mounted volume holds the drive, which blocks SCSITask creation. The
    // things we need from the mount (MKB, disc id) are already read; unmount,
    // authenticate, then put it back.
    let dev = std::path::PathBuf::from(format!("/dev/{bsd}"));
    let _ = crate::disc::aacs_unmount(&dev);
    std::thread::sleep(std::time::Duration::from_millis(400));

    let mut vid = [0u8; 16];
    let mut mk = [0u8; 16];
    let mut vuk = [0u8; 16];
    let rc = unsafe {
        bf_native_key(
            cpath.as_ptr(),
            header.host_priv.as_ptr(),
            header.host_cert.as_ptr(),
            mkb.as_ptr(),
            mkb.len(),
            header.dks.as_ptr(),
            header.dks.len() as i32,
            vid.as_mut_ptr(),
            mk.as_mut_ptr(),
            vuk.as_mut_ptr(),
        )
    };
    // Releasing exclusive access makes the drive re-appear; give the OS time to
    // re-read and remount the volume before the caller opens it again.
    for _ in 0..12 {
        crate::disc::remount(&dev);
        if crate::device::list_optical()
            .iter()
            .any(|d| d.mount_point.is_some())
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    if rc != 0 {
        return Err(CoreError::Disc(format!(
            "native AACS failed (code {rc}); is a disc inserted and the volume mounted?"
        )));
    }

    let name = mount
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "UNKNOWN".into());

    Ok(NativeKey {
        id,
        name,
        vid: hexstr(&vid),
        media_key: hexstr(&mk),
        vuk: hexstr(&vuk),
    })
}

/// Derive and store the key in KEYDB. Returns the entry (already existing ones
/// are left untouched).
pub fn derive_and_store(mount: &Path) -> Result<(NativeKey, bool), CoreError> {
    let key = derive(mount)?;
    let path = keys::keydb_path();
    if keys::has_at(&path, &key.id).unwrap_or(false) {
        return Ok((key, false));
    }
    let entry = KeyEntry {
        id: key.id.clone(),
        name: Some(format!("{} ({})", key.name, key.name)),
        media_key: Some(key.media_key.clone()),
        volume_id: Some(key.vid.clone()),
        vuk: Some(key.vuk.clone()),
        unit_keys: Vec::new(),
        note: Some("native".into()),
    };
    keys::upsert_at(&path, &entry)?;
    Ok((key, true))
}
