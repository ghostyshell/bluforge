//! AACS drive access on Linux and Windows.
//!
//! The macOS path drives IOKit's SCSITask interface. Here the same MMC
//! commands go through Linux `SG_IO` or Windows `IOCTL_SCSI_PASS_THROUGH_DIRECT`.
//!
//! Two ways to get the AACS volume id:
//!   1. LibreDrive vendor `READ BUFFER` 0x3c / buffer 0x4410. The drive returns
//!      the decrypted VID with no authentication. This is what MakeMKV does.
//!   2. Full AACS authentication with a host certificate from KEYDB: AGID,
//!      certificate challenge, drive certificate and key, host signature, then
//!      an authenticated `READ DISC STRUCTURE` 0x80.
//!
//! The crypto calls are the vendored libaacs helpers used by the media-key
//! derivation, so this does not need MakeMKV or a host-certificate library.

use crate::CoreError;

const SEND_KEY: u8 = 0xa3;
const REPORT_KEY: u8 = 0xa4;
const READ_DISC_STRUCTURE: u8 = 0xad;
const READ_BUFFER: u8 = 0x3c;
const KEY_CLASS_AACS: u8 = 0x02;

const RK_AGID: u8 = 0x00;
const RK_DRIVE_CERT_CHALLENGE: u8 = 0x01;
const RK_DRIVE_KEY: u8 = 0x02;
const RK_INVALIDATE_AGID: u8 = 0x3f;
const SK_HOST_CERT_CHALLENGE: u8 = 0x01;
const SK_HOST_KEY: u8 = 0x02;

const LIBREDRIVE_BUFFER: u32 = 0x4410;

extern "C" {
    fn crypto_create_nonce(buf: *mut u8, len: usize);
    fn crypto_create_host_key_pair(key: *mut u8, key_point: *mut u8);
    fn crypto_create_bus_key(priv_key: *const u8, drive_key_point: *const u8, bus_key: *mut u8);
    fn crypto_aacs_sign(
        cert: *const u8,
        priv_key: *const u8,
        signature: *mut u8,
        nonce: *const u8,
        point: *const u8,
    ) -> i32;
    fn crypto_aacs_verify(cert: *const u8, signature: *const u8, data: *const u8, len: u32) -> i32;
    fn crypto_aacs_verify_drive_cert(cert: *const u8) -> i32;
    fn crypto_aes_cmac_16(data: *const u8, aes_key: *const u8, cmac: *mut u8) -> i32;
}

trait Transport {
    /// `tx` sends data to the device; otherwise the device fills `data`.
    fn send(&mut self, cdb: &[u8; 16], data: &mut [u8], tx: bool) -> Result<(), CoreError>;
}

fn cdb_error(op: u8, detail: String) -> CoreError {
    CoreError::Disc(format!("AACS MMC op {op:02x}: {detail}"))
}

fn report_key<T: Transport>(
    t: &mut T,
    agid: u8,
    addr: u32,
    blocks: u8,
    format: u8,
    buf: &mut [u8],
) -> Result<(), CoreError> {
    let mut cdb = [0u8; 16];
    cdb[0] = REPORT_KEY;
    cdb[2] = (addr >> 24) as u8;
    cdb[3] = (addr >> 16) as u8;
    cdb[4] = (addr >> 8) as u8;
    cdb[5] = addr as u8;
    cdb[6] = blocks;
    cdb[7] = KEY_CLASS_AACS;
    cdb[8] = (buf.len() >> 8) as u8;
    cdb[9] = buf.len() as u8;
    cdb[10] = (agid << 6) | (format & 0x3f);
    t.send(&cdb, buf, false)
}

fn send_key<T: Transport>(
    t: &mut T,
    agid: u8,
    format: u8,
    buf: &mut [u8],
) -> Result<(), CoreError> {
    let mut cdb = [0u8; 16];
    cdb[0] = SEND_KEY;
    cdb[7] = KEY_CLASS_AACS;
    cdb[8] = (buf.len() >> 8) as u8;
    cdb[9] = buf.len() as u8;
    cdb[10] = (agid << 6) | (format & 0x3f);
    t.send(&cdb, buf, true)
}

fn read_disc_structure<T: Transport>(
    t: &mut T,
    agid: u8,
    format: u8,
    buf: &mut [u8],
) -> Result<(), CoreError> {
    let mut cdb = [0u8; 16];
    cdb[0] = READ_DISC_STRUCTURE;
    cdb[1] = 0x01;
    cdb[6] = 0x00;
    cdb[7] = format;
    cdb[8] = (buf.len() >> 8) as u8;
    cdb[9] = buf.len() as u8;
    cdb[10] = agid << 6;
    t.send(&cdb, buf, false)
}

