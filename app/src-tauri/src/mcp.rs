//! A local MCP (Model Context Protocol) server, hosted inside the app.
//!
//! Every capability the UI has is exposed as a tool here, so an agent can drive
//! BluForge without clicking. Transport is stateless JSON-RPC 2.0 over HTTP:
//! `POST /mcp` with a normal MCP request, `application/json` back. Bound to
//! loopback only, and only listening when `mcp_enabled` is set.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

use keepsake_core::config::AppConfig;
use keepsake_core::iso;
use keepsake_core::keys;
use keepsake_core::rip::{
    AudioSel, Container, EncodeSettings, RipMode, RipProgress, RipRequest, SubSel, VideoCodec,
};

const PROTOCOL: &str = "2025-06-18";
const SERVER_NAME: &str = "keepsake-atelier";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

// ---------------------------------------------------------------------------
// Job registry (ripping and ISO export are long; tools start them and poll)
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Job {
    id: u64,
    kind: String,
    state: String,
    message: String,
    percent: f64,
    dest: Option<String>,
    error: Option<String>,
    cancel: Option<Arc<AtomicBool>>,
}

fn jobs() -> &'static Mutex<Vec<Job>> {
    static JOBS: OnceLock<Mutex<Vec<Job>>> = OnceLock::new();
    JOBS.get_or_init(|| Mutex::new(Vec::new()))
}

static NEXT_JOB: AtomicU64 = AtomicU64::new(1);
static BUSY: AtomicBool = AtomicBool::new(false);

fn job_update(id: u64, f: impl FnOnce(&mut Job)) {
    if let Ok(mut v) = jobs().lock() {
        if let Some(j) = v.iter_mut().find(|j| j.id == id) {
            f(j);
        }
    }
}

fn job_json(j: &Job) -> Value {
    json!({
        "id": j.id,
        "kind": j.kind,
        "state": j.state,
        "message": j.message,
        "percent": j.percent,
        "dest": j.dest,
        "error": j.error,
    })
}

fn require_free_slot() -> Result<(), String> {
    if BUSY.load(Ordering::SeqCst) {
        return Err("a rip or export is already running".into());
    }
    BUSY.store(true, Ordering::SeqCst);
    Ok(())
}

// ---------------------------------------------------------------------------
// Server lifecycle
// ---------------------------------------------------------------------------

static RUNNING_PORT: Mutex<Option<u16>> = Mutex::new(None);
static STOP: AtomicBool = AtomicBool::new(false);

pub fn running_port() -> Option<u16> {
    *RUNNING_PORT.lock().unwrap()
}

pub fn start(port: u16) -> Result<u16, String> {
    if running_port() == Some(port) {
        return Ok(port);
    }
    stop();
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| format!("cannot bind 127.0.0.1:{port}: {e}"))?;
    let bound = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    STOP.store(false, Ordering::SeqCst);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            if STOP.load(Ordering::SeqCst) {
                break;
            }
            if let Ok(s) = stream {
                std::thread::spawn(move || handle(s));
            }
        }
    });
    *RUNNING_PORT.lock().unwrap() = Some(bound);
    tracing::info!(port = bound, "mcp server listening");
    Ok(bound)
}

pub fn stop() {
    if let Some(port) = RUNNING_PORT.lock().unwrap().take() {
        STOP.store(true, Ordering::SeqCst);
        // Unblock the blocking accept() so the listener is dropped.
        let _ = TcpStream::connect(("127.0.0.1", port));
        tracing::info!(port, "mcp server stopped");
    }
}

/// Start or stop the server to match the saved configuration.
pub fn apply(cfg: &AppConfig) {
    if cfg.mcp_enabled {
        if let Err(e) = start(cfg.mcp_port) {
            tracing::error!(%e, "mcp server did not start");
        }
    } else {
        stop();
    }
}

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

fn handle(mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(20)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(20)));
    let mut reader = match stream.try_clone() {
        Ok(s) => BufReader::new(s),
        Err(_) => return,
    };

    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("/").to_string();

    let mut len = 0usize;
    loop {
        let mut header = String::new();
        match reader.read_line(&mut header) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        if header == "\r\n" || header == "\n" {
            break;
        }
        if let Some((k, v)) = header.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0u8; len];
    if len > 0 && reader.read_exact(&mut body).is_err() {
        return;
    }

    let (status, payload) = if method == "GET" {
        (
            "200 OK",
            json!({
                "ok": true,
                "server": SERVER_NAME,
                "version": SERVER_VERSION,
                "protocol": PROTOCOL,
                "endpoint": "/mcp",
            })
            .to_string(),
        )
    } else if method != "POST" {
        (
            "405 Method Not Allowed",
            json!({"error": "POST /mcp"}).to_string(),
        )
    } else {
        match dispatch(&body) {
            Some(v) => ("200 OK", v.to_string()),
            None => ("202 Accepted", String::new()),
        }
    };

    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(payload.as_bytes());
    let _ = stream.flush();
    let _ = stream.shutdown(std::net::Shutdown::Both);
    let _ = path;
}

// ---------------------------------------------------------------------------
// JSON-RPC
// ---------------------------------------------------------------------------

fn dispatch(body: &[u8]) -> Option<Value> {
    let req: Value = serde_json::from_slice(body).ok()?;
    let id = req.get("id").cloned();
    // A request without an id is a notification and gets no body.
    let id = match id {
        Some(i) => i,
        None => {
            let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
            if method == "notifications/initialized" {
                tracing::debug!("mcp client initialized");
            }
            return None;
        }
    };
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(json!({}));

    let outcome: Result<Value, (i64, String)> = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION },
            "instructions": "Keepsake Atelier: inspect a Blu-ray, plan TV episodes, rip, export a decrypted ISO, fetch covers. Long jobs return an id; poll job_status."
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => call_tool(&params),
        "resources/list" => Ok(json!({ "resources": [] })),
        "prompts/list" => Ok(json!({ "prompts": [] })),
        _ => Err((-32601, format!("unknown method: {method}"))),
    };

    Some(match outcome {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message },
        }),
    })
}

fn call_tool(params: &Value) -> Result<Value, (i64, String)> {
    let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let result = run_tool(name, args);
    Ok(match result {
        Ok(v) => json!({
            "content": [{ "type": "text", "text": pretty(&v) }],
            "isError": false,
        }),
        Err(e) => json!({
            "content": [{ "type": "text", "text": e }],
            "isError": true,
        }),
    })
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}

