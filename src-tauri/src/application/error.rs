//! Application-layer error type.
//!
//! Wraps `DomainError` and adds infrastructure-specific variants
//! for storage, network, plugin, config, not-found, and validation errors.

use crate::domain::DomainError;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Domain(#[from] DomainError),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("network error: {0}")]
    Network(String),

    #[error("plugin error: {0}")]
    Plugin(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("validation error: {0}")]
    Validation(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn returns_app_error_via_question_mark() -> Result<(), AppError> {
        let result: Result<(), DomainError> = Err(DomainError::NotFound("test-id".to_string()));
        result?;
        Ok(())
    }

    #[test]
    fn test_app_error_from_domain_error_converts_with_from() {
        let domain_err = DomainError::NotFound("download-1".to_string());
        let app_err = AppError::from(domain_err);
        assert!(matches!(
            app_err,
            AppError::Domain(DomainError::NotFound(_))
        ));
    }

    #[test]
    fn test_app_error_from_domain_error_question_mark_propagates() {
        let result = returns_app_error_via_question_mark();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AppError::Domain(DomainError::NotFound(_))
        ));
    }

    #[test]
    fn test_app_error_display_storage_shows_message() {
        let err = AppError::Storage("disk full".to_string());
        assert_eq!(err.to_string(), "storage error: disk full");
    }

    #[test]
    fn test_app_error_display_network_shows_message() {
        let err = AppError::Network("connection refused".to_string());
        assert_eq!(err.to_string(), "network error: connection refused");
    }

    #[test]
    fn test_app_error_display_plugin_shows_message() {
        let err = AppError::Plugin("wasm panic".to_string());
        assert_eq!(err.to_string(), "plugin error: wasm panic");
    }

    #[test]
    fn test_app_error_display_config_shows_message() {
        let err = AppError::Config("missing field".to_string());
        assert_eq!(err.to_string(), "config error: missing field");
    }

    #[test]
    fn test_app_error_display_not_found_shows_message() {
        let err = AppError::NotFound("plugin-xyz".to_string());
        assert_eq!(err.to_string(), "not found: plugin-xyz");
    }

    #[test]
    fn test_app_error_display_validation_shows_message() {
        let err = AppError::Validation("url is required".to_string());
        assert_eq!(err.to_string(), "validation error: url is required");
    }

    #[test]
    fn test_app_error_display_domain_forwards_domain_error_display() {
        let domain_err = DomainError::NotFound("download-99".to_string());
        let app_err = AppError::Domain(domain_err);
        assert_eq!(app_err.to_string(), "Not found: download-99");
    }

    #[test]
    fn test_app_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AppError>();
    }
}
