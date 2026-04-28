//! Handler for [`UpdateAccountCommand`](super::UpdateAccountCommand).
//!
//! Applies a partial mutation to an existing account. Password rotation
//! is performed against the keyring; other fields update the SQLite row.
//! Each `None` in the [`AccountPatch`](super::AccountPatch) leaves the
//! corresponding column untouched.
//!
//! When the patch contains a non-empty string for `username` or
//! `password`, both are validated before any mutation lands so a bad
//! input never produces a partially-updated account.

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;
use crate::domain::model::account::Account;

impl CommandBus {
    pub async fn handle_update_account(
        &self,
        cmd: super::UpdateAccountCommand,
    ) -> Result<(), AppError> {
        let repo = self
            .account_repo()
            .ok_or_else(|| AppError::Validation("account repository not configured".into()))?;
        let store = self.account_credential_store().ok_or_else(|| {
            AppError::Validation("account credential store not configured".into())
        })?;

        let account = repo
            .find_by_id(&cmd.id)?
            .ok_or_else(|| AppError::NotFound(format!("account {} not found", cmd.id.as_str())))?;

        let username = match cmd.patch.username {
            Some(value) => {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    return Err(AppError::Validation("username must not be empty".into()));
                }
                trimmed.to_string()
            }
            None => account.username().to_string(),
        };
        let account_type = cmd.patch.account_type.unwrap_or(account.account_type());
        let enabled = cmd.patch.enabled.unwrap_or(account.is_enabled());

        if let Some(ref pw) = cmd.patch.password
            && pw.is_empty()
        {
            return Err(AppError::Validation("password must not be empty".into()));
        }

        let next = Account::reconstruct(
            account.id().clone(),
            account.service_name().to_string(),
            username,
            account_type,
            enabled,
            account.traffic_left(),
            account.traffic_total(),
            account.valid_until(),
            account.last_validated(),
            account.created_at(),
        );
        repo.save(&next)?;

        // Apply password rotation after the row is persisted. If the
        // keyring write fails we roll the row back to the original so
        // callers never observe a row that says "password rotated" while
        // the keyring still holds the previous secret.
        if let Some(pw) = cmd.patch.password
            && let Err(e) = store.store_password(&cmd.id, &pw)
        {
            if let Err(rollback_err) = repo.save(&account) {
                tracing::warn!(
                    account_id = %cmd.id.as_str(),
                    keyring_error = %e,
                    rollback_error = %rollback_err,
                    "keyring rotation failed and row rollback also failed; row metadata diverges from keyring"
                );
            }
            return Err(e.into());
        }

        self.event_bus()
            .publish(DomainEvent::AccountUpdated { id: cmd.id });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::{AccountPatch, AddAccountCommand, UpdateAccountCommand};
    use crate::application::commands::tests_support::{
        CapturingEventBus, FakeAccountCredentialStore, InMemoryAccountRepo, build_account_bus,
    };
    use crate::application::error::AppError;
    use crate::domain::error::DomainError;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::account::{AccountId, AccountType};
    use crate::domain::ports::driven::{AccountCredentialStore, AccountRepository};

    fn add_command(service: &str, user: &str, pw: &str) -> AddAccountCommand {
        AddAccountCommand {
            service_name: service.into(),
            username: user.into(),
            password: pw.into(),
            account_type: AccountType::Premium,
            created_at_ms: 1_700_000_000_000,
        }
    }

