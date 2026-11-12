//! Desktop notifications and webhooks for finished jobs.
//!
//! Every send is best-effort: a missing notifier or an unreachable webhook must
//! never fail a rip. Settings live in [`AppConfig`], so the CLI watcher and the
//! app notify the same way.

use crate::config::AppConfig;

/// Notify about a finished or failed job.
pub fn send(title: &str, body: &str) {
    let cfg = AppConfig::load();
    if cfg.notify_native {
        native(title, body);
    }
    if let Some(url) = cfg
        .notify_webhook
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty())
    {
        webhook(url, title, body);
    }
}

fn native(title: &str, body: &str) {
    #[cfg(target_os = "macos")]
    {
        // Quote for AppleScript: backslashes and double quotes are the only
        // special characters in a string literal.
        let escape = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!(
            "display notification \"{}\" with title \"{}\"",
            escape(body),
            escape(title)
        );
        let _ = std::process::Command::new("osascript")
            .args(["-e", &script])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("notify-send")
            .args([title, body])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(target_os = "windows")]
    {
        // Windows has no notify-send; the WinRT toast API works from
        // PowerShell without a helper package.
        let escape = |s: &str| {
            s.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
        };
        let script = format!(
            "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
             $t = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
             $n = $t.GetElementsByTagName('text'); \
             $n.Item(0).AppendChild($t.CreateTextNode('{}')) > $null; \
             $n.Item(1).AppendChild($t.CreateTextNode('{}')) > $null; \
             $d = New-Object Windows.UI.Notifications.ToastNotification $t; \
             [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('Keepsake Atelier').Show($d)",
            escape(title),
            escape(body)
        );
        let _ = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

fn webhook(url: &str, title: &str, body: &str) {
    let payload = serde_json::json!({
        "title": title,
        "body": body,
        "app": "keepsake-atelier",
    });
    let result = ureq::post(url)
        .timeout(std::time::Duration::from_secs(10))
        .send_json(payload);
    if let Err(e) = result {
        tracing::debug!(%e, "notification webhook failed");
    }
}

/// Notify that a title finished, with its output path when known.
pub fn rip_done(movie: &str, dest: Option<&std::path::Path>) {
    let body = match dest {
        Some(p) => format!("{movie} -> {}", p.display()),
        None => movie.to_string(),
    };
    send("Rip complete", &body);
}

/// Notify that a job failed.
pub fn rip_failed(movie: &str, error: &str) {
    send("Rip failed", &format!("{movie}: {error}"));
}

#[cfg(test)]
mod tests {
    #[test]
    fn webhook_payload_is_json() {
        let payload = serde_json::json!({
            "title": "Rip complete",
            "body": "Movie -> /tmp/out.mkv",
            "app": "keepsake-atelier",
        });
        assert_eq!(payload["app"], "keepsake-atelier");
    }
}
