//! Write repository for the `Package` aggregate (CQRS write side).
//!
//! `Package` aggregates a logical group of downloads (manual grouping,
//! auto-grouped playlist, container import, multi-part archive, …).
//! Persistence stores the package row plus the inverse foreign key on
//! `downloads.package_id` so deleting a package detaches its members
//! without losing the download history.

use crate::domain::error::DomainError;
use crate::domain::model::download::DownloadId;
use crate::domain::model::package::{Package, PackageId};

/// Persists and retrieves `Package` aggregates.
pub trait PackageRepository: Send + Sync {
    /// Look up a package by its identifier. Returns `None` when no row
    /// matches (and not an error).
    fn find_by_id(&self, id: &PackageId) -> Result<Option<Package>, DomainError>;

    /// Look up a package by its `external_id` natural key (playlist id,
    /// container hash…). Used by the auto-grouper to deduplicate packages
    /// when the same source is resolved twice. Returns the first match
    /// ordered by `created_at` ascending so reuses are deterministic.
    /// Returns `None` when no row matches.
    fn find_by_external_id(&self, external_id: &str) -> Result<Option<Package>, DomainError>;

    /// Insert or update a package. Implementations upsert by primary key
    /// and must preserve `created_at` across subsequent saves so list
    /// ordering stays stable.
    fn save(&self, package: &Package) -> Result<(), DomainError>;

    /// Every persisted package, ordered by `created_at` ascending then
    /// `id` ascending for a stable, deterministic order.
    fn list(&self) -> Result<Vec<Package>, DomainError>;

    /// Delete a package by id. No-op when the row is missing. Member
    /// downloads keep existing — `downloads.package_id` is reset to
    /// `NULL` by the FK's `ON DELETE SET NULL` clause.
    fn delete(&self, id: &PackageId) -> Result<(), DomainError>;

    /// Return the ids of every download currently attached to the given
    /// package, ordered by `queue_position` ascending so the caller can
    /// surface them in scheduling order. Returns an empty vector when
    /// no download references the package.
    fn list_downloads(&self, id: &PackageId) -> Result<Vec<DownloadId>, DomainError>;

    /// Set `downloads.package_id = package_id` for the given download.
    /// Idempotent — re-attaching a download already in the package is a
    /// no-op. Implementations must surface a [`DomainError::NotFound`]
    /// when the download row does not exist so handlers can surface a
    /// clean validation error to the IPC layer.
    fn attach_download(
        &self,
        package_id: &PackageId,
        download_id: DownloadId,
    ) -> Result<(), DomainError>;

    /// Set `downloads.package_id = NULL` for the given download. Idempotent
    /// — succeeds silently when the row is missing or already detached.
    fn detach_download(&self, download_id: DownloadId) -> Result<(), DomainError>;

    /// Return the package id currently owning the given download (FK
    /// `downloads.package_id`). Returns `Ok(None)` when the download is
    /// loose, when its row is missing, or when the download row predates
    /// the package_id column — callers must treat all three as "no
    /// owning package" so membership checks stay decoupled from row
    /// existence checks.
    fn find_package_of_download(
        &self,
        download_id: DownloadId,
    ) -> Result<Option<PackageId>, DomainError>;
}
