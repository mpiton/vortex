use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

use crate::domain::error::DomainError;
use crate::domain::model::views::{DailyVolume, HostStats, ModuleStats, StatsPeriod, StatsView};
use crate::domain::ports::driven::stats_repository::StatsRepository;

use super::util::{block_on, map_db_err, safe_u64};

const SECONDS_PER_DAY: u64 = 86_400;

pub struct SqliteStatsRepo {
    db: DatabaseConnection,
}

impl SqliteStatsRepo {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }
}

/// Cutoff boundaries for a given period.
///
/// Two parallel representations are needed because the `statistics.date`
/// column is written with SQLite `date('now', 'localtime')`, while the
/// `downloads.created_at` column stores a Unix epoch. Mixing the two
/// (e.g. comparing `statistics.date` to a UTC-derived string) drifts by a
/// day near midnight on timezones east of UTC.
///
/// `date_offset` is fed to SQLite's `date('now', 'localtime', ?)` modifier
/// so the comparison stays in the same timezone as the stored value.
/// `cutoff_ts` is used against `downloads.created_at` (Unix seconds,
/// timezone-agnostic).
///
/// For `AllTime` both fields are `None` so the SQL queries degrade to
/// `WHERE 1 = 1` equivalents.
struct PeriodCutoff {
    /// `created_at >= cutoff_ts` on the `downloads` / history tables.
    cutoff_ts: Option<i64>,
    /// Relative offset passed to `date('now','localtime', ?)` (e.g. `"-7 days"`).
    date_offset: Option<&'static str>,
}

fn period_cutoff(period: StatsPeriod, now_ts: u64) -> PeriodCutoff {
    let Some(days) = period.window_days() else {
        return PeriodCutoff {
            cutoff_ts: None,
            date_offset: None,
        };
    };
    let window = u64::from(days).saturating_mul(SECONDS_PER_DAY);
    let ts = now_ts.saturating_sub(window);
    let offset = match period {
        StatsPeriod::Last7Days => "-7 days",
        StatsPeriod::Last30Days => "-30 days",
        // Unreachable: AllTime returned early via `window_days() -> None`.
        StatsPeriod::AllTime => "-0 days",
    };
    PeriodCutoff {
        cutoff_ts: Some(ts as i64),
        date_offset: Some(offset),
    }
}

