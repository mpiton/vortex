use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::*;
use crate::application::commands::tests_support::{
    FakeAccountCredentialStore, InMemoryAccountRepo,
};
use crate::domain::model::account::{AccountId, AccountType};
use crate::domain::model::download::{DownloadId, Url};
use crate::domain::model::plugin::{PluginInfo, PluginManifest};
use crate::domain::ports::driven::{
    AccountCredentialStore, AccountRepository, ExtractedHosterLink,
};

struct FixedClock;

impl Clock for FixedClock {
    fn now_unix_secs(&self) -> u64 {
        1_700_000_000
    }
}

struct SaveFailingRepo {
    inner: InMemoryAccountRepo,
    fail_saves: AtomicBool,
}

impl SaveFailingRepo {
    fn new() -> Self {
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

struct DirectUrlPlugin {
    calls: Mutex<Vec<(String, String, String)>>,
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
        Ok(Vec::new())
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
        let credential = credential.expect("premium credential");
        self.calls.lock().unwrap().push((
            service.to_string(),
            url.to_string(),
            credential.password().to_string(),
        ));
        Ok(ExtractedHosterLink {
            source_url: url.to_string(),
            filename: Some("file.zip".into()),
            size_bytes: Some(42),
            direct_url: Some("https://1.1.1.1/short-lived-token".into()),
            traffic_used_bytes: Some(10),
            traffic_total_bytes: Some(100),
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resolves_the_direct_url_only_when_the_engine_requests_it() {
    let repo = Arc::new(InMemoryAccountRepo::new());
    let credentials = Arc::new(FakeAccountCredentialStore::new());
    let plugin = Arc::new(DirectUrlPlugin {
        calls: Mutex::new(Vec::new()),
    });
    let account_id = AccountId::new("account-1");
    let mut account = Account::new(
        account_id.clone(),
        "vortex-mod-1fichier".into(),
        "alice".into(),
        AccountType::Premium,
        1,
    );
    account.set_status(AccountStatus::Valid);
    repo.save(&account).unwrap();
    credentials.store_password(&account_id, "api-key").unwrap();
    let resolver = Arc::new(PremiumSourceResolver::new(
        repo.clone(),
        credentials,
        plugin.clone(),
        Arc::new(FixedClock),
        Arc::new(AccountOperationLocks::default()),
    ));
    let download = Download::new(
        DownloadId(1),
        Url::new("https://1fichier.com/?abc123").unwrap(),
        "file.zip".into(),
        "/tmp/file.zip".into(),
    )
    .with_module_name("vortex-mod-1fichier".into())
    .with_account_id(account_id.clone());

    let source = tokio::task::spawn_blocking(move || resolver.resolve(&download))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(source.request_url(), "https://1.1.1.1/short-lived-token");
    assert_eq!(
        plugin.calls.lock().unwrap().as_slice(),
        [(
            "vortex-mod-1fichier".into(),
            "https://1fichier.com/?abc123".into(),
            "api-key".into()
        )]
    );
    let stored = repo.find_by_id(&account_id).unwrap().unwrap();
    assert_eq!(stored.traffic_left(), Some(90));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_credential_persistence_failure_is_not_suppressed() {
    let repo = Arc::new(SaveFailingRepo::new());
    let account_id = AccountId::new("account-1");
    let mut account = Account::new(
        account_id.clone(),
        "vortex-mod-1fichier".into(),
        "alice".into(),
        AccountType::Premium,
        1,
    );
    account.set_status(AccountStatus::Valid);
    repo.save(&account).unwrap();
    repo.fail_saves.store(true, Ordering::SeqCst);
    let resolver = Arc::new(PremiumSourceResolver::new(
        repo,
        Arc::new(FakeAccountCredentialStore::new()),
        Arc::new(DirectUrlPlugin {
            calls: Mutex::new(Vec::new()),
        }),
        Arc::new(FixedClock),
        Arc::new(AccountOperationLocks::default()),
    ));
    let download = Download::new(
        DownloadId(1),
        Url::new("https://1fichier.com/?abc123").unwrap(),
        "file.zip".into(),
        "/tmp/file.zip".into(),
    )
    .with_module_name("vortex-mod-1fichier".into())
    .with_account_id(account_id);

    let error = tokio::task::spawn_blocking(move || resolver.resolve(&download))
        .await
        .unwrap()
        .expect_err("storage failure must win over missing credential");

    assert!(matches!(error, DomainError::StorageError(_)));
}
