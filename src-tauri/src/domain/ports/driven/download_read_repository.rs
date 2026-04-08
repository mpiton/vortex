//! Read repository for download views (CQRS read side).
//!
//! Returns flattened, display-ready DTOs produced by optimized SQL
//! queries. Never exposes mutation methods.

use crate::domain::error::DomainError;
use crate::domain::model::download::DownloadId;
use crate::domain::model::views::{
    DownloadDetailView, DownloadFilter, DownloadView, SortOrder, StateCountMap,
};

/// Reads download data as pre-computed views for the UI.
///
/// This is the **read** repository in the CQRS pattern. It returns
/// `DownloadView` / `DownloadDetailView` DTOs from SQL joins and
/// aggregations, without reconstructing domain aggregates.
///
/// **CQRS invariant:** this trait intentionally has no `save()` method.
pub trait DownloadReadRepository: Send + Sync {
    /// List downloads with optional filtering and sorting.
    fn find_downloads(
        &self,
        filter: Option<DownloadFilter>,
        sort: Option<SortOrder>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<DownloadView>, DomainError>;

    /// Get detailed view for a single download (including segments).
    fn find_download_detail(
        &self,
        id: DownloadId,
    ) -> Result<Option<DownloadDetailView>, DomainError>;

    /// Count downloads grouped by state.
    fn count_by_state(&self) -> Result<StateCountMap, DomainError>;
}
