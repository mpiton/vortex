use std::sync::Arc;

use crate::domain::event::DomainEvent;
use crate::domain::ports::driven::EventBus;

use super::download_log_store::DownloadLogStore;

pub fn spawn_download_log_bridge(event_bus: &dyn EventBus, store: Arc<DownloadLogStore>) {
    event_bus.subscribe(Box::new(move |event| {
        record_download_event(store.as_ref(), event);
    }));
}

fn record_download_event(store: &DownloadLogStore, event: &DomainEvent) {
    match event {
        DomainEvent::DownloadCreated { id } => {
            store.push(id.0, "[INFO] Download created".to_string());
        }
        DomainEvent::DownloadStarted { id } => {
            store.push(id.0, "[INFO] Download started".to_string());
        }
        DomainEvent::DownloadPaused { id } => {
            store.push(id.0, "[INFO] Download paused".to_string());
        }
        DomainEvent::DownloadResumed { id } => {
            store.push(id.0, "[INFO] Download resumed".to_string());
        }
        DomainEvent::DownloadResumedFromWait { id } => {
            store.push(id.0, "[INFO] Download resumed after waiting".to_string());
        }
        DomainEvent::DownloadCompleted { id } => {
            store.push(id.0, "[INFO] Download completed".to_string());
        }
        DomainEvent::DownloadFailed { id, error } => {
            store.push(id.0, format!("[ERROR] Download failed: {error}"));
        }
        DomainEvent::DownloadRetrying { id, attempt } => {
            store.push(
                id.0,
                format!("[WARN] Retrying download (attempt {attempt})"),
            );
        }
        DomainEvent::DownloadWaiting { id } => {
            store.push(id.0, "[INFO] Download waiting".to_string());
        }
        DomainEvent::DownloadChecking { id } => {
            store.push(id.0, "[INFO] Checking download".to_string());
        }
        DomainEvent::DownloadCancelled { id } => {
            store.push(id.0, "[INFO] Download cancelled".to_string());
        }
        DomainEvent::DownloadExtracting { id } => {
            store.push(id.0, "[INFO] Extracting archive".to_string());
        }
        DomainEvent::DownloadRemoved { id } => {
            store.remove(id.0);
        }
        DomainEvent::SegmentStarted {
            download_id,
            segment_id,
            ..
        } => {
            store.push(
                download_id.0,
                format!("[DEBUG] Segment {segment_id} started"),
            );
        }
        DomainEvent::SegmentCompleted {
            download_id,
            segment_id,
        } => {
            store.push(
                download_id.0,
                format!("[DEBUG] Segment {segment_id} completed"),
            );
        }
        DomainEvent::SegmentFailed {
            download_id,
            segment_id,
            error,
        } => {
            store.push(
                download_id.0,
                format!("[ERROR] Segment {segment_id} failed: {error}"),
            );
        }
        DomainEvent::DownloadProgress { .. }
        | DomainEvent::PluginLoaded { .. }
        | DomainEvent::PluginUnloaded { .. }
        | DomainEvent::PackageCreated { .. }
        | DomainEvent::ClipboardUrlDetected { .. }
        | DomainEvent::SettingsUpdated => {}
    }
}

#[cfg(test)]
mod tests {
    use super::record_download_event;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::download::DownloadId;

    use super::DownloadLogStore;

    #[test]
    fn records_download_failure_lines() {
        let store = DownloadLogStore::new(8);

        record_download_event(
            &store,
            &DomainEvent::DownloadFailed {
                id: DownloadId(42),
                error: "timeout".to_string(),
            },
        );

        assert_eq!(
            store.recent(42, 10),
            vec!["[ERROR] Download failed: timeout".to_string()]
        );
    }

    #[test]
    fn ignores_unscoped_events() {
        let store = DownloadLogStore::new(8);

        record_download_event(
            &store,
            &DomainEvent::PluginLoaded {
                name: "yt".to_string(),
                version: "1.0.0".to_string(),
            },
        );

        assert!(store.recent(42, 10).is_empty());
    }

    #[test]
    fn clears_logs_when_download_is_removed() {
        let store = DownloadLogStore::new(8);

        record_download_event(&store, &DomainEvent::DownloadStarted { id: DownloadId(42) });
        record_download_event(&store, &DomainEvent::DownloadRemoved { id: DownloadId(42) });

        assert!(store.recent(42, 10).is_empty());
    }
}
