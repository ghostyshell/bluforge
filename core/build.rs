use std::env;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=src/bluray_shim.c");
    println!("cargo:rerun-if-changed=src/aacs_mmc.c");
    println!("cargo:rerun-if-changed=src/aacs_native.c");
    println!("cargo:rerun-if-env-changed=BLURAY_PREFIX");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    // Try to locate libbluray (with libaacs/libbdplus as its runtime deps).
    let mut vcpkg_build = false;
    let probe = if target_os == "macos" {
        let prefix = env::var("BLURAY_PREFIX").unwrap_or_else(|_| {
            if Path::new("/opt/homebrew/include/libbluray").is_dir() {
                "/opt/homebrew".into()
            } else {
                "/usr/local".into()
            }
        });
        Ok::<(Vec<String>, Vec<String>, Vec<String>), std::io::Error>((
            vec![format!("{prefix}/include")],
            vec![format!("{prefix}/lib")],
            vec!["bluray".into()],
        ))
    } else if target_os == "windows" && env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // The MSVC app takes a static libbluray from vcpkg. find_package emits
        // the link directives itself; the extra link emission below is skipped.
        match vcpkg::Config::new().find_package("libbluray") {
            Ok(l) => {
                vcpkg_build = true;
                Ok((
                    l.include_paths
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect(),
                    Vec::new(),
                    Vec::new(),
                ))
            }
            Err(e) => Err(std::io::Error::other(e.to_string())),
        }
    } else if target_os == "windows" {
        // The MinGW CLI links the MSYS2 libbluray through pkg-config, exactly
        // like Linux. This keeps disc support in the Windows CLI zip.
        match pkg_config::Config::new().probe("libbluray") {
            Ok(l) => Ok((
                l.include_paths
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect(),
                l.link_paths
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect(),
                l.libs.clone(),
            )),
            Err(e) => Err(std::io::Error::other(e.to_string())),
        }
    } else {
        match pkg_config::Config::new().probe("libbluray") {
            Ok(l) => Ok((
                l.include_paths
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect(),
                l.link_paths
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect(),
                l.libs.clone(),
            )),
            Err(e) => Err(std::io::Error::other(e.to_string())),
        }
    };

    println!("cargo::rustc-check-cfg=cfg(have_bluray)");

    match probe {
        Ok((includes, link_paths, libs)) => {
            let mut b = cc::Build::new();
            b.file("src/bluray_shim.c");
            if target_os == "macos" {
                // AACS volume-ID read (libaacs cannot reach the drive on macOS).
                b.file("src/aacs_mmc.c");
                // Native auth + MKB derivation, reusing libaacs's inner crypto.
                b.file("src/aacs_native.c");
                b.file("src/aacs_key.c");
                b.flag_if_supported("-fobjc-arc");
            }
            for inc in &includes {
                b.include(inc);
            }
            b.warnings(false);
            b.compile("bfbluray");

            if !vcpkg_build {
                for path in &link_paths {
                    println!("cargo:rustc-link-search=native={path}");
                    if target_os == "macos" || target_os == "linux" {
                        println!("cargo:rustc-link-arg=-Wl,-rpath,{path}");
                    }
                }
                for lib in &libs {
                    println!("cargo:rustc-link-lib=dylib={lib}");
                }
            }
            if target_os == "macos" {
                println!("cargo:rustc-link-lib=framework=IOKit");
                println!("cargo:rustc-link-lib=framework=CoreFoundation");
                // libaacs internals (crypto/MKB/MMC) linked statically for the
                // native keyless path.
                println!("cargo:rustc-link-lib=static=aacs");
                println!("cargo:rustc-link-lib=dylib=gcrypt");
                println!("cargo:rustc-link-lib=dylib=gpg-error");
                println!("cargo:rustc-link-lib=framework=DiskArbitration");
            }
            println!("cargo:rustc-cfg=have_bluray");
        }
        Err(e) => {
            println!("cargo:warning=libbluray not found, disc features disabled: {e}");
        }
    }

    // Portable AACS crypto for Linux and Windows. It does not depend on
    // libbluray being present, so it is built outside the probe above.
    let msvc = target_os == "windows" && env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if target_os != "macos" && !msvc && env::var_os("KEEPSAKE_SKIP_AACS_C").is_none() {
        // KEEPSAKE_SKIP_AACS_C is a type-check escape hatch: it compiles the
        // Rust side without the vendored C when no gcrypt headers are present.
        // MSVC uses the MinGW-built keepsake-aacs.dll instead of compiling
        // this C: vcpkg's libgcrypt port does not support MSVC.
        let mut a = cc::Build::new();
        a.include("third_party/libaacs");
        a.file("src/aacs_key.c");
        a.file("third_party/libaacs/libaacs/crypto.c");
        a.file("third_party/libaacs/libaacs/mkb.c");
        a.file("third_party/libaacs/util/strutl.c");
        a.file("third_party/libaacs/util/logging_stub.c");
        a.warnings(false);
        a.compile("keepsake_aacs");
        println!("cargo:rerun-if-changed=src/aacs_key.c");
        println!("cargo:rerun-if-changed=third_party/libaacs");

        println!("cargo:rustc-link-lib=dylib=gcrypt");
        println!("cargo:rustc-link-lib=dylib=gpg-error");
    }
}