fn run_tool(name: &str, args: Value) -> Result<Value, String> {
    match name {
        "status" => {
            let cfg = AppConfig::load();
            let snap =
                serde_json::to_value(keepsake_core::snapshot()).map_err(|e| e.to_string())?;
            Ok(json!({
                "app": snap,
                "mcp": {
                    "enabled": cfg.mcp_enabled,
                    "port": running_port(),
                },
                "output_dir": cfg.output_dir,
            }))
        }
        "disc_info" => {
            let source = resolve_source(&str_arg(&args, "source")?);
            let disc = keepsake_core::Disc::open(Path::new(&source), Some(&keys::keydb_path()))
                .map_err(|e| e.to_string())?;
            let mut info = disc.info().map_err(|e| e.to_string())?;
            info.path = source.clone();
            info.name = keepsake_core::device::source_name(&source, &info.name);
            serde_json::to_value(info).map_err(|e| e.to_string())
        }
        "tv_plan" => {
            let source = resolve_source(&str_arg(&args, "source")?);
            let series = args.get("series").and_then(|v| v.as_str());
            let disc = keepsake_core::Disc::open(Path::new(&source), Some(&keys::keydb_path()))
                .map_err(|e| e.to_string())?;
            let info = disc.info().map_err(|e| e.to_string())?;
            let start = args.get("start").and_then(|v| v.as_u64()).map(|v| v as u32);
            let plan =
                keepsake_core::tv::plan(&disc, &info, series, start).map_err(|e| e.to_string())?;
            serde_json::to_value(plan).map_err(|e| e.to_string())
        }
        "rip_start" => start_rip(args),
        "dvd_info" => {
            let source = resolve_source(&str_arg(&args, "source")?);
            let info = keepsake_core::dvd::info(Path::new(&source)).map_err(|e| e.to_string())?;
            serde_json::to_value(info).map_err(|e| e.to_string())
        }
        "dvd_start" => start_dvd(args),
        "clone_start" => start_clone(args),
        "burn_start" => start_burn(args),
        "doctor" => serde_json::to_value(keepsake_core::doctor::run()).map_err(|e| e.to_string()),
        "verify" => {
            let path = resolve_source(&str_arg(&args, "path")?);
            let report = keepsake_core::verify::run(Path::new(&path)).map_err(|e| e.to_string())?;
            serde_json::to_value(report).map_err(|e| e.to_string())
        }
        "key_entry" => {
            let source = resolve_source(&str_arg(&args, "source")?);
            let mount = mount_for(&source);
            let Some(id) = keepsake_core::keys::disc_id_for_path(&mount) else {
                return Ok(json!({ "entry": null }));
            };
            let entry = keys::lookup_at(&keys::keydb_path(), &id).map_err(|e| e.to_string())?;
            Ok(json!({ "entry": entry }))
        }
        "keys_set_units" => {
            let source = resolve_source(&str_arg(&args, "source")?);
            let mount = mount_for(&source);
            let id = keepsake_core::keys::disc_id_for_path(&mount)
                .ok_or("no AACS/Unit_Key_RO.inf on this disc")?;
            let keys_in: Vec<String> = args
                .get("keys")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let entry = keys::set_unit_keys_at(&keys::keydb_path(), &id, keys_in)
                .map_err(|e| e.to_string())?;
            serde_json::to_value(entry).map_err(|e| e.to_string())
        }
        "menu_parse" => {
            let source = resolve_source(&str_arg(&args, "source")?);
            let doc = keepsake_core::menu::parse(Path::new(&source), Some(&keys::keydb_path()))
                .map_err(|e| e.to_string())?;
            if let Some(name) = args.get("save").and_then(|v| v.as_str()) {
                keepsake_core::menu::save(&keepsake_core::menu::store_dir(), name, &doc)
                    .map_err(|e| e.to_string())?;
            }
            serde_json::to_value(doc).map_err(|e| e.to_string())
        }
        "menu_list" => {
            let dir = keepsake_core::menu::store_dir();
            let menus: Vec<Value> = keepsake_core::menu::list(&dir)
                .into_iter()
                .map(|(name, path)| {
                    let doc = keepsake_core::menu::load(&dir, &name).ok();
                    json!({
                        "name": name,
                        "path": path.display().to_string(),
                        "label": doc.as_ref().map(|d| d.label.clone()).unwrap_or_default(),
                        "titles": doc.as_ref().map(|d| d.titles.len()).unwrap_or(0),
                    })
                })
                .collect();
            Ok(json!({ "dir": dir.display().to_string(), "menus": menus }))
        }
        "menu_show" => {
            let name = str_arg(&args, "template")?;
            let doc = keepsake_core::menu::load(&keepsake_core::menu::store_dir(), &name)
                .map_err(|e| e.to_string())?;
            serde_json::to_value(doc).map_err(|e| e.to_string())
        }
        "menu_edit" => {
            let name = str_arg(&args, "template")?;
            let dir = keepsake_core::menu::store_dir();
            let mut doc = keepsake_core::menu::load(&dir, &name).map_err(|e| e.to_string())?;
            let title = args
                .get("title")
                .and_then(|v| v.as_u64())
                .ok_or("missing integer argument `title`")? as u32;
            if args.get("main").and_then(|v| v.as_bool()).unwrap_or(false) {
                doc.set_main(title).map_err(|e| e.to_string())?;
            }
            if let Some(new_name) = args.get("name").and_then(|v| v.as_str()) {
                doc.set_name(title, new_name).map_err(|e| e.to_string())?;
            }
            if let Some(chapters) = args.get("chapters").and_then(|v| v.as_array()) {
                for c in chapters {
                    let spec = c
                        .as_str()
                        .ok_or("`chapters` entries must be strings like \"2:Name\"")?;
                    let (n, text) =
                        keepsake_core::menu::parse_chapter_spec(spec).map_err(|e| e.to_string())?;
                    doc.set_chapter(title, n, &text)
                        .map_err(|e| e.to_string())?;
                }
            }
            keepsake_core::menu::save(&dir, &name, &doc).map_err(|e| e.to_string())?;
            serde_json::to_value(doc).map_err(|e| e.to_string())
        }
        "menu_apply" => {
            let name = str_arg(&args, "template")?;
            let target = resolve_source(&str_arg(&args, "target")?);
            let dir = keepsake_core::menu::store_dir();
            let tpl = keepsake_core::menu::load(&dir, &name).map_err(|e| e.to_string())?;
            let target_doc =
                keepsake_core::menu::parse(Path::new(&target), Some(&keys::keydb_path()))
                    .map_err(|e| e.to_string())?;
            let save = args.get("save").and_then(|v| v.as_str());
            let doc = keepsake_core::menu::apply(&tpl, &target_doc, &name);
            if let Some(save) = save {
                keepsake_core::menu::save(&dir, save, &doc).map_err(|e| e.to_string())?;
            }
            serde_json::to_value(doc).map_err(|e| e.to_string())
        }
        "menu_remove" => {
            let name = str_arg(&args, "template")?;
            let path = keepsake_core::menu::remove(&keepsake_core::menu::store_dir(), &name)
                .map_err(|e| e.to_string())?;
            Ok(json!({ "removed": path.display().to_string() }))
        }
        "author_start" => start_author(args),
        "iso_start" => start_iso(args),
        "job_status" => {
            let want = args.get("id").and_then(|v| v.as_u64());
            let jobs = jobs().lock().unwrap();
            let out: Vec<Value> = jobs
                .iter()
                .filter(|j| want.map(|w| w == j.id).unwrap_or(true))
                .map(job_json)
                .collect();
            Ok(json!({ "jobs": out, "busy": BUSY.load(Ordering::SeqCst) }))
        }
        "job_cancel" => {
            let id = args.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let mut jobs = jobs().lock().unwrap();
            let job = if id == 0 {
                jobs.last_mut()
            } else {
                jobs.iter_mut().find(|j| j.id == id)
            };
            match job {
                Some(j) => {
                    if let Some(c) = &j.cancel {
                        c.store(true, Ordering::SeqCst);
                    }
                    j.state = "cancelled".into();
                    Ok(json!({ "cancelled": j.id }))
                }
                None => Err("no such job".into()),
            }
        }
        "covers_search" => {
            let query = str_arg(&args, "query")?;
            let source = args.get("source").and_then(|v| v.as_str()).unwrap_or("all");
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(12) as usize;
            let cfg = AppConfig::load();
            let sources: Vec<keepsake_core::CoverSource> = if source == "all" {
                keepsake_core::CoverSource::all().to_vec()
            } else {
                keepsake_core::CoverSource::parse(source)
                    .map(|s| vec![s])
                    .unwrap_or_else(|| keepsake_core::CoverSource::all().to_vec())
            };
            let mut hits = Vec::new();
            for s in sources {
                if let Ok(mut r) =
                    keepsake_core::covers::search(s, &query, limit, &cfg.hirescovers_dir)
                {
                    hits.append(&mut r);
                }
            }
            serde_json::to_value(hits).map_err(|e| e.to_string())
        }
        "covers_download" => {
            let url = str_arg(&args, "url")?;
            let dest = str_arg(&args, "dest")?;
            let cfg = AppConfig::load();
            let (bytes, kind) =
                keepsake_core::covers::fetch(&url, keepsake_core::covers::cookie(&cfg).as_deref())
                    .map_err(|e| e.to_string())?;
            if let Some(dir) = Path::new(&dest).parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::write(&dest, &bytes).map_err(|e| e.to_string())?;
            Ok(json!({ "path": dest, "bytes": bytes.len(), "content_type": kind }))
        }
        "keys_update" => {
            let st = keys::update().map_err(|e| e.to_string())?;
            serde_json::to_value(st).map_err(|e| e.to_string())
        }
        "keys_list" => {
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(40) as usize;
            let search = args.get("search").and_then(|v| v.as_str());
            let all = match search {
                Some(q) => keys::search_at(&keys::keydb_path(), q, limit),
                None => keys::list_at(&keys::keydb_path(), None),
            }
            .map_err(|e| e.to_string())?;
            let rows: Vec<Value> = all
                .into_iter()
                .filter(|e| e.vuk.is_some() || !e.unit_keys.is_empty())
                .take(limit)
                .map(|e| json!({ "disc_id": e.id, "name": e.name }))
                .collect();
            Ok(json!({ "entries": rows, "limit": limit }))
        }
        "keys_add" => {
            let id = keys_arg_disc(&args)?;
            let entry = keys::KeyEntry {
                id,
                name: args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                vuk: args.get("vuk").and_then(|v| v.as_str()).map(str::to_string),
                volume_id: args
                    .get("volume_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                media_key: args
                    .get("media_key")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                unit_keys: args
                    .get("unit_keys")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
                note: Some("keepsake-atelier mcp".into()),
            };
            let replaced =
                keys::upsert_at(&keys::keydb_path(), &entry).map_err(|e| e.to_string())?;
            Ok(json!({ "disc_id": entry.id, "replaced": replaced }))
        }
        "keys_remove" => {
            let id = keys_arg_disc(&args)?;
            let removed = keys::remove_at(&keys::keydb_path(), &id).map_err(|e| e.to_string())?;
            Ok(json!({ "disc_id": id, "removed": removed }))
        }
        "eject" => {
            let msg = keepsake_core::device::eject().map_err(|e| e.to_string())?;
            Ok(json!({ "message": msg }))
        }
        "config_get" => serde_json::to_value(AppConfig::load()).map_err(|e| e.to_string()),
        "config_set" => {
            let mut cfg = AppConfig::load();
            apply_config_patch(&mut cfg, &args)?;
            cfg.save().map_err(|e| e.to_string())?;
            apply(&cfg);
            serde_json::to_value(&cfg).map_err(|e| e.to_string())
        }
        "preview_output" => {
            let template = str_arg(&args, "template")?;
            let ext = args.get("ext").and_then(|v| v.as_str()).unwrap_or("mkv");
            let s = |k: &str| {
                args.get(k)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            };
            let v = keepsake_core::TemplateVars::build(
                &s("disc"),
                &s("title"),
                args.get("year").and_then(|x| x.as_str()),
                args.get("tmdb").and_then(|x| x.as_u64()),
                args.get("resolution").and_then(|x| x.as_str()),
                args.get("hdr").and_then(|x| x.as_str()),
                args.get("video_codec").and_then(|x| x.as_str()),
                args.get("audio_lang").and_then(|x| x.as_str()),
                args.get("audio_codec").and_then(|x| x.as_str()),
                args.get("runtime_secs")
                    .and_then(|x| x.as_f64())
                    .unwrap_or(0.0),
                args.get("chapters").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
                args.get("playlist").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
                args.get("index").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
                ext,
            );
            Ok(json!({
                "path": keepsake_core::render_template(&template, &v, ext, "{title} - {resolution}")
            }))
        }
        "template_placeholders" => Ok(json!(keepsake_core::template::PLACEHOLDERS
            .iter()
            .map(|(k, d)| json!({ "key": k, "description": d }))
            .collect::<Vec<_>>())),
        other => Err(format!("unknown tool: {other}")),
    }
}

/// `/dev/rdiskN` goes stale when the drive re-enumerates (it moved disk5 -> disk4
/// here). Fall back to the optical drive that is present now.
/// Resolve a disc id from a `disc` (40 hex) or a `source` path.
fn keys_arg_disc(args: &Value) -> Result<String, String> {
    if let Some(d) = args.get("disc").and_then(|v| v.as_str()) {
        let t = d
            .trim()
            .trim_start_matches("0x")
            .trim_start_matches("0X")
            .to_ascii_lowercase();
        if t.len() == 40 && t.chars().all(|c| c.is_ascii_hexdigit()) {
            return Ok(t);
        }
        return Err(format!("bad disc id '{d}'"));
    }
    if let Some(s) = args.get("source").and_then(|v| v.as_str()) {
        let resolved = resolve_source(s);
        let p = Path::new(&resolved);
        return keys::disc_id_for_path(p)
            .ok_or_else(|| format!("no AACS/Unit_Key_RO.inf under {}", p.display()));
    }
    Err("give `disc` or `source`".into())
}

/// A disc root whose AACS/Unit_Key_RO.inf can be read, resolving a raw device
/// node to the mounted optical volume.
fn mount_for(source: &str) -> PathBuf {
    let p = Path::new(source);
    if keepsake_core::keys::disc_id_for_path(p).is_some() {
        return p.to_path_buf();
    }
    keepsake_core::device::list_optical()
        .into_iter()
        .find_map(|d| d.mount_point)
        .map(PathBuf::from)
        .unwrap_or_else(|| p.to_path_buf())
}

fn resolve_source(p: &str) -> String {
    if Path::new(p).exists() {
        return p.to_string();
    }
    let name = Path::new(p)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !(name.starts_with("rdisk") || name.starts_with("disk")) {
        return p.to_string();
    }
    if let Some(d) = keepsake_core::device::list_optical().into_iter().next() {
        let path = d
            .device_path
            .unwrap_or_else(|| format!("/dev/r{}", d.bsd_name));
        if Path::new(&path).exists() {
            return path;
        }
    }
    p.to_string()
}

fn str_arg(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("missing string argument `{key}`"))
}

