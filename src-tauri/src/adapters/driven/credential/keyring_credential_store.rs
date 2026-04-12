//! Credential store backed by the OS keychain via `keyring-rs`.
//!
//! Stores credentials in the platform-native secret store:
//! macOS Keychain, Linux Secret Service / keyutils, Windows Credential Manager.
//! Each credential is stored as two keyring entries (username + password)
//! under the service name `vortex-{service}`.

use crate::domain::error::DomainError;
use crate::domain::model::credential::Credential;
use crate::domain::ports::driven::credential_store::CredentialStore;

/// Credential store backed by the OS keychain.
///
/// Uses [`keyring::Entry`] to persist credentials securely.
/// Two entries are created per service — one for the username, one for the
/// password — so that a full [`Credential`] can be reconstructed on retrieval
/// without any serialization format dependency.
///
/// # Concurrency
///
/// The two reads in [`get`](CredentialStore::get) (username then password) are
/// not atomic at the keychain level. A concurrent [`store`](CredentialStore::store)
/// or [`delete`](CredentialStore::delete) between the two reads can produce a
/// mismatched `Credential`. Callers that require strong consistency must
/// serialize access externally (e.g. wrap in a `Mutex`).
#[derive(Debug, Clone)]
pub struct KeyringCredentialStore;

impl KeyringCredentialStore {
    fn username_entry(service: &str) -> Result<keyring::Entry, DomainError> {
        let svc = format!("vortex-{service}");
        keyring::Entry::new(&svc, "vortex-username")
            .map_err(|e| DomainError::StorageError(sanitize_keyring_error(service, "entry", &e)))
    }

    fn password_entry(service: &str) -> Result<keyring::Entry, DomainError> {
        let svc = format!("vortex-{service}");
        keyring::Entry::new(&svc, "vortex-password")
            .map_err(|e| DomainError::StorageError(sanitize_keyring_error(service, "entry", &e)))
    }
}

/// Map a keyring error to a safe, opaque message that never leaks stored secrets.
///
/// `keyring::Error::Ambiguous` wraps platform `Credential` objects whose `Debug`
/// impl can print raw secret values. `BadEncoding` contains a raw `Vec<u8>` dump.
/// Neither should ever appear in a `DomainError` that can propagate to logs or UI.
fn sanitize_keyring_error(service: &str, operation: &str, err: &keyring::Error) -> String {
    match err {
        keyring::Error::Ambiguous(_) => {
            format!(
                "keyring {operation} error for service '{service}': ambiguous (multiple entries matched)"
            )
        }
        keyring::Error::BadEncoding(_) => {
            format!(
                "keyring {operation} error for service '{service}': stored value is not valid UTF-8"
            )
        }
        other => format!("keyring {operation} error for service '{service}': {other}"),
    }
}

impl CredentialStore for KeyringCredentialStore {
    fn get(&self, service: &str) -> Result<Option<Credential>, DomainError> {
        let user_entry = Self::username_entry(service)?;
        let pass_entry = Self::password_entry(service)?;

        let username = match user_entry.get_password() {
            Ok(val) => val,
            Err(keyring::Error::NoEntry) => return Ok(None),
            Err(e) => {
                return Err(DomainError::StorageError(sanitize_keyring_error(
                    service, "read", &e,
                )));
            }
        };

        let password = match pass_entry.get_password() {
            Ok(val) => val,
            Err(keyring::Error::NoEntry) => return Ok(None),
            Err(e) => {
                return Err(DomainError::StorageError(sanitize_keyring_error(
                    service, "read", &e,
                )));
            }
        };

        Ok(Some(Credential::new(username, password)))
    }

    fn store(&self, service: &str, credential: &Credential) -> Result<(), DomainError> {
        let user_entry = Self::username_entry(service)?;
        let pass_entry = Self::password_entry(service)?;

        user_entry
            .set_password(credential.username())
            .map_err(|e| DomainError::StorageError(sanitize_keyring_error(service, "write", &e)))?;

        pass_entry
            .set_password(credential.password())
            .map_err(|e| {
                if let Err(cleanup_err) = user_entry.delete_credential() {
                    tracing::warn!(
                        service = service,
                        error = %cleanup_err,
                        "password write failed and username cleanup also failed — \
                         orphaned username entry may remain in keychain"
                    );
                }
                DomainError::StorageError(sanitize_keyring_error(service, "write", &e))
            })?;

        Ok(())
    }

    fn delete(&self, service: &str) -> Result<(), DomainError> {
        let user_entry = Self::username_entry(service)?;
        let pass_entry = Self::password_entry(service)?;

        // Delete both entries; ignore NoEntry (already absent is fine).
        if let Err(e) = user_entry.delete_credential()
            && !matches!(e, keyring::Error::NoEntry)
        {
            return Err(DomainError::StorageError(sanitize_keyring_error(
                service, "delete", &e,
            )));
        }

        if let Err(e) = pass_entry.delete_credential()
            && !matches!(e, keyring::Error::NoEntry)
        {
            return Err(DomainError::StorageError(sanitize_keyring_error(
                service, "delete", &e,
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests require a running OS keychain backend and are skipped in CI.
    // Run locally with: cargo test -- --ignored keyring

    #[test]
    #[ignore = "requires OS keychain backend"]
    fn test_store_get_delete_cycle() {
        let store = KeyringCredentialStore;
        let service = "test-integration-cycle";
        let cred = Credential::new("alice", "s3cret");

        // Clean slate
        let _ = store.delete(service);

        // Store
        store.store(service, &cred).expect("store credential");

        // Get
        let retrieved = store.get(service).expect("get credential");
        let retrieved = retrieved.expect("credential should exist");
        assert_eq!(retrieved.username(), "alice");
        assert_eq!(retrieved.password(), "s3cret");

        // Delete
        store.delete(service).expect("delete credential");

        // Verify gone
        let after_delete = store.get(service).expect("get after delete");
        assert!(after_delete.is_none());
    }

    #[test]
    #[ignore = "requires OS keychain backend"]
    fn test_get_returns_none_when_absent() {
        let store = KeyringCredentialStore;
        let result = store
            .get("test-nonexistent-service")
            .expect("get nonexistent");
        assert!(result.is_none());
    }

    #[test]
    #[ignore = "requires OS keychain backend"]
    fn test_delete_absent_is_ok() {
        let store = KeyringCredentialStore;
        store
            .delete("test-nonexistent-delete")
            .expect("delete nonexistent should succeed");
    }

    #[test]
    #[ignore = "requires OS keychain backend"]
    fn test_store_overwrites_existing() {
        let store = KeyringCredentialStore;
        let service = "test-overwrite";

        let _ = store.delete(service);

        let cred1 = Credential::new("bob", "pass1");
        store.store(service, &cred1).expect("store first");

        let cred2 = Credential::new("charlie", "pass2");
        store.store(service, &cred2).expect("store second");

        let retrieved = store.get(service).expect("get").expect("should exist");
        assert_eq!(retrieved.username(), "charlie");
        assert_eq!(retrieved.password(), "pass2");

        let _ = store.delete(service);
    }
}
