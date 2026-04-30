//! Compilation and mock implementation tests for driven ports.
//!
//! Verifies that each trait can be implemented with an in-memory mock,
//! and that all mocks satisfy Send + Sync bounds.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use crate::domain::error::DomainError;
use crate::domain::event::DomainEvent;
use crate::domain::model::account::{Account, AccountId, AccountType};
use crate::domain::model::config::{AppConfig, ConfigPatch};
use crate::domain::model::credential::Credential;
use crate::domain::model::download::{Download, DownloadId, DownloadState};
use crate::domain::model::http::HttpResponse;
use crate::domain::model::meta::DownloadMeta;
use crate::domain::model::package::{Package, PackageId, PackageSourceType};
use crate::domain::model::plugin::{PluginCategory, PluginInfo, PluginManifest};
use crate::domain::model::views::{
    DownloadDetailView, DownloadFilter, DownloadView, HistoryEntry, HistoryFilter, HistorySort,
    SortOrder, StateCountMap, StatsView,
};
use crate::domain::ports::driven::*;

// ── InMemoryDownloadRepository ───────────────────────────────────

struct InMemoryDownloadRepository {
    store: Mutex<HashMap<u64, Download>>,
}

impl InMemoryDownloadRepository {
    fn new() -> Self {
        Self {
            store: Mutex::new(HashMap::new()),
        }
    }
}

impl DownloadRepository for InMemoryDownloadRepository {
    fn find_by_id(&self, id: DownloadId) -> Result<Option<Download>, DomainError> {
        Ok(self.store.lock().unwrap().get(&id.0).cloned())
    }

    fn save(&self, download: &Download) -> Result<(), DomainError> {
        self.store
            .lock()
            .unwrap()
            .insert(download.id().0, download.clone());
        Ok(())
    }

    fn delete(&self, id: DownloadId) -> Result<(), DomainError> {
        self.store.lock().unwrap().remove(&id.0);
        Ok(())
    }

    fn find_by_state(&self, state: DownloadState) -> Result<Vec<Download>, DomainError> {
        Ok(self
            .store
            .lock()
            .unwrap()
            .values()
            .filter(|d| d.state() == state)
            .cloned()
            .collect())
    }
}

// ── InMemoryDownloadReadRepository ──────────────────────────────

struct InMemoryDownloadReadRepository;

impl DownloadReadRepository for InMemoryDownloadReadRepository {
    fn find_downloads(
        &self,
        _filter: Option<DownloadFilter>,
        _sort: Option<SortOrder>,
        _limit: Option<usize>,
        _offset: Option<usize>,
    ) -> Result<Vec<DownloadView>, DomainError> {
        Ok(vec![])
    }

    fn find_download_detail(
        &self,
        _id: DownloadId,
    ) -> Result<Option<DownloadDetailView>, DomainError> {
        Ok(None)
    }

    fn count_by_state(&self) -> Result<StateCountMap, DomainError> {
        Ok(HashMap::new())
    }
}

// ── CollectingEventBus ──────────────────────────────────────────

struct CollectingEventBus {
    events: Mutex<Vec<DomainEvent>>,
}

impl CollectingEventBus {
    fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
        }
    }
}

impl EventBus for CollectingEventBus {
    fn publish(&self, event: DomainEvent) {
        self.events.lock().unwrap().push(event);
    }

    fn subscribe(&self, _handler: Box<dyn Fn(&DomainEvent) + Send + Sync>) {
        // No-op for testing
    }
}

// ── InMemoryFileStorage ─────────────────────────────────────────

struct InMemoryFileStorage {
    files: Mutex<HashMap<String, Vec<u8>>>,
    metas: Mutex<HashMap<String, DownloadMeta>>,
}

impl InMemoryFileStorage {
    fn new() -> Self {
        Self {
            files: Mutex::new(HashMap::new()),
            metas: Mutex::new(HashMap::new()),
        }
    }
}

impl FileStorage for InMemoryFileStorage {
    fn create_file(&self, path: &Path, size: u64) -> Result<(), DomainError> {
        self.files.lock().unwrap().insert(
            path.to_string_lossy().into_owned(),
            vec![0u8; size as usize],
        );
        Ok(())
    }

    fn write_segment(&self, path: &Path, offset: u64, data: &[u8]) -> Result<(), DomainError> {
        let key = path.to_string_lossy().into_owned();
        let mut files = self.files.lock().unwrap();
        if let Some(file) = files.get_mut(&key) {
            let start = offset as usize;
            let end = start + data.len();
            if end <= file.len() {
                file[start..end].copy_from_slice(data);
            }
        }
        Ok(())
    }

    fn read_meta(&self, path: &Path) -> Result<Option<DownloadMeta>, DomainError> {
        Ok(self
            .metas
            .lock()
            .unwrap()
            .get(&path.to_string_lossy().into_owned())
            .cloned())
    }

    fn write_meta(&self, path: &Path, meta: &DownloadMeta) -> Result<(), DomainError> {
        self.metas
            .lock()
            .unwrap()
            .insert(path.to_string_lossy().into_owned(), meta.clone());
        Ok(())
    }

    fn delete_meta(&self, path: &Path) -> Result<(), DomainError> {
        self.metas
            .lock()
            .unwrap()
            .remove(&path.to_string_lossy().into_owned());
        Ok(())
    }
}

// ── FakeHttpClient ──────────────────────────────────────────────

struct FakeHttpClient;

impl HttpClient for FakeHttpClient {
    fn head(&self, _url: &str) -> Result<HttpResponse, DomainError> {
        Ok(HttpResponse {
            status_code: 200,
            headers: HashMap::from([
                ("content-length".to_string(), vec!["1024".to_string()]),
                ("accept-ranges".to_string(), vec!["bytes".to_string()]),
            ]),
            body: vec![],
        })
    }

    fn get_range(&self, _url: &str, start: u64, end: u64) -> Result<Vec<u8>, DomainError> {
        let size = (end - start + 1) as usize;
        Ok(vec![0xAB; size])
    }

    fn supports_range(&self, _url: &str) -> Result<bool, DomainError> {
        Ok(true)
    }
}

// ── InMemoryCredentialStore ─────────────────────────────────────

struct InMemoryCredentialStore {
    store: Mutex<HashMap<String, Credential>>,
}

