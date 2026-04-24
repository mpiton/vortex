use crate::domain::model::download::DownloadId;

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
    DownloadCompletedPersisted {
        id: DownloadId,
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

    // Settings
    SettingsUpdated,
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
