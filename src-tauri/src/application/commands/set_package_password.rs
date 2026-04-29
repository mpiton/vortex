//! Handler for [`SetPackagePasswordCommand`](super::SetPackagePasswordCommand).
//!
//! The plaintext password is persisted in the OS keyring under the
//! convention `vortex.package.<id>` via [`CredentialStore`]. The
//! `packages.password` SQLite column only ever stores the keyring
//! service key as a marker — it never sees the password itself.
//!
//! `password = None` clears both the keyring entry and the marker.
//! Idempotent: clearing an already-empty entry is a no-op.
//!
//! Recovery on keyring failure: the marker is persisted first so a
//! crash between SQLite and the keyring leaves the DB consistent. If
//! the keyring write fails, the marker row is rolled back to its
//! previous value and any partial keyring entry is best-effort
//! cleared, so callers never observe a row claiming a secret that is
//! not actually there.

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;
use crate::domain::model::credential::Credential;
use crate::domain::model::package::{Package, PackageId};

/// Build the keyring service key for a package id. Centralised so the
/// IPC layer, the export/import flow and the keyring port use the same
/// scheme.
pub fn package_credential_service_key(id: &PackageId) -> String {
    format!("vortex.package.{}", id.as_str())
}

impl CommandBus {
    pub async fn handle_set_package_password(
        &self,
        cmd: super::SetPackagePasswordCommand,
    ) -> Result<(), AppError> {
        let repo = self
            .package_repo()
            .ok_or_else(|| AppError::Validation("package repository not configured".into()))?;
        let credentials = self.credential_store();

        let existing = repo
            .find_by_id(&cmd.id)?
            .ok_or_else(|| AppError::NotFound(format!("Package {} not found", cmd.id)))?;

        let key = package_credential_service_key(&cmd.id);
        let marker = match cmd.password.as_deref() {
            Some("") => {
                return Err(AppError::Validation(
                    "package password must not be empty (pass None to clear)".into(),
                ));
            }
            Some(_) => Some(key.clone()),
            None => None,
        };

        // Capture the existing credential BEFORE persisting the new row so
        // a keyring rotation failure can restore exactly what was there
        // before. Without this snapshot the cleanup branch below would
        // unconditionally `delete(&key)` and erase a previously valid
        // secret on a transient backend error.
        let previous_credential = credentials.get(&key)?;

        // Persist the marker BEFORE touching the keyring so a crash between
        // the two writes leaves the DB consistent. The reverse order would
        // leave an orphan keyring secret with no DB marker pointing at it.
        let updated = Package::reconstruct(
            existing.id().clone(),
            existing.name().to_string(),
            existing.source_type(),
            existing.folder_path().map(str::to_string),
            marker,
            existing.auto_extract(),
            existing.priority(),
            existing.created_at(),
        )?;
        repo.save(&updated)?;

        let keyring_op = match cmd.password.as_deref() {
            Some(secret) => credentials.store(&key, &Credential::new(String::new(), secret)),
            None => credentials.delete(&key),
        };
        if let Err(e) = keyring_op {
            // Roll the marker back so the row never claims a secret the
            // keyring does not have. Mirrors `update_account`'s recovery
            // path; both rollback and partial-write cleanup are best
            // effort because the keyring backend may have side-effects we
            // cannot undo.
            if let Err(rollback_err) = repo.save(&existing) {
                tracing::warn!(
                    package_id = %cmd.id,
                    keyring_error = %e,
                    rollback_error = %rollback_err,
                    "package marker rollback failed after keyring error; row metadata diverges from keyring"
                );
            }
            // Restore the prior keyring entry (or wipe if there was none)
            // so a transient store failure cannot destroy an
            // already-configured password while the command was rotating.
            let restore_result = match previous_credential {
                Some(prev) => credentials.store(&key, &prev),
                None => credentials.delete(&key),
            };
            if let Err(restore_err) = restore_result {
                tracing::warn!(
                    package_id = %cmd.id,
                    keyring_error = %e,
                    restore_error = %restore_err,
                    "keyring restore failed after rollback; keyring may hold a partially written secret"
                );
            }
            return Err(e.into());
        }

        self.event_bus()
            .publish(DomainEvent::PackageUpdated { id: cmd.id.clone() });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::{CreatePackageCommand, SetPackagePasswordCommand};
    use crate::application::commands::tests_support::{
        CapturingEventBus, InMemoryCredentialStore, InMemoryDownloadRepo, InMemoryPackageRepo,
        build_package_bus,
    };
    use crate::application::error::AppError;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::package::{PackageId, PackageSourceType};
    use crate::domain::ports::driven::{CredentialStore, PackageRepository};

    async fn seed(bus: &crate::application::command_bus::CommandBus) -> PackageId {
        bus.handle_create_package(CreatePackageCommand {
            name: "Pkg".into(),
            source_type: PackageSourceType::Manual,
            folder_path: None,
            created_at_ms: 0,
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn test_set_package_password_persists_secret_in_keyring_only() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds.clone(), events.clone(), dl_repo);
        let id = seed(&bus).await;

        bus.handle_set_package_password(SetPackagePasswordCommand {
            id: id.clone(),
            password: Some("s3cret".into()),
        })
        .await
        .expect("set ok");

        let key = format!("vortex.package.{}", id.as_str());
        let secret = creds.get(&key).unwrap().expect("present");
        assert_eq!(secret.password(), "s3cret");

        let stored = repo.find_by_id(&id).unwrap().unwrap();
        assert_eq!(
            stored.password(),
            Some(key.as_str()),
            "package row holds the keyring service key marker, never the secret"
        );
        assert!(events.snapshot().iter().any(|e| matches!(
            e,
            DomainEvent::PackageUpdated { id: x } if x == &id
        )));
    }

    #[tokio::test]
    async fn test_set_package_password_clear_removes_keyring_and_marker() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds.clone(), events, dl_repo);
        let id = seed(&bus).await;

        bus.handle_set_package_password(SetPackagePasswordCommand {
            id: id.clone(),
            password: Some("x".into()),
        })
        .await
        .unwrap();
        bus.handle_set_package_password(SetPackagePasswordCommand {
            id: id.clone(),
            password: None,
        })
        .await
        .unwrap();

        let stored = repo.find_by_id(&id).unwrap().unwrap();
        assert!(stored.password().is_none());
        assert_eq!(creds.entry_count(), 0);
    }