fn read_buffer<T: Transport>(
    t: &mut T,
    buffer_id: u32,
    offset: u32,
    buf: &mut [u8],
) -> Result<(), CoreError> {
    let mut cdb = [0u8; 16];
    cdb[0] = READ_BUFFER;
    cdb[1] = 0x01;
    cdb[2] = (buffer_id >> 8) as u8;
    cdb[3] = buffer_id as u8;
    cdb[4] = (offset >> 16) as u8;
    cdb[5] = (offset >> 8) as u8;
    cdb[6] = offset as u8;
    cdb[7] = (buf.len() >> 16) as u8;
    cdb[8] = (buf.len() >> 8) as u8;
    cdb[9] = buf.len() as u8;
    t.send(&cdb, buf, false)
}

fn alloc_agid<T: Transport>(t: &mut T) -> Result<u8, CoreError> {
    let mut sink = [0u8; 8];
    for i in 0..4u8 {
        let _ = report_key(t, i, 0, 0, RK_INVALIDATE_AGID, &mut sink);
    }
    let mut buf = [0u8; 8];
    report_key(t, 0, 0, 0, RK_AGID, &mut buf)?;
    Ok((buf[7] >> 6) & 0x03)
}

/// The LibreDrive vendor read, which needs no authentication.
pub fn libredrive_vid(device: &str) -> Result<[u8; 16], CoreError> {
    let mut t = open(device)?;
    libredrive_vid_from(&mut t)
}

fn libredrive_vid_from<T: Transport>(t: &mut T) -> Result<[u8; 16], CoreError> {
    let mut one = [0u8; 36];
    let _ = read_buffer(t, LIBREDRIVE_BUFFER, 0, &mut one);
    if one[0] == 0x00 && one[1] == 0x22 && one[2] == 0x00 && one[3] == 0x50 {
        let mut vid = [0u8; 16];
        vid.copy_from_slice(&one[4..20]);
        return Ok(vid);
    }

    let mut big = vec![0u8; 9216];
    read_buffer(t, LIBREDRIVE_BUFFER, 0, &mut big)
        .map_err(|e| CoreError::Disc(format!("LibreDrive VID read: {e}")))?;
    for i in (0..big.len() - 36).step_by(36) {
        if big[i] == 0x00 && big[i + 1] == 0x22 && big[i + 2] == 0x00 && big[i + 3] == 0x50 {
            let mut vid = [0u8; 16];
            vid.copy_from_slice(&big[i + 4..i + 20]);
            return Ok(vid);
        }
    }
    Err(CoreError::Disc(
        "no LibreDrive VID record in buffer 0x4410".into(),
    ))
}

