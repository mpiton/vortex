use crate::domain::model::account::AccountId;
use crate::domain::model::download::DownloadId;
use crate::domain::model::views::HistoryEntry;

/// Read-model projection inputs captured at the moment a `Download` is
/// persisted as `Completed`. Travels on `DomainEvent::DownloadCompletedPersisted`
/// so async subscribers (history/stats recorders) project from a frozen
/// snapshot instead of re-reading the repository, which would race with
/// later `clear` / `remove` / `change-directory` mutations.
///
/// All timestamps are Unix epoch milliseconds (matching `current_timestamp_ms`
/// and the storage representation on `Download`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadCompletedSnapshot {
    pub id: DownloadId,
    pub file_name: String,
    pub url: String,
    pub destination_path: String,
    pub file_size_bytes: Option<u64>,
    pub downloaded_bytes: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

impl DownloadCompletedSnapshot {
    /// Project the snapshot into a fresh `HistoryEntry`.
    ///
    /// `total_bytes` prefers the authoritative `file_size_bytes` (set when
    /// the upstream announces a Content-Length) and falls back to the
    /// running `downloaded_bytes` for streams of unknown size.
    ///
    /// `completed_at` (`HistoryEntry`'s Unix-seconds field) is derived
    /// from `updated_at_ms / 1_000`. `duration_seconds` is clamped to a
    /// `1`-second floor so very short transfers never divide by zero in
    /// `avg_speed`.
    pub fn to_history_entry(&self) -> HistoryEntry {
        let total_bytes = self
            .file_size_bytes
            .filter(|n| *n > 0)
            .unwrap_or(self.downloaded_bytes);
        let elapsed_ms = self.updated_at_ms.saturating_sub(self.created_at_ms);
        let duration_seconds = (elapsed_ms / 1_000).max(1);
        let avg_speed = total_bytes / duration_seconds;
        let completed_at = self.updated_at_ms / 1_000;
        HistoryEntry {
            id: 0,
            download_id: self.id,
            file_name: self.file_name.clone(),
            url: self.url.clone(),
            total_bytes,
            completed_at,
            duration_seconds,
            avg_speed,
            destination_path: self.destination_path.clone(),
        }
    }

    /// Pull the byte count and average speed off this snapshot for the
    /// `statistics` rollup.
    pub fn to_stats_record(&self) -> (u64, u64) {
        let bytes = self
            .file_size_bytes
            .filter(|n| *n > 0)
            .unwrap_or(self.downloaded_bytes);
        let elapsed_ms = self.updated_at_ms.saturating_sub(self.created_at_ms);
        let elapsed_secs = (elapsed_ms / 1_000).max(1);
        let avg_speed = bytes / elapsed_secs;
        (bytes, avg_speed)
    }

