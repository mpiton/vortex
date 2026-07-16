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
use crate::application::services::account_state::{apply_status, status_for_plugin_error};
use crate::domain::error::DomainError;
use crate::domain::event::DomainEvent;
use crate::domain::model::account::{Account, AccountStatus};
use crate::domain::ports::driven::{AccountValidator, ValidationOutcome};

pub(super) struct AccountValidationAttempt {
    pub outcome: ValidationOutcome,
    pub error: Option<DomainError>,
}

pub(super) fn validate_credentials(
    validator: &dyn AccountValidator,
    account: &Account,
    password: &str,
) -> AccountValidationAttempt {
    match validator.validate(account.service_name(), account.username(), password) {
        Ok(outcome) => AccountValidationAttempt {
            outcome,
            error: None,
        },
        Err(error) => match status_for_plugin_error(&error) {
            Some(status) => typed_rejection(status, error),
            None => AccountValidationAttempt {
                outcome: ValidationOutcome::rejected(AccountStatus::Error, error.to_string()),
                error: Some(error),
            },
        },
    }
}

fn typed_rejection(status: AccountStatus, error: DomainError) -> AccountValidationAttempt {
    AccountValidationAttempt {
        outcome: ValidationOutcome::rejected(status, error.to_string()),
        error: None,
    }
}

pub(super) fn apply_validation(
    account: &Account,
    outcome: &ValidationOutcome,
    now_ms: u64,
) -> Account {
    let mut next = account.clone();
    next.set_last_validated(now_ms);
    apply_status(&mut next, outcome.status, now_ms);
    if outcome.is_valid() {
        if let Some(traffic_left) = outcome.traffic_left {
            next.set_traffic_left(traffic_left);
        }
        if let Some(traffic_total) = outcome.traffic_total {
            next.set_traffic_total(traffic_total);
        }
        next.replace_valid_until(outcome.valid_until);
    }
    next
}

pub(super) fn publish_validation(
    bus: &CommandBus,
    id: crate::domain::model::account::AccountId,
    outcome: &ValidationOutcome,
) {
    if outcome.is_valid() {
        bus.event_bus().publish(DomainEvent::AccountValidated {
            id,
            latency_ms: outcome.latency_ms,
            traffic_left: outcome.traffic_left,
            traffic_total: outcome.traffic_total,
            valid_until: outcome.valid_until,
        });
    } else {
        bus.event_bus()
            .publish(DomainEvent::AccountValidationFailed {
                id,
                error: outcome
                    .error_message
                    .clone()
                    .unwrap_or_else(|| "validation rejected".into()),
            });
    }
}

