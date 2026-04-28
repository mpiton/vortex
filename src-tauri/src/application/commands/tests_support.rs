//! Shared test fixtures for the account-command handler tests.
//!
//! Gated behind `#[cfg(test)]` — never linked into release binaries.

#![cfg(test)]

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::application::command_bus::CommandBus;
use crate::application::test_support::NoopHistoryRepo;
use crate::domain::error::DomainError;
use crate::domain::event::DomainEvent;
use crate::domain::model::account::{Account, AccountId};
use crate::domain::model::archive::{ArchiveEntry, ArchiveFormat, ExtractSummary};
use crate::domain::model::config::{AppConfig, ConfigPatch};
use crate::domain::model::credential::Credential;
use crate::domain::model::download::{Download, DownloadId, DownloadState};
use crate::domain::model::http::HttpResponse;
use crate::domain::model::meta::DownloadMeta;
use crate::domain::model::plugin::{PluginInfo, PluginManifest};
use crate::domain::ports::driven::{
    AccountCredentialStore, AccountRepository, AccountValidator, ArchiveExtractor,
    ClipboardObserver, ConfigStore, CredentialStore, DownloadEngine, DownloadRepository, EventBus,
    FileStorage, HttpClient, PassphraseCodec, PluginLoader, ValidationOutcome,
};

// ── In-memory account repository ─────────────────────────────────────

pub(crate) struct InMemoryAccountRepo {
    store: Mutex<HashMap<AccountId, Account>>,
}

impl InMemoryAccountRepo {
    pub(crate) fn new() -> Self {
        Self {
            store: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn snapshot(&self) -> Vec<Account> {
        let mut accounts: Vec<Account> = self.store.lock().unwrap().values().cloned().collect();
        accounts.sort_by(|a, b| {
            a.created_at()
                .cmp(&b.created_at())
                .then_with(|| a.id().as_str().cmp(b.id().as_str()))
        });
        accounts
    }
}

impl AccountRepository for InMemoryAccountRepo {
    fn find_by_id(&self, id: &AccountId) -> Result<Option<Account>, DomainError> {
        Ok(self.store.lock().unwrap().get(id).cloned())
    }

    fn save(&self, account: &Account) -> Result<(), DomainError> {
        let mut guard = self.store.lock().unwrap();
        for (id, existing) in guard.iter() {
            if id != account.id()
                && existing.service_name() == account.service_name()
                && existing.username() == account.username()
            {
                return Err(DomainError::AlreadyExists(format!(
                    "{}::{}",
                    account.service_name(),
                    account.username()
                )));
            }
        }
        let stored = match guard.get(account.id()) {
            Some(existing) => Account::reconstruct(
                account.id().clone(),
                account.service_name().to_string(),
                account.username().to_string(),
                account.account_type(),
                account.is_enabled(),
                account.traffic_left(),
                account.traffic_total(),
                account.valid_until(),
                account.last_validated(),
                existing.created_at(),
            ),
            None => account.clone(),
        };
        guard.insert(account.id().clone(), stored);
        Ok(())
    }

    fn list(&self) -> Result<Vec<Account>, DomainError> {
        Ok(self.snapshot())
    }

    fn list_by_service(&self, service_name: &str) -> Result<Vec<Account>, DomainError> {
        Ok(self
            .snapshot()
            .into_iter()
            .filter(|a| a.service_name() == service_name)
            .collect())
    }

    fn delete(&self, id: &AccountId) -> Result<(), DomainError> {
        self.store.lock().unwrap().remove(id);
        Ok(())
    }
}

// ── Fake account credential store ────────────────────────────────────

pub(crate) struct FakeAccountCredentialStore {
    entries: Mutex<HashMap<AccountId, String>>,
    fail_on_write: bool,
}

impl FakeAccountCredentialStore {
    pub(crate) fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            fail_on_write: false,
        }
    }

    pub(crate) fn failing_on_write(mut self) -> Self {
        self.fail_on_write = true;
        self
    }

    pub(crate) fn entry_count(&self) -> usize {
        self.entries.lock().unwrap().len()
    }

