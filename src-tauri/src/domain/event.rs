use crate::domain::model::account::AccountId;
use crate::domain::model::download::DownloadId;
use crate::domain::model::link::LinkStatus;
use crate::domain::model::package::PackageId;
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
    /// Emitted by the wait manager when a hoster requests a delay before
    /// the download can resume. Carries the absolute deadline (Unix epoch
    /// milliseconds) so the frontend can render a live countdown without
    /// trusting client clock drift, plus the original `total_seconds` and
    /// human-readable `reason` (e.g. `"hoster cooldown"`, `"captcha
    /// solved, awaiting download slot"`).
    DownloadWaitingStarted {
        id: DownloadId,
        until_unix_ms: u64,
        total_seconds: u32,
        reason: String,
    },
    /// Emitted by the wait manager when a wait ticket is consumed —
    /// either because the timer expired naturally (`expired_naturally =
    /// true`) or because the user skipped / cancelled the download
    /// (`expired_naturally = false`). Subscribers cleaning up timers can
    /// listen on this event regardless of the cause.
    DownloadWaitingEnded {
        id: DownloadId,
        expired_naturally: bool,
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
    /// Emitted by the download engine when a Metalink mirror fails and
    /// the engine falls back to the next entry in the priority-sorted
    /// list. Carries the new mirror URL (resolved at switch time, not
    /// the canonical download URL) and the index inside
    /// [`Download::mirrors`] so the detail panel can highlight the
    /// active source.
    MirrorSwitched {
        id: DownloadId,
        new_mirror_index: u32,
        new_url: String,
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
        id: PackageId,
        name: String,
    },
    /// Emitted whenever a package's metadata (name / folder / priority /
    /// auto-extract / password / membership) changes. Fine-grained per-
    /// child events (e.g. `DownloadPrioritySet`, `DownloadDirectoryChanged`)
    /// are still emitted alongside this carrier so the queue manager
    /// re-schedules normally.
    PackageUpdated {
        id: PackageId,
    },
    /// Emitted after a `DeletePackageCommand` removed the package row.
    /// `delete_downloads` mirrors the command flag so subscribers can
    /// distinguish "package detached, downloads kept" from "everything
    /// gone" without re-reading the repo.
    PackageDeleted {
        id: PackageId,
        delete_downloads: bool,
    },
    /// Emitted by the split-archive grouper when the resolved link set
    /// for a base name is missing one or more numbered parts. The UI
    /// surfaces a notification so the user can fetch the gap before the
    /// extraction step blocks. `missing_parts` lists the human-readable
    /// suffixes (e.g. `"part05.rar"`, `"7z.003"`) of the gaps detected.
    SplitArchiveIncomplete {
        package_id: PackageId,
        base_name: String,
        missing_parts: Vec<String>,
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
    /// Emitted by `AccountSelector::select_best` when no enabled,
    /// non-expired account exists for the requested service. The
    /// scheduler / link-grabber can react by falling back to a free
    /// hoster path or surfacing a UI hint.
    NoAccountAvailable {
        service_name: String,
    },
    /// Emitted by `AccountSelector::select_best` whenever it picks an
    /// account. Carries the strategy name so the audit / telemetry
    /// layer can detect if a deployment is using anything other than
    /// the default `BestTraffic`.
    AccountSelected {
        id: AccountId,
        service_name: String,
        /// One of `"best_traffic"`, `"round_robin"`, `"manual"`.
        strategy: String,
    },
    /// Emitted by `AccountRotator::mark_exhausted` when a hoster signals
    /// quota exhaustion (HTTP 429, low `traffic_left`, …) so the account
    /// is taken out of the rotation until the cooldown expires or the
    /// next traffic refresh confirms availability. Carries `service_name`
    /// so the UI can group notifications per hoster.
    AccountExhausted {
        id: AccountId,
        service_name: String,
        /// Unix epoch milliseconds — deadline after which the rotator
        /// will consider the account eligible again.
        exhausted_until_ms: u64,
    },

    // Link Grabber
    /// Emitted by the `link_check_online` handler whenever a single URL
    /// transitions to a new [`LinkStatus`]. The Tauri bridge forwards
    /// each event so the Link Grabber view can render per-row badges
    /// progressively rather than waiting for the whole batch.
    LinkStatusUpdated {
        url: String,
        status: LinkStatus,
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
            id: PackageId::new("pkg-99"),
            name: "My Package".to_string(),
        };
        assert_eq!(
            event,
            DomainEvent::PackageCreated {
                id: PackageId::new("pkg-99"),
                name: "My Package".to_string()
            }
        );
    }

    #[test]
    fn test_package_updated_event_carries_id() {
        let event = DomainEvent::PackageUpdated {
            id: PackageId::new("pkg-up"),
        };
        let s = format!("{event:?}");
        assert!(s.contains("PackageUpdated"));
        assert!(s.contains("pkg-up"));
    }

    #[test]
    fn test_package_deleted_event_carries_id_and_cascade_flag() {
        let cascade = DomainEvent::PackageDeleted {
            id: PackageId::new("pkg-del"),
            delete_downloads: true,
        };
        let detach = DomainEvent::PackageDeleted {
            id: PackageId::new("pkg-del"),
            delete_downloads: false,
        };
        assert_ne!(cascade, detach);
        let s = format!("{cascade:?}");
        assert!(s.contains("PackageDeleted"));
        assert!(s.contains("pkg-del"));
    }

    #[test]
    fn test_split_archive_incomplete_event_carries_missing_parts() {
        let event = DomainEvent::SplitArchiveIncomplete {
            package_id: PackageId::new("pkg-split"),
            base_name: "movie".to_string(),
            missing_parts: vec!["part05.rar".to_string(), "part07.rar".to_string()],
        };
        let s = format!("{event:?}");
        assert!(s.contains("SplitArchiveIncomplete"));
        assert!(s.contains("pkg-split"));
        assert!(s.contains("movie"));
        assert!(s.contains("part05.rar"));

        let cloned = event.clone();
        assert_eq!(event, cloned);
    }

    #[test]
    fn test_download_waiting_started_carries_deadline_and_reason() {
        let event = DomainEvent::DownloadWaitingStarted {
            id: DownloadId(11),
            until_unix_ms: 1_700_000_000_000,
            total_seconds: 60,
            reason: "hoster cooldown".to_string(),
        };
        let s = format!("{event:?}");
        assert!(s.contains("DownloadWaitingStarted"));
        assert!(s.contains("11"));
        assert!(s.contains("1700000000000"));
        assert!(s.contains("hoster cooldown"));
        let cloned = event.clone();
        assert_eq!(event, cloned);
    }

    #[test]
    fn test_download_waiting_ended_distinguishes_expiry_from_cancel() {
        let expired = DomainEvent::DownloadWaitingEnded {
            id: DownloadId(12),
            expired_naturally: true,
        };
        let cancelled = DomainEvent::DownloadWaitingEnded {
            id: DownloadId(12),
            expired_naturally: false,
        };
        assert_ne!(expired, cancelled);
        let s = format!("{expired:?}");
        assert!(s.contains("DownloadWaitingEnded"));
        assert!(s.contains("expired_naturally"));
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
    fn test_mirror_switched_event_carries_index_and_url() {
        let event = DomainEvent::MirrorSwitched {
            id: DownloadId(7),
            new_mirror_index: 1,
            new_url: "https://mirror2.example.com/file.zip".to_string(),
        };
        let s = format!("{event:?}");
        assert!(s.contains("MirrorSwitched"));
        assert!(s.contains("mirror2.example.com"));
        let cloned = event.clone();
        assert_eq!(event, cloned);
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
