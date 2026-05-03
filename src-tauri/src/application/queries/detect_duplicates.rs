//! Duplicate-detection query (PRD §6.2.2).
//!
//! Compares the URLs the user is about to add against the existing
//! download list and the completed history so the Link Grabber UI can
//! surface "already in active / already in history" badges before the
//! user clicks Start. Comparison is performed on a normalised form
//! produced by [`crate::application::services::url_normalizer`], so an
//! entry only differing in tracking parameters (`utm_*`, `fbclid`, …)
//! still collapses onto the original.
//!
//! The query never mutates state — duplicate detection is a read-only
//! pre-flight check, so it lives on the [`QueryBus`] alongside the other
//! read handlers.

use crate::application::error::AppError;
use crate::application::query_bus::QueryBus;
use crate::application::services::url_normalizer::normalize_url;
use crate::domain::ports::Query;
use crate::domain::ports::driven::history_repository::MAX_HISTORY_PAGE_SIZE;

/// Hard cap on the size of a single duplicate-detection batch.
///
/// Mirrors the ceiling used by `resolve_links` and `link_check_online`
/// so the three Link Grabber pipeline steps share the same upper bound.
pub const MAX_DETECT_DUPLICATES_URLS: usize = 500;

/// Where the duplicate was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplicateSource {
    /// Match against an entry currently held in the downloads table
    /// (any state — queued, running, paused, completed-but-not-yet-purged).
    Active,
    /// Match against an entry in the completed-downloads history table.
    History,
}

/// Result of one URL lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateCheck {
    /// The URL as the caller submitted it (not normalised), so the UI
    /// can match it back to the row that triggered the check.
    pub url: String,
    pub is_duplicate: bool,
    pub source: Option<DuplicateSource>,
    /// Identifier of the existing entry (download id when `source ==
    /// Active`, history primary key as a string when `source ==
    /// History`). `None` when `is_duplicate` is `false`.
    pub existing_id: Option<String>,
    /// File name of the existing entry — surfaced in the tooltip so the
    /// user can recognise the original at a glance.
    pub existing_filename: Option<String>,
}

/// Pre-flight duplicate check for one or more URLs.
#[derive(Debug, Clone)]
pub struct DetectDuplicatesQuery {
    pub urls: Vec<String>,
}
impl Query for DetectDuplicatesQuery {}

