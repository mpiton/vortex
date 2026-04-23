//! Shared test doubles reused across command/query handler tests.
//!
//! Gated behind `#[cfg(test)]` so they never leak into release binaries.

#![cfg(test)]

use std::path::Path;
use std::sync::{Arc, Mutex};

use std::collections::HashMap;

use crate::application::command_bus::CommandBus;
use crate::application::query_bus::QueryBus;
use crate::domain::error::DomainError;
use crate::domain::event::DomainEvent;
use crate::domain::model::archive::{ArchiveEntry, ArchiveFormat, ExtractSummary};
use crate::domain::model::config::{AppConfig, ConfigPatch};
use crate::domain::model::credential::Credential;
use crate::domain::model::download::{Download, DownloadId, DownloadState};
use crate::domain::model::http::HttpResponse;
use crate::domain::model::meta::DownloadMeta;
use crate::domain::model::plugin::{PluginInfo, PluginManifest};
use crate::domain::model::views::{
    DownloadDetailView, DownloadFilter, DownloadView, HistoryEntry, HistoryFilter, HistorySort,
    HistorySortField, SortDirection, SortOrder, StateCountMap, StatsView,
};
use crate::domain::ports::driven::history_repository::{
    MAX_HISTORY_PAGE_SIZE, MAX_HISTORY_SEARCH_RESULTS,
};
use crate::domain::ports::driven::{
    ArchiveExtractor, ClipboardObserver, ConfigStore, CredentialStore, DownloadEngine,
    DownloadReadRepository, DownloadRepository, EventBus, FileStorage, HistoryRepository,
    HttpClient, PluginLoader, PluginReadRepository, StatsRepository,
};

fn host_component(url: &str) -> Option<&str> {
    let (_, after_scheme) = url.split_once("://")?;
    let authority_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let authority = &after_scheme[..authority_end];
    let host_with_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if host_with_port.starts_with('[') {
        host_with_port
            .find(']')
            .map_or(host_with_port, |end| &host_with_port[..=end])
    } else {
        host_with_port
            .split_once(':')
            .map_or(host_with_port, |(h, _)| h)
    };
    if host.is_empty() { None } else { Some(host) }
}

/// Minimal [`HistoryRepository`] impl that records nothing and returns defaults.
///
/// Useful when the command under test does not exercise history — it still
/// needs a history port to construct the bus.
pub(crate) struct NoopHistoryRepo;

impl HistoryRepository for NoopHistoryRepo {
    fn record(&self, _entry: &HistoryEntry) -> Result<(), DomainError> {
        Ok(())
    }

    fn find_recent(&self, _limit: usize) -> Result<Vec<HistoryEntry>, DomainError> {
        Ok(vec![])
    }

    fn find_by_download(&self, _id: DownloadId) -> Result<Vec<HistoryEntry>, DomainError> {
        Ok(vec![])
    }

    fn list(
        &self,
        _filter: Option<HistoryFilter>,
        _sort: Option<HistorySort>,
        _limit: Option<usize>,
        _offset: Option<usize>,
    ) -> Result<Vec<HistoryEntry>, DomainError> {
        Ok(vec![])
    }

    fn search(&self, _query: &str) -> Result<Vec<HistoryEntry>, DomainError> {
        Ok(vec![])
    }

    fn find_by_id(&self, _id: u64) -> Result<Option<HistoryEntry>, DomainError> {
        Ok(None)
    }

    fn delete_by_id(&self, _id: u64) -> Result<bool, DomainError> {
        Ok(false)
    }

    fn delete_all(&self) -> Result<u64, DomainError> {
        Ok(0)
    }

    fn delete_older_than(&self, _before_timestamp: u64) -> Result<u64, DomainError> {
        Ok(0)
    }
}

/// In-memory history repo that assigns sequential ids and supports filters.
pub(crate) struct InMemoryHistoryRepo {
    entries: Mutex<Vec<HistoryEntry>>,
    next_id: Mutex<u64>,
}

impl InMemoryHistoryRepo {
    pub(crate) fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            next_id: Mutex::new(1),
        }
    }

    pub(crate) fn snapshot(&self) -> Vec<HistoryEntry> {
        self.entries.lock().unwrap().clone()
    }
}

