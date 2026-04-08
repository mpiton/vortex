use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect,
};

use crate::domain::error::DomainError;
use crate::domain::model::download::DownloadId;
use crate::domain::model::views::HistoryEntry;
use crate::domain::ports::driven::history_repository::HistoryRepository;

use super::entities::history;
use super::util::{block_on, map_db_err, safe_u64};

fn model_to_entry(m: history::Model) -> HistoryEntry {
    HistoryEntry {
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

    fn make_entry(download_id: u64, completed_at: u64) -> HistoryEntry {
        HistoryEntry {
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
}
