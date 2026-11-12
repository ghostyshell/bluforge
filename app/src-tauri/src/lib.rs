use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use keepsake_core::{config::AppConfig, disc::DiscInfo, iso, keys::KeyDbStatus, rip, CoreSnapshot};

mod mcp;

/// Where the MCP server is listening, if it is.
#[tauri::command]
fn mcp_status() -> serde_json::Value {
    let cfg = AppConfig::load();
    serde_json::json!({
        "enabled": cfg.mcp_enabled,
        "port": mcp::running_port(),
    })
}

struct RpState {
    cancel: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    queue: Arc<Mutex<QueueState>>,
}

/// A queued rip. `request` is the exact `RipRequest` handed to the ripper.
#[derive(Clone, Serialize, Deserialize)]
struct QueueJob {
    id: u64,
    request: rip::RipRequest,
    /// queued | running | done | error | cancelled
    state: String,
    percent: f64,
    message: String,
    dest: Option<String>,
}

#[derive(Serialize)]
struct QueueJobView {
    id: u64,
    title: Option<u32>,
    source: String,
    output: Option<String>,
    state: String,
    percent: f64,
    message: String,
    dest: Option<String>,
}

impl QueueJob {
    fn view(&self) -> QueueJobView {
        QueueJobView {
            id: self.id,
            title: self.request.title,
            source: self.request.source.display().to_string(),
            output: self
                .request
                .output
                .as_ref()
                .map(|p| p.display().to_string()),
            state: self.state.clone(),
            percent: self.percent,
            message: self.message.clone(),
            dest: self.dest.clone(),
        }
    }
}

/// In-memory rip queue. Pure state transitions with no threads or events, so it
/// can be unit-tested; `pump_queue` owns the running thread.
#[derive(Default)]
struct QueueState {
    jobs: Vec<QueueJob>,
    next_id: u64,
}

impl QueueState {
    fn enqueue(&mut self, request: rip::RipRequest) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.jobs.push(QueueJob {
            id,
            request,
            state: "queued".into(),
            percent: 0.0,
            message: "waiting".into(),
            dest: None,
        });
        id
    }

    /// Claim the first queued job, marking it running. Returns `None` while
    /// another job is running, so one ripper thread at a time is an invariant
    /// of the queue itself.
    fn start_next(&mut self) -> Option<(u64, rip::RipRequest)> {
        if self.jobs.iter().any(|j| j.state == "running") {
            return None;
        }
        let job = self.jobs.iter_mut().find(|j| j.state == "queued")?;
        job.state = "running".into();
        job.percent = 0.0;
        job.message = "starting".into();
        Some((job.id, job.request.clone()))
    }

    fn update(&mut self, id: u64, percent: f64, message: &str, dest: Option<String>) {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == id) {
            job.percent = percent;
            job.message = message.to_string();
            if dest.is_some() {
                job.dest = dest;
            }
        }
    }

    fn finish(&mut self, id: u64, error: Option<String>, cancelled: bool) {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == id) {
            match error {
                None => {
                    job.state = "done".into();
                    job.percent = 100.0;
                    job.message = "done".into();
                }
                Some(e) => {
                    job.state = if cancelled { "cancelled" } else { "error" }.into();
                    job.message = e;
                }
            }
        }
    }

    /// Cancel a job. Returns true when it was running, so the caller can trip
    /// the ripper's cancel flag.
    fn cancel(&mut self, id: u64) -> bool {
        let Some(job) = self.jobs.iter_mut().find(|j| j.id == id) else {
            return false;
        };
        match job.state.as_str() {
            "running" => {
                job.message = "cancelling".into();
                true
            }
            "queued" => {
                job.state = "cancelled".into();
                job.message = "cancelled".into();
                false
            }
            _ => false,
        }
    }

    fn remove(&mut self, id: u64) {
        self.jobs.retain(|j| !(j.id == id && j.state != "running"));
    }

    fn clear_finished(&mut self) {
        self.jobs
            .retain(|j| j.state == "queued" || j.state == "running");
    }

    fn views(&self) -> Vec<QueueJobView> {
        self.jobs.iter().map(|j| j.view()).collect()
    }

    fn snapshot(&self) -> Vec<QueueJob> {
        self.jobs.clone()
    }

    /// Replace the queue from a saved snapshot. Anything that was running when
    /// the app stopped is put back in the queue; `next_id` never goes backwards.
    fn restore(&mut self, mut jobs: Vec<QueueJob>) {
        for job in &mut jobs {
            if job.state == "running" {
                job.state = "queued".into();
                job.message = "waiting".into();
            }
        }
        self.next_id = jobs.iter().map(|j| j.id).max().unwrap_or(0);
        self.jobs = jobs;
    }
}

