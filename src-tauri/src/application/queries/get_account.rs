//! Handler for [`GetAccountQuery`].
//!
//! Returns a single account as an [`AccountViewDto`]. Returns
//! `AppError::NotFound` when the id does not match any persisted row.

use crate::application::error::AppError;
use crate::application::query_bus::QueryBus;
use crate::application::read_models::account_view::AccountViewDto;

impl QueryBus {
    pub async fn handle_get_account(
        &self,
        query: super::GetAccountQuery,
    ) -> Result<AccountViewDto, AppError> {
        let repo = self
            .account_repo()
            .ok_or_else(|| AppError::Validation("account repository not configured".into()))?;

        let account = repo
            .find_by_id(&query.id)?
            .ok_or_else(|| AppError::NotFound(format!("account {}", query.id.as_str())))?;

        Ok(account.into())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::application::error::AppError;
    use crate::application::queries::GetAccountQuery;
    use crate::application::test_support::{
        InMemoryAccountRepoForQueries, query_bus_with_accounts,
    };
    use crate::domain::model::account::{Account, AccountId, AccountType};
    use crate::domain::ports::driven::AccountRepository;

    fn populate_repo() -> Arc<InMemoryAccountRepoForQueries> {
        let repo = Arc::new(InMemoryAccountRepoForQueries::new());
        repo.save(&Account::new(
            AccountId::new("acc-1"),
            "real-debrid".to_string(),
            "alice".to_string(),
            AccountType::Premium,
            1_700_000_000_000,
        ))
        .unwrap();
        repo
    }

    #[tokio::test]
    async fn test_get_account_returns_dto_when_found() {
        let repo = populate_repo();
        let bus = query_bus_with_accounts(repo);
        let dto = bus
            .handle_get_account(GetAccountQuery {
                id: AccountId::new("acc-1"),
            })
            .await
            .unwrap();
        assert_eq!(dto.id, "acc-1");
        assert_eq!(dto.service_name, "real-debrid");
        assert_eq!(dto.username, "alice");
        assert_eq!(dto.account_type, "premium");
    }

    #[tokio::test]
    async fn test_get_account_returns_not_found_when_missing() {
        let repo = populate_repo();
        let bus = query_bus_with_accounts(repo);
        let err = bus
            .handle_get_account(GetAccountQuery {
                id: AccountId::new("ghost"),
            })
            .await
            .expect_err("ghost id");
        assert!(matches!(err, AppError::NotFound(msg) if msg.contains("ghost")));
    }

    #[tokio::test]
    async fn test_get_account_dto_omits_password_field() {
        let repo = populate_repo();
        let bus = query_bus_with_accounts(repo);
        let dto = bus
            .handle_get_account(GetAccountQuery {
                id: AccountId::new("acc-1"),
            })
            .await
            .unwrap();
        let value = serde_json::to_value(&dto).unwrap();
        let object = value.as_object().unwrap();
        assert!(!object.contains_key("password"));
    }

    #[tokio::test]
    async fn test_get_account_returns_validation_error_when_repo_missing() {
        let bus = crate::application::test_support::make_history_query_bus(Arc::new(
            crate::application::test_support::NoopHistoryRepo,
        ));
        let err = bus
            .handle_get_account(GetAccountQuery {
                id: AccountId::new("acc-1"),
            })
            .await
            .expect_err("missing repo");
        assert!(matches!(err, AppError::Validation(_)));
    }
}
