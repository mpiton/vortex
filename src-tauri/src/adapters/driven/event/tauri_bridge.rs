use serde_json::json;
use tauri::{AppHandle, Emitter};

use crate::domain::event::DomainEvent;
use crate::domain::ports::driven::EventBus;

/// Subscribes to the EventBus and emits each event to the Tauri webview.
pub fn spawn_tauri_event_bridge(app_handle: AppHandle, event_bus: &dyn EventBus) {
    event_bus.subscribe(Box::new(move |event: &DomainEvent| {
        if !should_forward_to_frontend(event) {
            return;
        }
        let (name, payload) = to_tauri_event(event);
        app_handle.emit(name, payload).ok();
    }));
}

/// Gate `DomainEvent::DownloadCompleted` at the bridge.
///
/// The engine publishes `DownloadCompleted` synchronously when the last
/// segment is written. `QueueManager` subscribes to that same event to
/// persist `state = Completed` to SQLite and then re-publishes
/// `DownloadCompletedPersisted`. Only the second one should reach the
/// frontend — otherwise the UI receives two `"download-completed"`
/// notifications per download and the first refetch runs against the
/// pre-persist state (the race this flow exists to fix).
fn should_forward_to_frontend(event: &DomainEvent) -> bool {
    !matches!(event, DomainEvent::DownloadCompleted { .. })
}

fn event_name(event: &DomainEvent) -> &'static str {
    match event {
        DomainEvent::DownloadCreated { .. } => "download-created",
        DomainEvent::DownloadStarted { .. } => "download-started",
        DomainEvent::DownloadPaused { .. } => "download-paused",
        DomainEvent::DownloadResumed { .. } => "download-resumed",
        DomainEvent::DownloadResumedFromWait { .. } => "download-resumed-from-wait",
        DomainEvent::DownloadCompleted { .. } => "download-completed",
        // Post-persist notification: same frontend event, guaranteed to
        // fire after QueueManager has written state = Completed to SQLite.
        DomainEvent::DownloadCompletedPersisted { .. } => "download-completed",
        DomainEvent::DownloadFailed { .. } => "download-failed",
        DomainEvent::DownloadRetrying { .. } => "download-retrying",
        DomainEvent::DownloadWaiting { .. } => "download-waiting",
        DomainEvent::DownloadChecking { .. } => "download-checking",
        DomainEvent::DownloadCancelled { .. } => "download-cancelled",
        DomainEvent::DownloadRemoved { .. } => "download-removed",
        DomainEvent::DownloadExtracting { .. } => "download-extracting",
        DomainEvent::DownloadProgress { .. } => "download-progress",
        DomainEvent::SegmentStarted { .. } => "segment-started",
        DomainEvent::SegmentCompleted { .. } => "segment-completed",
        DomainEvent::SegmentFailed { .. } => "segment-failed",
        DomainEvent::PluginLoaded { .. } => "plugin-loaded",
        DomainEvent::PluginUnloaded { .. } => "plugin-unloaded",
        DomainEvent::PackageCreated { .. } => "package-created",
        DomainEvent::ClipboardUrlDetected { .. } => "clipboard-url-detected",
        DomainEvent::SettingsUpdated => "settings-updated",
        DomainEvent::ChecksumVerified { .. } => "checksum-verified",
        DomainEvent::ChecksumMismatch { .. } => "checksum-mismatch",
        DomainEvent::DownloadPrioritySet { .. } => "download-priority-set",
        DomainEvent::QueueReordered { .. } => "queue-reordered",
        DomainEvent::DownloadDirectoryChanged { .. } => "download-directory-changed",
    }
}

