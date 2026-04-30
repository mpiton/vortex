//! Handler for [`SetPackagePriorityCommand`](super::SetPackagePriorityCommand).
//!
//! Persists the new priority on the package row, then loops through
//! every member download and updates its `priority` via the existing
//! per-download `Priority` aggregate. Each impacted download triggers
//! a [`DomainEvent::DownloadPrioritySet`] so the queue manager re-
//! evaluates scheduling. The package itself emits a single
//! [`DomainEvent::PackageUpdated`] carrier event.
//!
//! Member downloads that have disappeared (FK left dangling) are
//! skipped with a debug log — the package row is the source of truth
//! for "this priority is now N" and we don't want a stale FK to abort
//! the cascade.

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;
use crate::domain::model::package::Package;
use crate::domain::model::queue::Priority;

impl CommandBus {
    pub async fn handle_set_package_priority(
        &self,
        cmd: super::SetPackagePriorityCommand,
    ) -> Result<(), AppError> {
        let repo = self
            .package_repo()
            .ok_or_else(|| AppError::Validation("package repository not configured".into()))?;

        let existing = repo
            .find_by_id(&cmd.id)?
            .ok_or_else(|| AppError::NotFound(format!("Package {} not found", cmd.id)))?;

        // Validate the priority via the aggregate's invariant before any
        // mutation so a bad value never produces partial cascade state.
        let domain_priority = Priority::new(cmd.priority)?;

        let updated = Package::reconstruct(
            existing.id().clone(),
            existing.name().to_string(),
            existing.source_type(),
            existing.folder_path().map(str::to_string),
            existing.password().map(str::to_string),
            existing.auto_extract(),
            cmd.priority,
            existing.created_at(),
            existing.external_id().map(str::to_string),
        )?;
        repo.save(&updated)?;

        let members = repo.list_downloads(&cmd.id)?;
        for download_id in members {
            let download = match self.download_repo().find_by_id(download_id)? {
                Some(d) => d,
                None => {
                    tracing::debug!(
                        package_id = %cmd.id,
                        download_id = download_id.0,
                        "skipping cascade: member download missing"
                    );
                    continue;
                }
            };
            let next = download.with_priority(domain_priority);
            self.download_repo().save(&next)?;
            self.event_bus().publish(DomainEvent::DownloadPrioritySet {
                id: download_id,
                priority: cmd.priority,
            });
        }

        self.event_bus()
            .publish(DomainEvent::PackageUpdated { id: cmd.id.clone() });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::{CreatePackageCommand, SetPackagePriorityCommand};
    use crate::application::commands::tests_support::{
        CapturingEventBus, InMemoryCredentialStore, InMemoryDownloadRepo, InMemoryPackageRepo,
        build_package_bus,
    };
    use crate::application::error::AppError;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::download::{Download, DownloadId, Url};
    use crate::domain::model::package::{PackageId, PackageSourceType};
    use crate::domain::model::queue::Priority;
    use crate::domain::ports::driven::{DownloadRepository, PackageRepository};

    fn make_download(id: u64) -> Download {
        Download::new(
            DownloadId(id),
            Url::new("http://example.com/f.zip").unwrap(),
            format!("file-{id}.zip"),
            format!("/tmp/file-{id}.zip"),
        )
    }

    async fn seed_package_with_members(
        bus: &crate::application::command_bus::CommandBus,
        repo: &Arc<InMemoryPackageRepo>,
        dl_repo: &Arc<InMemoryDownloadRepo>,
        member_ids: &[u64],
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
        for d in member_ids {
            dl_repo.seed(make_download(*d));
            repo.attach_download(&id, DownloadId(*d)).unwrap();
        }
        id
    }

    #[tokio::test]
    async fn test_set_package_priority_updates_package_row() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds, events.clone(), dl_repo.clone());
        let id = seed_package_with_members(&bus, &repo, &dl_repo, &[]).await;

        bus.handle_set_package_priority(SetPackagePriorityCommand {
            id: id.clone(),
            priority: 8,
        })
        .await
        .expect("set priority");

        let stored = repo.find_by_id(&id).unwrap().unwrap();
        assert_eq!(stored.priority(), 8);
        assert!(events.snapshot().iter().any(|e| matches!(
            e,
            DomainEvent::PackageUpdated { id: x } if x == &id
        )));
    }

    #[tokio::test]
    async fn test_set_package_priority_propagates_to_each_member_download() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds, events.clone(), dl_repo.clone());
        let id = seed_package_with_members(&bus, &repo, &dl_repo, &[1, 2, 3]).await;

        bus.handle_set_package_priority(SetPackagePriorityCommand {
            id: id.clone(),
            priority: 9,
        })
        .await
        .expect("set");

        // Each member persisted with the new priority.
        for i in [1u64, 2, 3] {
            let dl = dl_repo.find_by_id(DownloadId(i)).unwrap().unwrap();
            assert_eq!(dl.priority(), &Priority::new(9).unwrap());
        }

        // One DownloadPrioritySet event per member.
        let snap = events.snapshot();
        let events_count = snap
            .iter()
            .filter(|e| matches!(e, DomainEvent::DownloadPrioritySet { priority: 9, .. }))
            .count();
        assert_eq!(
            events_count, 3,
            "expected one DownloadPrioritySet per member download"
        );
    }

    #[tokio::test]
    async fn test_set_package_priority_invalid_priority_does_not_mutate_package() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds, events, dl_repo.clone());
        let id = seed_package_with_members(&bus, &repo, &dl_repo, &[]).await;

        let err = bus
            .handle_set_package_priority(SetPackagePriorityCommand {
                id: id.clone(),
                priority: 0,
            })
            .await
            .expect_err("0 rejected");
        assert!(matches!(err, AppError::Domain(_)));
        let stored = repo.find_by_id(&id).unwrap().unwrap();
        assert_eq!(stored.priority(), 5, "package row untouched on validation");
    }

    #[tokio::test]
    async fn test_set_package_priority_unknown_id_returns_not_found() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo, creds, events, dl_repo);

        let err = bus
            .handle_set_package_priority(SetPackagePriorityCommand {
                id: PackageId::new("ghost"),
                priority: 5,
            })
            .await
            .expect_err("missing");
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_set_package_priority_skips_dangling_member_silently() {
        // FK can be left dangling if a download was hard-deleted before
        // its package detach ran. The cascade must still update every
        // *existing* member and never abort.
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds, events.clone(), dl_repo.clone());
        let id = bus
            .handle_create_package(CreatePackageCommand {
                name: "Pkg".into(),
                source_type: PackageSourceType::Manual,
                folder_path: None,
                created_at_ms: 0,
            })
            .await
            .unwrap();
        // Member 7 exists, 999 doesn't. Both attached.
        dl_repo.seed(make_download(7));
        repo.attach_download(&id, DownloadId(7)).unwrap();
        repo.attach_download(&id, DownloadId(999)).unwrap();

        bus.handle_set_package_priority(SetPackagePriorityCommand { id, priority: 6 })
            .await
            .expect("cascade tolerates dangling member");

        let dl = dl_repo.find_by_id(DownloadId(7)).unwrap().unwrap();
        assert_eq!(dl.priority(), &Priority::new(6).unwrap());
        let cascade_count = events
            .snapshot()
            .iter()
            .filter(|e| matches!(e, DomainEvent::DownloadPrioritySet { .. }))
            .count();
        assert_eq!(cascade_count, 1, "only the existing member emits");
    }
}
