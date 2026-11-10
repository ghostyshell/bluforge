//! Native AACS key recovery: read the drive's volume id and derive a disc's key
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

#[cfg(target_os = "macos")]
extern "C" {
    fn bf_media_key(
        mkb: *const u8,
        mkb_len: usize,
        dks: *const BfDk,
        ndks: i32,
        out_mk: *mut u8,
    ) -> i32;
    fn bf_aesg(key: *const u8, data: *const u8, out: *mut u8);
}

fn makemkvcon() -> Option<std::path::PathBuf> {
    let p = std::path::PathBuf::from("/Applications/MakeMKV.app/Contents/MacOS/makemkvcon");
    p.is_file().then_some(p)
}

/// One `discatt.dat` record: 8-byte header `cat 00 00 id 00 00 00 len` + payload.
fn rec<'a>(data: &'a [u8], cat: u8, id: u8, len: usize) -> Option<&'a [u8]> {
    let mut needle = [0u8; 8];
    needle[0] = cat;
    needle[3] = id;
    needle[7] = len as u8;
    let i = data.windows(8).position(|w| w == needle)?;
    data.get(i + 8..i + 8 + len)
}

/// Drive ids for `SDF_STOP` (disables LibreDrive so the RDK is readable).
fn makemkv_drive_ids(exe: &std::path::Path) -> Vec<String> {
    let out = match std::process::Command::new(exe)
        .args(["f", "--list"])
        .output()
    {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && t.contains('_') && !t.starts_with(char::is_numeric)
        })
        .map(|l| l.trim().trim_end_matches(',').to_string())
        .filter(|l| l.contains('_'))
        .collect()
}

