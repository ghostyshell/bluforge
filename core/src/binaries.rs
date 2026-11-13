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
    let bin = crate::config_dir().join("bin").join(name);
    v.push(bin.clone());
    if cfg!(target_os = "windows") && Path::new(name).extension().is_none() {
        v.push(bin.with_extension("exe"));
    }
    v
}

/// Look a helper up on `PATH`, including `.exe` on Windows.
fn in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let mut names = vec![name.to_string()];
    if cfg!(target_os = "windows") && Path::new(name).extension().is_none() {
        names.insert(0, format!("{name}.exe"));
    }
    std::env::split_paths(&path)
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .find(|p| p.is_file())
}

/// First existing helper named `name`, if any. Bundled and data-directory
/// copies win over `PATH`.
pub fn helper(name: &str) -> Option<PathBuf> {
    candidates(name)
        .into_iter()
        .find(|p| p.is_file())
        .or_else(|| in_path(name))
}

/// The helper's path as a string, for code that builds command lines.
pub fn helper_str(name: &str) -> Option<String> {
    helper(name).map(|p| p.display().to_string())
}

/// Drain a child's stderr into tracing, one line per event, so a caller that
/// does not read it cannot dead-lock on a full pipe and the app can show it.
/// The thread ends when the child closes stderr.
pub fn drain_stderr(
    mut stderr: std::process::ChildStderr,
    label: &'static str,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(&mut stderr);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            let line = line.trim_end().to_string();
            if line.is_empty() {
                continue;
            }
            if line.contains("Error") || line.contains("error") || line.contains("failed") {
                tracing::error!(target: "process", process = label, "{line}");
            } else {
                tracing::info!(target: "process", process = label, "{line}");
            }
        }
    })
}

/// Take the child's stderr and start draining it. Returns the drain thread.
pub fn capture_stderr(
    child: &mut std::process::Child,
    label: &'static str,
) -> Option<std::thread::JoinHandle<()>> {
    child.stderr.take().map(|s| drain_stderr(s, label))
}