// ---------------------------------------------------------------------------
// Long jobs
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct RipArgs {
    source: String,
    #[serde(default)]
    title: Option<u32>,
    #[serde(default)]
    output: Option<String>,
    #[serde(default)]
    tv: bool,
    #[serde(default)]
    series: Option<String>,
    #[serde(default)]
    episodes: Option<String>,
    #[serde(default)]
    start_episode: Option<u32>,
    #[serde(default)]
    chapters: Option<bool>,
    #[serde(default)]
    poster: Option<bool>,
    #[serde(default)]
    covers: Option<bool>,
    #[serde(default)]
    movie: Option<String>,
    #[serde(default)]
    audio: Option<String>,
    #[serde(default)]
    subs: Option<String>,
    #[serde(default)]
    encode: Option<bool>,
    #[serde(default)]
    container: Option<String>,
    #[serde(default)]
    codec: Option<String>,
    #[serde(default)]
    quality: Option<u32>,
    #[serde(default)]
    preset: Option<String>,
    #[serde(default)]
    ten_bit: Option<bool>,
    #[serde(default)]
    aac_audio: Option<bool>,
}

fn start_rip(args: Value) -> Result<Value, String> {
    let a: RipArgs = serde_json::from_value(args).map_err(|e| format!("bad rip arguments: {e}"))?;
    require_free_slot()?;

    let container = match a.container.as_deref() {
        Some("mp4") => Container::Mp4,
        _ => Container::Mkv,
    };
    let mode = if a.encode.unwrap_or(false) {
        RipMode::Encode {
            container,
            settings: EncodeSettings {
                codec: match a.codec.as_deref() {
                    Some("x264") | Some("h264") => VideoCodec::X264,
                    Some("vt") | Some("videotoolbox") | Some("hevc_videotoolbox") => {
                        VideoCodec::HevcVideotoolbox
                    }
                    Some("h264_videotoolbox") => VideoCodec::H264Videotoolbox,
                    _ => VideoCodec::X265,
                },
                quality: a.quality.unwrap_or(20),
                preset: a.preset.clone().or_else(|| Some("medium".into())),
                ten_bit: a.ten_bit.unwrap_or(true),
                aac_audio: a.aac_audio.unwrap_or(false),
            },
        }
    } else {
        RipMode::Remux { container }
    };

    let req = RipRequest {
        source: PathBuf::from(resolve_source(&a.source)),
        keydb: Some(keys::keydb_path()),
        title: a.title,
        output: a.output.map(PathBuf::from),
        mode,
        chapters: a.chapters.unwrap_or(true),
        poster: a.poster.unwrap_or(true),
        movie: a.movie,
        audio: match a.audio.as_deref() {
            Some("main") => AudioSel::Main,
            Some("english") | Some("eng") => AudioSel::English,
            _ => AudioSel::All,
        },
        subs: match a.subs.as_deref() {
            Some("english") | Some("eng") => SubSel::English,
            Some("none") => SubSel::None,
            _ => SubSel::All,
        },
        covers: a.covers.unwrap_or(true),
        tv: a.tv,
        series: a.series,
        episodes: a.episodes,
        start_episode: a.start_episode,
    };

    let id = NEXT_JOB.fetch_add(1, Ordering::SeqCst);
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut v = jobs().lock().unwrap();
        v.push(Job {
            id,
            kind: if req.tv {
                "rip-tv".into()
            } else {
                "rip".into()
            },
            state: "running".into(),
            message: "starting".into(),
            percent: 0.0,
            dest: None,
            error: None,
            cancel: Some(cancel.clone()),
        });
    }

    let cancel = cancel.clone();
    std::thread::spawn(move || {
        let res = rip::run(&req, &cancel, |p: RipProgress| {
            job_update(id, |j| {
                j.percent = p.percent;
                if !p.message.is_empty() {
                    j.message = p.message.clone();
                }
                if let Some(d) = &p.dest {
                    j.dest = Some(d.display().to_string());
                }
            });
        });
        match res {
            Ok(()) => job_update(id, |j| {
                if j.state == "running" {
                    j.state = "done".into();
                }
                j.message = "finished".into();
            }),
            Err(e) => job_update(id, |j| {
                j.state = "error".into();
                j.error = Some(e.to_string());
            }),
        }
        BUSY.store(false, Ordering::SeqCst);
    });

    Ok(json!({ "job": id, "state": "running" }))
}

