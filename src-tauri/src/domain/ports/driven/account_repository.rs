//! Write repository for the `Account` aggregate (CQRS write side).
//!
//! Persists account metadata only. Credentials (passwords / tokens) live
//! in the OS keyring and are looked up via `Account::credential_ref()` —
//! never through this port.

use crate::domain::error::DomainError;
use crate::domain::model::account::{Account, AccountId};

/// Persists and retrieves `Account` aggregates.
pub trait AccountRepository: Send + Sync {
    /// Find an account by its unique identifier.
    fn find_by_id(&self, id: &AccountId) -> Result<Option<Account>, DomainError>;

    /// Persist an account (insert or update).
    ///
    /// Returns `DomainError::AlreadyExists` when the `(service_name, username)`
    /// pair already maps to a different `id` (UNIQUE constraint).
    fn save(&self, account: &Account) -> Result<(), DomainError>;

    /// List every persisted account, ordered by `created_at` ascending.
    fn list(&self) -> Result<Vec<Account>, DomainError>;

    /// List accounts for a single service (e.g. `"real-debrid"`),
    /// ordered by `created_at` ascending.
    fn list_by_service(&self, service_name: &str) -> Result<Vec<Account>, DomainError>;

    /// Delete an account by its identifier. No-op when the account does
    /// not exist.
    fn delete(&self, id: &AccountId) -> Result<(), DomainError>;
}
