//! Handler for [`GroupSplitArchivesCommand`](super::GroupSplitArchivesCommand).
//!
//! Routes the request through [`SplitArchiveGrouper`] so the same
//! idempotent natural-key logic backs both the IPC entry-point and any
//! future internal caller (e.g. the Link Grabber commit flow once it
//! learns to bundle split-archive links). The handler does NOT attach
//! downloads itself — it only ensures one [`Package`](crate::domain::model::package::Package)
//! exists per detected base name. Attaching member downloads is the
//! caller's responsibility once the resolved links have produced
//! [`DownloadId`](crate::domain::model::download::DownloadId)s.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::application::services::{SplitArchiveGroupResult, SplitArchiveGrouper};

impl CommandBus {
    pub async fn handle_group_split_archives(
        &self,
        cmd: super::GroupSplitArchivesCommand,
    ) -> Result<Vec<SplitArchiveGroupResult>, AppError> {
        let repo = self
            .package_repo_arc()
            .ok_or_else(|| AppError::Validation("package repository not configured".into()))?;
        let grouper = SplitArchiveGrouper::new(repo, self.event_bus_arc());

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        grouper.group_all(&cmd.links, now_ms)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::application::commands::GroupSplitArchivesCommand;
    use crate::application::commands::tests_support::{
        CapturingEventBus, InMemoryCredentialStore, InMemoryDownloadRepo, InMemoryPackageRepo,
        build_package_bus, bus_without_account_ports,
    };
    use crate::application::error::AppError;
    use crate::application::services::SplitArchiveLink;
    use crate::domain::ports::driven::PackageRepository;

    fn link(url: &str, filename: &str) -> SplitArchiveLink {
        SplitArchiveLink {
            url: url.to_string(),
            filename: filename.to_string(),
        }
    }

    fn ten_part_links(base: &str) -> Vec<SplitArchiveLink> {
        (1..=10)
            .map(|n| {
                let name = format!("{base}.part{:02}.rar", n);
                let url = format!("https://ex.com/{name}");
                link(&url, &name)
            })
            .collect()
    }

    #[tokio::test]
    async fn test_handle_group_split_archives_creates_one_package_per_base() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds, events, dl_repo);

        let mut links = ten_part_links("alpha");
        links.extend(ten_part_links("bravo"));

        let results = bus
            .handle_group_split_archives(GroupSplitArchivesCommand { links })
            .await
            .expect("group");

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.created));
        assert_eq!(repo.list().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_handle_group_split_archives_reuses_existing_package_on_re_resolve() {
        let repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus(repo.clone(), creds, events, dl_repo);

        let first = bus
            .handle_group_split_archives(GroupSplitArchivesCommand {
                links: ten_part_links("movie"),
            })
            .await
            .unwrap();
        let second = bus
            .handle_group_split_archives(GroupSplitArchivesCommand {
                links: ten_part_links("movie"),
            })
            .await
            .unwrap();

        assert!(first[0].created);
        assert!(!second[0].created);
        assert_eq!(first[0].package_id, second[0].package_id);
        assert_eq!(repo.list().unwrap().len(), 1, "no duplicate package");
    }

    #[tokio::test]
    async fn test_handle_group_split_archives_returns_validation_when_repo_missing() {
        let events = Arc::new(CapturingEventBus::new());
        let bus = bus_without_account_ports(events);

        let err = bus
            .handle_group_split_archives(GroupSplitArchivesCommand {
                links: ten_part_links("movie"),
            })
            .await
            .expect_err("missing repo");
        assert!(matches!(err, AppError::Validation(_)));
    }
}