pub(super) fn sync_validation_availability(
    bus: &CommandBus,
    id: &crate::domain::model::account::AccountId,
    outcome: &ValidationOutcome,
) -> Result<(), AppError> {
    if outcome.is_valid()
        && let Some(rotator) = bus.account_rotator()
    {
        rotator.clear_exhausted(id)?;
    }
    Ok(())
}

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
        let operation_lock = self.account_operation_lock(&cmd.id)?;
        let _operation_guard = operation_lock.lock().await;

        let account = repo
            .find_by_id(&cmd.id)?
            .ok_or_else(|| AppError::NotFound(format!("account {} not found", cmd.id.as_str())))?;

        let password = match store.get_password(&cmd.id)? {
            Some(password) => password,
            None => {
                let outcome = ValidationOutcome::rejected(
                    AccountStatus::MissingCredential,
                    format!("no stored password for account {}", cmd.id.as_str()),
                );
                repo.save(&apply_validation(&account, &outcome, cmd.now_ms))?;
                publish_validation(self, cmd.id.clone(), &outcome);
                return Err(AppError::NotFound(
                    outcome.error_message.unwrap_or_default(),
                ));
            }
        };

        let attempt = validate_credentials(validator, &account, &password);
        repo.save(&apply_validation(&account, &attempt.outcome, cmd.now_ms))?;
        sync_validation_availability(self, &cmd.id, &attempt.outcome)?;
        publish_validation(self, cmd.id, &attempt.outcome);

        match attempt.error {
            Some(DomainError::NotFound(message)) => Err(AppError::NotFound(message)),
            Some(error) => Err(error.into()),
            None => Ok(attempt.outcome.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::Duration;

    use super::super::{AddAccountCommand, DeleteAccountCommand, ValidateAccountCommand};
    use crate::application::commands::tests_support::{
        CapturingEventBus, FakeAccountCredentialStore, FakeAccountValidator, InMemoryAccountRepo,
        ValidatorBehavior, build_account_bus,
    };
    use crate::application::error::AppError;
    use crate::application::services::{AccountRotator, AccountSelector};
    use crate::domain::error::DomainError;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::account::{Account, AccountId, AccountStatus, AccountType};
    use crate::domain::ports::driven::{
        AccountCredentialStore, AccountRepository, AccountValidator, Clock, ValidationOutcome,
    };

    struct ValidationClock;

    impl Clock for ValidationClock {
        fn now_unix_secs(&self) -> u64 {
            1_700_000_000
        }
    }

    struct BlockingValidator {
        entered: Mutex<Option<std::sync::mpsc::Sender<()>>>,
        release: Arc<(Mutex<bool>, Condvar)>,
    }

    impl AccountValidator for BlockingValidator {
        fn validate(&self, _: &str, _: &str, _: &str) -> Result<ValidationOutcome, DomainError> {
            if let Some(sender) = self.entered.lock().expect("entered mutex").take() {
                sender.send(()).expect("test receiver remains alive");
            }
            let (mutex, condition) = &*self.release;
            let mut released = mutex.lock().expect("release mutex");
            while !*released {
                released = condition.wait(released).expect("release wait");
            }
            Ok(ValidationOutcome::ok())
        }
    }

    fn add_command(service: &str) -> AddAccountCommand {
        AddAccountCommand {
            service_name: service.into(),
            username: "alice".into(),
            password: "pw".into(),
            account_type: AccountType::Premium,
            created_at_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn successful_validation_clears_a_stale_expiry() {
        let mut account = Account::new(
            AccountId::new("account-1"),
            "vortex-mod-1fichier".into(),
            "alice".into(),
            AccountType::Premium,
            1,
        );
        account.set_status(AccountStatus::Valid);
        account.set_valid_until(100);

        let validated = super::apply_validation(&account, &ValidationOutcome::ok(), 200);

        assert_eq!(validated.valid_until(), None);
    }

    #[test]
    fn temporary_validation_failure_records_a_retry_deadline() {
        let account = Account::new(
            AccountId::new("account-1"),
            "vortex-mod-1fichier".into(),
            "alice".into(),
            AccountType::Premium,
            1,
        );
        let outcome = ValidationOutcome::rejected(
            AccountStatus::Cooldown,
            DomainError::AccountCooldown.to_string(),
        );

        let validated = super::apply_validation(&account, &outcome, 200);

        assert_eq!(validated.status(), AccountStatus::Cooldown);
        assert_eq!(validated.exhausted_until(), Some(60_200));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_delete_cannot_be_undone_by_a_slow_validation() {
        let repo = Arc::new(InMemoryAccountRepo::new());
        let credentials = Arc::new(FakeAccountCredentialStore::new());
        let account = Account::new(
            AccountId::new("account-1"),
            "vortex-mod-1fichier".into(),
            "alice".into(),
            AccountType::Premium,
            1,
        );
        repo.save(&account).expect("seed account");
        credentials
            .store_password(account.id(), "api-key")
            .expect("seed credential");

        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let validator = Arc::new(BlockingValidator {
            entered: Mutex::new(Some(entered_tx)),
            release: release.clone(),
        });
        let events = Arc::new(CapturingEventBus::new());
        let bus = Arc::new(build_account_bus(
            repo.clone(),
            credentials,
            events,
            Some(validator),
            None,
        ));

        let validating_bus = bus.clone();
        let validating = tokio::spawn(async move {
            validating_bus
                .handle_validate_account(ValidateAccountCommand {
                    id: AccountId::new("account-1"),
                    now_ms: 2,
                })
                .await
        });
        tokio::task::spawn_blocking(move || entered_rx.recv_timeout(Duration::from_secs(1)))
            .await
            .expect("join entered wait")
            .expect("validator entered");

        let deleting_bus = bus.clone();
        let mut deleting = tokio::spawn(async move {
            deleting_bus
                .handle_delete_account(DeleteAccountCommand {
                    id: AccountId::new("account-1"),
                })
                .await
        });
        let deleted_while_validation_blocked =
            tokio::time::timeout(Duration::from_millis(100), &mut deleting)
                .await
                .ok();

        let (mutex, condition) = &*release;
        *mutex.lock().expect("release mutex") = true;
        condition.notify_all();
        validating
            .await
            .expect("validation join")
            .expect("validation");
        match deleted_while_validation_blocked {
            Some(result) => result.expect("delete join").expect("delete"),
            None => deleting.await.expect("delete join").expect("delete"),
        }

        assert!(
            repo.find_by_id(account.id())
                .expect("read account")
                .is_none(),
            "a validation that began before deletion must not recreate the row"
        );
    }

    #[tokio::test]
    async fn successful_validation_clears_the_rotator_cooldown_cache() {
        let repo = Arc::new(InMemoryAccountRepo::new());
        let credentials = Arc::new(FakeAccountCredentialStore::new());
        let events = Arc::new(CapturingEventBus::new());
        let validator = Arc::new(FakeAccountValidator::new());
        validator.set(
            "vortex-mod-1fichier",
            ValidatorBehavior::Ok(ValidationOutcome::ok()),
        );
        let account = Account::reconstruct_with_status(
            AccountId::new("account-1"),
            "vortex-mod-1fichier".into(),
            "alice".into(),
            AccountType::Premium,
            true,
            None,
            None,
            Some(u64::MAX),
            Some(1),
            1,
            AccountStatus::Valid,
            None,
        );
        repo.save(&account).expect("seed account");
        credentials
            .store_password(account.id(), "api-key")
            .expect("seed credential");
        let clock: Arc<dyn Clock> = Arc::new(ValidationClock);
        let selector = AccountSelector::new(repo.clone(), events.clone(), clock.clone());
        let rotator = AccountRotator::new(selector, repo.clone(), events.clone(), clock);
        rotator
            .mark_exhausted(account.id(), account.service_name(), 60)
            .expect("mark exhausted");
        let bus = build_account_bus(repo.clone(), credentials, events, Some(validator), None)
            .with_account_rotator(rotator.clone());

        bus.handle_validate_account(ValidateAccountCommand {
            id: account.id().clone(),
            now_ms: 1_700_000_000_000,
        })
        .await
        .expect("validation succeeds");

        assert!(!rotator.is_exhausted(account.id()).expect("rotator state"));
        assert_eq!(
            repo.find_by_id(account.id()).unwrap().unwrap().status(),
            AccountStatus::Valid
        );
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
                status: crate::domain::model::account::AccountStatus::Valid,
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
        assert_eq!(after.status(), AccountStatus::InvalidCredentials);
        assert!(after.traffic_left().is_none(), "no traffic on reject");

        assert!(
            events
                .snapshot()
                .iter()
                .any(|e| matches!(e, DomainEvent::AccountValidationFailed { id: ev, error } if ev == &id && error == "wrong password"))
        );
    }

    #[tokio::test]
    async fn test_validate_account_maps_typed_plugin_error_to_persisted_status() {
        let repo = Arc::new(InMemoryAccountRepo::new());
        let creds = Arc::new(FakeAccountCredentialStore::new());
        let validator = Arc::new(FakeAccountValidator::new());
        validator.set(
            "vortex-mod-1fichier",
            ValidatorBehavior::Domain(DomainError::AccountExpired),
        );
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_account_bus(repo.clone(), creds, events, Some(validator), None);
        let id = bus
            .handle_add_account(add_command("vortex-mod-1fichier"))
            .await
            .expect("account remains configured");

        let outcome = bus
            .handle_validate_account(ValidateAccountCommand {
                id: id.clone(),
                now_ms: 1_900_000_000_000,
            })
            .await
            .expect("typed account failure is a validation outcome");

        assert!(!outcome.valid);
        let stored = repo.find_by_id(&id).unwrap().expect("account persisted");
        assert_eq!(stored.status(), AccountStatus::Expired);
        assert_eq!(stored.last_validated(), Some(1_900_000_000_000));
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
        let stored = repo.find_by_id(&id).unwrap().expect("account persisted");
        assert_eq!(stored.status(), AccountStatus::MissingCredential);
    }
}
