//! Bridges domain events to OS desktop notifications.
//!
//! Reads the current `AppConfig.notifications_enabled` flag on every
//! event so toggling the setting from the UI takes effect immediately,
//! enriches the body with file name and total size, and debounces
//! bursts of completions through `NotificationGrouper`. Average speed
//! and duration are deliberately omitted until the read model surfaces
//! a dedicated transfer-start metric (see `complete_body` for context).
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
use crate::domain::notification::{NotificationDecision, NotificationGrouper, format_size};
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
        // Side-effects that must run regardless of the user's
        // notification preference (logs, observability) live before
        // the gate so disabling toasts never silences error reporting.
        if let DomainEvent::DownloadFailed { id, error } = event {
            tracing::error!(download_id = id.0, error = %error, "download failed");
        }

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
                            &complete_body(id.0, detail.as_ref()),
                        );
                    }
                }
            }
            DomainEvent::DownloadFailed { id, error } => {
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

fn complete_body(id: u64, detail: Option<&DownloadDetailView>) -> String {
    let Some(d) = detail else {
        return format!("Download #{id} finished successfully");
    };
    let mut parts: Vec<String> = vec![d.file_name.clone()];
    if let Some(total) = d.total_bytes {
        parts.push(format_size(total));
    }
    // Speed + duration intentionally omitted: `DownloadDetailView`
    // exposes `created_at` (queue admission) but no transfer-start
    // marker, so any duration computed here would inflate by the time
    // the download spent queued or paused. Re-introduce when the read
    // model surfaces an active-transfer metric (e.g., via the history
    // entry produced on completion).
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
    // Reserve one slot for the ellipsis so the rendered string respects
    // the configured cap exactly. `MAX_ERROR_BODY_CHARS` is a const ≥ 2
    // so the saturating subtraction is here for defence-in-depth.
    let payload_chars = MAX_ERROR_BODY_CHARS.saturating_sub(1);
    let mut out: String = error.chars().take(payload_chars).collect();
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
            complete_body(42, None),
            "Download #42 finished successfully"
        );
    }

    #[test]
    fn test_complete_body_combines_filename_and_size() {
        let d = detail("video.mp4", Some(10 * 1024 * 1024), 1_000);
        assert_eq!(complete_body(7, Some(&d)), "video.mp4 · 10.0 MB");
    }

    #[test]
    fn test_complete_body_renders_filename_only_when_size_unknown() {
        let d = detail("stream.ts", None, 100);
        assert_eq!(complete_body(1, Some(&d)), "stream.ts");
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
    fn test_truncate_error_keeps_short_strings_verbatim() {
        let short = "x".repeat(MAX_ERROR_BODY_CHARS);
        assert_eq!(truncate_error(&short), short);
    }

    #[test]
    fn test_truncate_error_caps_at_max_chars_including_ellipsis() {
        let long = "x".repeat(MAX_ERROR_BODY_CHARS * 2);
        let truncated = truncate_error(&long);
        assert_eq!(truncated.chars().count(), MAX_ERROR_BODY_CHARS);
        assert!(truncated.ends_with('…'));
    }

    #[test]
    fn test_aggregated_body_format() {
        assert_eq!(aggregated_body(3), "3 downloads completed");
        assert_eq!(aggregated_body(7), "7 downloads completed");
    }
}
