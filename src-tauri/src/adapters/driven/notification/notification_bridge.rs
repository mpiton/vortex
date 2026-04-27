//! Bridges domain events to OS desktop notifications.
//!
//! Reads the current `AppConfig.notifications_enabled` flag on every
//! event so toggling the setting from the UI takes effect immediately,
//! enriches the body with file name / size / speed / duration, and
//! debounces bursts of completions through `NotificationGrouper`.
//!
//! ## Click action limitation
//!
//! `tauri-plugin-notification` 2.3.3 desktop API delegates to
//! `notify_rust` and intentionally drops the `NotificationHandle` so
//! the closure returned by the OS for "user clicked the toast" is
//! unreachable. PRD §7.5 mentions click-to-open and click-to-focus as
//! desired UX; that requires either replacing the plugin with direct
//! `notify_rust` use (Linux/macOS) or upstream patch. Tracked: revisit
//! when tauri-plugin-notification exposes `on_event`.

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;
use tracing::warn;

use crate::domain::error::DomainError;
use crate::domain::event::DomainEvent;
use crate::domain::model::views::DownloadDetailView;
use crate::domain::notification::{
    NotificationDecision, NotificationGrouper, format_duration, format_size, format_speed,
};
use crate::domain::ports::driven::{ConfigStore, DownloadReadRepository, EventBus};

/// Cap error messages embedded in notification bodies. Long stack traces
/// or HTML bodies returned by hosters would otherwise overflow the OS
/// toast and may leak credentials embedded in the URL.
const MAX_ERROR_BODY_CHARS: usize = 200;

/// Subscribe to the EventBus and surface key download lifecycle events
/// as desktop notifications.
///
/// The bridge owns its grouper state through `Arc<Mutex<…>>` because
/// `EventBus::subscribe` requires `Fn` (re-entrant per event); the
/// mutex is contended only on the notification path which is already
/// dominated by I/O latency.
pub fn spawn_notification_bridge(
    app_handle: AppHandle,
    event_bus: &dyn EventBus,
    config_store: Arc<dyn ConfigStore>,
    read_repo: Arc<dyn DownloadReadRepository>,
) {
    let grouper = Arc::new(Mutex::new(NotificationGrouper::new()));

    event_bus.subscribe(Box::new(move |event: &DomainEvent| {
        if !is_notifications_enabled(config_store.as_ref()) {
            return;
        }

        match event {
            DomainEvent::DownloadCompleted { id } => {
                let now = epoch_secs();
                let decision = match grouper.lock() {
                    Ok(mut g) => g.record(now),
                    Err(poisoned) => {
                        warn!("notification grouper mutex poisoned; recovering");
                        poisoned.into_inner().record(now)
                    }
                };
                match decision {
                    NotificationDecision::Suppress => {}
                    NotificationDecision::ShowAggregated { count } => {
                        send(&app_handle, "Downloads complete", &aggregated_body(count));
                    }
                    NotificationDecision::ShowSingle => {
                        let detail = lookup_detail(read_repo.as_ref(), id.0);
                        send(
                            &app_handle,
                            "Download complete",
                            &complete_body(id.0, detail.as_ref(), now),
                        );
                    }
                }
            }
            DomainEvent::DownloadFailed { id, error } => {
                tracing::error!(download_id = id.0, error = %error, "download failed");
                let detail = lookup_detail(read_repo.as_ref(), id.0);
                send(
                    &app_handle,
                    "Download failed",
                    &failed_body(id.0, detail.as_ref(), error),
                );
            }
            _ => {}
        }
    }));
}

fn is_notifications_enabled(config_store: &dyn ConfigStore) -> bool {
    match config_store.get_config() {
        Ok(c) => c.notifications_enabled,
        Err(e) => {
            warn!("failed to read notifications_enabled flag: {e}");
            // Default-allow on read error so a single corrupt config write
            // does not silently disable user-visible feedback.
            true
        }
    }
}

fn lookup_detail(read_repo: &dyn DownloadReadRepository, id: u64) -> Option<DownloadDetailView> {
    use crate::domain::model::download::DownloadId;
    match read_repo.find_download_detail(DownloadId(id)) {
        Ok(view) => view,
        Err(DomainError::NotFound(_)) => None,
        Err(e) => {
            warn!(download_id = id, error = %e, "notification: detail lookup failed");
            None
        }
    }
}

