//! SQLite implementation of [`PackageReadRepository`] (CQRS read side).
//!
//! Statistics (`downloads_count`, `total_bytes`, `progress_percent`,
//! `all_completed`) are computed in a single `LEFT JOIN` between
//! `packages` and `downloads` so listing N packages costs one query
//! instead of N+1.

use std::collections::HashMap;

use sea_orm::{
    ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter, Statement, sea_query::Value,
};

use crate::adapters::driven::sqlite::entities::{download, download_segment};
use crate::adapters::driven::sqlite::util::{block_on, map_db_err, safe_u64};
use crate::domain::error::DomainError;
use crate::domain::model::download::DownloadId;
use crate::domain::model::package::PackageId;
use crate::domain::model::views::{DownloadView, PackageFilter, PackageView};
use crate::domain::ports::driven::package_read_repository::PackageReadRepository;

pub struct SqlitePackageReadRepo {
    db: DatabaseConnection,
}

impl SqlitePackageReadRepo {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }
}

/// Round to one decimal place. Mirrors `download_read_repo` to keep the
/// progress display consistent across the UI.
fn round_one_dp(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn aggregate_progress_percent(downloaded: u64, total: u64, all_completed: bool) -> f64 {
    if all_completed {
        return 100.0;
    }
    if total == 0 {
        return 0.0;
    }
    round_one_dp(downloaded as f64 / total as f64 * 100.0)
}

/// Map an aggregated row back to a [`PackageView`]. Centralised so both
/// list and single-id paths apply the same coercion rules.
fn row_to_view(row: &sea_orm::QueryResult) -> Result<PackageView, DomainError> {
    let id: String = row.try_get_by_index(0).map_err(map_db_err)?;
    let name: String = row.try_get_by_index(1).map_err(map_db_err)?;
    let source_type: String = row.try_get_by_index(2).map_err(map_db_err)?;
    let folder_path: Option<String> = row.try_get_by_index(3).map_err(map_db_err)?;
    let auto_extract_raw: i64 = row.try_get_by_index(4).map_err(map_db_err)?;
    let priority_raw: i64 = row.try_get_by_index(5).map_err(map_db_err)?;
    let created_at_raw: i64 = row.try_get_by_index(6).map_err(map_db_err)?;
    let count_raw: i64 = row.try_get_by_index(7).map_err(map_db_err)?;
    // SUM(...) produces NULL when no row matches the LEFT JOIN — surface
    // it as 0 instead of erroring.
    let total_bytes_raw: Option<i64> = row.try_get_by_index(8).map_err(map_db_err)?;
    let downloaded_bytes_raw: Option<i64> = row.try_get_by_index(9).map_err(map_db_err)?;
    let completed_count_raw: Option<i64> = row.try_get_by_index(10).map_err(map_db_err)?;

    let auto_extract = match auto_extract_raw {
        0 => false,
        1 => true,
        other => {
            return Err(DomainError::ValidationError(format!(
                "package {id}: auto_extract {other} out of bool range",
            )));
        }
    };
    let priority = u8::try_from(priority_raw).map_err(|_| {
        DomainError::ValidationError(format!(
            "package {id}: priority {priority_raw} out of u8 range",
        ))
    })?;
    if !(1..=10).contains(&priority) {
        return Err(DomainError::ValidationError(format!(
            "package {id}: priority {priority} outside [1, 10]",
        )));
    }
    let created_at = u64::try_from(created_at_raw).map_err(|_| {
        DomainError::ValidationError(format!(
            "package {id}: created_at {created_at_raw} out of u64 range",
        ))
    })?;
    let downloads_count = safe_u64(count_raw);
    let total_bytes = total_bytes_raw.map(safe_u64).unwrap_or(0);
    let downloaded_bytes = downloaded_bytes_raw.map(safe_u64).unwrap_or(0);
    let completed_count = completed_count_raw.map(safe_u64).unwrap_or(0);
    let all_completed = downloads_count > 0 && completed_count == downloads_count;
    let progress_percent = aggregate_progress_percent(downloaded_bytes, total_bytes, all_completed);

    Ok(PackageView {
        id,
        name,
        source_type,
        folder_path,
        auto_extract,
        priority,
        created_at,
        downloads_count,
        total_bytes,
        downloaded_bytes,
        progress_percent,
        all_completed,
    })
}

const PACKAGE_AGG_SELECT: &str = "SELECT \
    p.id, p.name, p.source_type, p.folder_path, p.auto_extract, p.priority, p.created_at, \
    COUNT(d.id) AS downloads_count, \
    COALESCE(SUM(COALESCE(d.total_bytes, 0)), 0) AS total_bytes_sum, \
    COALESCE(SUM(d.downloaded_bytes), 0) AS downloaded_bytes_sum, \
    COALESCE(SUM(CASE WHEN d.state = 'Completed' THEN 1 ELSE 0 END), 0) AS completed_count \
    FROM packages p LEFT JOIN downloads d ON d.package_id = p.id";

fn compute_progress_percent_for_download(state: &str, downloaded: u64, total: Option<u64>) -> f64 {
    if state == "Completed" {
        return 100.0;
    }
    match total {
        Some(t) if t > 0 => round_one_dp(downloaded as f64 / t as f64 * 100.0),
        _ => 0.0,
    }
}

fn download_row_to_view(
    model: &download::Model,
    segments_active: u32,
    segments_total: u32,
) -> Result<DownloadView, DomainError> {
    let total = model.total_bytes.map(safe_u64);
    let downloaded = safe_u64(model.downloaded_bytes);
    let speed = safe_u64(model.speed_bytes_per_sec);
    let progress_percent = compute_progress_percent_for_download(&model.state, downloaded, total);
    let eta_seconds = match total {
        Some(t) if speed > 0 && t > downloaded => Some((t - downloaded) / speed),
        _ => None,
    };
    let state = model.state.parse().map_err(|_| {
        DomainError::StorageError(format!("invalid download state in DB: {}", model.state))
    })?;
    let priority_u8 = u8::try_from(model.priority).unwrap_or(5);
    let created_at = safe_u64(model.created_at);

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
        account_name: None,
        error_message: model.error_message.clone(),
        priority: priority_u8,
        queue_position: model.queue_position,
        created_at,
    })
}

