use crate::domain::model::download::DownloadState;
use crate::domain::model::segment::SegmentState;

#[derive(Debug, Clone, PartialEq)]
pub enum DomainError {
    InvalidTransition {
        from: DownloadState,
        to: DownloadState,
    },
    InvalidSegmentTransition {
        from: SegmentState,
        to: SegmentState,
    },
    MaxRetriesExceeded {
        download_id: u64,
    },
    InvalidUrl(String),
    InvalidPriority(String),
    NotFound(String),
    AlreadyExists(String),
    StorageError(String),
    NetworkError(String),
    ValidationError(String),
    PluginError(String),
    AdaptiveStreamOnly,
}

impl std::fmt::Display for DomainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DomainError::InvalidTransition { from, to } => {
                write!(f, "Invalid state transition from '{from}' to '{to}'")
            }
            DomainError::InvalidSegmentTransition { from, to } => {
                write!(f, "Invalid segment transition from '{from}' to '{to}'")
            }
            DomainError::MaxRetriesExceeded { download_id } => {
                write!(f, "Max retries exceeded for download {download_id}")
            }
            DomainError::InvalidUrl(url) => {
                write!(f, "Invalid URL: {url}")
            }
            DomainError::InvalidPriority(msg) => {
                write!(f, "Invalid priority: {msg}")
            }
            DomainError::NotFound(id) => {
                write!(f, "Not found: {id}")
            }
            DomainError::AlreadyExists(id) => {
                write!(f, "Already exists: {id}")
            }
            DomainError::StorageError(msg) => {
                write!(f, "Storage error: {msg}")
            }
            DomainError::NetworkError(msg) => {
                write!(f, "Network error: {msg}")
            }
            DomainError::ValidationError(msg) => {
                write!(f, "Validation error: {msg}")
            }
            DomainError::PluginError(msg) => {
                write!(f, "Plugin error: {msg}")
            }
            DomainError::AdaptiveStreamOnly => write!(
                f,
                "Video is only available as adaptive stream (DASH/HLS); use download_to_file"
            ),
        }
    }
}

impl std::error::Error for DomainError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_invalid_transition() {
        let err = DomainError::InvalidTransition {
            from: DownloadState::Queued,
            to: DownloadState::Completed,
        };
        assert_eq!(
            err.to_string(),
            "Invalid state transition from 'Queued' to 'Completed'"
        );
    }

    #[test]
    fn test_display_max_retries_exceeded() {
        let err = DomainError::MaxRetriesExceeded { download_id: 42 };
        assert_eq!(err.to_string(), "Max retries exceeded for download 42");
    }

    #[test]
    fn test_display_invalid_url() {
        let err = DomainError::InvalidUrl("bad://url".to_string());
        assert_eq!(err.to_string(), "Invalid URL: bad://url");
    }

    #[test]
    fn test_display_invalid_priority() {
        let err = DomainError::InvalidPriority("must be 1-10".to_string());
        assert_eq!(err.to_string(), "Invalid priority: must be 1-10");
    }

    #[test]
    fn test_display_not_found() {
        let err = DomainError::NotFound("download-99".to_string());
        assert_eq!(err.to_string(), "Not found: download-99");
    }

    #[test]
    fn test_display_already_exists() {
        let err = DomainError::AlreadyExists("download-1".to_string());
        assert_eq!(err.to_string(), "Already exists: download-1");
    }

    #[test]
    fn test_display_storage_error() {
        let err = DomainError::StorageError("disk full".to_string());
        assert_eq!(err.to_string(), "Storage error: disk full");
    }

    #[test]
    fn test_display_validation_error() {
        let err = DomainError::ValidationError("file name cannot be empty".to_string());
        assert_eq!(
            err.to_string(),
            "Validation error: file name cannot be empty"
        );
    }

    #[test]
    fn test_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<DomainError>();
    }

    #[test]
    fn test_display_plugin_error() {
        let err = DomainError::PluginError("wasm load failed".to_string());
        assert_eq!(err.to_string(), "Plugin error: wasm load failed");
    }

    #[test]
    fn test_domain_error_implements_error_trait() {
        let err: Box<dyn std::error::Error> = Box::new(DomainError::NotFound("x".to_string()));
        assert!(err.to_string().contains("Not found"));
    }

    #[test]
    fn test_display_adaptive_stream_only() {
        let err = DomainError::AdaptiveStreamOnly;
        assert_eq!(
            err.to_string(),
            "Video is only available as adaptive stream (DASH/HLS); use download_to_file"
        );
    }
}
