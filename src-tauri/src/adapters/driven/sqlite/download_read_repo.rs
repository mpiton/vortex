use std::collections::HashMap;

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect};

use crate::domain::error::DomainError;
use crate::domain::model::download::{DownloadId, DownloadState};
use crate::domain::model::views::{
    DownloadDetailView, DownloadFilter, DownloadView, SegmentView, SortDirection, SortField,
    SortOrder, StateCountMap,
};
use crate::domain::ports::driven::download_read_repository::DownloadReadRepository;

use super::entities::{download, download_segment};
use super::util::{
    MIN_PLAUSIBLE_UNIX_MS, block_on, infer_timestamp_ms_from_download_id,
    inferred_download_created_at_order_expr, map_db_err, safe_u32, safe_u64,
};

pub struct SqliteDownloadReadRepo {
    db: DatabaseConnection,
}

impl SqliteDownloadReadRepo {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }
}

fn read_created_at(model: &download::Model) -> u64 {
    let created_at = safe_u64(model.created_at);
    if created_at > 0 {
        created_at
    } else if let Some(inferred) = infer_timestamp_ms_from_download_id(model.id) {
        inferred
    } else {
        let updated_at = safe_u64(model.updated_at);
        if updated_at > 0 {
            updated_at
        } else {
            MIN_PLAUSIBLE_UNIX_MS
        }
    }
}

/// Compute progress percent rounded to one decimal place.
///
/// - `Completed` always returns 100.0 regardless of `downloaded_bytes` (the
///   last `DownloadProgress` event may lag behind the final chunk by up to 500ms).
/// - Unknown total returns 0.0.
/// - All other states: `downloaded / total * 100`, rounded to 1 dp.
fn compute_progress_percent(state: &str, downloaded: u64, total: Option<u64>) -> f64 {
    if state == "Completed" {
        return 100.0;
    }
    match total {
        Some(t) if t > 0 => ((downloaded as f64 / t as f64 * 1000.0).round()) / 10.0,
        _ => 0.0,
    }
}

fn model_to_view(
    model: &download::Model,
    segments_active: u32,
    segments_total: u32,
) -> Result<DownloadView, DomainError> {
    let total = model.total_bytes.map(safe_u64);
    let downloaded = safe_u64(model.downloaded_bytes);
    let speed = safe_u64(model.speed_bytes_per_sec);

    let progress_percent = compute_progress_percent(&model.state, downloaded, total);

    let eta_seconds = match total {
        Some(t) if speed > 0 && t > downloaded => Some((t - downloaded) / speed),
        _ => None,
    };

    let state = model.state.parse().map_err(|_| {
        DomainError::StorageError(format!("invalid download state in DB: {}", model.state))
    })?;

    Ok(DownloadView {
        id: DownloadId(safe_u64(model.id)),
        file_name: model.file_name.clone(),
        url: model.url.clone(),
        source_hostname: model.source_hostname.clone(),
        state,
        progress_percent,
        speed_bytes_per_sec: speed,
        downloaded_bytes: downloaded,
        total_bytes: total,
        eta_seconds,
        segments_active,
        segments_total,
        module_name: model.module_name.clone(),
        account_name: None, // Resolved when accounts table is implemented
        error_message: model.error_message.clone(),
        created_at: read_created_at(model),
    })
}

fn segment_model_to_view(model: &download_segment::Model) -> Result<SegmentView, DomainError> {
    let state = model.state.parse().map_err(|_| {
        DomainError::StorageError(format!("invalid segment state in DB: {}", model.state))
    })?;

    Ok(SegmentView {
        id: safe_u32(model.segment_index as i64),
        start_byte: safe_u64(model.start_byte),
        end_byte: safe_u64(model.end_byte),
        downloaded_bytes: safe_u64(model.downloaded_bytes),
        state,
    })
}

