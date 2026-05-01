//! Link status types returned by URL checking.

/// Result of checking a URL's availability and metadata.
///
/// PRD §6.2.2 pipeline: `Online | Offline | PremiumOnly | Checking | Unknown`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkStatus {
    /// Probe in flight; emitted as initial state for a URL the
    /// link grabber pipeline just started checking. Replaced by a
    /// terminal variant as soon as the probe resolves.
    Checking,
    /// URL is reachable and returns a successful response.
    Online {
        filename: Option<String>,
        size: Option<u64>,
        resumable: bool,
    },
    /// URL exists but the host requires a premium / authenticated
    /// account before the file can be downloaded (HTTP 401 / 402,
    /// or plugin-reported "Premium-only").
    PremiumOnly,
    /// URL returned 404 or similar — resource not found.
    Offline,
    /// Status could not be determined (server error, timeout, etc.).
    Unknown,
}

impl LinkStatus {
    /// `true` for the terminal variants — those that should stop a
    /// retry / refresh loop. `Checking` is transient and never terminal.
    pub fn is_terminal(&self) -> bool {
        !matches!(self, LinkStatus::Checking)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_link_status_online_with_all_fields() {
        let status = LinkStatus::Online {
            filename: Some("file.zip".to_string()),
            size: Some(1024),
            resumable: true,
        };
        assert_eq!(
            status,
            LinkStatus::Online {
                filename: Some("file.zip".to_string()),
                size: Some(1024),
                resumable: true,
            }
        );
    }

    #[test]
    fn test_link_status_online_minimal() {
        let status = LinkStatus::Online {
            filename: None,
            size: None,
            resumable: false,
        };
        assert_eq!(
            status,
            LinkStatus::Online {
                filename: None,
                size: None,
                resumable: false,
            }
        );
    }

    #[test]
    fn test_link_status_offline() {
        let status = LinkStatus::Offline;
        assert_eq!(status, LinkStatus::Offline);
    }

    #[test]
    fn test_link_status_unknown() {
        let status = LinkStatus::Unknown;
        assert_eq!(status, LinkStatus::Unknown);
    }

    #[test]
    fn test_link_status_clone_and_eq() {
        let original = LinkStatus::Online {
            filename: Some("archive.tar.gz".to_string()),
            size: Some(2048),
            resumable: true,
        };
        let cloned = original.clone();
        assert_eq!(original, cloned);

        let offline = LinkStatus::Offline;
        let offline_clone = offline.clone();
        assert_eq!(offline, offline_clone);

        assert_ne!(original, LinkStatus::Offline);
        assert_ne!(original, LinkStatus::Unknown);
        assert_ne!(offline, LinkStatus::Unknown);
    }

    #[test]
    fn test_link_status_premium_only_distinct_variant() {
        let premium = LinkStatus::PremiumOnly;
        assert_eq!(premium, LinkStatus::PremiumOnly);
        assert_ne!(premium, LinkStatus::Offline);
        assert_ne!(premium, LinkStatus::Unknown);
    }

    #[test]
    fn test_link_status_checking_distinct_variant() {
        let checking = LinkStatus::Checking;
        assert_eq!(checking, LinkStatus::Checking);
        assert_ne!(checking, LinkStatus::Offline);
        assert_ne!(checking, LinkStatus::PremiumOnly);
    }

    #[test]
    fn test_link_status_is_terminal_returns_false_only_for_checking() {
        assert!(!LinkStatus::Checking.is_terminal());
        assert!(LinkStatus::Offline.is_terminal());
        assert!(LinkStatus::PremiumOnly.is_terminal());
        assert!(LinkStatus::Unknown.is_terminal());
        assert!(
            LinkStatus::Online {
                filename: None,
                size: None,
                resumable: false,
            }
            .is_terminal()
        );
    }
}
