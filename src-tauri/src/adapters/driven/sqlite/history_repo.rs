use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, Condition, DatabaseConnection, EntityTrait,
    Order, QueryFilter, QueryOrder, QuerySelect,
};

use crate::domain::error::DomainError;
use crate::domain::model::download::DownloadId;
use crate::domain::model::views::{
    HistoryEntry, HistoryFilter, HistorySort, HistorySortField, SortDirection,
};
use crate::domain::ports::driven::history_repository::{
    HistoryRepository, MAX_HISTORY_PAGE_SIZE, MAX_HISTORY_SEARCH_RESULTS,
};

use super::entities::history;
use super::util::{block_on, map_db_err, safe_u64};

/// Extract the host component (without userinfo, port, path, query or fragment)
/// from an absolute URL. Returns `None` when the input does not contain a
/// scheme separator.
///
/// Uses a small state machine rather than pulling the `url` crate: history
/// entries already store the URL the download engine handed us, which is
/// guaranteed to start with `scheme://`.
fn extract_host(url: &str) -> Option<&str> {
    let (_, after_scheme) = url.split_once("://")?;
    let authority_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let authority = &after_scheme[..authority_end];
    let host_with_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = host_with_port
        .split_once(':')
        .map_or(host_with_port, |(h, _)| h);
    if host.is_empty() { None } else { Some(host) }
}

fn matches_hostname_filter(entry_url: &str, wanted: &str) -> bool {
    let wanted = wanted.to_ascii_lowercase();
    match extract_host(entry_url) {
        Some(host) => host.to_ascii_lowercase() == wanted,
        None => false,
    }
}

fn matches_search(entry: &HistoryEntry, needle_lower: &str) -> bool {
    entry.file_name.to_lowercase().contains(needle_lower)
        || entry.url.to_lowercase().contains(needle_lower)
        || entry.destination_path.to_lowercase().contains(needle_lower)
}

fn model_to_entry(m: history::Model) -> HistoryEntry {
    HistoryEntry {
        id: safe_u64(m.id),
        download_id: DownloadId(safe_u64(m.download_id)),
        file_name: m.file_name,
        url: m.url,
        total_bytes: safe_u64(m.total_bytes),
        completed_at: safe_u64(m.completed_at),
        duration_seconds: safe_u64(m.duration_seconds),
        avg_speed: safe_u64(m.avg_speed),
        destination_path: m.destination_path,
    }
}

fn sort_to_order(sort: HistorySort) -> (history::Column, Order) {
    let column = match sort.field {
        HistorySortField::CompletedAt => history::Column::CompletedAt,
        HistorySortField::FileName => history::Column::FileName,
        HistorySortField::TotalBytes => history::Column::TotalBytes,
        HistorySortField::DurationSeconds => history::Column::DurationSeconds,
    };
    let order = match sort.direction {
        SortDirection::Ascending => Order::Asc,
        SortDirection::Descending => Order::Desc,
    };
    (column, order)
}

pub struct SqliteHistoryRepo {
    db: DatabaseConnection,
}

impl SqliteHistoryRepo {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }
}

impl HistoryRepository for SqliteHistoryRepo {
    fn record(&self, entry: &HistoryEntry) -> Result<(), DomainError> {
        let active = history::ActiveModel {
            id: sea_orm::ActiveValue::NotSet,
            download_id: Set(entry.download_id.0 as i64),
            file_name: Set(entry.file_name.clone()),
            url: Set(entry.url.clone()),
            total_bytes: Set(entry.total_bytes as i64),
            completed_at: Set(entry.completed_at as i64),
            duration_seconds: Set(entry.duration_seconds as i64),
            avg_speed: Set(entry.avg_speed as i64),
            destination_path: Set(entry.destination_path.clone()),
        };
        block_on(active.insert(&self.db)).map_err(map_db_err)?;
        Ok(())
    }

    fn find_recent(&self, limit: usize) -> Result<Vec<HistoryEntry>, DomainError> {
        let models = block_on(
            history::Entity::find()
                .order_by_desc(history::Column::CompletedAt)
                .limit(Some(limit as u64))
                .all(&self.db),
        )
        .map_err(map_db_err)?;
        Ok(models.into_iter().map(model_to_entry).collect())
    }

    fn find_by_download(&self, id: DownloadId) -> Result<Vec<HistoryEntry>, DomainError> {
        let models = block_on(
            history::Entity::find()
                .filter(history::Column::DownloadId.eq(id.0 as i64))
                .order_by_desc(history::Column::CompletedAt)
                .all(&self.db),
        )
        .map_err(map_db_err)?;
        Ok(models.into_iter().map(model_to_entry).collect())
    }

