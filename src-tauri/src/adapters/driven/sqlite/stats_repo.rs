use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

use crate::domain::error::DomainError;
use crate::domain::model::views::{DailyVolume, HostStats, StatsView};
use crate::domain::ports::driven::stats_repository::StatsRepository;

use super::util::{block_on, map_db_err, safe_u64};

pub struct SqliteStatsRepo {
    db: DatabaseConnection,
}

impl SqliteStatsRepo {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }
}

impl StatsRepository for SqliteStatsRepo {
    fn record_completed(&self, bytes: u64, avg_speed: u64) -> Result<(), DomainError> {
        block_on(async {
            let sql = "\
                INSERT INTO statistics (id, date, bytes_downloaded, files_completed, avg_speed, peak_speed) \
                VALUES (CAST(strftime('%Y%m%d', 'now') AS INTEGER), date('now'), ?1, 1, ?2, ?2) \
                ON CONFLICT(date) DO UPDATE SET \
                  bytes_downloaded = bytes_downloaded + excluded.bytes_downloaded, \
                  files_completed = files_completed + 1, \
                  avg_speed = CAST((CAST(avg_speed AS REAL) * files_completed + excluded.avg_speed) / (files_completed + 1) AS INTEGER), \
                  peak_speed = MAX(peak_speed, excluded.peak_speed)";

            self.db
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    sql,
                    [
                        sea_orm::Value::BigInt(Some(bytes as i64)),
                        sea_orm::Value::BigInt(Some(avg_speed as i64)),
                    ],
                ))
                .await
                .map_err(map_db_err)?;

            Ok(())
        })
    }

    fn get_stats(&self) -> Result<StatsView, DomainError> {
        block_on(async {
            // 1. Totals
            let totals_row = self
                .db
                .query_one(Statement::from_string(
                    DatabaseBackend::Sqlite,
                    "\
                    SELECT \
                      COALESCE(SUM(bytes_downloaded),0), \
                      COALESCE(SUM(files_completed),0), \
                      CASE WHEN SUM(files_completed)>0 \
                        THEN CAST(SUM(CAST(avg_speed AS REAL)*files_completed)/SUM(files_completed) AS INTEGER) \
                        ELSE 0 END, \
                      COALESCE(MAX(peak_speed),0) \
                    FROM statistics"
                        .to_string(),
                ))
                .await
                .map_err(map_db_err)?;

            let (total_downloaded_bytes, total_files, avg_speed, peak_speed) = match totals_row {
                Some(row) => {
                    let b: i64 = row
                        .try_get_by_index(0)
                        .map_err(|e| DomainError::StorageError(e.to_string()))?;
                    let f: i64 = row
                        .try_get_by_index(1)
                        .map_err(|e| DomainError::StorageError(e.to_string()))?;
                    let a: i64 = row
                        .try_get_by_index(2)
                        .map_err(|e| DomainError::StorageError(e.to_string()))?;
                    let p: i64 = row
                        .try_get_by_index(3)
                        .map_err(|e| DomainError::StorageError(e.to_string()))?;
                    (safe_u64(b), safe_u64(f), safe_u64(a), safe_u64(p))
                }
                None => (0u64, 0u64, 0u64, 0u64),
            };

            // 2. Daily volumes
            let daily_rows = self
                .db
                .query_all(Statement::from_string(
                    DatabaseBackend::Sqlite,
                    "SELECT date, bytes_downloaded, files_completed FROM statistics ORDER BY date DESC LIMIT 30"
                        .to_string(),
                ))
                .await
                .map_err(map_db_err)?;

            let mut daily_volumes = Vec::with_capacity(daily_rows.len());
            for row in daily_rows {
                let date: String = row
                    .try_get_by_index(0)
                    .map_err(|e| DomainError::StorageError(e.to_string()))?;
                let bytes: i64 = row
                    .try_get_by_index(1)
                    .map_err(|e| DomainError::StorageError(e.to_string()))?;
                let count: i64 = row
                    .try_get_by_index(2)
                    .map_err(|e| DomainError::StorageError(e.to_string()))?;
                daily_volumes.push(DailyVolume {
                    date,
                    bytes: safe_u64(bytes),
                    count: safe_u64(count),
                });
            }

            // 3. Success rate
            let success_row = self
                .db
                .query_one(Statement::from_string(
                    DatabaseBackend::Sqlite,
                    "SELECT COUNT(*), COALESCE(SUM(CASE WHEN state='Completed' THEN 1 ELSE 0 END),0) \
                     FROM downloads WHERE state IN ('Completed','Failed','Cancelled')"
                        .to_string(),
                ))
                .await
                .map_err(map_db_err)?;

            let success_rate = match success_row {
                Some(row) => {
                    let total: i64 = row
                        .try_get_by_index(0)
                        .map_err(|e| DomainError::StorageError(e.to_string()))?;
                    let completed: i64 = row
                        .try_get_by_index(1)
                        .map_err(|e| DomainError::StorageError(e.to_string()))?;
                    if total > 0 {
                        completed as f64 / total as f64
                    } else {
                        0.0
                    }
                }
                None => 0.0,
            };

            // 4. Top hosts
            let host_rows = self
                .db
                .query_all(Statement::from_string(
                    DatabaseBackend::Sqlite,
                    "SELECT source_hostname, SUM(downloaded_bytes), COUNT(*) \
                     FROM downloads \
                     WHERE source_hostname IS NOT NULL AND source_hostname != '' \
                     GROUP BY source_hostname \
                     ORDER BY 2 DESC LIMIT 10"
                        .to_string(),
                ))
                .await
                .map_err(map_db_err)?;

            let mut top_hosts = Vec::with_capacity(host_rows.len());
            for row in host_rows {
                let hostname: String = row
                    .try_get_by_index(0)
                    .map_err(|e| DomainError::StorageError(e.to_string()))?;
                let total_bytes: i64 = row
                    .try_get_by_index(1)
                    .map_err(|e| DomainError::StorageError(e.to_string()))?;
                let count: i64 = row
                    .try_get_by_index(2)
                    .map_err(|e| DomainError::StorageError(e.to_string()))?;
                top_hosts.push(HostStats {
                    hostname,
                    total_bytes: safe_u64(total_bytes),
                    download_count: safe_u64(count),
                });
            }

            Ok(StatsView {
                total_downloaded_bytes,
                total_files,
                avg_speed,
                peak_speed,
                success_rate,
                daily_volumes,
                top_hosts,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::connection::setup_test_db;
    use super::*;
    use sea_orm::{ConnectionTrait, Statement};

    #[tokio::test(flavor = "multi_thread")]
    async fn test_record_completed_inserts_new_day() {
        let db = setup_test_db().await.expect("failed to setup test db");
        let repo = SqliteStatsRepo::new(db.clone());

        repo.record_completed(1024, 512).expect("record_completed");

        let row = db
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT bytes_downloaded, files_completed FROM statistics LIMIT 1".to_string(),
            ))
            .await
            .expect("query failed")
            .expect("no row");

        let bytes: i64 = row.try_get_by_index(0).unwrap();
        let files: i64 = row.try_get_by_index(1).unwrap();
        assert_eq!(bytes, 1024);
        assert_eq!(files, 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_record_completed_accumulates_same_day() {
        let db = setup_test_db().await.expect("failed to setup test db");
        let repo = SqliteStatsRepo::new(db.clone());

        repo.record_completed(1000, 200).expect("first record");
        repo.record_completed(500, 400).expect("second record");

        let row = db
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT bytes_downloaded, files_completed, avg_speed, peak_speed FROM statistics LIMIT 1"
                    .to_string(),
            ))
            .await
            .expect("query failed")
            .expect("no row");

        let bytes: i64 = row.try_get_by_index(0).unwrap();
        let files: i64 = row.try_get_by_index(1).unwrap();
        let avg: i64 = row.try_get_by_index(2).unwrap();
        let peak: i64 = row.try_get_by_index(3).unwrap();
        assert_eq!(bytes, 1500);
        assert_eq!(files, 2);
        // Running average: (200 * 1 + 400) / 2 = 300
        assert_eq!(avg, 300);
        assert_eq!(peak, 400);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_get_stats_empty_db() {
        let db = setup_test_db().await.expect("failed to setup test db");
        let repo = SqliteStatsRepo::new(db);

        let stats = repo.get_stats().expect("get_stats");
        assert_eq!(stats.total_downloaded_bytes, 0);
        assert_eq!(stats.total_files, 0);
        assert_eq!(stats.avg_speed, 0);
        assert_eq!(stats.peak_speed, 0);
        assert_eq!(stats.success_rate, 0.0);
        assert!(stats.daily_volumes.is_empty());
        assert!(stats.top_hosts.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_get_stats_daily_volumes() {
        let db = setup_test_db().await.expect("failed to setup test db");

        // Insert two rows with explicit dates
        db.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            "INSERT INTO statistics (id, date, bytes_downloaded, files_completed, avg_speed, peak_speed) \
             VALUES (20260101, '2026-01-01', 2000, 2, 100, 200)".to_string(),
        ))
        .await
        .expect("insert day 1");

        db.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            "INSERT INTO statistics (id, date, bytes_downloaded, files_completed, avg_speed, peak_speed) \
             VALUES (20260102, '2026-01-02', 3000, 3, 150, 300)".to_string(),
        ))
        .await
        .expect("insert day 2");

        let repo = SqliteStatsRepo::new(db);
        let stats = repo.get_stats().expect("get_stats");

        assert_eq!(stats.daily_volumes.len(), 2);
        // DESC order: most recent first
        assert_eq!(stats.daily_volumes[0].date, "2026-01-02");
        assert_eq!(stats.daily_volumes[0].bytes, 3000);
        assert_eq!(stats.daily_volumes[1].date, "2026-01-01");
        assert_eq!(stats.daily_volumes[1].bytes, 2000);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_get_stats_success_rate() {
        let db = setup_test_db().await.expect("failed to setup test db");

        // Insert 3 terminal downloads: 2 Completed, 1 Failed — all count for success rate
        for (id, state) in [(1i64, "Completed"), (2, "Completed"), (3, "Failed")] {
            db.execute(Statement::from_string(
                DatabaseBackend::Sqlite,
                format!(
                    "INSERT INTO downloads \
                     (id, url, file_name, state, priority, downloaded_bytes, speed_bytes_per_sec, \
                      retry_count, max_retries, segments_count, resume_supported, \
                      source_hostname, protocol, destination_path, created_at, updated_at) \
                     VALUES ({id}, 'https://example.com/{id}', 'file{id}.zip', '{state}', 5, 0, 0, \
                             0, 5, 1, 0, 'example.com', 'https', '/tmp', 1000, 2000)"
                ),
            ))
            .await
            .expect("insert download");
        }

        let repo = SqliteStatsRepo::new(db);
        let stats = repo.get_stats().expect("get_stats");

        let expected_rate = 2.0 / 3.0;
        assert!((stats.success_rate - expected_rate).abs() < 1e-9);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_get_stats_top_hosts() {
        let db = setup_test_db().await.expect("failed to setup test db");

        // 3 downloads on host-a (more bytes), 1 on host-b
        for (id, hostname, bytes) in [
            (1i64, "host-a.com", 5000i64),
            (2, "host-a.com", 3000),
            (3, "host-a.com", 2000),
            (4, "host-b.com", 1000),
        ] {
            db.execute(Statement::from_string(
                DatabaseBackend::Sqlite,
                format!(
                    "INSERT INTO downloads \
                     (id, url, file_name, state, priority, downloaded_bytes, speed_bytes_per_sec, \
                      retry_count, max_retries, segments_count, resume_supported, \
                      source_hostname, protocol, destination_path, created_at, updated_at) \
                     VALUES ({id}, 'https://{hostname}/{id}', 'file{id}.zip', 'Completed', 5, \
                             {bytes}, 0, 0, 5, 1, 0, '{hostname}', 'https', '/tmp', 1000, 2000)"
                ),
            ))
            .await
            .expect("insert download");
        }

        let repo = SqliteStatsRepo::new(db);
        let stats = repo.get_stats().expect("get_stats");

        assert_eq!(stats.top_hosts.len(), 2);
        // host-a has more total bytes, should be first
        assert_eq!(stats.top_hosts[0].hostname, "host-a.com");
        assert_eq!(stats.top_hosts[0].total_bytes, 10000);
        assert_eq!(stats.top_hosts[0].download_count, 3);
        assert_eq!(stats.top_hosts[1].hostname, "host-b.com");
    }
}
