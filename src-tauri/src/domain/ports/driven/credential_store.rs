//! Port for secure credential storage.
//!
//! Abstracts the system keychain (keyring) for storing account
//! credentials used by plugins and download services.

use crate::domain::error::DomainError;
use crate::domain::model::credential::Credential;

/// Stores and retrieves service credentials securely.
///
/// The adapter implementation uses the OS keychain (via `keyring-rs`)
/// so that passwords and tokens are never stored in the SQLite database.
pub trait CredentialStore: Send + Sync {
    /// Retrieve the credential for a named service.
    fn get(&self, service: &str) -> Result<Option<Credential>, DomainError>;

    /// Store (or overwrite) a credential for a named service.
    fn store(&self, service: &str, credential: &Credential) -> Result<(), DomainError>;

    /// Delete the credential for a named service.
    fn delete(&self, service: &str) -> Result<(), DomainError>;
}