fn queue_path() -> std::path::PathBuf {
    keepsake_core::config_dir().join("queue.json")
}

/// Write the queue snapshot atomically. Called on state transitions, not on
/// every progress tick.
fn persist_queue(state: &RpState) {
    let jobs = match state.queue.lock() {
        Ok(q) => q.snapshot(),
        Err(_) => return,
    };
    let path = queue_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Ok(text) = serde_json::to_string_pretty(&jobs) else {
        return;
    };
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, text + "\n").is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

fn load_queue() -> Vec<QueueJob> {
    let Ok(text) = std::fs::read_to_string(queue_path()) else {
        return Vec::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn pump_queue(app: AppHandle, state: &RpState) {
    if state.running.load(Ordering::Relaxed) {
        return;
    }
    let (id, request) = {
        let Ok(mut q) = state.queue.lock() else {
            return;
        };
        let Some(next) = q.start_next() else {
            return;
        };
        next
    };
    state.cancel.store(false, Ordering::Relaxed);
    state.running.store(true, Ordering::Relaxed);
    persist_queue(state);
    let _ = app.emit("queue://changed", ());

    let app2 = app.clone();
    let cancel = state.cancel.clone();
    let running = state.running.clone();
    let queue = state.queue.clone();
    std::thread::spawn(move || {
        let res = rip::run(&request, &cancel, |p| {
            if let Ok(mut q) = queue.lock() {
                q.update(
                    id,
                    p.percent,
                    &p.message,
                    p.dest.as_ref().map(|d| d.display().to_string()),
                );
            }
            let _ = app2.emit("rip://progress", p.clone());
            let _ = app2.emit(
                "queue://progress",
                serde_json::json!({ "id": id, "progress": p }),
            );
        });
        running.store(false, Ordering::Relaxed);
        let cancelled = cancel.load(Ordering::Relaxed);
        if let Ok(mut q) = queue.lock() {
            q.finish(id, res.as_ref().err().map(|e| e.to_string()), cancelled);
        }
        if let Err(e) = &res {
            if !cancelled {
                let _ = app2.emit("rip://error", e.to_string());
            }
        }
        let _ = app2.emit("queue://changed", ());
        if let Some(st) = app2.try_state::<RpState>() {
            persist_queue(&st);
            pump_queue(app2.clone(), &st);
        }
    });
}

#[tauri::command]
fn queue_add(
    app: AppHandle,
    state: State<RpState>,
    request: rip::RipRequest,
) -> Result<u64, String> {
    let id = {
        let mut q = state.queue.lock().map_err(|_| "queue lock poisoned")?;
        q.enqueue(request)
    };
    persist_queue(&state);
    let _ = app.emit("queue://changed", ());
    pump_queue(app, &state);
    Ok(id)
}

#[tauri::command]
fn queue_list(state: State<RpState>) -> Vec<QueueJobView> {
    state.queue.lock().map(|q| q.views()).unwrap_or_default()
}

#[tauri::command]
fn queue_cancel(state: State<RpState>, id: u64) {
    let running = state
        .queue
        .lock()
        .map(|mut q| q.cancel(id))
        .unwrap_or(false);
    if running {
        state.cancel.store(true, Ordering::Relaxed);
    }
    persist_queue(&state);
}

#[tauri::command]
fn queue_remove(state: State<RpState>, id: u64) {
    if let Ok(mut q) = state.queue.lock() {
        q.remove(id);
    }
    persist_queue(&state);
}

#[tauri::command]
fn queue_clear(state: State<RpState>) {
    if let Ok(mut q) = state.queue.lock() {
        q.clear_finished();
    }
    persist_queue(&state);
}

/// Run the restored queue after an app restart. Jobs are never auto-started on
/// launch, so a restart cannot begin ripping before the user asks.
#[tauri::command]
fn queue_resume(app: AppHandle, state: State<RpState>) {
    pump_queue(app, &state);
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
    let v = keepsake_core::TemplateVars::build(
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
    let rel = keepsake_core::render_template(&template, &v, &vars.ext, "{title} - {resolution}");
    std::path::PathBuf::from(&vars.output_dir)
        .join(rel)
        .display()
        .to_string()
}

/// Placeholders offered for the output template: `(name, description)`.
#[tauri::command]
fn template_placeholders() -> Vec<(String, String)> {
    keepsake_core::template::PLACEHOLDERS
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
    keepsake_core::lookup_movie(&movie, runtime_secs)
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
fn covers_search(query: String, source: String, limit: usize) -> Vec<keepsake_core::CoverHit> {
    let cfg = AppConfig::load();
    let sources: Vec<keepsake_core::CoverSource> = if source == "all" {
        keepsake_core::CoverSource::all().to_vec()
    } else {
        keepsake_core::CoverSource::parse(&source)
            .map(|s| vec![s])
            .unwrap_or_else(|| keepsake_core::CoverSource::all().to_vec())
    };
    let mut out = Vec::new();
    for s in sources {
        if let Ok(mut r) = keepsake_core::covers::search(s, &query, limit, &cfg.hirescovers_dir) {
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
    let cookie = keepsake_core::covers::cookie(&cfg);
    let (bytes, ctype) = keepsake_core::covers::fetch(&url, cookie.as_deref()).ok()?;
    if bytes.len() > 6_000_000 {
        return None;
    }
    let mime = if ctype.contains("png") {
        "image/png"
    } else {
        "image/jpeg"
    };
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
fn save_covers(
    dest_dir: String,
    stem: String,
    items: Vec<SaveItem>,
) -> Result<Vec<SavedCover>, String> {
    let cfg = AppConfig::load();
    let cookie = keepsake_core::covers::cookie(&cfg);
    let dir = std::path::PathBuf::from(&dest_dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for it in items {
        let src = keepsake_core::CoverSource::parse(&it.source).ok_or("unknown source")?;
        let hit = keepsake_core::covers::by_id(src, &it.id, &cfg.hirescovers_dir)
            .map_err(|e| e.to_string())?
            .or_else(|| {
                let q = it.title.clone()?;
                keepsake_core::covers::search(src, &q, 60, &cfg.hirescovers_dir)
                    .ok()?
                    .into_iter()
                    .find(|h| h.id == it.id)
            })
            .ok_or_else(|| format!("{}:{} not found", it.source, it.id))?;
        let name = keepsake_core::covers::role_filename(&it.role, &stem);
        let saved = keepsake_core::covers::download(&hit, &dir.join(name), cookie.as_deref())
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
fn tv_plan(source: String, start: Option<u32>) -> Result<keepsake_core::tv::Plan, String> {
    let disc = keepsake_core::Disc::open(std::path::Path::new(&source), None)
        .map_err(|e| e.to_string())?;
    let info = disc.info().map_err(|e| e.to_string())?;
    keepsake_core::tv::plan(&disc, &info, None, start).map_err(|e| e.to_string())
}

#[tauri::command]
fn eject() -> Result<String, String> {
    keepsake_core::device::eject().map_err(|e| e.to_string())
}

#[tauri::command]
fn snapshot() -> CoreSnapshot {
    keepsake_core::snapshot()
}

#[tauri::command]
fn keys_status() -> KeyDbStatus {
    keepsake_core::keys::status()
}

#[tauri::command]
fn keys_update() -> Result<KeyDbStatus, String> {
    keepsake_core::keys::update().map_err(|e| e.to_string())
}

#[tauri::command]
fn config_get() -> AppConfig {
    AppConfig::load()
}

#[tauri::command]
fn config_set(config: AppConfig) -> Result<(), String> {
    config.save().map_err(|e| e.to_string())?;
    // Starts or stops the MCP listener to match the setting.
    mcp::apply(&config);
    Ok(())
}

#[tauri::command]
fn open_disc(path: String) -> Result<DiscInfo, String> {
    let p = PathBuf::from(&path);
    let disc = keepsake_core::Disc::open(&p, Some(&keepsake_core::keys::keydb_path()))
        .map_err(|e| e.to_string())?;
    let mut info = disc.info().map_err(|e| e.to_string())?;
    info.path = path.clone();
    info.name = keepsake_core::device::source_name(&path, &info.name);
    Ok(info)
}

/// Resolve a UI "disc source" to something the menu parser can read.
///
/// The UI often holds a raw device node (`/dev/rdiskN`), which has no BDMV
/// directory; fall back to the mounted optical volume in that case.
fn menu_source(source: &str) -> PathBuf {
    let p = PathBuf::from(source);
    if !source.is_empty() && keepsake_core::menu::bdmv_dir(&p).is_some() {
        return p;
    }
    if let Some(m) = keepsake_core::device::list_optical()
        .into_iter()
        .find_map(|d| d.mount_point)
    {
        return PathBuf::from(m);
    }
    p
}

#[tauri::command]
fn disc_streams(path: String, title: u32) -> Result<Vec<keepsake_core::disc::StreamInfo>, String> {
    let disc = keepsake_core::Disc::open(
        std::path::Path::new(&path),
        Some(&keepsake_core::keys::keydb_path()),
    )
    .map_err(|e| e.to_string())?;
    disc.streams(title).map_err(|e| e.to_string())
}

#[tauri::command]
fn menu_parse(source: String) -> Result<keepsake_core::menu::DiscMenu, String> {
    keepsake_core::menu::parse(
        &menu_source(&source),
        Some(&keepsake_core::keys::keydb_path()),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn menu_list() -> Result<Vec<serde_json::Value>, String> {
    let dir = keepsake_core::menu::store_dir();
    Ok(keepsake_core::menu::list(&dir)
        .into_iter()
        .map(|(name, path)| {
            let doc = keepsake_core::menu::load(&dir, &name).ok();
            serde_json::json!({
                "name": name,
                "path": path.display().to_string(),
                "label": doc.as_ref().map(|d| d.label.clone()).unwrap_or_default(),
                "titles": doc.as_ref().map(|d| d.titles.len()).unwrap_or(0),
            })
        })
        .collect())
}

#[tauri::command]
fn menu_load(template: String) -> Result<keepsake_core::menu::DiscMenu, String> {
    keepsake_core::menu::load(&keepsake_core::menu::store_dir(), &template)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn menu_save(template: String, doc: keepsake_core::menu::DiscMenu) -> Result<String, String> {
    keepsake_core::menu::save(&keepsake_core::menu::store_dir(), &template, &doc)
        .map(|p| p.display().to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn menu_apply(
    template: String,
    target: String,
    save: Option<String>,
) -> Result<keepsake_core::menu::DiscMenu, String> {
    let dir = keepsake_core::menu::store_dir();
    let tpl = keepsake_core::menu::load(&dir, &template).map_err(|e| e.to_string())?;
    let target_doc = keepsake_core::menu::parse(
        &menu_source(&target),
        Some(&keepsake_core::keys::keydb_path()),
    )
    .map_err(|e| e.to_string())?;
    let doc = keepsake_core::menu::apply(&tpl, &target_doc, &template);
    if let Some(name) = save {
        keepsake_core::menu::save(&dir, &name, &doc).map_err(|e| e.to_string())?;
    }
    Ok(doc)
}

#[tauri::command]
fn menu_remove(template: String) -> Result<String, String> {
    keepsake_core::menu::remove(&keepsake_core::menu::store_dir(), &template)
        .map(|p| p.display().to_string())
        .map_err(|e| e.to_string())
}

/// Derive and store a key for the inserted disc, natively (LibreDrive VID +
/// the disc's own MKB + the KEYDB device keys). No MakeMKV, no certificate.
#[tauri::command]
fn keys_native(source: Option<String>) -> Result<serde_json::Value, String> {
    let mount = menu_source(source.as_deref().unwrap_or(""));
    let (key, added) =
        keepsake_core::native::derive_and_store(&mount).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "key": key, "added": added }))
}

/// Install the public BD+ VM data so BD+ discs decrypt without MakeMKV.
/// The KEYDB entry for the inserted disc, if one exists.
#[tauri::command]
fn keys_search(query: String, limit: usize) -> Result<Vec<keepsake_core::keys::KeyEntry>, String> {
    keepsake_core::keys::search_at(&keepsake_core::keys::keydb_path(), &query, limit)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn keys_remove(id: String) -> Result<bool, String> {
    keepsake_core::keys::remove_at(&keepsake_core::keys::keydb_path(), &id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn keys_import(path: String) -> Result<(usize, usize), String> {
    keepsake_core::keys::import_at(
        &keepsake_core::keys::keydb_path(),
        std::path::Path::new(&path),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn key_entry(source: String) -> Result<Option<keepsake_core::keys::KeyEntry>, String> {
    let mount = menu_source(&source);
    let Some(id) = keepsake_core::keys::disc_id_for_path(&mount) else {
        return Ok(None);
    };
    keepsake_core::keys::lookup_at(&keepsake_core::keys::keydb_path(), &id)
        .map_err(|e| e.to_string())
}

/// Replace the unit-key table of the inserted disc's entry.
#[tauri::command]
fn keys_set_units(
    source: String,
    keys: Vec<String>,
) -> Result<keepsake_core::keys::KeyEntry, String> {
    let mount = menu_source(&source);
    let id = keepsake_core::keys::disc_id_for_path(&mount)
        .ok_or("no AACS/Unit_Key_RO.inf on this disc")?;
    keepsake_core::keys::set_unit_keys_at(&keepsake_core::keys::keydb_path(), &id, keys)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn bdplus_install() -> Result<String, String> {
    keepsake_core::bdplus::install()
        .map(|p| p.display().to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn tsmuxer_install() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(keepsake_core::tsmuxer::install)
        .await
        .map_err(|e| e.to_string())?
        .map(|p| p.display().to_string())
        .map_err(|e| e.to_string())
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
fn burn_start(
    app: AppHandle,
    state: State<RpState>,
    request: keepsake_core::burn::BurnRequest,
) -> Result<(), String> {
    if state.running.load(Ordering::Relaxed) {
        return Err("a rip, clone, burn or DVD rip is already running".into());
    }
    state.cancel.store(false, Ordering::Relaxed);
    state.running.store(true, Ordering::Relaxed);
    let app2 = app.clone();
    let cancel = state.cancel.clone();
    let running = state.running.clone();
    std::thread::spawn(move || {
        let res = keepsake_core::burn::run(&request, &cancel, |p| {
            let _ = app2.emit("burn://progress", p);
        });
        running.store(false, Ordering::Relaxed);
        match res {
            Ok(()) => {
                let _ = app2.emit("burn://done", request.image.display().to_string());
            }
            Err(e) => {
                let _ = app2.emit("burn://error", e.to_string());
            }
        }
        if let Some(st) = app2.try_state::<RpState>() {
            pump_queue(app2.clone(), &st);
        }
    });
    Ok(())
}

#[tauri::command]
fn dvd_info(path: String) -> Result<keepsake_core::dvd::DvdInfo, String> {
    keepsake_core::dvd::info(std::path::Path::new(&path)).map_err(|e| e.to_string())
}

#[tauri::command]
fn dvd_rip(
    app: AppHandle,
    state: State<RpState>,
    request: keepsake_core::dvd::DvdRipRequest,
) -> Result<(), String> {
    if state.running.load(Ordering::Relaxed) {
        return Err("a rip, clone or DVD rip is already running".into());
    }
    state.cancel.store(false, Ordering::Relaxed);
    state.running.store(true, Ordering::Relaxed);
    let app2 = app.clone();
    let cancel = state.cancel.clone();
    let running = state.running.clone();
    std::thread::spawn(move || {
        let res = keepsake_core::dvd::rip(&request, &cancel, |p| {
            let _ = app2.emit("dvd://progress", p);
        });
        running.store(false, Ordering::Relaxed);
        match res {
            Ok(path) => {
                let _ = app2.emit("dvd://done", path.display().to_string());
            }
            Err(e) => {
                let _ = app2.emit("dvd://error", e.to_string());
            }
        }
        if let Some(st) = app2.try_state::<RpState>() {
            pump_queue(app2.clone(), &st);
        }
    });
    Ok(())
}

#[tauri::command]
fn doctor() -> keepsake_core::doctor::DoctorReport {
    keepsake_core::doctor::run()
}

#[tauri::command]
fn verify(path: String) -> Result<keepsake_core::verify::VerifyReport, String> {
    keepsake_core::verify::run(std::path::Path::new(&path)).map_err(|e| e.to_string())
}

#[tauri::command]
fn clone_start(
    app: AppHandle,
    state: State<RpState>,
    request: keepsake_core::clone::CloneRequest,
) -> Result<(), String> {
    if state.running.load(Ordering::Relaxed) {
        return Err("a rip or clone is already running".into());
    }
    state.cancel.store(false, Ordering::Relaxed);
    state.running.store(true, Ordering::Relaxed);
    let app2 = app.clone();
    let cancel = state.cancel.clone();
    let running = state.running.clone();
    std::thread::spawn(move || {
        let res = keepsake_core::clone::run(&request, &cancel, |p| {
            let _ = app2.emit("clone://progress", p);
        });
        running.store(false, Ordering::Relaxed);
        match res {
            Ok(bytes) => {
                let _ = app2.emit(
                    "clone://done",
                    serde_json::json!({
                        "bytes": bytes,
                        "dest": request.dest.display().to_string(),
                    }),
                );
            }
            Err(e) => {
                let _ = app2.emit("clone://error", e.to_string());
            }
        }
        // A clone can hold the drive while rips are queued; resume them.
        if let Some(st) = app2.try_state::<RpState>() {
            pump_queue(app2.clone(), &st);
        }
    });
    Ok(())
}

#[tauri::command]
fn author_start(
    app: AppHandle,
    request: keepsake_core::author::AuthorRequest,
) -> Result<(), String> {
    let app2 = app.clone();
    std::thread::spawn(move || {
        let res = keepsake_core::author::run(&request, |p| {
            let _ = app2.emit("author://progress", p);
        });
        match res {
            Ok(path) => {
                let _ = app2.emit("author://done", path.display().to_string());
            }
            Err(e) => {
                let _ = app2.emit("author://error", e.to_string());
            }
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
    // Core logs go through `tracing`; the app had no subscriber, so every
    // rip/disc log line was dropped. RUST_LOG=debug for detail.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

    // The MCP listener is off unless the saved config enables it.
    mcp::apply(&AppConfig::load());

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(RpState {
            cancel: Arc::new(AtomicBool::new(false)),
            running: Arc::new(AtomicBool::new(false)),
            queue: Arc::new(Mutex::new(QueueState::default())),
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
            author_start,
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
            tmdb_lookup,
            mcp_status,
            menu_parse,
            menu_list,
            menu_load,
            menu_save,
            menu_apply,
            menu_remove,
            keys_native,
            key_entry,
            keys_set_units,
            keys_search,
            keys_remove,
            keys_import,
            bdplus_install,
            tsmuxer_install,
            disc_streams,
            queue_add,
            queue_list,
            queue_cancel,
            queue_remove,
            queue_clear,
            queue_resume,
            clone_start,
            verify,
            doctor,
            dvd_info,
            dvd_rip,
            burn_start
        ])
        .setup(|app| {
            if let Some(st) = app.try_state::<RpState>() {
                if let Ok(mut q) = st.queue.lock() {
                    q.restore(load_queue());
                }
            }
            let _ = app.get_webview_window("main");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Keepsake Atelier");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(title: u32) -> rip::RipRequest {
        rip::RipRequest {
            source: "/tmp/disc".into(),
            keydb: None,
            title: Some(title),
            output: None,
            mode: rip::RipMode::Remux {
                container: rip::Container::Mkv,
            },
            chapters: true,
            poster: false,
            movie: None,
            audio: rip::AudioSel::All,
            subs: rip::SubSel::None,
            covers: false,
            tv: false,
            series: None,
            episodes: None,
            start_episode: None,
        }
    }

    #[test]
    fn drains_in_order_and_marks_states() {
        let mut q = QueueState::default();
        assert!(q.start_next().is_none());
        let a = q.enqueue(req(1));
        let b = q.enqueue(req(2));
        assert!(a < b);
        assert_eq!(q.views().len(), 2);

        let (id, r) = q.start_next().expect("first");
        assert_eq!(id, a);
        assert_eq!(r.title, Some(1));
        assert!(q.start_next().is_none(), "only one job runs at a time");

        q.update(a, 42.0, "encoding", None);
        assert_eq!(q.views()[0].percent, 42.0);
        q.finish(a, None, false);
        assert_eq!(q.views()[0].state, "done");
        assert_eq!(q.views()[0].percent, 100.0);

        let (id, _) = q.start_next().expect("second");
        assert_eq!(id, b);
        q.finish(b, Some("boom".into()), false);
        assert_eq!(q.views()[1].state, "error");
        assert_eq!(q.views()[1].message, "boom");
        assert!(q.start_next().is_none());
    }

    #[test]
    fn restores_running_jobs_as_queued() {
        let mut q = QueueState::default();
        q.enqueue(req(1));
        q.enqueue(req(2));
        q.start_next();
        let saved = serde_json::to_string(&q.snapshot()).unwrap();
        let back: Vec<QueueJob> = serde_json::from_str(&saved).unwrap();

        let mut restored = QueueState::default();
        restored.restore(back);
        let views = restored.views();
        assert_eq!(views.len(), 2);
        assert!(views.iter().all(|v| v.state == "queued"));
        let next = restored.enqueue(req(3));
        assert!(next > views.iter().map(|v| v.id).max().unwrap());
    }

    #[test]
    fn cancels_running_and_queued_jobs() {
        let mut q = QueueState::default();
        let a = q.enqueue(req(1));
        let b = q.enqueue(req(2));
        q.start_next();
        assert!(q.cancel(a), "a running job signals the ripper");
        q.finish(a, Some("cancelled".into()), true);
        assert_eq!(q.views()[0].state, "cancelled");
        assert!(!q.cancel(b), "a queued job is cancelled without a thread");
        assert_eq!(q.views()[1].state, "cancelled");
    }

    #[test]
    fn removes_finished_but_keeps_running() {
        let mut q = QueueState::default();
        let a = q.enqueue(req(1));
        let b = q.enqueue(req(2));
        let c = q.enqueue(req(3));
        q.start_next();
        q.finish(a, None, false);
        q.clear_finished();
        let ids: Vec<u64> = q.views().iter().map(|v| v.id).collect();
        assert!(ids.contains(&b) && ids.contains(&c));
        assert!(!ids.contains(&a));
        q.start_next();
        q.remove(b);
        assert!(q.views().iter().any(|v| v.id == b), "running job stays");
    }
}