fn event_payload(event: &DomainEvent) -> serde_json::Value {
    match event {
        DomainEvent::DownloadCreated { id }
        | DomainEvent::DownloadStarted { id }
        | DomainEvent::DownloadPaused { id }
        | DomainEvent::DownloadResumed { id }
        | DomainEvent::DownloadResumedFromWait { id }
        | DomainEvent::DownloadCompleted { id }
        | DomainEvent::DownloadCompletedPersisted { id }
        | DomainEvent::DownloadCancelled { id }
        | DomainEvent::DownloadRemoved { id }
        | DomainEvent::DownloadWaiting { id }
        | DomainEvent::DownloadChecking { id }
        | DomainEvent::DownloadExtracting { id } => json!({ "id": id.0 }),

        DomainEvent::DownloadFailed { id, error } => json!({ "id": id.0, "error": error }),
        DomainEvent::DownloadRetrying { id, attempt } => {
            json!({ "id": id.0, "attempt": attempt })
        }
        DomainEvent::DownloadProgress {
            id,
            downloaded_bytes,
            total_bytes,
        } => {
            json!({ "id": id.0, "downloadedBytes": downloaded_bytes, "totalBytes": total_bytes })
        }

        DomainEvent::SegmentStarted {
            download_id,
            segment_id,
            start_byte,
            end_byte,
        } => {
            let end_byte_payload = if *end_byte == u64::MAX {
                json!(-1_i64)
            } else {
                json!(*end_byte)
            };
            json!({ "downloadId": download_id.0, "segmentId": segment_id, "startByte": start_byte, "endByte": end_byte_payload })
        }
        DomainEvent::SegmentCompleted {
            download_id,
            segment_id,
        } => {
            json!({ "downloadId": download_id.0, "segmentId": segment_id })
        }
        DomainEvent::SegmentFailed {
            download_id,
            segment_id,
            error,
        } => {
            json!({ "downloadId": download_id.0, "segmentId": segment_id, "error": error })
        }

        DomainEvent::PluginLoaded { name, version } => {
            json!({ "name": name, "version": version })
        }
        DomainEvent::PluginUnloaded { name } => json!({ "name": name }),
        DomainEvent::PackageCreated { id, name } => json!({ "id": id.to_string(), "name": name }),
        DomainEvent::ClipboardUrlDetected { urls } => json!({ "urls": urls }),
        DomainEvent::SettingsUpdated => json!({}),
        DomainEvent::ChecksumVerified {
            id,
            algorithm,
            checksum,
        } => json!({ "id": id.0, "algorithm": algorithm, "checksum": checksum }),
        DomainEvent::ChecksumMismatch {
            id,
            algorithm,
            expected,
            computed,
        } => {
            json!({ "id": id.0, "algorithm": algorithm, "expected": expected, "computed": computed })
        }
        DomainEvent::DownloadPrioritySet { id, priority } => {
            json!({ "id": id.0, "priority": priority })
        }
        DomainEvent::QueueReordered { affected_ids } => {
            let ids: Vec<u64> = affected_ids.iter().map(|id| id.0).collect();
            json!({ "affectedIds": ids })
        }
        DomainEvent::DownloadDirectoryChanged {
            id,
            new_destination_path,
        } => {
            json!({ "id": id.0, "newDestinationPath": new_destination_path })
        }
    }
}