/// Run `makemkvcon backup` just far enough to get `discatt.dat` (holds the VID
/// and, without LibreDrive, the RDK). MakeMKV authenticates with its own
/// (unrevoked) host certificate; we never talk AACS to the drive ourselves.
fn discatt_for(exe: &std::path::Path) -> Option<Vec<u8>> {
    let dir = std::env::temp_dir().join(format!("keepsake-discatt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("discatt.dat");

    let mut cmd = std::process::Command::new(exe);
    cmd.args(["backup", "disc:0"])
        .arg(&dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let ids = makemkv_drive_ids(exe);
    if !ids.is_empty() {
        cmd.env("SDF_STOP", ids.join(" "));
    }
    let mut child = cmd.spawn().ok()?;
    for _ in 0..160 {
        if path.is_file() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    let _ = child.kill();
    let _ = child.wait();
    let data = std::fs::read(&path).ok();
    let _ = std::fs::remove_dir_all(&dir);
    data
}

/// Recover the disc keys via MakeMKV's `discatt.dat` (VID/RDK) plus our offline
/// MKB derivation. Returns the key and the (optional) drive cert + RDK.
#[cfg(target_os = "macos")]
pub fn from_makemkv(
    mount: &Path,
) -> Result<(NativeKey, Option<Vec<u8>>, Option<Vec<u8>>), CoreError> {
    let mounted = crate::device::ensure_mounted(mount);
    let mount = mounted.as_path();
    let exe =
        makemkvcon().ok_or_else(|| CoreError::Keys("MakeMKV (makemkvcon) not found".into()))?;
    let header = read_header(&keys::keydb_path())?;
    let mkb = read_mkb(mount)?;
    let (ukf, aacs2) = read_unit_key_file(mount);
    let id = keys::disc_id_for_path(mount).ok_or_else(|| {
        CoreError::Disc(format!("no AACS/Unit_Key_RO.inf under {}", mount.display()))
    })?;

    let data = discatt_for(&exe)
        .ok_or_else(|| CoreError::Disc("MakeMKV produced no discatt.dat".into()))?;
    let vid = rec(&data, 0x04, 0x80, 16)
        .ok_or_else(|| CoreError::Disc("discatt.dat has no VID".into()))?;
    let rdk = rec(&data, 0x04, 0x84, 32).map(|r| r.to_vec());
    let dc = rec(&data, 0x01, 0x03, 92).map(|r| r.to_vec());

    let mut mk = [0u8; 16];
    let rc = unsafe {
        bf_media_key(
            mkb.as_ptr(),
            mkb.len(),
            header.dks.as_ptr(),
            header.dks.len() as i32,
            mk.as_mut_ptr(),
        )
    };
    if rc != 0 {
        return Err(CoreError::Disc(format!(
            "media key derivation failed (code {rc})"
        )));
    }
    let mut vuk = [0u8; 16];
    unsafe { bf_aesg(mk.as_ptr(), vid.as_ptr(), vuk.as_mut_ptr()) };
    let unit_keys = unit_keys_from(ukf.as_deref(), aacs2, &vuk);

    let name = mount
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "UNKNOWN".into());
    Ok((
        NativeKey {
            id,
            name,
            vid: hexstr(vid),
            media_key: hexstr(&mk),
            vuk: hexstr(&vuk),
            unit_keys,
        },
        dc,
        rdk,
    ))
}

fn aacs_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
        .join("Library/Preferences/aacs")
}

/// Store the key in KEYDB and cache VID/RDK where libaacs looks for them
/// (RDK is per drive+disc; needed for bus-encrypted discs once the HC is dead).
#[cfg(target_os = "macos")]
pub fn from_makemkv_and_store(mount: &Path) -> Result<(NativeKey, bool), CoreError> {
    let (key, dc, rdk) = from_makemkv(mount)?;
    let path = keys::keydb_path();
    let mut added = false;
    if !keys::has_at(&path, &key.id).unwrap_or(false) {
        let entry = KeyEntry {
            id: key.id.clone(),
            name: Some(format!("{} ({})", key.name, key.name)),
            media_key: Some(key.media_key.clone()),
            volume_id: Some(key.vid.clone()),
            vuk: Some(key.vuk.clone()),
            unit_keys: key.unit_keys.clone(),
            note: Some("from-makemkv".into()),
        };
        keys::upsert_at(&path, &entry)?;
        added = true;
    }
    let vid_raw = hexbytes(&key.vid).unwrap_or_default();
    let aacs = aacs_dir();
    if !vid_raw.is_empty() {
        let _ = std::fs::create_dir_all(aacs.join("vid"));
        let _ = std::fs::write(aacs.join("vid").join(&key.id), &vid_raw);
    }
    if let (Some(dc), Some(rdk)) = (dc, rdk) {
        use sha1::{Digest, Sha1};
        let driveid: String = Sha1::digest(&dc)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let dir = aacs.join("rdk").join(driveid);
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(&key.id), &rdk);
    }
    Ok((key, added))
}

#[cfg(target_os = "macos")]
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
    /// AACS2/UHD CPS unit keys, derived from the disc's Unit_Key_RO.inf.
    #[serde(default)]
    pub unit_keys: Vec<String>,
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

/// The disc's `Unit_Key_RO.inf` and whether `Content000.cer` marks AACS2.
fn read_unit_key_file(mount: &Path) -> (Option<Vec<u8>>, bool) {
    let ukf = std::fs::read(mount.join("AACS").join("Unit_Key_RO.inf")).ok();
    let aacs2 = std::fs::read(mount.join("AACS").join("Content000.cer"))
        .ok()
        .and_then(|d| d.first().copied())
        .map(|b| b == 0x10)
        .unwrap_or(false);
    (ukf, aacs2)
}

/// Decrypt the CPS unit keys from `Unit_Key_RO.inf` with the VUK.
///
/// The file stores encrypted unit keys; each one is AES-128 ECB decrypted with
/// the VUK. AACS1 records are 48 bytes apart, AACS2 64 (16 extra metadata bytes).
fn unit_keys_from(ukf: Option<&[u8]>, aacs2: bool, vuk: &[u8; 16]) -> Vec<String> {
    use aes::cipher::{BlockDecrypt, KeyInit};

    let Some(d) = ukf else {
        return Vec::new();
    };
    if d.len() < 20 {
        return Vec::new();
    }
    let count_pos = u32::from_be_bytes([d[0], d[1], d[2], d[3]]) as usize;
    if count_pos + 2 > d.len() {
        return Vec::new();
    }
    let num = u16::from_be_bytes([d[count_pos], d[count_pos + 1]]) as usize;
    if num == 0 {
        return Vec::new();
    }
    let stride = if aacs2 { 64 } else { 48 };
    let first = count_pos + 48;
    if first + (num - 1) * stride + 16 > d.len() {
        return Vec::new();
    }
    let cipher = aes::Aes128::new(vuk.into());
    let mut keys = Vec::with_capacity(num);
    for i in 0..num {
        let off = first + i * stride;
        let mut block = aes::Block::clone_from_slice(&d[off..off + 16]);
        cipher.decrypt_block(&mut block);
        keys.push(hexstr(&block));
    }
    keys
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
    host_priv: Option<Vec<u8>>,
    host_cert: Option<Vec<u8>>,
    dks: Vec<BfDk>,
}

/// Host certificate and device keys from the KEYDB header (`| HC |`, `| DK |`).
///
/// The certificate is optional: a LibreDrive drive returns the VID itself, so
/// `derive` only needs the device keys. A certificate is still read when present
/// because it is the fallback path for a drive that is not LibreDrive-enabled.
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

    // A host certificate/key supplied by the environment wins (lets a user
    // bring their own licensed certificate without editing KEYDB).
    let env_priv = crate::env_compat("HOST_PRIV").and_then(|v| hexbytes(&v));
    let env_cert = crate::env_compat("HOST_CERT").and_then(|v| hexbytes(&v));
    if let (Some(k), Some(c)) = (env_priv, env_cert) {
        if k.len() == 20 && c.len() == 92 {
            return Ok(Header {
                host_priv: Some(k),
                host_cert: Some(c),
                dks,
            });
        }
    }

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
#[cfg(target_os = "macos")]
pub fn derive(mount: &Path) -> Result<NativeKey, CoreError> {
    let keydb = keys::keydb_path();
    let header = read_header(&keydb)?;
    let mkb = read_mkb(mount)?;
    let (ukf, aacs2) = read_unit_key_file(mount);

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
    // read the VID, then put it back.
    let dev = std::path::PathBuf::from(format!("/dev/{bsd}"));
    let _ = crate::disc::aacs_unmount(&dev);
    std::thread::sleep(std::time::Duration::from_millis(400));

    let mut vid = [0u8; 16];
    let mut mk = [0u8; 16];
    let mut vuk = [0u8; 16];
    // The vendor VID read needs no certificate; only the fallback does.
    let host_priv = header
        .host_priv
        .as_deref()
        .map_or(std::ptr::null(), |v| v.as_ptr());
    let host_cert = header
        .host_cert
        .as_deref()
        .map_or(std::ptr::null(), |v| v.as_ptr());
    let rc = unsafe {
        bf_native_key(
            cpath.as_ptr(),
            host_priv,
            host_cert,
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

    let unit_keys = unit_keys_from(ukf.as_deref(), aacs2, &vuk);
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
        unit_keys,
    })
}

/// Derive and store the key in KEYDB. Returns the entry (already existing ones
/// are left untouched).
#[cfg(target_os = "macos")]
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
        unit_keys: key.unit_keys.clone(),
        note: Some("native".into()),
    };
    keys::upsert_at(&path, &entry)?;
    Ok((key, true))
}

