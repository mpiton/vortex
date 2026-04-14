//! SQLite implementation of the `DownloadRepository` (CQRS write side).

use sea_orm::ActiveValue::Set;
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::domain::error::DomainError;
use crate::domain::model::download::{Download, DownloadId, DownloadState};
use crate::domain::ports::driven::download_repository::DownloadRepository;

use super::entities::download;
use super::util::{
    block_on, current_timestamp_ms, infer_timestamp_ms_from_download_id, map_db_err,
};

pub struct SqliteDownloadRepo {
    db: DatabaseConnection,
}

impl SqliteDownloadRepo {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }
}

impl DownloadRepository for SqliteDownloadRepo {
    fn find_by_id(&self, id: DownloadId) -> Result<Option<Download>, DomainError> {
        block_on(async {
            let model = download::Entity::find_by_id(id.0 as i64)
                .one(&self.db)
                .await
                .map_err(map_db_err)?;

            match model {
                Some(m) => Ok(Some(m.into_domain()?)),
                None => Ok(None),
            }
        })
    }

    fn save(&self, download: &Download) -> Result<(), DomainError> {
        block_on(async {
            let mut active_model = download::ActiveModel::from_domain(download);
            let now = current_timestamp_ms();
            let created_at = if download.created_at() == 0 {
                infer_timestamp_ms_from_download_id(download.id().0 as i64).unwrap_or(now)
            } else {
                download.created_at()
            };
            let updated_at = if download.updated_at() == 0 {
                created_at
            } else {
                now.max(download.updated_at())
            };

            active_model.created_at = Set(created_at as i64);
            active_model.updated_at = Set(updated_at as i64);

            download::Entity::insert(active_model)
                .on_conflict(
                    sea_orm::sea_query::OnConflict::column(download::Column::Id)
                        // SpeedBytesPerSec excluded: it's a runtime value
                        // written by the download engine, not the write repo.
                        .update_columns([
                            download::Column::Url,
                            download::Column::FileName,
                            download::Column::State,
                            download::Column::Priority,
                            download::Column::TotalBytes,
                            download::Column::DownloadedBytes,
                            download::Column::RetryCount,
                            download::Column::MaxRetries,
                            download::Column::SegmentsCount,
                            download::Column::ChecksumExpected,
                            download::Column::SourceHostname,
                            download::Column::Protocol,
                            download::Column::ResumeSupported,
                            download::Column::ModuleName,
                            download::Column::AccountId,
                            download::Column::DestinationPath,
                        ])
                        .value(
                            download::Column::CreatedAt,
                            Expr::cust(
                                "CASE WHEN created_at > 0 THEN created_at ELSE excluded.created_at END",
                            ),
                        )
                        .value(download::Column::UpdatedAt, now as i64)
                        .to_owned(),
                )
                .exec(&self.db)
                .await
                .map_err(map_db_err)?;

            Ok(())
        })
    }

    fn delete(&self, id: DownloadId) -> Result<(), DomainError> {
        block_on(async {
            download::Entity::delete_by_id(id.0 as i64)
                .exec(&self.db)
                .await
                .map_err(map_db_err)?;

            Ok(())
        })
    }