    pub(crate) fn snapshot(&self) -> Vec<(AccountId, String)> {
        let mut entries: Vec<(AccountId, String)> = self
            .entries
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        entries.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
        entries
    }
}

impl AccountCredentialStore for FakeAccountCredentialStore {
    fn store_password(&self, account_id: &AccountId, password: &str) -> Result<(), DomainError> {
        if self.fail_on_write {
            return Err(DomainError::StorageError(
                "fake keyring write failure".into(),
            ));
        }
        self.entries
            .lock()
            .unwrap()
            .insert(account_id.clone(), password.to_string());
        Ok(())
    }

    fn get_password(&self, account_id: &AccountId) -> Result<Option<String>, DomainError> {
        Ok(self.entries.lock().unwrap().get(account_id).cloned())
    }

    fn delete_password(&self, account_id: &AccountId) -> Result<(), DomainError> {
        self.entries.lock().unwrap().remove(account_id);
        Ok(())
    }
}

// ── Fake account validator ───────────────────────────────────────────

pub(crate) struct FakeAccountValidator {
    behavior: Mutex<HashMap<String, ValidatorBehavior>>,
}

#[derive(Clone)]
pub(crate) enum ValidatorBehavior {
    Ok(ValidationOutcome),
    Reject(String),
    Missing,
    Storage(String),
}

