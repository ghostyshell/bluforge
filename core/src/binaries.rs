//! Locate the helper binaries (ffmpeg / ffprobe).
//!
//! Priority: an explicit `KEEPSAKE_*` override, a binary bundled with the app
//! (`.app/Contents/Resources`, next to the executable, or the dev resource
//! directory), a copy in the data directory, then `PATH`.

use std::path::{Path, PathBuf};

fn candidates(name: &str) -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        let dir = exe.parent().unwrap_or(Path::new(".")).to_path_buf();
        v.push(dir.join(name));
        v.push(format!("{name}.exe").into());
        v.push(dir.join("resources").join(name));
        v.push(dir.join("resources").join(format!("{name}.exe")));
        // macOS bundle: Contents/MacOS/<exe> -> Contents/Resources/<name>
        if let Some(contents) = dir.parent() {
            v.push(contents.join("Resources").join(name));
        }
        // dev tree: target/debug -> <repo>/app/src-tauri/resources and vendor
        if let Some(repo) = dir.parent().and_then(|p| p.parent()) {
            v.push(repo.join("app/src-tauri/resources").join(name));
            v.push(repo.join("core/assets/menu").join(name));
            v.push(repo.join("vendor/ffmpeg").join(name));
        }
        // Linux installs (deb/FHS and AppImage) put resources in
        // /usr/lib/<product>/ while the binary is in /usr/bin.
        if let Some(prefix) = dir.parent() {
            v.push(prefix.join("lib").join("Keepsake Atelier").join(name));
            v.push(prefix.join("lib").join("keepsake-atelier").join(name));
        }
    }
    v.push(crate::config_dir().join("bin").join(name));
    v
}

/// First existing helper named `name`, if any.
pub fn helper(name: &str) -> Option<PathBuf> {
    candidates(name).into_iter().find(|p| p.is_file())
}

/// The helper's path as a string, for code that builds command lines.
pub fn helper_str(name: &str) -> Option<String> {
    helper(name).map(|p| p.display().to_string())
}
