//! Command handler for one credential-scoped premium hoster resolution.

use std::sync::Arc;

use crate::application::services::AccountRotator;
use crate::application::services::account_operation_locks::AccountOperationLocks;
use crate::domain::error::DomainError;
use crate::domain::model::account::AccountId;
use crate::domain::model::download::Download;
use crate::domain::ports::driven::{
    AccountCredentialStore, AccountRepository, Clock, ConfigStore, DownloadRepository,
    DownloadSourceResolver, EventBus, ExtractedHosterLink, PluginLoader, ResolutionCancellation,
    ResolvedDownloadSource,
};
use crate::domain::ports::driving::Command;

#[path = "premium_account_resolution.rs"]
mod resolution;
#[path = "premium_account_rotation.rs"]
mod rotation;

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

#[derive(Clone)]
pub struct ResolvePremiumSourceHandler {
    repo: Arc<dyn AccountRepository>,
    credentials: Arc<dyn AccountCredentialStore>,
    plugins: Arc<dyn PluginLoader>,
    events: Arc<dyn EventBus>,
    clock: Arc<dyn Clock>,
    locks: Arc<AccountOperationLocks>,
    downloads: Arc<dyn DownloadRepository>,
    config: Arc<dyn ConfigStore>,
    rotator: Arc<AccountRotator>,
}

impl ResolvePremiumSourceHandler {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        repo: Arc<dyn AccountRepository>,
        credentials: Arc<dyn AccountCredentialStore>,
        plugins: Arc<dyn PluginLoader>,
        events: Arc<dyn EventBus>,
        clock: Arc<dyn Clock>,
        locks: Arc<AccountOperationLocks>,
        downloads: Arc<dyn DownloadRepository>,
        config: Arc<dyn ConfigStore>,
        rotator: Arc<AccountRotator>,
    ) -> Self {
        Self {
            repo,
            credentials,
            plugins,
            events,
            clock,
            locks,
            downloads,
            config,
            rotator,
        }
    }

    pub async fn handle(
        &self,
        command: ResolvePremiumSourceCommand,
    ) -> Result<ExtractedHosterLink, DomainError> {
        let account_id = command.account_id.clone();
        let handler = self.clone();
        let cancellation = ResolutionCancellation::default();
        let link = tokio::task::spawn_blocking(move || {
            let lock = handler.account_lock(&command.account_id)?;
            let _guard = lock.blocking_lock();
            handler.resolve_locked(command, &cancellation)
        })
        .await
        .map_err(|_| DomainError::PluginError("premium source resolver stopped".into()))??;
        self.publish_success(&account_id);
        Ok(link)
    }

    fn account_lock(&self, id: &AccountId) -> Result<Arc<tokio::sync::Mutex<()>>, DomainError> {
        self.locks
            .lock_for(id)
            .map_err(|_| DomainError::StorageError("account lock unavailable".into()))
    }
}

impl DownloadSourceResolver for ResolvePremiumSourceHandler {
    fn resolve(&self, download: &Download) -> Result<ResolvedDownloadSource, DomainError> {
        self.resolve_download(download, &ResolutionCancellation::default())
    }

    fn resolve_cancellable(
        &self,
        download: &Download,
        cancellation: &ResolutionCancellation,
    ) -> Result<ResolvedDownloadSource, DomainError> {
        self.resolve_download(download, cancellation)
    }
}

#[cfg(test)]
#[path = "resolve_premium_source_tests.rs"]
mod tests;
