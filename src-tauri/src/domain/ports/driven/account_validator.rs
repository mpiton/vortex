//! Validates account credentials against a remote service.
//!
//! Implementations delegate to the hoster / debrid plugin matching the
//! account's `service_name`. When no plugin is registered for the
//! service, the implementation MUST return
//! [`DomainError::NotFound`] with a message that names the service so
//! the calling handler can surface a clear "no plugin for service X"
//! error to the user.

use crate::domain::error::DomainError;

/// Result of an account validation attempt.
///
/// Carries the data that the "test connection" UI surface needs — even
/// when the credentials are rejected, an `error_message` lets the panel
/// explain *why* (wrong password, expired, rate-limited, ...).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidationOutcome {
    pub valid: bool,
    pub latency_ms: Option<u64>,
    pub traffic_left: Option<u64>,
    pub traffic_total: Option<u64>,
    pub valid_until: Option<u64>,
    pub error_message: Option<String>,
}

impl ValidationOutcome {
    pub fn ok() -> Self {
        Self {
            valid: true,
            ..Self::default()
        }
    }

    pub fn rejected(error_message: impl Into<String>) -> Self {
        Self {
            valid: false,
            error_message: Some(error_message.into()),
            ..Self::default()
        }
    }
}

/// Validates an account's credentials by attempting to connect to the
/// remote service it represents.
pub trait AccountValidator: Send + Sync {
    /// Probe the remote service named `service_name` with the given
    /// credentials and return the resulting [`ValidationOutcome`].
    ///
    /// Returns [`DomainError::NotFound`] when no plugin is registered
    /// for `service_name`.
    fn validate(
        &self,
        service_name: &str,
        username: &str,
        password: &str,
    ) -> Result<ValidationOutcome, DomainError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_outcome_ok_marks_valid_with_no_error() {
        let out = ValidationOutcome::ok();
        assert!(out.valid);
        assert!(out.error_message.is_none());
        assert!(out.latency_ms.is_none());
        assert!(out.traffic_left.is_none());
    }

    #[test]
    fn test_validation_outcome_rejected_records_message_and_invalid_flag() {
        let out = ValidationOutcome::rejected("wrong password");
        assert!(!out.valid);
        assert_eq!(out.error_message.as_deref(), Some("wrong password"));
    }

    #[test]
    fn test_validation_outcome_default_is_invalid_and_empty() {
        let out = ValidationOutcome::default();
        assert!(!out.valid);
        assert!(out.latency_ms.is_none());
        assert!(out.error_message.is_none());
    }
}