impl PackageReadRepository for SqlitePackageReadRepo {
    fn find_packages(
        &self,
        filter: Option<PackageFilter>,
    ) -> Result<Vec<PackageView>, DomainError> {
        let mut sql = String::from(PACKAGE_AGG_SELECT);
        let mut clauses: Vec<&'static str> = Vec::new();
        let mut params: Vec<Value> = Vec::new();
        let lowered_name: Option<String>;
        if let Some(ref f) = filter {
            if let Some(ref source) = f.source_type {
                clauses.push("p.source_type = ?");
                params.push(Value::from(source.clone()));
            }
            if let Some(ref needle) = f.name_q {
                let trimmed = needle.trim();
                if !trimmed.is_empty() {
                    clauses.push("LOWER(p.name) LIKE ?");
                    lowered_name = Some(format!("%{}%", trimmed.to_lowercase()));
                    params.push(Value::from(lowered_name.unwrap()));
                }
            }
        }
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(" GROUP BY p.id ORDER BY p.created_at ASC, p.id ASC");

        block_on(async {
            let rows = self
                .db
                .query_all(Statement::from_sql_and_values(
                    sea_orm::DatabaseBackend::Sqlite,
                    &sql,
                    params,
                ))
                .await
                .map_err(map_db_err)?;
            rows.iter().map(row_to_view).collect()
        })
    }

    fn find_package_by_id(&self, id: &PackageId) -> Result<Option<PackageView>, DomainError> {
        let sql = format!("{PACKAGE_AGG_SELECT} WHERE p.id = ? GROUP BY p.id");
        let id_value = id.as_str().to_string();
        block_on(async {
            let row = self
                .db
                .query_one(Statement::from_sql_and_values(
                    sea_orm::DatabaseBackend::Sqlite,
                    &sql,
                    [Value::from(id_value)],
                ))
                .await
                .map_err(map_db_err)?;
            match row {
                None => Ok(None),
                Some(r) => Ok(Some(row_to_view(&r)?)),
            }
        })
    }

