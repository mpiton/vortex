//! Handler for [`TogglePackageAutoExtractCommand`](super::TogglePackageAutoExtractCommand).
//!
//! Flips the `auto_extract` flag on the package row. Convenience over
//! [`UpdatePackageCommand`] for a one-shot UI toggle (kebab menu) so
//! callers don't have to read-modify-write the current value.

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;
use crate::domain::model::package::Package;

impl CommandBus {
    pub async fn handle_toggle_package_auto_extract(
        &self,
        cmd: super::TogglePackageAutoExtractCommand,
    ) -> Result<bool, AppError> {
        let repo = self
            .package_repo()
            .ok_or_else(|| AppError::Validation("package repository not configured".into()))?;
        let existing = repo
            .find_by_id(&cmd.id)?
            .ok_or_else(|| AppError::NotFound(format!("Package {} not found", cmd.id)))?;
        let next_value = !existing.auto_extract();
        let updated = Package::reconstruct(
            existing.id().clone(),
            existing.name().to_string(),
            existing.source_type(),
            existing.folder_path().map(str::to_string),
            existing.password().map(str::to_string),
            next_value,
            existing.priority(),
            existing.created_at(),
        )?;
        repo.save(&updated)?;
        self.event_bus()
            .publish(DomainEvent::PackageUpdated { id: cmd.id.clone() });
        Ok(next_value)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::{CreatePackageCommand, TogglePackageAutoExtractCommand};
    use crate::application::commands::tests_support::{
        CapturingEventBus, InMemoryCredentialStore, InMemoryDownloadRepo, InMemoryPackageRepo,
        build_package_bus,
    };
    use crate::application::error::AppError;
    use crate::domain::model::package::{PackageId, PackageSourceType};
    use crate::domain::ports::driven::PackageRepository;

    #[tokio::test]
    async fn test_toggle_auto_extract_flips_value_and_returns_new_state() {
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

        // Default is true (Package::new sets auto_extract = true) → first
        // toggle returns false, second returns true.
        let after_first = bus
            .handle_toggle_package_auto_extract(TogglePackageAutoExtractCommand { id: id.clone() })
            .await
            .unwrap();
        assert!(!after_first);
        assert!(!repo.find_by_id(&id).unwrap().unwrap().auto_extract());

        let after_second = bus
            .handle_toggle_package_auto_extract(TogglePackageAutoExtractCommand { id: id.clone() })
            .await
            .unwrap();
        assert!(after_second);
        assert!(repo.find_by_id(&id).unwrap().unwrap().auto_extract());
    }

    #[tokio::test]
    async fn test_toggle_auto_extract_unknown_id_returns_not_found() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo, creds, events, dl_repo);
        let err = bus
            .handle_toggle_package_auto_extract(TogglePackageAutoExtractCommand {
                id: PackageId::new("ghost"),
            })
            .await
            .expect_err("missing");
        assert!(matches!(err, AppError::NotFound(_)));
    }
}
