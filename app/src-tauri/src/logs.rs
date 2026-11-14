//! In-memory log ring for the app: backend steps, child-process stderr and
//! frontend failures, so a failed job can be inspected instead of guessed at.
//!
//! Entries live in a bounded buffer (newest last) and are also mirrored to the
//! terminal by the normal tracing subscriber.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

const CAPACITY: usize = 2000;

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub id: u64,
    /// Milliseconds since the Unix epoch.
    pub at: u64,
    /// error | warn | info | debug
    pub level: String,
    /// Which part of the app wrote it (rip, iso, author, ffmpeg, ui, ...).
    pub source: String,
    pub message: String,
    pub detail: Option<String>,
    /// The job this line belongs to, when it was written during one.
    pub job: Option<u64>,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
/// Running jobs, innermost last. One job runs at a time, so a stack is enough
/// and child threads (process stderr) inherit it by peeking.
static CURRENT: Mutex<Vec<u64>> = Mutex::new(Vec::new());
static LOGS: Mutex<VecDeque<Entry>> = Mutex::new(VecDeque::new());

fn current_job() -> Option<u64> {
    CURRENT.lock().ok().and_then(|c| c.last().copied())
}

/// Mark a job as running for this thread and every thread it spawns. Dropping
/// the guard closes it out, even on a panic.
pub struct JobGuard(u64);

impl Drop for JobGuard {
    fn drop(&mut self) {
        // Close the section before popping, so the marker keeps the tag.
        info("job", format!("job {} finished", self.0));
        if let Ok(mut current) = CURRENT.lock() {
            if let Some(pos) = current.iter().rposition(|id| *id == self.0) {
                current.remove(pos);
            }
        }
    }
}

