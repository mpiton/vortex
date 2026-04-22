use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, Condition, DatabaseConnection, EntityTrait,
    Order, QueryFilter, QueryOrder, QuerySelect,
};

use crate::domain::error::DomainError;
use crate::domain::model::download::DownloadId;
use crate::domain::model::views::{
    HistoryEntry, HistoryFilter, HistorySort, HistorySortField, SortDirection,
};
use crate::domain::ports::driven::history_repository::HistoryRepository;

use super::entities::history;
use super::util::{block_on, map_db_err, safe_u64};

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
        let mut condition = Condition::all();
        if let Some(f) = filter {
            if let Some(from) = f.date_from {
                condition = condition.add(history::Column::CompletedAt.gte(from as i64));
            }
            if let Some(to) = f.date_to {
                condition = condition.add(history::Column::CompletedAt.lte(to as i64));
            }
            if let Some(host) = f.hostname.filter(|h| !h.is_empty()) {
                let like = format!("%{host}%");
                condition = condition.add(history::Column::Url.like(&like));
            }
        }

        let (column, order) = sort
            .map(sort_to_order)
            .unwrap_or((history::Column::CompletedAt, Order::Desc));

        let mut query = history::Entity::find()
            .filter(condition)
            .order_by(column, order);
        if let Some(lim) = limit {
            query = query.limit(lim as u64);
        }
        if let Some(off) = offset {
            query = query.offset(off as u64);
        }

        let models = block_on(query.all(&self.db)).map_err(map_db_err)?;
        Ok(models.into_iter().map(model_to_entry).collect())
    }

    fn search(&self, query: &str) -> Result<Vec<HistoryEntry>, DomainError> {
        let like = format!("%{}%", query.replace('%', r"\%"));
        let condition = Condition::any()
            .add(history::Column::FileName.like(&like))
            .add(history::Column::Url.like(&like))
            .add(history::Column::DestinationPath.like(&like));
        let models = block_on(
            history::Entity::find()
                .filter(condition)
                .order_by_desc(history::Column::CompletedAt)
                .all(&self.db),
        )
        .map_err(map_db_err)?;
        Ok(models.into_iter().map(model_to_entry).collect())
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
