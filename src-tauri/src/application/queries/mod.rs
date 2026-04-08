//! CQRS query types.
//!
//! Each query represents a read request. Queries never modify state.
//! Handlers will be implemented in later tasks.
#![allow(dead_code)] // All queries consumed by handlers (tasks 11-12)

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

#[derive(Debug)]
pub struct GetHistoryQuery {
    pub limit: usize,
}
impl Query for GetHistoryQuery {}

#[derive(Debug)]
pub struct GetStatsQuery;
impl Query for GetStatsQuery {}

#[derive(Debug)]
pub struct ListPluginsQuery;
impl Query for ListPluginsQuery {}

#[derive(Debug)]
pub struct CountDownloadsByStateQuery;
impl Query for CountDownloadsByStateQuery {}
