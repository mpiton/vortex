//! Handler for [`SetPackagePasswordCommand`](super::SetPackagePasswordCommand).
//!
//! The plaintext password is persisted in the OS keyring under the
//! convention `vortex.package.<id>` via [`CredentialStore`]. The
//! `packages.password` SQLite column only ever stores the keyring
//! service key as a marker — it never sees the password itself.
//!
//! `password = None` clears both the keyring entry and the marker.
//! Idempotent: clearing an already-empty entry is a no-op.

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

        // Persist the marker BEFORE touching the keyring. If the keyring
        // write fails the DB still describes a consistent state; on retry
        // both sides converge because `store`/`delete` are idempotent. The
        // reverse order would leave an orphaned keyring secret with no DB
        // marker pointing at it.
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

        match cmd.password.as_deref() {
            Some(secret) => credentials.store(&key, &Credential::new(String::new(), secret))?,
            None => credentials.delete(&key)?,
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
