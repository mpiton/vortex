//! Handler for [`MovePackageToFolderCommand`](super::MovePackageToFolderCommand).
//!
//! Walks the package's member downloads and re-uses the per-download
//! move logic (task 13's `ChangeDirectoryCommand`) for each one. The
//! package row's `folder_path` is updated to the new folder so future
//! members default to the same destination.
//!
//! Per-child failures are collected and returned as
//! [`PackageMoveOutcome`] so the frontend can surface partial success
//! without aborting the whole package — same pattern as
//! `ChangeDirectoryBulkCommand`.

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;
use crate::domain::model::package::Package;

use super::PackageMoveOutcome;
use super::change_directory::ChangeDirectoryFailure;

impl CommandBus {
    pub async fn handle_move_package_to_folder(
        &self,
        cmd: super::MovePackageToFolderCommand,
    ) -> Result<PackageMoveOutcome, AppError> {
        let repo = self
            .package_repo()
            .ok_or_else(|| AppError::Validation("package repository not configured".into()))?;

        let existing = repo
            .find_by_id(&cmd.id)?
            .ok_or_else(|| AppError::NotFound(format!("Package {} not found", cmd.id)))?;

        let new_folder_str = cmd.new_folder.to_string_lossy().to_string();
        if new_folder_str.trim().is_empty() {
            return Err(AppError::Validation(
                "destination folder must not be empty".into(),
            ));
        }
        // Reject relative paths so a crafted IPC payload (e.g. "../") cannot
        // walk outside the working directory before the per-download move
        // routines run.
        if !cmd.new_folder.is_absolute() {
            return Err(AppError::Validation(
                "destination folder must be an absolute path".into(),
            ));
        }

        let updated = Package::reconstruct(
            existing.id().clone(),
            existing.name().to_string(),
            existing.source_type(),
            Some(new_folder_str),
            existing.password().map(str::to_string),
            existing.auto_extract(),
            existing.priority(),
            existing.created_at(),
        )?;
        repo.save(&updated)?;

        let members = repo.list_downloads(&cmd.id)?;
        let mut outcome = PackageMoveOutcome::default();
        for download_id in members {
            match self
                .handle_change_directory(super::ChangeDirectoryCommand {
                    id: download_id,
                    new_destination_dir: cmd.new_folder.clone(),
                })
                .await
            {
                Ok(()) => outcome.moved.push(download_id),
                Err(e) => outcome.failed.push(ChangeDirectoryFailure {
                    id: download_id,
                    message: e.to_string(),
                }),
            }
        }

        self.event_bus()
            .publish(DomainEvent::PackageUpdated { id: cmd.id.clone() });
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::super::{CreatePackageCommand, MovePackageToFolderCommand};
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

    async fn seed(
        bus: &crate::application::command_bus::CommandBus,
        repo: &Arc<InMemoryPackageRepo>,
        dl_repo: &Arc<InMemoryDownloadRepo>,
        members: &[u64],
    ) -> PackageId {
        let id = bus
            .handle_create_package(CreatePackageCommand {
                name: "Pkg".into(),
                source_type: PackageSourceType::Manual,
                folder_path: None,
                created_at_ms: 0,
            })
            .await
            .unwrap();
        for d in members {
            dl_repo.seed(make_download(*d));
            repo.attach_download(&id, DownloadId(*d)).unwrap();
        }
        id
    }

    #[tokio::test]
    async fn test_move_package_updates_folder_and_each_download_path() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds, events.clone(), dl_repo.clone());
        let id = seed(&bus, &repo, &dl_repo, &[1, 2]).await;

        let outcome = bus
            .handle_move_package_to_folder(MovePackageToFolderCommand {
                id: id.clone(),
                new_folder: PathBuf::from("/srv/new"),
            })
            .await
            .expect("move ok");
        assert_eq!(outcome.moved.len(), 2);
        assert!(outcome.failed.is_empty());

        let stored = repo.find_by_id(&id).unwrap().unwrap();
        assert_eq!(stored.folder_path(), Some("/srv/new"));
        for i in [1u64, 2] {
            let dl = dl_repo.find_by_id(DownloadId(i)).unwrap().unwrap();
            assert_eq!(dl.destination_path(), format!("/srv/new/file-{i}.zip"));
        }
        assert!(events.snapshot().iter().any(|e| matches!(
            e,
            DomainEvent::PackageUpdated { id: x } if x == &id
        )));
    }

    #[tokio::test]
    async fn test_move_package_empty_destination_rejected() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds, events, dl_repo.clone());
        let id = seed(&bus, &repo, &dl_repo, &[]).await;

        let err = bus
            .handle_move_package_to_folder(MovePackageToFolderCommand {
                id: id.clone(),
                new_folder: PathBuf::from(""),
            })
            .await
            .expect_err("empty path rejected");
        assert!(matches!(err, AppError::Validation(_)));
        let stored = repo.find_by_id(&id).unwrap().unwrap();
        assert!(stored.folder_path().is_none());
    }

    #[tokio::test]
    async fn test_move_package_relative_path_rejected() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds, events, dl_repo.clone());
        let id = seed(&bus, &repo, &dl_repo, &[]).await;

        for relative in ["../escape", "./local", "relative/sub"] {
            let err = bus
                .handle_move_package_to_folder(MovePackageToFolderCommand {
                    id: id.clone(),
                    new_folder: PathBuf::from(relative),
                })
                .await
                .expect_err("relative rejected");
            assert!(
                matches!(err, AppError::Validation(_)),
                "expected validation error for {relative:?}"
            );
        }
        let stored = repo.find_by_id(&id).unwrap().unwrap();
        assert!(stored.folder_path().is_none());
    }

    #[tokio::test]
    async fn test_move_package_unknown_id_returns_not_found() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo, creds, events, dl_repo);

        let err = bus
            .handle_move_package_to_folder(MovePackageToFolderCommand {
                id: PackageId::new("ghost"),
                new_folder: PathBuf::from("/x"),
            })
            .await
            .expect_err("missing");
        assert!(matches!(err, AppError::NotFound(_)));
    }
}