fn current_unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl StatsRepository for SqliteStatsRepo {
    fn record_completed(&self, bytes: u64, avg_speed: u64) -> Result<(), DomainError> {
        block_on(async {
            // peak_speed stores the highest per-download avg_speed seen that day
            // (the trait only exposes avg_speed, not instantaneous peak).
            let sql = "\
                INSERT INTO statistics (id, date, bytes_downloaded, files_completed, avg_speed, peak_speed) \
                VALUES (CAST(strftime('%Y%m%d', 'now', 'localtime') AS INTEGER), date('now', 'localtime'), ?1, 1, ?2, ?2) \
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

    fn get_stats(&self, period: StatsPeriod) -> Result<StatsView, DomainError> {
        let cutoff = period_cutoff(period, current_unix_seconds());

        block_on(async {
            // 1. Totals from the `statistics` daily rollup (bounded by date).
            // `statistics.date` is written with SQLite `date('now','localtime')`
            // so the filter must evaluate the cutoff in the same timezone —
            // a UTC-derived string would drift a day near midnight on
            // timezones east of UTC.
            let (totals_sql, totals_values) = match cutoff.date_offset {
                Some(offset) => (
                    "SELECT \
                      COALESCE(SUM(bytes_downloaded),0), \
                      COALESCE(SUM(files_completed),0), \
                      CASE WHEN SUM(files_completed)>0 \
                        THEN CAST(SUM(CAST(avg_speed AS REAL)*files_completed)/SUM(files_completed) AS INTEGER) \
                        ELSE 0 END, \
                      COALESCE(MAX(peak_speed),0) \
                    FROM statistics WHERE date >= date('now','localtime',?1)",
                    vec![sea_orm::Value::String(Some(Box::new(offset.to_string())))],
                ),
                None => (
                    "SELECT \
                      COALESCE(SUM(bytes_downloaded),0), \
                      COALESCE(SUM(files_completed),0), \
                      CASE WHEN SUM(files_completed)>0 \
                        THEN CAST(SUM(CAST(avg_speed AS REAL)*files_completed)/SUM(files_completed) AS INTEGER) \
                        ELSE 0 END, \
                      COALESCE(MAX(peak_speed),0) \
                    FROM statistics",
                    vec![],
                ),
            };

            let totals_row = self
                .db
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    totals_sql,
                    totals_values,
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

            // 2. Daily volumes (cap result set to 30 most recent rows within period).
            let (daily_sql, daily_values) = match cutoff.date_offset {
                Some(offset) => (
                    "SELECT date, bytes_downloaded, files_completed FROM statistics \
                     WHERE date >= date('now','localtime',?1) ORDER BY date DESC LIMIT 30",
                    vec![sea_orm::Value::String(Some(Box::new(offset.to_string())))],
                ),
                None => (
                    "SELECT date, bytes_downloaded, files_completed FROM statistics \
                     ORDER BY date DESC LIMIT 30",
                    vec![],
                ),
            };
            let daily_rows = self
                .db
                .query_all(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    daily_sql,
                    daily_values,
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

            // 3. Success rate computed from downloads.created_at within period.
            let (success_sql, success_values) = match cutoff.cutoff_ts {
                Some(ts) => (
                    "SELECT COUNT(*), COALESCE(SUM(CASE WHEN state='Completed' THEN 1 ELSE 0 END),0) \
                     FROM downloads WHERE state IN ('Completed','Error') AND created_at >= ?1",
                    vec![sea_orm::Value::BigInt(Some(ts))],
                ),
                None => (
                    "SELECT COUNT(*), COALESCE(SUM(CASE WHEN state='Completed' THEN 1 ELSE 0 END),0) \
                     FROM downloads WHERE state IN ('Completed','Error')",
                    vec![],
                ),
            };
            let success_row = self
                .db
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    success_sql,
                    success_values,
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

            // 4. Top hosts bounded by created_at.
            let (hosts_sql, hosts_values) = match cutoff.cutoff_ts {
                Some(ts) => (
                    "SELECT source_hostname, SUM(downloaded_bytes), COUNT(*) \
                     FROM downloads \
                     WHERE state = 'Completed' \
                       AND source_hostname IS NOT NULL AND source_hostname != '' \
                       AND created_at >= ?1 \
                     GROUP BY source_hostname \
                     ORDER BY 2 DESC LIMIT 10",
                    vec![sea_orm::Value::BigInt(Some(ts))],
                ),
                None => (
                    "SELECT source_hostname, SUM(downloaded_bytes), COUNT(*) \
                     FROM downloads \
                     WHERE state = 'Completed' \
                       AND source_hostname IS NOT NULL AND source_hostname != '' \
                     GROUP BY source_hostname \
                     ORDER BY 2 DESC LIMIT 10",
                    vec![],
                ),
            };
            let host_rows = self
                .db
                .query_all(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    hosts_sql,
                    hosts_values,
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

    fn top_modules(&self, limit: u32) -> Result<Vec<ModuleStats>, DomainError> {
        let limit = limit.max(1) as i64;
        block_on(async {
            let rows = self
                .db
                .query_all(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "SELECT module_name, COUNT(*), COALESCE(SUM(downloaded_bytes),0) \
                     FROM downloads \
                     WHERE state = 'Completed' \
                       AND module_name IS NOT NULL AND module_name != '' \
                     GROUP BY module_name \
                     ORDER BY 2 DESC, 3 DESC \
                     LIMIT ?1",
                    [sea_orm::Value::BigInt(Some(limit))],
                ))
                .await
                .map_err(map_db_err)?;

            let mut modules = Vec::with_capacity(rows.len());
            for row in rows {
                let name: String = row
                    .try_get_by_index(0)
                    .map_err(|e| DomainError::StorageError(e.to_string()))?;
                let count: i64 = row
                    .try_get_by_index(1)
                    .map_err(|e| DomainError::StorageError(e.to_string()))?;
                let bytes: i64 = row
                    .try_get_by_index(2)
                    .map_err(|e| DomainError::StorageError(e.to_string()))?;
                modules.push(ModuleStats {
                    module_name: name,
                    download_count: safe_u64(count),
                    total_bytes: safe_u64(bytes),
                });
            }
            Ok(modules)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::connection::setup_test_db;
    use super::*;
    use sea_orm::{ConnectionTrait, Statement};

    #[test]
    fn test_period_cutoff_all_time_has_no_bound() {
        let cutoff = period_cutoff(StatsPeriod::AllTime, 1_768_435_200);
        assert!(cutoff.cutoff_ts.is_none());
        assert!(cutoff.date_offset.is_none());
    }

    #[test]
    fn test_period_cutoff_7d_shifts_by_7_days() {
        let now: u64 = 1_768_435_200; // 2026-01-15
        let cutoff = period_cutoff(StatsPeriod::Last7Days, now);
        assert_eq!(cutoff.cutoff_ts, Some((now - 7 * SECONDS_PER_DAY) as i64));
        assert_eq!(cutoff.date_offset, Some("-7 days"));
    }

    #[test]
    fn test_period_cutoff_30d_shifts_by_30_days() {
        let now: u64 = 1_768_435_200;
        let cutoff = period_cutoff(StatsPeriod::Last30Days, now);
        assert_eq!(cutoff.cutoff_ts, Some((now - 30 * SECONDS_PER_DAY) as i64));
        assert_eq!(cutoff.date_offset, Some("-30 days"));
    }

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

        let stats = repo.get_stats(StatsPeriod::AllTime).expect("get_stats");
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
        let stats = repo.get_stats(StatsPeriod::AllTime).expect("get_stats");

        assert_eq!(stats.daily_volumes.len(), 2);
        // DESC order: most recent first
        assert_eq!(stats.daily_volumes[0].date, "2026-01-02");
        assert_eq!(stats.daily_volumes[0].bytes, 3000);
        assert_eq!(stats.daily_volumes[1].date, "2026-01-01");
        assert_eq!(stats.daily_volumes[1].bytes, 2000);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_get_stats_7d_excludes_older_statistics_rows() {
        let db = setup_test_db().await.expect("failed to setup test db");

        // Compute today's date string inside SQLite so the filter boundary
        // matches the implementation's clock. `localtime` mirrors the
        // modifier used by both `record_completed` and the period filter,
        // so the test is stable across timezones.
        let today_row = db
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT date('now','localtime'), date('now','localtime','-10 days')".to_string(),
            ))
            .await
            .unwrap()
            .expect("row");
        let today: String = today_row.try_get_by_index(0).unwrap();
        let ten_days_ago: String = today_row.try_get_by_index(1).unwrap();

        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO statistics (id, date, bytes_downloaded, files_completed, avg_speed, peak_speed) \
             VALUES (1, ?1, 1000, 1, 100, 200)",
            [sea_orm::Value::String(Some(Box::new(ten_days_ago.clone())))],
        ))
        .await
        .expect("insert old row");

        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO statistics (id, date, bytes_downloaded, files_completed, avg_speed, peak_speed) \
             VALUES (2, ?1, 3000, 3, 300, 400)",
            [sea_orm::Value::String(Some(Box::new(today.clone())))],
        ))
        .await
        .expect("insert today row");

        let repo = SqliteStatsRepo::new(db);

        let stats7 = repo.get_stats(StatsPeriod::Last7Days).expect("7d");
        assert_eq!(stats7.total_files, 3);
        assert_eq!(stats7.total_downloaded_bytes, 3000);
        assert_eq!(stats7.daily_volumes.len(), 1);
        assert_eq!(stats7.daily_volumes[0].date, today);

        let stats_all = repo.get_stats(StatsPeriod::AllTime).expect("all");
        assert_eq!(stats_all.total_files, 4);
        assert_eq!(stats_all.total_downloaded_bytes, 4000);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_get_stats_success_rate() {
        let db = setup_test_db().await.expect("failed to setup test db");

        // Insert 3 terminal downloads: 2 Completed, 1 Error — all count for success rate
        for (id, state) in [(1i64, "Completed"), (2, "Completed"), (3, "Error")] {
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
        let stats = repo.get_stats(StatsPeriod::AllTime).expect("get_stats");

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
        let stats = repo.get_stats(StatsPeriod::AllTime).expect("get_stats");

        assert_eq!(stats.top_hosts.len(), 2);
        // host-a has more total bytes, should be first
        assert_eq!(stats.top_hosts[0].hostname, "host-a.com");
        assert_eq!(stats.top_hosts[0].total_bytes, 10000);
        assert_eq!(stats.top_hosts[0].download_count, 3);
        assert_eq!(stats.top_hosts[1].hostname, "host-b.com");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_top_modules_empty_db() {
        let db = setup_test_db().await.expect("failed to setup test db");
        let repo = SqliteStatsRepo::new(db);

        let modules = repo.top_modules(10).expect("top_modules");
        assert!(modules.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_top_modules_groups_and_orders_by_count() {
        let db = setup_test_db().await.expect("failed to setup test db");

        // 3 YouTube (big bytes), 2 SoundCloud, 1 Null module (ignored).
        let rows = [
            (1i64, Some("vortex-mod-youtube"), 5000i64),
            (2, Some("vortex-mod-youtube"), 3000),
            (3, Some("vortex-mod-youtube"), 2000),
            (4, Some("vortex-mod-soundcloud"), 1000),
            (5, Some("vortex-mod-soundcloud"), 500),
            (6, None, 100),
        ];
        for (id, module, bytes) in rows {
            let module_expr = match module {
                Some(name) => format!("'{name}'"),
                None => "NULL".to_string(),
            };
            db.execute(Statement::from_string(
                DatabaseBackend::Sqlite,
                format!(
                    "INSERT INTO downloads \
                     (id, url, file_name, state, priority, downloaded_bytes, speed_bytes_per_sec, \
                      retry_count, max_retries, segments_count, resume_supported, \
                      source_hostname, protocol, module_name, destination_path, created_at, updated_at) \
                     VALUES ({id}, 'https://example.com/{id}', 'file{id}.zip', 'Completed', 5, \
                             {bytes}, 0, 0, 5, 1, 0, 'example.com', 'https', {module_expr}, '/tmp', 1000, 2000)"
                ),
            ))
            .await
            .expect("insert download");
        }

        let repo = SqliteStatsRepo::new(db);
        let modules = repo.top_modules(5).expect("top_modules");

        assert_eq!(modules.len(), 2);
        assert_eq!(modules[0].module_name, "vortex-mod-youtube");
        assert_eq!(modules[0].download_count, 3);
        assert_eq!(modules[0].total_bytes, 10_000);
        assert_eq!(modules[1].module_name, "vortex-mod-soundcloud");
        assert_eq!(modules[1].download_count, 2);
        assert_eq!(modules[1].total_bytes, 1_500);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_top_modules_respects_limit() {
        let db = setup_test_db().await.expect("failed to setup test db");

        for (id, module) in [
            (1i64, "alpha"),
            (2, "alpha"),
            (3, "alpha"),
            (4, "beta"),
            (5, "beta"),
            (6, "gamma"),
        ] {
            db.execute(Statement::from_string(
                DatabaseBackend::Sqlite,
                format!(
                    "INSERT INTO downloads \
                     (id, url, file_name, state, priority, downloaded_bytes, speed_bytes_per_sec, \
                      retry_count, max_retries, segments_count, resume_supported, \
                      source_hostname, protocol, module_name, destination_path, created_at, updated_at) \
                     VALUES ({id}, 'https://example.com/{id}', 'file{id}.zip', 'Completed', 5, \
                             100, 0, 0, 5, 1, 0, 'example.com', 'https', '{module}', '/tmp', 1000, 2000)"
                ),
            ))
            .await
            .expect("insert download");
        }

        let repo = SqliteStatsRepo::new(db);
        let modules = repo.top_modules(2).expect("top_modules");
        assert_eq!(modules.len(), 2);
        assert_eq!(modules[0].module_name, "alpha");
        assert_eq!(modules[1].module_name, "beta");
    }
}
