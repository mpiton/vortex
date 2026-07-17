use crate::application::services::account_state::{apply_status, status_for_plugin_error};
use crate::domain::error::DomainError;
use crate::domain::event::DomainEvent;
use crate::domain::model::account::{Account, AccountStatus};
use crate::domain::model::credential::Credential;
use crate::domain::ports::driven::{ExtractedHosterLink, ResolutionCancellation};

use super::{ResolveHosterSourceHandler, ResolvePremiumSourceCommand};

impl ResolveHosterSourceHandler {
    pub(super) fn resolve_locked(
        &self,
        command: ResolvePremiumSourceCommand,
        cancellation: &ResolutionCancellation,
    ) -> Result<ExtractedHosterLink, DomainError> {
        cancellation.ensure_active()?;
        let mut account = self.load_available_account(&command)?;
        let credential = self.load_credential(&mut account, cancellation)?;
        let link = match self.plugins.extract_hoster_link(
            &command.service_name,
            &command.source_url,
            Some(&credential),
        ) {
            Ok(link) => link,
            Err(error) => {
                return self.reject_plugin_failure(&mut account, error, cancellation);
            }
        };
        self.persist_success(&mut account, &link, cancellation)?;
        Ok(link)
    }

    fn load_available_account(
        &self,
        command: &ResolvePremiumSourceCommand,
    ) -> Result<Account, DomainError> {
        let account = self.repo.find_by_id(&command.account_id)?.ok_or_else(|| {
            DomainError::NotFound(format!("account {}", command.account_id.as_str()))
        })?;
        if account.service_name() != command.service_name
            || !account.is_selectable(self.clock.now_unix_ms())
        {
            return Err(DomainError::ValidationError(
                "premium account is unavailable".into(),
            ));
        }
        Ok(account)
    }

    fn load_credential(
        &self,
        account: &mut Account,
        cancellation: &ResolutionCancellation,
    ) -> Result<Credential, DomainError> {
        let Some(password) = self.credentials.get_password(account.id())? else {
            account.set_status(AccountStatus::MissingCredential);
            cancellation.run_if_active(|| {
                self.rotator.save_account(account)?;
                self.publish_failure(account, "Account credential is unavailable");
                Ok(())
            })?;
            return Err(DomainError::NotFound(format!(
                "credential for account {}",
                account.id().as_str()
            )));
        };
        cancellation.ensure_active()?;
        Ok(Credential::new(account.username(), password))
    }

    fn reject_plugin_failure(
        &self,
        account: &mut Account,
        error: DomainError,
        cancellation: &ResolutionCancellation,
    ) -> Result<ExtractedHosterLink, DomainError> {
        let Some(status) = status_for_plugin_error(&error) else {
            return Err(DomainError::PluginError(
                "premium source resolution failed".into(),
            ));
        };
        apply_status(account, status, self.clock.now_unix_ms());
        cancellation.run_if_active(|| {
            self.rotator.save_account(account)?;
            self.publish_typed_failure(account, status);
            Ok(())
        })?;
        Err(error)
    }

    fn persist_success(
        &self,
        account: &mut Account,
        link: &ExtractedHosterLink,
        cancellation: &ResolutionCancellation,
    ) -> Result<(), DomainError> {
        if link
            .direct_url
            .as_deref()
            .is_none_or(|url| url.trim().is_empty())
        {
            return Err(DomainError::PluginError(
                "premium plugin returned no direct URL".into(),
            ));
        }
        if let Some(total) = link.traffic_total_bytes {
            account.set_traffic_total(total);
            if let Some(used) = link.traffic_used_bytes {
                let remaining = total.saturating_sub(used);
                account.set_traffic_left(remaining);
                if total > 0 && remaining == 0 {
                    apply_status(
                        account,
                        AccountStatus::QuotaExhausted,
                        self.clock.now_unix_ms(),
                    );
                    cancellation.run_if_active(|| {
                        self.rotator.save_account(account)?;
                        self.publish_typed_failure(account, AccountStatus::QuotaExhausted);
                        Ok(())
                    })?;
                    return Err(DomainError::AccountQuotaExceeded);
                }
            }
        }
        account.set_status(AccountStatus::Valid);
        cancellation.run_if_active(|| self.rotator.save_account(account))?;
        Ok(())
    }

    pub(super) fn publish_success(&self, id: &crate::domain::model::account::AccountId) {
        self.events
            .publish(DomainEvent::AccountUpdated { id: id.clone() });
    }

    fn publish_typed_failure(&self, account: &Account, status: AccountStatus) {
        if status == AccountStatus::QuotaExhausted {
            self.events.publish(DomainEvent::AccountExhausted {
                id: account.id().clone(),
                service_name: account.service_name().to_string(),
                exhausted_until_ms: account.exhausted_until().unwrap_or_default(),
            });
        } else {
            self.publish_failure(account, account_failure_message(status));
        }
    }

    fn publish_failure(&self, account: &Account, error: &str) {
        self.events.publish(DomainEvent::AccountValidationFailed {
            id: account.id().clone(),
            error: error.to_string(),
        });
    }
}

fn account_failure_message(status: AccountStatus) -> &'static str {
    match status {
        AccountStatus::InvalidCredentials => "Account credentials were rejected",
        AccountStatus::Expired => "Account is expired",
        AccountStatus::Cooldown => "Account is temporarily rate-limited",
        AccountStatus::QuotaExhausted => "Account quota is exhausted",
        _ => "Account validation failed",
    }
}
