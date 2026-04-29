//! Handler for [`UpdatePackageCommand`](super::UpdatePackageCommand).
//!
//! Applies a [`PackagePatch`](super::PackagePatch) to an existing
//! package and persists the result. Optional fields left as `None`
//! keep the persisted value untouched.
//!
//! Priority changes via this handler do NOT cascade to member
//! downloads — that is `set_package_priority`'s job. Use this command
//! for plain rename / folder / auto-extract / package-priority edits.

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;
use crate::domain::model::package::Package;

impl CommandBus {
    pub async fn handle_update_package(
        &self,
        cmd: super::UpdatePackageCommand,
    ) -> Result<(), AppError> {
        let repo = self
            .package_repo()
            .ok_or_else(|| AppError::Validation("package repository not configured".into()))?;

        let existing = repo
            .find_by_id(&cmd.id)?
            .ok_or_else(|| AppError::NotFound(format!("Package {} not found", cmd.id)))?;

        let mut updated = clone_for_update(&existing);

        if let Some(name) = cmd.patch.name {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                return Err(AppError::Validation(
                    "package name must not be empty".into(),
                ));
            }
            updated = Package::reconstruct(
                updated.id().clone(),
                trimmed.to_string(),
                updated.source_type(),
                updated.folder_path().map(str::to_string),
                updated.password().map(str::to_string),
                updated.auto_extract(),
                updated.priority(),
                updated.created_at(),
            )?;
        }
        if let Some(folder) = cmd.patch.folder_path {
            let normalised = folder
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            updated.set_folder_path(normalised);
        }
        if let Some(priority) = cmd.patch.priority {
            updated.set_priority(priority)?;
        }
        if let Some(auto_extract) = cmd.patch.auto_extract {
            updated.set_auto_extract(auto_extract);
        }

        repo.save(&updated)?;
        self.event_bus().publish(DomainEvent::PackageUpdated {
            id: updated.id().clone(),
        });
        Ok(())
    }
}

fn clone_for_update(existing: &Package) -> Package {
    Package::reconstruct(
        existing.id().clone(),
        existing.name().to_string(),
        existing.source_type(),
        existing.folder_path().map(str::to_string),
        existing.password().map(str::to_string),
        existing.auto_extract(),
        existing.priority(),
        existing.created_at(),
    )
    .expect("existing package always validates")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::{CreatePackageCommand, PackagePatch, UpdatePackageCommand};
    use crate::application::commands::tests_support::{
        CapturingEventBus, InMemoryCredentialStore, InMemoryDownloadRepo, InMemoryPackageRepo,
        build_package_bus,
    };
    use crate::application::error::AppError;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::package::{PackageId, PackageSourceType};
    use crate::domain::ports::driven::PackageRepository;

    async fn seed_package(bus: &crate::application::command_bus::CommandBus) -> PackageId {
        bus.handle_create_package(CreatePackageCommand {
            name: "Initial".into(),
            source_type: PackageSourceType::Manual,
            folder_path: None,
            created_at_ms: 1,
        })
        .await
        .expect("seed package")
    }

    #[tokio::test]
    async fn test_update_package_renames_and_emits_event() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds, events.clone(), dl_repo);
        let id = seed_package(&bus).await;

        bus.handle_update_package(UpdatePackageCommand {
            id: id.clone(),
            patch: PackagePatch {
                name: Some("Renamed".into()),
                ..Default::default()
            },
        })
        .await
        .expect("update");

        let stored = repo.find_by_id(&id).unwrap().unwrap();
        assert_eq!(stored.name(), "Renamed");
        assert!(events.snapshot().iter().any(|e| matches!(
            e,
            DomainEvent::PackageUpdated { id: x } if x == &id
        )));
    }

    #[tokio::test]
    async fn test_update_package_changes_folder_priority_and_auto_extract() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds, events, dl_repo);
        let id = seed_package(&bus).await;

        bus.handle_update_package(UpdatePackageCommand {
            id: id.clone(),
            patch: PackagePatch {
                name: None,
                folder_path: Some(Some("/srv/packs".into())),
                priority: Some(9),
                auto_extract: Some(false),
            },
        })
        .await
        .expect("update");

        let stored = repo.find_by_id(&id).unwrap().unwrap();
        assert_eq!(stored.folder_path(), Some("/srv/packs"));
        assert_eq!(stored.priority(), 9);
        assert!(!stored.auto_extract());
    }

    #[tokio::test]
    async fn test_update_package_clears_folder_when_some_none() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds, events, dl_repo);
        let id = seed_package(&bus).await;
        bus.handle_update_package(UpdatePackageCommand {
            id: id.clone(),
            patch: PackagePatch {
                folder_path: Some(Some("/x".into())),
                ..Default::default()
            },
        })
        .await
        .unwrap();

        bus.handle_update_package(UpdatePackageCommand {
            id: id.clone(),
            patch: PackagePatch {
                folder_path: Some(None),
                ..Default::default()
            },
        })
        .await
        .unwrap();

        let stored = repo.find_by_id(&id).unwrap().unwrap();
        assert!(stored.folder_path().is_none());
    }

    #[tokio::test]
    async fn test_update_package_blank_name_rejected() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds, events, dl_repo);
        let id = seed_package(&bus).await;

        let err = bus
            .handle_update_package(UpdatePackageCommand {
                id: id.clone(),
                patch: PackagePatch {
                    name: Some("   ".into()),
                    ..Default::default()
                },
            })
            .await
            .expect_err("blank rejected");
        assert!(matches!(err, AppError::Validation(_)));
        let stored = repo.find_by_id(&id).unwrap().unwrap();
        assert_eq!(stored.name(), "Initial");
    }

    #[tokio::test]
    async fn test_update_package_invalid_priority_rejected() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds, events, dl_repo);
        let id = seed_package(&bus).await;

        let err = bus
            .handle_update_package(UpdatePackageCommand {
                id: id.clone(),
                patch: PackagePatch {
                    priority: Some(0),
                    ..Default::default()
                },
            })
            .await
            .expect_err("0 rejected");
        assert!(matches!(err, AppError::Domain(_)));
        let stored = repo.find_by_id(&id).unwrap().unwrap();
        assert_eq!(stored.priority(), 5);
    }

    #[tokio::test]
    async fn test_update_package_unknown_id_returns_not_found() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo, creds, events, dl_repo);
        let err = bus
            .handle_update_package(UpdatePackageCommand {
                id: PackageId::new("ghost"),
                patch: PackagePatch {
                    name: Some("X".into()),
                    ..Default::default()
                },
            })
            .await
            .expect_err("missing");
        assert!(matches!(err, AppError::NotFound(_)));
    }
}
