//! Duplicate-detection query (PRD §6.2.2).
//!
//! Compares URLs against the active downloads and the completed
//! history so the Link Grabber UI can surface "already in active /
//! already in history" badges before Start. Comparison runs on a
//! normalised form (see [`url_normalizer`](crate::application::services::url_normalizer))
//! so a URL that only differs from an existing entry in tracking
//! parameters still collapses onto it.

use std::collections::HashMap;

use crate::application::error::AppError;
use crate::application::query_bus::QueryBus;
use crate::application::services::history_paginate::for_each_history_page;
use crate::application::services::url_normalizer::normalize_url;
use crate::domain::ports::Query;

/// Mirrors the per-batch cap used by `resolve_links` and
/// `link_check_online` so the three Link Grabber pipeline steps share
/// the same upper bound.
pub const MAX_DETECT_DUPLICATES_URLS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplicateSource {
    Active,
    History,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateCheck {
    /// The URL as the caller submitted it (not normalised), so the UI
    /// can match the result back to the row that triggered the check.
    pub url: String,
    pub is_duplicate: bool,
    pub source: Option<DuplicateSource>,
    /// Existing-entry identifier as a string. Stays a string because
    /// active and history ids share the `u64` space and are
    /// distinguished only by `source` — the JSON shape stays uniform.
    pub existing_id: Option<String>,
    pub existing_filename: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DetectDuplicatesQuery {
    pub urls: Vec<String>,
}
impl Query for DetectDuplicatesQuery {}

impl QueryBus {
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

        // Active wins over history so a redownload still in the queue
        // surfaces as "active" rather than "history".
        let active_views = self
            .download_read_repo()
            .find_downloads(None, None, None, None)?;
        let mut by_normalized: HashMap<String, (DuplicateSource, u64, String)> =
            HashMap::with_capacity(active_views.len());
        for view in active_views {
            let key = normalize_url(&view.url);
            if key.is_empty() {
                continue;
            }
            by_normalized.entry(key).or_insert((
                DuplicateSource::Active,
                view.id.0,
                view.file_name,
            ));
        }

        for_each_history_page(self.history_repo(), |page| {
            for entry in page {
                let key = normalize_url(&entry.url);
                // `contains_key` skips the tuple allocation for entries
                // already shadowed by an active match.
                if key.is_empty() || by_normalized.contains_key(&key) {
                    continue;
                }
                by_normalized.insert(key, (DuplicateSource::History, entry.id, entry.file_name));
            }
        })?;

        let results = query
            .urls
            .into_iter()
            .map(|url| {
                let key = normalize_url(&url);
                let hit = if key.is_empty() {
                    None
                } else {
                    by_normalized.get(&key)
                };
                match hit {
                    Some((source, id, file_name)) => DuplicateCheck {
                        url,
                        is_duplicate: true,
                        source: Some(*source),
                        existing_id: Some(id.to_string()),
                        existing_filename: Some(file_name.clone()),
                    },
                    None => DuplicateCheck {
                        url,
                        is_duplicate: false,
                        source: None,
                        existing_id: None,
                        existing_filename: None,
                    },
                }
            })
            .collect();

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::application::test_support::{
        InMemoryDownloadReadRepo, InMemoryHistoryRepo, make_history_and_downloads_query_bus,
        make_history_query_bus,
    };
    use crate::domain::model::download::{DownloadId, DownloadState};
    use crate::domain::model::views::{DownloadView, HistoryEntry};
    use crate::domain::ports::driven::HistoryRepository;

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

    fn make_bus(
        active: Vec<DownloadView>,
        history: Arc<dyn HistoryRepository>,
    ) -> crate::application::query_bus::QueryBus {
        make_history_and_downloads_query_bus(
            Arc::new(InMemoryDownloadReadRepo::with_views(active)),
            history,
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