#[cfg(test)]
mod unit_key_tests {
    use super::*;
    use aes::cipher::{BlockEncrypt, KeyInit};

    const VUK: [u8; 16] = [0x42; 16];

    fn build_ukf(aacs2: bool, plains: &[[u8; 16]]) -> Vec<u8> {
        let count_pos = 0x20usize;
        let stride = if aacs2 { 64 } else { 48 };
        let mut d = vec![0u8; count_pos + 48 + (plains.len() - 1) * stride + 16];
        d[0..4].copy_from_slice(&(count_pos as u32).to_be_bytes());
        d[count_pos..count_pos + 2].copy_from_slice(&(plains.len() as u16).to_be_bytes());
        let cipher = aes::Aes128::new((&VUK).into());
        for (i, p) in plains.iter().enumerate() {
            let mut block = aes::Block::clone_from_slice(p);
            cipher.encrypt_block(&mut block);
            let off = count_pos + 48 + i * stride;
            d[off..off + 16].copy_from_slice(&block);
        }
        d
    }

    #[test]
    fn decrypts_aacs1_and_aacs2_unit_keys() {
        let plains = [[0x01u8; 16], [0x02u8; 16], [0x03u8; 16]];
        for aacs2 in [false, true] {
            let d = build_ukf(aacs2, &plains);
            let keys = unit_keys_from(Some(&d), aacs2, &VUK);
            assert_eq!(keys.len(), 3, "aacs2={aacs2}");
            for (i, p) in plains.iter().enumerate() {
                assert_eq!(keys[i], hexstr(p), "key {i} aacs2={aacs2}");
            }
        }
    }

    #[test]
    fn returns_nothing_without_keys() {
        let mut d = vec![0u8; 0x40];
        d[0..4].copy_from_slice(&0x20u32.to_be_bytes());
        assert!(unit_keys_from(Some(&d), false, &VUK).is_empty());
        assert!(unit_keys_from(None, true, &VUK).is_empty());
    }
}

// Native key recovery links libaacs internals and the macOS MMC layer. The
// other platforms use libaacs with a KEYDB entry instead; keep the same API so
// the CLI and MCP tools build and return a clear error.
#[cfg(not(target_os = "macos"))]
pub fn from_makemkv(
    _mount: &Path,
) -> Result<(NativeKey, Option<Vec<u8>>, Option<Vec<u8>>), CoreError> {
    Err(CoreError::Disc(
        "native AACS key recovery is macOS-only; add a KEYDB entry or use libaacs".into(),
    ))
}

#[cfg(not(target_os = "macos"))]
pub fn from_makemkv_and_store(_mount: &Path) -> Result<(NativeKey, bool), CoreError> {
    Err(CoreError::Disc(
        "native AACS key recovery is macOS-only; add a KEYDB entry or use libaacs".into(),
    ))
}

#[cfg(not(target_os = "macos"))]
pub fn derive(_mount: &Path) -> Result<NativeKey, CoreError> {
    Err(CoreError::Disc(
        "native AACS key recovery is macOS-only; add a KEYDB entry or use libaacs".into(),
    ))
}

#[cfg(not(target_os = "macos"))]
pub fn derive_and_store(_mount: &Path) -> Result<(NativeKey, bool), CoreError> {
    Err(CoreError::Disc(
        "native AACS key recovery is macOS-only; add a KEYDB entry or use libaacs".into(),
    ))
}
