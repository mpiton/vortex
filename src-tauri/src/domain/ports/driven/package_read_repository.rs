//! Read repository for the `Package` aggregate (CQRS read side).
//!
//! Returns flattened, display-ready DTOs produced by SQL aggregations
//! (`COUNT`, `SUM`, `AVG` on the `downloads.package_id` foreign key) so
//! the UI does not have to fetch every member download to render package
//! statistics. Never exposes mutation methods — the write port lives in
//! [`crate::domain::ports::driven::PackageRepository`].

use crate::domain::error::DomainError;
use crate::domain::model::package::PackageId;
use crate::domain::model::views::{DownloadView, PackageFilter, PackageView};

/// Reads package data as pre-computed views for the UI.
///
/// **CQRS invariant:** this trait intentionally has no `save()` method.
pub trait PackageReadRepository: Send + Sync {
    /// List packages matching the optional filter, ordered by
    /// `created_at` ascending then `id` ascending so the result is
    /// deterministic across calls.
    ///
    /// `filter.name_q` is a case-insensitive substring match against the
    /// stored `packages.name`. `filter.source_type` is an exact match
    /// against the lowercase wire form. Multiple fields AND together.
    fn find_packages(&self, filter: Option<PackageFilter>)
    -> Result<Vec<PackageView>, DomainError>;

    /// Fetch the aggregated view for a single package.
    ///
    /// Returns `Ok(None)` when no row matches — error variants are
    /// reserved for storage / data-shape problems.
    fn find_package_by_id(&self, id: &PackageId) -> Result<Option<PackageView>, DomainError>;

    /// Fetch every download currently attached to the given package as
    /// `DownloadView` rows, ordered by `queue_position` ascending then
    /// `id` ascending. Returns an empty vector when the package has no
    /// members or does not exist.
    fn find_package_downloads(&self, id: &PackageId) -> Result<Vec<DownloadView>, DomainError>;

    /// Fetch the aggregated view for the package whose `external_id`
    /// matches the given key. Returns `Ok(None)` when no row matches.
    fn find_package_by_external_id(
        &self,
        external_id: &str,
    ) -> Result<Option<PackageView>, DomainError>;
}