/// Authenticate with a host certificate and read the VID.
pub fn auth_vid(device: &str, host_priv: &[u8], host_cert: &[u8]) -> Result<[u8; 16], CoreError> {
    if host_priv.len() < 20 || host_cert.len() < 92 {
        return Err(CoreError::Disc(
            "host certificate/private key in KEYDB is truncated".into(),
        ));
    }
    let mut t = open(device)?;
    let agid = alloc_agid(&mut t)?;

    let mut host_nonce = [0u8; 20];
    let mut host_priv_key = [0u8; 16];
    let mut host_pub = [0u8; 40];
    unsafe {
        crypto_create_nonce(host_nonce.as_mut_ptr(), host_nonce.len());
        crypto_create_host_key_pair(host_priv_key.as_mut_ptr(), host_pub.as_mut_ptr());
    }

    let mut buf = [0u8; 116];
    buf[1] = 0x32;
    buf[4..24].copy_from_slice(&host_nonce);
    buf[24..116].copy_from_slice(&host_cert[..92]);
    send_key(&mut t, agid, SK_HOST_CERT_CHALLENGE, &mut buf)
        .map_err(|e| CoreError::Disc(format!("host certificate rejected: {e}")))?;

    let mut buf = [0u8; 116];
    report_key(&mut t, agid, 0, 0, RK_DRIVE_CERT_CHALLENGE, &mut buf)
        .map_err(|e| CoreError::Disc(format!("no drive certificate: {e}")))?;
    let drive_nonce = &buf[4..24];
    let drive_cert = &buf[24..116];
    let drive_cert_ok = unsafe { crypto_aacs_verify_drive_cert(drive_cert.as_ptr()) } == 0;
    if !drive_cert_ok {
        return Err(CoreError::Disc(
            "the drive certificate signature is invalid".into(),
        ));
    }

    let mut buf = [0u8; 116];
    report_key(&mut t, agid, 0, 0, RK_DRIVE_KEY, &mut buf)
        .map_err(|e| CoreError::Disc(format!("no drive key: {e}")))?;
    let drive_point = &buf[4..44];
    let drive_sig = &buf[44..84];

    let mut signed = [0u8; 60];
    signed[..20].copy_from_slice(&host_nonce);
    signed[20..60].copy_from_slice(drive_point);
    if unsafe { crypto_aacs_verify(drive_cert.as_ptr(), drive_sig.as_ptr(), signed.as_ptr(), 60) }
        != 0
    {
        return Err(CoreError::Disc("the drive signature is invalid".into()));
    }

    let mut host_sig = [0u8; 40];
    if unsafe {
        crypto_aacs_sign(
            host_cert.as_ptr(),
            host_priv.as_ptr(),
            host_sig.as_mut_ptr(),
            drive_nonce.as_ptr(),
            host_pub.as_ptr(),
        )
    } != 0
    {
        return Err(CoreError::Disc("host signing failed".into()));
    }
    let mut buf = [0u8; 116];
    buf[1] = 0x52;
    buf[4..44].copy_from_slice(&host_pub);
    buf[44..84].copy_from_slice(&host_sig);
    send_key(&mut t, agid, SK_HOST_KEY, &mut buf)
        .map_err(|e| CoreError::Disc(format!("host key rejected: {e}")))?;

    let mut bus_key = [0u8; 16];
    unsafe {
        crypto_create_bus_key(
            host_priv_key.as_ptr(),
            drive_point.as_ptr(),
            bus_key.as_mut_ptr(),
        )
    }

    let mut buf = [0u8; 36];
    read_disc_structure(&mut t, agid, 0x80, &mut buf)
        .map_err(|e| CoreError::Disc(format!("could not read the volume id: {e}")))?;
    let mut cmac = [0u8; 16];
    unsafe {
        crypto_aes_cmac_16(buf[4..20].as_ptr(), bus_key.as_ptr(), cmac.as_mut_ptr());
    }
    if cmac != buf[20..36] {
        return Err(CoreError::Disc(
            "the volume id CMAC does not match the bus key".into(),
        ));
    }

    let mut vid = [0u8; 16];
    vid.copy_from_slice(&buf[4..20]);
    Ok(vid)
}

/// LibreDrive first, authenticated AACS as the fallback.
pub fn read_vid(
    device: &str,
    host_priv: Option<&[u8]>,
    host_cert: Option<&[u8]>,
) -> Result<[u8; 16], CoreError> {
    match libredrive_vid(device) {
        Ok(vid) => Ok(vid),
        Err(e) => match (host_priv, host_cert) {
            (Some(priv_key), Some(cert)) => auth_vid(device, priv_key, cert).map_err(|auth| {
                CoreError::Disc(format!(
                    "no LibreDrive VID ({e}); authenticated read failed ({auth})"
                ))
            }),
            _ => Err(e),
        },
    }
}