impl HistoryRepository for InMemoryHistoryRepo {
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
        let mut sorted: Vec<HistoryEntry> = entries.clone();
        sorted.sort_by_key(|e| std::cmp::Reverse(e.completed_at));
        Ok(sorted.into_iter().take(limit).collect())
    }

    fn find_by_download(&self, id: DownloadId) -> Result<Vec<HistoryEntry>, DomainError> {
        Ok(self
            .entries
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.download_id == id)
            .cloned()
            .collect())
    }

    fn list(
        &self,
        filter: Option<HistoryFilter>,
        sort: Option<HistorySort>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<HistoryEntry>, DomainError> {
        let entries = self.entries.lock().unwrap();
        let hostname_filter = filter.as_ref().and_then(|f| {
            f.hostname
                .as_ref()
                .map(|h| h.trim())
                .filter(|h| !h.is_empty())
                .map(|h| h.to_ascii_lowercase())
        });
        let mut filtered: Vec<HistoryEntry> = entries
            .iter()
            .filter(|e| match &filter {
                None => true,
                Some(f) => {
                    f.date_from.is_none_or(|from| e.completed_at >= from)
                        && f.date_to.is_none_or(|to| e.completed_at <= to)
                }
            })
            .filter(|e| match &hostname_filter {
                None => true,
                Some(wanted) => host_component(&e.url)
                    .map(|h| h.to_ascii_lowercase() == *wanted)
                    .unwrap_or(false),
            })
            .cloned()
            .collect();

        let (field, direction) = sort
            .map(|s| (s.field, s.direction))
            .unwrap_or((HistorySortField::CompletedAt, SortDirection::Descending));
        filtered.sort_by(|a, b| match field {
            HistorySortField::CompletedAt => a.completed_at.cmp(&b.completed_at),
            HistorySortField::FileName => a.file_name.cmp(&b.file_name),
            HistorySortField::TotalBytes => a.total_bytes.cmp(&b.total_bytes),
            HistorySortField::DurationSeconds => a.duration_seconds.cmp(&b.duration_seconds),
        });
        if matches!(direction, SortDirection::Descending) {
            filtered.reverse();
        }

        let start = offset.unwrap_or(0);
        let take = limit
            .unwrap_or(MAX_HISTORY_PAGE_SIZE)
            .min(MAX_HISTORY_PAGE_SIZE);
        Ok(filtered.into_iter().skip(start).take(take).collect())
    }

    fn search(&self, query: &str) -> Result<Vec<HistoryEntry>, DomainError> {
        let needle = query.to_lowercase();
        if needle.is_empty() {
            return Ok(Vec::new());
        }
        // Match the SQLite adapter: inspect the most recent
        // MAX_HISTORY_SEARCH_RESULTS rows, newest first, then filter.
        let mut recent: Vec<HistoryEntry> = self.entries.lock().unwrap().clone();
        recent.sort_by_key(|e| std::cmp::Reverse(e.completed_at));
        Ok(recent
            .into_iter()
            .take(MAX_HISTORY_SEARCH_RESULTS)
            .filter(|e| {
                e.file_name.to_lowercase().contains(&needle)
                    || e.url.to_lowercase().contains(&needle)
                    || e.destination_path.to_lowercase().contains(&needle)
            })
            .collect())
    }

    fn find_by_id(&self, id: u64) -> Result<Option<HistoryEntry>, DomainError> {
        Ok(self
            .entries
            .lock()
            .unwrap()
            .iter()
            .find(|e| e.id == id)
            .cloned())
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

// ── Stub adapters sufficient to construct a `CommandBus` for tests ──

struct StubDownloadRepo;
impl DownloadRepository for StubDownloadRepo {
    fn find_by_id(&self, _id: DownloadId) -> Result<Option<Download>, DomainError> {
        Ok(None)
    }
    fn save(&self, _download: &Download) -> Result<(), DomainError> {
        Ok(())
    }
    fn delete(&self, _id: DownloadId) -> Result<(), DomainError> {
        Ok(())
    }
    fn find_by_state(&self, _state: DownloadState) -> Result<Vec<Download>, DomainError> {
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

struct StubEventBus;
impl EventBus for StubEventBus {
    fn publish(&self, _event: DomainEvent) {}
    fn subscribe(&self, _handler: Box<dyn Fn(&DomainEvent) + Send + Sync>) {}
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
        Err(DomainError::NotFound("not mocked".to_string()))
    }
    fn get_media_variants(&self, _url: &str) -> Result<String, DomainError> {
        Err(DomainError::NotFound("not mocked".to_string()))
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

/// Build a [`CommandBus`] wired with the given history repository.
///
/// All other ports use minimal stubs — the returned bus is only
/// suitable for tests that exercise history-only commands.
pub(crate) fn make_history_command_bus(history: Arc<dyn HistoryRepository>) -> CommandBus {
    CommandBus::new(
        Arc::new(StubDownloadRepo),
        Arc::new(StubDownloadEngine),
        Arc::new(StubEventBus),
        Arc::new(StubFileStorage),
        Arc::new(StubHttpClient),
        Arc::new(StubPluginLoader),
        Arc::new(StubConfigStore),
        Arc::new(StubCredentialStore),
        Arc::new(StubClipboardObserver),
        Arc::new(StubArchiveExtractor),
        history,
        None,
    )
}

// ── Stub read-side adapters for `QueryBus` construction ──────────────

struct StubDownloadReadRepo;
impl DownloadReadRepository for StubDownloadReadRepo {
    fn find_downloads(
        &self,
        _: Option<DownloadFilter>,
        _: Option<SortOrder>,
        _: Option<usize>,
        _: Option<usize>,
    ) -> Result<Vec<DownloadView>, DomainError> {
        Ok(vec![])
    }
    fn find_download_detail(
        &self,
        _: DownloadId,
    ) -> Result<Option<DownloadDetailView>, DomainError> {
        Ok(None)
    }
    fn count_by_state(&self) -> Result<StateCountMap, DomainError> {
        Ok(HashMap::new())
    }
}

struct StubStatsRepo;
impl StatsRepository for StubStatsRepo {
    fn record_completed(&self, _: u64, _: u64) -> Result<(), DomainError> {
        Ok(())
    }
    fn get_stats(&self) -> Result<StatsView, DomainError> {
        Ok(StatsView {
            total_downloaded_bytes: 0,
            total_files: 0,
            avg_speed: 0,
            peak_speed: 0,
            success_rate: 0.0,
            daily_volumes: vec![],
            top_hosts: vec![],
        })
    }
}

struct StubPluginReadRepo;
impl PluginReadRepository for StubPluginReadRepo {
    fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
        Ok(vec![])
    }
}

struct StubQueryArchiveExtractor;
impl ArchiveExtractor for StubQueryArchiveExtractor {
    fn detect_format(&self, _: &Path) -> Result<Option<ArchiveFormat>, DomainError> {
        Ok(None)
    }
    fn can_extract(&self, _: &Path) -> Result<bool, DomainError> {
        Ok(false)
    }
    fn extract(&self, _: &Path, _: &Path, _: Option<&str>) -> Result<ExtractSummary, DomainError> {
        Ok(ExtractSummary {
            extracted_files: 0,
            extracted_bytes: 0,
            duration_ms: 0,
            warnings: vec![],
        })
    }
    fn list_contents(&self, _: &Path, _: Option<&str>) -> Result<Vec<ArchiveEntry>, DomainError> {
        Ok(vec![])
    }
    fn detect_segments(&self, _: &Path) -> Result<Option<Vec<std::path::PathBuf>>, DomainError> {
        Ok(None)
    }
}

/// Build a [`QueryBus`] wired with the given history repository.
///
/// Other read ports return empty/default data — suitable for tests that
/// only exercise history queries.
pub(crate) fn make_history_query_bus(history: Arc<dyn HistoryRepository>) -> QueryBus {
    QueryBus::new(
        Arc::new(StubDownloadReadRepo),
        history,
        Arc::new(StubStatsRepo),
        Arc::new(StubPluginReadRepo),
        Arc::new(StubQueryArchiveExtractor),
    )
}