    fn find_package_downloads(&self, id: &PackageId) -> Result<Vec<DownloadView>, DomainError> {
        use sea_orm::ColumnTrait;

        let id_value = id.as_str().to_string();
        block_on(async {
            // The `download::Model` does not yet expose `package_id` as a
            // typed sea-orm column (the FK was added in a later
            // migration), so resolve member ids through raw SQL — same
            // approach `SqlitePackageRepo::list_downloads` uses on the
            // write side.
            let id_rows = self
                .db
                .query_all(Statement::from_sql_and_values(
                    sea_orm::DatabaseBackend::Sqlite,
                    "SELECT id FROM downloads WHERE package_id = ? ORDER BY queue_position ASC, id ASC",
                    [Value::from(id_value)],
                ))
                .await
                .map_err(map_db_err)?;

            if id_rows.is_empty() {
                return Ok(Vec::new());
            }

            let download_ids: Vec<i64> = id_rows
                .iter()
                .map(|r| r.try_get_by_index::<i64>(0).map_err(map_db_err))
                .collect::<Result<Vec<_>, _>>()?;

            let downloads = download::Entity::find()
                .filter(download::Column::Id.is_in(download_ids.clone()))
                .all(&self.db)
                .await
                .map_err(map_db_err)?;

            let segments = download_segment::Entity::find()
                .filter(download_segment::Column::DownloadId.is_in(download_ids.clone()))
                .all(&self.db)
                .await
                .map_err(map_db_err)?;

            let mut seg_map: HashMap<i64, (u32, u32)> = HashMap::new();
            for seg in &segments {
                let entry = seg_map.entry(seg.download_id).or_insert((0, 0));
                entry.1 = entry.1.saturating_add(1);
                if seg.state == "Downloading" {
                    entry.0 = entry.0.saturating_add(1);
                }
            }

            // Map by id for stable lookup, then re-emit in the order the
            // raw query produced (queue_position ASC, id ASC).
            let mut by_id: HashMap<i64, &download::Model> = HashMap::new();
            for d in &downloads {
                by_id.insert(d.id, d);
            }

            download_ids
                .iter()
                .filter_map(|id| by_id.get(id).copied())
                .map(|d| {
                    let (active, total) = seg_map.get(&d.id).copied().unwrap_or((0, 0));
                    download_row_to_view(d, active, total)
                })
                .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::driven::sqlite::connection::setup_test_db;
    use crate::adapters::driven::sqlite::package_repo::SqlitePackageRepo;
    use crate::domain::model::package::{Package, PackageId, PackageSourceType};
    use crate::domain::ports::driven::package_repository::PackageRepository;
    use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};

    fn make_package(id: &str, name: &str, source: PackageSourceType, created: u64) -> Package {
        Package::new(PackageId::new(id), name.to_string(), source, created)
    }

    async fn insert_download(
        db: &DatabaseConnection,
        id: i64,
        package_id: Option<&str>,
        state: &str,
        total: Option<i64>,
        downloaded: i64,
        queue_position: i64,
    ) {
        let pkg = match package_id {
            Some(p) => format!("'{p}'"),
            None => "NULL".to_string(),
        };
        let total_sql = match total {
            Some(t) => t.to_string(),
            None => "NULL".to_string(),
        };
        let sql = format!(
            "INSERT INTO downloads (id, url, file_name, state, priority, queue_position, total_bytes, downloaded_bytes, speed_bytes_per_sec, retry_count, max_retries, segments_count, source_hostname, protocol, resume_supported, destination_path, created_at, updated_at, package_id) VALUES ({id}, 'https://example.com/f.zip', 'f.zip', '{state}', 5, {queue_position}, {total_sql}, {downloaded}, 0, 0, 5, 1, 'example.com', 'https', 0, '/tmp', 1, 1, {pkg})"
        );
        db.execute(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            sql,
        ))
        .await
        .expect("seed download");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_packages_returns_empty_when_no_packages() {
        let db = setup_test_db().await.expect("test db");
        let read = SqlitePackageReadRepo::new(db);
        let result = read.find_packages(None).expect("find_packages");
        assert!(result.is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_packages_returns_view_with_zero_stats_for_empty_package() {
        let db = setup_test_db().await.expect("test db");
        let write = SqlitePackageRepo::new(db.clone());
        let read = SqlitePackageReadRepo::new(db);

        write
            .save(&make_package("p-1", "Solo", PackageSourceType::Manual, 100))
            .expect("save");

        let result = read.find_packages(None).unwrap();
        assert_eq!(result.len(), 1);
        let v = &result[0];
        assert_eq!(v.id, "p-1");
        assert_eq!(v.name, "Solo");
        assert_eq!(v.source_type, "manual");
        assert!(v.folder_path.is_none());
        assert!(v.auto_extract);
        assert_eq!(v.priority, 5);
        assert_eq!(v.created_at, 100);
        assert_eq!(v.downloads_count, 0);
        assert_eq!(v.total_bytes, 0);
        assert_eq!(v.downloaded_bytes, 0);
        assert_eq!(v.progress_percent, 0.0);
        assert!(!v.all_completed, "empty package must not report completed");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_packages_aggregates_member_downloads() {
        let db = setup_test_db().await.expect("test db");
        let write = SqlitePackageRepo::new(db.clone());
        let read = SqlitePackageReadRepo::new(db.clone());

        write
            .save(&make_package(
                "agg",
                "Aggregate",
                PackageSourceType::Playlist,
                42,
            ))
            .unwrap();

        // 3 members: 2 partially downloaded, 1 completed (100% by state).
        insert_download(&db, 1, Some("agg"), "Downloading", Some(1000), 250, 0).await;
        insert_download(&db, 2, Some("agg"), "Downloading", Some(2000), 500, 1).await;
        insert_download(&db, 3, Some("agg"), "Completed", Some(500), 500, 2).await;
        // One unattached download must NOT influence the aggregate.
        insert_download(&db, 4, None, "Downloading", Some(99_999), 99_999, 9).await;

        let result = read.find_packages(None).unwrap();
        assert_eq!(result.len(), 1);
        let v = &result[0];
        assert_eq!(v.id, "agg");
        assert_eq!(v.downloads_count, 3);
        assert_eq!(v.total_bytes, 3500);
        assert_eq!(v.downloaded_bytes, 1250);
        // 1250 / 3500 = 35.714... → 35.7
        assert!(
            (v.progress_percent - 35.7).abs() < 0.01,
            "progress_percent = {}",
            v.progress_percent
        );
        assert!(!v.all_completed, "one member still Downloading");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_packages_all_completed_is_true_when_every_member_completed() {
        let db = setup_test_db().await.expect("test db");
        let write = SqlitePackageRepo::new(db.clone());
        let read = SqlitePackageReadRepo::new(db.clone());

        write
            .save(&make_package("done", "Done", PackageSourceType::Manual, 7))
            .unwrap();
        insert_download(&db, 10, Some("done"), "Completed", Some(100), 100, 0).await;
        insert_download(&db, 11, Some("done"), "Completed", Some(200), 200, 1).await;

        let v = &read.find_packages(None).unwrap()[0];
        assert!(v.all_completed);
        assert_eq!(v.progress_percent, 100.0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_packages_treats_unknown_total_as_zero() {
        let db = setup_test_db().await.expect("test db");
        let write = SqlitePackageRepo::new(db.clone());
        let read = SqlitePackageReadRepo::new(db.clone());

        write
            .save(&make_package(
                "no-total",
                "Untracked",
                PackageSourceType::Manual,
                10,
            ))
            .unwrap();
        // total_bytes = NULL must contribute 0 to the SUM.
        insert_download(&db, 50, Some("no-total"), "Downloading", None, 100, 0).await;

        let v = &read.find_packages(None).unwrap()[0];
        assert_eq!(v.downloads_count, 1);
        assert_eq!(v.total_bytes, 0);
        assert_eq!(v.downloaded_bytes, 100);
        assert_eq!(
            v.progress_percent, 0.0,
            "no known total => progress unknown => 0"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_packages_orders_by_created_at_then_id() {
        let db = setup_test_db().await.expect("test db");
        let write = SqlitePackageRepo::new(db.clone());
        let read = SqlitePackageReadRepo::new(db);

        write
            .save(&make_package("c", "C", PackageSourceType::Manual, 20))
            .unwrap();
        write
            .save(&make_package("a", "A", PackageSourceType::Manual, 10))
            .unwrap();
        write
            .save(&make_package("b", "B", PackageSourceType::Manual, 10))
            .unwrap();

        let result = read.find_packages(None).unwrap();
        let ids: Vec<&str> = result.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_packages_filter_by_source_type() {
        let db = setup_test_db().await.expect("test db");
        let write = SqlitePackageRepo::new(db.clone());
        let read = SqlitePackageReadRepo::new(db);

        write
            .save(&make_package("m", "M", PackageSourceType::Manual, 1))
            .unwrap();
        write
            .save(&make_package("p", "P", PackageSourceType::Playlist, 2))
            .unwrap();
        write
            .save(&make_package("c", "C", PackageSourceType::Container, 3))
            .unwrap();

        let result = read
            .find_packages(Some(PackageFilter {
                source_type: Some("playlist".to_string()),
                name_q: None,
            }))
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "p");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_packages_filter_by_name_q_is_case_insensitive_substring() {
        let db = setup_test_db().await.expect("test db");
        let write = SqlitePackageRepo::new(db.clone());
        let read = SqlitePackageReadRepo::new(db);

        write
            .save(&make_package(
                "1",
                "Holiday Photos 2025",
                PackageSourceType::Manual,
                1,
            ))
            .unwrap();
        write
            .save(&make_package(
                "2",
                "Music — Holidays",
                PackageSourceType::Manual,
                2,
            ))
            .unwrap();
        write
            .save(&make_package("3", "Misc", PackageSourceType::Manual, 3))
            .unwrap();

        let result = read
            .find_packages(Some(PackageFilter {
                source_type: None,
                name_q: Some("HOLIDAY".to_string()),
            }))
            .unwrap();
        let ids: Vec<&str> = result.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["1", "2"]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_packages_filter_combines_source_and_name_q_with_and() {
        let db = setup_test_db().await.expect("test db");
        let write = SqlitePackageRepo::new(db.clone());
        let read = SqlitePackageReadRepo::new(db);

        write
            .save(&make_package(
                "p1",
                "Holiday Mix",
                PackageSourceType::Playlist,
                1,
            ))
            .unwrap();
        write
            .save(&make_package(
                "m1",
                "Holiday Manual",
                PackageSourceType::Manual,
                2,
            ))
            .unwrap();

        let result = read
            .find_packages(Some(PackageFilter {
                source_type: Some("playlist".to_string()),
                name_q: Some("holiday".to_string()),
            }))
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "p1");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_packages_filter_blank_name_q_is_ignored() {
        let db = setup_test_db().await.expect("test db");
        let write = SqlitePackageRepo::new(db.clone());
        let read = SqlitePackageReadRepo::new(db);

        write
            .save(&make_package("p1", "X", PackageSourceType::Manual, 1))
            .unwrap();

        let result = read
            .find_packages(Some(PackageFilter {
                source_type: None,
                name_q: Some("   ".to_string()),
            }))
            .unwrap();
        assert_eq!(result.len(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_package_by_id_returns_none_when_missing() {
        let db = setup_test_db().await.expect("test db");
        let read = SqlitePackageReadRepo::new(db);
        let result = read
            .find_package_by_id(&PackageId::new("ghost"))
            .expect("query");
        assert!(result.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_package_by_id_returns_aggregated_view() {
        let db = setup_test_db().await.expect("test db");
        let write = SqlitePackageRepo::new(db.clone());
        let read = SqlitePackageReadRepo::new(db.clone());

        write
            .save(&make_package(
                "single",
                "Single",
                PackageSourceType::Manual,
                1,
            ))
            .unwrap();
        insert_download(&db, 60, Some("single"), "Downloading", Some(1000), 250, 0).await;

        let v = read
            .find_package_by_id(&PackageId::new("single"))
            .unwrap()
            .expect("present");
        assert_eq!(v.id, "single");
        assert_eq!(v.downloads_count, 1);
        assert_eq!(v.total_bytes, 1000);
        assert_eq!(v.downloaded_bytes, 250);
        assert!((v.progress_percent - 25.0).abs() < 0.01);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_package_downloads_returns_empty_for_missing_package() {
        let db = setup_test_db().await.expect("test db");
        let read = SqlitePackageReadRepo::new(db);
        let result = read
            .find_package_downloads(&PackageId::new("never"))
            .unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_package_downloads_returns_members_ordered_by_queue_position() {
        let db = setup_test_db().await.expect("test db");
        let write = SqlitePackageRepo::new(db.clone());
        let read = SqlitePackageReadRepo::new(db.clone());

        write
            .save(&make_package("ord", "Ord", PackageSourceType::Manual, 1))
            .unwrap();
        insert_download(&db, 700, Some("ord"), "Downloading", Some(100), 50, 5).await;
        insert_download(&db, 701, Some("ord"), "Downloading", Some(100), 25, 1).await;
        insert_download(&db, 702, Some("ord"), "Downloading", Some(100), 75, 3).await;
        // Unattached must NOT appear.
        insert_download(&db, 999, None, "Downloading", Some(100), 50, 0).await;

        let result = read.find_package_downloads(&PackageId::new("ord")).unwrap();
        let ids: Vec<u64> = result.iter().map(|d| d.id.0).collect();
        assert_eq!(ids, vec![701, 702, 700]);
        assert!(result.iter().all(|d| d.id.0 != 999));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_package_downloads_progress_matches_individual_download() {
        let db = setup_test_db().await.expect("test db");
        let write = SqlitePackageRepo::new(db.clone());
        let read = SqlitePackageReadRepo::new(db.clone());

        write
            .save(&make_package("prog", "Prog", PackageSourceType::Manual, 1))
            .unwrap();
        insert_download(&db, 800, Some("prog"), "Downloading", Some(1000), 333, 0).await;
        insert_download(&db, 801, Some("prog"), "Completed", Some(2000), 1500, 1).await;

        let views = read
            .find_package_downloads(&PackageId::new("prog"))
            .unwrap();
        assert_eq!(views.len(), 2);
        // 333 / 1000 = 33.3
        assert!((views[0].progress_percent - 33.3).abs() < 0.01);
        // Completed always reports 100 even when downloaded < total.
        assert_eq!(views[1].progress_percent, 100.0);
    }
}