impl InMemoryCredentialStore {
    fn new() -> Self {
        Self {
            store: Mutex::new(HashMap::new()),
        }
    }
}

impl CredentialStore for InMemoryCredentialStore {
    fn get(&self, service: &str) -> Result<Option<Credential>, DomainError> {
        Ok(self.store.lock().unwrap().get(service).cloned())
    }

    fn store(&self, service: &str, credential: &Credential) -> Result<(), DomainError> {
        self.store
            .lock()
            .unwrap()
            .insert(service.to_string(), credential.clone());
        Ok(())
    }

    fn delete(&self, service: &str) -> Result<(), DomainError> {
        self.store.lock().unwrap().remove(service);
        Ok(())
    }
}

// ── InMemoryConfigStore ───────────────────────────────────────────

struct InMemoryConfigStore {
    config: Mutex<AppConfig>,
}

impl InMemoryConfigStore {
    fn new() -> Self {
        Self {
            config: Mutex::new(AppConfig::default()),
        }
    }
}

impl ConfigStore for InMemoryConfigStore {
    fn get_config(&self) -> Result<AppConfig, DomainError> {
        Ok(self.config.lock().unwrap().clone())
    }

    fn update_config(&self, patch: ConfigPatch) -> Result<AppConfig, DomainError> {
        let mut config = self.config.lock().unwrap();
        crate::domain::model::config::apply_patch(&mut config, &patch);
        Ok(config.clone())
    }
}

// ── InMemoryHistoryRepository ───────────────────────────────────

struct InMemoryHistoryRepository {
    entries: Mutex<Vec<HistoryEntry>>,
    next_id: Mutex<u64>,
}

impl InMemoryHistoryRepository {
    fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            next_id: Mutex::new(1),
        }
    }
}

impl HistoryRepository for InMemoryHistoryRepository {
    fn record(&self, entry: &HistoryEntry) -> Result<(), DomainError> {
        let mut stored = entry.clone();
        let mut next = self.next_id.lock().unwrap();
        stored.id = *next;
        *next += 1;
        self.entries.lock().unwrap().push(stored);
        Ok(())
    }

    fn find_recent(&self, limit: usize) -> Result<Vec<HistoryEntry>, DomainError> {
        let entries = self.entries.lock().unwrap();
        Ok(entries.iter().rev().take(limit).cloned().collect())
    }

    fn find_by_download(&self, id: DownloadId) -> Result<Vec<HistoryEntry>, DomainError> {
        let entries = self.entries.lock().unwrap();
        Ok(entries
            .iter()
            .filter(|e| e.download_id == id)
            .cloned()
            .collect())
    }

    fn list(
        &self,
        filter: Option<HistoryFilter>,
        _sort: Option<HistorySort>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<HistoryEntry>, DomainError> {
        let entries = self.entries.lock().unwrap();
        let hostname = filter.as_ref().and_then(|f| {
            f.hostname
                .as_ref()
                .map(|h| h.trim().to_ascii_lowercase())
                .filter(|h| !h.is_empty())
        });
        let mut result: Vec<HistoryEntry> = entries
            .iter()
            .filter(|e| match &filter {
                None => true,
                Some(f) => {
                    f.date_from.is_none_or(|from| e.completed_at >= from)
                        && f.date_to.is_none_or(|to| e.completed_at <= to)
                }
            })
            .filter(|e| match &hostname {
                None => true,
                Some(host) => e
                    .url
                    .split_once("://")
                    .and_then(|(_, rest)| {
                        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
                        rest.get(..end)
                    })
                    .map(|authority| {
                        let trimmed = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
                        let no_port = trimmed.split_once(':').map_or(trimmed, |(h, _)| h);
                        no_port.eq_ignore_ascii_case(host)
                    })
                    .unwrap_or(false),
            })
            .cloned()
            .collect();
        result.sort_by_key(|e| std::cmp::Reverse(e.completed_at));
        let start = offset.unwrap_or(0);
        let take = limit.unwrap_or(usize::MAX);
        Ok(result.into_iter().skip(start).take(take).collect())
    }

    fn search(&self, query: &str) -> Result<Vec<HistoryEntry>, DomainError> {
        let needle = query.to_lowercase();
        let entries = self.entries.lock().unwrap();
        Ok(entries
            .iter()
            .filter(|e| {
                e.file_name.to_lowercase().contains(&needle)
                    || e.url.to_lowercase().contains(&needle)
                    || e.destination_path.to_lowercase().contains(&needle)
            })
            .cloned()
            .collect())
    }

    fn find_by_id(&self, id: u64) -> Result<Option<HistoryEntry>, DomainError> {
        let entries = self.entries.lock().unwrap();
        Ok(entries.iter().find(|e| e.id == id).cloned())
    }

    fn delete_by_id(&self, id: u64) -> Result<bool, DomainError> {
        let mut entries = self.entries.lock().unwrap();
        let before = entries.len();
        entries.retain(|e| e.id != id);
        Ok(before != entries.len())
    }

    fn delete_all(&self) -> Result<u64, DomainError> {
        let mut entries = self.entries.lock().unwrap();
        let count = entries.len() as u64;
        entries.clear();
        Ok(count)
    }

    fn delete_older_than(&self, before_timestamp: u64) -> Result<u64, DomainError> {
        let mut entries = self.entries.lock().unwrap();
        let before = entries.len();
        entries.retain(|e| e.completed_at >= before_timestamp);
        Ok((before - entries.len()) as u64)
    }
}

// ── InMemoryStatsRepository ─────────────────────────────────────

struct InMemoryStatsRepository {
    total_bytes: Mutex<u64>,
    total_files: Mutex<u64>,
}

impl InMemoryStatsRepository {
    fn new() -> Self {
        Self {
            total_bytes: Mutex::new(0),
            total_files: Mutex::new(0),
        }
    }
}

impl StatsRepository for InMemoryStatsRepository {
    fn record_completed(&self, bytes: u64, _avg_speed: u64) -> Result<(), DomainError> {
        *self.total_bytes.lock().unwrap() += bytes;
        *self.total_files.lock().unwrap() += 1;
        Ok(())
    }

