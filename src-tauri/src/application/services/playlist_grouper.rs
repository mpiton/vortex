//! Auto-group playlist items into a [`Package`].
//!
//! When a crawler plugin (YouTube, SoundCloud) returns a list of links
//! that all share the same `playlist_id`, the Link Grabber wants to
//! create *one* [`Package`] holding every item. Re-resolving the same
//! playlist must reuse the previously-created package instead of
//! producing a duplicate (PRD-v2 §P1.11).
//!
//! The grouper is the single point of truth for that idempotency: it
//! looks up the package by its `external_id` natural key and either
//! returns the existing one or creates a new one. The caller (the
//! resolver / Link Grabber pipeline) then attaches the resolved items
//! by id once the downloads have been persisted.
//!
//! Domain-pure: no plugin loader, no IPC, no HTTP. Just `PackageRepository`
//! + `EventBus`. Tests run entirely in-memory.
//!
//! # Source kinds covered
//!
//! YouTube and SoundCloud playlists are the two crawlers that surface
//! `playlist_id` today. Both go through this service the same way —
//! the grouper only cares about the natural key, not about which
//! plugin emitted it.

use std::sync::Arc;

use uuid::Uuid;

use crate::application::error::AppError;
use crate::application::services::group_lock::acquire_grouper_lock;
use crate::domain::event::DomainEvent;
use crate::domain::model::package::{Package, PackageId, PackageSourceType};
use crate::domain::ports::driven::EventBus;
use crate::domain::ports::driven::PackageRepository;

/// One playlist seen by the resolver. The grouper turns one or more
/// `PlaylistGroup` instances into a `Package` per unique `playlist_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistGroup {
    /// Natural id of the source playlist (YouTube `PL…` id, SoundCloud
    /// `set:slug`, etc.). Used as the package's `external_id`.
    pub playlist_id: String,
    /// Human-readable name for the new package. Trimmed before use; an
    /// empty value falls back to a generic `"Playlist {playlist_id}"`
    /// label so the UI never surfaces a blank row.
    pub playlist_name: String,
    /// Number of resolved items in the playlist. Surfaced to the caller
    /// via `PlaylistGroupResult::item_count` so the Link Grabber preview
    /// can show "Will create package X with N items" before the user
    /// hits Start.
    pub item_count: usize,
}

/// Outcome of [`PlaylistGrouper::group_one`]. The caller uses
/// `package_id` to attach resolved downloads via
/// `PackageRepository::attach_download` and `created` to know whether
/// the package was just created (true) or reused (false).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistGroupResult {
    pub package_id: PackageId,
    pub package_name: String,
    pub created: bool,
    pub item_count: usize,
}

/// Stable label used when the input `playlist_name` is empty / blank.
/// Returns a generic English string rather than embedding the raw
/// `external_id`: the natural key may be a full URL or an opaque
/// canonical token (e.g. `youtube:playlist:PL…`), neither of which is
/// useful to surface in the package list. Frontend i18n still applies
/// to the banner; the persisted package name is intentionally generic.
fn fallback_name() -> String {
    "Untitled playlist".to_string()
}

pub struct PlaylistGrouper {
    repo: Arc<dyn PackageRepository>,
    event_bus: Arc<dyn EventBus>,
}

impl PlaylistGrouper {
    pub fn new(repo: Arc<dyn PackageRepository>, event_bus: Arc<dyn EventBus>) -> Self {
        Self { repo, event_bus }
    }

    /// Find or create the package for a single playlist. Idempotent on
    /// `playlist_id`: re-running with the same id yields the same
    /// `package_id` and `created = false`.
    ///
    /// The find-then-save pair is protected by a process-wide mutex so
    /// two concurrent calls for the same `playlist_id` cannot both miss
    /// the lookup and insert duplicate packages.
    pub fn group_one(
        &self,
        group: &PlaylistGroup,
        created_at_ms: u64,
    ) -> Result<PlaylistGroupResult, AppError> {
        let trimmed_id = group.playlist_id.trim();
        if trimmed_id.is_empty() {
            return Err(AppError::Validation("playlist_id must not be empty".into()));
        }

        // Hold the shared grouper lock only across the find-then-save
        // window. Releasing it before publishing keeps a slow subscriber
        // from blocking other concurrent grouping calls (and avoids the
        // re-entrant-publish deadlock risk if a subscriber ends up
        // touching a grouper itself).
        let (package_id, name, created) = {
            let _guard = acquire_grouper_lock();

            if let Some(existing) = self.repo.find_by_external_id(trimmed_id)? {
                return Ok(PlaylistGroupResult {
                    package_id: existing.id().clone(),
                    package_name: existing.name().to_string(),
                    created: false,
                    item_count: group.item_count,
                });
            }

            let trimmed_name = group.playlist_name.trim();
            let name = if trimmed_name.is_empty() {
                fallback_name()
            } else {
                trimmed_name.to_string()
            };

            let package_id = PackageId::new(Uuid::new_v4().to_string());
            let mut package = Package::new(
                package_id.clone(),
                name.clone(),
                PackageSourceType::Playlist,
                created_at_ms,
            );
            package.set_external_id(Some(trimmed_id.to_string()));

            // Save with conflict-recovery: a cross-process writer (the lock
            // above only serialises within one process) may have inserted
            // the same `external_id` between our `find_by_external_id` and
            // here, in which case the SQLite UNIQUE index makes our save
            // fail. Re-querying decides whether the failure was a race
            // (return the existing package as a reuse) or a real error.
            if let Err(save_err) = self.repo.save(&package) {
                if let Some(existing) = self.repo.find_by_external_id(trimmed_id)? {
                    return Ok(PlaylistGroupResult {
                        package_id: existing.id().clone(),
                        package_name: existing.name().to_string(),
                        created: false,
                        item_count: group.item_count,
                    });
                }
                return Err(save_err.into());
            }

            (package_id, name, true)
        };

        self.event_bus.publish(DomainEvent::PackageCreated {
            id: package_id.clone(),
            name: name.clone(),
        });

        Ok(PlaylistGroupResult {
            package_id,
            package_name: name,
            created,
            item_count: group.item_count,
        })
    }