#[derive(Deserialize)]
struct AuthorArgs {
    /// Single input, kept for compatibility.
    #[serde(default)]
    input: Option<String>,
    /// One title per input.
    #[serde(default)]
    inputs: Option<Vec<String>>,
    dest: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    chapters: Option<bool>,
    #[serde(default)]
    auto_chapter_minutes: Option<u32>,
    #[serde(default)]
    split_gb: Option<f64>,
    /// Add an authored BD-J title menu (experimental).
    #[serde(default)]
    menu: bool,
    /// Background image for the authored menu.
    #[serde(default)]
    menu_background: Option<String>,
    /// Saved menu document to take title names from.
    #[serde(default)]
    menu_template: Option<String>,
}

fn start_author(args: Value) -> Result<Value, String> {
    let a: AuthorArgs =
        serde_json::from_value(args).map_err(|e| format!("bad author arguments: {e}"))?;
    require_free_slot()?;

    let mut inputs: Vec<PathBuf> = a
        .inputs
        .unwrap_or_default()
        .iter()
        .map(PathBuf::from)
        .collect();
    if inputs.is_empty() {
        if let Some(one) = &a.input {
            inputs.push(PathBuf::from(one));
        }
    }
    if inputs.is_empty() {
        return Err("pass `input` or `inputs`".into());
    }
    let req = keepsake_core::author::AuthorRequest {
        inputs,
        dest: PathBuf::from(&a.dest),
        label: a.label,
        chapters: a.chapters.unwrap_or(true),
        auto_chapter_minutes: a.auto_chapter_minutes,
        split_gb: a.split_gb,
        menu: a.menu,
        menu_background: a.menu_background.map(PathBuf::from),
        menu_template: a.menu_template.map(PathBuf::from),
    };

    let id = NEXT_JOB.fetch_add(1, Ordering::SeqCst);
    {
        let mut v = jobs().lock().unwrap();
        v.push(Job {
            id,
            kind: "author".into(),
            state: "running".into(),
            message: "starting".into(),
            percent: 0.0,
            dest: Some(a.dest.clone()),
            error: None,
            cancel: None,
        });
    }

    std::thread::spawn(move || {
        let res = keepsake_core::author::run(&req, |p: keepsake_core::author::AuthorProgress| {
            job_update(id, |j| {
                j.percent = p.percent;
                if !p.message.is_empty() {
                    j.message = p.message.clone();
                }
            });
        });
        match res {
            Ok(path) => job_update(id, |j| {
                j.state = "done".into();
                j.message = "finished".into();
                j.dest = Some(path.display().to_string());
            }),
            Err(e) => job_update(id, |j| {
                j.state = "error".into();
                j.error = Some(e.to_string());
            }),
        }
        BUSY.store(false, Ordering::SeqCst);
    });

    Ok(json!({ "job": id, "state": "running" }))
}