impl FakeAccountValidator {
    pub(crate) fn new() -> Self {
        Self {
            behavior: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn set(&self, service_name: &str, behavior: ValidatorBehavior) {
        self.behavior
            .lock()
            .unwrap()
            .insert(service_name.to_string(), behavior);
    }
}

impl AccountValidator for FakeAccountValidator {
    fn validate(
        &self,
        service_name: &str,
        _username: &str,
        _password: &str,
    ) -> Result<ValidationOutcome, DomainError> {
        let behavior = self
            .behavior
            .lock()
            .unwrap()
            .get(service_name)
            .cloned()
            .unwrap_or(ValidatorBehavior::Missing);
        match behavior {
            ValidatorBehavior::Ok(outcome) => Ok(outcome),
            ValidatorBehavior::Reject(msg) => Ok(ValidationOutcome::rejected(msg)),
            ValidatorBehavior::Missing => Err(DomainError::NotFound(format!(
                "no plugin for service {service_name}"
            ))),
            ValidatorBehavior::Storage(msg) => Err(DomainError::StorageError(msg)),
        }
    }
}

// ── Fake passphrase codec (XOR + length-prefixed passphrase tag) ─────

/// Toy codec used in handler tests so the import / export flow can be
/// exercised without depending on the AES adapter. The format is:
///
/// - 1 byte: ciphertext version (`0x01`)
/// - 1 byte: passphrase length `n`
/// - n bytes: passphrase echoed back (lets `open` reject the wrong key)
/// - rest: plaintext bytes (no XOR — the test fixture only needs to be
///   reversible and to fail on the wrong passphrase, not actually be
///   confidential).
pub(crate) struct FakePassphraseCodec;

impl PassphraseCodec for FakePassphraseCodec {
    fn seal(&self, passphrase: &str, plaintext: &[u8]) -> Result<Vec<u8>, DomainError> {
        let pass_bytes = passphrase.as_bytes();
        if pass_bytes.len() > u8::MAX as usize {
            return Err(DomainError::ValidationError("passphrase too long".into()));
        }
        let mut out = Vec::with_capacity(2 + pass_bytes.len() + plaintext.len());
        out.push(0x01);
        out.push(pass_bytes.len() as u8);
        out.extend_from_slice(pass_bytes);
        out.extend_from_slice(plaintext);
        Ok(out)
    }

    fn open(&self, passphrase: &str, ciphertext: &[u8]) -> Result<Vec<u8>, DomainError> {
        if ciphertext.len() < 2 {
            return Err(DomainError::ValidationError("ciphertext truncated".into()));
        }
        if ciphertext[0] != 0x01 {
            return Err(DomainError::ValidationError(
                "unsupported ciphertext version".into(),
            ));
        }
        let pass_len = ciphertext[1] as usize;
        if ciphertext.len() < 2 + pass_len {
            return Err(DomainError::ValidationError("ciphertext truncated".into()));
        }
        let stored = &ciphertext[2..2 + pass_len];
        if stored != passphrase.as_bytes() {
            return Err(DomainError::ValidationError("wrong passphrase".into()));
        }
        Ok(ciphertext[2 + pass_len..].to_vec())
    }
}

// ── Capturing event bus ──────────────────────────────────────────────

pub(crate) struct CapturingEventBus {
    events: Mutex<Vec<DomainEvent>>,
}

impl CapturingEventBus {
    pub(crate) fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn snapshot(&self) -> Vec<DomainEvent> {
        self.events.lock().unwrap().clone()
    }
}

impl EventBus for CapturingEventBus {
    fn publish(&self, event: DomainEvent) {
        self.events.lock().unwrap().push(event);
    }

    fn subscribe(&self, _handler: Box<dyn Fn(&DomainEvent) + Send + Sync>) {}
}

// ── Stubs for the unrelated ports the bus still requires ─────────────

struct StubDownloadRepo;
impl DownloadRepository for StubDownloadRepo {
    fn find_by_id(&self, _id: DownloadId) -> Result<Option<Download>, DomainError> {
        Ok(None)
    }
    fn save(&self, _d: &Download) -> Result<(), DomainError> {
        Ok(())
    }
    fn delete(&self, _id: DownloadId) -> Result<(), DomainError> {
        Ok(())
    }
    fn find_by_state(&self, _s: DownloadState) -> Result<Vec<Download>, DomainError> {
        Ok(vec![])
    }
}

struct StubDownloadEngine;
impl DownloadEngine for StubDownloadEngine {
    fn start(&self, _download: &Download) -> Result<(), DomainError> {
        Ok(())
    }
    fn pause(&self, _id: DownloadId) -> Result<(), DomainError> {
        Ok(())
    }
    fn resume(&self, _id: DownloadId) -> Result<(), DomainError> {
        Ok(())
    }
    fn cancel(&self, _id: DownloadId) -> Result<(), DomainError> {
        Ok(())
    }
}

struct StubFileStorage;
impl FileStorage for StubFileStorage {
    fn create_file(&self, _path: &Path, _size: u64) -> Result<(), DomainError> {
        Ok(())
    }
    fn write_segment(&self, _path: &Path, _offset: u64, _data: &[u8]) -> Result<(), DomainError> {
        Ok(())
    }
    fn read_meta(&self, _path: &Path) -> Result<Option<DownloadMeta>, DomainError> {
        Ok(None)
    }
    fn write_meta(&self, _path: &Path, _meta: &DownloadMeta) -> Result<(), DomainError> {
        Ok(())
    }
    fn delete_meta(&self, _path: &Path) -> Result<(), DomainError> {
        Ok(())
    }
}

struct StubHttpClient;
impl HttpClient for StubHttpClient {
    fn head(&self, _url: &str) -> Result<HttpResponse, DomainError> {
        Ok(HttpResponse {
            status_code: 200,
            headers: Default::default(),
            body: vec![],
        })
    }
    fn get_range(&self, _url: &str, _start: u64, _end: u64) -> Result<Vec<u8>, DomainError> {
        Ok(vec![])
    }
    fn supports_range(&self, _url: &str) -> Result<bool, DomainError> {
        Ok(false)
    }
}

struct StubPluginLoader;
impl PluginLoader for StubPluginLoader {
    fn load(&self, _manifest: &PluginManifest) -> Result<(), DomainError> {
        Ok(())
    }
    fn unload(&self, _name: &str) -> Result<(), DomainError> {
        Ok(())
    }
    fn resolve_url(&self, _url: &str) -> Result<Option<PluginInfo>, DomainError> {
        Ok(None)
    }
    fn extract_links(&self, _url: &str) -> Result<String, DomainError> {
        Err(DomainError::NotFound("not mocked".into()))
    }
    fn get_media_variants(&self, _url: &str) -> Result<String, DomainError> {
        Err(DomainError::NotFound("not mocked".into()))
    }
    fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
        Ok(vec![])
    }
    fn set_enabled(&self, _name: &str, _enabled: bool) -> Result<(), DomainError> {
        Ok(())
    }
}

struct StubConfigStore;
impl ConfigStore for StubConfigStore {
    fn get_config(&self) -> Result<AppConfig, DomainError> {
        Ok(AppConfig::default())
    }
    fn update_config(&self, _patch: ConfigPatch) -> Result<AppConfig, DomainError> {
        Ok(AppConfig::default())
    }
}

struct StubCredentialStore;
impl CredentialStore for StubCredentialStore {
    fn get(&self, _service: &str) -> Result<Option<Credential>, DomainError> {
        Ok(None)
    }
    fn store(&self, _service: &str, _credential: &Credential) -> Result<(), DomainError> {
        Ok(())
    }
    fn delete(&self, _service: &str) -> Result<(), DomainError> {
        Ok(())
    }
}

struct StubClipboardObserver;
impl ClipboardObserver for StubClipboardObserver {
    fn start(&self) -> Result<(), DomainError> {
        Ok(())
    }
    fn stop(&self) -> Result<(), DomainError> {
        Ok(())
    }
    fn get_urls(&self) -> Result<Vec<String>, DomainError> {
        Ok(vec![])
    }
}

struct StubArchiveExtractor;
impl ArchiveExtractor for StubArchiveExtractor {
    fn detect_format(&self, _file_path: &Path) -> Result<Option<ArchiveFormat>, DomainError> {
        Ok(None)
    }
    fn can_extract(&self, _file_path: &Path) -> Result<bool, DomainError> {
        Ok(false)
    }
    fn extract(
        &self,
        _file_path: &Path,
        _dest_dir: &Path,
        _password: Option<&str>,
    ) -> Result<ExtractSummary, DomainError> {
        Ok(ExtractSummary {
            extracted_files: 0,
            extracted_bytes: 0,
            duration_ms: 0,
            warnings: vec![],
        })
    }
    fn list_contents(
        &self,
        _file_path: &Path,
        _password: Option<&str>,
    ) -> Result<Vec<ArchiveEntry>, DomainError> {
        Ok(vec![])
    }
    fn detect_segments(
        &self,
        _file_path: &Path,
    ) -> Result<Option<Vec<std::path::PathBuf>>, DomainError> {
        Ok(None)
    }
}

/// Build a [`CommandBus`] wired with the supplied account ports plus
/// stubs for everything else.
pub(crate) fn build_account_bus(
    account_repo: Arc<dyn AccountRepository>,
    credential_store: Arc<dyn AccountCredentialStore>,
    event_bus: Arc<CapturingEventBus>,
    validator: Option<Arc<dyn AccountValidator>>,
    codec: Option<Arc<dyn PassphraseCodec>>,
) -> CommandBus {
    let mut bus = CommandBus::new(
        Arc::new(StubDownloadRepo),
        Arc::new(StubDownloadEngine),
        event_bus,
        Arc::new(StubFileStorage),
        Arc::new(StubHttpClient),
        Arc::new(StubPluginLoader),
        Arc::new(StubConfigStore),
        Arc::new(StubCredentialStore),
        Arc::new(StubClipboardObserver),
        Arc::new(StubArchiveExtractor),
        Arc::new(NoopHistoryRepo),
        None,
    )
    .with_account_repo(account_repo)
    .with_account_credential_store(credential_store);

    if let Some(v) = validator {
        bus = bus.with_account_validator(v);
    }
    if let Some(c) = codec {
        bus = bus.with_passphrase_codec(c);
    }
    bus
}

/// Build a bus with no account ports — used to assert handlers refuse
/// to run when their dependencies are missing.
pub(crate) fn bus_without_account_ports(event_bus: Arc<CapturingEventBus>) -> CommandBus {
    CommandBus::new(
        Arc::new(StubDownloadRepo),
        Arc::new(StubDownloadEngine),
        event_bus,
        Arc::new(StubFileStorage),
        Arc::new(StubHttpClient),
        Arc::new(StubPluginLoader),
        Arc::new(StubConfigStore),
        Arc::new(StubCredentialStore),
        Arc::new(StubClipboardObserver),
        Arc::new(StubArchiveExtractor),
        Arc::new(NoopHistoryRepo),
        None,
    )
}
