//! Just-in-time premium URL resolution for queued downloads.

use std::sync::Arc;

use crate::application::services::account_operation_locks::AccountOperationLocks;
use crate::application::services::account_state::{apply_status, status_for_plugin_error};
use crate::domain::error::DomainError;
use crate::domain::model::account::{Account, AccountStatus};
use crate::domain::model::credential::Credential;
use crate::domain::model::download::Download;
use crate::domain::ports::driven::{
    AccountCredentialStore, AccountRepository, Clock, DownloadSourceResolver, PluginLoader,
    ResolvedDownloadSource,
};

pub struct PremiumSourceResolver {
    repo: Arc<dyn AccountRepository>,
    credentials: Arc<dyn AccountCredentialStore>,
    plugins: Arc<dyn PluginLoader>,
    clock: Arc<dyn Clock>,
    locks: Arc<AccountOperationLocks>,
}

impl PremiumSourceResolver {
    pub fn new(
        repo: Arc<dyn AccountRepository>,
        credentials: Arc<dyn AccountCredentialStore>,
        plugins: Arc<dyn PluginLoader>,
        clock: Arc<dyn Clock>,
        locks: Arc<AccountOperationLocks>,
    ) -> Self {
        Self {
            repo,
            credentials,
            plugins,
            clock,
            locks,
        }
    }

    fn persist_failure(&self, account: &mut Account, error: &DomainError) {
        if let Some(status) = status_for_plugin_error(error) {
            apply_status(account, status, self.clock.now_unix_ms());
            if let Err(save_error) = self.repo.save(account) {
                tracing::warn!(
                    account_id = %account.id().as_str(),
                    error = %save_error,
                    "failed to persist premium account failure"
                );
            }
        }
    }
}

impl DownloadSourceResolver for PremiumSourceResolver {
    fn resolve(&self, download: &Download) -> Result<ResolvedDownloadSource, DomainError> {
        let account_id = download.account_id().ok_or_else(|| {
            DomainError::ValidationError("premium download has no account association".into())
        })?;
        let service_name = download.module_name().ok_or_else(|| {
            DomainError::ValidationError("premium download has no plugin association".into())
        })?;
        let lock = self
            .locks
            .lock_for(account_id)
            .map_err(|_| DomainError::StorageError("account lock unavailable".into()))?;
        let _guard = lock.blocking_lock();
        let mut account = self
            .repo
            .find_by_id(account_id)?
            .ok_or_else(|| DomainError::NotFound(format!("account {}", account_id.as_str())))?;
        if account.service_name() != service_name
            || !account.is_selectable(self.clock.now_unix_ms())
        {
            return Err(DomainError::ValidationError(
                "premium account is unavailable".into(),
            ));
        }
        let password = self.credentials.get_password(account_id)?.ok_or_else(|| {
            account.set_status(AccountStatus::MissingCredential);
            let _ = self.repo.save(&account);
            DomainError::NotFound(format!("credential for account {}", account_id.as_str()))
        })?;
        let credential = Credential::new(account.username(), password);
        let link = match self.plugins.extract_hoster_link(
            service_name,
            download.url().as_str(),
            Some(&credential),
        ) {
            Ok(link) => link,
            Err(error) => {
                self.persist_failure(&mut account, &error);
                return Err(match error {
                    DomainError::AccountInvalidCredentials
                    | DomainError::AccountExpired
                    | DomainError::AccountCooldown
                    | DomainError::AccountQuotaExceeded => error,
                    _ => DomainError::PluginError("premium source resolution failed".into()),
                });
            }
        };
        let direct_url = link.direct_url.ok_or_else(|| {
            DomainError::PluginError("premium plugin returned no direct URL".into())
        })?;
        if let Some(total) = link.traffic_total_bytes {
            account.set_traffic_total(total);
            account.set_traffic_left(total.saturating_sub(link.traffic_used_bytes.unwrap_or(0)));
        }
        account.set_status(AccountStatus::Valid);
        self.repo.save(&account)?;
        Ok(ResolvedDownloadSource::sensitive(direct_url))
    }
}

#[cfg(test)]
#[path = "premium_source_resolver_tests.rs"]
mod tests;
