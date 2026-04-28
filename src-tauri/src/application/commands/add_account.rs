//! Handler for [`AddAccountCommand`](super::AddAccountCommand).
//!
//! Generates a fresh [`AccountId`] (UUID v4), persists the metadata
//! through [`AccountRepository`], stores the password through
//! [`AccountCredentialStore`], and emits
//! [`DomainEvent::AccountAdded`] on success.
//!
//! Inputs are trimmed and validated before any I/O so a bad payload
//! never reaches the keyring or SQLite.

use uuid::Uuid;

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;
use crate::domain::model::account::{Account, AccountId};

impl CommandBus {
    pub async fn handle_add_account(
        &self,
        cmd: super::AddAccountCommand,
    ) -> Result<AccountId, AppError> {
        let service_name = trim_required(&cmd.service_name, "service_name")?;
        let username = trim_required(&cmd.username, "username")?;
        if cmd.password.is_empty() {
            return Err(AppError::Validation("password must not be empty".into()));
        }

        let repo = self
            .account_repo()
            .ok_or_else(|| AppError::Validation("account repository not configured".into()))?;
        let store = self.account_credential_store().ok_or_else(|| {
            AppError::Validation("account credential store not configured".into())
        })?;

        let id = AccountId::new(Uuid::new_v4().to_string());
        let account = Account::new(
            id.clone(),
            service_name,
            username,
            cmd.account_type,
            cmd.created_at_ms,
        );

        // Persist the metadata first; the keyring write only matters
        // when the row exists. If the keyring step fails we roll back
        // by deleting the row so we never end up with a metadata-only
        // account whose password is missing.
        repo.save(&account)?;
        if let Err(e) = store.store_password(&id, &cmd.password) {
            let _ = repo.delete(&id);
            return Err(e.into());
        }

        self.event_bus().publish(DomainEvent::AccountAdded {
            id: id.clone(),
            service_name: account.service_name().to_string(),
        });

        Ok(id)
    }
}

fn trim_required(value: &str, field: &str) -> Result<String, AppError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::Validation(format!("{field} must not be empty")));
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::AddAccountCommand;
    use crate::application::commands::tests_support::{
        CapturingEventBus, FakeAccountCredentialStore, InMemoryAccountRepo, build_account_bus,
    };
    use crate::application::error::AppError;
    use crate::domain::error::DomainError;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::account::AccountType;
    use crate::domain::ports::driven::{AccountCredentialStore, AccountRepository};

    fn add_command(service: &str, user: &str, password: &str) -> AddAccountCommand {
        AddAccountCommand {
            service_name: service.into(),
            username: user.into(),
            password: password.into(),
            account_type: AccountType::Premium,
            created_at_ms: 1_700_000_000_000,
        }
    }

    #[tokio::test]
    async fn test_add_account_persists_account_and_password() {
        let repo = Arc::new(InMemoryAccountRepo::new());
        let creds = Arc::new(FakeAccountCredentialStore::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_account_bus(repo.clone(), creds.clone(), events.clone(), None, None);

        let id = bus
            .handle_add_account(add_command("real-debrid", "alice", "s3cret"))
            .await
            .expect("add ok");

        let stored = repo.find_by_id(&id).unwrap().expect("present");
        assert_eq!(stored.service_name(), "real-debrid");
        assert_eq!(stored.username(), "alice");
        assert_eq!(stored.account_type(), AccountType::Premium);
        assert_eq!(stored.created_at(), 1_700_000_000_000);

        assert_eq!(
            creds.get_password(&id).unwrap().as_deref(),
            Some("s3cret"),
            "password must land in the keyring under the new account id"
        );

        let events = events.snapshot();
        assert_eq!(events.len(), 1);
        match &events[0] {
            DomainEvent::AccountAdded {
                id: ev_id,
                service_name,
            } => {
                assert_eq!(ev_id, &id);
                assert_eq!(service_name, "real-debrid");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_add_account_blank_service_returns_validation() {
        let repo = Arc::new(InMemoryAccountRepo::new());
        let creds = Arc::new(FakeAccountCredentialStore::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_account_bus(repo.clone(), creds.clone(), events, None, None);

        let err = bus
            .handle_add_account(add_command("   ", "alice", "pw"))
            .await
            .expect_err("blank service rejected");
        assert!(matches!(err, AppError::Validation(_)));
        assert!(repo.list().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_add_account_empty_password_rejected() {
        let repo = Arc::new(InMemoryAccountRepo::new());
        let creds = Arc::new(FakeAccountCredentialStore::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_account_bus(repo.clone(), creds.clone(), events, None, None);

        let err = bus
            .handle_add_account(add_command("real-debrid", "alice", ""))
            .await
            .expect_err("empty password rejected");
        assert!(matches!(err, AppError::Validation(_)));
        assert!(repo.list().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_add_account_duplicate_returns_already_exists_and_no_keyring_leak() {
        let repo = Arc::new(InMemoryAccountRepo::new());
        let creds = Arc::new(FakeAccountCredentialStore::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_account_bus(repo.clone(), creds.clone(), events, None, None);

        bus.handle_add_account(add_command("real-debrid", "alice", "pw1"))
            .await
            .expect("first ok");

        let err = bus
            .handle_add_account(add_command("real-debrid", "alice", "pw2"))
            .await
            .expect_err("duplicate must fail");

        assert!(
            matches!(err, AppError::Domain(DomainError::AlreadyExists(_))),
            "unexpected error: {err:?}"
        );
        assert_eq!(creds.entry_count(), 1, "second password must not be stored");
    }

    #[tokio::test]
    async fn test_add_account_rolls_back_when_keyring_fails() {
        let repo = Arc::new(InMemoryAccountRepo::new());
        let creds = Arc::new(FakeAccountCredentialStore::new().failing_on_write());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_account_bus(repo.clone(), creds.clone(), events.clone(), None, None);

        let err = bus
            .handle_add_account(add_command("real-debrid", "alice", "pw"))
            .await
            .expect_err("keyring failure surfaces");
        assert!(matches!(
            err,
            AppError::Domain(DomainError::StorageError(_))
        ));

        assert!(
            repo.list().unwrap().is_empty(),
            "row must be rolled back when keyring write fails"
        );
        assert!(events.snapshot().is_empty(), "no event on failure");
    }

    #[tokio::test]
    async fn test_add_account_emits_no_event_when_repo_missing() {
        let creds = Arc::new(FakeAccountCredentialStore::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus =
            crate::application::commands::tests_support::bus_without_account_ports(events.clone());

        let err = bus
            .handle_add_account(add_command("real-debrid", "alice", "pw"))
            .await
            .expect_err("repo missing");
        assert!(matches!(err, AppError::Validation(_)));
        assert!(events.snapshot().is_empty());
        let _unused = creds; // keep clippy happy without a binding mismatch
    }
}
