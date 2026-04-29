//! SQLite implementation of `PackageRepository` (CQRS write side).

use sea_orm::{DatabaseConnection, EntityTrait, QueryOrder, sea_query::OnConflict};

use crate::adapters::driven::sqlite::entities::package;
use crate::adapters::driven::sqlite::util::{block_on, map_db_err, safe_u64};
use crate::domain::error::DomainError;
use crate::domain::model::download::DownloadId;
use crate::domain::model::package::{Package, PackageId};
use crate::domain::ports::driven::package_repository::PackageRepository;

pub struct SqlitePackageRepo {
    db: DatabaseConnection,
}

impl SqlitePackageRepo {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }
}

impl PackageRepository for SqlitePackageRepo {
    fn find_by_id(&self, id: &PackageId) -> Result<Option<Package>, DomainError> {
        let id_value = id.as_str().to_string();
        block_on(async {
            let model = package::Entity::find_by_id(id_value)
                .one(&self.db)
                .await
                .map_err(map_db_err)?;
            match model {
                Some(m) => Ok(Some(m.into_domain()?)),
                None => Ok(None),
            }
        })
    }

    fn save(&self, package: &Package) -> Result<(), DomainError> {
        let active = package::ActiveModel::from_domain(package)?;

        block_on(async {
            // Upsert by primary key. `created_at` is intentionally omitted
            // from the update column list so the original insertion
            // timestamp stays stable across subsequent saves — consistent
            // with the account repo's behavior and required for stable
            // list ordering.
            package::Entity::insert(active)
                .on_conflict(
                    OnConflict::column(package::Column::Id)
                        .update_columns([
                            package::Column::Name,
                            package::Column::SourceType,
                            package::Column::FolderPath,
                            package::Column::Password,
                            package::Column::AutoExtract,
                            package::Column::Priority,
                        ])
                        .to_owned(),
                )
                .exec(&self.db)
                .await
                .map_err(map_db_err)?;
            Ok(())
        })
    }

    fn list(&self) -> Result<Vec<Package>, DomainError> {
        block_on(async {
            let models = package::Entity::find()
                .order_by_asc(package::Column::CreatedAt)
                .order_by_asc(package::Column::Id)
                .all(&self.db)
                .await
                .map_err(map_db_err)?;
            models.into_iter().map(|m| m.into_domain()).collect()
        })
    }

    fn delete(&self, id: &PackageId) -> Result<(), DomainError> {
        let id_value = id.as_str().to_string();
        block_on(async {
            package::Entity::delete_by_id(id_value)
                .exec(&self.db)
                .await
                .map_err(map_db_err)?;
            Ok(())
        })
    }