    fn find_by_state(&self, state: DownloadState) -> Result<Vec<Download>, DomainError> {
        block_on(async {
            let models = download::Entity::find()
                .filter(download::Column::State.eq(state.to_string()))
                .all(&self.db)
                .await
                .map_err(map_db_err)?;

            models.into_iter().map(|m| m.into_domain()).collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::driven::sqlite::connection::setup_test_db;
    use crate::adapters::driven::sqlite::entities::download;
    use crate::domain::model::download::Url;
    use crate::domain::model::queue::Priority;
    use sea_orm::ActiveModelTrait;
    use std::time::Duration;

    fn make_download(id: u64) -> Download {
        let url = Url::new("https://example.com/file.zip").expect("valid url");
        Download::new(
            DownloadId(id),
            url,
            "file.zip".to_string(),
            "/tmp".to_string(),
        )
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_save_and_find_download_round_trip() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqliteDownloadRepo::new(db);

        let download = make_download(1).with_priority(Priority::new(3).expect("valid priority"));

        // Save
        let save_result = repo.save(&download);
        assert!(save_result.is_ok(), "save failed: {:?}", save_result.err());

        // Find
        let found = repo.find_by_id(DownloadId(1)).expect("find_by_id failed");
        assert!(found.is_some());
        let found = found.expect("download should exist");
        assert_eq!(found.id(), DownloadId(1));
        assert_eq!(found.file_name(), "file.zip");
        assert_eq!(found.destination_path(), "/tmp");
        assert_eq!(found.state(), DownloadState::Queued);
        assert_eq!(found.priority(), &Priority::new(3).expect("valid priority"));
        assert_eq!(found.url().as_str(), "https://example.com/file.zip");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_save_upsert_updates_existing() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqliteDownloadRepo::new(db);
        let created_at = 1_700_000_000_000_u64;
        let id = (created_at << 12) | 1;

        let download = make_download(id);
        repo.save(&download).expect("first save");
        let first = repo
            .find_by_id(DownloadId(id))
            .expect("find_by_id")
            .expect("should exist");

        // Modify and save again (upsert)
        std::thread::sleep(Duration::from_millis(2));
        let updated = Download::new(
            DownloadId(id),
            Url::new("https://example.com/updated.zip").expect("valid url"),
            "updated.zip".to_string(),
            "/downloads".to_string(),
        );
        repo.save(&updated).expect("upsert save");

        let found = repo
            .find_by_id(DownloadId(id))
            .expect("find_by_id")
            .expect("should exist");
        assert_eq!(found.file_name(), "updated.zip");
        assert_eq!(found.destination_path(), "/downloads");
        assert_eq!(found.created_at(), first.created_at());
        assert!(found.updated_at() > first.updated_at());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_save_refreshes_updated_at_for_existing_download() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqliteDownloadRepo::new(db);

        let download = make_download(1);
        repo.save(&download).expect("first save");

        let found = repo
            .find_by_id(DownloadId(1))
            .expect("find_by_id")
            .expect("should exist");
        let previous_updated_at = found.updated_at();

        std::thread::sleep(Duration::from_millis(2));
        repo.save(&found).expect("second save");

        let reloaded = repo
            .find_by_id(DownloadId(1))
            .expect("find_by_id")
            .expect("should exist");

        assert!(reloaded.updated_at() > previous_updated_at);
        assert_eq!(reloaded.created_at(), found.created_at());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_save_heals_legacy_zero_created_at_on_upsert() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqliteDownloadRepo::new(db.clone());
        let created_at = 1_700_000_000_000_u64;
        let id = ((created_at << 12) | 7) as i64;

        let legacy = download::ActiveModel {
            id: Set(id),
            url: Set("https://example.com/file.zip".to_string()),
            file_name: Set("file.zip".to_string()),
            state: Set("Queued".to_string()),
            priority: Set(5),
            total_bytes: Set(None),
            downloaded_bytes: Set(0),
            speed_bytes_per_sec: Set(0),
            retry_count: Set(0),
            max_retries: Set(5),
            segments_count: Set(1),
            checksum_expected: Set(None),
            source_hostname: Set("example.com".to_string()),
            protocol: Set("https".to_string()),
            resume_supported: Set(0),
            module_name: Set(None),
            account_id: Set(None),
            destination_path: Set("/tmp".to_string()),
            created_at: Set(0),
            updated_at: Set(0),
        };
        legacy.insert(&db).await.expect("insert legacy row");

        std::thread::sleep(Duration::from_millis(2));
        let updated = Download::new(
            DownloadId(id as u64),
            Url::new("https://example.com/updated.zip").expect("valid url"),
            "updated.zip".to_string(),
            "/downloads".to_string(),
        );
        repo.save(&updated).expect("upsert save");

        let found = repo
            .find_by_id(DownloadId(id as u64))
            .expect("find_by_id")
            .expect("should exist");
        assert_eq!(found.created_at(), created_at);
        assert!(found.updated_at() > created_at);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_by_id_not_found_returns_none() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqliteDownloadRepo::new(db);

        let result = repo.find_by_id(DownloadId(999)).expect("find_by_id");
        assert!(result.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_delete_download_removes_from_db() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqliteDownloadRepo::new(db);

        let download = make_download(1);
        repo.save(&download).expect("save");

        repo.delete(DownloadId(1)).expect("delete");

        let found = repo.find_by_id(DownloadId(1)).expect("find_by_id");
        assert!(found.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_by_state_filters_correctly() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqliteDownloadRepo::new(db);

        // Create downloads in different states
        let queued1 = make_download(1);
        let queued2 = make_download(2);
        let mut started = make_download(3);
        started.start().expect("start transition");

        repo.save(&queued1).expect("save queued1");
        repo.save(&queued2).expect("save queued2");
        repo.save(&started).expect("save started");

        let queued = repo
            .find_by_state(DownloadState::Queued)
            .expect("find_by_state Queued");
        assert_eq!(queued.len(), 2);

        let downloading = repo
            .find_by_state(DownloadState::Downloading)
            .expect("find_by_state Downloading");
        assert_eq!(downloading.len(), 1);
        assert_eq!(downloading[0].id(), DownloadId(3));

        let completed = repo
            .find_by_state(DownloadState::Completed)
            .expect("find_by_state Completed");
        assert!(completed.is_empty());
    }
}
