//! Handler for [`AddDownloadToPackageCommand`](super::AddDownloadToPackageCommand).
//!
//! Verifies that both the package and the download exist, then sets
//! the FK on the download row via `PackageRepository::attach_download`.
//! Idempotent at the repo layer (re-attaching is a no-op).

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;

impl CommandBus {
    pub async fn handle_add_download_to_package(
        &self,
        cmd: super::AddDownloadToPackageCommand,
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
        if self.download_repo().find_by_id(cmd.download_id)?.is_none() {
            return Err(AppError::NotFound(format!(
                "Download {} not found",
                cmd.download_id.0
            )));
        }

        repo.attach_download(&cmd.package_id, cmd.download_id)?;
        self.event_bus().publish(DomainEvent::PackageUpdated {
            id: cmd.package_id.clone(),
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::{AddDownloadToPackageCommand, CreatePackageCommand};
    use crate::application::commands::tests_support::{
        CapturingEventBus, InMemoryCredentialStore, InMemoryDownloadRepo, InMemoryPackageRepo,
        build_package_bus,
    };
    use crate::application::error::AppError;
    use crate::domain::event::DomainEvent;
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
    async fn test_add_download_to_package_attaches_member() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds, events.clone(), dl_repo.clone());
        let id = bus
            .handle_create_package(CreatePackageCommand {
                name: "P".into(),
                source_type: PackageSourceType::Manual,
                folder_path: None,
                created_at_ms: 0,
            })
            .await
            .unwrap();
        dl_repo.seed(make_download(42));

        bus.handle_add_download_to_package(AddDownloadToPackageCommand {
            package_id: id.clone(),
            download_id: DownloadId(42),
        })
        .await
        .expect("attach");

        assert_eq!(
            repo.list_downloads(&id).unwrap(),
            vec![DownloadId(42)],
            "member registered"
        );
        assert!(events.snapshot().iter().any(|e| matches!(
            e,
            DomainEvent::PackageUpdated { id: x } if x == &id
        )));
    }

    #[tokio::test]
    async fn test_add_download_to_package_unknown_package_rejected() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo, creds, events, dl_repo.clone());
        dl_repo.seed(make_download(1));

        let err = bus
            .handle_add_download_to_package(AddDownloadToPackageCommand {
                package_id: PackageId::new("ghost"),
                download_id: DownloadId(1),
            })
            .await
            .expect_err("missing pkg");
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_add_download_to_package_unknown_download_rejected() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds, events, dl_repo);
        let id = bus
            .handle_create_package(CreatePackageCommand {
                name: "P".into(),
                source_type: PackageSourceType::Manual,
                folder_path: None,
                created_at_ms: 0,
            })
            .await
            .unwrap();
        let err = bus
            .handle_add_download_to_package(AddDownloadToPackageCommand {
                package_id: id,
                download_id: DownloadId(999),
            })
            .await
            .expect_err("missing dl");
        assert!(matches!(err, AppError::NotFound(_)));
    }
}
