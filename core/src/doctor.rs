//! System check: what is installed, and which capability each missing piece
//! blocks. Every "missing" line names the command that fixes it, so the
//! DVD/burning/menu limits are diagnosable instead of mysterious.

use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Capability {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct DoctorReport {
    pub version: String,
    pub checks: Vec<Check>,
    pub capabilities: Vec<Capability>,
    pub errors: Vec<String>,
    /// Core stack present: libbluray + ffmpeg.
    pub ok: bool,
}

fn check(name: &str, ok: bool, detail: impl Into<String>, fix: Option<&str>) -> Check {
    Check {
        name: name.into(),
        ok,
        detail: detail.into(),
        fix: fix.map(str::to_string),
    }
}

fn in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

fn first_line(output: Option<std::process::Output>) -> Option<String> {
    output
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        })
        .filter(|s| !s.is_empty())
}

fn ffmpeg_demuxers(bin: &std::path::Path) -> String {
    std::process::Command::new(bin)
        .args(["-hide_banner", "-demuxers"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

pub fn run() -> DoctorReport {
    let mut r = DoctorReport {
        version: crate::version(),
        ..Default::default()
    };

    // libbluray
    let lb = crate::disc::library_version();
    let lb_ok = lb != "unavailable";
    r.checks.push(check(
        "libbluray",
        lb_ok,
        if lb_ok {
            lb.clone()
        } else {
            "not loaded".into()
        },
        Some(if cfg!(target_os = "macos") {
            "brew install libbluray"
        } else {
            "install libbluray (apt install libbluray2)"
        }),
    ));

    // libaacs
    let aacs = crate::disc::libaacs_path();
    r.checks.push(check(
        "libaacs",
        aacs.is_some(),
        aacs.map(|p| {
            format!(
                "{p}{}",
                if cfg!(target_os = "macos") {
                    ".dylib"
                } else {
                    ".so"
                }
            )
        })
        .unwrap_or_else(|| "not installed".into()),
        Some(if cfg!(target_os = "macos") {
            "brew install libaacs"
        } else {
            "install libaacs (apt install libaacs0)"
        }),
    ));

    // libbdplus + VM data
    let bdplus = std::env::var("LIBBDPLUS_PATH")
        .ok()
        .or_else(|| crate::disc::libbdplus_path().map(|p| format!("{p}.dylib")));
    let vm = crate::bdplus::installed();
    r.checks.push(check(
        "libbdplus",
        bdplus.is_some(),
        match (&bdplus, vm) {
            (Some(p), true) => format!(
                "{p}{}, VM installed",
                if cfg!(target_os = "macos") {
                    ".dylib"
                } else {
                    ".so"
                }
            ),
            (Some(p), false) => format!(
                "{p}{}, VM data missing",
                if cfg!(target_os = "macos") {
                    ".dylib"
                } else {
                    ".so"
                }
            ),
            (None, _) => "not installed".into(),
        },
        Some(if vm {
            if cfg!(target_os = "macos") {
                "brew install libbdplus"
            } else {
                "install libbdplus (apt install libbdplus0)"
            }
        } else {
            "keepsake-atelier bdplus --install"
        }),
    ));

    // ffmpeg + the dvdvideo demuxer
    let ff = crate::rip::ffmpeg_path();
    let ff_version = first_line(
        std::process::Command::new(&ff)
            .arg("-version")
            .output()
            .ok(),
    )
    .unwrap_or_else(|| "not found".into());
    let demuxers = ffmpeg_demuxers(&ff);
    let dvdvideo = demuxers
        .lines()
        .any(|l| l.split_whitespace().any(|w| w == "dvdvideo"));
    let encoders = std::process::Command::new(&ff)
        .args(["-hide_banner", "-encoders"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let has = |name: &str| {
        encoders
            .lines()
            .any(|l| l.split_whitespace().any(|w| w == name))
    };
    let x264 = has("libx264");
    let x265 = has("libx265");
    let vt = has("hevc_videotoolbox") || has("h264_videotoolbox");
    r.checks.push(check(
        "ffmpeg",
        ff_version != "not found",
        format!(
            "{} (dvdvideo: {}, libx264: {}, libx265: {}, videotoolbox: {})",
            ff.display(),
            if dvdvideo { "yes" } else { "no" },
            if x264 { "yes" } else { "no" },
            if x265 { "yes" } else { "no" },
            if vt { "yes" } else { "no" }
        ),
        Some(if cfg!(target_os = "macos") {
            "run scripts/build-ffmpeg.sh, or brew install ffmpeg"
        } else {
            "install ffmpeg (apt install ffmpeg)"
        }),
    ));
    if !dvdvideo {
        r.checks.push(check(
            "ffmpeg dvdvideo",
            false,
            "this ffmpeg cannot read a VIDEO_TS tree",
            Some("install an ffmpeg built with --enable-libdvdread --enable-libdvdnav"),
        ));
    }

    // tsMuxeR
    let ts = crate::author::tsmuxer_path();
    let ts_version = ts
        .as_ref()
        .and_then(|p| first_line(std::process::Command::new(p).output().ok()))
        .unwrap_or_else(|| "not installed".into());
    r.checks.push(check(
        "tsMuxeR",
        ts.is_some(),
        match &ts {
            Some(p) => format!("{} ({ts_version})", p.display()),
            None => "not installed".into(),
        },
        Some("put tsMuxeR on PATH or in the keepsake-atelier data bin directory"),
    ));

    // KEYDB
    let kb = crate::keys::status();
    r.checks.push(check(
        "KEYDB",
        kb.exists,
        format!("{} entries ({} bytes)", kb.entries, kb.bytes),
        Some("keepsake-atelier keys update"),
    ));

    // Drive
    let drives = crate::device::list_optical();
    let drive = drives.iter().find_map(|d| {
        d.product
            .clone()
            .or_else(|| d.vendor.clone())
            .or_else(|| Some(d.bsd_name.clone()))
    });
    r.checks.push(check(
        "optical drive",
        !drives.is_empty(),
        drive.unwrap_or_else(|| "none detected".into()),
        Some("attach the drive directly (not through a hub), then rescan"),
    ));

    // Burning stack
    let native_burn = cfg!(target_os = "macos") && Path::new("/usr/bin/hdiutil").is_file();
    let growisofs = in_path("growisofs");
    let cdrecord = in_path("cdrecord");
    let powershell = in_path("powershell.exe").or_else(|| in_path("powershell"));
    let imapi = cfg!(target_os = "windows") && powershell.is_some();
    let burning = native_burn || imapi || growisofs.is_some() || cdrecord.is_some();
    r.checks.push(check(
        "burning",
        burning,
        if native_burn {
            format!(
                "hdiutil burn (native){}",
                if growisofs.is_some() {
                    "; growisofs fallback"
                } else {
                    ""
                }
            )
        } else if imapi {
            "IMAPI2 (native PowerShell)".into()
        } else if let Some(g) = &growisofs {
            g.display().to_string()
        } else if let Some(c) = &cdrecord {
            c.display().to_string()
        } else {
            "no writer front-end".into()
        },
        Some(if cfg!(target_os = "windows") {
            "IMAPI2 is built into Windows; connect a writer drive"
        } else if cfg!(target_os = "macos") {
            "macOS hdiutil (built in), or brew install dvd+rw-tools"
        } else {
            "install dvd+rw-tools (growisofs)"
        }),
    ));

    // Data directory
    let dir = crate::config_dir();
    let dir_ok = dir.is_dir() || std::fs::create_dir_all(&dir).is_ok();
    r.checks.push(check(
        "data directory",
        dir_ok,
        dir.display().to_string(),
        None,
    ));

    // Capability matrix
    let cap = |name: &str, ok: bool, detail: &str| Capability {
        name: name.into(),
        ok,
        detail: detail.into(),
    };
    r.capabilities.push(cap(
        "Blu-ray rip / remux",
        lb_ok && aacs.is_some() && ff_version != "not found",
        if aacs.is_some() {
            "libbluray + libaacs + ffmpeg"
        } else {
            "needs libaacs"
        },
    ));
    r.capabilities.push(cap(
        "BD+ rip",
        bdplus.is_some() && vm,
        if bdplus.is_some() && vm {
            "libbdplus with VM data"
        } else {
            "needs libbdplus and the VM data"
        },
    ));
    let helper_ok = if cfg!(all(target_os = "windows", target_env = "msvc")) {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("windows-libs").join("keepsake-aacs.dll")))
            .is_some_and(|p| p.is_file())
    } else {
        true
    };
    let native_keys = helper_ok;
    r.capabilities.push(cap(
        "Native key recovery",
        native_keys,
        if !native_keys {
            "keepsake-aacs.dll is missing from windows-libs; reinstall the app"
        } else if cfg!(target_os = "macos") {
            "IOKit + LibreDrive drive or KEYDB host certificate; VUK and AACS2 unit keys from the disc"
        } else if cfg!(target_os = "linux") {
            "SG_IO + LibreDrive drive or KEYDB host certificate; VUK and AACS2 unit keys from the disc"
        } else {
            "SCSI pass-through + LibreDrive drive or KEYDB host certificate; VUK and AACS2 unit keys from the disc"
        },
    ));
    r.capabilities.push(cap(
        "Encode H.264 / HEVC",
        x264 && x265,
        if x264 && x265 {
            "libx264 + libx265"
        } else {
            "the bundled ffmpeg is missing libx264/libx265"
        },
    ));
    r.capabilities.push(cap(
        "Authoring (single or multi-title)",
        ts.is_some(),
        if ts.is_some() {
            "tsMuxeR"
        } else {
            "needs tsMuxeR"
        },
    ));
    r.capabilities.push(cap(
        "DVD (VIDEO_TS) rip",
        dvdvideo,
        if dvdvideo {
            "dvdvideo; MPEG-2 and AC-3/DTS copied to MKV"
        } else {
            "needs an ffmpeg with libdvdread/libdvdnav"
        },
    ));
    r.capabilities.push(cap(
        "Burning to disc",
        burning && !drives.is_empty(),
        if burning && !drives.is_empty() {
            if cfg!(target_os = "windows") {
                "IMAPI2 + an optical drive"
            } else if cfg!(target_os = "macos") {
                "hdiutil burn + an optical drive"
            } else {
                "growisofs + an optical drive"
            }
        } else if burning {
            "writer front-end ready; connect a writer drive"
        } else {
            "needs a writer front-end and a writer drive"
        },
    ));
    r.capabilities.push(cap(
        "BDMV verification",
        true,
        "built in (keepsake-atelier verify)",
    ));

    r.ok = lb_ok && ff_version != "not found";
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_the_core_stack_and_capabilities() {
        let r = run();
        assert!(!r.version.is_empty());
        assert!(r.checks.iter().any(|c| c.name == "libbluray"));
        assert!(r.checks.iter().any(|c| c.name == "ffmpeg"));
        assert!(r.capabilities.iter().any(|c| c.name.starts_with("DVD")));
        // The capability matrix always covers the main paths.
        for want in [
            "Blu-ray rip",
            "BD+ rip",
            "Native key recovery",
            "Authoring",
            "DVD",
            "Burning",
            "BDMV verification",
        ] {
            assert!(
                r.capabilities.iter().any(|c| c.name.starts_with(want)),
                "missing capability {want}"
            );
        }
    }
}