#[derive(Deserialize)]
struct IsoArgs {
    source: String,
    dest: String,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    label: Option<String>,
}

#[derive(Deserialize)]
struct CloneArgs {
    source: String,
    dest: String,
}

#[derive(Deserialize)]
struct BurnArgs {
    image: String,
    #[serde(default)]
    device: Option<String>,
    #[serde(default)]
    speed: Option<u32>,
    #[serde(default)]
    dry_run: Option<bool>,
    #[serde(default)]
    confirm: Option<bool>,
}

fn start_burn(args: Value) -> Result<Value, String> {
    let a: BurnArgs =
        serde_json::from_value(args).map_err(|e| format!("bad burn arguments: {e}"))?;
    require_free_slot()?;
    // Writing erases the disc: only do it when both the caller and confirm agree.
    let dry_run = a.dry_run.unwrap_or(true) || a.confirm != Some(true);
    let req = keepsake_core::burn::BurnRequest {
        image: PathBuf::from(&a.image),
        device: a.device.clone().map(PathBuf::from),
        speed: a.speed,
        dry_run,
    };
    let id = NEXT_JOB.fetch_add(1, Ordering::SeqCst);
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut v = jobs().lock().unwrap();
        v.push(Job {
            id,
            kind: "burn".into(),
            state: "running".into(),
            message: if dry_run {
                "dry run".into()
            } else {
                "starting".into()
            },
            percent: 0.0,
            dest: Some(a.image.clone()),
            error: None,
            cancel: Some(cancel.clone()),
        });
    }
    let cancel2 = cancel.clone();
    std::thread::spawn(move || {
        let res = keepsake_core::burn::run(&req, &cancel2, |p| {
            job_update(id, |j| {
                if !p.done {
                    j.percent = p.percent;
                    j.message = p.message.clone();
                }
            });
        });
        match res {
            Ok(()) => job_update(id, |j| {
                j.state = "done".into();
                j.message = if dry_run {
                    "dry run: nothing written".into()
                } else {
                    "burned".into()
                };
                if !dry_run {
                    j.percent = 100.0;
                }
            }),
            Err(e) => job_update(id, |j| {
                j.state = if cancel2.load(Ordering::SeqCst) {
                    "cancelled".into()
                } else {
                    "error".into()
                };
                j.message = e.to_string();
                j.error = Some(e.to_string());
            }),
        }
    });
    Ok(json!({ "id": id, "dry_run": dry_run }))
}

fn start_clone(args: Value) -> Result<Value, String> {
    let a: CloneArgs =
        serde_json::from_value(args).map_err(|e| format!("bad clone arguments: {e}"))?;
    require_free_slot()?;
    let req = keepsake_core::clone::CloneRequest {
        source: PathBuf::from(resolve_source(&a.source)),
        dest: PathBuf::from(&a.dest),
    };
    let id = NEXT_JOB.fetch_add(1, Ordering::SeqCst);
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut v = jobs().lock().unwrap();
        v.push(Job {
            id,
            kind: "clone".into(),
            state: "running".into(),
            message: "starting".into(),
            percent: 0.0,
            dest: Some(a.dest.clone()),
            error: None,
            cancel: Some(cancel.clone()),
        });
    }
    let cancel2 = cancel.clone();
    std::thread::spawn(move || {
        let res = keepsake_core::clone::run(&req, &cancel2, |p| {
            job_update(id, |j| {
                j.percent = p.percent;
                j.message = p.message.clone();
            });
        });
        match res {
            Ok(bytes) => job_update(id, |j| {
                j.state = "done".into();
                j.percent = 100.0;
                j.message = format!("wrote {bytes} bytes");
            }),
            Err(e) => job_update(id, |j| {
                j.state = if cancel2.load(Ordering::SeqCst) {
                    "cancelled".into()
                } else {
                    "error".into()
                };
                j.message = e.to_string();
                j.error = Some(e.to_string());
            }),
        }
    });
    Ok(json!({ "id": id, "dest": a.dest }))
}

