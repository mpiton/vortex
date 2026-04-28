//! Per-account credential storage.
//!
//! Keys credentials by [`AccountId`] so each persisted `Account` row
//! has exactly one matching keyring entry, even when the same
//! `(service_name, username)` pair appears under multiple ids
//! (e.g. legacy migrations or duplicate-detection tests).
//!
//! The lower-level [`CredentialStore`](super::CredentialStore) port is
//! keyed by service name and re-used by other call sites; this port
//! exists so the account-management commands never need to construct
//! ad-hoc keyring service strings.

use crate::domain::error::DomainError;
use crate::domain::model::account::AccountId;

pub trait AccountCredentialStore: Send + Sync {
    /// Persist `password` under `account_id`. Overwrites any existing
    /// value for the same id.
    fn store_password(&self, account_id: &AccountId, password: &str) -> Result<(), DomainError>;

    /// Retrieve the password previously stored under `account_id`, or
    /// `None` when nothing has been saved.
    fn get_password(&self, account_id: &AccountId) -> Result<Option<String>, DomainError>;

    /// Delete the password stored under `account_id`. No-op when no
    /// entry exists.
    fn delete_password(&self, account_id: &AccountId) -> Result<(), DomainError>;
}
