//! Handler for [`RemoveDownloadFromPackageCommand`](super::RemoveDownloadFromPackageCommand).
//!
//! Detaches the download from any package (the FK is a singleton —
//! package_id is either set or NULL). Idempotent: detaching an
//! already-loose download is a no-op. We still surface a NotFound
//! when the package does not exist so the IPC layer can flag the
//! caller's stale state.

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;

impl CommandBus {
    pub async fn handle_remove_download_from_package(
        &self,
        cmd: super::RemoveDownloadFromPackageCommand,
    ) -> Result<(), AppError> {
        let repo = self
            .package_repo()
            .ok_or_else(|| AppError::Validation("package repository not configured".into()))?;

        if repo.find_by_id(&cmd.package_id)?.is_none() {
            return Err(AppError::NotFound(format!(
                "Package {} not found",
                cmd.package_id
            )));
        }

        repo.detach_download(cmd.download_id)?;
        self.event_bus().publish(DomainEvent::PackageUpdated {
            id: cmd.package_id.clone(),
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::{
        AddDownloadToPackageCommand, CreatePackageCommand, RemoveDownloadFromPackageCommand,
    };
    use crate::application::commands::tests_support::{
        CapturingEventBus, InMemoryCredentialStore, InMemoryDownloadRepo, InMemoryPackageRepo,
        build_package_bus,
    };
    use crate::application::error::AppError;
    use crate::domain::model::download::{Download, DownloadId, Url};
    use crate::domain::model::package::{PackageId, PackageSourceType};
    use crate::domain::ports::driven::PackageRepository;

    fn make_download(id: u64) -> Download {
        Download::new(
            DownloadId(id),
            Url::new("http://example.com/f.zip").unwrap(),
            format!("file-{id}.zip"),
            format!("/tmp/file-{id}.zip"),
        )
    }

    #[tokio::test]
    async fn test_remove_download_from_package_detaches_member() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds, events, dl_repo.clone());
        let id = bus
            .handle_create_package(CreatePackageCommand {
                name: "P".into(),
                source_type: PackageSourceType::Manual,
                folder_path: None,
                created_at_ms: 0,
            })
            .await
            .unwrap();
        dl_repo.seed(make_download(7));
        bus.handle_add_download_to_package(AddDownloadToPackageCommand {
            package_id: id.clone(),
            download_id: DownloadId(7),
        })
        .await
        .unwrap();

        bus.handle_remove_download_from_package(RemoveDownloadFromPackageCommand {
            package_id: id.clone(),
            download_id: DownloadId(7),
        })
        .await
        .expect("detach");

        assert!(repo.list_downloads(&id).unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_remove_download_from_package_idempotent_when_not_attached() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo, creds, events, dl_repo);
        let id = bus
            .handle_create_package(CreatePackageCommand {
                name: "P".into(),
                source_type: PackageSourceType::Manual,
                folder_path: None,
                created_at_ms: 0,
            })
            .await
            .unwrap();
        bus.handle_remove_download_from_package(RemoveDownloadFromPackageCommand {
            package_id: id,
            download_id: DownloadId(404),
        })
        .await
        .expect("idempotent");
    }

    #[tokio::test]
    async fn test_remove_download_from_package_unknown_package_rejected() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo, creds, events, dl_repo);
        let err = bus
            .handle_remove_download_from_package(RemoveDownloadFromPackageCommand {
                package_id: PackageId::new("ghost"),
                download_id: DownloadId(1),
            })
            .await
            .expect_err("missing pkg");
        assert!(matches!(err, AppError::NotFound(_)));
    }
}