#[derive(Deserialize)]
struct DvdArgs {
    source: String,
    title: u16,
    #[serde(default)]
    dest: Option<String>,
    #[serde(default)]
    subtitles: Option<bool>,
}

fn start_dvd(args: Value) -> Result<Value, String> {
    let a: DvdArgs = serde_json::from_value(args).map_err(|e| format!("bad dvd arguments: {e}"))?;
    require_free_slot()?;
    let source = resolve_source(&a.source);
    let info = keepsake_core::dvd::info(Path::new(&source)).map_err(|e| e.to_string())?;
    let label = info.label.clone();
    let dest = a.dest.map(PathBuf::from).unwrap_or_else(|| {
        let cfg = AppConfig::load();
        cfg.output_dir
            .join(&label)
            .join(format!("{label} - Title {}.mkv", a.title))
    });
    let req = keepsake_core::dvd::DvdRipRequest {
        source: PathBuf::from(&source),
        title: a.title,
        output: dest.clone(),
        subtitles: a.subtitles.unwrap_or(false),
    };

    let id = NEXT_JOB.fetch_add(1, Ordering::SeqCst);
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut v = jobs().lock().unwrap();
        v.push(Job {
            id,
            kind: "dvd".into(),
            state: "running".into(),
            message: "starting".into(),
            percent: 0.0,
            dest: Some(dest.display().to_string()),
            error: None,
            cancel: Some(cancel.clone()),
        });
    }

    let cancel2 = cancel.clone();
    std::thread::spawn(move || {
        let res = keepsake_core::dvd::rip(&req, &cancel2, |p| {
            job_update(id, |j| {
                j.percent = p.percent;
                j.message = p.message.clone();
            });
        });
        match res {
            Ok(path) => job_update(id, |j| {
                j.state = "done".into();
                j.percent = 100.0;
                j.message = "done".into();
                j.dest = Some(path.display().to_string());
            }),
            Err(e) => job_update(id, |j| {
                j.state = if cancel2.load(Ordering::SeqCst) {
                    "cancelled".into()
                } else {
                    "error".into()
                };
                j.message = e.to_string();
                j.error = Some(e.to_string());
            }),
        }
    });

    Ok(json!({
        "id": id,
        "dest": dest.display().to_string(),
        "titles": info.titles.len(),
    }))
}

fn start_iso(args: Value) -> Result<Value, String> {
    let a: IsoArgs = serde_json::from_value(args).map_err(|e| format!("bad iso arguments: {e}"))?;
    require_free_slot()?;

    let req = iso::IsoRequest {
        source: PathBuf::from(resolve_source(&a.source)),
        keydb: Some(keys::keydb_path()),
        dest: PathBuf::from(&a.dest),
        format: match a.format.as_deref() {
            Some("folder") => iso::IsoFormat::Folder,
            _ => iso::IsoFormat::Iso,
        },
        label: a.label,
    };

    let id = NEXT_JOB.fetch_add(1, Ordering::SeqCst);
    {
        let mut v = jobs().lock().unwrap();
        v.push(Job {
            id,
            kind: "iso".into(),
            state: "running".into(),
            message: "starting".into(),
            percent: 0.0,
            dest: Some(a.dest.clone()),
            error: None,
            cancel: None,
        });
    }

    std::thread::spawn(move || {
        let res = iso::run(&req, |p: iso::IsoProgress| {
            job_update(id, |j| {
                j.percent = p.percent;
                if !p.message.is_empty() {
                    j.message = p.message.clone();
                }
            });
        });
        match res {
            Ok(()) => job_update(id, |j| {
                j.state = "done".into();
                j.message = "finished".into();
            }),
            Err(e) => job_update(id, |j| {
                j.state = "error".into();
                j.error = Some(e.to_string());
            }),
        }
        BUSY.store(false, Ordering::SeqCst);
    });

    Ok(json!({ "job": id, "state": "running" }))
}

// ---------------------------------------------------------------------------
// Config patching
// ---------------------------------------------------------------------------

