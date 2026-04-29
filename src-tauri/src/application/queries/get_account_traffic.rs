//! Handler for [`GetAccountTrafficQuery`].
//!
//! Reads the persisted traffic counters for one account. The "refresh
//! from upstream" step is deliberately not in this handler — that
//! mutates state and lives in the [`ValidateAccountCommand`](
//! crate::application::commands::ValidateAccountCommand) handler.
//! Splitting them this way keeps queries side-effect free, in line with
//! the project-wide CQRS rule.

use crate::application::error::AppError;
use crate::application::query_bus::QueryBus;
use crate::application::read_models::account_view::AccountTrafficDto;

impl QueryBus {
    pub async fn handle_get_account_traffic(
        &self,
        query: super::GetAccountTrafficQuery,
    ) -> Result<AccountTrafficDto, AppError> {
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
    use crate::application::queries::GetAccountTrafficQuery;
    use crate::application::test_support::{
        InMemoryAccountRepoForQueries, query_bus_with_accounts,
    };
    use crate::domain::model::account::{Account, AccountId, AccountType};
    use crate::domain::ports::driven::AccountRepository;

    #[tokio::test]
    async fn test_get_account_traffic_returns_persisted_counters() {
        let repo = Arc::new(InMemoryAccountRepoForQueries::new());
        let mut acc = Account::new(
            AccountId::new("acc-1"),
            "real-debrid".to_string(),
            "alice".to_string(),
            AccountType::Premium,
            1_700_000_000_000,
        );
        acc.set_traffic_left(50_000);
        acc.set_traffic_total(100_000);
        acc.set_valid_until(2_500_000_000_000);
        acc.set_last_validated(1_900_000_000_000);
        repo.save(&acc).unwrap();

        let bus = query_bus_with_accounts(repo);
        let dto = bus
            .handle_get_account_traffic(GetAccountTrafficQuery {
                id: AccountId::new("acc-1"),
            })
            .await
            .unwrap();
        assert_eq!(dto.id, "acc-1");
        assert_eq!(dto.traffic_left, Some(50_000));
        assert_eq!(dto.traffic_total, Some(100_000));
        assert_eq!(dto.valid_until, Some(2_500_000_000_000));
        assert_eq!(dto.last_validated, Some(1_900_000_000_000));
    }

    #[tokio::test]
    async fn test_get_account_traffic_returns_none_counters_when_unset() {
        let repo = Arc::new(InMemoryAccountRepoForQueries::new());
        repo.save(&Account::new(
            AccountId::new("acc-2"),
            "service".to_string(),
            "u".to_string(),
            AccountType::Free,
            0,
        ))
        .unwrap();

        let bus = query_bus_with_accounts(repo);
        let dto = bus
            .handle_get_account_traffic(GetAccountTrafficQuery {
                id: AccountId::new("acc-2"),
            })
            .await
            .unwrap();
        assert_eq!(dto.traffic_left, None);
        assert_eq!(dto.traffic_total, None);
        assert_eq!(dto.valid_until, None);
        assert_eq!(dto.last_validated, None);
    }

    #[tokio::test]
    async fn test_get_account_traffic_returns_not_found_when_missing() {
        let repo = Arc::new(InMemoryAccountRepoForQueries::new());
        let bus = query_bus_with_accounts(repo);
        let err = bus
            .handle_get_account_traffic(GetAccountTrafficQuery {
                id: AccountId::new("ghost"),
            })
            .await
            .expect_err("ghost id");
        assert!(matches!(err, AppError::NotFound(msg) if msg.contains("ghost")));
    }

    #[tokio::test]
    async fn test_get_account_traffic_returns_validation_error_when_repo_missing() {
        let bus = crate::application::test_support::make_history_query_bus(Arc::new(
            crate::application::test_support::NoopHistoryRepo,
        ));
        let err = bus
            .handle_get_account_traffic(GetAccountTrafficQuery {
                id: AccountId::new("acc-1"),
            })
            .await
            .expect_err("missing repo");
        assert!(matches!(err, AppError::Validation(_)));
    }
}
