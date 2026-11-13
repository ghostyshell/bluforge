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
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static LOGS: Mutex<VecDeque<Entry>> = Mutex::new(VecDeque::new());

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
    let id = fail(Some(app), source, summary.as_ref(), detail);
    let _ = app.emit(
        "logs://fail",
        serde_json::json!({ "id": id, "message": summary.as_ref() }),
    );
    id
}

pub fn events(limit: usize, filter: Option<&str>) -> Vec<Entry> {
    let needle = filter.map(|f| f.to_ascii_lowercase());
    let Ok(logs) = LOGS.lock() else {
        return Vec::new();
    };
    logs.iter()
        .rev()
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
        let all = events(10, None);
        assert_eq!(all[0].id, fail_id, "newest first");
        assert_eq!(events(10, Some("mux")).len(), 1);
        assert_eq!(events(10, Some("nothing-here")).len(), 0);
        assert_eq!(events(10, Some("ffmpeg")).len(), 1, "source matches");
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