fn apply_config_patch(cfg: &mut AppConfig, patch: &Value) -> Result<(), String> {
    let obj = patch.as_object().ok_or("arguments must be an object")?;
    for (key, val) in obj {
        match key.as_str() {
            "output_dir" => {
                cfg.output_dir = PathBuf::from(val.as_str().ok_or("output_dir must be a string")?)
            }
            "output_template" => {
                cfg.output_template = val
                    .as_str()
                    .ok_or("output_template must be a string")?
                    .to_string()
            }
            "tv_template" => {
                cfg.tv_template = val
                    .as_str()
                    .ok_or("tv_template must be a string")?
                    .to_string()
            }
            "tv_subs" => cfg.tv_subs = val.as_str().ok_or("tv_subs must be a string")?.to_string(),
            "auto_update_keys" => {
                cfg.auto_update_keys = val.as_bool().ok_or("auto_update_keys must be a bool")?
            }
            "auto_check_updates" => {
                cfg.auto_check_updates = val.as_bool().ok_or("auto_check_updates must be a bool")?
            }
            "auto_cover" => cfg.auto_cover = val.as_bool().ok_or("auto_cover must be a bool")?,
            "hirescovers_dir" => {
                cfg.hirescovers_dir =
                    PathBuf::from(val.as_str().ok_or("hirescovers_dir must be a string")?)
            }
            "hirescovers_user" => cfg.hirescovers_user = val.as_str().map(|s| s.to_string()),
            "hirescovers_pass" => cfg.hirescovers_pass = val.as_str().map(|s| s.to_string()),
            "hirescovers_cookie" => cfg.hirescovers_cookie = val.as_str().map(|s| s.to_string()),
            "mcp_enabled" => cfg.mcp_enabled = val.as_bool().ok_or("mcp_enabled must be a bool")?,
            "mcp_port" => {
                cfg.mcp_port = val
                    .as_u64()
                    .and_then(|p| u16::try_from(p).ok())
                    .ok_or("mcp_port must be 1..65535")?
            }
            _ => {}
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tool catalogue
// ---------------------------------------------------------------------------

fn tools() -> Vec<Value> {
    let src = |about: &str| {
        json!({
            "type": "string",
            "description": format!("{about} A device node (/dev/rdiskN), a mounted volume, a BDMV folder or an ISO.")
        })
    };
    vec![
        json!({
            "name": "status",
            "description": "App version, optical drives, key database state, MCP port and output folder.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "disc_info",
            "description": "Inspect a disc: titles with playlist/duration/resolution/HDR, streams, chapters.",
            "inputSchema": { "type": "object", "properties": { "source": src("Disc to read.") }, "required": ["source"] }
        }),
        json!({
            "name": "tv_plan",
            "description": "Plan a TV/anime disc as episodes: clip boundaries split into SxxExx with TMDB titles, stills and a poster.",
            "inputSchema": { "type": "object", "properties": {
                "source": src("Disc to plan."),
                "series": { "type": "string", "description": "Series name override for the TMDB lookup." },
                "start": { "type": "integer", "description": "First episode number on this disc (overrides the remembered mapping)." }
            }, "required": ["source"] }
        }),
        json!({
            "name": "burn_start",
            "description": "Write a single-title Blu-ray ISO to a writer drive. Dry run unless confirm=true (erases the disc). Returns a job id.",
            "inputSchema": { "type": "object", "properties": {
                "image": { "type": "string", "description": "The .iso to write." },
                "device": { "type": "string", "description": "Raw device node for growisofs, eg. /dev/rdisk4. Omit for native hdiutil burn (auto-detects the writer)." },
                "speed": { "type": "integer" },
                "dry_run": { "type": "boolean", "description": "Default true; print the command only." },
                "confirm": { "type": "boolean", "description": "Must be true for dry_run=false; writing erases the disc." }
            }, "required": ["image"] }
        }),
        json!({
            "name": "clone_start",
            "description": "1:1 sector copy of a disc to an ISO image (AACS/BD+ kept). Returns a job id.",
            "inputSchema": { "type": "object", "properties": {
                "source": src("Raw device (/dev/rdiskN) or a readable file."),
                "dest": { "type": "string", "description": "Output .iso." }
            }, "required": ["source", "dest"] }
        }),
        json!({
            "name": "dvd_info",
            "description": "List a DVD-Video's titles (number, chapters, angles, duration).",
            "inputSchema": { "type": "object", "properties": { "source": src("Mounted DVD or VIDEO_TS directory (a raw device resolves to its mount).") }, "required": ["source"] }
        }),
        json!({
            "name": "dvd_start",
            "description": "Rip one DVD-Video title to MKV (MPEG-2 and AC-3/DTS copied, no re-encode). Returns a job id.",
            "inputSchema": { "type": "object", "properties": {
                "source": src("Mounted DVD or VIDEO_TS directory."),
                "title": { "type": "integer", "description": "1-based title number (dvd_info)." },
                "dest": { "type": "string", "description": "Output .mkv; omit for the configured output folder." },
                "subtitles": { "type": "boolean", "description": "Include VobSub subtitles." }
            }, "required": ["source", "title"] }
        }),
        json!({
            "name": "doctor",
            "description": "Installed tools, versions, and which capability each missing piece blocks (DVD, burning, BD+, ...).",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "verify",
            "description": "Verify a BDMV tree: index/MovieObject navigation, playlists and clip files. Returns per-title results and errors.",
            "inputSchema": { "type": "object", "properties": { "path": src("Disc root, BDMV folder or mounted volume.") }, "required": ["path"] }
        }),
        json!({
            "name": "key_entry",
            "description": "The KEYDB entry for a disc: media key, volume id, VUK and AACS2 unit keys.",
            "inputSchema": { "type": "object", "properties": { "source": src("Disc to look up.") }, "required": ["source"] }
        }),
        json!({
            "name": "keys_set_units",
            "description": "Replace the AACS2/UHD unit-key table of a disc's existing KEYDB entry.",
            "inputSchema": { "type": "object", "properties": {
                "source": src("Disc whose entry to update."),
                "keys": { "type": "array", "items": { "type": "string" }, "description": "Unit keys, hex, in index order." }
            }, "required": ["source", "keys"] }
        }),
        json!({
            "name": "menu_parse",
            "description": "Parse a disc's menu (index.bdmv navigation + titles) into an editable document. Optionally save it as a template.",
            "inputSchema": { "type": "object", "properties": {
                "source": src("Disc to parse."),
                "save": { "type": "string", "description": "Save the document as a template with this name." }
            }, "required": ["source"] }
        }),
        json!({
            "name": "menu_list",
            "description": "List saved disc-menu templates.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "menu_show",
            "description": "Show one saved menu template (or a JSON document path).",
            "inputSchema": { "type": "object", "properties": {
                "template": { "type": "string", "description": "Template name or .json path." }
            }, "required": ["template"] }
        }),
        json!({
            "name": "menu_edit",
            "description": "Edit a saved menu template in place: rename a title, mark it as the main feature, or set chapter labels.",
            "inputSchema": { "type": "object", "properties": {
                "template": { "type": "string" },
                "title": { "type": "integer", "description": "Title index." },
                "name": { "type": "string", "description": "New display name." },
                "main": { "type": "boolean", "description": "Make this the single main feature." },
                "chapters": { "type": "array", "items": { "type": "string" }, "description": "Chapter labels as \"N:Text\", 1-based." }
            }, "required": ["template", "title"] }
        }),
        json!({
            "name": "menu_apply",
            "description": "Apply a saved menu template to a different disc, matching titles by playlist then duration. Optionally save the result.",
            "inputSchema": { "type": "object", "properties": {
                "template": { "type": "string" },
                "target": src("Disc to apply it to."),
                "save": { "type": "string", "description": "Save the result as a template with this name." }
            }, "required": ["template", "target"] }
        }),
        json!({
            "name": "menu_remove",
            "description": "Delete a saved menu template.",
            "inputSchema": { "type": "object", "properties": {
                "template": { "type": "string" }
            }, "required": ["template"] }
        }),
        json!({
            "name": "rip_start",
            "description": "Start a rip (remux or encode; movie or TV). Returns a job id; poll job_status. One rip at a time.",
            "inputSchema": { "type": "object", "properties": {
                "source": src("Disc to rip."),
                "title": { "type": "integer", "description": "Title index; omit for the main title." },
                "output": { "type": "string", "description": "Exact file or directory; omit to use the output template." },
                "tv": { "type": "boolean", "description": "One file per episode, named from the TV template." },
                "series": { "type": "string", "description": "Series name override for TV rips." },
                "episodes": { "type": "string", "description": "Episode filter for TV rips, eg. \"1-3\" or \"2,5\"." },
                "start_episode": { "type": "integer", "description": "First episode number on this disc, for multi-disc sets with uneven splits (D2 of 8+4 starts at 9)." },
                "chapters": { "type": "boolean" },
                "poster": { "type": "boolean", "description": "Fetch and attach the TMDB poster." },
                "covers": { "type": "boolean", "description": "Save covers next to the output." },
                "movie": { "type": "string", "description": "Title used for metadata lookups." },
                "audio": { "type": "string", "enum": ["all", "main", "english"] },
                "subs": { "type": "string", "enum": ["all", "english", "none"] },
                "encode": { "type": "boolean", "description": "Re-encode instead of remux." },
                "container": { "type": "string", "enum": ["mkv", "mp4"] },
                "codec": { "type": "string", "enum": ["x265", "x264", "vt"] },
                "quality": { "type": "integer", "description": "CRF / q 1..51 (encode only)." },
                "preset": { "type": "string" },
                "ten_bit": { "type": "boolean" },
                "aac_audio": { "type": "boolean" }
            }, "required": ["source"] }
        }),
        json!({
            "name": "iso_start",
            "description": "Export the disc decrypted as a UDF ISO or a BDMV folder. Returns a job id.",
            "inputSchema": { "type": "object", "properties": {
                "source": src("Disc to export."),
                "dest": { "type": "string", "description": "Output .iso file or folder." },
                "format": { "type": "string", "enum": ["iso", "folder"] },
                "label": { "type": "string", "description": "ISO volume label." }
            }, "required": ["source", "dest"] }
        }),
        json!({
            "name": "author_start",
            "description": "Author a Blu-ray (BDMV/ISO) from finished files. One or several inputs become one title each, written as a UDF 2.50 ISO (native newfs_udf) or a BDMV folder. Chapters come from the input's marks; needs tsMuxeR. Returns a job id.",
            "inputSchema": { "type": "object", "properties": {
                "input": { "type": "string", "description": "Single MKV/MP4/M2TS (kept for compatibility)." },
                "inputs": { "type": "array", "items": { "type": "string" }, "description": "One title per file, merged into one BDMV folder." },
                "dest": { "type": "string", "description": "Output .iso file (single input) or BDMV folder." },
                "label": { "type": "string" },
                "chapters": { "type": "boolean" },
                "auto_chapter_minutes": { "type": "integer", "description": "Force a chapter every N minutes instead." },
                "split_gb": { "type": "number", "description": "Split whole inputs across discs of at most this many GB (25/50); writes one folder per disc." },
                "menu": { "type": "boolean", "description": "Add an authored BD-J title menu as first play and top menu (experimental; verified with libbluray, not on hardware)." },
                "menu_background": { "type": "string", "description": "Background image for the authored menu." },
                "menu_template": { "type": "string", "description": "Saved menu document (name or path) whose title names feed the menu." }
            }, "required": ["dest"] }
        }),
        json!({
            "name": "job_status",
            "description": "Progress of rips/exports: percent, message, state, destination.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "integer", "description": "A job id; omit for all jobs." }
            } }
        }),
        json!({
            "name": "job_cancel",
            "description": "Cancel a running rip/export.",
            "inputSchema": { "type": "object", "properties": { "id": { "type": "integer" } } }
        }),
        json!({
            "name": "covers_search",
            "description": "Search TMDB / DVD cover art / hirescovers for a title. Returns hits with a url and kind.",
            "inputSchema": { "type": "object", "properties": {
                "query": { "type": "string" },
                "source": { "type": "string", "enum": ["all", "tmdb", "dvdcover", "hirescovers"] },
                "limit": { "type": "integer" }
            }, "required": ["query"] }
        }),
        json!({
            "name": "covers_download",
            "description": "Download an image url (from covers_search) to a local path.",
            "inputSchema": { "type": "object", "properties": {
                "url": { "type": "string" },
                "dest": { "type": "string" }
            }, "required": ["url", "dest"] }
        }),
        json!({
            "name": "keys_update",
            "description": "Refresh the AACS/BD+ key database (KEYDB.cfg).",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "keys_list",
            "description": "List keyed discs in the local key store (disc id + name), optionally filtered by name or id.",
            "inputSchema": { "type": "object", "properties": {
                "limit": { "type": "integer" },
                "search": { "type": "string", "description": "Filter by name or disc id (case-insensitive)." }
            } }
        }),
        json!({
            "name": "keys_add",
            "description": "Add or replace a disc key so it decrypts with native libaacs (no MakeMKV). Give `source` (a mounted disc) or `disc` (40-hex id), plus the VUK and/or unit key.",
            "inputSchema": { "type": "object", "properties": {
                "source": { "type": "string" },
                "disc": { "type": "string" },
                "vuk": { "type": "string" },
                "unit_keys": { "type": "array", "items": { "type": "string" } },
                "media_key": { "type": "string" },
                "volume_id": { "type": "string" },
                "name": { "type": "string" }
            } }
        }),
        json!({
            "name": "keys_remove",
            "description": "Remove a disc's key entry. Give `disc` (40-hex id) or `source`.",
            "inputSchema": { "type": "object", "properties": {
                "disc": { "type": "string" },
                "source": { "type": "string" }
            } }
        }),
        json!({
            "name": "eject",
            "description": "Eject the disc from the optical drive.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "config_get",
            "description": "Read the saved settings.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "config_set",
            "description": "Patch settings (output_dir, output_template, tv_template, tv_subs, auto_update_keys, auto_check_updates, auto_cover, hirescovers_*, mcp_enabled, mcp_port).",
            "inputSchema": { "type": "object", "properties": {
                "output_dir": { "type": "string" },
                "output_template": { "type": "string" },
                "tv_template": { "type": "string" },
                "tv_subs": { "type": "string", "enum": ["all", "signs"] },
                "auto_update_keys": { "type": "boolean" },
                "auto_check_updates": { "type": "boolean" },
                "auto_cover": { "type": "boolean" },
                "hirescovers_dir": { "type": "string" },
                "hirescovers_user": { "type": "string" },
                "hirescovers_pass": { "type": "string" },
                "hirescovers_cookie": { "type": "string" },
                "mcp_enabled": { "type": "boolean" },
                "mcp_port": { "type": "integer" }
            } }
        }),
        json!({
            "name": "preview_output",
            "description": "Render an output template with the same code the ripper uses.",
            "inputSchema": { "type": "object", "properties": {
                "template": { "type": "string" },
                "ext": { "type": "string" },
                "disc": { "type": "string" },
                "title": { "type": "string" },
                "year": { "type": "string" },
                "tmdb": { "type": "integer" },
                "resolution": { "type": "string" },
                "hdr": { "type": "string" },
                "video_codec": { "type": "string" },
                "audio_lang": { "type": "string" },
                "audio_codec": { "type": "string" },
                "runtime_secs": { "type": "number" },
                "chapters": { "type": "integer" },
                "playlist": { "type": "integer" },
                "index": { "type": "integer" }
            }, "required": ["template"] }
        }),
        json!({
            "name": "template_placeholders",
            "description": "List the output template placeholders and what they expand to.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
    ]
}

// re-export so lib.rs can call the same module
use keepsake_core::rip;
