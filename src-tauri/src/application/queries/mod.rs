//! CQRS query types and handlers.
//!
//! Each query represents a read request. Queries never modify state.
//! Handler implementations live in submodules and add methods to `QueryBus`.

mod count_by_state;
mod get_account;
mod get_account_traffic;
mod get_download_detail;
mod get_downloads;
mod get_history_entry;
mod get_package;
mod get_plugin_config;
mod get_plugin_store;
mod get_stats;
mod list_accounts;
mod list_archive_contents;
mod list_history;
mod list_package_downloads;
mod list_packages;
mod list_plugins;
mod search_history;
mod top_modules;

use crate::domain::model::account::{AccountId, AccountType};
use crate::domain::model::download::DownloadId;
use crate::domain::model::package::PackageId;
use crate::domain::model::views::{
    DownloadFilter, HistoryFilter, HistorySort, PackageFilter, SortOrder, StatsPeriod,
};
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

/// Fetch aggregated statistics for a given period.
#[derive(Debug)]
pub struct GetStatsQuery {
    pub period: StatsPeriod,
}
impl Query for GetStatsQuery {}

/// Return the top N resolving modules by completed download count.
#[derive(Debug)]
pub struct TopModulesQuery {
    pub limit: u32,
}
impl Query for TopModulesQuery {}

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

/// Read the schema and current values for a single plugin's
/// configuration. Powers the dynamic UI form rendered in the plugin
/// row's "Configure" dialog.
#[derive(Debug)]
pub struct GetPluginConfigQuery {
    pub plugin_name: String,
}
impl Query for GetPluginConfigQuery {}

/// Filter combinable on the `ListAccountsQuery`. Each field is
/// optional; missing fields don't constrain the result. Multiple fields
/// AND together (service `"real-debrid"` AND type `Premium` AND
/// `enabled = true`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccountFilter {
    pub service_name: Option<String>,
    pub account_type: Option<AccountType>,
    pub enabled: Option<bool>,
}

/// List accounts, optionally filtered. Results are ordered by
/// `created_at` ascending — same convention as the underlying repo.
#[derive(Debug, Default)]
pub struct ListAccountsQuery {
    pub filter: Option<AccountFilter>,
}
impl Query for ListAccountsQuery {}

/// Fetch a single account by id.
#[derive(Debug)]
pub struct GetAccountQuery {
    pub id: AccountId,
}
impl Query for GetAccountQuery {}

/// Fetch the persisted traffic counters for one account.
#[derive(Debug)]
pub struct GetAccountTrafficQuery {
    pub id: AccountId,
}
impl Query for GetAccountTrafficQuery {}

/// List packages with optional filtering. Results are ordered by
/// `created_at` ascending then by `id` ascending so successive calls
/// yield a deterministic order.
#[derive(Debug, Default)]
pub struct ListPackagesQuery {
    pub filter: Option<PackageFilter>,
}
impl Query for ListPackagesQuery {}

/// Fetch a single package's aggregated read view.
#[derive(Debug)]
pub struct GetPackageQuery {
    pub id: PackageId,
}
impl Query for GetPackageQuery {}

/// Fetch the downloads attached to a package, ordered by
/// `queue_position` ascending then `id` ascending.
#[derive(Debug)]
pub struct ListPackageDownloadsQuery {
    pub id: PackageId,
}
impl Query for ListPackageDownloadsQuery {}
