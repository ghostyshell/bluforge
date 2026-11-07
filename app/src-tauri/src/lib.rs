use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use bluforge_core::{config::AppConfig, disc::DiscInfo, iso, keys::KeyDbStatus, rip, CoreSnapshot};

struct RpState {
    cancel: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
}

#[derive(Serialize)]
struct UpdateInfo {
    version: Option<String>,
    notes: Option<String>,
}

/// Values the UI has to hand when previewing an output path.
#[derive(serde::Deserialize)]
struct PreviewVars {
    output_dir: String,
    disc: String,
    title: String,
    year: Option<String>,
    tmdb: Option<u64>,
    resolution: Option<String>,
    hdr: Option<String>,
    video_codec: Option<String>,
    audio_lang: Option<String>,
    audio_codec: Option<String>,
    runtime_secs: f64,
    chapters: usize,
    playlist: u32,
    index: u32,
    ext: String,
}

/// Render an output path exactly as the ripper would.
#[tauri::command]
fn preview_output(template: String, vars: PreviewVars) -> String {
    let v = bluforge_core::TemplateVars::build(
        &vars.disc,
        &vars.title,
        vars.year.as_deref(),
        vars.tmdb,
        vars.resolution.as_deref(),
        vars.hdr.as_deref(),
        vars.video_codec.as_deref(),
        vars.audio_lang.as_deref(),
        vars.audio_codec.as_deref(),
        vars.runtime_secs,
        vars.chapters,
        vars.playlist,
        vars.index,
        &vars.ext,
    );
    let rel = bluforge_core::render_template(&template, &v, &vars.ext, "{title} - {resolution}");
    std::path::PathBuf::from(&vars.output_dir)
        .join(rel)
        .display()
        .to_string()
}

/// Placeholders offered for the output template: `(name, description)`.
#[tauri::command]
fn template_placeholders() -> Vec<(String, String)> {
    bluforge_core::template::PLACEHOLDERS
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

#[derive(Serialize)]
struct MovieHit {
    title: String,
    year: Option<String>,
    tmdb_id: u64,
}

/// TMDB lookup for the template preview.
#[tauri::command]
fn tmdb_lookup(movie: String, runtime_secs: f64) -> Option<MovieHit> {
    bluforge_core::lookup_movie(&movie, runtime_secs)
        .ok()
        .flatten()
        .map(|m| MovieHit {
            title: m.title,
            year: m.year,
            tmdb_id: m.tmdb_id,
        })
}

#[tauri::command]
fn snapshot() -> CoreSnapshot {
    bluforge_core::snapshot()
}

#[tauri::command]
fn keys_status() -> KeyDbStatus {
    bluforge_core::keys::status()
}

#[tauri::command]
fn keys_update() -> Result<KeyDbStatus, String> {
    bluforge_core::keys::update().map_err(|e| e.to_string())
}

#[tauri::command]
fn config_get() -> AppConfig {
    AppConfig::load()
}

#[tauri::command]
fn config_set(config: AppConfig) -> Result<(), String> {
    config.save().map_err(|e| e.to_string())
}

#[tauri::command]
fn open_disc(path: String) -> Result<DiscInfo, String> {
    let p = PathBuf::from(&path);
    let disc = bluforge_core::Disc::open(&p, Some(&bluforge_core::keys::keydb_path()))
        .map_err(|e| e.to_string())?;
    let mut info = disc.info().map_err(|e| e.to_string())?;
    info.path = path.clone();
    info.name = bluforge_core::device::source_name(&path, &info.name);
    Ok(info)
}

#[tauri::command]
fn start_rip(
    app: AppHandle,
    state: State<RpState>,
    request: rip::RipRequest,
) -> Result<(), String> {
    if state.running.load(Ordering::Relaxed) {
        return Err("a rip is already running".into());
    }
    state.cancel.store(false, Ordering::Relaxed);
    state.running.store(true, Ordering::Relaxed);
    let cancel = state.cancel.clone();
    let running = state.running.clone();
    let app2 = app.clone();
    std::thread::spawn(move || {
        let res = rip::run(&request, &cancel, |p| {
            let _ = app2.emit("rip://progress", p);
        });
        running.store(false, Ordering::Relaxed);
        if let Err(e) = res {
            let _ = app2.emit("rip://error", e.to_string());
        }
    });
    Ok(())
}

#[tauri::command]
fn cancel_rip(state: State<RpState>) {
    state.cancel.store(true, Ordering::Relaxed);
}

#[tauri::command]
fn export_iso(app: AppHandle, request: iso::IsoRequest) -> Result<(), String> {
    let app2 = app.clone();
    std::thread::spawn(move || {
        let res = iso::run(&request, |p| {
            let _ = app2.emit("iso://progress", p);
        });
        match res {
            Ok(()) => {
                let _ = app2.emit("iso://done", ());
            }
            Err(e) => {
                let _ = app2.emit("iso://error", e.to_string());
            }
        }
    });
    Ok(())
}

#[tauri::command]
async fn check_update(app: AppHandle) -> Result<UpdateInfo, String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| e.to_string())?;
    match updater.check().await {
        Ok(Some(u)) => Ok(UpdateInfo {
            version: Some(u.version.clone()),
            notes: u.body.clone(),
        }),
        Ok(None) => Ok(UpdateInfo {
            version: None,
            notes: None,
        }),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| e.to_string())?;
    if let Some(u) = updater.check().await.map_err(|e| e.to_string())? {
        u.download_and_install(|_, _| {}, || {})
            .await
            .map_err(|e| e.to_string())?;
        app.restart();
    }
    Ok(())
}

#[tauri::command]
fn reveal(app: AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let p = PathBuf::from(&path);
    let dir = if p.is_dir() {
        p
    } else {
        p.parent().map(|d| d.to_path_buf()).unwrap_or(p)
    };
    app.opener()
        .reveal_item_in_dir(dir)
        .map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(RpState {
            cancel: Arc::new(AtomicBool::new(false)),
            running: Arc::new(AtomicBool::new(false)),
        })
        .invoke_handler(tauri::generate_handler![
            snapshot,
            keys_status,
            keys_update,
            config_get,
            config_set,
            open_disc,
            start_rip,
            cancel_rip,
            export_iso,
            check_update,
            install_update,
            reveal,
            preview_output,
            template_placeholders,
            tmdb_lookup
        ])
        .setup(|app| {
            let _ = app.get_webview_window("main");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running BluForge");
}
