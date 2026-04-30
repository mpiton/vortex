//! Handler for [`GroupPlaylistsCommand`](super::GroupPlaylistsCommand).
//!
//! Routes the request through [`PlaylistGrouper`] so the same
//! idempotent natural-key logic backs both the IPC entry-point and any
//! future internal caller (e.g. the start-download flow once it learns
//! to bundle playlist links). The handler does NOT attach downloads
//! itself — it only ensures one [`Package`](crate::domain::model::package::Package)
//! exists per `playlist_id`. Attaching member downloads is the caller's
//! responsibility once the resolved links have produced
//! [`DownloadId`](crate::domain::model::download::DownloadId)s.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::application::services::{PlaylistGroupResult, PlaylistGrouper};

impl CommandBus {
    pub async fn handle_group_playlists(
        &self,
        cmd: super::GroupPlaylistsCommand,
    ) -> Result<Vec<PlaylistGroupResult>, AppError> {
        let repo = self
            .package_repo_arc()
            .ok_or_else(|| AppError::Validation("package repository not configured".into()))?;
        let grouper = PlaylistGrouper::new(repo, self.event_bus_arc());

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        grouper.group_all(&cmd.groups, now_ms)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::application::commands::GroupPlaylistsCommand;
    use crate::application::commands::tests_support::{
        CapturingEventBus, InMemoryCredentialStore, InMemoryDownloadRepo, InMemoryPackageRepo,
        build_package_bus,
    };
    use crate::application::services::PlaylistGroup;
    use crate::domain::ports::driven::PackageRepository;

    fn group(id: &str, name: &str, count: usize) -> PlaylistGroup {
        PlaylistGroup {
            playlist_id: id.into(),
            playlist_name: name.into(),
            item_count: count,
        }
    }

    #[tokio::test]
    async fn test_group_playlists_creates_one_package_per_unique_id() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds, events, dl_repo);

        let results = bus
            .handle_group_playlists(GroupPlaylistsCommand {
                groups: vec![group("PL-A", "Alpha", 1), group("PL-B", "Bravo", 2)],
            })
            .await
            .expect("group");

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.created));
        assert_eq!(repo.list().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_group_playlists_reuses_existing_package_on_re_resolve() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds, events, dl_repo);

        let first = bus
            .handle_group_playlists(GroupPlaylistsCommand {
                groups: vec![group("PL-DUP", "First", 5)],
            })
            .await
            .unwrap();
        let second = bus
            .handle_group_playlists(GroupPlaylistsCommand {
                groups: vec![group("PL-DUP", "Second", 5)],
            })
            .await
            .unwrap();

        assert!(first[0].created);
        assert!(!second[0].created);
        assert_eq!(first[0].package_id, second[0].package_id);
        assert_eq!(repo.list().unwrap().len(), 1, "no duplicate package");
    }

    #[tokio::test]
    async fn test_group_playlists_returns_validation_when_repo_missing() {
        let events = Arc::new(CapturingEventBus::new());
        let bus =
            crate::application::commands::tests_support::bus_without_account_ports(events.clone());

        let err = bus
            .handle_group_playlists(GroupPlaylistsCommand {
                groups: vec![group("PL-x", "X", 1)],
            })
            .await
            .expect_err("missing repo");
        assert!(matches!(
            err,
            crate::application::error::AppError::Validation(_)
        ));
    }
}
