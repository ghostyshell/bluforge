//! Optical / block device discovery via IOKit.
//!
//! We deliberately talk to IOKit directly rather than shelling out to
//! `diskutil`/`drutil`, so the same code can later be reused by the Tauri
//! backend on macOS. Non-macOS stubs are provided for cross-platform builds.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockDevice {
    /// e.g. `disk5`
    pub bsd_name: String,
    /// Human readable media name (`BD-ROM`, `Deadpool`, ...)
    pub media_name: Option<String>,
    /// True for whole disks (not partitions).
    pub is_whole: bool,
    pub removable: bool,
    pub size_bytes: u64,
    pub vendor: Option<String>,
    pub product: Option<String>,
    pub revision: Option<String>,
    /// SCSI peripheral device type (0 = disk, 5 = CD/DVD/BD).
    pub scsi_pdt: Option<u8>,
    /// True if an optical (DVD/BD) driver is attached anywhere in the chain.
    pub optical: bool,
    /// Transport, e.g. `USB`, `SATA`, `Apple Fabric`, `Disk Image`.
    pub interconnect: Option<String>,
    pub mount_point: Option<String>,
    /// Raw character-device path for the whole disk (`/dev/rdisk5`), if any.
    pub device_path: Option<String>,
    /// IORegistry path of the owning service (stable-ish identifier).
    pub registry_path: String,
}

impl BlockDevice {
    pub fn display(&self) -> String {
        match (&self.vendor, &self.product) {
            (Some(v), Some(p)) => format!("{v} {p}"),
            _ => self.media_name.clone().unwrap_or_else(|| self.bsd_name.clone()),
        }
    }
}

