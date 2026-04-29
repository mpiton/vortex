use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use sea_orm::SqlxSqliteConnector;
use sea_orm::sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};
use sea_orm_migration::MigratorTrait;

use super::migrations::Migrator;

pub async fn establish_connection(db_path: &Path) -> Result<DatabaseConnection, sea_orm::DbErr> {
    // Use filename() instead of URL interpolation — handles Windows paths,
    // non-UTF-8 paths, and URI-reserved characters safely.
    let sqlite_opts = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .pragma("foreign_keys", "ON");

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(8))
        .connect_with(sqlite_opts)
        .await
        .map_err(|e| sea_orm::DbErr::Conn(sea_orm::RuntimeErr::Internal(e.to_string())))?;

    let db = SqlxSqliteConnector::from_sqlx_sqlite_pool(pool);

    // WAL is a file-level property — setting it once applies to all connections.
    db.execute(Statement::from_string(
        sea_orm::DatabaseBackend::Sqlite,
        "PRAGMA journal_mode=WAL;".to_string(),
    ))
    .await?;

    Ok(db)
}

pub async fn run_migrations(db: &DatabaseConnection) -> Result<(), sea_orm::DbErr> {
    Migrator::up(db, None).await?;
    Ok(())
}

/// Create an in-memory SQLite database for tests, with migrations applied.
pub async fn setup_test_db() -> Result<DatabaseConnection, sea_orm::DbErr> {
    let sqlite_opts = SqliteConnectOptions::from_str("sqlite::memory:")
        .map_err(|e| sea_orm::DbErr::Conn(sea_orm::RuntimeErr::Internal(e.to_string())))?
        .pragma("foreign_keys", "ON");

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(sqlite_opts)
        .await
        .map_err(|e| sea_orm::DbErr::Conn(sea_orm::RuntimeErr::Internal(e.to_string())))?;

    let db = SqlxSqliteConnector::from_sqlx_sqlite_pool(pool);
    run_migrations(&db).await?;
    Ok(db)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::ConnectionTrait;

    #[tokio::test]
    async fn test_migration_creates_all_tables() {
        let db = setup_test_db().await.unwrap();
        let tables: Vec<sea_orm::QueryResult> = db
            .query_all(Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name != 'seaql_migrations' ORDER BY name"
                    .to_string(),
            ))
            .await
            .unwrap();

        let names: Vec<String> = tables
            .iter()
            .map(|r| r.try_get_by_index::<String>(0).unwrap())
            .collect();

        assert!(names.contains(&"downloads".to_string()));
        assert!(names.contains(&"download_segments".to_string()));
        assert!(names.contains(&"packages".to_string()));
        assert!(names.contains(&"history".to_string()));
        assert!(names.contains(&"plugins".to_string()));
        assert!(names.contains(&"statistics".to_string()));
        assert!(names.contains(&"accounts".to_string()));
    }

    #[tokio::test]
    async fn test_accounts_migration_applies_cleanly_on_existing_db() {
        // Stand up a DB at the schema state immediately before the
        // accounts migration (5 migrations applied), seed prior tables,
        // then run the remaining migrations and verify the new table
        // exists and existing data is preserved.
        let sqlite_opts = sea_orm::sqlx::sqlite::SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .pragma("foreign_keys", "ON");
        let pool = sea_orm::sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(sqlite_opts)
            .await
            .unwrap();
        let db = sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(pool);

        Migrator::up(&db, Some(5))
            .await
            .expect("first 5 migrations");

        let pre = db
            .query_all(Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                "SELECT name FROM sqlite_master WHERE type='table' AND name='accounts'".to_string(),
            ))
            .await
            .unwrap();
        assert!(
            pre.is_empty(),
            "accounts table must not exist before its migration"
        );

        // Seed a download row that must survive the migration.
        db.execute(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "INSERT INTO downloads (id, url, file_name, state, priority, queue_position, downloaded_bytes, speed_bytes_per_sec, retry_count, max_retries, segments_count, source_hostname, protocol, resume_supported, destination_path, created_at, updated_at) VALUES (1, 'https://example.com/f.zip', 'f.zip', 'Queued', 5, 0, 0, 0, 0, 5, 1, 'example.com', 'https', 0, '/tmp', 1, 1)"
                .to_string(),
        ))
        .await
        .expect("seed download");

        Migrator::up(&db, None).await.expect("remaining migrations");

        let post = db
            .query_all(Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                "SELECT name FROM sqlite_master WHERE type='table' AND name='accounts'".to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(post.len(), 1, "accounts table created by migration");

        let downloads = db
            .query_all(Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                "SELECT id FROM downloads".to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(downloads.len(), 1, "existing data preserved");
    }

    #[tokio::test]
    async fn test_accounts_table_enforces_unique_service_username() {
        let db = setup_test_db().await.unwrap();
        let now: i64 = 1_700_000_000_000;
        db.execute(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            format!(
                "INSERT INTO accounts (id, service_name, username, account_type, enabled, created_at) VALUES ('a1', 'real-debrid', 'alice', 'debrid', 1, {now})"
            ),
        ))
        .await
        .expect("insert first");

        let dup_err = db
            .execute(Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                format!(
                    "INSERT INTO accounts (id, service_name, username, account_type, enabled, created_at) VALUES ('a2', 'real-debrid', 'alice', 'debrid', 1, {now})"
                ),
            ))
            .await
            .expect_err("UNIQUE(service_name, username) must reject");
        assert!(
            dup_err.to_string().to_ascii_lowercase().contains("unique"),
            "expected UNIQUE constraint error, got: {dup_err}"
        );

        let other = db
            .execute(Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                format!(
                    "INSERT INTO accounts (id, service_name, username, account_type, enabled, created_at) VALUES ('a2', 'alldebrid', 'alice', 'debrid', 1, {now})"
                ),
            ))
            .await;
        assert!(other.is_ok(), "different service must be allowed");
    }

    #[tokio::test]
    async fn test_packages_migration_applies_cleanly_on_existing_db() {
        // Stand up a DB at the schema state immediately before the
        // packages migration (6 migrations applied), seed prior tables,
        // then run the remaining migrations and verify the new schema
        // exists and existing data is preserved.
        let sqlite_opts = sea_orm::sqlx::sqlite::SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .pragma("foreign_keys", "ON");
        let pool = sea_orm::sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(sqlite_opts)
            .await
            .unwrap();
        let db = sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(pool);

        Migrator::up(&db, Some(6))
            .await
            .expect("first 6 migrations");

        // Seed a download row that must survive the migration.
        db.execute(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "INSERT INTO downloads (id, url, file_name, state, priority, queue_position, downloaded_bytes, speed_bytes_per_sec, retry_count, max_retries, segments_count, source_hostname, protocol, resume_supported, destination_path, created_at, updated_at) VALUES (1, 'https://example.com/f.zip', 'f.zip', 'Queued', 5, 0, 0, 0, 0, 5, 1, 'example.com', 'https', 0, '/tmp', 1, 1)"
                .to_string(),
        ))
        .await
        .expect("seed download");

        Migrator::up(&db, None).await.expect("remaining migrations");

        // packages table replaced with the new schema.
        let cols = db
            .query_all(Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                "PRAGMA table_info(packages)".to_string(),
            ))
            .await
            .unwrap();
        let names: Vec<String> = cols
            .iter()
            .map(|r| r.try_get_by_index::<String>(1).unwrap())
            .collect();
        for required in [
            "id",
            "name",
            "source_type",
            "folder_path",
            "password",
            "auto_extract",
            "priority",
            "created_at",
        ] {
            assert!(
                names.iter().any(|n| n == required),
                "packages must have column '{required}', got: {names:?}"
            );
        }

        // downloads gained the package_id FK column and its index.
        let dl_cols = db
            .query_all(Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                "PRAGMA table_info(downloads)".to_string(),
            ))
            .await
            .unwrap();
        let dl_names: Vec<String> = dl_cols
            .iter()
            .map(|r| r.try_get_by_index::<String>(1).unwrap())
            .collect();
        assert!(
            dl_names.iter().any(|n| n == "package_id"),
            "downloads must expose 'package_id', got: {dl_names:?}"
        );

        let indexes = db
            .query_all(Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                "SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='downloads'"
                    .to_string(),
            ))
            .await
            .unwrap();
        let idx_names: Vec<String> = indexes
            .iter()
            .map(|r| r.try_get_by_index::<String>(0).unwrap())
            .collect();
        assert!(
            idx_names.iter().any(|n| n == "idx_downloads_package"),
            "expected idx_downloads_package, got: {idx_names:?}"
        );

        // Existing data preserved.
        let downloads = db
            .query_all(Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                "SELECT id FROM downloads".to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(downloads.len(), 1, "existing download row preserved");
    }

    #[tokio::test]
    async fn test_wal_mode_enabled() {
        let test_id = std::process::id();
        let dir = std::env::temp_dir().join(format!("vortex_test_wal_{test_id}"));
        let _ = std::fs::create_dir_all(&dir);
        let db_path = dir.join("test.db");
        let _ = std::fs::remove_file(&db_path);

        let db = establish_connection(&db_path).await.unwrap();

        let result = db
            .query_one(Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                "PRAGMA journal_mode;".to_string(),
            ))
            .await
            .unwrap()
            .unwrap();

        let mode: String = result.try_get_by_index(0).unwrap();
        assert_eq!(mode.to_lowercase(), "wal");

        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_foreign_keys_enabled() {
        let db = setup_test_db().await.unwrap();

        let result = db
            .query_one(Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                "PRAGMA foreign_keys;".to_string(),
            ))
            .await
            .unwrap()
            .unwrap();

        let fk_enabled: i32 = result.try_get_by_index(0).unwrap();
        assert_eq!(fk_enabled, 1, "foreign_keys should be ON");
    }
}
