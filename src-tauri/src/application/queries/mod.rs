//! CQRS query types and handlers.
//!
//! Each query represents a read request. Queries never modify state.
//! Handler implementations live in submodules and add methods to `QueryBus`.

mod count_by_state;
mod get_download_detail;
mod get_downloads;
mod list_plugins;

use crate::domain::model::download::DownloadId;
use crate::domain::model::views::{DownloadFilter, SortOrder};
use crate::domain::ports::driving::Query;

#[derive(Debug)]
pub struct GetDownloadsQuery {
    pub filter: Option<DownloadFilter>,
    pub sort: Option<SortOrder>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}
impl Query for GetDownloadsQuery {}

#[derive(Debug)]
pub struct GetDownloadDetailQuery {
    pub id: DownloadId,
}
impl Query for GetDownloadDetailQuery {}

// Handler: task 23 (history view)
#[derive(Debug)]
#[expect(dead_code)]
pub struct GetHistoryQuery {
    pub limit: usize,
    pub offset: Option<usize>,
}
impl Query for GetHistoryQuery {}

// Handler: task 23 (statistics view)
#[derive(Debug)]
#[expect(dead_code)]
pub struct GetStatsQuery;
impl Query for GetStatsQuery {}

#[derive(Debug)]
pub struct ListPluginsQuery;
impl Query for ListPluginsQuery {}

#[derive(Debug)]
pub struct CountDownloadsByStateQuery;
impl Query for CountDownloadsByStateQuery {}
