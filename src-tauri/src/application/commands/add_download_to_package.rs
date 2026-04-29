//! Handler for [`AddDownloadToPackageCommand`](super::AddDownloadToPackageCommand).
//!
//! Verifies that both the package and the download exist, then sets
//! the FK on the download row via `PackageRepository::attach_download`.
//! Idempotent at the repo layer (re-attaching is a no-op).
//!
//! Reassignment (download already belongs to another package) is
//! supported — `attach_download` overwrites the FK. To keep event
//! consumers (counts, lists) consistent, both the source and the
//! destination package emit `PackageUpdated` so the source's listing
//! refreshes alongside the destination's.

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

        let previous_owner = repo.find_package_of_download(cmd.download_id)?;
        repo.attach_download(&cmd.package_id, cmd.download_id)?;

        if let Some(prev) = previous_owner.filter(|p| p != &cmd.package_id) {
            self.event_bus()
                .publish(DomainEvent::PackageUpdated { id: prev });
        }
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
    async fn test_add_download_to_package_reassignment_emits_for_source_and_destination() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds, events.clone(), dl_repo.clone());
        let source = bus
            .handle_create_package(CreatePackageCommand {
                name: "Src".into(),
                source_type: PackageSourceType::Manual,
                folder_path: None,
                created_at_ms: 0,
            })
            .await
            .unwrap();
        let destination = bus
            .handle_create_package(CreatePackageCommand {
                name: "Dst".into(),
                source_type: PackageSourceType::Manual,
                folder_path: None,
                created_at_ms: 1,
            })
            .await
            .unwrap();
        dl_repo.seed(make_download(7));
        repo.attach_download(&source, DownloadId(7)).unwrap();

        bus.handle_add_download_to_package(AddDownloadToPackageCommand {
            package_id: destination.clone(),
            download_id: DownloadId(7),
        })
        .await
        .expect("reassign");

        // FK now points at destination, source bucket is empty.
        assert_eq!(
            repo.list_downloads(&destination).unwrap(),
            vec![DownloadId(7)]
        );
        assert!(repo.list_downloads(&source).unwrap().is_empty());

        let snap = events.snapshot();
        let updated_for = |target: &PackageId| {
            snap.iter()
                .filter(|e| matches!(e, DomainEvent::PackageUpdated { id } if id == target))
                .count()
        };
        assert_eq!(updated_for(&source), 1, "source emits once on hand-off");
        assert_eq!(
            updated_for(&destination),
            1,
            "destination emits once on hand-off"
        );
    }

    #[tokio::test]
    async fn test_add_download_to_package_idempotent_does_not_double_emit() {
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
        dl_repo.seed(make_download(11));

        for _ in 0..2 {
            bus.handle_add_download_to_package(AddDownloadToPackageCommand {
                package_id: id.clone(),
                download_id: DownloadId(11),
            })
            .await
            .unwrap();
        }

        // Same destination twice → no source emit (previous_owner == destination).
        let updates = events
            .snapshot()
            .iter()
            .filter(|e| matches!(e, DomainEvent::PackageUpdated { id: x } if x == &id))
            .count();
        assert_eq!(updates, 2, "one PackageUpdated per call, never doubled");
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