    #[tokio::test]
    async fn test_set_package_password_empty_string_is_validation_error() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds.clone(), events, dl_repo);
        let id = seed(&bus).await;

        let err = bus
            .handle_set_package_password(SetPackagePasswordCommand {
                id: id.clone(),
                password: Some(String::new()),
            })
            .await
            .expect_err("empty rejected");
        assert!(matches!(err, AppError::Validation(_)));
        assert_eq!(creds.entry_count(), 0);
    }

    #[tokio::test]
    async fn test_set_package_password_keyring_failure_rolls_back_marker_on_set() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds.clone(), events.clone(), dl_repo);
        let id = seed(&bus).await;

        creds.set_store_fails(true);
        let err = bus
            .handle_set_package_password(SetPackagePasswordCommand {
                id: id.clone(),
                password: Some("never-lands".into()),
            })
            .await
            .expect_err("keyring fail surfaces");
        assert!(matches!(err, AppError::Domain(_)));

        // DB marker rolled back to None (the original state) and keyring
        // is empty — no event emitted because the command failed.
        let stored = repo.find_by_id(&id).unwrap().unwrap();
        assert!(
            stored.password().is_none(),
            "marker must roll back to original on keyring failure"
        );
        assert_eq!(creds.entry_count(), 0);
        assert!(
            !events
                .snapshot()
                .iter()
                .any(|e| matches!(e, DomainEvent::PackageUpdated { id: x } if x == &id)),
            "no PackageUpdated emitted for a failed command"
        );
    }

    #[tokio::test]
    async fn test_set_package_password_failed_rotation_preserves_previous_secret() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds.clone(), events, dl_repo);
        let id = seed(&bus).await;
        bus.handle_set_package_password(SetPackagePasswordCommand {
            id: id.clone(),
            password: Some("original".into()),
        })
        .await
        .unwrap();
        let key = format!("vortex.package.{}", id.as_str());
        assert_eq!(creds.get(&key).unwrap().unwrap().password(), "original");

        // Rotation fails partway: the new write errors but the previous
        // secret must NOT be destroyed by the cleanup branch. The current
        // marker should also be back to pointing at the (still valid)
        // existing key.
        creds.set_store_fails(true);
        let err = bus
            .handle_set_package_password(SetPackagePasswordCommand {
                id: id.clone(),
                password: Some("rotated".into()),
            })
            .await
            .expect_err("rotate fail surfaces");
        assert!(matches!(err, AppError::Domain(_)));
        creds.set_store_fails(false);

        let stored = repo.find_by_id(&id).unwrap().unwrap();
        assert_eq!(
            stored.password(),
            Some(key.as_str()),
            "marker rolled back to previous keyring pointer"
        );
        assert_eq!(
            creds.get(&key).unwrap().expect("survives").password(),
            "original",
            "failed rotation must not erase the prior valid secret"
        );
    }

    #[tokio::test]
    async fn test_set_package_password_keyring_failure_rolls_back_marker_on_clear() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds.clone(), events, dl_repo);
        let id = seed(&bus).await;
        bus.handle_set_package_password(SetPackagePasswordCommand {
            id: id.clone(),
            password: Some("seed".into()),
        })
        .await
        .unwrap();
        let key = format!("vortex.package.{}", id.as_str());

        creds.set_delete_fails(true);
        let err = bus
            .handle_set_package_password(SetPackagePasswordCommand {
                id: id.clone(),
                password: None,
            })
            .await
            .expect_err("delete fail surfaces");
        assert!(matches!(err, AppError::Domain(_)));

        // Marker preserved, secret remains in keyring — both sides
        // unchanged so the next retry can converge.
        let stored = repo.find_by_id(&id).unwrap().unwrap();
        assert_eq!(stored.password(), Some(key.as_str()));
        creds.set_delete_fails(false);
        assert_eq!(
            creds.get(&key).unwrap().expect("still present").password(),
            "seed"
        );
    }

    #[tokio::test]
    async fn test_set_package_password_unknown_id_returns_not_found() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo, creds, events, dl_repo);

        let err = bus
            .handle_set_package_password(SetPackagePasswordCommand {
                id: PackageId::new("ghost"),
                password: Some("x".into()),
            })
            .await
            .expect_err("missing");
        assert!(matches!(err, AppError::NotFound(_)));
    }
}