// ---------------------------------------------------------------------------
// Linux: SG_IO
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    const SG_IO: libc::c_ulong = 0x2285;
    const SG_DXFER_NONE: libc::c_int = -1;
    const SG_DXFER_TO_DEV: libc::c_int = -2;
    const SG_DXFER_FROM_DEV: libc::c_int = -3;

    #[repr(C)]
    struct SgIoHdr {
        interface_id: libc::c_int,
        dxfer_direction: libc::c_int,
        cmd_len: libc::c_uchar,
        mx_sb_len: libc::c_uchar,
        iovec_count: libc::c_ushort,
        dxfer_len: libc::c_uint,
        dxferp: *mut libc::c_void,
        cmdp: *mut libc::c_uchar,
        sbp: *mut libc::c_uchar,
        timeout: libc::c_uint,
        flags: libc::c_uint,
        pack_id: libc::c_int,
        usr_ptr: *mut libc::c_void,
        status: libc::c_uchar,
        masked_status: libc::c_uchar,
        msg_status: libc::c_uchar,
        sb_len_wr: libc::c_uchar,
        host_status: libc::c_ushort,
        driver_status: libc::c_ushort,
        resid: libc::c_int,
        duration: libc::c_uint,
        info: libc::c_uint,
    }

    pub struct SgDevice {
        fd: libc::c_int,
    }

    impl Drop for SgDevice {
        fn drop(&mut self) {
            unsafe {
                libc::close(self.fd);
            }
        }
    }

    pub fn open(device: &str) -> Result<SgDevice, CoreError> {
        let path = if device.starts_with('/') {
            device.to_string()
        } else {
            format!("/dev/{device}")
        };
        let c = CString::new(path.as_bytes())
            .map_err(|e| CoreError::Disc(format!("device path: {e}")))?;
        let fd = unsafe {
            libc::open(
                c.as_ptr(),
                libc::O_RDWR | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(CoreError::Disc(format!(
                "open {path}: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(SgDevice { fd })
    }

    impl Transport for SgDevice {
        fn send(&mut self, cdb: &[u8; 16], data: &mut [u8], tx: bool) -> Result<(), CoreError> {
            let mut sense = [0u8; 32];
            let mut hdr = SgIoHdr {
                interface_id: b'S' as libc::c_int,
                dxfer_direction: if data.is_empty() {
                    SG_DXFER_NONE
                } else if tx {
                    SG_DXFER_TO_DEV
                } else {
                    SG_DXFER_FROM_DEV
                },
                cmd_len: 16,
                mx_sb_len: sense.len() as u8,
                iovec_count: 0,
                dxfer_len: data.len() as u32,
                dxferp: if data.is_empty() {
                    std::ptr::null_mut()
                } else {
                    data.as_mut_ptr() as *mut libc::c_void
                },
                cmdp: cdb.as_ptr() as *mut u8,
                sbp: sense.as_mut_ptr(),
                timeout: 10_000,
                flags: 0,
                pack_id: 0,
                usr_ptr: std::ptr::null_mut(),
                status: 0,
                masked_status: 0,
                msg_status: 0,
                sb_len_wr: 0,
                host_status: 0,
                driver_status: 0,
                resid: 0,
                duration: 0,
                info: 0,
            };
            let rc = unsafe { libc::ioctl(self.fd, SG_IO, &mut hdr) };
            if rc < 0 {
                return Err(CoreError::Disc(format!(
                    "SG_IO: {}",
                    std::io::Error::last_os_error()
                )));
            }
            if hdr.status != 0 || hdr.host_status != 0 || hdr.driver_status != 0 {
                return Err(CoreError::Disc(format!(
                    "SCSI status {:02x} host {:04x} driver {:04x} sense {:02x}/{:02x}/{:02x}",
                    hdr.status,
                    hdr.host_status,
                    hdr.driver_status,
                    sense[2] & 0x0f,
                    sense[12],
                    sense[13]
                )));
            }
            Ok(())
        }
    }
}

#[cfg(target_os = "linux")]
use platform::SgDevice as Device;
#[cfg(target_os = "linux")]
fn open(device: &str) -> Result<Device, CoreError> {
    platform::open(device)
}

// ---------------------------------------------------------------------------
// Windows: IOCTL_SCSI_PASS_THROUGH_DIRECT
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;

    const IOCTL_SCSI_PASS_THROUGH_DIRECT: u32 = 0x0004d014;

    #[repr(C)]
    struct Sptd {
        length: u16,
        scsi_status: u8,
        path_id: u8,
        target_id: u8,
        lun: u8,
        cdb_length: u8,
        sense_info_length: u8,
        data_in: u8,
        data_transfer_length: u32,
        time_out_value: u32,
        data_buffer: *mut core::ffi::c_void,
        sense_info_offset: u32,
        cdb: [u8; 16],
    }

    pub struct WinDevice {
        handle: windows_sys::Win32::Foundation::HANDLE,
    }

    impl Drop for WinDevice {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.handle);
            }
        }
    }

    pub fn open(device: &str) -> Result<WinDevice, CoreError> {
        let drive = device.trim_end_matches('\\');
        let drive = if drive.ends_with(':') {
            drive.to_string()
        } else {
            format!("{drive}:")
        };
        let path = format!("\\\\.\\{drive}");
        let wide: Vec<u16> = std::ffi::OsStr::new(&path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(CoreError::Disc(format!(
                "open {path}: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(WinDevice { handle })
    }

    impl Transport for WinDevice {
        fn send(&mut self, cdb: &[u8; 16], data: &mut [u8], tx: bool) -> Result<(), CoreError> {
            const SENSE_LEN: usize = 32;
            let mut buf = vec![0u8; std::mem::size_of::<Sptd>() + SENSE_LEN];
            let sptd = buf.as_mut_ptr() as *mut Sptd;
            unsafe {
                (*sptd).length = std::mem::size_of::<Sptd>() as u16;
                (*sptd).scsi_status = 0;
                (*sptd).path_id = 0;
                (*sptd).target_id = 0;
                (*sptd).lun = 0;
                (*sptd).cdb_length = 16;
                (*sptd).sense_info_length = SENSE_LEN as u8;
                (*sptd).data_in = if tx { 0 } else { 1 };
                (*sptd).data_transfer_length = data.len() as u32;
                (*sptd).time_out_value = 10;
                (*sptd).data_buffer = data.as_mut_ptr() as *mut core::ffi::c_void;
                (*sptd).sense_info_offset = std::mem::size_of::<Sptd>() as u32;
                (*sptd).cdb = *cdb;
            }
            let mut returned = 0u32;
            let ok = unsafe {
                DeviceIoControl(
                    self.handle,
                    IOCTL_SCSI_PASS_THROUGH_DIRECT,
                    buf.as_ptr() as *const core::ffi::c_void,
                    buf.len() as u32,
                    buf.as_mut_ptr() as *mut core::ffi::c_void,
                    buf.len() as u32,
                    &mut returned,
                    ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(CoreError::Disc(format!(
                    "SCSI pass-through: {}",
                    std::io::Error::last_os_error()
                )));
            }
            let status = unsafe { (*sptd).scsi_status };
            if status != 0 {
                let sense = &buf[std::mem::size_of::<Sptd>()..];
                return Err(CoreError::Disc(format!(
                    "SCSI status {status:02x} sense {:02x}/{:02x}/{:02x}",
                    sense[2] & 0x0f,
                    sense[12],
                    sense[13]
                )));
            }
            Ok(())
        }
    }
}

#[cfg(target_os = "windows")]
use platform::WinDevice as Device;
#[cfg(target_os = "windows")]
fn open(device: &str) -> Result<Device, CoreError> {
    platform::open(device)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Mock {
        cdbs: Vec<[u8; 16]>,
        reply: Vec<u8>,
        tx: Vec<bool>,
    }

    impl Mock {
        fn new(reply: Vec<u8>) -> Self {
            Self {
                cdbs: Vec::new(),
                reply,
                tx: Vec::new(),
            }
        }
    }

    impl Transport for Mock {
        fn send(&mut self, cdb: &[u8; 16], data: &mut [u8], tx: bool) -> Result<(), CoreError> {
            self.cdbs.push(*cdb);
            self.tx.push(tx);
            for (i, b) in data.iter_mut().enumerate() {
                *b = self.reply.get(i).copied().unwrap_or(0);
            }
            Ok(())
        }
    }

    #[test]
    fn read_buffer_cdb_matches_the_vendor_command() {
        let mut m = Mock::new(vec![0; 36]);
        let mut buf = [0u8; 36];
        read_buffer(&mut m, 0x4410, 0, &mut buf).unwrap();
        assert_eq!(m.cdbs[0][0], 0x3c);
        assert_eq!(m.cdbs[0][1], 0x01);
        assert_eq!(&m.cdbs[0][2..4], &[0x44, 0x10]);
        assert_eq!(&m.cdbs[0][7..10], &[0x00, 0x00, 36]);
        assert!(!m.tx[0]);
    }

    #[test]
    fn report_key_carries_aacs_class_and_agid() {
        let mut m = Mock::new(vec![0; 8]);
        let mut buf = [0u8; 8];
        report_key(&mut m, 2, 0, 0, RK_AGID, &mut buf).unwrap();
        assert_eq!(m.cdbs[0][0], 0xa4);
        assert_eq!(m.cdbs[0][7], 0x02);
        assert_eq!(m.cdbs[0][10], 0x80);
    }

    #[test]
    fn finds_the_libredrive_record_at_an_offset() {
        let mut reply = vec![0u8; 9216];
        reply[72..76].copy_from_slice(&[0x00, 0x22, 0x00, 0x50]);
        reply[76..92].copy_from_slice(&[0xAB; 16]);
        let mut m = Mock::new(reply);
        let vid = libredrive_vid_from(&mut m).unwrap();
        assert_eq!(vid, [0xAB; 16]);
    }
}