    fn list(
        &self,
        filter: Option<HistoryFilter>,
        sort: Option<HistorySort>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<HistoryEntry>, DomainError> {
        // Only date bounds go through SQL — hostname matching has to compare
        // against the parsed URL host, which sqlite cannot do without a
        // per-entry helper. Doing it in Rust also sidesteps LIKE wildcard
        // escaping for user-supplied input.
        let mut condition = Condition::all();
        let mut hostname_filter: Option<String> = None;
        if let Some(f) = filter {
            if let Some(from) = f.date_from {
                condition = condition.add(history::Column::CompletedAt.gte(from as i64));
            }
            if let Some(to) = f.date_to {
                condition = condition.add(history::Column::CompletedAt.lte(to as i64));
            }
            hostname_filter = f.hostname.filter(|h| !h.trim().is_empty());
        }

        let (column, order) = sort
            .map(sort_to_order)
            .unwrap_or((history::Column::CompletedAt, Order::Desc));

        let effective_limit = limit
            .unwrap_or(MAX_HISTORY_PAGE_SIZE)
            .min(MAX_HISTORY_PAGE_SIZE);
        let effective_offset = offset.unwrap_or(0);

        let mut query = history::Entity::find()
            .filter(condition)
            .order_by(column, order);

        // When a hostname filter is active we have to post-filter in Rust, so
        // push offset/limit into that step and fetch up to the server cap.
        if hostname_filter.is_some() {
            query = query.limit(MAX_HISTORY_PAGE_SIZE as u64);
        } else {
            query = query.limit(effective_limit as u64);
            if effective_offset > 0 {
                query = query.offset(effective_offset as u64);
            }
        }

        let models = block_on(query.all(&self.db)).map_err(map_db_err)?;
        let entries = models.into_iter().map(model_to_entry);

        let filtered: Vec<HistoryEntry> = match hostname_filter {
            Some(host) => entries
                .filter(|e| matches_hostname_filter(&e.url, &host))
                .skip(effective_offset)
                .take(effective_limit)
                .collect(),
            None => entries.collect(),
        };
        Ok(filtered)
    }

    fn search(&self, query: &str) -> Result<Vec<HistoryEntry>, DomainError> {
        // Full-text search needs to be case-insensitive across arbitrary user
        // input, so we fetch the most recent rows up to a server cap and
        // filter in Rust. Avoids every LIKE-wildcard/ESCAPE edge case.
        let models = block_on(
            history::Entity::find()
                .order_by_desc(history::Column::CompletedAt)
                .limit(MAX_HISTORY_SEARCH_RESULTS as u64)
                .all(&self.db),
        )
        .map_err(map_db_err)?;
        let needle = query.to_lowercase();
        if needle.is_empty() {
            return Ok(Vec::new());
        }
        Ok(models
            .into_iter()
            .map(model_to_entry)
            .filter(|entry| matches_search(entry, &needle))
            .collect())
    }

    fn find_by_id(&self, id: u64) -> Result<Option<HistoryEntry>, DomainError> {
        let model =
            block_on(history::Entity::find_by_id(id as i64).one(&self.db)).map_err(map_db_err)?;
        Ok(model.map(model_to_entry))
    }

    fn delete_by_id(&self, id: u64) -> Result<bool, DomainError> {
        let result = block_on(history::Entity::delete_by_id(id as i64).exec(&self.db))
            .map_err(map_db_err)?;
        Ok(result.rows_affected > 0)
    }

    fn delete_all(&self) -> Result<u64, DomainError> {
        let result = block_on(history::Entity::delete_many().exec(&self.db)).map_err(map_db_err)?;
        Ok(result.rows_affected)
    }

    fn delete_older_than(&self, before_timestamp: u64) -> Result<u64, DomainError> {
        let result = block_on(
            history::Entity::delete_many()
                .filter(history::Column::CompletedAt.lt(before_timestamp as i64))
                .exec(&self.db),
        )
        .map_err(map_db_err)?;
        Ok(result.rows_affected)
    }
}

#[cfg(test)]
mod tests {
    use super::super::connection::setup_test_db;
    use super::*;
    use crate::domain::model::views::HistorySortField;