impl QueryBus {
    /// Compare each URL in the query against the active downloads and
    /// the history; return one [`DuplicateCheck`] per input.
    pub async fn handle_detect_duplicates(
        &self,
        query: DetectDuplicatesQuery,
    ) -> Result<Vec<DuplicateCheck>, AppError> {
        if query.urls.len() > MAX_DETECT_DUPLICATES_URLS {
            return Err(AppError::Validation(format!(
                "Too many URLs: {} (max {})",
                query.urls.len(),
                MAX_DETECT_DUPLICATES_URLS
            )));
        }

        // Build the active-side index first. Active downloads win over
        // a duplicate history entry so a redownload still in the queue
        // shows up as "in active" rather than "in history".
        let active_views = self
            .download_read_repo()
            .find_downloads(None, None, None, None)?;
        let mut by_normalized: std::collections::HashMap<
            String,
            (DuplicateSource, String, String),
        > = std::collections::HashMap::with_capacity(active_views.len());
        for view in active_views {
            let key = normalize_url(&view.url);
            if key.is_empty() {
                continue;
            }
            by_normalized.entry(key).or_insert((
                DuplicateSource::Active,
                view.id.0.to_string(),
                view.file_name,
            ));
        }

        // Page through history. The port caps each call at
        // `MAX_HISTORY_PAGE_SIZE`, so a callback-style loop is the only
        // way to inspect the full table — same pattern the export
        // command uses.
        let mut offset = 0usize;
        loop {
            let page =
                self.history_repo()
                    .list(None, None, Some(MAX_HISTORY_PAGE_SIZE), Some(offset))?;
            let len = page.len();
            for entry in page {
                let key = normalize_url(&entry.url);
                if key.is_empty() {
                    continue;
                }
                by_normalized.entry(key).or_insert((
                    DuplicateSource::History,
                    entry.id.to_string(),
                    entry.file_name,
                ));
            }
            if len < MAX_HISTORY_PAGE_SIZE {
                break;
            }
            offset += MAX_HISTORY_PAGE_SIZE;
        }

        // Look every input up against the index built above.
        let mut results = Vec::with_capacity(query.urls.len());
        for url in query.urls {
            let key = normalize_url(&url);
            let hit = if key.is_empty() {
                None
            } else {
                by_normalized.get(&key)
            };
            match hit {
                Some((source, id, file_name)) => results.push(DuplicateCheck {
                    url,
                    is_duplicate: true,
                    source: Some(*source),
                    existing_id: Some(id.clone()),
                    existing_filename: Some(file_name.clone()),
                }),
                None => results.push(DuplicateCheck {
                    url,
                    is_duplicate: false,
                    source: None,
                    existing_id: None,
                    existing_filename: None,
                }),
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::application::query_bus::QueryBus;
    use crate::application::test_support::{InMemoryHistoryRepo, make_history_query_bus};
    use crate::domain::error::DomainError;
    use crate::domain::model::archive::ArchiveFormat;
    use crate::domain::model::download::{DownloadId, DownloadState};
    use crate::domain::model::plugin::PluginInfo;
    use crate::domain::model::views::{
        DownloadDetailView, DownloadFilter, DownloadView, HistoryEntry, ModuleStats, SortOrder,
        StateCountMap, StatsPeriod, StatsView,
    };
    use crate::domain::ports::driven::{
        ArchiveExtractor, DownloadReadRepository, HistoryRepository, PluginReadRepository,
        StatsRepository,
    };

    struct InMemoryDownloadReadRepo {
        views: Mutex<Vec<DownloadView>>,
    }

    impl InMemoryDownloadReadRepo {
        fn with_views(views: Vec<DownloadView>) -> Self {
            Self {
                views: Mutex::new(views),
            }
        }
    }

    impl DownloadReadRepository for InMemoryDownloadReadRepo {
        fn find_downloads(
            &self,
            _filter: Option<DownloadFilter>,
            _sort: Option<SortOrder>,
            _limit: Option<usize>,
            _offset: Option<usize>,
        ) -> Result<Vec<DownloadView>, DomainError> {
            Ok(self.views.lock().unwrap().clone())
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

    struct StubStatsRepo;
    impl StatsRepository for StubStatsRepo {
        fn record_completed(&self, _: u64, _: u64) -> Result<(), DomainError> {
            Ok(())
        }
        fn get_stats(&self, _: StatsPeriod) -> Result<StatsView, DomainError> {
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
        fn top_modules(&self, _: u32) -> Result<Vec<ModuleStats>, DomainError> {
            Ok(vec![])
        }
    }

    struct StubPluginReadRepo;
    impl PluginReadRepository for StubPluginReadRepo {
        fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
            Ok(vec![])
        }
    }

    struct FakeArchiveExtractor;
    impl ArchiveExtractor for FakeArchiveExtractor {
        fn detect_format(&self, _: &std::path::Path) -> Result<Option<ArchiveFormat>, DomainError> {
            Ok(None)
        }
        fn can_extract(&self, _: &std::path::Path) -> Result<bool, DomainError> {
            Ok(false)
        }
        fn extract(
            &self,
            _: &std::path::Path,
            _: &std::path::Path,
            _: Option<&str>,
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
            _: &std::path::Path,
            _: Option<&str>,
        ) -> Result<Vec<crate::domain::model::archive::ArchiveEntry>, DomainError> {
            Ok(vec![])
        }
        fn detect_segments(
            &self,
            _: &std::path::Path,
        ) -> Result<Option<Vec<std::path::PathBuf>>, DomainError> {
            Ok(None)
        }
    }

    fn make_view(id: u64, file_name: &str, url: &str) -> DownloadView {
        DownloadView {
            id: DownloadId(id),
            file_name: file_name.to_string(),
            url: url.to_string(),
            source_hostname: "example.com".to_string(),
            state: DownloadState::Queued,
            progress_percent: 0.0,
            speed_bytes_per_sec: 0,
            downloaded_bytes: 0,
            total_bytes: None,
            eta_seconds: None,
            segments_active: 0,
            segments_total: 0,
            module_name: None,
            account_name: None,
            error_message: None,
            priority: 5,
            queue_position: 0,
            created_at: 0,
        }
    }

    fn make_history_entry(id: u64, file_name: &str, url: &str) -> HistoryEntry {
        HistoryEntry {
            id,
            download_id: DownloadId(id),
            file_name: file_name.to_string(),
            url: url.to_string(),
            total_bytes: 0,
            completed_at: id * 1_000,
            duration_seconds: 0,
            avg_speed: 0,
            destination_path: format!("/tmp/{file_name}"),
        }
    }

    fn make_bus(active: Vec<DownloadView>, history: Arc<dyn HistoryRepository>) -> QueryBus {
        QueryBus::new(
            Arc::new(InMemoryDownloadReadRepo::with_views(active)),
            history,
            Arc::new(StubStatsRepo),
            Arc::new(StubPluginReadRepo),
            Arc::new(FakeArchiveExtractor),
        )
    }

    #[tokio::test]
    async fn detect_duplicates_flags_url_matching_active_download() {
        let active = vec![make_view(1, "f.zip", "https://example.com/f.zip")];
        let history = Arc::new(InMemoryHistoryRepo::new());
        let bus = make_bus(active, history);

        let result = bus
            .handle_detect_duplicates(DetectDuplicatesQuery {
                urls: vec!["https://example.com/f.zip".to_string()],
            })
            .await
            .unwrap();

        assert_eq!(result.len(), 1);
        assert!(result[0].is_duplicate);
        assert_eq!(result[0].source, Some(DuplicateSource::Active));
        assert_eq!(result[0].existing_id.as_deref(), Some("1"));
        assert_eq!(result[0].existing_filename.as_deref(), Some("f.zip"));
    }

    #[tokio::test]
    async fn detect_duplicates_flags_url_matching_history_entry() {
        let active = Vec::new();
        let history = Arc::new(InMemoryHistoryRepo::new());
        history
            .record(&make_history_entry(
                7,
                "old.zip",
                "https://example.com/old.zip",
            ))
            .unwrap();
        let bus = make_bus(active, history);

        let result = bus
            .handle_detect_duplicates(DetectDuplicatesQuery {
                urls: vec!["https://example.com/old.zip".to_string()],
            })
            .await
            .unwrap();

        assert_eq!(result.len(), 1);
        assert!(result[0].is_duplicate);
        assert_eq!(result[0].source, Some(DuplicateSource::History));
        assert_eq!(result[0].existing_id.as_deref(), Some("1"));
        assert_eq!(result[0].existing_filename.as_deref(), Some("old.zip"));
    }

    #[tokio::test]
    async fn detect_duplicates_recognises_url_with_only_utm_difference_as_duplicate() {
        let active = vec![make_view(1, "post.html", "https://example.com/post?id=42")];
        let history = Arc::new(InMemoryHistoryRepo::new());
        let bus = make_bus(active, history);

        let result = bus
            .handle_detect_duplicates(DetectDuplicatesQuery {
                urls: vec!["https://example.com/post?id=42&utm_source=tw".to_string()],
            })
            .await
            .unwrap();

        assert!(result[0].is_duplicate);
        assert_eq!(result[0].source, Some(DuplicateSource::Active));
    }

    #[tokio::test]
    async fn detect_duplicates_returns_false_for_unrelated_url() {
        let active = vec![make_view(1, "f.zip", "https://example.com/f.zip")];
        let history = Arc::new(InMemoryHistoryRepo::new());
        let bus = make_bus(active, history);

        let result = bus
            .handle_detect_duplicates(DetectDuplicatesQuery {
                urls: vec!["https://other.com/g.zip".to_string()],
            })
            .await
            .unwrap();

        assert_eq!(result.len(), 1);
        assert!(!result[0].is_duplicate);
        assert!(result[0].source.is_none());
        assert!(result[0].existing_id.is_none());
    }

    #[tokio::test]
    async fn detect_duplicates_active_takes_precedence_over_history() {
        let active = vec![make_view(42, "active.zip", "https://example.com/x.zip")];
        let history = Arc::new(InMemoryHistoryRepo::new());
        history
            .record(&make_history_entry(
                99,
                "old.zip",
                "https://example.com/x.zip",
            ))
            .unwrap();
        let bus = make_bus(active, history);

        let result = bus
            .handle_detect_duplicates(DetectDuplicatesQuery {
                urls: vec!["https://example.com/x.zip".to_string()],
            })
            .await
            .unwrap();

        assert_eq!(result[0].source, Some(DuplicateSource::Active));
        assert_eq!(result[0].existing_id.as_deref(), Some("42"));
    }

    #[tokio::test]
    async fn detect_duplicates_returns_false_for_empty_url() {
        let active = vec![make_view(1, "f.zip", "https://example.com/f.zip")];
        let history = Arc::new(InMemoryHistoryRepo::new());
        let bus = make_bus(active, history);

        let result = bus
            .handle_detect_duplicates(DetectDuplicatesQuery {
                urls: vec![String::new()],
            })
            .await
            .unwrap();

        assert!(!result[0].is_duplicate);
    }

    #[tokio::test]
    async fn detect_duplicates_rejects_batches_above_max() {
        let history = Arc::new(InMemoryHistoryRepo::new());
        let bus = make_bus(Vec::new(), history);

        let urls = (0..(MAX_DETECT_DUPLICATES_URLS + 1))
            .map(|i| format!("https://example.com/{i}"))
            .collect();

        let err = bus
            .handle_detect_duplicates(DetectDuplicatesQuery { urls })
            .await
            .unwrap_err();

        assert!(matches!(err, AppError::Validation(_)));
    }

    #[tokio::test]
    async fn detect_duplicates_preserves_input_order() {
        let active = vec![make_view(1, "a.zip", "https://example.com/a.zip")];
        let history = Arc::new(InMemoryHistoryRepo::new());
        let bus = make_bus(active, history);

        let result = bus
            .handle_detect_duplicates(DetectDuplicatesQuery {
                urls: vec![
                    "https://example.com/x.zip".to_string(),
                    "https://example.com/a.zip".to_string(),
                    "https://example.com/y.zip".to_string(),
                ],
            })
            .await
            .unwrap();

        assert_eq!(result.len(), 3);
        assert_eq!(result[0].url, "https://example.com/x.zip");
        assert!(!result[0].is_duplicate);
        assert_eq!(result[1].url, "https://example.com/a.zip");
        assert!(result[1].is_duplicate);
        assert_eq!(result[2].url, "https://example.com/y.zip");
        assert!(!result[2].is_duplicate);
    }

    #[tokio::test]
    async fn detect_duplicates_with_no_active_or_history_returns_unique() {
        let bus = make_history_query_bus(Arc::new(InMemoryHistoryRepo::new()));
        let result = bus
            .handle_detect_duplicates(DetectDuplicatesQuery {
                urls: vec!["https://example.com/a".to_string()],
            })
            .await
            .unwrap();
        assert!(!result[0].is_duplicate);
    }
}
