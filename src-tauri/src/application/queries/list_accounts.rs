//! Handler for [`ListAccountsQuery`].
//!
//! Returns persisted accounts as [`AccountViewDto`] read models.
//! Filters AND together — service + type + enabled all match. The DTO
//! carries no password or raw secret material, so no plaintext secret
//! can leak through this read path. Non-secret identifiers (username,
//! opaque `credential_ref`) are present and intentional — only the
//! credential itself is fetched server-side via the keyring.

use crate::application::error::AppError;
use crate::application::query_bus::QueryBus;
use crate::application::read_models::account_view::AccountViewDto;
use crate::domain::model::account::Account;

impl QueryBus {
    pub async fn handle_list_accounts(
        &self,
        query: super::ListAccountsQuery,
    ) -> Result<Vec<AccountViewDto>, AppError> {
        let repo = self
            .account_repo()
            .ok_or_else(|| AppError::Validation("account repository not configured".into()))?;

        let accounts = match query.filter.as_ref().and_then(|f| f.service_name.as_ref()) {
            Some(service) => repo.list_by_service(service)?,
            None => repo.list()?,
        };

        let filtered: Vec<Account> = accounts
            .into_iter()
            .filter(|a| match &query.filter {
                None => true,
                Some(f) => {
                    f.account_type.is_none_or(|t| a.account_type() == t)
                        && f.enabled.is_none_or(|e| a.is_enabled() == e)
                }
            })
            .collect();

        Ok(filtered.into_iter().map(AccountViewDto::from).collect())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::application::queries::{AccountFilter, ListAccountsQuery};
    use crate::application::test_support::{
        InMemoryAccountRepoForQueries, query_bus_with_accounts,
    };
    use crate::domain::model::account::{Account, AccountId, AccountType};
    use crate::domain::ports::driven::AccountRepository;

    fn account(id: &str, service: &str, user: &str, ty: AccountType, created: u64) -> Account {
        Account::new(
            AccountId::new(id),
            service.to_string(),
            user.to_string(),
            ty,
            created,
        )
    }

    fn populate_default_repo() -> Arc<InMemoryAccountRepoForQueries> {
        let repo = Arc::new(InMemoryAccountRepoForQueries::new());
        repo.save(&account(
            "rd-1",
            "real-debrid",
            "alice",
            AccountType::Premium,
            1,
        ))
        .unwrap();
        repo.save(&account("rd-2", "real-debrid", "bob", AccountType::Free, 2))
            .unwrap();
        let mut disabled = account("ad-1", "alldebrid", "carol", AccountType::Debrid, 3);
        disabled.disable();
        repo.save(&disabled).unwrap();
        repo
    }

    #[tokio::test]
    async fn test_list_accounts_no_filter_returns_all_ordered_by_created_at() {
        let repo = populate_default_repo();
        let bus = query_bus_with_accounts(repo);
        let result = bus
            .handle_list_accounts(ListAccountsQuery::default())
            .await
            .unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].id, "rd-1");
        assert_eq!(result[1].id, "rd-2");
        assert_eq!(result[2].id, "ad-1");
    }

    #[tokio::test]
    async fn test_list_accounts_filter_by_service_only() {
        let repo = populate_default_repo();
        let bus = query_bus_with_accounts(repo);
        let result = bus
            .handle_list_accounts(ListAccountsQuery {
                filter: Some(AccountFilter {
                    service_name: Some("real-debrid".into()),
                    ..Default::default()
                }),
            })
            .await
            .unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|a| a.service_name == "real-debrid"));
    }

    #[tokio::test]
    async fn test_list_accounts_filter_combines_service_and_type() {
        let repo = populate_default_repo();
        let bus = query_bus_with_accounts(repo);
        let result = bus
            .handle_list_accounts(ListAccountsQuery {
                filter: Some(AccountFilter {
                    service_name: Some("real-debrid".into()),
                    account_type: Some(AccountType::Premium),
                    enabled: None,
                }),
            })
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "rd-1");
    }

    #[tokio::test]
    async fn test_list_accounts_filter_combines_type_and_enabled() {
        let repo = populate_default_repo();
        let bus = query_bus_with_accounts(repo);
        let result = bus
            .handle_list_accounts(ListAccountsQuery {
                filter: Some(AccountFilter {
                    service_name: None,
                    account_type: Some(AccountType::Debrid),
                    enabled: Some(false),
                }),
            })
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "ad-1");
    }

    #[tokio::test]
    async fn test_list_accounts_filter_enabled_only_excludes_disabled() {
        let repo = populate_default_repo();
        let bus = query_bus_with_accounts(repo);
        let result = bus
            .handle_list_accounts(ListAccountsQuery {
                filter: Some(AccountFilter {
                    enabled: Some(true),
                    ..Default::default()
                }),
            })
            .await
            .unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|a| a.enabled));
    }

    #[tokio::test]
    async fn test_list_accounts_filter_unknown_service_returns_empty() {
        let repo = populate_default_repo();
        let bus = query_bus_with_accounts(repo);
        let result = bus
            .handle_list_accounts(ListAccountsQuery {
                filter: Some(AccountFilter {
                    service_name: Some("ghost".into()),
                    ..Default::default()
                }),
            })
            .await
            .unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_list_accounts_returns_validation_error_when_repo_missing() {
        let bus = crate::application::test_support::make_history_query_bus(Arc::new(
            crate::application::test_support::NoopHistoryRepo,
        ));
        let err = bus
            .handle_list_accounts(ListAccountsQuery::default())
            .await
            .expect_err("missing repo");
        assert!(
            matches!(err, crate::application::error::AppError::Validation(msg) if msg.contains("account"))
        );
    }

    #[tokio::test]
    async fn test_list_accounts_dto_omits_password_field() {
        let repo = populate_default_repo();
        let bus = query_bus_with_accounts(repo);
        let result = bus
            .handle_list_accounts(ListAccountsQuery::default())
            .await
            .unwrap();
        for dto in &result {
            let value = serde_json::to_value(dto).unwrap();
            let object = value.as_object().unwrap();
            assert!(!object.contains_key("password"));
            assert!(!object.contains_key("credential"));
        }
    }
}
