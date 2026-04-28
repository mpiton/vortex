//! [`AccountCredentialStore`] backed by `keyring-rs`.
//!
//! Stores one keyring entry per persisted [`AccountId`]. The keyring
//! service name is `vortex-account-{id}`; the username slot is the
//! constant marker `vortex-account-password`. A single store call
//! therefore writes a single secret — no race window between two
//! related entries (contrast with the broader
//! [`KeyringCredentialStore`] which juggles `username` and `password`
//! sub-entries).

use crate::domain::error::DomainError;
use crate::domain::model::account::AccountId;
use crate::domain::ports::driven::AccountCredentialStore;

const KEYRING_USERNAME_SLOT: &str = "vortex-account-password";

#[derive(Debug, Clone, Default)]
pub struct KeyringAccountStore;

impl KeyringAccountStore {
    pub fn new() -> Self {
        Self
    }

    fn entry(account_id: &AccountId) -> Result<keyring::Entry, DomainError> {
        let svc = format!("vortex-account-{}", account_id.as_str());
        keyring::Entry::new(&svc, KEYRING_USERNAME_SLOT)
            .map_err(|e| DomainError::StorageError(sanitize(account_id.as_str(), "entry", &e)))
    }
}

impl AccountCredentialStore for KeyringAccountStore {
    fn store_password(&self, account_id: &AccountId, password: &str) -> Result<(), DomainError> {
        let entry = Self::entry(account_id)?;
        entry
            .set_password(password)
            .map_err(|e| DomainError::StorageError(sanitize(account_id.as_str(), "write", &e)))
    }

    fn get_password(&self, account_id: &AccountId) -> Result<Option<String>, DomainError> {
        let entry = Self::entry(account_id)?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(DomainError::StorageError(sanitize(
                account_id.as_str(),
                "read",
                &e,
            ))),
        }
    }

    fn delete_password(&self, account_id: &AccountId) -> Result<(), DomainError> {
        let entry = Self::entry(account_id)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(DomainError::StorageError(sanitize(
                account_id.as_str(),
                "delete",
                &e,
            ))),
        }
    }
}

/// Map a keyring error to a sanitised string. Mirrors the policy used
/// by [`KeyringCredentialStore`](super::KeyringCredentialStore): keyring's
/// `Ambiguous` variant wraps `Credential` `Debug` impls that can leak
/// raw secrets, and `BadEncoding` wraps the raw byte buffer; neither
/// should ever propagate unfiltered.
fn sanitize(account_id: &str, operation: &str, err: &keyring::Error) -> String {
    match err {
        keyring::Error::Ambiguous(_) => format!(
            "keyring {operation} error for account '{account_id}': ambiguous (multiple entries matched)"
        ),
        keyring::Error::BadEncoding(_) => format!(
            "keyring {operation} error for account '{account_id}': stored value is not valid UTF-8"
        ),
        other => format!("keyring {operation} error for account '{account_id}': {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_ambiguous_omits_inner_credentials() {
        let err = keyring::Error::Ambiguous(Vec::new());
        let msg = sanitize("acc-1", "read", &err);
        assert!(msg.contains("ambiguous"));
        assert!(msg.contains("acc-1"));
        assert!(!msg.contains("Credential"));
    }

    #[test]
    fn test_sanitize_bad_encoding_omits_raw_bytes() {
        let err = keyring::Error::BadEncoding(vec![0xFF, 0xFE]);
        let msg = sanitize("acc-2", "read", &err);
        assert!(msg.contains("not valid UTF-8"));
        assert!(!msg.contains("0xFF"));
    }

    #[test]
    fn test_sanitize_no_entry_includes_id_and_operation() {
        let err = keyring::Error::NoEntry;
        let msg = sanitize("acc-3", "delete", &err);
        assert!(msg.contains("acc-3"));
        assert!(msg.contains("delete"));
    }

    // The end-to-end keyring round-trip test exercises a real OS
    // keychain so it is gated behind `--ignored`. CI relies on the
    // FakeAccountCredentialStore in `tests_support` to cover the
    // command handlers.

    #[test]
    #[ignore = "requires OS keychain backend"]
    fn test_store_get_delete_cycle_roundtrips() {
        let store = KeyringAccountStore::new();
        let id = AccountId::new("kc-test-id");

        let _ = store.delete_password(&id);

        store.store_password(&id, "s3cret").expect("store");
        assert_eq!(
            store.get_password(&id).expect("get").as_deref(),
            Some("s3cret")
        );

        store.store_password(&id, "rotated").expect("rotate");
        assert_eq!(
            store.get_password(&id).expect("get").as_deref(),
            Some("rotated")
        );

        store.delete_password(&id).expect("delete");
        assert!(store.get_password(&id).expect("get").is_none());
    }
}