impl DownloadReadRepository for SqliteDownloadReadRepo {
    fn find_downloads(
        &self,
        filter: Option<DownloadFilter>,
        sort: Option<SortOrder>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<DownloadView>, DomainError> {
        block_on(async {
            let mut query = download::Entity::find();

            if let Some(ref f) = filter {
                if let Some(ref state) = f.state {
                    query = query.filter(download::Column::State.eq(state.to_string()));
                }
                if let Some(ref search) = f.search {
                    query = query.filter(download::Column::FileName.contains(search));
                }
                if let Some(ref host) = f.host {
                    query = query.filter(download::Column::SourceHostname.eq(host.as_str()));
                }
            }

            if let Some(ref s) = sort {
                // Progress is a derived value (downloaded/total ratio), not a stored column.
                // We approximate by sorting on downloaded_bytes — a true ratio sort would
                // need a computed SQL expression, deferred to the UI layer for now.
                query = match (s.field, s.direction) {
                    (SortField::CreatedAt, SortDirection::Ascending) => {
                        query.order_by_asc(inferred_download_created_at_order_expr())
                    }
                    (SortField::CreatedAt, SortDirection::Descending) => {
                        query.order_by_desc(inferred_download_created_at_order_expr())
                    }
                    (SortField::FileName, SortDirection::Ascending) => {
                        query.order_by_asc(download::Column::FileName)
                    }
                    (SortField::FileName, SortDirection::Descending) => {
                        query.order_by_desc(download::Column::FileName)
                    }
                    (SortField::FileSize, SortDirection::Ascending) => {
                        query.order_by_asc(download::Column::TotalBytes)
                    }
                    (SortField::FileSize, SortDirection::Descending) => {
                        query.order_by_desc(download::Column::TotalBytes)
                    }
                    (SortField::Progress, SortDirection::Ascending) => {
                        query.order_by_asc(download::Column::DownloadedBytes)
                    }
                    (SortField::Progress, SortDirection::Descending) => {
                        query.order_by_desc(download::Column::DownloadedBytes)
                    }
                    (SortField::Speed, SortDirection::Ascending) => {
                        query.order_by_asc(download::Column::SpeedBytesPerSec)
                    }
                    (SortField::Speed, SortDirection::Descending) => {
                        query.order_by_desc(download::Column::SpeedBytesPerSec)
                    }
                    (SortField::State, SortDirection::Ascending) => {
                        query.order_by_asc(download::Column::State)
                    }
                    (SortField::State, SortDirection::Descending) => {
                        query.order_by_desc(download::Column::State)
                    }
                };
            } else {
                query = query.order_by_desc(inferred_download_created_at_order_expr());
            }

            if let Some(o) = offset {
                query = query.offset(o as u64);
            }
            if let Some(l) = limit {
                query = query.limit(l as u64);
            }

            let downloads = query.all(&self.db).await.map_err(map_db_err)?;

            if downloads.is_empty() {
                return Ok(Vec::new());
            }

            let download_ids: Vec<i64> = downloads.iter().map(|d| d.id).collect();

            let all_segments = download_segment::Entity::find()
                .filter(download_segment::Column::DownloadId.is_in(download_ids))
                .all(&self.db)
                .await
                .map_err(map_db_err)?;

            let mut seg_map: HashMap<i64, (u32, u32)> = HashMap::new();
            for seg in &all_segments {
                let entry = seg_map.entry(seg.download_id).or_insert((0, 0));
                entry.1 += 1; // total
                if seg.state == "Downloading" {
                    entry.0 += 1; // active
                }
            }

            let views: Vec<DownloadView> = downloads
                .iter()
                .map(|d| {
                    let (active, total) = seg_map.get(&d.id).copied().unwrap_or((0, 0));
                    model_to_view(d, active, total)
                })
                .collect::<Result<_, _>>()?;

            Ok(views)
        })
    }

    fn find_download_detail(
        &self,
        id: DownloadId,
    ) -> Result<Option<DownloadDetailView>, DomainError> {
        block_on(async {
            let model = download::Entity::find_by_id(id.0 as i64)
                .one(&self.db)
                .await
                .map_err(map_db_err)?;

            let model = match model {
                Some(m) => m,
                None => return Ok(None),
            };

            let segments = download_segment::Entity::find()
                .filter(download_segment::Column::DownloadId.eq(model.id))
                .order_by_asc(download_segment::Column::SegmentIndex)
                .all(&self.db)
                .await
                .map_err(map_db_err)?;

            let segment_views: Vec<SegmentView> = segments
                .iter()
                .map(segment_model_to_view)
                .collect::<Result<_, _>>()?;

            let total = model.total_bytes.map(safe_u64);
            let downloaded = safe_u64(model.downloaded_bytes);
            let speed = safe_u64(model.speed_bytes_per_sec);

            let progress_percent = compute_progress_percent(&model.state, downloaded, total);

            let eta_seconds = match total {
                Some(t) if speed > 0 && t > downloaded => Some((t - downloaded) / speed),
                _ => None,
            };

            let state = model.state.parse().map_err(|_| {
                DomainError::StorageError(format!("invalid download state in DB: {}", model.state))
            })?;
            let created_at = read_created_at(&model);
            let updated_at = {
                let stored = safe_u64(model.updated_at);
                if stored > 0 { stored } else { created_at }
            };

            let detail = DownloadDetailView {
                id: DownloadId(safe_u64(model.id)),
                file_name: model.file_name.clone(),
                url: model.url.clone(),
                source_hostname: model.source_hostname.clone(),
                state,
                progress_percent,
                speed_bytes_per_sec: speed,
                downloaded_bytes: downloaded,
                total_bytes: total,
                eta_seconds,
                segments: segment_views,
                checksum_expected: model.checksum_expected.clone(),
                destination_path: model.destination_path.clone(),
                module_name: model.module_name.clone(),
                account_name: None, // Resolved when accounts table is implemented
                resume_supported: model.resume_supported != 0,
                retry_count: safe_u32(model.retry_count as i64),
                max_retries: safe_u32(model.max_retries as i64),
                created_at,
                updated_at,
            };

            Ok(Some(detail))
        })
    }

    fn count_by_state(&self) -> Result<StateCountMap, DomainError> {
        use sea_orm::{ConnectionTrait, Statement};

        block_on(async {
            let rows = self
                .db
                .query_all(Statement::from_string(
                    sea_orm::DatabaseBackend::Sqlite,
                    "SELECT state, COUNT(*) as cnt FROM downloads GROUP BY state".to_string(),
                ))
                .await
                .map_err(map_db_err)?;

            let mut result: StateCountMap = HashMap::new();
            for row in rows {
                let state_str: String = row
                    .try_get_by_index(0)
                    .map_err(|e| DomainError::StorageError(e.to_string()))?;
                let count: i64 = row
                    .try_get_by_index(1)
                    .map_err(|e| DomainError::StorageError(e.to_string()))?;

                if let Ok(state) = state_str.parse::<DownloadState>()
                    && count > 0
                {
                    result.insert(state, count as usize);
                }
            }

            Ok(result)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::driven::sqlite::download_repo::SqliteDownloadRepo;
    use crate::domain::model::download::{Download, Url};
    use crate::domain::model::segment::SegmentState;
    use crate::domain::ports::driven::download_repository::DownloadRepository;
    use sea_orm::ActiveValue::Set;
    use sea_orm::{ActiveModelTrait, DatabaseConnection};

    use super::super::connection::setup_test_db;

    async fn setup() -> DatabaseConnection {
        setup_test_db().await.expect("Failed to setup test DB")
    }

    async fn insert_download(db: &DatabaseConnection, id: i64, state: &str, file_name: &str) {
        let model = download::ActiveModel {
            id: Set(id),
            url: Set(format!("https://example.com/{file_name}")),
            file_name: Set(file_name.to_string()),
            state: Set(state.to_string()),
            priority: Set(5),
            total_bytes: Set(Some(1000)),
            downloaded_bytes: Set(500),
            speed_bytes_per_sec: Set(100),
            retry_count: Set(0),
            max_retries: Set(5),
            segments_count: Set(2),
            checksum_expected: Set(None),
            source_hostname: Set("example.com".to_string()),
            protocol: Set("https".to_string()),
            resume_supported: Set(1),
            module_name: Set(None),
            account_id: Set(None),
            destination_path: Set("/tmp/downloads".to_string()),
            error_message: Set(None),
            created_at: Set(1000 + id),
            updated_at: Set(2000 + id),
        };
        model.insert(db).await.expect("Failed to insert download");
    }

    async fn insert_segment(
        db: &DatabaseConnection,
        id: i64,
        download_id: i64,
        index: i32,
        state: &str,
    ) {
        let model = download_segment::ActiveModel {
            id: Set(id),
            download_id: Set(download_id),
            segment_index: Set(index),
            start_byte: Set(0),
            end_byte: Set(500),
            downloaded_bytes: Set(250),
            state: Set(state.to_string()),
        };
        model.insert(db).await.expect("Failed to insert segment");
    }

    // --- Unit tests for compute_progress_percent ---

    #[test]
    fn test_progress_completed_always_100() {
        // Even if downloaded_bytes < total_bytes (last progress event lagged),
        // a Completed download must show 100%.
        assert_eq!(
            compute_progress_percent("Completed", 9_000_000, Some(10_000_000)),
            100.0
        );
        assert_eq!(compute_progress_percent("Completed", 0, None), 100.0);
    }

    #[test]
    fn test_progress_rounded_to_one_decimal() {
        // 1/3 = 33.333... → rounds to 33.3
        let p = compute_progress_percent("Downloading", 1, Some(3));
        assert_eq!(p, 33.3);

        // 2/3 = 66.666... → rounds to 66.7
        let p = compute_progress_percent("Downloading", 2, Some(3));
        assert_eq!(p, 66.7);
    }

    #[test]
    fn test_progress_unknown_total_returns_zero() {
        assert_eq!(compute_progress_percent("Downloading", 5000, None), 0.0);
        assert_eq!(compute_progress_percent("Downloading", 5000, Some(0)), 0.0);
    }

    // --- Integration tests ---

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_downloads_returns_views() {
        let db = setup().await;
        insert_download(&db, 1, "Downloading", "file1.zip").await;
        insert_download(&db, 2, "Queued", "file2.zip").await;
        insert_segment(&db, 1, 1, 0, "Downloading").await;
        insert_segment(&db, 2, 1, 1, "Pending").await;

        let repo = SqliteDownloadReadRepo::new(db);
        let views = repo.find_downloads(None, None, None, None).unwrap();

        assert_eq!(views.len(), 2);
        assert_eq!(views[0].file_name, "file2.zip");
        assert_eq!(views[1].file_name, "file1.zip");
        assert_eq!(views[1].segments_active, 1);
        assert_eq!(views[1].segments_total, 2);
        assert!((views[1].progress_percent - 50.0).abs() < 0.01);
        assert_eq!(views[1].error_message, None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_downloads_includes_error_message() {
        let db = setup().await;
        let model = download::ActiveModel {
            id: Set(3),
            url: Set("https://example.com/video.mp4".to_string()),
            file_name: Set("video.mp4".to_string()),
            state: Set("Error".to_string()),
            priority: Set(5),
            total_bytes: Set(Some(1000)),
            downloaded_bytes: Set(500),
            speed_bytes_per_sec: Set(0),
            retry_count: Set(0),
            max_retries: Set(5),
            segments_count: Set(1),
            checksum_expected: Set(None),
            source_hostname: Set("example.com".to_string()),
            protocol: Set("https".to_string()),
            resume_supported: Set(1),
            module_name: Set(None),
            account_id: Set(None),
            destination_path: Set("/tmp/downloads/video.mp4".to_string()),
            error_message: Set(Some("tls handshake failed".to_string())),
            created_at: Set(1003),
            updated_at: Set(2003),
        };
        model
            .insert(&db)
            .await
            .expect("Failed to insert failed download");

        let repo = SqliteDownloadReadRepo::new(db);
        let views = repo.find_downloads(None, None, None, None).unwrap();

        assert_eq!(views.len(), 1);
        assert_eq!(
            views[0].error_message.as_deref(),
            Some("tls handshake failed")
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_download_detail_with_segments() {
        let db = setup().await;
        insert_download(&db, 1, "Downloading", "file1.zip").await;
        insert_segment(&db, 1, 1, 0, "Downloading").await;
        insert_segment(&db, 2, 1, 1, "Completed").await;

        let repo = SqliteDownloadReadRepo::new(db);
        let detail = repo
            .find_download_detail(DownloadId(1))
            .unwrap()
            .expect("Should find download");

        assert_eq!(detail.file_name, "file1.zip");
        assert_eq!(detail.segments.len(), 2);
        assert_eq!(detail.segments[0].state, SegmentState::Downloading);
        assert_eq!(detail.segments[1].state, SegmentState::Completed);
        assert!(detail.resume_supported);
        assert_eq!(detail.retry_count, 0);
        assert_eq!(detail.max_retries, 5);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_download_detail_populates_timestamps_for_saved_downloads() {
        let db = setup().await;
        let write_repo = SqliteDownloadRepo::new(db.clone());
        let read_repo = SqliteDownloadReadRepo::new(db);
        let created_at = 1_700_000_000_000_u64;
        let id = (created_at << 12) | 1;

        let download = Download::new(
            DownloadId(id),
            Url::new("https://example.com/file1.zip").unwrap(),
            "file1.zip".to_string(),
            "/tmp/downloads/file1.zip".to_string(),
        );

        write_repo.save(&download).unwrap();

        let detail = read_repo
            .find_download_detail(DownloadId(id))
            .unwrap()
            .expect("Should find download");

        assert_eq!(detail.created_at, created_at);
        assert_eq!(detail.updated_at, created_at);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_download_detail_infers_timestamps_for_legacy_zero_rows() {
        let db = setup().await;
        let created_at = 1_700_000_000_000_u64;
        let id = ((created_at << 12) | 7) as i64;

        let model = download::ActiveModel {
            id: Set(id),
            url: Set("https://example.com/file1.zip".to_string()),
            file_name: Set("file1.zip".to_string()),
            state: Set("Downloading".to_string()),
            priority: Set(5),
            total_bytes: Set(Some(1000)),
            downloaded_bytes: Set(500),
            speed_bytes_per_sec: Set(100),
            retry_count: Set(0),
            max_retries: Set(5),
            segments_count: Set(1),
            checksum_expected: Set(None),
            source_hostname: Set("example.com".to_string()),
            protocol: Set("https".to_string()),
            resume_supported: Set(1),
            module_name: Set(None),
            account_id: Set(None),
            destination_path: Set("/tmp/downloads/file1.zip".to_string()),
            error_message: Set(None),
            created_at: Set(0),
            updated_at: Set(0),
        };
        model.insert(&db).await.expect("Failed to insert download");

        let repo = SqliteDownloadReadRepo::new(db);
        let detail = repo
            .find_download_detail(DownloadId(id as u64))
            .unwrap()
            .expect("Should find download");

        assert_eq!(detail.created_at, created_at);
        assert_eq!(detail.updated_at, created_at);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_download_detail_uses_updated_at_for_undecodable_legacy_rows() {
        let db = setup().await;
        let model = download::ActiveModel {
            id: Set(1),
            url: Set("https://example.com/file1.zip".to_string()),
            file_name: Set("file1.zip".to_string()),
            state: Set("Downloading".to_string()),
            priority: Set(5),
            total_bytes: Set(Some(1000)),
            downloaded_bytes: Set(500),
            speed_bytes_per_sec: Set(100),
            retry_count: Set(0),
            max_retries: Set(5),
            segments_count: Set(1),
            checksum_expected: Set(None),
            source_hostname: Set("example.com".to_string()),
            protocol: Set("https".to_string()),
            resume_supported: Set(1),
            module_name: Set(None),
            account_id: Set(None),
            destination_path: Set("/tmp/downloads/file1.zip".to_string()),
            error_message: Set(None),
            created_at: Set(0),
            updated_at: Set(1_700_000_000_123_i64),
        };
        model.insert(&db).await.expect("Failed to insert download");

        let repo = SqliteDownloadReadRepo::new(db);
        let detail = repo
            .find_download_detail(DownloadId(1))
            .unwrap()
            .expect("Should find download");

        assert_eq!(detail.created_at, 1_700_000_000_123_u64);
        assert_eq!(detail.updated_at, 1_700_000_000_123_u64);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_download_detail_falls_back_to_min_plausible_time_for_undecodable_zero_rows()
    {
        let db = setup().await;
        let model = download::ActiveModel {
            id: Set(1),
            url: Set("https://example.com/file1.zip".to_string()),
            file_name: Set("file1.zip".to_string()),
            state: Set("Downloading".to_string()),
            priority: Set(5),
            total_bytes: Set(Some(1000)),
            downloaded_bytes: Set(500),
            speed_bytes_per_sec: Set(100),
            retry_count: Set(0),
            max_retries: Set(5),
            segments_count: Set(1),
            checksum_expected: Set(None),
            source_hostname: Set("example.com".to_string()),
            protocol: Set("https".to_string()),
            resume_supported: Set(1),
            module_name: Set(None),
            account_id: Set(None),
            destination_path: Set("/tmp/downloads/file1.zip".to_string()),
            error_message: Set(None),
            created_at: Set(0),
            updated_at: Set(0),
        };
        model.insert(&db).await.expect("Failed to insert download");

        let repo = SqliteDownloadReadRepo::new(db);
        let detail = repo
            .find_download_detail(DownloadId(1))
            .unwrap()
            .expect("Should find download");

        assert_eq!(detail.created_at, MIN_PLAUSIBLE_UNIX_MS);
        assert_eq!(detail.updated_at, MIN_PLAUSIBLE_UNIX_MS);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_downloads_sorts_by_inferred_created_at_for_legacy_rows() {
        let db = setup().await;
        let legacy_created_at = 1_700_000_000_000_u64;
        let legacy_id = ((legacy_created_at << 12) | 7) as i64;

        let legacy = download::ActiveModel {
            id: Set(legacy_id),
            url: Set("https://example.com/legacy.zip".to_string()),
            file_name: Set("legacy.zip".to_string()),
            state: Set("Queued".to_string()),
            priority: Set(5),
            total_bytes: Set(Some(1000)),
            downloaded_bytes: Set(0),
            speed_bytes_per_sec: Set(0),
            retry_count: Set(0),
            max_retries: Set(5),
            segments_count: Set(1),
            checksum_expected: Set(None),
            source_hostname: Set("example.com".to_string()),
            protocol: Set("https".to_string()),
            resume_supported: Set(1),
            module_name: Set(None),
            account_id: Set(None),
            destination_path: Set("/tmp/downloads/legacy.zip".to_string()),
            error_message: Set(None),
            created_at: Set(0),
            updated_at: Set(0),
        };
        legacy
            .insert(&db)
            .await
            .expect("Failed to insert legacy download");

        insert_download(&db, 1, "Queued", "recent.zip").await;

        let repo = SqliteDownloadReadRepo::new(db);
        let views = repo.find_downloads(None, None, None, None).unwrap();

        assert_eq!(views[0].file_name, "legacy.zip");
        assert_eq!(views[0].created_at, legacy_created_at);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_downloads_with_state_filter() {
        let db = setup().await;
        insert_download(&db, 1, "Downloading", "file1.zip").await;
        insert_download(&db, 2, "Queued", "file2.zip").await;
        insert_download(&db, 3, "Downloading", "file3.zip").await;

        let repo = SqliteDownloadReadRepo::new(db);
        let filter = DownloadFilter {
            state: Some(DownloadState::Downloading),
            ..Default::default()
        };
        let views = repo.find_downloads(Some(filter), None, None, None).unwrap();

        assert_eq!(views.len(), 2);
        assert!(views.iter().all(|v| v.state == DownloadState::Downloading));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_count_by_state() {
        let db = setup().await;
        insert_download(&db, 1, "Downloading", "file1.zip").await;
        insert_download(&db, 2, "Queued", "file2.zip").await;
        insert_download(&db, 3, "Downloading", "file3.zip").await;
        insert_download(&db, 4, "Completed", "file4.zip").await;

        let repo = SqliteDownloadReadRepo::new(db);
        let counts = repo.count_by_state().unwrap();

        assert_eq!(counts.get(&DownloadState::Downloading), Some(&2));
        assert_eq!(counts.get(&DownloadState::Queued), Some(&1));
        assert_eq!(counts.get(&DownloadState::Completed), Some(&1));
        assert_eq!(counts.get(&DownloadState::Paused), None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_download_detail_not_found() {
        let db = setup().await;

        let repo = SqliteDownloadReadRepo::new(db);
        let detail = repo.find_download_detail(DownloadId(999)).unwrap();

        assert!(detail.is_none());
    }
}
