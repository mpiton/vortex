//! Handler for [`ValidateAccountCommand`](super::ValidateAccountCommand).
//!
//! Looks up the account, reads its password from the keyring, hands
//! both off to [`AccountValidator`], and applies the resulting
//! [`ValidationOutcome`] to the persisted row. The handler returns a
//! detailed [`ValidationOutcomeDto`] so the caller can drive both
//! `account_validate` (boolean OK/fail) and `account_test_connection`
//! (full latency + traffic readout) without re-reading the row.

use super::ValidationOutcomeDto;
use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::error::DomainError;
use crate::domain::event::DomainEvent;
use crate::domain::model::account::Account;

impl CommandBus {
    pub async fn handle_validate_account(
        &self,
        cmd: super::ValidateAccountCommand,
    ) -> Result<ValidationOutcomeDto, AppError> {
        let repo = self
            .account_repo()
            .ok_or_else(|| AppError::Validation("account repository not configured".into()))?;
        let store = self.account_credential_store().ok_or_else(|| {
            AppError::Validation("account credential store not configured".into())
        })?;
        let validator = self
            .account_validator()
            .ok_or_else(|| AppError::Validation("account validator not configured".into()))?;

        let account = repo
            .find_by_id(&cmd.id)?
            .ok_or_else(|| AppError::NotFound(format!("account {} not found", cmd.id.as_str())))?;

        let password = store.get_password(&cmd.id)?.ok_or_else(|| {
            AppError::NotFound(format!(
                "no stored password for account {}",
                cmd.id.as_str()
            ))
        })?;

        let outcome =
            match validator.validate(account.service_name(), account.username(), &password) {
                Ok(o) => o,
                Err(DomainError::NotFound(msg)) => {
                    self.event_bus()
                        .publish(DomainEvent::AccountValidationFailed {
                            id: cmd.id.clone(),
                            error: msg.clone(),
                        });
                    return Err(AppError::NotFound(msg));
                }
                Err(other) => {
                    // Network failures, keyring read errors, and any
                    // other domain error coming back from the validator
                    // are surfaced as `AccountValidationFailed` so the
                    // UI can react identically whether the upstream
                    // service rejected the credentials or was simply
                    // unreachable.
                    self.event_bus()
                        .publish(DomainEvent::AccountValidationFailed {
                            id: cmd.id.clone(),
                            error: other.to_string(),
                        });
                    return Err(other.into());
                }
            };

        let mut next = clone_account(&account);
        next.set_last_validated(cmd.now_ms);
        if outcome.valid {
            if let Some(t) = outcome.traffic_left {
                next.set_traffic_left(t);
            }
            if let Some(t) = outcome.traffic_total {
                next.set_traffic_total(t);
            }
            if let Some(v) = outcome.valid_until {
                next.set_valid_until(v);
            }
        }
        repo.save(&next)?;

        if outcome.valid {
            self.event_bus().publish(DomainEvent::AccountValidated {
                id: cmd.id,
                latency_ms: outcome.latency_ms,
                traffic_left: outcome.traffic_left,
                traffic_total: outcome.traffic_total,
                valid_until: outcome.valid_until,
            });
        } else {
            self.event_bus()
                .publish(DomainEvent::AccountValidationFailed {
                    id: cmd.id,
                    error: outcome
                        .error_message
                        .clone()
                        .unwrap_or_else(|| "validation rejected".into()),
                });
        }

        Ok(outcome.into())
    }
}