    /// Convenience helper: process several playlists in a single pass,
    /// preserving caller-supplied order and producing one
    /// `PlaylistGroupResult` per input. Failures stop the loop early so
    /// the caller can decide how to recover; partial work is committed
    /// through the repo regardless (each `group_one` is its own
    /// transaction at the SQLite layer).
    pub fn group_all(
        &self,
        groups: &[PlaylistGroup],
        created_at_ms: u64,
    ) -> Result<Vec<PlaylistGroupResult>, AppError> {
        let mut out = Vec::with_capacity(groups.len());
        for group in groups {
            out.push(self.group_one(group, created_at_ms)?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    use crate::application::commands::tests_support::{CapturingEventBus, InMemoryPackageRepo};
    use crate::domain::ports::driven::PackageRepository;

    fn arc_repo_and_bus() -> (Arc<InMemoryPackageRepo>, Arc<CapturingEventBus>) {
        (
            Arc::new(InMemoryPackageRepo::new()),
            Arc::new(CapturingEventBus::new()),
        )
    }

    fn group(id: &str, name: &str, count: usize) -> PlaylistGroup {
        PlaylistGroup {
            playlist_id: id.to_string(),
            playlist_name: name.to_string(),
            item_count: count,
        }
    }

    #[test]
    fn test_group_one_creates_new_playlist_package() {
        let (repo, bus) = arc_repo_and_bus();
        let grouper = PlaylistGrouper::new(repo.clone() as Arc<dyn PackageRepository>, bus.clone());

        let result = grouper
            .group_one(&group("PL-yt-1", "Holiday Mix", 7), 1_700_000_000_000)
            .expect("create");

        assert!(result.created);
        assert_eq!(result.package_name, "Holiday Mix");
        assert_eq!(result.item_count, 7);

        let stored = repo
            .find_by_id(&result.package_id)
            .unwrap()
            .expect("present");
        assert_eq!(stored.source_type(), PackageSourceType::Playlist);
        assert_eq!(stored.external_id(), Some("PL-yt-1"));
        assert_eq!(stored.name(), "Holiday Mix");
        assert_eq!(stored.created_at(), 1_700_000_000_000);

        let snap = bus.snapshot();
        assert!(
            snap.iter().any(
                |e| matches!(e, DomainEvent::PackageCreated { name, .. } if name == "Holiday Mix")
            ),
            "PackageCreated event must fire on first creation"
        );
    }

    #[test]
    fn test_group_one_reuses_package_when_external_id_already_known() {
        let (repo, bus) = arc_repo_and_bus();
        let grouper = PlaylistGrouper::new(repo.clone() as Arc<dyn PackageRepository>, bus.clone());

        let first = grouper
            .group_one(&group("PL-dup", "Initial", 3), 1)
            .expect("first");
        let second = grouper
            .group_one(&group("PL-dup", "Updated label", 9), 2)
            .expect("second");

        assert!(first.created);
        assert!(!second.created, "second resolve must reuse existing");
        assert_eq!(
            first.package_id, second.package_id,
            "same playlist_id must yield same package id"
        );
        // The reused package keeps the original name; the second call's
        // `playlist_name` is informational, not a rename.
        assert_eq!(second.package_name, "Initial");
        // Only one PackageCreated event over the two calls.
        let created_events = bus
            .snapshot()
            .iter()
            .filter(|e| matches!(e, DomainEvent::PackageCreated { .. }))
            .count();
        assert_eq!(created_events, 1);
    }

    #[test]
    fn test_group_one_rejects_blank_playlist_id() {
        let (repo, bus) = arc_repo_and_bus();
        let grouper = PlaylistGrouper::new(repo.clone() as Arc<dyn PackageRepository>, bus);

        let err = grouper
            .group_one(&group("   ", "Anything", 2), 0)
            .expect_err("blank id must be rejected");
        assert!(matches!(err, AppError::Validation(_)));
        assert!(
            repo.list().unwrap().is_empty(),
            "no package should be persisted on validation error"
        );
    }

    #[test]
    fn test_group_one_uses_fallback_name_when_input_blank() {
        let (repo, bus) = arc_repo_and_bus();
        let grouper = PlaylistGrouper::new(repo.clone() as Arc<dyn PackageRepository>, bus);

        let result = grouper
            .group_one(&group("PL-no-name", "   ", 4), 0)
            .expect("create");

        assert_eq!(result.package_name, "Untitled playlist");
        let stored = repo.find_by_id(&result.package_id).unwrap().unwrap();
        assert_eq!(stored.name(), "Untitled playlist");
    }

    #[test]
    fn test_group_all_creates_one_package_per_unique_playlist_id() {
        let (repo, bus) = arc_repo_and_bus();
        let grouper = PlaylistGrouper::new(repo.clone() as Arc<dyn PackageRepository>, bus);

        let groups = vec![
            group("PL-A", "Alpha", 1),
            // Same id repeated — must NOT create a second package.
            group("PL-A", "Alpha (alt label)", 1),
            group("PL-B", "Bravo", 2),
        ];
        let results = grouper.group_all(&groups, 0).expect("group all");

        assert_eq!(results.len(), 3);
        assert!(results[0].created);
        assert!(!results[1].created, "duplicate playlist_id reuses package");
        assert_eq!(results[0].package_id, results[1].package_id);
        assert!(results[2].created);
        assert_eq!(repo.list().unwrap().len(), 2);
    }

    #[test]
    fn test_group_all_propagates_first_error_and_stops() {
        let (repo, bus) = arc_repo_and_bus();
        let grouper = PlaylistGrouper::new(repo.clone() as Arc<dyn PackageRepository>, bus);

        let groups = vec![
            group("PL-ok", "fine", 1),
            group("", "broken", 0),
            group("PL-never", "should not run", 1),
        ];
        let err = grouper.group_all(&groups, 0).expect_err("blank id stops");
        assert!(matches!(err, AppError::Validation(_)));

        // The first item still committed (each call is its own write),
        // but the third never ran.
        let stored = repo.list().unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].external_id(), Some("PL-ok"));
    }

    #[test]
    fn test_group_one_serialises_concurrent_calls_for_same_playlist_id() {
        // Spawn N threads that all hit `group_one` for the same
        // `playlist_id` at once. Without the lock, the find-then-save
        // race can produce two `PackageCreated` events and two rows
        // sharing the same `external_id`. With the lock, exactly one
        // creation must win and the rest must reuse it.
        const THREADS: usize = 16;
        let (repo, bus) = arc_repo_and_bus();
        let grouper = Arc::new(PlaylistGrouper::new(
            repo.clone() as Arc<dyn PackageRepository>,
            bus.clone(),
        ));

        let handles: Vec<_> = (0..THREADS)
            .map(|i| {
                let grouper = grouper.clone();
                std::thread::spawn(move || {
                    grouper
                        .group_one(&group("PL-race", "Race", 1), 1_700_000_000_000 + i as u64)
                        .expect("group_one")
                })
            })
            .collect();

        let results: Vec<PlaylistGroupResult> = handles
            .into_iter()
            .map(|h| h.join().expect("thread"))
            .collect();

        let created_count = results.iter().filter(|r| r.created).count();
        assert_eq!(
            created_count, 1,
            "exactly one thread must create the package"
        );
        let unique_ids: std::collections::HashSet<&PackageId> =
            results.iter().map(|r| &r.package_id).collect();
        assert_eq!(
            unique_ids.len(),
            1,
            "every call must yield the same package id"
        );

        let stored = repo.list().expect("list");
        assert_eq!(stored.len(), 1, "exactly one package row must exist");
        assert_eq!(stored[0].external_id(), Some("PL-race"));

        let created_events = bus
            .snapshot()
            .iter()
            .filter(|e| matches!(e, DomainEvent::PackageCreated { .. }))
            .count();
        assert_eq!(created_events, 1, "PackageCreated must fire exactly once");
    }

    /// Repo wrapper that simulates a cross-process race: the first
    /// `save` call rejects with a UNIQUE-style error after seeding the
    /// inner store with the "winning" package, so the next
    /// `find_by_external_id` succeeds. Subsequent saves pass through.
    struct RacingPackageRepo {
        inner: Arc<InMemoryPackageRepo>,
        winner: Mutex<Option<Package>>,
    }

    impl RacingPackageRepo {
        fn new(winner: Package) -> Arc<Self> {
            Arc::new(Self {
                inner: Arc::new(InMemoryPackageRepo::new()),
                winner: Mutex::new(Some(winner)),
            })
        }
    }

    impl PackageRepository for RacingPackageRepo {
        fn find_by_id(
            &self,
            id: &PackageId,
        ) -> Result<Option<Package>, crate::domain::error::DomainError> {
            self.inner.find_by_id(id)
        }

        fn find_by_external_id(
            &self,
            external_id: &str,
        ) -> Result<Option<Package>, crate::domain::error::DomainError> {
            self.inner.find_by_external_id(external_id)
        }

        fn save(&self, package: &Package) -> Result<(), crate::domain::error::DomainError> {
            // Simulate a cross-process winner that already inserted the
            // same `external_id`: seed the inner store, then surface a
            // UNIQUE-style error.
            if let Some(winner) = self.winner.lock().expect("winner lock").take() {
                self.inner.save(&winner)?;
                return Err(crate::domain::error::DomainError::StorageError(
                    "UNIQUE constraint failed: packages.external_id".to_string(),
                ));
            }
            self.inner.save(package)
        }

        fn list(&self) -> Result<Vec<Package>, crate::domain::error::DomainError> {
            self.inner.list()
        }

        fn delete(&self, id: &PackageId) -> Result<(), crate::domain::error::DomainError> {
            self.inner.delete(id)
        }

        fn list_downloads(
            &self,
            id: &PackageId,
        ) -> Result<
            Vec<crate::domain::model::download::DownloadId>,
            crate::domain::error::DomainError,
        > {
            self.inner.list_downloads(id)
        }

        fn attach_download(
            &self,
            package_id: &PackageId,
            download_id: crate::domain::model::download::DownloadId,
        ) -> Result<(), crate::domain::error::DomainError> {
            self.inner.attach_download(package_id, download_id)
        }

        fn detach_download(
            &self,
            download_id: crate::domain::model::download::DownloadId,
        ) -> Result<(), crate::domain::error::DomainError> {
            self.inner.detach_download(download_id)
        }

        fn find_package_of_download(
            &self,
            download_id: crate::domain::model::download::DownloadId,
        ) -> Result<Option<PackageId>, crate::domain::error::DomainError> {
            self.inner.find_package_of_download(download_id)
        }
    }

    #[test]
    fn test_group_one_recovers_when_save_loses_unique_race() {
        // Cross-process race: another writer inserts the same
        // `external_id` between our `find` and `save`. The UNIQUE index
        // (added in migration m20260430_000008) makes our save fail.
        // The grouper must re-query and surface the winner as a reuse
        // instead of bubbling the constraint error to the caller.
        let bus = Arc::new(CapturingEventBus::new());
        let mut winner = Package::new(
            PackageId::new("pkg-winner"),
            "Winner".to_string(),
            PackageSourceType::Playlist,
            500,
        );
        winner.set_external_id(Some("PL-race".to_string()));
        let repo = RacingPackageRepo::new(winner);
        let grouper = PlaylistGrouper::new(repo.clone(), bus.clone());

        let result = grouper
            .group_one(&group("PL-race", "Loser", 4), 1_000)
            .expect("conflict must be recovered, not propagated");

        assert!(
            !result.created,
            "post-conflict result must surface the winner as a reuse"
        );
        assert_eq!(result.package_id.as_str(), "pkg-winner");
        assert_eq!(result.package_name, "Winner");
        // The grouper must NOT publish a `PackageCreated` event for a
        // package it didn't actually create.
        let created_events = bus
            .snapshot()
            .iter()
            .filter(|e| matches!(e, DomainEvent::PackageCreated { .. }))
            .count();
        assert_eq!(
            created_events, 0,
            "no PackageCreated must fire when the save lost the race"
        );

        let stored = repo.list().expect("list");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].id().as_str(), "pkg-winner");
    }

    #[test]
    fn test_group_one_trims_playlist_id_for_lookup_and_storage() {
        let (repo, bus) = arc_repo_and_bus();
        let grouper = PlaylistGrouper::new(repo.clone() as Arc<dyn PackageRepository>, bus.clone());

        let first = grouper
            .group_one(&group("  PL-pad  ", "Pad", 1), 0)
            .expect("create");
        let second = grouper
            .group_one(&group("PL-pad", "Pad again", 1), 0)
            .expect("reuse");

        assert!(first.created);
        assert!(!second.created);
        assert_eq!(first.package_id, second.package_id);
        let stored = repo.find_by_id(&first.package_id).unwrap().unwrap();
        // Stored without surrounding whitespace.
        assert_eq!(stored.external_id(), Some("PL-pad"));
    }
}
