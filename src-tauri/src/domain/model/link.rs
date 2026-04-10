//! Link status types returned by URL checking.

/// Result of checking a URL's availability and metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkStatus {
    /// URL is reachable and returns a successful response.
    Online {
        filename: Option<String>,
        size: Option<u64>,
        resumable: bool,
    },
    /// URL returned 404 or similar — resource not found.
    Offline,
    /// Status could not be determined (server error, timeout, etc.).
    Unknown,
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
}
