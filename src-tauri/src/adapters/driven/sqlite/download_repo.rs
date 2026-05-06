//! SQLite implementation of the `DownloadRepository` (CQRS write side).

use sea_orm::ActiveValue::Set;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};

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

    fn save_internal(
        &self,
        download: &Download,
        error_message: Option<&str>,
        update_error_message: bool,
    ) -> Result<(), DomainError> {
        block_on(async {
            persist_download(&self.db, download, error_message, update_error_message).await
        })
    }
}

async fn persist_download<C: ConnectionTrait>(
    conn: &C,
    download: &Download,
    error_message: Option<&str>,
    update_error_message: bool,
) -> Result<(), DomainError> {
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
    active_model.error_message = Set(error_message.map(str::to_string));

    let mut on_conflict = sea_orm::sea_query::OnConflict::column(download::Column::Id);
    on_conflict
        // SpeedBytesPerSec excluded: it's a runtime value
        // written by the download engine, not the write repo.
        // DownloadedBytes excluded from update_columns: we use a
        // MAX expression below so that progress_bridge writes
        // (which may race with state-transition saves) are never
        // regressed back to a stale lower value.
        // CurrentMirrorIndex excluded: progress_bridge owns this
        // column via MirrorSwitched / DownloadFailed events. A
        // generic save() carrying a stale in-memory cursor would
        // race with the event-driven write and overwrite a fresh
        // failover position with the cursor at the time the
        // aggregate was loaded.
        .update_columns([
            download::Column::Url,
            download::Column::FileName,
            download::Column::State,
            download::Column::Priority,
            download::Column::QueuePosition,
            download::Column::TotalBytes,
            download::Column::RetryCount,
            download::Column::MaxRetries,
            download::Column::SegmentsCount,
            download::Column::ChecksumExpected,
            download::Column::ChecksumComputed,
            download::Column::ChecksumAlgorithm,
            download::Column::SourceHostname,
            download::Column::Protocol,
            download::Column::ResumeSupported,
            download::Column::ModuleName,
            download::Column::AccountId,
            download::Column::DestinationPath,
            download::Column::MirrorsJson,
        ]);
    if update_error_message {
        on_conflict.update_column(download::Column::ErrorMessage);
    }

    download::Entity::insert(active_model)
        .on_conflict(
            on_conflict
                .value(
                    download::Column::CreatedAt,
                    Expr::cust(
                        "CASE WHEN created_at > 0 THEN created_at ELSE excluded.created_at END",
                    ),
                )
                .value(download::Column::UpdatedAt, now as i64)
                // Keep the larger of the two values so that a
                // state-transition save (which may carry a stale 0)
                // never overwrites bytes already written by
                // progress_bridge's update_download_progress().
                .value(
                    download::Column::DownloadedBytes,
                    Expr::cust(
                        "MAX(excluded.downloaded_bytes, COALESCE(downloads.downloaded_bytes, 0))",
                    ),
                )
                .to_owned(),
        )
        .exec(conn)
        .await
        .map_err(map_db_err)?;

    Ok(())
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
        if download.state() == DownloadState::Error {
            self.save_internal(download, None, false)
        } else {
            self.save_internal(download, None, true)
        }
    }

    fn save_failed(&self, download: &Download, error_message: &str) -> Result<(), DomainError> {
        self.save_internal(download, Some(error_message), true)
    }

    fn save_batch(&self, downloads: &[Download]) -> Result<(), DomainError> {
        if downloads.is_empty() {
            return Ok(());
        }
        block_on(async {
            let txn = self.db.begin().await.map_err(map_db_err)?;
            for download in downloads {
                let update_error_message = download.state() != DownloadState::Error;
                persist_download(&txn, download, None, update_error_message).await?;
            }
            txn.commit().await.map_err(map_db_err)?;
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
            // Deterministic order so callers (e.g. queue_position
            // tie-breaker in handle_reorder_queue) get a stable iteration
            // sequence across runs.
            let models = download::Entity::find()
                .filter(download::Column::State.eq(state.to_string()))
                .order_by_asc(download::Column::QueuePosition)
                .order_by_asc(download::Column::CreatedAt)
                .order_by_asc(download::Column::Id)
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
    use sea_orm::{ActiveModelTrait, EntityTrait};
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
    async fn test_save_round_trips_mirrors_with_priority_and_country() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqliteDownloadRepo::new(db);

        let mirror_a = crate::domain::model::Mirror::new(
            Url::new("https://a.example.com/file.zip").unwrap(),
            80,
            Some("US".to_string()),
        )
        .unwrap();
        let mirror_b = crate::domain::model::Mirror::new(
            Url::new("https://b.example.com/file.zip").unwrap(),
            40,
            None,
        )
        .unwrap();

        let download = make_download(42).with_mirrors(vec![mirror_a, mirror_b]);
        repo.save(&download).expect("save with mirrors");

        let reloaded = repo
            .find_by_id(DownloadId(42))
            .expect("find_by_id")
            .expect("download exists");
        let mirrors = reloaded.mirrors();
        assert_eq!(mirrors.len(), 2, "both mirrors round-tripped");
        assert_eq!(mirrors[0].priority(), 80, "highest priority first");
        assert_eq!(mirrors[0].country(), Some("US"));
        assert_eq!(mirrors[0].url().host(), "a.example.com");
        assert_eq!(mirrors[1].priority(), 40);
        assert!(mirrors[1].country().is_none());
        assert_eq!(reloaded.current_mirror_index(), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_save_round_trips_current_mirror_index_after_advance() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqliteDownloadRepo::new(db);

        let mirrors = vec![
            crate::domain::model::Mirror::new(
                Url::new("https://m1.example.com/f").unwrap(),
                90,
                None,
            )
            .unwrap(),
            crate::domain::model::Mirror::new(
                Url::new("https://m2.example.com/f").unwrap(),
                50,
                None,
            )
            .unwrap(),
        ];
        let mut download = make_download(43).with_mirrors(mirrors);
        download.advance_mirror().expect("advance to slot 1");
        repo.save(&download).expect("save advanced");

        let reloaded = repo
            .find_by_id(DownloadId(43))
            .expect("find")
            .expect("exists");
        assert_eq!(reloaded.current_mirror_index(), 1);
        assert_eq!(reloaded.active_url().host(), "m2.example.com");
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
    async fn test_save_failed_persists_error_message() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqliteDownloadRepo::new(db.clone());
        let mut download = make_download(1);

        download.start().expect("Queued -> Downloading");
        download
            .fail("certificate has expired".to_string())
            .expect("Downloading -> Error");

        repo.save_failed(&download, "certificate has expired")
            .expect("save failed state");

        let row = download::Entity::find_by_id(1)
            .one(&db)
            .await
            .expect("query row")
            .expect("row should exist");

        assert_eq!(row.state, "Error");
        assert_eq!(
            row.error_message.as_deref(),
            Some("certificate has expired")
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_save_clears_error_message_when_leaving_error_state() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqliteDownloadRepo::new(db.clone());
        let mut download = make_download(2);

        download.start().expect("Queued -> Downloading");
        download
            .fail("tls handshake failed".to_string())
            .expect("Downloading -> Error");
        repo.save_failed(&download, "tls handshake failed")
            .expect("save failed state");

        download.retry_manually().expect("Error -> Retry");
        repo.save(&download).expect("save retry state");

        let row = download::Entity::find_by_id(2)
            .one(&db)
            .await
            .expect("query row")
            .expect("row should exist");

        assert_eq!(row.state, "Retry");
        assert_eq!(row.error_message, None);
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
            queue_position: Set(0),
            total_bytes: Set(None),
            downloaded_bytes: Set(0),
            speed_bytes_per_sec: Set(0),
            retry_count: Set(0),
            max_retries: Set(5),
            segments_count: Set(1),
            checksum_expected: Set(None),
            checksum_computed: Set(None),
            checksum_algorithm: Set(None),
            source_hostname: Set("example.com".to_string()),
            protocol: Set("https".to_string()),
            resume_supported: Set(0),
            module_name: Set(None),
            account_id: Set(None),
            destination_path: Set("/tmp".to_string()),
            error_message: Set(None),
            mirrors_json: Set(None),
            current_mirror_index: Set(0),
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_by_state_returns_deterministic_order() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqliteDownloadRepo::new(db);

        // Saving three downloads with the same queue_position twice in
        // different sequences must yield identical iteration orders so
        // the reorder tie-breaker is stable across processes.
        let d3 = make_download(3).with_queue_position(0);
        let d1 = make_download(1).with_queue_position(0);
        let d2 = make_download(2).with_queue_position(0);
        repo.save(&d3).expect("save d3");
        std::thread::sleep(Duration::from_millis(2));
        repo.save(&d1).expect("save d1");
        std::thread::sleep(Duration::from_millis(2));
        repo.save(&d2).expect("save d2");

        let first = repo
            .find_by_state(DownloadState::Queued)
            .expect("find_by_state Queued");
        let second = repo
            .find_by_state(DownloadState::Queued)
            .expect("find_by_state Queued");
        let ids_first: Vec<u64> = first.iter().map(|d| d.id().0).collect();
        let ids_second: Vec<u64> = second.iter().map(|d| d.id().0).collect();
        assert_eq!(
            ids_first, ids_second,
            "two consecutive find_by_state calls must return the same order"
        );
        assert_eq!(ids_first.len(), 3);

        // queue_position is the primary sort key, so a smaller value
        // must come before the default 0 set regardless of created_at.
        let d4 = make_download(4).with_queue_position(-5);
        repo.save(&d4).expect("save d4");
        let queued = repo
            .find_by_state(DownloadState::Queued)
            .expect("find_by_state Queued");
        assert_eq!(
            queued.first().map(|d| d.id().0),
            Some(4),
            "queue_position is the primary sort key"
        );
    }
}
