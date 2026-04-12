//! No-op credential store stub.
//!
//! A placeholder implementation that returns `Ok(None)` for reads
//! and no-ops for writes/deletes. Used during development until
//! the keyring-rs integration is fully implemented.

use crate::domain::error::DomainError;
use crate::domain::model::credential::Credential;
use crate::domain::ports::driven::credential_store::CredentialStore;

/// Stub credential store that performs no operations.
///
/// Credentials are not persisted. This adapter is intended as a temporary
/// placeholder until the full keyring-rs backend is integrated.
#[derive(Debug, Clone)]
pub struct NoopCredentialStore;

impl CredentialStore for NoopCredentialStore {
    fn get(&self, service: &str) -> Result<Option<Credential>, DomainError> {
        tracing::warn!(
            service,
            "credential requested but keyring store is not yet implemented"
        );
        Ok(None)
    }

    fn store(&self, service: &str, _credential: &Credential) -> Result<(), DomainError> {
        tracing::warn!(
            service,
            "credential write ignored: keyring store is not yet implemented"
        );
        Ok(())
    }

    fn delete(&self, service: &str) -> Result<(), DomainError> {
        tracing::warn!(
            service,
            "credential delete ignored: keyring store is not yet implemented"
        );
        Ok(())
    }
}
