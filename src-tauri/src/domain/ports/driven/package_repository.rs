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
}