fn complete_body(id: u64, detail: Option<&DownloadDetailView>, now_secs: u64) -> String {
    let Some(d) = detail else {
        return format!("Download #{id} finished successfully");
    };
    let mut parts: Vec<String> = vec![d.file_name.clone()];
    if let Some(total) = d.total_bytes {
        parts.push(format_size(total));
    }
    let duration = now_secs.saturating_sub(d.created_at);
    if duration > 0 {
        if let Some(total) = d.total_bytes {
            parts.push(format_speed(total / duration.max(1)));
        }
        parts.push(format_duration(duration));
    }
    parts.join(" · ")
}

fn failed_body(id: u64, detail: Option<&DownloadDetailView>, error: &str) -> String {
    let name = detail
        .map(|d| d.file_name.clone())
        .unwrap_or_else(|| format!("#{id}"));
    let truncated = truncate_error(error);
    format!("{name} · Error: {truncated}")
}

fn aggregated_body(count: usize) -> String {
    format!("{count} downloads completed")
}

fn truncate_error(error: &str) -> String {
    if error.chars().count() <= MAX_ERROR_BODY_CHARS {
        return error.to_string();
    }
    let mut out: String = error.chars().take(MAX_ERROR_BODY_CHARS).collect();
    out.push('…');
    out
}

fn send(app_handle: &AppHandle, title: &str, body: &str) {
    if let Err(e) = app_handle
        .notification()
        .builder()
        .title(title)
        .body(body)
        .show()
    {
        warn!("failed to show notification '{title}': {e}");
    }
}

fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::download::{DownloadId, DownloadState};
    use crate::domain::model::views::DownloadDetailView;

    fn detail(file_name: &str, total_bytes: Option<u64>, created_at: u64) -> DownloadDetailView {
        DownloadDetailView {
            id: DownloadId(7),
            file_name: file_name.to_string(),
            url: "https://example.com/file".into(),
            source_hostname: "example.com".into(),
            state: DownloadState::Completed,
            progress_percent: 100.0,
            speed_bytes_per_sec: 0,
            downloaded_bytes: total_bytes.unwrap_or(0),
            total_bytes,
            eta_seconds: None,
            segments: vec![],
            checksum_expected: None,
            checksum_computed: None,
            checksum_algorithm: None,
            destination_path: "/tmp/file".into(),
            module_name: None,
            account_name: None,
            resume_supported: true,
            retry_count: 0,
            max_retries: 5,
            created_at,
            updated_at: created_at + 60,
        }
    }

    #[test]
    fn test_complete_body_falls_back_when_detail_missing() {
        assert_eq!(
            complete_body(42, None, 100),
            "Download #42 finished successfully"
        );
    }

    #[test]
    fn test_complete_body_combines_filename_size_speed_duration() {
        // 10 MiB downloaded in 5 s → 2 MiB/s avg.
        let d = detail("video.mp4", Some(10 * 1024 * 1024), 1_000);
        let body = complete_body(7, Some(&d), 1_005);
        assert_eq!(body, "video.mp4 · 10.0 MB · 2.0 MB/s · 5s");
    }

    #[test]
    fn test_complete_body_omits_speed_when_duration_zero() {
        let d = detail("instant.bin", Some(1024), 100);
        let body = complete_body(1, Some(&d), 100);
        assert_eq!(body, "instant.bin · 1.0 KB");
    }

    #[test]
    fn test_complete_body_handles_unknown_size() {
        let d = detail("stream.ts", None, 100);
        let body = complete_body(1, Some(&d), 130);
        assert_eq!(body, "stream.ts · 30s");
    }

    #[test]
    fn test_failed_body_includes_filename_and_error() {
        let d = detail("archive.zip", Some(1024), 0);
        assert_eq!(
            failed_body(9, Some(&d), "connection reset"),
            "archive.zip · Error: connection reset"
        );
    }

    #[test]
    fn test_failed_body_uses_id_when_detail_missing() {
        assert_eq!(failed_body(99, None, "timeout"), "#99 · Error: timeout");
    }

    #[test]
    fn test_failed_body_truncates_long_error_messages() {
        let long = "x".repeat(MAX_ERROR_BODY_CHARS * 2);
        let body = failed_body(1, None, &long);
        // ASCII path: byte length == char count + ellipsis.
        let chars: Vec<char> = body.chars().collect();
        assert!(chars.last() == Some(&'…'));
        // Body = "#1 · Error: " (12 chars) + cap chars + ellipsis.
        let prefix = "#1 · Error: ";
        let payload: String = chars
            .iter()
            .skip(prefix.chars().count())
            .take(MAX_ERROR_BODY_CHARS)
            .collect();
        assert_eq!(payload.chars().count(), MAX_ERROR_BODY_CHARS);
    }

    #[test]
    fn test_aggregated_body_format() {
        assert_eq!(aggregated_body(3), "3 downloads completed");
        assert_eq!(aggregated_body(7), "7 downloads completed");
    }
}