/// Open a job section: a start marker and the tag that every line carries.
pub fn begin_job(id: u64, kind: &str, what: &str) -> JobGuard {
    if let Ok(mut current) = CURRENT.lock() {
        current.push(id);
    }
    push(
        None,
        "info",
        "job",
        format!("job {id} started: {kind} {what}"),
        None,
    );
    JobGuard(id)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Scrub the values that are useless (or risky) to paste into a report: the
/// user's home path, long hex blobs (keys, hashes) and URL query strings.
fn redact(text: &str) -> String {
    static HEX: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static QUERY: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let hex = HEX.get_or_init(|| regex::Regex::new(r"\b[0-9a-fA-F]{32,}\b").expect("hex regex"));
    let query = QUERY.get_or_init(|| regex::Regex::new(r"\?[^\s]+").expect("query regex"));

    let home = std::env::var("HOME").unwrap_or_default();
    let mut out = if home.len() > 1 {
        text.replace(&home, "~")
    } else {
        text.to_string()
    };
    out = hex
        .replace_all(&out, |c: &regex::Captures| {
            let s = c[0].to_string();
            format!("{}…", &s[..8])
        })
        .into_owned();
    out = query.replace_all(&out, "?…").into_owned();
    out
}

fn push_inner(level: &str, source: &str, message: impl AsRef<str>, detail: Option<&str>) -> u64 {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let entry = Entry {
        id,
        at: now_ms(),
        level: level.to_string(),
        source: source.to_string(),
        message: redact(message.as_ref()),
        detail: detail.map(|d| redact(d.trim_end())),
        job: current_job(),
    };
    if let Ok(mut logs) = LOGS.lock() {
        logs.push_back(entry);
        while logs.len() > CAPACITY {
            logs.pop_front();
        }
    }
    id
}

fn push(
    app: Option<&AppHandle>,
    level: &str,
    source: &str,
    message: impl AsRef<str>,
    detail: Option<&str>,
) -> u64 {
    let id = push_inner(level, source, message, detail);
    if let Some(app) = app {
        let _ = app.emit("logs://new", id);
    }
    id
}

#[allow(dead_code)]
pub fn info(source: &str, message: impl AsRef<str>) {
    push(None, "info", source, message.as_ref(), None);
}

#[allow(dead_code)]
pub fn warn(source: &str, message: impl AsRef<str>) {
    push(None, "warn", source, message.as_ref(), None);
}

/// Record a failure with its context and return the entry id, which the UI
/// links from the error banner.
pub fn fail(
    app: Option<&AppHandle>,
    source: &str,
    message: impl AsRef<str>,
    detail: impl AsRef<str>,
) -> u64 {
    push(
        app,
        "error",
        source,
        message.as_ref(),
        Some(detail.as_ref()),
    )
}

/// The last `want` entries written by one source, oldest first.
pub fn tail_of(source: &str, want: usize) -> Vec<String> {
    LOGS.lock()
        .map(|logs| {
            let mut rows: Vec<&Entry> = logs.iter().filter(|e| e.source == source).collect();
            rows.reverse();
            rows.into_iter()
                .take(want)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .map(|e| format!("{} {}", e.level, e.message))
                .collect()
        })
        .unwrap_or_default()
}

/// Record a failed job with its context and the tail of the producing process,
/// and tell the UI so the banner can link straight to it.
pub fn fail_job(
    app: &AppHandle,
    source: &str,
    summary: impl AsRef<str>,
    context: &[(&str, String)],
    error: impl AsRef<str>,
) -> u64 {
    let mut detail = format!("{}\n", error.as_ref().trim_end());
    for (key, value) in context {
        detail.push_str(&format!("{key}: {value}\n"));
    }
    detail.push_str(&format!("--- stderr ({} lines) ---\n", source));
    let tail = tail_of(source, 40);
    if tail.is_empty() {
        detail.push_str("(nothing captured)");
    } else {
        detail.push_str(&tail.join("\n"));
    }
    let summary: String = match current_job() {
        Some(job) => format!("job {job}: {}", summary.as_ref()),
        None => summary.as_ref().to_string(),
    };
    let id = fail(Some(app), source, &summary, detail);
    let _ = app.emit(
        "logs://fail",
        serde_json::json!({ "id": id, "message": summary.clone(), "job": current_job() }),
    );
    id
}

pub fn events(limit: usize, filter: Option<&str>, job: Option<u64>) -> Vec<Entry> {
    let needle = filter.map(|f| f.to_ascii_lowercase());
    let Ok(logs) = LOGS.lock() else {
        return Vec::new();
    };
    logs.iter()
        .rev()
        .filter(|e| job.map(|j| e.job == Some(j)).unwrap_or(true))
        .filter(|e| match &needle {
            None => true,
            Some(n) if n.is_empty() => true,
            Some(n) => {
                e.message.to_ascii_lowercase().contains(n)
                    || e.source.to_ascii_lowercase().contains(n)
                    || e.level.to_ascii_lowercase().contains(n)
                    || e.detail
                        .as_deref()
                        .map(|d| d.to_ascii_lowercase().contains(n))
                        .unwrap_or(false)
            }
        })
        .take(limit.min(CAPACITY))
        .cloned()
        .collect()
}

pub fn get(id: u64) -> Option<Entry> {
    LOGS.lock()
        .ok()
        .and_then(|logs| logs.iter().find(|e| e.id == id).cloned())
}

pub fn clear() {
    if let Ok(mut logs) = LOGS.lock() {
        logs.clear();
    }
}

pub fn count() -> usize {
    LOGS.lock().map(|l| l.len()).unwrap_or(0)
}

/// tracing layer: mirrors every event into the ring so nothing is lost when the
/// terminal is not attached.
pub struct Layer {
    pub app: Option<AppHandle>,
}

struct Visitor(String);

impl tracing::field::Visit for Visitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.0.push_str(value);
        } else if !value.is_empty() {
            if !self.0.is_empty() {
                self.0.push(' ');
            }
            self.0.push_str(field.name());
            self.0.push('=');
            self.0.push_str(value);
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0.push_str(&format!("{value:?}"));
        }
    }
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Layer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if self.app.is_none() && count() >= CAPACITY {
            return;
        }
        let meta = event.metadata();
        let mut visitor = Visitor(String::new());
        event.record(&mut visitor);
        let level = match *meta.level() {
            tracing::Level::ERROR => "error",
            tracing::Level::WARN => "warn",
            tracing::Level::INFO => "info",
            _ => "debug",
        };
        // `ui` is the frontend's own channel; keep the target as the source.
        push(self.app.as_ref(), level, meta.target(), &visitor.0, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The buffer is global; serialize the tests that clear it.
    static LOCK: Mutex<()> = Mutex::new(());

    fn guard() -> std::sync::MutexGuard<'static, ()> {
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn newest_first_and_filtered() {
        let _g = guard();
        clear();
        push_inner("info", "rip", "starting title 1", None);
        let fail_id = push_inner("error", "ffmpeg", "mux failed", Some("line 1\nline 2"));
        assert_eq!(
            get(fail_id).unwrap().detail.as_deref(),
            Some("line 1\nline 2")
        );
        let all = events(10, None, None);
        assert_eq!(all[0].id, fail_id, "newest first");
        assert_eq!(events(10, Some("mux"), None).len(), 1);
        assert_eq!(events(10, Some("nothing-here"), None).len(), 0);
        assert_eq!(events(10, Some("ffmpeg"), None).len(), 1, "source matches");
        clear();
    }

    #[test]
    fn redacts_keys_queries_and_home() {
        let _g = guard();
        let home = std::env::var("HOME").unwrap_or_default();
        let text = format!("{home}/x?token=abc abcdef0123456789abcdef0123456789");
        let out = redact(&text);
        assert!(out.starts_with("~/"), "{out}");
        assert!(out.contains("?…"), "{out}");
        assert!(out.contains("abcdef01…"), "{out}");
        assert!(!out.contains("abcdef0123456789abcdef0123456789"), "{out}");
    }

    #[test]
    fn fail_job_carries_context_and_tail() {
        let _g = guard();
        clear();
        push_inner("debug", "ffmpeg", "Opening decoder", None);
        push_inner("error", "ffmpeg", "mux failed", None);
        let id = push_inner("error", "rip", "rip failed", Some("ctx"));
        let tail = tail_of("ffmpeg", 10);
        assert_eq!(tail, vec!["debug Opening decoder", "error mux failed"]);
        assert_eq!(get(id).unwrap().source, "rip");
        clear();
    }

    #[test]
    fn a_job_tags_its_lines_and_closes() {
        let _g = guard();
        clear();
        let job = begin_job(7, "rip", "title 1");
        push_inner("info", "ffmpeg", "frame 1", None);
        drop(job);
        let rows = events(10, None, Some(7));
        assert!(rows.iter().all(|e| e.job == Some(7)));
        assert!(rows.iter().any(|e| e.message.contains("job 7 started")));
        assert!(rows.iter().any(|e| e.message.contains("job 7 finished")));
        assert_eq!(events(10, None, Some(8)).len(), 0);
        assert!(current_job().is_none(), "guard pops the job");
        clear();
    }

    #[test]
    fn bounded() {
        let _g = guard();
        clear();
        for i in 0..CAPACITY + 5 {
            push_inner("info", "t", format!("e{i}"), None);
        }
        assert_eq!(count(), CAPACITY);
        clear();
    }
}