    fn get_stats(
        &self,
        _: crate::domain::model::views::StatsPeriod,
    ) -> Result<StatsView, DomainError> {
        Ok(StatsView {
            total_downloaded_bytes: *self.total_bytes.lock().unwrap(),
            total_files: *self.total_files.lock().unwrap(),
            avg_speed: 0,
            peak_speed: 0,
            success_rate: 1.0,
            daily_volumes: vec![],
            top_hosts: vec![],
        })
    }

    fn top_modules(
        &self,
        _: u32,
    ) -> Result<Vec<crate::domain::model::views::ModuleStats>, DomainError> {
        Ok(vec![])
    }
}

// ── FakeClipboardObserver ───────────────────────────────────────

struct FakeClipboardObserver {
    urls: Mutex<Vec<String>>,
    running: Mutex<bool>,
}

impl FakeClipboardObserver {
    fn new() -> Self {
        Self {
            urls: Mutex::new(Vec::new()),
            running: Mutex::new(false),
        }
    }
}

impl ClipboardObserver for FakeClipboardObserver {
    fn start(&self) -> Result<(), DomainError> {
        *self.running.lock().unwrap() = true;
        Ok(())
    }

    fn stop(&self) -> Result<(), DomainError> {
        *self.running.lock().unwrap() = false;
        Ok(())
    }

    fn get_urls(&self) -> Result<Vec<String>, DomainError> {
        Ok(self.urls.lock().unwrap().drain(..).collect())
    }
}

// ── FakePluginLoader ────────────────────────────────────────────

struct FakePluginLoader {
    plugins: Mutex<HashMap<String, PluginInfo>>,
}

impl FakePluginLoader {
    fn new() -> Self {
        Self {
            plugins: Mutex::new(HashMap::new()),
        }
    }
}

impl PluginLoader for FakePluginLoader {
    fn load(&self, manifest: &PluginManifest) -> Result<(), DomainError> {
        let info = manifest.info().clone();
        self.plugins
            .lock()
            .unwrap()
            .insert(info.name().to_string(), info);
        Ok(())
    }

    fn unload(&self, name: &str) -> Result<(), DomainError> {
        self.plugins.lock().unwrap().remove(name);
        Ok(())
    }

    fn resolve_url(&self, _url: &str) -> Result<Option<PluginInfo>, DomainError> {
        Ok(None)
    }

    fn extract_links(&self, _url: &str) -> Result<String, DomainError> {
        Err(DomainError::NotFound(
            "extract_links not implemented".to_string(),
        ))
    }

    fn get_media_variants(&self, _url: &str) -> Result<String, DomainError> {
        Err(DomainError::NotFound(
            "get_media_variants not implemented".to_string(),
        ))
    }

    fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
        Ok(self.plugins.lock().unwrap().values().cloned().collect())
    }

    fn set_enabled(&self, _name: &str, _enabled: bool) -> Result<(), DomainError> {
        Ok(())
    }
}

// ── FakeDownloadEngine ──────────────────────────────────────────

struct FakeDownloadEngine {
    started: Mutex<Vec<DownloadId>>,
}

impl FakeDownloadEngine {
    fn new() -> Self {
        Self {
            started: Mutex::new(Vec::new()),
        }
    }
}

