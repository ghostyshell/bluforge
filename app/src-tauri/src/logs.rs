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

fn push_inner(level: &str, source: &str, message: impl AsRef<str>, detail: Option<&str>) -> u64 {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let entry = Entry {
        id,
        at: now_ms(),
        level: level.to_string(),
        source: source.to_string(),
        message: message.as_ref().to_string(),
        detail: detail.map(|d| d.trim_end().to_string()),
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

/// Drain a child's stderr into the log, one entry per line, from a thread.
pub fn drain_stderr(
    mut stderr: std::process::ChildStderr,
    source: &'static str,
    app: Option<AppHandle>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(&mut stderr);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            if line.trim().is_empty() {
                continue;
            }
            let level =
                if line.contains("Error") || line.contains("error") || line.contains("failed") {
                    "error"
                } else {
                    "debug"
                };
            push(app.as_ref(), level, source, &line, None);
        }
    })
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

    #[test]
    fn newest_first_and_filtered() {
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
    fn bounded() {
        clear();
        for i in 0..CAPACITY + 5 {
            push_inner("info", "t", format!("e{i}"), None);
        }
        assert_eq!(count(), CAPACITY);
        clear();
    }
}
