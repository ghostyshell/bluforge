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

fn b64(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len() * 4 / 3 + 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Cover search across the sources.
#[tauri::command]
fn covers_search(query: String, source: String, limit: usize) -> Vec<bluforge_core::CoverHit> {
    let cfg = AppConfig::load();
    let sources: Vec<bluforge_core::CoverSource> = if source == "all" {
        bluforge_core::CoverSource::all().to_vec()
    } else {
        bluforge_core::CoverSource::parse(&source)
            .map(|s| vec![s])
            .unwrap_or_else(|| bluforge_core::CoverSource::all().to_vec())
    };
    let mut out = Vec::new();
    for s in sources {
        if let Ok(mut r) = bluforge_core::covers::search(s, &query, limit, &cfg.hirescovers_dir) {
            out.append(&mut r);
        }
    }
    out
}

/// Fetch a cover image as a data: URL so the webview can show it without CORS
/// or referer problems.
#[tauri::command]
fn cover_thumb(url: String) -> Option<String> {
    let cfg = AppConfig::load();
    let cookie = bluforge_core::covers::cookie(&cfg);
    let (bytes, ctype) = bluforge_core::covers::fetch(&url, cookie.as_deref()).ok()?;
    if bytes.len() > 6_000_000 {
        return None;
    }
    let mime = if ctype.contains("png") { "image/png" } else { "image/jpeg" };
    Some(format!("data:{mime};base64,{}", b64(&bytes)))
}

/// Download one cover into `dest_dir` (defaults to the configured output dir).
#[derive(Serialize)]
struct SavedCover {
    role: String,
    path: String,
    original: bool,
    watermarked: bool,
    width: u32,
    height: u32,
}

/// One chosen image: which role it is for, and where it came from.
#[derive(serde::Deserialize)]
struct SaveItem {
    /// "cover" | "poster" | "disc"
    role: String,
    source: String,
    id: String,
    title: Option<String>,
}

/// Save the chosen cover / poster / disc label into `dest_dir`, named after the
/// rip so they sit alongside it.
#[tauri::command]
fn save_covers(dest_dir: String, stem: String, items: Vec<SaveItem>) -> Result<Vec<SavedCover>, String> {
    let cfg = AppConfig::load();
    let cookie = bluforge_core::covers::cookie(&cfg);
    let dir = std::path::PathBuf::from(&dest_dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for it in items {
        let src = bluforge_core::CoverSource::parse(&it.source).ok_or("unknown source")?;
        let hit = bluforge_core::covers::by_id(src, &it.id, &cfg.hirescovers_dir)
            .map_err(|e| e.to_string())?
            .or_else(|| {
                let q = it.title.clone()?;
                bluforge_core::covers::search(src, &q, 60, &cfg.hirescovers_dir)
                    .ok()?
                    .into_iter()
                    .find(|h| h.id == it.id)
            })
            .ok_or_else(|| format!("{}:{} not found", it.source, it.id))?;
        let name = bluforge_core::covers::role_filename(&it.role, &stem);
        let saved = bluforge_core::covers::download(&hit, &dir.join(name), cookie.as_deref())
            .map_err(|e| e.to_string())?;
        out.push(SavedCover {
            role: it.role,
            path: saved.path.display().to_string(),
            original: saved.original,
            watermarked: saved.info.watermarked,
            width: saved.info.width,
            height: saved.info.height,
        });
    }
    Ok(out)
}

/// Detect the episodes on a TV/anime disc (clip boundaries + TMDB titles).
#[tauri::command]
fn tv_plan(source: String) -> Result<bluforge_core::tv::Plan, String> {
    let disc = bluforge_core::Disc::open(std::path::Path::new(&source), None).map_err(|e| e.to_string())?;
    let info = disc.info().map_err(|e| e.to_string())?;
    bluforge_core::tv::plan(&disc, &info, None).map_err(|e| e.to_string())
}

#[tauri::command]
fn eject() -> Result<String, String> {
    bluforge_core::device::eject().map_err(|e| e.to_string())
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
            covers_search,
            cover_thumb,
            save_covers,
            preview_output,
            eject,
            tv_plan,
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
