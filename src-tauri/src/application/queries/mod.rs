//! CQRS query types and handlers.
//!
//! Each query represents a read request. Queries never modify state.
//! Handler implementations live in submodules and add methods to `QueryBus`.

mod count_by_state;
mod get_download_detail;
mod get_downloads;
mod get_history_entry;
mod get_plugin_store;
mod list_archive_contents;
mod list_history;
mod list_plugins;
mod search_history;

use crate::domain::model::download::DownloadId;
use crate::domain::model::views::{DownloadFilter, HistoryFilter, HistorySort, SortOrder};
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

/// List history entries with filter, sort, pagination.
#[derive(Debug)]
pub struct ListHistoryQuery {
    pub filter: Option<HistoryFilter>,
    pub sort: Option<HistorySort>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}
impl Query for ListHistoryQuery {}

/// Full-text history search (file name, URL, destination).
#[derive(Debug)]
pub struct SearchHistoryQuery {
    pub query: String,
}
impl Query for SearchHistoryQuery {}

/// Fetch a single history entry by its primary key.
#[derive(Debug)]
pub struct GetHistoryEntryQuery {
    pub id: u64,
}
impl Query for GetHistoryEntryQuery {}

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

// Handler: task 26 (archive contents listing)
#[derive(Debug)]
pub struct ListArchiveContentsQuery {
    pub file_path: String,
    pub password: Option<String>,
}
impl Query for ListArchiveContentsQuery {}