    #[tokio::test]
    async fn test_update_account_partial_patch_changes_only_listed_fields() {
        let repo = Arc::new(InMemoryAccountRepo::new());
        let creds = Arc::new(FakeAccountCredentialStore::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_account_bus(repo.clone(), creds.clone(), events.clone(), None, None);
        let id = bus
            .handle_add_account(add_command("real-debrid", "alice", "old-pw"))
            .await
            .unwrap();

        bus.handle_update_account(UpdateAccountCommand {
            id: id.clone(),
            patch: AccountPatch {
                enabled: Some(false),
                ..AccountPatch::default()
            },
        })
        .await
        .expect("update ok");

        let after = repo.find_by_id(&id).unwrap().unwrap();
        assert!(!after.is_enabled());
        assert_eq!(after.username(), "alice", "untouched field stays as-is");
        assert_eq!(after.account_type(), AccountType::Premium);
        assert_eq!(creds.get_password(&id).unwrap().as_deref(), Some("old-pw"));
    }

    #[tokio::test]
    async fn test_update_account_password_rotation_writes_new_keyring_value() {
        let repo = Arc::new(InMemoryAccountRepo::new());
        let creds = Arc::new(FakeAccountCredentialStore::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_account_bus(repo.clone(), creds.clone(), events, None, None);
        let id = bus
            .handle_add_account(add_command("real-debrid", "alice", "old-pw"))
            .await
            .unwrap();

        bus.handle_update_account(UpdateAccountCommand {
            id: id.clone(),
            patch: AccountPatch {
                password: Some("new-pw".into()),
                ..AccountPatch::default()
            },
        })
        .await
        .unwrap();

        assert_eq!(creds.get_password(&id).unwrap().as_deref(), Some("new-pw"));
    }

    #[tokio::test]
    async fn test_update_account_unknown_id_returns_not_found() {
        let repo = Arc::new(InMemoryAccountRepo::new());
        let creds = Arc::new(FakeAccountCredentialStore::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_account_bus(repo, creds, events, None, None);

        let err = bus
            .handle_update_account(UpdateAccountCommand {
                id: AccountId::new("missing"),
                patch: AccountPatch::default(),
            })
            .await
            .expect_err("missing id");
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_update_account_blank_username_rejected() {
        let repo = Arc::new(InMemoryAccountRepo::new());
        let creds = Arc::new(FakeAccountCredentialStore::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_account_bus(repo.clone(), creds, events, None, None);
        let id = bus
            .handle_add_account(add_command("real-debrid", "alice", "pw"))
            .await
            .unwrap();

        let err = bus
            .handle_update_account(UpdateAccountCommand {
                id: id.clone(),
                patch: AccountPatch {
                    username: Some("   ".into()),
                    ..AccountPatch::default()
                },
            })
            .await
            .expect_err("blank rejected");
        assert!(matches!(err, AppError::Validation(_)));
        let unchanged = repo.find_by_id(&id).unwrap().unwrap();
        assert_eq!(unchanged.username(), "alice");
    }

    #[tokio::test]
    async fn test_update_account_empty_password_rejected_without_keyring_write() {
        let repo = Arc::new(InMemoryAccountRepo::new());
        let creds = Arc::new(FakeAccountCredentialStore::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_account_bus(repo.clone(), creds.clone(), events, None, None);
        let id = bus
            .handle_add_account(add_command("real-debrid", "alice", "pw"))
            .await
            .unwrap();

        let err = bus
            .handle_update_account(UpdateAccountCommand {
                id: id.clone(),
                patch: AccountPatch {
                    password: Some("".into()),
                    ..AccountPatch::default()
                },
            })
            .await
            .expect_err("empty pw rejected");
        assert!(matches!(err, AppError::Validation(_)));
        assert_eq!(creds.get_password(&id).unwrap().as_deref(), Some("pw"));
    }

    #[tokio::test]
    async fn test_update_account_emits_event_and_keeps_created_at() {
        let repo = Arc::new(InMemoryAccountRepo::new());
        let creds = Arc::new(FakeAccountCredentialStore::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_account_bus(repo.clone(), creds, events.clone(), None, None);
        let id = bus
            .handle_add_account(add_command("real-debrid", "alice", "pw"))
            .await
            .unwrap();
        events.snapshot(); // discard creation event from comparison

        bus.handle_update_account(UpdateAccountCommand {
            id: id.clone(),
            patch: AccountPatch {
                account_type: Some(AccountType::Debrid),
                ..AccountPatch::default()
            },
        })
        .await
        .unwrap();

        let snapshot = events.snapshot();
        assert!(
            snapshot
                .iter()
                .any(|e| matches!(e, DomainEvent::AccountUpdated { id: ev } if ev == &id)),
            "AccountUpdated event missing"
        );
        let after = repo.find_by_id(&id).unwrap().unwrap();
        assert_eq!(after.created_at(), 1_700_000_000_000);
        assert_eq!(after.account_type(), AccountType::Debrid);
    }

    #[tokio::test]
    async fn test_update_account_propagates_repo_error() {
        // No fake "failing repo" exists yet — simulate a save failure
        // by triggering the unique-constraint check in `save`.
        let repo = Arc::new(InMemoryAccountRepo::new());
        let creds = Arc::new(FakeAccountCredentialStore::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_account_bus(repo.clone(), creds.clone(), events, None, None);

        // Two accounts on the same service: id1 = (real-debrid, alice),
        // id2 = (real-debrid, bob).
        let id1 = bus
            .handle_add_account(add_command("real-debrid", "alice", "pw1"))
            .await
            .unwrap();
        let _id2 = bus
            .handle_add_account(add_command("real-debrid", "bob", "pw2"))
            .await
            .unwrap();

        // Renaming id1 to "bob" must collide with id2 and surface as
        // an `AlreadyExists` domain error from the repo.
        let err = bus
            .handle_update_account(UpdateAccountCommand {
                id: id1,
                patch: AccountPatch {
                    username: Some("bob".into()),
                    ..AccountPatch::default()
                },
            })
            .await
            .expect_err("collision");
        assert!(matches!(
            err,
            AppError::Domain(DomainError::AlreadyExists(_))
        ));
    }
}
