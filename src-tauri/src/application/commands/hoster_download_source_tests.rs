//! The debrid rung can still fail after the link check accepted it, and
//! R-04 wants the next rung tried rather than a dead download.

use std::sync::{Arc, Mutex};

use super::*;
use crate::application::commands::tests_support::{
    CapturingEventBus, FakeAccountCredentialStore, InMemoryAccountRepo, InMemoryDownloadRepo,
};
use crate::application::services::account_operation_locks::AccountOperationLocks;
use crate::application::services::{AccountRotator, AccountSelector};
use crate::domain::model::account::{Account, AccountId, AccountStatus, AccountType};
use crate::domain::model::config::{AppConfig, ConfigPatch};
use crate::domain::model::credential::Credential;
use crate::domain::model::download::{DownloadId, Url};
use crate::domain::model::plugin::{PluginInfo, PluginManifest};
use crate::domain::ports::driven::{
    AccountCredentialStore, AccountRepository, Clock, ConfigStore, PluginLoader,
};

const DEBRID: &str = "vortex-mod-alldebrid";
const HOSTER: &str = "vortex-mod-mediafire";
const URL: &str = "https://www.mediafire.com/file/abc/archive.zip/file";

struct FixedClock;

impl Clock for FixedClock {
    fn now_unix_secs(&self) -> u64 {
        1_700_000_000
    }
}

/// The default order, so the free rung stays configured.
struct DefaultConfig;

impl ConfigStore for DefaultConfig {
    fn get_config(&self) -> Result<AppConfig, DomainError> {
        Ok(AppConfig::default())
    }

    fn update_config(&self, _: ConfigPatch) -> Result<AppConfig, DomainError> {
        Ok(AppConfig::default())
    }
}

/// Every account-backed rung fails; the anonymous one serves the file
/// unless the hoster is down too.
struct FallThroughLoader {
    hoster_serves: bool,
    anonymous_calls: Mutex<Vec<String>>,
}

impl FallThroughLoader {
    fn new(hoster_serves: bool) -> Self {
        Self {
            hoster_serves,
            anonymous_calls: Mutex::new(Vec::new()),
        }
    }

    fn info(name: &str) -> PluginInfo {
        let category = if name == DEBRID {
            PluginCategory::Debrid
        } else {
            PluginCategory::Hoster
        };
        PluginInfo::new(
            name.to_string(),
            "1.0.0".into(),
            name.to_string(),
            "vortex".into(),
            category,
        )
    }
}

impl PluginLoader for FallThroughLoader {
    fn load(&self, _: &PluginManifest) -> Result<(), DomainError> {
        Ok(())
    }

    fn unload(&self, _: &str) -> Result<(), DomainError> {
        Ok(())
    }

    fn resolve_url(&self, _: &str) -> Result<Option<PluginInfo>, DomainError> {
        Ok(Some(Self::info(HOSTER)))
    }

    fn plugin_can_handle(&self, _: &str, _: &str) -> Result<bool, DomainError> {
        Ok(true)
    }

    fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
        Ok(vec![Self::info(HOSTER), Self::info(DEBRID)])
    }

    fn set_enabled(&self, _: &str, _: bool) -> Result<(), DomainError> {
        Ok(())
    }

    fn extract_hoster_link(
        &self,
        service_name: &str,
        url: &str,
        credential: Option<&Credential>,
    ) -> Result<ExtractedHosterLink, DomainError> {
        if credential.is_some() {
            return Err(DomainError::AccountQuotaExceeded);
        }
        self.anonymous_calls
            .lock()
            .expect("call log")
            .push(service_name.to_string());
        if !self.hoster_serves {
            return Err(DomainError::HosterNoFile);
        }
        Ok(ExtractedHosterLink {
            source_url: url.to_string(),
            filename: Some("archive.zip".into()),
            size_bytes: Some(42),
            direct_url: Some("https://cdn.example/archive.zip".into()),
            resumable: Some(true),
            request_headers: Vec::new(),
            traffic_used_bytes: None,
            traffic_total_bytes: None,
            captcha: None,
        })
    }
}

fn seed(
    repo: &InMemoryAccountRepo,
    credentials: &FakeAccountCredentialStore,
    id: &str,
    service: &str,
) {
    let mut account = Account::new(
        AccountId::new(id),
        service.to_string(),
        "user".into(),
        AccountType::Debrid,
        0,
    );
    account.set_status(AccountStatus::Valid);
    repo.save(&account).expect("seed account");
    credentials
        .store_password(account.id(), "secret")
        .expect("seed credential");
}

fn handler(loader: Arc<FallThroughLoader>) -> ResolveHosterSourceHandler {
    let repo = Arc::new(InMemoryAccountRepo::new());
    let credentials = Arc::new(FakeAccountCredentialStore::new());
    seed(&repo, &credentials, "debrid-1", DEBRID);
    seed(&repo, &credentials, "premium-1", HOSTER);
    let events = Arc::new(CapturingEventBus::new());
    let clock: Arc<dyn Clock> = Arc::new(FixedClock);
    let repo: Arc<dyn AccountRepository> = repo;
    let selector = AccountSelector::new(repo.clone(), events.clone(), clock.clone());
    let rotator = AccountRotator::new(selector, repo.clone(), events.clone(), clock.clone());
    ResolveHosterSourceHandler::new(
        repo,
        credentials,
        loader,
        events,
        clock,
        Arc::new(AccountOperationLocks::default()),
        Arc::new(InMemoryDownloadRepo::new()),
        Arc::new(DefaultConfig),
        rotator,
    )
}

fn download(module: &str, account_id: &str) -> Download {
    Download::new(
        DownloadId(1),
        Url::new(URL).expect("valid url"),
        "archive.zip".into(),
        "/tmp/archive.zip".into(),
    )
    .with_module_name(module.to_string())
    .with_account_id(AccountId::new(account_id))
}

#[test]
fn test_a_spent_debrid_quota_falls_through_to_anonymous_extraction() {
    let loader = Arc::new(FallThroughLoader::new(true));
    let handler = handler(loader.clone());

    let source = handler
        .resolve(&download(DEBRID, "debrid-1"))
        .expect("the free rung still owns the URL");

    assert_eq!(source.request_url(), "https://cdn.example/archive.zip");
    assert_eq!(*loader.anonymous_calls.lock().expect("call log"), [HOSTER]);
}

#[test]
fn test_both_rungs_failing_reports_both_reasons_and_never_a_direct_url() {
    let loader = Arc::new(FallThroughLoader::new(false));
    let handler = handler(loader.clone());

    let error = handler
        .resolve(&download(DEBRID, "debrid-1"))
        .expect_err("no rung produced a file");

    let message = error.to_string();
    assert!(message.contains("debrid:"), "{message}");
    assert!(message.contains("free:"), "{message}");
}

#[test]
fn test_a_failed_premium_hoster_is_never_downgraded_to_the_free_path() {
    // Only the debrid rung falls through. Retrying a paid hoster account
    // anonymously would hand the user a throttled free download while
    // reporting success.
    let loader = Arc::new(FallThroughLoader::new(true));
    let handler = handler(loader.clone());

    let error = handler
        .resolve(&download(HOSTER, "premium-1"))
        .expect_err("the premium failure stands");

    assert!(
        matches!(error, DomainError::AccountQuotaExceeded),
        "{error}"
    );
    assert!(loader.anonymous_calls.lock().expect("call log").is_empty());
}
