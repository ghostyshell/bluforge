use std::env;

fn main() {
    println!("cargo:rerun-if-changed=src/bluray_shim.c");
    println!("cargo:rerun-if-changed=src/aacs_mmc.c");
    println!("cargo:rerun-if-changed=src/aacs_native.c");
    println!("cargo:rerun-if-env-changed=BLURAY_PREFIX");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    // Try to locate libbluray (with libaacs/libbdplus as its runtime deps).
    let probe = if target_os == "macos" {
        let prefix = env::var("BLURAY_PREFIX").unwrap_or_else(|_| "/opt/homebrew".into());
        Ok::<(Vec<String>, Vec<String>, Vec<String>), std::io::Error>((
            vec![format!("{prefix}/include")],
            vec![format!("{prefix}/lib")],
            vec!["bluray".into()],
        ))
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
                b.flag_if_supported("-fobjc-arc");
            }
            for inc in &includes {
                b.include(inc);
            }
            b.warnings(false);
            b.compile("bfbluray");

            for path in &link_paths {
                println!("cargo:rustc-link-search=native={path}");
                if target_os == "macos" || target_os == "linux" {
                    println!("cargo:rustc-link-arg=-Wl,-rpath,{path}");
                }
            }
            for lib in &libs {
                println!("cargo:rustc-link-lib=dylib={lib}");
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
}
