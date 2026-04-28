//! SQLite implementation of `AccountRepository` (CQRS write side).

use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, RuntimeErr,
    sea_query::OnConflict,
};

use crate::domain::error::DomainError;
use crate::domain::model::account::{Account, AccountId};
use crate::domain::ports::driven::account_repository::AccountRepository;

use super::entities::account;
use super::util::{block_on, map_db_err};

pub struct SqliteAccountRepo {
    db: DatabaseConnection,
}

impl SqliteAccountRepo {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }
}

impl AccountRepository for SqliteAccountRepo {
    fn find_by_id(&self, id: &AccountId) -> Result<Option<Account>, DomainError> {
        let id_value = id.as_str().to_string();
        block_on(async {
            let model = account::Entity::find_by_id(id_value)
                .one(&self.db)
                .await
                .map_err(map_db_err)?;
            match model {
                Some(m) => Ok(Some(m.into_domain()?)),
                None => Ok(None),
            }
        })
    }

    fn save(&self, account: &Account) -> Result<(), DomainError> {
        let active = account::ActiveModel::from_domain(account)?;

        block_on(async {
            // Upsert by primary key. The (service_name, username) UNIQUE
            // index lets the DB itself enforce the constraint and surface
            // it as a uniqueness violation we translate below. `created_at`
            // is intentionally omitted so the original insertion timestamp
            // stays stable across subsequent saves.
            let result = account::Entity::insert(active)
                .on_conflict(
                    OnConflict::column(account::Column::Id)
                        .update_columns([
                            account::Column::ServiceName,
                            account::Column::Username,
                            account::Column::AccountType,
                            account::Column::Enabled,
                            account::Column::TrafficLeft,
                            account::Column::TrafficTotal,
                            account::Column::ValidUntil,
                            account::Column::LastValidated,
                        ])
                        .to_owned(),
                )
                .exec(&self.db)
                .await;

            match result {
                Ok(_) => Ok(()),
                Err(sea_orm::DbErr::Exec(RuntimeErr::SqlxError(e)))
                    if is_unique_violation(&e.to_string()) =>
                {
                    Err(DomainError::AlreadyExists(format!(
                        "account ({}, {}) already exists",
                        account.service_name(),
                        account.username()
                    )))
                }
                Err(sea_orm::DbErr::Query(RuntimeErr::SqlxError(e)))
                    if is_unique_violation(&e.to_string()) =>
                {
                    Err(DomainError::AlreadyExists(format!(
                        "account ({}, {}) already exists",
                        account.service_name(),
                        account.username()
                    )))
                }
                Err(e) if is_unique_violation(&e.to_string()) => {
                    Err(DomainError::AlreadyExists(format!(
                        "account ({}, {}) already exists",
                        account.service_name(),
                        account.username()
                    )))
                }
                Err(e) => Err(map_db_err(e)),
            }
        })
    }

    fn list(&self) -> Result<Vec<Account>, DomainError> {
        block_on(async {
            let models = account::Entity::find()
                .order_by_asc(account::Column::CreatedAt)
                .order_by_asc(account::Column::Id)
                .all(&self.db)
                .await
                .map_err(map_db_err)?;
            models.into_iter().map(|m| m.into_domain()).collect()
        })
    }

    fn list_by_service(&self, service_name: &str) -> Result<Vec<Account>, DomainError> {
        let svc = service_name.to_string();
        block_on(async {
            let models = account::Entity::find()
                .filter(account::Column::ServiceName.eq(svc))
                .order_by_asc(account::Column::CreatedAt)
                .order_by_asc(account::Column::Id)
                .all(&self.db)
                .await
                .map_err(map_db_err)?;
            models.into_iter().map(|m| m.into_domain()).collect()
        })
    }

    fn delete(&self, id: &AccountId) -> Result<(), DomainError> {
        let id_value = id.as_str().to_string();
        block_on(async {
            account::Entity::delete_by_id(id_value)
                .exec(&self.db)
                .await
                .map_err(map_db_err)?;
            Ok(())
        })
    }
}