    /// Minimal stub for tests that only need the carrier event without
    /// caring about projection inputs. Production publish sites must use
    /// `application::services::queue_manager::build_completed_snapshot`
    /// to derive the snapshot from the persisted aggregate.
    #[cfg(test)]
    pub(crate) fn for_test(id: DownloadId) -> Self {
        Self {
            id,
            file_name: String::new(),
            url: String::new(),
            destination_path: String::new(),
            file_size_bytes: None,
            downloaded_bytes: 0,
            created_at_ms: 0,
            updated_at_ms: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DomainEvent {
    // Download lifecycle
    DownloadCreated {
        id: DownloadId,
    },
    DownloadStarted {
        id: DownloadId,
    },
    DownloadPaused {
        id: DownloadId,
    },
    DownloadResumed {
        id: DownloadId,
    },
    DownloadResumedFromWait {
        id: DownloadId,
    },
    DownloadCompleted {
        id: DownloadId,
    },
    /// Emitted by QueueManager *after* persisting `state = Completed` to
    /// SQLite.  The Tauri bridge forwards this to the frontend so that the
    /// UI re-fetch that follows is guaranteed to read the correct state,
    /// regardless of how fast the download finished.
    ///
    /// Carries a `snapshot` of the projection inputs taken at publish time
    /// so async subscribers (history/stats recorders) cannot race with a
    /// later mutation of the persisted row (clear/remove/change-directory).
    DownloadCompletedPersisted {
        id: DownloadId,
        snapshot: DownloadCompletedSnapshot,
    },
    DownloadFailed {
        id: DownloadId,
        error: String,
    },
    DownloadRetrying {
        id: DownloadId,
        attempt: u32,
    },
    DownloadWaiting {
        id: DownloadId,
    },
    DownloadChecking {
        id: DownloadId,
    },
    DownloadCancelled {
        id: DownloadId,
    },
    DownloadRemoved {
        id: DownloadId,
    },
    DownloadExtracting {
        id: DownloadId,
    },
    DownloadProgress {
        id: DownloadId,
        downloaded_bytes: u64,
        total_bytes: u64,
    },
    /// File checksum was computed and matched the expected value.
    ChecksumVerified {
        id: DownloadId,
        algorithm: String,
        checksum: String,
    },
    /// File checksum was computed but did not match the expected value.
    ChecksumMismatch {
        id: DownloadId,
        algorithm: String,
        expected: String,
        computed: String,
    },

    // Segments
    SegmentStarted {
        download_id: DownloadId,
        segment_id: u32,
        /// Inclusive start byte of this segment's range.
        start_byte: u64,
        /// Exclusive end byte (or u64::MAX when no Range header is used).
        end_byte: u64,
    },
    SegmentCompleted {
        download_id: DownloadId,
        segment_id: u32,
    },
    SegmentFailed {
        download_id: DownloadId,
        segment_id: u32,
        error: String,
    },
    /// A still-running segment was split in two by the dynamic-split
    /// scheduler so the remaining range can be parallelised. The original
    /// segment now ends at `split_at`; a fresh segment with `new_segment_id`
    /// covers `[split_at, original_end)`.
    SegmentSplit {
        download_id: DownloadId,
        original_segment_id: u32,
        new_segment_id: u32,
        split_at: u64,
    },

    // Plugins
    PluginLoaded {
        name: String,
        version: String,
    },
    PluginUnloaded {
        name: String,
    },

    // Packages
    PackageCreated {
        id: u64,
        name: String,
    },

    // Clipboard
    ClipboardUrlDetected {
        urls: Vec<String>,
    },

    // Queue management
    /// Priority of a download changed. The QueueManager listens and re-evaluates
    /// scheduling so a high-priority item starts as soon as a slot is free.
    DownloadPrioritySet {
        id: DownloadId,
        priority: u8,
    },
    /// One or more downloads had their `queue_position` updated (move to top /
    /// bottom or drag-reorder). The QueueManager re-runs scheduling so the new
    /// ordering takes effect on the next free slot.
    QueueReordered {
        affected_ids: Vec<DownloadId>,
    },
    /// A download's on-disk file (and its `.vortex-meta` sidecar when
    /// applicable) was successfully relocated to a new directory. The Tauri
    /// bridge forwards this to the frontend so the UI can refresh the path
    /// shown in the detail panel.
    DownloadDirectoryChanged {
        id: DownloadId,
        new_destination_path: String,
    },

    // Settings
    SettingsUpdated,

    // Accounts
    AccountAdded {
        id: AccountId,
        service_name: String,
    },
    AccountUpdated {
        id: AccountId,
    },
    AccountDeleted {
        id: AccountId,
    },
    /// Emitted by `validate_account` when the upstream service confirms
    /// the credentials. Carries the freshly observed metadata so the
    /// frontend can refresh traffic counters / expiry without round-
    /// tripping back through `list_accounts`.
    AccountValidated {
        id: AccountId,
        latency_ms: Option<u64>,
        traffic_left: Option<u64>,
        traffic_total: Option<u64>,
        valid_until: Option<u64>,
    },
    /// Emitted by `validate_account` when the credentials are rejected
    /// or the upstream service is unreachable.
    AccountValidationFailed {
        id: AccountId,
        error: String,
    },
    /// Emitted after `import_accounts` decrypts the bundle and
    /// successfully persists every entry it contained.
    AccountsImported {
        count: u32,
    },
    /// Emitted after `export_accounts` writes the encrypted bundle to
    /// disk.
    AccountsExported {
        count: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_event_debug() {
        let event = DomainEvent::DownloadStarted { id: DownloadId(42) };
        let s = format!("{event:?}");
        assert!(s.contains("DownloadStarted"));
        assert!(s.contains("42"));
    }

    #[test]
    fn test_domain_event_clone() {
        let event = DomainEvent::DownloadFailed {
            id: DownloadId(1),
            error: "timeout".to_string(),
        };
        let cloned = event.clone();
        assert_eq!(event, cloned);
    }

    #[test]
    fn test_domain_event_partial_eq() {
        let a = DomainEvent::DownloadCompleted { id: DownloadId(5) };
        let b = DomainEvent::DownloadCompleted { id: DownloadId(5) };
        let c = DomainEvent::DownloadCompleted { id: DownloadId(6) };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_segment_completed_event() {
        let event = DomainEvent::SegmentCompleted {
            download_id: DownloadId(10),
            segment_id: 3,
        };
        assert_eq!(
            event,
            DomainEvent::SegmentCompleted {
                download_id: DownloadId(10),
                segment_id: 3
            }
        );
    }

    #[test]
    fn test_plugin_events() {
        let loaded = DomainEvent::PluginLoaded {
            name: "my-plugin".to_string(),
            version: "1.0.0".to_string(),
        };
        let unloaded = DomainEvent::PluginUnloaded {
            name: "my-plugin".to_string(),
        };
        assert_ne!(loaded, unloaded);
    }

    #[test]
    fn test_download_progress_event() {
        let event = DomainEvent::DownloadProgress {
            id: DownloadId(7),
            downloaded_bytes: 512,
            total_bytes: 1024,
        };
        let s = format!("{event:?}");
        assert!(s.contains("DownloadProgress"));
        assert!(s.contains("512"));
        assert!(s.contains("1024"));
    }

    #[test]
    fn test_package_created_event() {
        let event = DomainEvent::PackageCreated {
            id: 99,
            name: "My Package".to_string(),
        };
        assert_eq!(
            event,
            DomainEvent::PackageCreated {
                id: 99,
                name: "My Package".to_string()
            }
        );
    }

    #[test]
    fn test_clipboard_url_detected_event() {
        let event = DomainEvent::ClipboardUrlDetected {
            urls: vec!["https://example.com/file.zip".to_string()],
        };
        let s = format!("{event:?}");
        assert!(s.contains("ClipboardUrlDetected"));
        assert!(s.contains("example.com"));
    }

    #[test]
    fn test_clipboard_url_detected_clone() {
        let event = DomainEvent::ClipboardUrlDetected {
            urls: vec!["https://a.com".into(), "https://b.com".into()],
        };
        let cloned = event.clone();
        assert_eq!(event, cloned);
    }

    #[test]
    fn test_checksum_verified_event_carries_algorithm_and_value() {
        let event = DomainEvent::ChecksumVerified {
            id: DownloadId(11),
            algorithm: "SHA-256".to_string(),
            checksum: "deadbeef".to_string(),
        };
        let s = format!("{event:?}");
        assert!(s.contains("ChecksumVerified"));
        assert!(s.contains("SHA-256"));
        assert!(s.contains("deadbeef"));
    }

    #[test]
    fn test_checksum_mismatch_event_includes_expected_and_computed() {
        let event = DomainEvent::ChecksumMismatch {
            id: DownloadId(12),
            algorithm: "MD5".to_string(),
            expected: "aaa".to_string(),
            computed: "bbb".to_string(),
        };
        let s = format!("{event:?}");
        assert!(s.contains("ChecksumMismatch"));
        assert!(s.contains("aaa"));
        assert!(s.contains("bbb"));
    }
}