    fn make_entry(download_id: u64, completed_at: u64) -> HistoryEntry {
        HistoryEntry {
            id: 0,
            download_id: DownloadId(download_id),
            file_name: format!("file_{download_id}.zip"),
            url: format!("https://example.com/file_{download_id}.zip"),
            total_bytes: 1024 * download_id,
            completed_at,
            duration_seconds: 60,
            avg_speed: 1024,
            destination_path: format!("/tmp/file_{download_id}.zip"),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_record_and_find_recent() {
        let db = setup_test_db().await.expect("failed to setup test db");
        let repo = SqliteHistoryRepo::new(db);

        repo.record(&make_entry(1, 1000)).expect("record 1");
        repo.record(&make_entry(2, 2000)).expect("record 2");
        repo.record(&make_entry(3, 3000)).expect("record 3");

        let recent = repo.find_recent(2).expect("find_recent");
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].download_id, DownloadId(3));
        assert_eq!(recent[1].download_id, DownloadId(2));
        assert!(recent[0].id > 0, "record assigns a primary key");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_find_by_download() {
        let db = setup_test_db().await.expect("failed to setup test db");
        let repo = SqliteHistoryRepo::new(db);

        repo.record(&make_entry(10, 1000)).expect("record 10");
        repo.record(&make_entry(20, 2000)).expect("record 20");
        repo.record(&make_entry(10, 3000)).expect("record 10 again");

        let entries = repo
            .find_by_download(DownloadId(10))
            .expect("find_by_download");
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|e| e.download_id == DownloadId(10)));
        assert_eq!(entries[0].completed_at, 3000);
        assert_eq!(entries[1].completed_at, 1000);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_delete_older_than() {
        let db = setup_test_db().await.expect("failed to setup test db");
        let repo = SqliteHistoryRepo::new(db);

        repo.record(&make_entry(1, 1000)).expect("record 1");
        repo.record(&make_entry(2, 2000)).expect("record 2");
        repo.record(&make_entry(3, 3000)).expect("record 3");

        let deleted = repo.delete_older_than(2500).expect("delete_older_than");
        assert_eq!(deleted, 2);

        let remaining = repo.find_recent(10).expect("find_recent");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].download_id, DownloadId(3));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_find_recent_empty() {
        let db = setup_test_db().await.expect("failed to setup test db");
        let repo = SqliteHistoryRepo::new(db);

        let recent = repo.find_recent(10).expect("find_recent");
        assert!(recent.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_list_filters_date_range() {
        let db = setup_test_db().await.expect("failed to setup test db");
        let repo = SqliteHistoryRepo::new(db);
        repo.record(&make_entry(1, 1000)).unwrap();
        repo.record(&make_entry(2, 2000)).unwrap();
        repo.record(&make_entry(3, 3000)).unwrap();

        let filter = HistoryFilter {
            date_from: Some(1500),
            date_to: Some(2500),
            hostname: None,
        };
        let results = repo.list(Some(filter), None, None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].completed_at, 2000);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_list_paginates() {
        let db = setup_test_db().await.expect("failed to setup test db");
        let repo = SqliteHistoryRepo::new(db);
        for i in 1..=5 {
            repo.record(&make_entry(i, i * 1000)).unwrap();
        }

        let page1 = repo.list(None, None, Some(2), Some(0)).unwrap();
        assert_eq!(page1.len(), 2);
        assert_eq!(page1[0].completed_at, 5000);

        let page2 = repo.list(None, None, Some(2), Some(2)).unwrap();
        assert_eq!(page2.len(), 2);
        assert_eq!(page2[0].completed_at, 3000);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_list_sorts_by_file_name_ascending() {
        let db = setup_test_db().await.expect("failed to setup test db");
        let repo = SqliteHistoryRepo::new(db);
        repo.record(&make_entry(3, 1000)).unwrap();
        repo.record(&make_entry(1, 2000)).unwrap();
        repo.record(&make_entry(2, 3000)).unwrap();

        let sort = HistorySort {
            field: HistorySortField::FileName,
            direction: SortDirection::Ascending,
        };
        let results = repo.list(None, Some(sort), None, None).unwrap();
        assert_eq!(results[0].file_name, "file_1.zip");
        assert_eq!(results[1].file_name, "file_2.zip");
        assert_eq!(results[2].file_name, "file_3.zip");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_search_matches_filename_and_url() {
        let db = setup_test_db().await.expect("failed to setup test db");
        let repo = SqliteHistoryRepo::new(db);
        repo.record(&make_entry(1, 1000)).unwrap();
        let mut custom = make_entry(2, 2000);
        custom.file_name = "alpha-movie.mkv".into();
        custom.url = "https://custom.test/alpha-movie.mkv".into();
        custom.destination_path = "/data/alpha-movie.mkv".into();
        repo.record(&custom).unwrap();

        let hits = repo.search("alpha").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].file_name, "alpha-movie.mkv");

        let url_hits = repo.search("example.com").unwrap();
        assert_eq!(url_hits.len(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_find_by_id_and_delete_by_id() {
        let db = setup_test_db().await.expect("failed to setup test db");
        let repo = SqliteHistoryRepo::new(db);
        repo.record(&make_entry(1, 1000)).unwrap();
        let recent = repo.find_recent(10).unwrap();
        let id = recent[0].id;

        let found = repo.find_by_id(id).unwrap();
        assert!(found.is_some());
        let missing = repo.find_by_id(9999).unwrap();
        assert!(missing.is_none());

        let removed = repo.delete_by_id(id).unwrap();
        assert!(removed);
        let again = repo.delete_by_id(id).unwrap();
        assert!(!again);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_list_filters_by_exact_host_case_insensitive() {
        let db = setup_test_db().await.expect("failed to setup test db");
        let repo = SqliteHistoryRepo::new(db);
        let mut a = make_entry(1, 1000);
        a.url = "https://Example.COM/one.zip".into();
        repo.record(&a).unwrap();
        let mut b = make_entry(2, 2000);
        b.url = "https://other.test/two.zip".into();
        repo.record(&b).unwrap();
        let mut c = make_entry(3, 3000);
        // Path mentions "example.com" but the real host is other.test —
        // the hostname filter must NOT match this row.
        c.url = "https://other.test/?next=https://example.com/x".into();
        repo.record(&c).unwrap();

        let results = repo
            .list(
                Some(HistoryFilter {
                    date_from: None,
                    date_to: None,
                    hostname: Some("example.com".into()),
                }),
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].download_id, DownloadId(1));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_list_ignores_blank_hostname() {
        let db = setup_test_db().await.expect("failed to setup test db");
        let repo = SqliteHistoryRepo::new(db);
        repo.record(&make_entry(1, 1000)).unwrap();
        repo.record(&make_entry(2, 2000)).unwrap();

        let results = repo
            .list(
                Some(HistoryFilter {
                    date_from: None,
                    date_to: None,
                    hostname: Some("   ".into()),
                }),
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_search_treats_wildcards_as_literal() {
        let db = setup_test_db().await.expect("failed to setup test db");
        let repo = SqliteHistoryRepo::new(db);
        let mut literal = make_entry(1, 1000);
        literal.file_name = "snapshot_v2.tar.gz".into();
        repo.record(&literal).unwrap();
        let mut unrelated = make_entry(2, 2000);
        unrelated.file_name = "snapshotAv2.tar.gz".into();
        repo.record(&unrelated).unwrap();

        // `_` is a LIKE wildcard in SQLite; a naive implementation would
        // match both rows. Post-filtering in Rust treats it as a literal.
        let hits = repo.search("snapshot_v2").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].file_name, "snapshot_v2.tar.gz");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_search_is_case_insensitive() {
        let db = setup_test_db().await.expect("failed to setup test db");
        let repo = SqliteHistoryRepo::new(db);
        let mut entry = make_entry(1, 1000);
        entry.file_name = "MixedCase.Bin".into();
        repo.record(&entry).unwrap();

        let hits = repo.search("mixedcase").unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_list_clamps_limit_to_server_cap() {
        let db = setup_test_db().await.expect("failed to setup test db");
        let repo = SqliteHistoryRepo::new(db);
        for i in 1..=3 {
            repo.record(&make_entry(i, i * 1000)).unwrap();
        }

        let results = repo.list(None, None, Some(usize::MAX), Some(0)).unwrap();
        // We only seeded 3 rows so the cap doesn't reject them, but the
        // adapter must not panic or refuse unbounded `limit` values.
        assert_eq!(results.len(), 3);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_delete_all() {
        let db = setup_test_db().await.expect("failed to setup test db");
        let repo = SqliteHistoryRepo::new(db);
        for i in 1..=4 {
            repo.record(&make_entry(i, i * 1000)).unwrap();
        }

        let cleared = repo.delete_all().unwrap();
        assert_eq!(cleared, 4);
        assert!(repo.find_recent(10).unwrap().is_empty());
    }
}
