use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseConnection, Statement};
use sea_orm_migration::MigratorTrait;
use std::time::Duration;

use super::migrations::Migrator;

pub async fn establish_connection(db_path: &str) -> Result<DatabaseConnection, sea_orm::DbErr> {
    let url = format!("sqlite://{}?mode=rwc", db_path);
    let mut opt = ConnectOptions::new(url);
    opt.max_connections(5)
        .connect_timeout(Duration::from_secs(8))
        .sqlx_logging(false);

    let db = Database::connect(opt).await?;

    db.execute(Statement::from_string(
        sea_orm::DatabaseBackend::Sqlite,
        "PRAGMA journal_mode=WAL;".to_string(),
    ))
    .await?;
    db.execute(Statement::from_string(
        sea_orm::DatabaseBackend::Sqlite,
        "PRAGMA foreign_keys=ON;".to_string(),
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
    let db = Database::connect("sqlite::memory:").await?;
    db.execute(Statement::from_string(
        sea_orm::DatabaseBackend::Sqlite,
        "PRAGMA foreign_keys=ON;".to_string(),
    ))
    .await?;
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
        let dir = std::env::temp_dir().join("vortex_test_wal");
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
}
