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
    DownloadExtracting {
        id: DownloadId,
    },
    DownloadProgress {
        id: DownloadId,
        downloaded_bytes: u64,
        total_bytes: u64,
    },

    // Segments
    SegmentStarted {
        download_id: DownloadId,
        segment_id: u32,
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
}