fn to_tauri_event(event: &DomainEvent) -> (&'static str, serde_json::Value) {
    (event_name(event), event_payload(event))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::download::DownloadId;

    #[test]
    fn test_download_completed_persisted_is_the_frontend_completion_event() {
        // Post-persist event: must be forwarded and carry the `download-completed`
        // name the frontend listens to.
        assert!(should_forward_to_frontend(
            &DomainEvent::DownloadCompletedPersisted { id: DownloadId(5) }
        ));
        let (name, payload) =
            to_tauri_event(&DomainEvent::DownloadCompletedPersisted { id: DownloadId(42) });
        assert_eq!(name, "download-completed");
        assert_eq!(payload["id"], 42);
    }

    #[test]
    fn test_pre_persist_download_completed_is_not_forwarded() {
        // The engine's DownloadCompleted fires before SQLite is written; only
        // DownloadCompletedPersisted should reach the frontend so a re-fetch
        // never races the write.
        assert!(!should_forward_to_frontend(
            &DomainEvent::DownloadCompleted { id: DownloadId(7) }
        ));
    }

    #[test]
    fn test_event_name_download_variants() {
        assert_eq!(
            event_name(&DomainEvent::DownloadCreated { id: DownloadId(1) }),
            "download-created"
        );
        assert_eq!(
            event_name(&DomainEvent::DownloadStarted { id: DownloadId(1) }),
            "download-started"
        );
        assert_eq!(
            event_name(&DomainEvent::DownloadPaused { id: DownloadId(1) }),
            "download-paused"
        );
        assert_eq!(
            event_name(&DomainEvent::DownloadResumed { id: DownloadId(1) }),
            "download-resumed"
        );
        assert_eq!(
            event_name(&DomainEvent::DownloadResumedFromWait { id: DownloadId(1) }),
            "download-resumed-from-wait"
        );
        assert_eq!(
            event_name(&DomainEvent::DownloadCompleted { id: DownloadId(1) }),
            "download-completed"
        );
        assert_eq!(
            event_name(&DomainEvent::DownloadFailed {
                id: DownloadId(1),
                error: "err".into()
            }),
            "download-failed"
        );
        assert_eq!(
            event_name(&DomainEvent::DownloadRetrying {
                id: DownloadId(1),
                attempt: 1
            }),
            "download-retrying"
        );
        assert_eq!(
            event_name(&DomainEvent::DownloadWaiting { id: DownloadId(1) }),
            "download-waiting"
        );
        assert_eq!(
            event_name(&DomainEvent::DownloadChecking { id: DownloadId(1) }),
            "download-checking"
        );
        assert_eq!(
            event_name(&DomainEvent::DownloadRemoved { id: DownloadId(1) }),
            "download-removed"
        );
        assert_eq!(
            event_name(&DomainEvent::DownloadExtracting { id: DownloadId(1) }),
            "download-extracting"
        );
        assert_eq!(
            event_name(&DomainEvent::DownloadProgress {
                id: DownloadId(1),
                downloaded_bytes: 0,
                total_bytes: 100
            }),
            "download-progress"
        );
    }

    #[test]
    fn test_event_name_segment_variants() {
        let started = DomainEvent::SegmentStarted {
            download_id: DownloadId(1),
            segment_id: 0,
            start_byte: 0,
            end_byte: 1024,
        };
        assert_eq!(event_name(&started), "segment-started");
        let (event, payload) = to_tauri_event(&started);
        assert_eq!(event, "segment-started");
        assert_eq!(payload["downloadId"].as_u64(), Some(1));
        assert_eq!(payload["segmentId"].as_u64(), Some(0));
        assert_eq!(payload["startByte"].as_u64(), Some(0));
        assert_eq!(payload["endByte"].as_u64(), Some(1024));

        let sentinel = DomainEvent::SegmentStarted {
            download_id: DownloadId(2),
            segment_id: 3,
            start_byte: 99,
            end_byte: u64::MAX,
        };
        let (_, sentinel_payload) = to_tauri_event(&sentinel);
        assert_eq!(sentinel_payload["downloadId"].as_u64(), Some(2));
        assert_eq!(sentinel_payload["segmentId"].as_u64(), Some(3));
        assert_eq!(sentinel_payload["startByte"].as_u64(), Some(99));
        assert_eq!(sentinel_payload["endByte"].as_i64(), Some(-1));

        assert_eq!(
            event_name(&DomainEvent::SegmentCompleted {
                download_id: DownloadId(1),
                segment_id: 0
            }),
            "segment-completed"
        );
        assert_eq!(
            event_name(&DomainEvent::SegmentFailed {
                download_id: DownloadId(1),
                segment_id: 0,
                error: "err".into()
            }),
            "segment-failed"
        );
    }

    #[test]
    fn test_event_name_plugin_variants() {
        assert_eq!(
            event_name(&DomainEvent::PluginLoaded {
                name: "p".into(),
                version: "1.0".into()
            }),
            "plugin-loaded"
        );
        assert_eq!(
            event_name(&DomainEvent::PluginUnloaded { name: "p".into() }),
            "plugin-unloaded"
        );
        assert_eq!(
            event_name(&DomainEvent::PackageCreated {
                id: 1,
                name: "pkg".into()
            }),
            "package-created"
        );
    }

    #[test]
    fn test_event_payload_download_progress_camel_case() {
        let event = DomainEvent::DownloadProgress {
            id: DownloadId(7),
            downloaded_bytes: 512,
            total_bytes: 1024,
        };
        let (_, payload) = to_tauri_event(&event);
        assert_eq!(payload["id"], 7);
        assert_eq!(payload["downloadedBytes"], 512);
        assert_eq!(payload["totalBytes"], 1024);
        // Verify snake_case keys are not present
        assert!(payload.get("downloaded_bytes").is_none());
        assert!(payload.get("total_bytes").is_none());
    }

    #[test]
    fn test_event_payload_segment_camel_case() {
        let event = DomainEvent::SegmentCompleted {
            download_id: DownloadId(3),
            segment_id: 2,
        };
        let (_, payload) = to_tauri_event(&event);
        assert_eq!(payload["downloadId"], 3);
        assert_eq!(payload["segmentId"], 2);
        // Verify snake_case keys are not present
        assert!(payload.get("download_id").is_none());
        assert!(payload.get("segment_id").is_none());
    }

    #[test]
    fn test_event_payload_download_removed() {
        let event = DomainEvent::DownloadRemoved { id: DownloadId(9) };
        let (name, payload) = to_tauri_event(&event);

        assert_eq!(name, "download-removed");
        assert_eq!(payload["id"], 9);
    }

    #[test]
    fn test_event_name_clipboard_url_detected() {
        assert_eq!(
            event_name(&DomainEvent::ClipboardUrlDetected {
                urls: vec!["https://example.com".into()]
            }),
            "clipboard-url-detected"
        );
    }

    #[test]
    fn test_settings_updated_event_bridge_mapping() {
        let event = DomainEvent::SettingsUpdated;
        let (name, payload) = to_tauri_event(&event);
        assert_eq!(name, "settings-updated");
        assert_eq!(payload, serde_json::json!({}));
    }

    #[test]
    fn test_download_directory_changed_event_bridge_mapping() {
        let event = DomainEvent::DownloadDirectoryChanged {
            id: DownloadId(11),
            new_destination_path: "/new/folder/file.bin".to_string(),
        };
        let (name, payload) = to_tauri_event(&event);
        assert_eq!(name, "download-directory-changed");
        assert_eq!(payload["id"], 11);
        assert_eq!(payload["newDestinationPath"], "/new/folder/file.bin");
        assert!(payload.get("new_destination_path").is_none());
    }

    #[test]
    fn test_event_payload_clipboard_url_detected() {
        let event = DomainEvent::ClipboardUrlDetected {
            urls: vec![
                "https://a.com/file.zip".into(),
                "ftp://b.com/data.tar".into(),
            ],
        };
        let (name, payload) = to_tauri_event(&event);
        assert_eq!(name, "clipboard-url-detected");
        let urls = payload["urls"].as_array().unwrap();
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://a.com/file.zip");
        assert_eq!(urls[1], "ftp://b.com/data.tar");
    }
}
