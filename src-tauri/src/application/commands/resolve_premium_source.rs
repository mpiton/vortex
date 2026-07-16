//! Command handler for one credential-scoped premium hoster resolution.

use std::sync::Arc;

use crate::application::services::account_operation_locks::AccountOperationLocks;
use crate::domain::error::DomainError;
use crate::domain::model::account::AccountId;
use crate::domain::model::download::Download;
use crate::domain::ports::driven::{
    AccountCredentialStore, AccountRepository, Clock, DownloadSourceResolver, EventBus,
    ExtractedHosterLink, PluginLoader, ResolvedDownloadSource,
};
use crate::domain::ports::driving::Command;

#[path = "premium_account_resolution.rs"]
mod resolution;

#[derive(Debug)]
pub struct ResolvePremiumSourceCommand {
    account_id: AccountId,
    service_name: String,
    source_url: String,
}

impl ResolvePremiumSourceCommand {
    pub fn new(account_id: AccountId, service_name: String, source_url: String) -> Self {
        Self {
            account_id,
            service_name,
            source_url,
        }
    }
}

impl Command for ResolvePremiumSourceCommand {}

pub struct ResolvePremiumSourceHandler {
    repo: Arc<dyn AccountRepository>,
    credentials: Arc<dyn AccountCredentialStore>,
    plugins: Arc<dyn PluginLoader>,
    events: Arc<dyn EventBus>,
    clock: Arc<dyn Clock>,
    locks: Arc<AccountOperationLocks>,
}

impl ResolvePremiumSourceHandler {
    pub fn new(
        repo: Arc<dyn AccountRepository>,
        credentials: Arc<dyn AccountCredentialStore>,
        plugins: Arc<dyn PluginLoader>,
        events: Arc<dyn EventBus>,
        clock: Arc<dyn Clock>,
        locks: Arc<AccountOperationLocks>,
    ) -> Self {
        Self {
            repo,
            credentials,
            plugins,
            events,
            clock,
            locks,
        }
    }

    pub async fn handle(
        &self,
        command: ResolvePremiumSourceCommand,
    ) -> Result<ExtractedHosterLink, DomainError> {
        let lock = self.account_lock(&command.account_id)?;
        let _guard = lock.lock().await;
        self.resolve_locked(command)
    }

    fn handle_blocking(
        &self,
        command: ResolvePremiumSourceCommand,
    ) -> Result<ExtractedHosterLink, DomainError> {
        let lock = self.account_lock(&command.account_id)?;
        let _guard = lock.blocking_lock();
        self.resolve_locked(command)
    }

    fn account_lock(&self, id: &AccountId) -> Result<Arc<tokio::sync::Mutex<()>>, DomainError> {
        self.locks
            .lock_for(id)
            .map_err(|_| DomainError::StorageError("account lock unavailable".into()))
    }
}

impl DownloadSourceResolver for ResolvePremiumSourceHandler {
    fn resolve(&self, download: &Download) -> Result<ResolvedDownloadSource, DomainError> {
        let account_id = download.account_id().cloned().ok_or_else(|| {
            DomainError::ValidationError("premium download has no account association".into())
        })?;
        let service_name = download.module_name().map(str::to_string).ok_or_else(|| {
            DomainError::ValidationError("premium download has no plugin association".into())
        })?;
        let command = ResolvePremiumSourceCommand::new(
            account_id,
            service_name,
            download.url().as_str().to_string(),
        );
        let link = self.handle_blocking(command)?;
        let direct_url = link.direct_url.ok_or_else(|| {
            DomainError::PluginError("premium plugin returned no direct URL".into())
        })?;
        Ok(ResolvedDownloadSource::sensitive(direct_url))
    }
}

#[cfg(test)]
#[path = "resolve_premium_source_tests.rs"]
mod tests;