// ---------------------------------------------------------------------------
// macOS implementation
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod mac {
    use super::BlockDevice;
    use libc::{c_char, c_int, c_void};
    use std::ffi::CStr;

    type KernReturn = c_int;
    type IoObject = u32;
    type CFTypeRef = *const c_void;
    type AllocatorRef = *const c_void;
    type MutDictRef = *mut c_void;

    const KERN_SUCCESS: KernReturn = 0;
    const UTF8: u32 = 0x0800_0100;
    const CF_SINT64: c_int = 4;

    #[link(name = "IOKit", kind = "framework")]
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn IOServiceMatching(name: *const c_char) -> MutDictRef;
        fn IOServiceGetMatchingServices(
            main_port: u32,
            matching: MutDictRef,
            existing: *mut IoObject,
        ) -> KernReturn;
        fn IOIteratorNext(iterator: IoObject) -> IoObject;
        fn IOObjectRelease(object: IoObject) -> KernReturn;
        fn IORegistryEntryCreateCFProperty(
            entry: IoObject,
            key: CFTypeRef,
            allocator: AllocatorRef,
            options: u32,
        ) -> CFTypeRef;
        fn IORegistryEntryGetParentEntry(
            entry: IoObject,
            plane: *const c_char,
            parent: *mut IoObject,
        ) -> KernReturn;
        fn IORegistryEntryGetPath(
            entry: IoObject,
            plane: *const c_char,
            path: *mut c_char,
            size: *mut u32,
        ) -> KernReturn;
        fn IOObjectGetClass(object: IoObject, class_name: *mut c_char) -> KernReturn;
        fn CFStringCreateWithCString(alloc: AllocatorRef, s: *const c_char, enc: u32) -> CFTypeRef;
        fn CFStringGetCString(s: CFTypeRef, buf: *mut c_char, size: isize, enc: u32) -> bool;
        fn CFRelease(cf: CFTypeRef);
        fn CFNumberGetValue(number: CFTypeRef, ty: c_int, out: *mut c_void) -> bool;
        fn CFBooleanGetValue(b: CFTypeRef) -> bool;
        fn CFGetTypeID(cf: CFTypeRef) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFNumberGetTypeID() -> usize;
        fn CFBooleanGetTypeID() -> usize;
        fn CFDictionaryGetValue(dict: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
    }

    fn key(name: &str) -> CFTypeRef {
        let c = std::ffi::CString::new(name).unwrap();
        unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), UTF8) }
    }

    fn cf_string(v: CFTypeRef) -> Option<String> {
        if v.is_null() || unsafe { CFGetTypeID(v) } != unsafe { CFStringGetTypeID() } {
            return None;
        }
        let mut buf = [0i8; 1024];
        let ok = unsafe { CFStringGetCString(v, buf.as_mut_ptr(), buf.len() as isize, UTF8) };
        if !ok {
            return None;
        }
        Some(
            unsafe { CStr::from_ptr(buf.as_ptr()) }
                .to_string_lossy()
                .into_owned(),
        )
    }

    fn cf_i64(v: CFTypeRef) -> Option<i64> {
        if v.is_null() || unsafe { CFGetTypeID(v) } != unsafe { CFNumberGetTypeID() } {
            return None;
        }
        let mut out: i64 = 0;
        let ok = unsafe { CFNumberGetValue(v, CF_SINT64, &mut out as *mut i64 as *mut c_void) };
        if ok {
            Some(out)
        } else {
            None
        }
    }

    fn cf_bool(v: CFTypeRef) -> Option<bool> {
        if v.is_null() || unsafe { CFGetTypeID(v) } != unsafe { CFBooleanGetTypeID() } {
            return None;
        }
        Some(unsafe { CFBooleanGetValue(v) })
    }

    fn prop(entry: IoObject, name: &str) -> CFTypeRef {
        let k = key(name);
        let v = unsafe { IORegistryEntryCreateCFProperty(entry, k, std::ptr::null(), 0) };
        unsafe { CFRelease(k) };
        v
    }

    fn prop_string(entry: IoObject, name: &str) -> Option<String> {
        let v = prop(entry, name);
        let r = cf_string(v);
        if !v.is_null() {
            unsafe { CFRelease(v) };
        }
        r
    }

    fn prop_i64(entry: IoObject, name: &str) -> Option<i64> {
        let v = prop(entry, name);
        let r = cf_i64(v);
        if !v.is_null() {
            unsafe { CFRelease(v) };
        }
        r
    }

    fn prop_bool(entry: IoObject, name: &str) -> Option<bool> {
        let v = prop(entry, name);
        let r = cf_bool(v);
        if !v.is_null() {
            unsafe { CFRelease(v) };
        }
        r
    }

    fn class_of(entry: IoObject) -> String {
        let mut buf = [0i8; 128];
        unsafe { IOObjectGetClass(entry, buf.as_mut_ptr()) };
        unsafe { CStr::from_ptr(buf.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    }

    fn registry_path(entry: IoObject) -> String {
        let mut buf = [0i8; 1024];
        let mut size = buf.len() as u32;
        unsafe { IORegistryEntryGetPath(entry, b"IOService\0".as_ptr() as *const c_char, buf.as_mut_ptr(), &mut size) };
        let s = unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned();
        s.strip_prefix("IOService:").map(|x| x.to_string()).unwrap_or(s)
    }

    /// Collect vendor/product/revision/pdt/interconnect while walking the
    /// parent chain from a media node up to the controller.
    struct Chain {
        vendor: Option<String>,
        product: Option<String>,
        revision: Option<String>,
        pdt: Option<u8>,
        optical: bool,
        interconnect: Option<String>,
    }

    fn walk(entry: IoObject) -> Chain {
        let mut chain = Chain {
            vendor: None,
            product: None,
            revision: None,
            pdt: None,
            optical: false,
            interconnect: None,
        };
        let mut cur = entry;
        let mut owned = false;
        for _ in 0..16 {
            let cls = class_of(cur);
            if cls.contains("IODVD") || cls.contains("IOBD") || cls.starts_with("IODVDB") {
                chain.optical = true;
            }
            if chain.vendor.is_none() {
                if let Some(v) = prop_string(cur, "Vendor Identification") {
                    chain.vendor = Some(v);
                } else if let Some(d) = {
                    let d = prop(cur, "Device Characteristics");
                    Some(d)
                } {
                    if !d.is_null() {
                        let k = key("Vendor Name");
                        let vv = unsafe { CFDictionaryGetValue(d, k) };
                        chain.vendor = cf_string(vv);
                        unsafe { CFRelease(k); CFRelease(d) };
                    }
                }
            }
            if chain.product.is_none() {
                if let Some(v) = prop_string(cur, "Product Identification") {
                    chain.product = Some(v);
                } else if let Some(d) = {
                    let d = prop(cur, "Device Characteristics");
                    Some(d)
                } {
                    if !d.is_null() {
                        let k = key("Product Name");
                        let vv = unsafe { CFDictionaryGetValue(d, k) };
                        chain.product = cf_string(vv);
                        unsafe { CFRelease(k); CFRelease(d) };
                    }
                }
            }
            if chain.pdt.is_none() {
                if let Some(p) = prop_i64(cur, "Peripheral Device Type") {
                    chain.pdt = Some(p as u8);
                    if p == 5 {
                        chain.optical = true;
                    }
                }
            }
            if chain.revision.is_none() {
                chain.revision = prop_string(cur, "Product Revision Level");
            }
            if chain.interconnect.is_none() {
                if let Some(s) = prop_string(cur, "Physical Interconnect") {
                    chain.interconnect = Some(s);
                }
            }
            let mut parent: IoObject = 0;
            if unsafe { IORegistryEntryGetParentEntry(cur, b"IOService\0".as_ptr() as *const c_char, &mut parent) } != KERN_SUCCESS
                || parent == 0
            {
                break;
            }
            if owned {
                unsafe { IOObjectRelease(cur) };
            }
            cur = parent;
            owned = true;
        }
        if owned {
            unsafe { IOObjectRelease(cur) };
        }
        chain
    }

    pub fn list() -> Vec<BlockDevice> {
        let mut out = Vec::new();
        let mut iter: IoObject = 0;
        let matching = unsafe { IOServiceMatching(b"IOMedia\0".as_ptr() as *const c_char) };
        if unsafe { IOServiceGetMatchingServices(0, matching, &mut iter) } != KERN_SUCCESS {
            return out;
        }
        loop {
            let svc = unsafe { IOIteratorNext(iter) };
            if svc == 0 {
                break;
            }
            let is_whole = prop_bool(svc, "Whole").unwrap_or(false);
            if is_whole {
                let chain = walk(svc);
                let bsd = prop_string(svc, "BSD Name");
                if let Some(bsd_name) = bsd {
                    let device_path = Some(format!("/dev/r{bsd_name}"));
                    out.push(BlockDevice {
                        bsd_name,
                        media_name: prop_string(svc, "Media Name")
                            .or_else(|| prop_string(svc, "Volume Name")),
                        is_whole: true,
                        removable: prop_bool(svc, "Removable").unwrap_or(false),
                        size_bytes: prop_i64(svc, "Size").unwrap_or(0).max(0) as u64,
                        vendor: chain.vendor,
                        product: chain.product,
                        revision: chain.revision,
                        scsi_pdt: chain.pdt,
                        optical: chain.optical,
                        interconnect: chain.interconnect,
                        mount_point: None,
                        device_path,
                        registry_path: registry_path(svc),
                    });
                }
            }
            unsafe { IOObjectRelease(svc) };
        }
        unsafe { IOObjectRelease(iter) };
        attach_mount_points(&mut out);
        out.sort_by(|a, b| a.bsd_name.cmp(&b.bsd_name));
        out
    }

    fn attach_mount_points(devs: &mut [BlockDevice]) {
        // Parse `mount` output; cheap and good enough.
        if let Ok(out) = std::process::Command::new("mount").output() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                // "/dev/disk3s1 on /Volumes/foo (apfs, ...)"
                let mut it = line.split_whitespace();
                let dev = it.next().unwrap_or("");
                if it.next() != Some("on") {
                    continue;
                }
                let mp = it.next().unwrap_or("");
                if let Some(name) = dev.strip_prefix("/dev/") {
                    for d in devs.iter_mut() {
                        if d.bsd_name == name {
                            d.mount_point = Some(mp.to_string());
                        }
                    }
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub use mac::list;

/// List optical-class devices (drives + discs).
pub fn list_optical() -> Vec<BlockDevice> {
    let all = list();
    all.into_iter()
        .filter(|d| d.optical || d.interconnect.as_deref() == Some("USB") && d.scsi_pdt == Some(5))
        .collect()
}

pub fn human_bytes(n: u64) -> String {
    const U: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1000.0 && i < U.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.2} {}", U[i])
    }
}