impl DownloadEngine for FakeDownloadEngine {
    fn start(&self, download: &Download) -> Result<(), DomainError> {
        self.started.lock().unwrap().push(download.id());
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

// ── FakeArchiveExtractor ────────────────────────────────────────

struct FakeArchiveExtractor;

impl ArchiveExtractor for FakeArchiveExtractor {
    fn detect_format(
        &self,
        _file_path: &Path,
    ) -> Result<Option<crate::domain::model::archive::ArchiveFormat>, DomainError> {
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
    ) -> Result<crate::domain::model::archive::ExtractSummary, DomainError> {
        Ok(crate::domain::model::archive::ExtractSummary {
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
    ) -> Result<Vec<crate::domain::model::archive::ArchiveEntry>, DomainError> {
        Ok(vec![])
    }

    fn detect_segments(
        &self,
        _file_path: &Path,
    ) -> Result<Option<Vec<std::path::PathBuf>>, DomainError> {
        Ok(None)
    }
}

// ── RecordingFileOpener ─────────────────────────────────────────

struct RecordingFileOpener {
    opened: Mutex<Vec<std::path::PathBuf>>,
    revealed: Mutex<Vec<std::path::PathBuf>>,
}

impl RecordingFileOpener {
    fn new() -> Self {
        Self {
            opened: Mutex::new(Vec::new()),
            revealed: Mutex::new(Vec::new()),
        }
    }
}

impl FileOpener for RecordingFileOpener {
    fn open_file(&self, path: &Path) -> Result<(), DomainError> {
        self.opened.lock().unwrap().push(path.to_path_buf());
        Ok(())
    }

    fn reveal_file(&self, path: &Path) -> Result<(), DomainError> {
        self.revealed.lock().unwrap().push(path.to_path_buf());
        Ok(())
    }
}

// ── InMemoryAccountRepository ────────────────────────────────────

struct InMemoryAccountRepository {
    store: Mutex<HashMap<AccountId, Account>>,
}

impl InMemoryAccountRepository {
    fn new() -> Self {
        Self {
            store: Mutex::new(HashMap::new()),
        }
    }
}

impl AccountRepository for InMemoryAccountRepository {
    fn find_by_id(&self, id: &AccountId) -> Result<Option<Account>, DomainError> {
        Ok(self.store.lock().unwrap().get(id).cloned())
    }

    fn save(&self, account: &Account) -> Result<(), DomainError> {
        let mut guard = self.store.lock().unwrap();
        // Detect (service, username) collision against another id
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

        // Mirror the SQLite adapter: `created_at` is insert-only. On re-save
        // of the same id, keep the existing timestamp so the mock cannot
        // diverge from production on list ordering or round-trip behavior.
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
        let mut accounts: Vec<Account> = self.store.lock().unwrap().values().cloned().collect();
        // Secondary sort by id breaks ties from `HashMap` iteration order
        // when multiple accounts share a `created_at`, mirroring the SQLite
        // adapter which orders by (created_at, id).
        accounts.sort_by(|a, b| {
            a.created_at()
                .cmp(&b.created_at())
                .then_with(|| a.id().as_str().cmp(b.id().as_str()))
        });
        Ok(accounts)
    }

    fn list_by_service(&self, service_name: &str) -> Result<Vec<Account>, DomainError> {
        let mut accounts: Vec<Account> = self
            .store
            .lock()
            .unwrap()
            .values()
            .filter(|a| a.service_name() == service_name)
            .cloned()
            .collect();
        accounts.sort_by(|a, b| {
            a.created_at()
                .cmp(&b.created_at())
                .then_with(|| a.id().as_str().cmp(b.id().as_str()))
        });
        Ok(accounts)
    }

    fn delete(&self, id: &AccountId) -> Result<(), DomainError> {
        self.store.lock().unwrap().remove(id);
        Ok(())
    }
}

// ── InMemoryPackageRepository ────────────────────────────────────

struct InMemoryPackageRepository {
    store: Mutex<HashMap<PackageId, Package>>,
    /// Members are stored as `(queue_position, download_id)` so that
    /// `list_downloads` can mirror the SQLite adapter's ordering
    /// contract (asc by `queue_position`).
    members: Mutex<HashMap<PackageId, Vec<(i64, DownloadId)>>>,
}

impl InMemoryPackageRepository {
    fn new() -> Self {
        Self {
            store: Mutex::new(HashMap::new()),
            members: Mutex::new(HashMap::new()),
        }
    }

    fn seed_member(&self, package_id: &PackageId, queue_position: i64, download: DownloadId) {
        self.members
            .lock()
            .unwrap()
            .entry(package_id.clone())
            .or_default()
            .push((queue_position, download));
    }
}

impl PackageRepository for InMemoryPackageRepository {
    fn find_by_id(&self, id: &PackageId) -> Result<Option<Package>, DomainError> {
        Ok(self.store.lock().unwrap().get(id).cloned())
    }

    fn save(&self, package: &Package) -> Result<(), DomainError> {
        let mut guard = self.store.lock().unwrap();
        // Mirror SQLite: created_at is insert-only.
        let created_at = match guard.get(package.id()) {
            Some(existing) => existing.created_at(),
            None => package.created_at(),
        };
        let stored = Package::reconstruct(
            package.id().clone(),
            package.name().to_string(),
            package.source_type(),
            package.folder_path().map(str::to_string),
            package.password().map(str::to_string),
            package.auto_extract(),
            package.priority(),
            created_at,
        )?;
        guard.insert(package.id().clone(), stored);
        Ok(())
    }

    fn list(&self) -> Result<Vec<Package>, DomainError> {
        let mut packages: Vec<Package> = self.store.lock().unwrap().values().cloned().collect();
        packages.sort_by(|a, b| {
            a.created_at()
                .cmp(&b.created_at())
                .then_with(|| a.id().as_str().cmp(b.id().as_str()))
        });
        Ok(packages)
    }

    fn delete(&self, id: &PackageId) -> Result<(), DomainError> {
        self.store.lock().unwrap().remove(id);
        // FK ON DELETE SET NULL semantics: detach members but keep them.
        self.members.lock().unwrap().remove(id);
        Ok(())
    }

    fn list_downloads(&self, id: &PackageId) -> Result<Vec<DownloadId>, DomainError> {
        let mut members = self
            .members
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .unwrap_or_default();
        members.sort_by(|(qa, da), (qb, db)| qa.cmp(qb).then_with(|| da.0.cmp(&db.0)));
        Ok(members.into_iter().map(|(_, id)| id).collect())
    }

    fn attach_download(
        &self,
        package_id: &PackageId,
        download_id: DownloadId,
    ) -> Result<(), DomainError> {
        let mut guard = self.members.lock().unwrap();
        // Same-package reattach must be a true no-op so the mock mirrors
        // the FK-singleton semantics of the SQL adapter (which never
        // rewrites `queue_position` on `UPDATE ... WHERE package_id =
        // same`). Detach from foreign packages first, bail if the
        // download is already in the target bucket so its existing
        // position survives.
        let already_in_target = guard
            .get(package_id)
            .is_some_and(|entries| entries.iter().any(|(_, id)| id == &download_id));
        for (pkg, entries) in guard.iter_mut() {
            if pkg != package_id {
                entries.retain(|(_, id)| id != &download_id);
            }
        }
        if already_in_target {
            return Ok(());
        }
        let bucket = guard.entry(package_id.clone()).or_default();
        let next_position = bucket
            .iter()
            .map(|(p, _)| *p)
            .max()
            .map(|m| m + 1)
            .unwrap_or(0);
        bucket.push((next_position, download_id));
        Ok(())
    }

    fn detach_download(&self, download_id: DownloadId) -> Result<(), DomainError> {
        let mut guard = self.members.lock().unwrap();
        for entries in guard.values_mut() {
            entries.retain(|(_, id)| id != &download_id);
        }
        Ok(())
    }

    fn find_package_of_download(
        &self,
        download_id: DownloadId,
    ) -> Result<Option<PackageId>, DomainError> {
        let guard = self.members.lock().unwrap();
        for (pkg, entries) in guard.iter() {
            if entries.iter().any(|(_, id)| id == &download_id) {
                return Ok(Some(pkg.clone()));
            }
        }
        Ok(None)
    }
}

#[test]
fn in_memory_account_repository_round_trip_preserves_fields() {
    let repo = InMemoryAccountRepository::new();
    let mut account = Account::new(
        AccountId::new("acc-rt"),
        "real-debrid".to_string(),
        "alice".to_string(),
        AccountType::Debrid,
        1_700_000_000_000,
    );
    account.set_traffic_left(10);
    account.set_traffic_total(20);
    account.set_valid_until(30);
    account.set_last_validated(40);
    account.disable();

    repo.save(&account).expect("save");
    let found = repo
        .find_by_id(&AccountId::new("acc-rt"))
        .expect("find")
        .expect("present");
    assert_eq!(found, account);
}

#[test]
fn in_memory_account_repository_save_preserves_original_created_at() {
    // Same divergence guard as the SQLite adapter: re-saving a known id
    // must not rewrite created_at, otherwise list ordering becomes
    // unstable across writes.
    let repo = InMemoryAccountRepository::new();
    let original = Account::new(
        AccountId::new("acc-stable"),
        "real-debrid".to_string(),
        "alice".to_string(),
        AccountType::Debrid,
        1_700_000_000_000,
    );
    repo.save(&original).expect("first save");

    let updated = Account::new(
        AccountId::new("acc-stable"),
        "real-debrid".to_string(),
        "alice".to_string(),
        AccountType::Debrid,
        9_999_999_999_999,
    );
    repo.save(&updated).expect("upsert");

    let found = repo
        .find_by_id(&AccountId::new("acc-stable"))
        .expect("find")
        .expect("present");
    assert_eq!(found.created_at(), 1_700_000_000_000);
}

#[test]
fn in_memory_account_repository_unique_constraint_rejects_duplicate_service_username() {
    let repo = InMemoryAccountRepository::new();
    let a = Account::new(
        AccountId::new("acc-a"),
        "real-debrid".to_string(),
        "bob".to_string(),
        AccountType::Debrid,
        0,
    );
    let b = Account::new(
        AccountId::new("acc-b"),
        "real-debrid".to_string(),
        "bob".to_string(),
        AccountType::Debrid,
        0,
    );
    repo.save(&a).expect("first save");
    let err = repo.save(&b).expect_err("conflicting save must fail");
    assert!(matches!(err, DomainError::AlreadyExists(_)));
}

#[test]
fn in_memory_account_repository_list_by_service_filters_correctly() {
    let repo = InMemoryAccountRepository::new();
    let rd = Account::new(
        AccountId::new("rd-1"),
        "real-debrid".to_string(),
        "alice".to_string(),
        AccountType::Debrid,
        1,
    );
    let ad = Account::new(
        AccountId::new("ad-1"),
        "alldebrid".to_string(),
        "alice".to_string(),
        AccountType::Debrid,
        2,
    );
    repo.save(&rd).expect("save rd");
    repo.save(&ad).expect("save ad");
    let only_rd = repo.list_by_service("real-debrid").expect("filter");
    assert_eq!(only_rd.len(), 1);
    assert_eq!(only_rd[0].id().as_str(), "rd-1");
}

// ── Send + Sync compile-time assertions ─────────────────────────

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn all_driven_port_mocks_are_send_sync() {
    assert_send_sync::<InMemoryDownloadRepository>();
    assert_send_sync::<InMemoryDownloadReadRepository>();
    assert_send_sync::<CollectingEventBus>();
    assert_send_sync::<InMemoryFileStorage>();
    assert_send_sync::<FakeHttpClient>();
    assert_send_sync::<InMemoryCredentialStore>();
    assert_send_sync::<InMemoryConfigStore>();
    assert_send_sync::<InMemoryHistoryRepository>();
    assert_send_sync::<InMemoryStatsRepository>();
    assert_send_sync::<FakeClipboardObserver>();
    assert_send_sync::<FakePluginLoader>();
    assert_send_sync::<FakeDownloadEngine>();
    assert_send_sync::<FakeArchiveExtractor>();
    assert_send_sync::<RecordingFileOpener>();
    assert_send_sync::<InMemoryAccountRepository>();
    assert_send_sync::<InMemoryPackageRepository>();
}

#[test]
fn in_memory_package_repository_round_trip_preserves_all_fields() {
    let repo = InMemoryPackageRepository::new();
    let mut pkg = Package::new(
        PackageId::new("pkg-rt"),
        "Holiday photos".to_string(),
        PackageSourceType::Playlist,
        1_700_000_000_000,
    );
    pkg.set_folder_path(Some("/tmp/holiday".to_string()));
    pkg.set_password(Some("keyring://pkg/holiday".to_string()));
    pkg.set_auto_extract(false);
    pkg.set_priority(8).expect("valid priority");

    repo.save(&pkg).expect("save");
    let found = repo
        .find_by_id(&PackageId::new("pkg-rt"))
        .expect("find")
        .expect("present");
    assert_eq!(found.id().as_str(), "pkg-rt");
    assert_eq!(found.name(), "Holiday photos");
    assert_eq!(found.source_type(), PackageSourceType::Playlist);
    assert_eq!(found.folder_path(), Some("/tmp/holiday"));
    assert_eq!(found.password(), Some("keyring://pkg/holiday"));
    assert!(!found.auto_extract());
    assert_eq!(found.priority(), 8);
    assert_eq!(found.created_at(), 1_700_000_000_000);
}

#[test]
fn in_memory_package_repository_save_preserves_original_created_at() {
    let repo = InMemoryPackageRepository::new();
    let original = Package::new(
        PackageId::new("pkg-stable"),
        "Vol 1".to_string(),
        PackageSourceType::Manual,
        1_700_000_000_000,
    );
    repo.save(&original).expect("first save");

    let updated = Package::new(
        PackageId::new("pkg-stable"),
        "Vol 1 — updated".to_string(),
        PackageSourceType::Manual,
        9_999_999_999_999,
    );
    repo.save(&updated).expect("upsert");

    let found = repo
        .find_by_id(&PackageId::new("pkg-stable"))
        .expect("find")
        .expect("present");
    assert_eq!(found.created_at(), 1_700_000_000_000);
    assert_eq!(found.name(), "Vol 1 — updated");
}

#[test]
fn in_memory_package_repository_list_orders_by_created_at_then_id() {
    let repo = InMemoryPackageRepository::new();
    repo.save(&Package::new(
        PackageId::new("c"),
        "C".to_string(),
        PackageSourceType::Manual,
        20,
    ))
    .unwrap();
    repo.save(&Package::new(
        PackageId::new("a"),
        "A".to_string(),
        PackageSourceType::Manual,
        10,
    ))
    .unwrap();
    repo.save(&Package::new(
        PackageId::new("b"),
        "B".to_string(),
        PackageSourceType::Manual,
        10,
    ))
    .unwrap();

    let listed = repo.list().expect("list");
    assert_eq!(listed.len(), 3);
    // Ordered by (created_at asc, id asc) → a, b, c
    assert_eq!(listed[0].id().as_str(), "a");
    assert_eq!(listed[1].id().as_str(), "b");
    assert_eq!(listed[2].id().as_str(), "c");
}

#[test]
fn in_memory_package_repository_delete_drops_member_attachments() {
    let repo = InMemoryPackageRepository::new();
    let pkg = Package::new(
        PackageId::new("pkg-del"),
        "Doomed".to_string(),
        PackageSourceType::Manual,
        0,
    );
    repo.save(&pkg).unwrap();
    repo.seed_member(&PackageId::new("pkg-del"), 0, DownloadId(1));
    assert_eq!(
        repo.list_downloads(&PackageId::new("pkg-del"))
            .unwrap()
            .len(),
        1
    );

    repo.delete(&PackageId::new("pkg-del")).unwrap();
    assert!(
        repo.find_by_id(&PackageId::new("pkg-del"))
            .unwrap()
            .is_none()
    );
    assert!(
        repo.list_downloads(&PackageId::new("pkg-del"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn in_memory_package_repository_list_downloads_returns_attached_ids() {
    let repo = InMemoryPackageRepository::new();
    let pkg_id = PackageId::new("pkg-x");
    repo.save(&Package::new(
        pkg_id.clone(),
        "X".to_string(),
        PackageSourceType::Manual,
        0,
    ))
    .unwrap();
    repo.seed_member(&pkg_id, 0, DownloadId(7));
    repo.seed_member(&pkg_id, 1, DownloadId(11));

    let members = repo.list_downloads(&pkg_id).unwrap();
    assert_eq!(members, vec![DownloadId(7), DownloadId(11)]);
    // Other packages have no members.
    assert!(
        repo.list_downloads(&PackageId::new("ghost"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn in_memory_package_repository_attach_download_via_trait_adds_member() {
    let repo = InMemoryPackageRepository::new();
    let pkg_id = PackageId::new("pkg-att");
    repo.save(&Package::new(
        pkg_id.clone(),
        "Att".to_string(),
        PackageSourceType::Manual,
        0,
    ))
    .unwrap();
    repo.attach_download(&pkg_id, DownloadId(1)).unwrap();
    repo.attach_download(&pkg_id, DownloadId(2)).unwrap();
    let members = repo.list_downloads(&pkg_id).unwrap();
    assert_eq!(members, vec![DownloadId(1), DownloadId(2)]);
}

#[test]
fn in_memory_package_repository_attach_download_moves_from_other_package() {
    let repo = InMemoryPackageRepository::new();
    let a = PackageId::new("pkg-a");
    let b = PackageId::new("pkg-b");
    repo.save(&Package::new(
        a.clone(),
        "A".to_string(),
        PackageSourceType::Manual,
        0,
    ))
    .unwrap();
    repo.save(&Package::new(
        b.clone(),
        "B".to_string(),
        PackageSourceType::Manual,
        0,
    ))
    .unwrap();
    repo.attach_download(&a, DownloadId(99)).unwrap();
    repo.attach_download(&b, DownloadId(99)).unwrap();
    assert!(repo.list_downloads(&a).unwrap().is_empty());
    assert_eq!(repo.list_downloads(&b).unwrap(), vec![DownloadId(99)]);
}

#[test]
fn in_memory_package_repository_detach_download_removes_member() {
    let repo = InMemoryPackageRepository::new();
    let pkg_id = PackageId::new("pkg-det");
    repo.save(&Package::new(
        pkg_id.clone(),
        "Det".to_string(),
        PackageSourceType::Manual,
        0,
    ))
    .unwrap();
    repo.attach_download(&pkg_id, DownloadId(5)).unwrap();
    repo.detach_download(DownloadId(5)).unwrap();
    assert!(repo.list_downloads(&pkg_id).unwrap().is_empty());
    // Idempotent: detaching again is a no-op.
    repo.detach_download(DownloadId(5)).unwrap();
}

#[test]
fn in_memory_package_repository_attach_download_same_package_preserves_position() {
    let repo = InMemoryPackageRepository::new();
    let pkg_id = PackageId::new("pkg-stable");
    repo.save(&Package::new(
        pkg_id.clone(),
        "Stable".to_string(),
        PackageSourceType::Manual,
        0,
    ))
    .unwrap();
    repo.attach_download(&pkg_id, DownloadId(10)).unwrap();
    repo.attach_download(&pkg_id, DownloadId(20)).unwrap();

    // Re-attach the first download; without the no-op guard it would
    // shift to the end of the bucket and inflate queue_position.
    repo.attach_download(&pkg_id, DownloadId(10)).unwrap();

    let members = repo.list_downloads(&pkg_id).unwrap();
    assert_eq!(
        members,
        vec![DownloadId(10), DownloadId(20)],
        "same-package reattach must not reorder existing members"
    );
}

#[test]
fn in_memory_package_repository_find_package_of_download_returns_owner() {
    let repo = InMemoryPackageRepository::new();
    let pkg_id = PackageId::new("pkg-find");
    repo.save(&Package::new(
        pkg_id.clone(),
        "Find".to_string(),
        PackageSourceType::Manual,
        0,
    ))
    .unwrap();
    repo.attach_download(&pkg_id, DownloadId(42)).unwrap();

    let owner = repo.find_package_of_download(DownloadId(42)).unwrap();
    assert_eq!(owner, Some(pkg_id));
    assert!(
        repo.find_package_of_download(DownloadId(404))
            .unwrap()
            .is_none(),
        "missing or loose downloads return None"
    );
}

#[test]
fn in_memory_package_repository_list_downloads_orders_by_queue_position() {
    // Mock must mirror the SQLite adapter's contract: members come back
    // ordered by queue_position regardless of insertion order, otherwise
    // port-level tests would let production diverge from the mock.
    let repo = InMemoryPackageRepository::new();
    let pkg_id = PackageId::new("pkg-order");
    repo.save(&Package::new(
        pkg_id.clone(),
        "Ordered".to_string(),
        PackageSourceType::Manual,
        0,
    ))
    .unwrap();
    // Insert out of order on purpose.
    repo.seed_member(&pkg_id, 5, DownloadId(50));
    repo.seed_member(&pkg_id, 1, DownloadId(10));
    repo.seed_member(&pkg_id, 3, DownloadId(30));

    let members = repo.list_downloads(&pkg_id).unwrap();
    assert_eq!(
        members,
        vec![DownloadId(10), DownloadId(30), DownloadId(50)],
        "list_downloads must sort by queue_position asc"
    );
}

#[test]
fn file_opener_records_open_and_reveal_calls() {
    let opener = RecordingFileOpener::new();
    opener.open_file(Path::new("/tmp/file.mp4")).unwrap();
    opener.reveal_file(Path::new("/tmp/file.mp4")).unwrap();

    let opened = opener.opened.lock().unwrap();
    let revealed = opener.revealed.lock().unwrap();
    assert_eq!(opened.len(), 1);
    assert_eq!(revealed.len(), 1);
    assert_eq!(opened[0], Path::new("/tmp/file.mp4"));
    assert_eq!(revealed[0], Path::new("/tmp/file.mp4"));
}

// ── Functional tests ────────────────────────────────────────────

#[test]
fn download_repository_save_and_find() {
    let repo = InMemoryDownloadRepository::new();
    let url = crate::domain::model::download::Url::new("https://example.com/file.zip").unwrap();
    let download = Download::new(
        DownloadId(1),
        url,
        "file.zip".to_string(),
        "/tmp".to_string(),
    );

    repo.save(&download).unwrap();
    let found = repo.find_by_id(DownloadId(1)).unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().file_name(), "file.zip");
}

#[test]
fn download_repository_find_by_state() {
    let repo = InMemoryDownloadRepository::new();
    let url = crate::domain::model::download::Url::new("https://example.com/a.zip").unwrap();
    let download = Download::new(DownloadId(1), url, "a.zip".to_string(), "/tmp".to_string());

    repo.save(&download).unwrap();
    let queued = repo.find_by_state(DownloadState::Queued).unwrap();
    assert_eq!(queued.len(), 1);

    let downloading = repo.find_by_state(DownloadState::Downloading).unwrap();
    assert!(downloading.is_empty());
}

#[test]
fn download_repository_delete() {
    let repo = InMemoryDownloadRepository::new();
    let url = crate::domain::model::download::Url::new("https://example.com/b.zip").unwrap();
    let download = Download::new(DownloadId(2), url, "b.zip".to_string(), "/tmp".to_string());

    repo.save(&download).unwrap();
    repo.delete(DownloadId(2)).unwrap();
    assert!(repo.find_by_id(DownloadId(2)).unwrap().is_none());
}

#[test]
fn download_read_repository_has_no_save_method() {
    // This is a compile-time check: DownloadReadRepository doesn't expose save().
    // If someone adds save() to the trait, this test file won't compile
    // because InMemoryDownloadReadRepository doesn't implement it.
    let repo = InMemoryDownloadReadRepository;
    let _ = repo.count_by_state().unwrap();
}

#[test]
fn event_bus_collects_events() {
    let bus = CollectingEventBus::new();
    bus.publish(DomainEvent::DownloadStarted { id: DownloadId(1) });
    bus.publish(DomainEvent::DownloadCompleted { id: DownloadId(1) });
    assert_eq!(bus.events.lock().unwrap().len(), 2);
}

#[test]
fn file_storage_create_and_write() {
    let storage = InMemoryFileStorage::new();
    let path = Path::new("/tmp/test.bin");

    storage.create_file(path, 1024).unwrap();
    storage.write_segment(path, 0, &[1, 2, 3, 4]).unwrap();

    let file = storage.files.lock().unwrap();
    let data = file.get("/tmp/test.bin").unwrap();
    assert_eq!(&data[0..4], &[1, 2, 3, 4]);
    assert_eq!(data.len(), 1024);
}

#[test]
fn file_storage_meta_roundtrip() {
    let storage = InMemoryFileStorage::new();
    let path = Path::new("/tmp/test.vortex-meta");
    let meta = DownloadMeta {
        download_id: DownloadId(42),
        url: "https://example.com/file.zip".to_string(),
        file_name: "file.zip".to_string(),
        total_bytes: Some(1024),
        segments: vec![],
        checksum_expected: None,
        created_at: 100,
        updated_at: 200,
    };

    storage.write_meta(path, &meta).unwrap();
    let loaded = storage.read_meta(path).unwrap().unwrap();
    assert_eq!(loaded.download_id, DownloadId(42));
    assert_eq!(loaded.total_bytes, Some(1024));

    storage.delete_meta(path).unwrap();
    assert!(storage.read_meta(path).unwrap().is_none());
}

#[test]
fn http_client_head_and_range() {
    let client = FakeHttpClient;

    let head = client.head("https://example.com/file.zip").unwrap();
    assert_eq!(head.status_code, 200);
    assert!(head.is_success());
    assert_eq!(head.content_length(), Some(1024));

    let range_data = client
        .get_range("https://example.com/file.zip", 0, 99)
        .unwrap();
    assert_eq!(range_data.len(), 100);

    assert!(
        client
            .supports_range("https://example.com/file.zip")
            .unwrap()
    );
}

#[test]
fn credential_store_crud() {
    let store = InMemoryCredentialStore::new();
    let cred = Credential::new("test-user", "test-value");

    assert!(store.get("mega").unwrap().is_none());

    store.store("mega", &cred).unwrap();
    let loaded = store.get("mega").unwrap().unwrap();
    assert_eq!(loaded.username(), "test-user");
    assert_eq!(loaded.password(), "test-value");

    store.delete("mega").unwrap();
    assert!(store.get("mega").unwrap().is_none());
}

#[test]
fn config_store_get_and_update() {
    let store = InMemoryConfigStore::new();
    let config = store.get_config().unwrap();
    assert_eq!(config.max_concurrent_downloads, 4);

    let patch = ConfigPatch {
        max_concurrent_downloads: Some(10),
        download_dir: Some(Some("/downloads".to_string())),
        ..Default::default()
    };
    let updated = store.update_config(patch).unwrap();
    assert_eq!(updated.max_concurrent_downloads, 10);
    assert_eq!(updated.download_dir, Some("/downloads".to_string()));
}

fn make_history_entry(download_id: u64, completed_at: u64, name: &str) -> HistoryEntry {
    HistoryEntry {
        id: 0,
        download_id: DownloadId(download_id),
        file_name: name.to_string(),
        url: format!("https://example.com/{name}"),
        total_bytes: 1024,
        completed_at,
        duration_seconds: 60,
        avg_speed: 17,
        destination_path: format!("/tmp/{name}"),
    }
}

#[test]
fn history_repository_record_and_find() {
    let repo = InMemoryHistoryRepository::new();
    repo.record(&make_history_entry(1, 1000, "file.zip"))
        .unwrap();
    let recent = repo.find_recent(10).unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].file_name, "file.zip");
    assert_ne!(recent[0].id, 0, "record assigns a primary key");

    let by_dl = repo.find_by_download(DownloadId(1)).unwrap();
    assert_eq!(by_dl.len(), 1);

    let deleted = repo.delete_older_than(2000).unwrap();
    assert_eq!(deleted, 1);
    assert!(repo.find_recent(10).unwrap().is_empty());
}

#[test]
fn history_repository_list_filters_and_paginates() {
    let repo = InMemoryHistoryRepository::new();
    repo.record(&make_history_entry(1, 1000, "a.zip")).unwrap();
    repo.record(&make_history_entry(2, 2000, "b.zip")).unwrap();
    repo.record(&make_history_entry(3, 3000, "c.zip")).unwrap();

    let page = repo.list(None, None, Some(2), Some(0)).unwrap();
    assert_eq!(page.len(), 2);
    assert_eq!(page[0].completed_at, 3000);

    let second_page = repo.list(None, None, Some(2), Some(2)).unwrap();
    assert_eq!(second_page.len(), 1);
    assert_eq!(second_page[0].completed_at, 1000);

    let filtered = repo
        .list(
            Some(HistoryFilter {
                date_from: Some(1500),
                date_to: Some(2500),
                hostname: None,
            }),
            None,
            None,
            None,
        )
        .unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].completed_at, 2000);
}

#[test]
fn history_repository_search_matches_filename_and_url() {
    let repo = InMemoryHistoryRepository::new();
    repo.record(&make_history_entry(1, 1000, "alpha.zip"))
        .unwrap();
    repo.record(&make_history_entry(2, 2000, "beta.txt"))
        .unwrap();

    let hits = repo.search("alpha").unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].file_name, "alpha.zip");

    let url_hits = repo.search("example.com").unwrap();
    assert_eq!(url_hits.len(), 2);
}

#[test]
fn history_repository_delete_by_id_and_clear() {
    let repo = InMemoryHistoryRepository::new();
    repo.record(&make_history_entry(1, 1000, "one.zip"))
        .unwrap();
    repo.record(&make_history_entry(2, 2000, "two.zip"))
        .unwrap();
    let all = repo.find_recent(10).unwrap();
    let victim = all.first().expect("entry exists");

    let removed = repo.delete_by_id(victim.id).unwrap();
    assert!(removed);
    let missing = repo.delete_by_id(9999).unwrap();
    assert!(!missing);

    let cleared = repo.delete_all().unwrap();
    assert_eq!(cleared, 1);
    assert!(repo.find_recent(10).unwrap().is_empty());
}

#[test]
fn stats_repository_record_and_get() {
    let repo = InMemoryStatsRepository::new();
    repo.record_completed(1024, 512).unwrap();
    repo.record_completed(2048, 1024).unwrap();

    let stats = repo
        .get_stats(crate::domain::model::views::StatsPeriod::AllTime)
        .unwrap();
    assert_eq!(stats.total_downloaded_bytes, 3072);
    assert_eq!(stats.total_files, 2);
}

#[test]
fn clipboard_observer_lifecycle() {
    let observer = FakeClipboardObserver::new();
    observer.start().unwrap();
    assert!(*observer.running.lock().unwrap());

    // Simulate clipboard detection
    observer
        .urls
        .lock()
        .unwrap()
        .push("https://example.com".to_string());
    let urls = observer.get_urls().unwrap();
    assert_eq!(urls.len(), 1);

    // Buffer drained after get_urls
    assert!(observer.get_urls().unwrap().is_empty());

    observer.stop().unwrap();
    assert!(!*observer.running.lock().unwrap());
}

#[test]
fn plugin_loader_lifecycle() {
    let loader = FakePluginLoader::new();
    let info = PluginInfo::new(
        "test-plugin".to_string(),
        "1.0.0".to_string(),
        "A test plugin".to_string(),
        "author".to_string(),
        PluginCategory::Crawler,
    );
    let manifest = PluginManifest::new(info);

    loader.load(&manifest).unwrap();
    let loaded = loader.list_loaded().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].name(), "test-plugin");

    assert!(loader.resolve_url("https://example.com").unwrap().is_none());

    loader.unload("test-plugin").unwrap();
    assert!(loader.list_loaded().unwrap().is_empty());
}

#[test]
fn download_engine_fire_and_forget() {
    let engine = FakeDownloadEngine::new();
    let url = crate::domain::model::download::Url::new("https://example.com/big.iso").unwrap();
    let download = Download::new(
        DownloadId(42),
        url,
        "big.iso".to_string(),
        "/tmp".to_string(),
    );

    engine.start(&download).unwrap();
    assert_eq!(engine.started.lock().unwrap().len(), 1);
    assert_eq!(engine.started.lock().unwrap()[0], DownloadId(42));

    engine.pause(DownloadId(42)).unwrap();
    engine.cancel(DownloadId(42)).unwrap();
}

#[test]
fn credential_debug_redacts_password() {
    let cred = Credential::new("test-user", "test-credential-value");
    let debug_output = format!("{cred:?}");
    assert!(debug_output.contains("user"));
    assert!(debug_output.contains("<redacted>"));
    assert!(!debug_output.contains("test-credential-value"));
}

// ── Driving port compile-time checks ────────────────────────────

#[test]
fn driving_port_traits_compile() {
    use crate::domain::ports::driving::{Command, CommandHandler, Query, QueryHandler};

    // Verify marker traits can be implemented
    struct TestCommand;
    impl Command for TestCommand {}

    struct TestQuery;
    impl Query for TestQuery {}

    // Verify handler traits can be implemented
    struct TestCommandHandler;
    impl CommandHandler<TestCommand> for TestCommandHandler {
        type Output = u64;
        async fn handle(&self, _cmd: TestCommand) -> Result<u64, DomainError> {
            Ok(42)
        }
    }

    struct TestQueryHandler;
    impl QueryHandler<TestQuery> for TestQueryHandler {
        type Output = String;
        async fn handle(&self, _query: TestQuery) -> Result<String, DomainError> {
            Ok("result".to_string())
        }
    }

    assert_send_sync::<TestCommandHandler>();
    assert_send_sync::<TestQueryHandler>();
}
