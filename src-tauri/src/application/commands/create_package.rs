//! Handler for [`CreatePackageCommand`](super::CreatePackageCommand).
//!
//! Generates a fresh [`PackageId`] (UUID v4), validates the inputs,
//! persists the aggregate via [`PackageRepository`], and emits
//! [`DomainEvent::PackageCreated`] on success.

use uuid::Uuid;

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;
use crate::domain::model::package::{Package, PackageId};

impl CommandBus {
    pub async fn handle_create_package(
        &self,
        cmd: super::CreatePackageCommand,
    ) -> Result<PackageId, AppError> {
        let name = cmd.name.trim();
        if name.is_empty() {
            return Err(AppError::Validation(
                "package name must not be empty".into(),
            ));
        }
        let folder_path = cmd
            .folder_path
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());

        let repo = self
            .package_repo()
            .ok_or_else(|| AppError::Validation("package repository not configured".into()))?;

        let id = PackageId::new(Uuid::new_v4().to_string());
        let mut package = Package::new(
            id.clone(),
            name.to_string(),
            cmd.source_type,
            cmd.created_at_ms,
        );
        if folder_path.is_some() {
            package.set_folder_path(folder_path);
        }

        repo.save(&package)?;
        self.event_bus().publish(DomainEvent::PackageCreated {
            id: id.clone(),
            name: package.name().to_string(),
        });
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::CreatePackageCommand;
    use crate::application::commands::tests_support::{
        CapturingEventBus, InMemoryCredentialStore, InMemoryDownloadRepo, InMemoryPackageRepo,
        build_package_bus,
    };
    use crate::application::error::AppError;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::package::PackageSourceType;
    use crate::domain::ports::driven::PackageRepository;

    fn create_command(name: &str) -> CreatePackageCommand {
        CreatePackageCommand {
            name: name.into(),
            source_type: PackageSourceType::Manual,
            folder_path: None,
            created_at_ms: 1_700_000_000_000,
        }
    }

    #[tokio::test]
    async fn test_create_package_persists_and_emits_event() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds, events.clone(), dl_repo);

        let id = bus
            .handle_create_package(create_command("Holiday"))
            .await
            .expect("create ok");

        let stored = repo.find_by_id(&id).unwrap().expect("present");
        assert_eq!(stored.name(), "Holiday");
        assert_eq!(stored.source_type(), PackageSourceType::Manual);
        assert_eq!(stored.created_at(), 1_700_000_000_000);

        let snapshot = events.snapshot();
        assert!(snapshot.iter().any(|e| matches!(
            e,
            DomainEvent::PackageCreated { id: ev_id, name } if ev_id == &id && name == "Holiday"
        )));
    }

    #[tokio::test]
    async fn test_create_package_persists_folder_path_when_provided() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds, events, dl_repo);

        let id = bus
            .handle_create_package(CreatePackageCommand {
                name: "Vids".into(),
                source_type: PackageSourceType::Playlist,
                folder_path: Some("/srv/vids".into()),
                created_at_ms: 0,
            })
            .await
            .expect("create ok");

        let stored = repo.find_by_id(&id).unwrap().unwrap();
        assert_eq!(stored.folder_path(), Some("/srv/vids"));
    }

    #[tokio::test]
    async fn test_create_package_blank_name_rejected() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds, events.clone(), dl_repo);

        let err = bus
            .handle_create_package(create_command("   "))
            .await
            .expect_err("blank name rejected");
        assert!(matches!(err, AppError::Validation(_)));
        assert!(repo.list().unwrap().is_empty());
        assert!(events.snapshot().is_empty());
    }

    #[tokio::test]
    async fn test_create_package_without_repo_returns_validation() {
        let creds = Arc::new(InMemoryCredentialStore::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus =
            crate::application::commands::tests_support::bus_without_account_ports(events.clone());
        let _ = creds;
        let err = bus
            .handle_create_package(create_command("X"))
            .await
            .expect_err("no repo");
        assert!(matches!(err, AppError::Validation(_)));
    }
}