    fn list_downloads(&self, id: &PackageId) -> Result<Vec<DownloadId>, DomainError> {
        // `download::Model` does not yet expose `package_id` as a typed
        // column (the FK was added in a later migration), so query via
        // raw SQL to keep this commit self-contained. Future tasks that
        // wire `package_id` into the download write path can swap this
        // for a typed `find().filter(...)` chain.
        use sea_orm::{ConnectionTrait, Statement};

        let id_value = id.as_str().to_string();
        block_on(async {
            let rows = self
                .db
                .query_all(Statement::from_sql_and_values(
                    sea_orm::DatabaseBackend::Sqlite,
                    "SELECT id FROM downloads WHERE package_id = $1 ORDER BY queue_position ASC, id ASC",
                    [id_value.into()],
                ))
                .await
                .map_err(map_db_err)?;

            rows.into_iter()
                .map(|row| {
                    row.try_get_by_index::<i64>(0)
                        .map(|raw| DownloadId(safe_u64(raw)))
                        .map_err(map_db_err)
                })
                .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::driven::sqlite::connection::setup_test_db;
    use crate::domain::model::package::{Package, PackageId, PackageSourceType};
    use sea_orm::{ConnectionTrait, Statement};

    fn make_package(id: &str, name: &str, source_type: PackageSourceType) -> Package {
        Package::new(
            PackageId::new(id),
            name.to_string(),
            source_type,
            1_700_000_000_000,
        )
    }

    /// Insert a minimal `downloads` row referencing a package id. Only the
    /// not-null columns required by the schema are populated — irrelevant
    /// fields default. Returns the inserted download id (i64).
    async fn insert_download_in_package(
        db: &sea_orm::DatabaseConnection,
        download_id: i64,
        queue_position: i64,
        package_id: Option<&str>,
    ) {
        let pkg = match package_id {
            Some(p) => format!("'{p}'"),
            None => "NULL".to_string(),
        };
        let sql = format!(
            "INSERT INTO downloads (id, url, file_name, state, priority, queue_position, downloaded_bytes, speed_bytes_per_sec, retry_count, max_retries, segments_count, source_hostname, protocol, resume_supported, destination_path, created_at, updated_at, package_id) VALUES ({download_id}, 'https://example.com/f.zip', 'f.zip', 'Queued', 5, {queue_position}, 0, 0, 0, 5, 1, 'example.com', 'https', 0, '/tmp', 1, 1, {pkg})"
        );
        db.execute(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            sql,
        ))
        .await
        .expect("seed download");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_save_and_find_package_round_trip_preserves_all_fields() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqlitePackageRepo::new(db);

        let mut pkg = make_package("pkg-1", "Holiday", PackageSourceType::Playlist);
        pkg.set_folder_path(Some("/tmp/holiday".to_string()));
        pkg.set_password(Some("keyring://pkg/holiday".to_string()));
        pkg.set_auto_extract(false);
        pkg.set_priority(9).expect("valid priority");

        repo.save(&pkg).expect("save");

        let found = repo
            .find_by_id(&PackageId::new("pkg-1"))
            .expect("find")
            .expect("package should exist");

        assert_eq!(found.id().as_str(), "pkg-1");
        assert_eq!(found.name(), "Holiday");
        assert_eq!(found.source_type(), PackageSourceType::Playlist);
        assert_eq!(found.folder_path(), Some("/tmp/holiday"));
        assert_eq!(found.password(), Some("keyring://pkg/holiday"));
        assert!(!found.auto_extract());
        assert_eq!(found.priority(), 9);
        assert_eq!(found.created_at(), 1_700_000_000_000);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_save_upsert_updates_existing_package() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqlitePackageRepo::new(db);

        let mut pkg = make_package("pkg-up", "Initial", PackageSourceType::Manual);
        repo.save(&pkg).expect("first save");

        pkg = Package::reconstruct(
            PackageId::new("pkg-up"),
            "Renamed".to_string(),
            PackageSourceType::Container,
            Some("/srv/x".to_string()),
            None,
            false,
            2,
            // Different created_at — must NOT overwrite the stored value.
            9_999_999_999_999,
        )
        .expect("valid priority");
        repo.save(&pkg).expect("upsert");

        let found = repo
            .find_by_id(&PackageId::new("pkg-up"))
            .expect("find")
            .expect("present");
        assert_eq!(found.name(), "Renamed");
        assert_eq!(found.source_type(), PackageSourceType::Container);
        assert_eq!(found.folder_path(), Some("/srv/x"));
        assert!(!found.auto_extract());
        assert_eq!(found.priority(), 2);
        assert_eq!(
            found.created_at(),
            1_700_000_000_000,
            "upsert must not rewrite created_at"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_by_id_not_found_returns_none() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqlitePackageRepo::new(db);
        let result = repo
            .find_by_id(&PackageId::new("missing"))
            .expect("find_by_id");
        assert!(result.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_list_returns_packages_ordered_by_created_at_then_id() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqlitePackageRepo::new(db);

        let a = Package::new(
            PackageId::new("a"),
            "A".to_string(),
            PackageSourceType::Manual,
            10,
        );
        let b = Package::new(
            PackageId::new("b"),
            "B".to_string(),
            PackageSourceType::Manual,
            10,
        );
        let c = Package::new(
            PackageId::new("c"),
            "C".to_string(),
            PackageSourceType::Manual,
            20,
        );
        repo.save(&c).unwrap();
        repo.save(&a).unwrap();
        repo.save(&b).unwrap();

        let listed = repo.list().expect("list");
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0].id().as_str(), "a");
        assert_eq!(listed[1].id().as_str(), "b");
        assert_eq!(listed[2].id().as_str(), "c");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_delete_removes_package() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqlitePackageRepo::new(db);

        repo.save(&make_package("pkg-del", "X", PackageSourceType::Manual))
            .expect("save");
        repo.delete(&PackageId::new("pkg-del")).expect("delete");

        let found = repo.find_by_id(&PackageId::new("pkg-del")).expect("find");
        assert!(found.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_delete_missing_package_is_noop() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqlitePackageRepo::new(db);
        repo.delete(&PackageId::new("ghost")).expect("delete");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_delete_package_sets_member_downloads_package_id_to_null() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqlitePackageRepo::new(db.clone());

        repo.save(&make_package("pkg-fk", "FK", PackageSourceType::Manual))
            .expect("save package");

        // Seed two downloads attached to the package.
        insert_download_in_package(&db, 1, 0, Some("pkg-fk")).await;
        insert_download_in_package(&db, 2, 1, Some("pkg-fk")).await;

        // Sanity: list_downloads sees both, ordered by queue_position.
        let members_before = repo.list_downloads(&PackageId::new("pkg-fk")).unwrap();
        assert_eq!(members_before, vec![DownloadId(1), DownloadId(2)]);

        repo.delete(&PackageId::new("pkg-fk")).expect("delete");

        // The downloads still exist — only the FK is cleared.
        let rows = db
            .query_all(Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                "SELECT id, package_id FROM downloads WHERE id IN (1, 2) ORDER BY id".to_string(),
            ))
            .await
            .expect("query downloads");
        assert_eq!(rows.len(), 2, "downloads must survive package deletion");
        for row in &rows {
            let pkg_id: Option<String> = row.try_get_by_index(1).unwrap();
            assert!(
                pkg_id.is_none(),
                "package_id must be NULL after package deletion (got {pkg_id:?})"
            );
        }

        // And list_downloads now returns empty for that package id.
        let members_after = repo.list_downloads(&PackageId::new("pkg-fk")).unwrap();
        assert!(members_after.is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_list_downloads_filters_by_package_id_and_orders_by_queue_position() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqlitePackageRepo::new(db.clone());

        repo.save(&make_package("pkg-ord", "Ord", PackageSourceType::Manual))
            .expect("save");

        // 3 downloads in pkg-ord with shuffled queue_position, plus one
        // unattached download that must NOT show up in the result.
        insert_download_in_package(&db, 100, 5, Some("pkg-ord")).await;
        insert_download_in_package(&db, 101, 1, Some("pkg-ord")).await;
        insert_download_in_package(&db, 102, 3, Some("pkg-ord")).await;
        insert_download_in_package(&db, 999, 0, None).await;

        let members = repo.list_downloads(&PackageId::new("pkg-ord")).unwrap();
        assert_eq!(
            members,
            vec![DownloadId(101), DownloadId(102), DownloadId(100)],
            "results ordered by queue_position ascending and exclude unattached downloads"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_list_downloads_returns_empty_for_unknown_package() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqlitePackageRepo::new(db);
        let members = repo
            .list_downloads(&PackageId::new("never-existed"))
            .unwrap();
        assert!(members.is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_source_type_round_trip_through_db_for_each_variant() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqlitePackageRepo::new(db);

        let kinds = [
            ("ct-id", PackageSourceType::Container),
            ("pl-id", PackageSourceType::Playlist),
            ("mn-id", PackageSourceType::Manual),
            ("sa-id", PackageSourceType::SplitArchive),
        ];
        for (id, src) in kinds {
            let pkg = Package::new(PackageId::new(id), "n".to_string(), src, 0);
            repo.save(&pkg).expect("save");
            let found = repo.find_by_id(&PackageId::new(id)).unwrap().unwrap();
            assert_eq!(found.source_type(), src);
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_by_id_returns_validation_error_on_unknown_source_type() {
        // Defensive path: a row whose source_type slipped past the
        // application layer (e.g. manual migration, dropped enum
        // variant) must surface as ValidationError, not panic.
        let db = setup_test_db().await.expect("test db");
        db.execute(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "INSERT INTO packages (id, name, source_type, auto_extract, priority, created_at) VALUES ('pkg-bad', 'Bad', 'unknown-type', 1, 5, 0)"
                .to_string(),
        ))
        .await
        .expect("seed bad row");

        let repo = SqlitePackageRepo::new(db);
        let err = repo
            .find_by_id(&PackageId::new("pkg-bad"))
            .expect_err("invalid source_type must fail");
        assert!(
            matches!(err, DomainError::ValidationError(_)),
            "expected ValidationError, got {err:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_by_id_returns_validation_error_when_priority_out_of_u8_range() {
        let db = setup_test_db().await.expect("test db");
        db.execute(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "INSERT INTO packages (id, name, source_type, auto_extract, priority, created_at) VALUES ('pkg-prio', 'Prio', 'manual', 1, 9999, 0)"
                .to_string(),
        ))
        .await
        .expect("seed bad priority");

        let repo = SqlitePackageRepo::new(db);
        let err = repo
            .find_by_id(&PackageId::new("pkg-prio"))
            .expect_err("priority overflow must fail");
        assert!(
            matches!(err, DomainError::ValidationError(_)),
            "expected ValidationError, got {err:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_by_id_rejects_priority_zero() {
        let db = setup_test_db().await.expect("test db");
        db.execute(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "INSERT INTO packages (id, name, source_type, auto_extract, priority, created_at) VALUES ('pkg-zero', 'Zero', 'manual', 1, 0, 0)"
                .to_string(),
        ))
        .await
        .expect("seed");

        let repo = SqlitePackageRepo::new(db);
        let err = repo
            .find_by_id(&PackageId::new("pkg-zero"))
            .expect_err("priority 0 must be rejected");
        assert!(matches!(err, DomainError::ValidationError(_)));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_by_id_rejects_negative_created_at() {
        // A corrupt row with a negative created_at must surface as
        // ValidationError instead of being silently coerced to 0 and
        // jumping to the front of the ordered list.
        let db = setup_test_db().await.expect("test db");
        db.execute(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "INSERT INTO packages (id, name, source_type, auto_extract, priority, created_at) VALUES ('pkg-neg', 'Neg', 'manual', 1, 5, -1)"
                .to_string(),
        ))
        .await
        .expect("seed");

        let repo = SqlitePackageRepo::new(db);
        let err = repo
            .find_by_id(&PackageId::new("pkg-neg"))
            .expect_err("negative created_at must be rejected");
        assert!(matches!(err, DomainError::ValidationError(_)));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_by_id_rejects_auto_extract_outside_zero_one() {
        let db = setup_test_db().await.expect("test db");
        db.execute(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "INSERT INTO packages (id, name, source_type, auto_extract, priority, created_at) VALUES ('pkg-ae', 'AE', 'manual', 7, 5, 0)"
                .to_string(),
        ))
        .await
        .expect("seed");

        let repo = SqlitePackageRepo::new(db);
        let err = repo
            .find_by_id(&PackageId::new("pkg-ae"))
            .expect_err("auto_extract=7 must be rejected");
        assert!(matches!(err, DomainError::ValidationError(_)));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_save_returns_validation_error_when_created_at_overflows_i64() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqlitePackageRepo::new(db);

        let pkg = Package::reconstruct(
            PackageId::new("pkg-of"),
            "Overflow".to_string(),
            PackageSourceType::Manual,
            None,
            None,
            true,
            5,
            // Beyond i64::MAX → must be rejected at conversion.
            u64::MAX,
        )
        .expect("valid priority");
        let err = repo.save(&pkg).expect_err("created_at overflow must fail");
        assert!(
            matches!(err, DomainError::ValidationError(_)),
            "expected ValidationError, got {err:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_optional_fields_persist_as_null_when_unset() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqlitePackageRepo::new(db);

        let pkg = make_package("pkg-null", "N", PackageSourceType::Manual);
        repo.save(&pkg).expect("save");

        let found = repo
            .find_by_id(&PackageId::new("pkg-null"))
            .unwrap()
            .unwrap();
        assert!(found.folder_path().is_none());
        assert!(found.password().is_none());
        // Defaults populated from `Package::new`.
        assert!(found.auto_extract());
        assert_eq!(found.priority(), 5);
    }
}
