//! Handler for [`DeleteAccountCommand`](super::DeleteAccountCommand).
//!
//! Idempotent: succeeds without errors when neither the SQLite row nor
//! the keyring entry exists. Always emits
//! [`DomainEvent::AccountDeleted`] so the queue manager and read-model
//! caches can drop any state keyed by the id.
//!
//! The SQLite row is the canonical source of truth for "account
//! exists". We delete it first; from the user's perspective the account
//! is gone the moment that succeeds. The keyring secret is best-effort
//! cleanup — if the OS keyring rejects the delete (locked keychain,
//! permission denied) we log the orphan and still emit the deletion
//! event so the rest of the system drops its state.

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;

impl CommandBus {
    pub async fn handle_delete_account(
        &self,
        cmd: super::DeleteAccountCommand,
    ) -> Result<(), AppError> {
        let repo = self
            .account_repo()
            .ok_or_else(|| AppError::Validation("account repository not configured".into()))?;
        let store = self.account_credential_store().ok_or_else(|| {
            AppError::Validation("account credential store not configured".into())
        })?;

        repo.delete(&cmd.id)?;
        if let Err(e) = store.delete_password(&cmd.id) {
            tracing::warn!(
                account_id = %cmd.id.as_str(),
                error = %e,
                "failed to delete keyring password for deleted account; orphan secret may remain"
            );
        }

        self.event_bus()
            .publish(DomainEvent::AccountDeleted { id: cmd.id });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::{AddAccountCommand, DeleteAccountCommand};
    use crate::application::commands::tests_support::{
        CapturingEventBus, FakeAccountCredentialStore, InMemoryAccountRepo, build_account_bus,
    };
    use crate::domain::event::DomainEvent;
    use crate::domain::model::account::{AccountId, AccountType};
    use crate::domain::ports::driven::{AccountCredentialStore, AccountRepository};

    fn add_command() -> AddAccountCommand {
        AddAccountCommand {
            service_name: "real-debrid".into(),
            username: "alice".into(),
            password: "pw".into(),
            account_type: AccountType::Premium,
            created_at_ms: 1_700_000_000_000,
        }
    }

    #[tokio::test]
    async fn test_delete_account_removes_repo_entry_and_keyring_password() {
        let repo = Arc::new(InMemoryAccountRepo::new());
        let creds = Arc::new(FakeAccountCredentialStore::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_account_bus(repo.clone(), creds.clone(), events.clone(), None, None);
        let id = bus.handle_add_account(add_command()).await.unwrap();

        bus.handle_delete_account(DeleteAccountCommand { id: id.clone() })
            .await
            .expect("delete ok");

        assert!(repo.find_by_id(&id).unwrap().is_none());
        assert!(creds.get_password(&id).unwrap().is_none());
        assert!(
            events
                .snapshot()
                .iter()
                .any(|e| matches!(e, DomainEvent::AccountDeleted { id: x } if x == &id))
        );
    }

    #[tokio::test]
    async fn test_delete_account_unknown_id_is_idempotent() {
        let repo = Arc::new(InMemoryAccountRepo::new());
        let creds = Arc::new(FakeAccountCredentialStore::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_account_bus(repo, creds, events.clone(), None, None);

        bus.handle_delete_account(DeleteAccountCommand {
            id: AccountId::new("ghost"),
        })
        .await
        .expect("idempotent delete");

        assert_eq!(events.snapshot().len(), 1, "still emits AccountDeleted");
    }
}
