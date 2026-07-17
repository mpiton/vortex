use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::application::commands::tests_support::{
    CapturingEventBus, FakeAccountCredentialStore, InMemoryAccountRepo, InMemoryDownloadRepo,
};
use crate::application::services::account_operation_locks::AccountOperationLocks;
use crate::application::services::{AccountRotator, AccountSelector};
use crate::domain::error::DomainError;
use crate::domain::model::account::{Account, AccountId, AccountStatus, AccountType};
use crate::domain::model::config::{AppConfig, ConfigPatch};
use crate::domain::model::credential::Credential;
use crate::domain::model::download::{Download, DownloadId, Url};
use crate::domain::model::plugin::{PluginCategory, PluginInfo, PluginManifest};
use crate::domain::ports::driven::{
    AccountRepository, Clock, ConfigStore, DownloadRepository, ExtractedHosterLink, PluginLoader,
};

use super::ResolveHosterSourceHandler;

pub(super) struct FixedClock;

impl Clock for FixedClock {
    fn now_unix_secs(&self) -> u64 {
        1_700_000_000
    }
}

pub(super) struct DirectUrlPlugin {
    pub(super) calls: Mutex<Vec<(String, String, String)>>,
}

impl DirectUrlPlugin {
    pub(super) fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
        }
    }
}

impl PluginLoader for DirectUrlPlugin {
    fn load(&self, _: &PluginManifest) -> Result<(), DomainError> {
        Ok(())
    }
    fn unload(&self, _: &str) -> Result<(), DomainError> {
        Ok(())
    }
    fn resolve_url(&self, _: &str) -> Result<Option<PluginInfo>, DomainError> {
        Ok(None)
    }
    fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
        Ok(vec![PluginInfo::new(
            "vortex-mod-1fichier".into(),
            "1.1.0".into(),
            "1fichier".into(),
            "vortex".into(),
            PluginCategory::Hoster,
        )])
    }
    fn set_enabled(&self, _: &str, _: bool) -> Result<(), DomainError> {
        Ok(())
    }
    fn extract_hoster_link(
        &self,
        service: &str,
        url: &str,
        credential: Option<&Credential>,
    ) -> Result<ExtractedHosterLink, DomainError> {
        let password = credential.map(Credential::password).unwrap_or_default();
        self.calls.lock().unwrap().push((
            service.to_string(),
            url.to_string(),
            password.to_string(),
        ));
        if password == "quota-key" {
            return Err(DomainError::AccountQuotaExceeded);
        }
        if password == "expired-key" {
            return Err(DomainError::AccountExpired);
        }
        if password == "cooldown-key" {
            return Err(DomainError::AccountCooldown);
        }
        let traffic_used_bytes = if password == "zero-traffic-key" {
            Some(100)
        } else {
            Some(10)
        };
        Ok(ExtractedHosterLink {
            source_url: url.to_string(),
            filename: Some("file.zip".into()),
            size_bytes: Some(42),
            direct_url: (password != "missing-url")
                .then(|| "https://1.1.1.1/short-lived-token".into()),
            resumable: Some(true),
            request_headers: vec![("Referer".into(), "https://1fichier.com/".into())],
            traffic_used_bytes,
            traffic_total_bytes: Some(100),
        })
    }
}

struct FixedConfigStore;

impl ConfigStore for FixedConfigStore {
    fn get_config(&self) -> Result<AppConfig, DomainError> {
        Ok(AppConfig::default())
    }

    fn update_config(&self, _: ConfigPatch) -> Result<AppConfig, DomainError> {
        Ok(AppConfig::default())
    }
}

pub(super) struct SaveFailingRepo {
    inner: InMemoryAccountRepo,
    pub(super) fail_saves: AtomicBool,
}

impl SaveFailingRepo {
    pub(super) fn new() -> Self {
        Self {
            inner: InMemoryAccountRepo::new(),
            fail_saves: AtomicBool::new(false),
        }
    }
}

impl AccountRepository for SaveFailingRepo {
    fn find_by_id(&self, id: &AccountId) -> Result<Option<Account>, DomainError> {
        self.inner.find_by_id(id)
    }
    fn save(&self, account: &Account) -> Result<(), DomainError> {
        if self.fail_saves.load(Ordering::SeqCst) {
            return Err(DomainError::StorageError("database unavailable".into()));
        }
        self.inner.save(account)
    }
    fn list(&self) -> Result<Vec<Account>, DomainError> {
        self.inner.list()
    }
    fn list_by_service(&self, service_name: &str) -> Result<Vec<Account>, DomainError> {
        self.inner.list_by_service(service_name)
    }
    fn delete(&self, id: &AccountId) -> Result<(), DomainError> {
        self.inner.delete(id)
    }
}

pub(super) fn valid_account(id: &str) -> Account {
    let mut account = Account::new(
        AccountId::new(id),
        "vortex-mod-1fichier".into(),
        format!("user-{id}"),
        AccountType::Premium,
        1,
    );
    account.set_status(AccountStatus::Valid);
    account
}

pub(super) fn download(account_id: AccountId) -> Download {
    Download::new(
        DownloadId(1),
        Url::new("https://1fichier.com/?abc123").unwrap(),
        "file.zip".into(),
        "/tmp/file.zip".into(),
    )
    .with_module_name("vortex-mod-1fichier".into())
    .with_account_id(account_id)
}

pub(super) fn handler(
    repo: Arc<dyn AccountRepository>,
    credentials: Arc<FakeAccountCredentialStore>,
    plugin: Arc<DirectUrlPlugin>,
    events: Arc<CapturingEventBus>,
) -> Arc<ResolveHosterSourceHandler> {
    handler_with_downloads(
        repo,
        credentials,
        plugin,
        events,
        Arc::new(InMemoryDownloadRepo::new()),
    )
}

pub(super) fn handler_with_downloads(
    repo: Arc<dyn AccountRepository>,
    credentials: Arc<FakeAccountCredentialStore>,
    plugin: Arc<DirectUrlPlugin>,
    events: Arc<CapturingEventBus>,
    downloads: Arc<dyn DownloadRepository>,
) -> Arc<ResolveHosterSourceHandler> {
    let clock: Arc<dyn Clock> = Arc::new(FixedClock);
    let selector = AccountSelector::new(repo.clone(), events.clone(), clock.clone());
    let rotator = AccountRotator::new(selector, repo.clone(), events.clone(), clock.clone());
    Arc::new(ResolveHosterSourceHandler::new(
        repo,
        credentials,
        plugin,
        events,
        clock,
        Arc::new(AccountOperationLocks::default()),
        downloads,
        Arc::new(FixedConfigStore),
        rotator,
    ))
}