/// SQLite reports UNIQUE failures with one of these markers depending on
/// the driver path (sea-orm vs raw sqlx). Match either form so the
/// adapter doesn't depend on a specific error variant layout.
fn is_unique_violation(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("unique constraint failed")
        || lower.contains("constraint failed: unique")
        || (lower.contains("error code 2067") && lower.contains("unique"))
        || lower.contains("(2067)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::driven::sqlite::connection::setup_test_db;
    use crate::domain::model::account::{Account, AccountId, AccountType};

    fn make_account(id: &str, service: &str, user: &str) -> Account {
        Account::new(
            AccountId::new(id),
            service.to_string(),
            user.to_string(),
            AccountType::Debrid,
            1_700_000_000_000,
        )
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_save_and_find_account_round_trip_preserves_all_fields() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqliteAccountRepo::new(db);

        let mut account = make_account("acc-1", "real-debrid", "alice@example.com");
        account.set_traffic_left(123_456);
        account.set_traffic_total(1_000_000);
        account.set_valid_until(1_800_000_000_000);
        account.set_last_validated(1_700_001_000_000);
        account.disable();

        repo.save(&account).expect("save");

        let found = repo
            .find_by_id(&AccountId::new("acc-1"))
            .expect("find_by_id")
            .expect("account should exist");

        assert_eq!(found.id().as_str(), "acc-1");
        assert_eq!(found.service_name(), "real-debrid");
        assert_eq!(found.username(), "alice@example.com");
        assert_eq!(found.account_type(), AccountType::Debrid);
        assert!(!found.is_enabled());
        assert_eq!(found.traffic_left(), Some(123_456));
        assert_eq!(found.traffic_total(), Some(1_000_000));
        assert_eq!(found.valid_until(), Some(1_800_000_000_000));
        assert_eq!(found.last_validated(), Some(1_700_001_000_000));
        assert_eq!(found.created_at(), 1_700_000_000_000);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_save_upsert_updates_existing_account() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqliteAccountRepo::new(db);

        let mut account = make_account("acc-1", "real-debrid", "alice");
        repo.save(&account).expect("first save");

        account.disable();
        account.set_traffic_left(999);
        repo.save(&account).expect("upsert");

        let found = repo
            .find_by_id(&AccountId::new("acc-1"))
            .expect("find_by_id")
            .expect("present");
        assert!(!found.is_enabled());
        assert_eq!(found.traffic_left(), Some(999));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_save_upsert_preserves_original_created_at() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqliteAccountRepo::new(db);

        // First save with original timestamp.
        let original = Account::new(
            AccountId::new("acc-stable"),
            "real-debrid".to_string(),
            "alice".to_string(),
            AccountType::Debrid,
            1_700_000_000_000,
        );
        repo.save(&original).expect("first save");

        // Re-save the same id with a different created_at. It must NOT
        // overwrite the stored value, otherwise list ordering becomes
        // unstable across writes.
        let updated = Account::new(
            AccountId::new("acc-stable"),
            "real-debrid".to_string(),
            "alice".to_string(),
            AccountType::Debrid,
            9_999_999_999_999,
        );
        repo.save(&updated).expect("upsert");

        let found = repo
            .find_by_id(&AccountId::new("acc-stable"))
            .expect("find")
            .expect("present");
        assert_eq!(
            found.created_at(),
            1_700_000_000_000,
            "upsert must not rewrite created_at"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_find_by_id_not_found_returns_none() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqliteAccountRepo::new(db);
        let result = repo
            .find_by_id(&AccountId::new("missing"))
            .expect("find_by_id");
        assert!(result.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_list_returns_all_accounts_ordered_by_created_at() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqliteAccountRepo::new(db);

        let mut a = make_account("a", "real-debrid", "u1");
        let mut b = make_account("b", "alldebrid", "u2");
        // Force a deterministic created_at order: a < b
        a = Account::reconstruct(
            AccountId::new("a"),
            a.service_name().to_string(),
            a.username().to_string(),
            a.account_type(),
            a.is_enabled(),
            a.traffic_left(),
            a.traffic_total(),
            a.valid_until(),
            a.last_validated(),
            10,
        );
        b = Account::reconstruct(
            AccountId::new("b"),
            b.service_name().to_string(),
            b.username().to_string(),
            b.account_type(),
            b.is_enabled(),
            b.traffic_left(),
            b.traffic_total(),
            b.valid_until(),
            b.last_validated(),
            20,
        );

        repo.save(&b).expect("save b first");
        repo.save(&a).expect("save a second");

        let all = repo.list().expect("list");
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id().as_str(), "a");
        assert_eq!(all[1].id().as_str(), "b");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_list_by_service_filters_correctly() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqliteAccountRepo::new(db);

        repo.save(&make_account("rd-1", "real-debrid", "alice"))
            .expect("save rd-1");
        repo.save(&make_account("rd-2", "real-debrid", "bob"))
            .expect("save rd-2");
        repo.save(&make_account("ad-1", "alldebrid", "carol"))
            .expect("save ad-1");

        let rd = repo.list_by_service("real-debrid").expect("filter rd");
        assert_eq!(rd.len(), 2);
        for acc in &rd {
            assert_eq!(acc.service_name(), "real-debrid");
        }

        let ad = repo.list_by_service("alldebrid").expect("filter ad");
        assert_eq!(ad.len(), 1);
        assert_eq!(ad[0].id().as_str(), "ad-1");

        let none = repo.list_by_service("unknown").expect("filter unknown");
        assert!(none.is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_save_unique_violation_returns_already_exists() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqliteAccountRepo::new(db);

        repo.save(&make_account("first", "real-debrid", "alice"))
            .expect("save first");

        let dup = make_account("second", "real-debrid", "alice");
        let err = repo.save(&dup).expect_err("duplicate save must fail");
        assert!(
            matches!(err, DomainError::AlreadyExists(_)),
            "expected AlreadyExists, got {err:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_delete_removes_account() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqliteAccountRepo::new(db);

        repo.save(&make_account("acc-1", "real-debrid", "alice"))
            .expect("save");

        repo.delete(&AccountId::new("acc-1")).expect("delete");

        let found = repo
            .find_by_id(&AccountId::new("acc-1"))
            .expect("find_by_id");
        assert!(found.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_delete_missing_account_is_noop() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqliteAccountRepo::new(db);
        repo.delete(&AccountId::new("ghost")).expect("delete");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_account_type_round_trip_through_db_for_each_variant() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqliteAccountRepo::new(db);

        let kinds = [
            ("free-id", "free-host", AccountType::Free),
            ("prem-id", "prem-host", AccountType::Premium),
            ("deb-id", "deb-host", AccountType::Debrid),
        ];
        for (id, svc, t) in kinds {
            let acc = Account::new(AccountId::new(id), svc.to_string(), "u".to_string(), t, 0);
            repo.save(&acc).expect("save");
            let found = repo
                .find_by_id(&AccountId::new(id))
                .expect("find")
                .expect("present");
            assert_eq!(found.account_type(), t);
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_optional_fields_persist_as_null_when_unset() {
        let db = setup_test_db().await.expect("test db");
        let repo = SqliteAccountRepo::new(db);

        let acc = make_account("acc-null", "real-debrid", "u");
        repo.save(&acc).expect("save");

        let found = repo
            .find_by_id(&AccountId::new("acc-null"))
            .expect("find")
            .expect("present");
        assert!(found.traffic_left().is_none());
        assert!(found.traffic_total().is_none());
        assert!(found.valid_until().is_none());
        assert!(found.last_validated().is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_unique_violation_helper_recognises_sqlite_messages() {
        // The helper has to match a couple of slightly different wordings
        // depending on driver path; lock those in as regression guards.
        assert!(is_unique_violation("UNIQUE constraint failed: accounts.id"));
        assert!(is_unique_violation(
            "(code: 2067) UNIQUE constraint failed: accounts.service_name, accounts.username"
        ));
        assert!(!is_unique_violation("disk I/O error"));
    }
}
