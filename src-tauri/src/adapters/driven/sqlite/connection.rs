use sea_orm::SqlxSqliteConnector;
use sea_orm::sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};
use sea_orm_migration::MigratorTrait;
use std::str::FromStr;
use std::time::Duration;

use super::migrations::Migrator;

pub async fn establish_connection(db_path: &str) -> Result<DatabaseConnection, sea_orm::DbErr> {
    // Per-connection PRAGMA via SqliteConnectOptions ensures every pool
    // connection enforces FK constraints, not just the first one.
    let sqlite_opts = SqliteConnectOptions::from_str(&format!("sqlite://{}?mode=rwc", db_path))
        .map_err(|e| sea_orm::DbErr::Conn(sea_orm::RuntimeErr::Internal(e.to_string())))?
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
    }

    #[tokio::test]
    async fn test_wal_mode_enabled() {
        let test_id = std::process::id();
        let dir = std::env::temp_dir().join(format!("vortex_test_wal_{test_id}"));
        let _ = std::fs::create_dir_all(&dir);
        let db_path = dir.join("test.db");
        let _ = std::fs::remove_file(&db_path);

        let db = establish_connection(db_path.to_str().unwrap())
            .await
            .unwrap();

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