fn clone_account(account: &Account) -> Account {
    Account::reconstruct(
        account.id().clone(),
        account.service_name().to_string(),
        account.username().to_string(),
        account.account_type(),
        account.is_enabled(),
        account.traffic_left(),
        account.traffic_total(),
        account.valid_until(),
        account.last_validated(),
        account.created_at(),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::{AddAccountCommand, ValidateAccountCommand};
    use crate::application::commands::tests_support::{
        CapturingEventBus, FakeAccountCredentialStore, FakeAccountValidator, InMemoryAccountRepo,
        ValidatorBehavior, build_account_bus,
    };
    use crate::application::error::AppError;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::account::{AccountId, AccountType};
    use crate::domain::ports::driven::{
        AccountCredentialStore, AccountRepository, ValidationOutcome,
    };

    fn add_command(service: &str) -> AddAccountCommand {
        AddAccountCommand {
            service_name: service.into(),
            username: "alice".into(),
            password: "pw".into(),
            account_type: AccountType::Premium,
            created_at_ms: 1_700_000_000_000,
        }
    }

    #[tokio::test]
    async fn test_validate_account_unknown_service_returns_not_found() {
        let repo = Arc::new(InMemoryAccountRepo::new());
        let creds = Arc::new(FakeAccountCredentialStore::new());
        let validator = Arc::new(FakeAccountValidator::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_account_bus(repo, creds, events.clone(), Some(validator), None);
        let id = bus
            .handle_add_account(add_command("mystery"))
            .await
            .unwrap();

        let err = bus
            .handle_validate_account(ValidateAccountCommand {
                id: id.clone(),
                now_ms: 2_000_000_000_000,
            })
            .await
            .expect_err("missing plugin");
        assert!(matches!(err, AppError::NotFound(ref m) if m.contains("mystery")));
        assert!(
            events
                .snapshot()
                .iter()
                .any(|e| matches!(
                    e,
                    DomainEvent::AccountValidationFailed { id: ev, error } if ev == &id && error.contains("mystery")
                ))
        );
    }

    #[tokio::test]
    async fn test_validate_account_success_updates_metadata_and_emits_event() {
        let repo = Arc::new(InMemoryAccountRepo::new());
        let creds = Arc::new(FakeAccountCredentialStore::new());
        let validator = Arc::new(FakeAccountValidator::new());
        validator.set(
            "real-debrid",
            ValidatorBehavior::Ok(ValidationOutcome {
                valid: true,
                latency_ms: Some(120),
                traffic_left: Some(50_000),
                traffic_total: Some(100_000),
                valid_until: Some(2_500_000_000_000),
                error_message: None,
            }),
        );
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_account_bus(repo.clone(), creds, events.clone(), Some(validator), None);
        let id = bus
            .handle_add_account(add_command("real-debrid"))
            .await
            .unwrap();

        let outcome = bus
            .handle_validate_account(ValidateAccountCommand {
                id: id.clone(),
                now_ms: 1_900_000_000_000,
            })
            .await
            .expect("validate ok");

        assert!(outcome.valid);
        assert_eq!(outcome.latency_ms, Some(120));
        assert_eq!(outcome.traffic_left, Some(50_000));

        let after = repo.find_by_id(&id).unwrap().unwrap();
        assert_eq!(after.last_validated(), Some(1_900_000_000_000));
        assert_eq!(after.traffic_left(), Some(50_000));
        assert_eq!(after.traffic_total(), Some(100_000));
        assert_eq!(after.valid_until(), Some(2_500_000_000_000));

        assert!(
            events
                .snapshot()
                .iter()
                .any(|e| matches!(e, DomainEvent::AccountValidated { id: ev, traffic_left: Some(50_000), .. } if ev == &id))
        );
    }

    #[tokio::test]
    async fn test_validate_account_rejected_records_last_validated_but_not_traffic() {
        let repo = Arc::new(InMemoryAccountRepo::new());
        let creds = Arc::new(FakeAccountCredentialStore::new());
        let validator = Arc::new(FakeAccountValidator::new());
        validator.set(
            "real-debrid",
            ValidatorBehavior::Reject("wrong password".into()),
        );
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_account_bus(repo.clone(), creds, events.clone(), Some(validator), None);
        let id = bus
            .handle_add_account(add_command("real-debrid"))
            .await
            .unwrap();

        let outcome = bus
            .handle_validate_account(ValidateAccountCommand {
                id: id.clone(),
                now_ms: 1_900_000_000_000,
            })
            .await
            .expect("call returns Ok with valid=false");
        assert!(!outcome.valid);
        assert_eq!(outcome.error_message.as_deref(), Some("wrong password"));

        let after = repo.find_by_id(&id).unwrap().unwrap();
        assert_eq!(after.last_validated(), Some(1_900_000_000_000));
        assert!(after.traffic_left().is_none(), "no traffic on reject");

        assert!(
            events
                .snapshot()
                .iter()
                .any(|e| matches!(e, DomainEvent::AccountValidationFailed { id: ev, error } if ev == &id && error == "wrong password"))
        );
    }

    #[tokio::test]
    async fn test_validate_account_storage_error_emits_validation_failed_event() {
        let repo = Arc::new(InMemoryAccountRepo::new());
        let creds = Arc::new(FakeAccountCredentialStore::new());
        let validator = Arc::new(FakeAccountValidator::new());
        validator.set(
            "real-debrid",
            ValidatorBehavior::Storage("upstream timeout".into()),
        );
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_account_bus(repo, creds, events.clone(), Some(validator), None);
        let id = bus
            .handle_add_account(add_command("real-debrid"))
            .await
            .unwrap();

        let err = bus
            .handle_validate_account(ValidateAccountCommand {
                id: id.clone(),
                now_ms: 1_900_000_000_000,
            })
            .await
            .expect_err("storage error surfaces");
        assert!(matches!(err, AppError::Domain(_)));
        assert!(
            events.snapshot().iter().any(|e| matches!(
                e,
                DomainEvent::AccountValidationFailed { id: ev, error } if ev == &id && error.contains("upstream timeout")
            )),
            "AccountValidationFailed must fire on validator storage errors too"
        );
    }

    #[tokio::test]
    async fn test_validate_account_unknown_id_returns_not_found() {
        let repo = Arc::new(InMemoryAccountRepo::new());
        let creds = Arc::new(FakeAccountCredentialStore::new());
        let validator = Arc::new(FakeAccountValidator::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_account_bus(repo, creds, events, Some(validator), None);

        let err = bus
            .handle_validate_account(ValidateAccountCommand {
                id: AccountId::new("ghost"),
                now_ms: 0,
            })
            .await
            .expect_err("ghost id");
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_validate_account_missing_keyring_password_errors() {
        let repo = Arc::new(InMemoryAccountRepo::new());
        let creds = Arc::new(FakeAccountCredentialStore::new());
        let validator = Arc::new(FakeAccountValidator::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_account_bus(repo.clone(), creds.clone(), events, Some(validator), None);

        let id = bus
            .handle_add_account(add_command("real-debrid"))
            .await
            .unwrap();
        // Simulate a keyring eviction: delete the password under id.
        creds
            .delete_password(&id)
            .expect("infallible in test fixture");

        let err = bus
            .handle_validate_account(ValidateAccountCommand {
                id: id.clone(),
                now_ms: 0,
            })
            .await
            .expect_err("missing pw");
        assert!(matches!(err, AppError::NotFound(_)));
    }
}
