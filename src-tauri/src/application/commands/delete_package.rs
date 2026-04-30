//! Handler for [`DeletePackageCommand`](super::DeletePackageCommand).
//!
//! Two cleanup paths:
//! - `delete_downloads = false` (default): the FK on member downloads
//!   is cleared (`detach_download` on each), then the package row is
//!   removed. Downloads survive as detached entries.
//! - `delete_downloads = true`: every member download is removed via
//!   the existing `RemoveDownloadCommand` (which deletes engine state,
//!   files, and the SQLite row), then the package row is removed.
//!
//! In both cases the keyring entry for the package password is best-
//! effort cleaned. Failures are logged but never block the deletion —
//! the package metadata is the source of truth for "package exists".

use super::set_package_password::package_credential_service_key;
use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;

impl CommandBus {
    pub async fn handle_delete_package(
        &self,
        cmd: super::DeletePackageCommand,
    ) -> Result<(), AppError> {
        let repo = self
            .package_repo()
            .ok_or_else(|| AppError::Validation("package repository not configured".into()))?;

        // Look up the existing aggregate and its members up front so the
        // cascade decision works off a frozen snapshot. NotFound is
        // surfaced as a hard error rather than silently no-op'd because
        // double-delete is a UI bug, not a benign retry.
        repo.find_by_id(&cmd.id)?
            .ok_or_else(|| AppError::NotFound(format!("Package {} not found", cmd.id)))?;
        let members = repo.list_downloads(&cmd.id)?;

        if cmd.delete_downloads {
            for download_id in &members {
                self.handle_remove_download(super::RemoveDownloadCommand {
                    id: *download_id,
                    delete_files: true,
                })
                .await?;
            }
        } else {
            for download_id in &members {
                repo.detach_download(*download_id)?;
            }
        }

        repo.delete(&cmd.id)?;

        let key = package_credential_service_key(&cmd.id);
        if let Err(e) = self.credential_store().delete(&key) {
            tracing::warn!(
                package_id = %cmd.id,
                error = %e,
                "failed to remove package keyring entry on delete"
            );
        }

        self.event_bus().publish(DomainEvent::PackageDeleted {
            id: cmd.id,
            delete_downloads: cmd.delete_downloads,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::{CreatePackageCommand, DeletePackageCommand, SetPackagePasswordCommand};
    use crate::application::commands::tests_support::{
        CapturingEventBus, InMemoryCredentialStore, InMemoryDownloadRepo, InMemoryPackageRepo,
        build_package_bus,
    };
    use crate::application::error::AppError;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::download::{Download, DownloadId, Url};
    use crate::domain::model::package::{PackageId, PackageSourceType};
    use crate::domain::ports::driven::{DownloadRepository, PackageRepository};

    fn make_download(id: u64) -> Download {
        Download::new(
            DownloadId(id),
            Url::new("http://example.com/f.zip").unwrap(),
            format!("file-{id}.zip"),
            format!("/tmp/file-{id}.zip"),
        )
    }

    async fn seed_package(bus: &crate::application::command_bus::CommandBus) -> PackageId {
        bus.handle_create_package(CreatePackageCommand {
            name: "P".into(),
            source_type: PackageSourceType::Manual,
            folder_path: None,
            created_at_ms: 0,
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn test_delete_package_without_cascade_detaches_members_and_removes_row() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds, events.clone(), dl_repo.clone());
        let id = seed_package(&bus).await;

        dl_repo.seed(make_download(1));
        dl_repo.seed(make_download(2));
        repo.attach_download(&id, DownloadId(1)).unwrap();
        repo.attach_download(&id, DownloadId(2)).unwrap();

        bus.handle_delete_package(DeletePackageCommand {
            id: id.clone(),
            delete_downloads: false,
        })
        .await
        .expect("delete");

        assert!(repo.find_by_id(&id).unwrap().is_none());
        // Downloads keep existing.
        assert!(dl_repo.find_by_id(DownloadId(1)).unwrap().is_some());
        assert!(dl_repo.find_by_id(DownloadId(2)).unwrap().is_some());
        // FK detached.
        assert!(repo.list_downloads(&id).unwrap().is_empty());
        assert!(events.snapshot().iter().any(|e| matches!(
            e,
            DomainEvent::PackageDeleted { id: x, delete_downloads: false } if x == &id
        )));
    }

    #[tokio::test]
    async fn test_delete_package_cascade_removes_member_downloads() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds, events.clone(), dl_repo.clone());
        let id = seed_package(&bus).await;

        dl_repo.seed(make_download(10));
        dl_repo.seed(make_download(20));
        repo.attach_download(&id, DownloadId(10)).unwrap();
        repo.attach_download(&id, DownloadId(20)).unwrap();

        bus.handle_delete_package(DeletePackageCommand {
            id: id.clone(),
            delete_downloads: true,
        })
        .await
        .expect("delete cascade");

        assert!(repo.find_by_id(&id).unwrap().is_none());
        assert!(dl_repo.find_by_id(DownloadId(10)).unwrap().is_none());
        assert!(dl_repo.find_by_id(DownloadId(20)).unwrap().is_none());
        assert!(events.snapshot().iter().any(|e| matches!(
            e,
            DomainEvent::PackageDeleted {
                delete_downloads: true,
                ..
            }
        )));
    }

    #[tokio::test]
    async fn test_delete_package_cleans_up_keyring_password() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo, creds.clone(), events, dl_repo);
        let id = seed_package(&bus).await;
        bus.handle_set_package_password(SetPackagePasswordCommand {
            id: id.clone(),
            password: Some("k".into()),
        })
        .await
        .unwrap();
        assert_eq!(creds.entry_count(), 1);

        bus.handle_delete_package(DeletePackageCommand {
            id,
            delete_downloads: false,
        })
        .await
        .expect("delete");
        assert_eq!(creds.entry_count(), 0);
    }

    #[tokio::test]
    async fn test_delete_package_unknown_id_returns_not_found() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo, creds, events, dl_repo);

        let err = bus
            .handle_delete_package(DeletePackageCommand {
                id: PackageId::new("ghost"),
                delete_downloads: false,
            })
            .await
            .expect_err("missing");
        assert!(matches!(err, AppError::NotFound(_)));
    }
}
