//! Handler for [`CheckOnlineCommand`] — Link Grabber pipeline step 2.
//!
//! Probes each URL via the HTTP HEAD port and publishes a
//! [`DomainEvent::LinkStatusUpdated`] for every transition. The Tauri
//! bridge fans the events out to the frontend so the UI can colour each
//! row as soon as its probe resolves.
//!
//! Concurrency is bounded by [`AppConfig::link_check_parallelism`] and
//! each probe is capped by [`AppConfig::link_check_timeout_secs`] before
//! it falls back to [`LinkStatus::Unknown`]. The handler always emits a
//! `Checking` event up-front so the UI can render the spinner without
//! waiting for the first probe to land.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;
use crate::domain::model::config::normalize_link_check_parallelism;
use crate::domain::model::http::HttpResponse;
use crate::domain::model::link::LinkStatus;
use crate::domain::ports::driven::HttpClient;

use super::CheckOnlineCommand;

/// Cap on the size of a single check batch. Mirrors the bound used by
/// `resolve_links` so a paste of 10 000 URLs cannot trip the bus before
/// the user notices something is wrong.
const MAX_URLS: usize = 500;

impl CommandBus {
    /// Probe each URL and stream `LinkStatusUpdated` events.
    pub async fn handle_check_online(&self, cmd: CheckOnlineCommand) -> Result<(), AppError> {
        if cmd.urls.len() > MAX_URLS {
            return Err(AppError::Validation(format!(
                "Too many URLs: {} (max {})",
                cmd.urls.len(),
                MAX_URLS
            )));
        }

        let config = self.config_store().get_config().map_err(AppError::from)?;
        let parallelism = normalize_link_check_parallelism(config.link_check_parallelism);
        let timeout = Duration::from_secs(config.link_check_timeout_secs.max(1) as u64);
        let semaphore = Arc::new(Semaphore::new(parallelism));
        let http_client = self.http_client_arc();
        let event_bus = self.event_bus_arc();

        // Emit "Checking" once per URL before spawning so the UI gets a
        // synchronous spinner even when every probe queues behind the
        // semaphore. Empty / scheme-rejected entries jump straight to
        // `Unknown` so every input URL gets exactly one terminal event
        // and the row never sticks on "Checking".
        let mut probes: Vec<String> = Vec::with_capacity(cmd.urls.len());
        for url in cmd.urls {
            // Normalize once: a pasted entry like ` https://example.com/ `
            // would otherwise fail `is_probeable_scheme` (leading space)
            // or get sent to `head()` with trailing whitespace.
            let url = url.trim().to_string();
            if url.is_empty() || !is_probeable_scheme(&url) {
                event_bus.publish(DomainEvent::LinkStatusUpdated {
                    url,
                    status: LinkStatus::Unknown,
                });
                continue;
            }
            event_bus.publish(DomainEvent::LinkStatusUpdated {
                url: url.clone(),
                status: LinkStatus::Checking,
            });
            probes.push(url);
        }

        let mut handles = Vec::with_capacity(probes.len());
        for url in probes {
            let sem = semaphore.clone();
            let http = http_client.clone();
            let bus = event_bus.clone();
            handles.push(tokio::spawn(async move {
                let permit = sem.acquire_owned().await.expect("semaphore not closed");
                let status = run_probe(http, url.clone(), timeout, permit).await;
                bus.publish(DomainEvent::LinkStatusUpdated { url, status });
            }));
        }

        for handle in handles {
            // A panicking probe must not poison the whole batch. Log
            // and keep draining so partial batches still flush their
            // remaining `Checking` rows.
            if let Err(err) = handle.await {
                tracing::warn!(error = %err, "link_check_online probe task panicked");
            }
        }

        Ok(())
    }
}

async fn run_probe(
    http: Arc<dyn HttpClient>,
    url: String,
    timeout: Duration,
    permit: OwnedSemaphorePermit,
) -> LinkStatus {
    // The driven `HttpClient::head` is sync and may itself block on a
    // dedicated tokio runtime (`reqwest_client::block_on`). Driving it
    // through `spawn_blocking` keeps the parent runtime responsive even
    // if a probe takes the full timeout.
    //
    // `spawn_blocking` cannot be aborted: when `tokio::time::timeout`
    // elapses below the blocking thread keeps running until the sync
    // `head` call returns. Move `permit` into the closure so the
    // semaphore stays held for the lifetime of the blocking work; the
    // parent task can still report `Unknown` to the UI without freeing
    // capacity for a new probe to start in parallel.
    let url_for_log = url.clone();
    let join = tokio::task::spawn_blocking(move || {
        let result = http.head(&url);
        drop(permit);
        result
    });
    match tokio::time::timeout(timeout, join).await {
        Ok(Ok(Ok(response))) => classify_response(&response),
        Ok(Ok(Err(err))) => {
            tracing::debug!(url = %url_for_log, error = %err, "link probe failed");
            LinkStatus::Unknown
        }
        Ok(Err(join_err)) => {
            tracing::warn!(url = %url_for_log, error = %join_err, "link probe panicked");
            LinkStatus::Unknown
        }
        Err(_elapsed) => {
            tracing::debug!(url = %url_for_log, "link probe timed out");
            LinkStatus::Unknown
        }
    }
}

fn classify_response(response: &HttpResponse) -> LinkStatus {
    LinkStatus::from_status_code(response.status_code).unwrap_or_else(|| LinkStatus::Online {
        filename: extract_filename(response),
        size: response.content_length(),
        resumable: response.accept_ranges_bytes(),
    })
}

fn extract_filename(response: &HttpResponse) -> Option<String> {
    // `HttpResponse::header` does case-insensitive matching so a server
    // sending the canonical `Content-Disposition` casing still resolves.
    response
        .header("content-disposition")
        .and_then(parse_content_disposition_filename)
}

fn parse_content_disposition_filename(value: &str) -> Option<String> {
    // RFC 6266 simplified: split params on `;` first, then locate the
    // `filename=` parameter in isolation. Without the split, a header
    // like `attachment; filename="x.zip"; size=123` leaks the trailing
    // `size=...` into the returned name.
    const KEY: &str = "filename=";
    // `p.get(..KEY.len())` returns `None` when the requested byte index
    // does not fall on a UTF-8 boundary — keeps the parser panic-free
    // on non-ASCII headers like `attachment; nàme=...` that just happen
    // to be `KEY.len()` bytes long without the `filename=` prefix.
    let part = value.split(';').map(str::trim).find(|p| {
        p.get(..KEY.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(KEY))
    })?;
    let stripped = part[KEY.len()..].trim().trim_matches('"');
    if stripped.is_empty() {
        None
    } else {
        Some(stripped.to_string())
    }
}

fn is_probeable_scheme(url: &str) -> bool {
    let lower = url.to_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use crate::application::commands::tests_support::{
        StubArchiveExtractor, StubClipboardObserver, StubCredentialStore, StubDownloadEngine,
        StubDownloadRepo, StubFileStorage, StubPluginLoader,
    };
    use crate::application::test_support::NoopHistoryRepo;
    use crate::domain::error::DomainError;
    use crate::domain::model::config::{AppConfig, ConfigPatch};
    use crate::domain::model::http::HttpResponse;
    use crate::domain::ports::driven::{ConfigStore, EventBus, HttpClient};

    use super::*;

    /// Minimal `HttpClient` whose response is keyed by URL. Anything not
    /// mapped returns a 200 so the test author can describe only the
    /// "interesting" URLs.
    struct ScriptedHttp {
        responses: HashMap<String, HttpResponse>,
        delay: Option<Duration>,
        calls: Mutex<Vec<String>>,
    }

    impl ScriptedHttp {
        fn new(responses: HashMap<String, HttpResponse>, delay: Option<Duration>) -> Self {
            Self {
                responses,
                delay,
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl HttpClient for ScriptedHttp {
        fn head(&self, url: &str) -> Result<HttpResponse, DomainError> {
            self.calls.lock().unwrap().push(url.to_string());
            if let Some(d) = self.delay {
                std::thread::sleep(d);
            }
            Ok(self
                .responses
                .get(url)
                .cloned()
                .unwrap_or_else(|| HttpResponse {
                    status_code: 200,
                    headers: HashMap::new(),
                    body: vec![],
                }))
        }
        fn get_range(&self, _url: &str, _start: u64, _end: u64) -> Result<Vec<u8>, DomainError> {
            Ok(vec![])
        }
        fn supports_range(&self, _url: &str) -> Result<bool, DomainError> {
            Ok(false)
        }
    }

    struct CapturingBus {
        events: Mutex<Vec<DomainEvent>>,
    }

    impl CapturingBus {
        fn new() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
            }
        }

        fn snapshot(&self) -> Vec<DomainEvent> {
            self.events.lock().unwrap().clone()
        }
    }

    impl EventBus for CapturingBus {
        fn publish(&self, event: DomainEvent) {
            self.events.lock().unwrap().push(event);
        }
        fn subscribe(&self, _h: Box<dyn Fn(&DomainEvent) + Send + Sync>) {}
    }

    struct InMemoryConfig {
        config: Mutex<AppConfig>,
    }

    impl InMemoryConfig {
        fn new(config: AppConfig) -> Self {
            Self {
                config: Mutex::new(config),
            }
        }
    }

    impl ConfigStore for InMemoryConfig {
        fn get_config(&self) -> Result<AppConfig, DomainError> {
            Ok(self.config.lock().unwrap().clone())
        }
        fn update_config(&self, _patch: ConfigPatch) -> Result<AppConfig, DomainError> {
            Ok(self.config.lock().unwrap().clone())
        }
    }

    fn build_bus(
        http: Arc<dyn HttpClient>,
        bus: Arc<CapturingBus>,
        config: Arc<dyn ConfigStore>,
    ) -> CommandBus {
        CommandBus::new(
            Arc::new(StubDownloadRepo),
            Arc::new(StubDownloadEngine),
            bus,
            Arc::new(StubFileStorage),
            http,
            Arc::new(StubPluginLoader),
            config,
            Arc::new(StubCredentialStore),
            Arc::new(StubClipboardObserver),
            Arc::new(StubArchiveExtractor),
            Arc::new(NoopHistoryRepo),
            None,
        )
    }

    fn response(status: u16) -> HttpResponse {
        HttpResponse {
            status_code: status,
            headers: HashMap::new(),
            body: vec![],
        }
    }

    fn online_response(filename: &str, size: u64) -> HttpResponse {
        let mut headers = HashMap::new();
        headers.insert("content-length".to_string(), vec![size.to_string()]);
        headers.insert(
            "content-disposition".to_string(),
            vec![format!("attachment; filename=\"{filename}\"")],
        );
        headers.insert("accept-ranges".to_string(), vec!["bytes".to_string()]);
        HttpResponse {
            status_code: 200,
            headers,
            body: vec![],
        }
    }

    fn extract_status_for(events: &[DomainEvent], url: &str) -> Option<LinkStatus> {
        events.iter().rev().find_map(|e| match e {
            DomainEvent::LinkStatusUpdated { url: u, status } if u == url => Some(status.clone()),
            _ => None,
        })
    }

    #[tokio::test]
    async fn handle_check_online_emits_checking_then_terminal_for_each_url() {
        let mut responses = HashMap::new();
        responses.insert("https://a/".to_string(), online_response("file.zip", 1024));
        responses.insert("https://b/".to_string(), response(404));

        let http = Arc::new(ScriptedHttp::new(responses, None));
        let event_bus = Arc::new(CapturingBus::new());
        let config = Arc::new(InMemoryConfig::new(AppConfig::default()));
        let bus = build_bus(http.clone(), event_bus.clone(), config);

        bus.handle_check_online(CheckOnlineCommand {
            urls: vec!["https://a/".to_string(), "https://b/".to_string()],
        })
        .await
        .expect("ok");

        let events = event_bus.snapshot();
        // Two `Checking` then two terminal events — order between
        // terminals is non-deterministic so we assert by URL.
        let checking_count = events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    DomainEvent::LinkStatusUpdated {
                        status: LinkStatus::Checking,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(checking_count, 2);
        match extract_status_for(&events, "https://a/").unwrap() {
            LinkStatus::Online {
                filename,
                size,
                resumable,
            } => {
                assert_eq!(filename.as_deref(), Some("file.zip"));
                assert_eq!(size, Some(1024));
                assert!(resumable);
            }
            other => panic!("expected Online for a, got {other:?}"),
        }
        assert_eq!(
            extract_status_for(&events, "https://b/"),
            Some(LinkStatus::Offline)
        );
    }

    #[tokio::test]
    async fn handle_check_online_maps_unauthorized_to_premium_only() {
        let mut responses = HashMap::new();
        responses.insert("https://a/".to_string(), response(401));
        responses.insert("https://b/".to_string(), response(402));

        let http = Arc::new(ScriptedHttp::new(responses, None));
        let event_bus = Arc::new(CapturingBus::new());
        let config = Arc::new(InMemoryConfig::new(AppConfig::default()));
        let bus = build_bus(http, event_bus.clone(), config);

        bus.handle_check_online(CheckOnlineCommand {
            urls: vec!["https://a/".to_string(), "https://b/".to_string()],
        })
        .await
        .expect("ok");

        let events = event_bus.snapshot();
        assert_eq!(
            extract_status_for(&events, "https://a/"),
            Some(LinkStatus::PremiumOnly)
        );
        assert_eq!(
            extract_status_for(&events, "https://b/"),
            Some(LinkStatus::PremiumOnly)
        );
    }

    #[tokio::test]
    async fn handle_check_online_maps_other_status_codes_to_unknown() {
        let mut responses = HashMap::new();
        responses.insert("https://a/".to_string(), response(500));
        responses.insert("https://b/".to_string(), response(403));

        let http = Arc::new(ScriptedHttp::new(responses, None));
        let event_bus = Arc::new(CapturingBus::new());
        let config = Arc::new(InMemoryConfig::new(AppConfig::default()));
        let bus = build_bus(http, event_bus.clone(), config);

        bus.handle_check_online(CheckOnlineCommand {
            urls: vec!["https://a/".to_string(), "https://b/".to_string()],
        })
        .await
        .expect("ok");

        let events = event_bus.snapshot();
        assert_eq!(
            extract_status_for(&events, "https://a/"),
            Some(LinkStatus::Unknown)
        );
        assert_eq!(
            extract_status_for(&events, "https://b/"),
            Some(LinkStatus::Unknown)
        );
    }

    #[tokio::test]
    async fn handle_check_online_skips_non_http_schemes_with_unknown() {
        let http = Arc::new(ScriptedHttp::new(HashMap::new(), None));
        let event_bus = Arc::new(CapturingBus::new());
        let config = Arc::new(InMemoryConfig::new(AppConfig::default()));
        let bus = build_bus(http.clone(), event_bus.clone(), config);

        bus.handle_check_online(CheckOnlineCommand {
            urls: vec![
                "ftp://example.com/file".to_string(),
                "magnet:?xt=urn:btih:abc".to_string(),
            ],
        })
        .await
        .expect("ok");

        let events = event_bus.snapshot();
        // Both rejected without firing a HEAD probe.
        assert!(http.calls().is_empty());
        let ftp = extract_status_for(&events, "ftp://example.com/file").unwrap();
        let magnet = extract_status_for(&events, "magnet:?xt=urn:btih:abc").unwrap();
        assert_eq!(ftp, LinkStatus::Unknown);
        assert_eq!(magnet, LinkStatus::Unknown);
        // No `Checking` event for skipped URLs.
        let checking_count = events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    DomainEvent::LinkStatusUpdated {
                        status: LinkStatus::Checking,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(checking_count, 0);
    }

    #[tokio::test]
    async fn handle_check_online_returns_unknown_when_probe_times_out() {
        let mut responses = HashMap::new();
        responses.insert("https://slow/".to_string(), response(200));
        // 200ms head delay but timeout is 1s in test config (so sleep
        // longer than that).
        let http = Arc::new(ScriptedHttp::new(
            responses,
            Some(Duration::from_millis(1500)),
        ));
        let event_bus = Arc::new(CapturingBus::new());
        let cfg = AppConfig {
            link_check_timeout_secs: 1,
            ..AppConfig::default()
        };
        let config = Arc::new(InMemoryConfig::new(cfg));
        let bus = build_bus(http, event_bus.clone(), config);

        bus.handle_check_online(CheckOnlineCommand {
            urls: vec!["https://slow/".to_string()],
        })
        .await
        .expect("ok");

        let events = event_bus.snapshot();
        assert_eq!(
            extract_status_for(&events, "https://slow/"),
            Some(LinkStatus::Unknown)
        );
    }

    #[tokio::test]
    async fn handle_check_online_rejects_batches_above_max() {
        let http = Arc::new(ScriptedHttp::new(HashMap::new(), None));
        let event_bus = Arc::new(CapturingBus::new());
        let config = Arc::new(InMemoryConfig::new(AppConfig::default()));
        let bus = build_bus(http, event_bus.clone(), config);

        let urls = (0..(MAX_URLS + 1))
            .map(|i| format!("https://example.com/{i}"))
            .collect();
        let result = bus.handle_check_online(CheckOnlineCommand { urls }).await;

        assert!(matches!(result, Err(AppError::Validation(_))));
        // No event must have been published when validation rejects.
        assert!(event_bus.snapshot().is_empty());
    }

    #[tokio::test]
    async fn handle_check_online_bounded_parallelism_caps_concurrent_probes() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::Duration;

        struct ConcurrencyTracker {
            in_flight: AtomicUsize,
            peak: AtomicUsize,
        }

        impl ConcurrencyTracker {
            fn new() -> Arc<Self> {
                Arc::new(Self {
                    in_flight: AtomicUsize::new(0),
                    peak: AtomicUsize::new(0),
                })
            }

            fn enter(&self) {
                let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                let mut peak = self.peak.load(Ordering::SeqCst);
                while now > peak {
                    match self
                        .peak
                        .compare_exchange(peak, now, Ordering::SeqCst, Ordering::SeqCst)
                    {
                        Ok(_) => break,
                        Err(actual) => peak = actual,
                    }
                }
            }

            fn leave(&self) {
                self.in_flight.fetch_sub(1, Ordering::SeqCst);
            }
        }

        struct TrackingHttp {
            tracker: Arc<ConcurrencyTracker>,
        }

        impl HttpClient for TrackingHttp {
            fn head(&self, _url: &str) -> Result<HttpResponse, DomainError> {
                self.tracker.enter();
                std::thread::sleep(Duration::from_millis(80));
                self.tracker.leave();
                Ok(HttpResponse {
                    status_code: 200,
                    headers: HashMap::new(),
                    body: vec![],
                })
            }
            fn get_range(
                &self,
                _url: &str,
                _start: u64,
                _end: u64,
            ) -> Result<Vec<u8>, DomainError> {
                Ok(vec![])
            }
            fn supports_range(&self, _url: &str) -> Result<bool, DomainError> {
                Ok(false)
            }
        }

        let tracker = ConcurrencyTracker::new();
        let http = Arc::new(TrackingHttp {
            tracker: tracker.clone(),
        });
        let event_bus = Arc::new(CapturingBus::new());
        let cfg = AppConfig {
            link_check_parallelism: 3,
            ..AppConfig::default()
        };
        let config = Arc::new(InMemoryConfig::new(cfg));
        let bus = build_bus(http, event_bus, config);

        let urls: Vec<String> = (0..12)
            .map(|i| format!("https://example.com/{i}"))
            .collect();
        bus.handle_check_online(CheckOnlineCommand { urls })
            .await
            .expect("ok");

        let peak = tracker.peak.load(Ordering::SeqCst);
        assert!(
            peak <= 3,
            "peak concurrency {peak} exceeded configured cap of 3"
        );
        assert!(peak >= 1, "no probes ever ran");
    }

    /// Regression — RFC 6266 simplified parser must isolate the
    /// `filename=` parameter even when other params follow it. Without
    /// the per-param split the old impl returned `file.zip"; size=123`.
    #[test]
    fn parse_content_disposition_filename_isolates_when_more_params_follow() {
        let got = super::parse_content_disposition_filename(
            "attachment; filename=\"file.zip\"; size=123",
        );
        assert_eq!(got.as_deref(), Some("file.zip"));

        let got_unquoted =
            super::parse_content_disposition_filename("attachment; filename=plain.bin; size=42");
        assert_eq!(got_unquoted.as_deref(), Some("plain.bin"));

        let got_case =
            super::parse_content_disposition_filename("attachment; FileName=\"upper.txt\"");
        assert_eq!(got_case.as_deref(), Some("upper.txt"));

        assert_eq!(
            super::parse_content_disposition_filename("attachment; filename="),
            None
        );
    }

    /// Regression — non-ASCII characters in a parameter that happens to
    /// be the same byte length as `KEY` ("filename=") must not panic the
    /// parser. The old `p[..KEY.len()]` slice would split a multi-byte
    /// codepoint mid-char; `p.get(..KEY.len())` returns `None` instead.
    #[test]
    fn parse_content_disposition_filename_does_not_panic_on_non_ascii() {
        // `attachment; nàme=foo` — the 9-byte prefix `nàme=foo` straddles
        // the 2-byte `à`. Must return None, not panic.
        let got = super::parse_content_disposition_filename("attachment; nàme=foo");
        assert_eq!(got, None);

        // Real filename parameter still works alongside non-ASCII garbage.
        let got = super::parse_content_disposition_filename(
            "attachment; nàme=foo; filename=\"clean.txt\"",
        );
        assert_eq!(got.as_deref(), Some("clean.txt"));
    }

    /// Regression — `Accept-Ranges: bytes ` (trailing space) must still
    /// flag the response as resumable. Bug #pre-trim returned `false`.
    #[test]
    fn accept_ranges_bytes_trims_surrounding_whitespace() {
        let mut headers = HashMap::new();
        headers.insert("accept-ranges".to_string(), vec![" bytes ".to_string()]);
        let resp = HttpResponse {
            status_code: 200,
            headers,
            body: vec![],
        };
        assert!(resp.accept_ranges_bytes());
    }

    /// Regression — `extract_filename` must locate `Content-Disposition`
    /// regardless of header casing. Servers that send the canonical
    /// `Content-Disposition` would otherwise lose the filename.
    #[test]
    fn extract_filename_is_case_insensitive_on_header_name() {
        let mut headers = HashMap::new();
        headers.insert("content-length".to_string(), vec!["1024".to_string()]);
        // Note the canonical capital-letter casing.
        headers.insert(
            "Content-Disposition".to_string(),
            vec!["attachment; filename=\"upper.bin\"".to_string()],
        );
        let resp = HttpResponse {
            status_code: 200,
            headers,
            body: vec![],
        };
        assert_eq!(super::extract_filename(&resp).as_deref(), Some("upper.bin"));
    }

    /// Regression — every input URL must produce exactly one terminal
    /// event, including blanks. The handler emits `Unknown` for empty
    /// entries instead of dropping them silently so the IPC contract
    /// stays one-event-per-input.
    #[tokio::test]
    async fn handle_check_online_emits_unknown_for_blank_inputs() {
        let http = Arc::new(ScriptedHttp::new(HashMap::new(), None));
        let event_bus = Arc::new(CapturingBus::new());
        let config = Arc::new(InMemoryConfig::new(AppConfig::default()));
        let bus = build_bus(http.clone(), event_bus.clone(), config);

        bus.handle_check_online(CheckOnlineCommand {
            urls: vec!["".to_string(), "   ".to_string()],
        })
        .await
        .expect("ok");

        // No HEAD probe ever fires for blanks.
        assert!(http.calls().is_empty());
        let events = event_bus.snapshot();
        // Both blanks collapse to the empty string after trim and emit
        // a single `Unknown`. We assert by counting matching events.
        let unknown_count = events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    DomainEvent::LinkStatusUpdated {
                        url,
                        status: LinkStatus::Unknown,
                    } if url.is_empty()
                )
            })
            .count();
        assert_eq!(unknown_count, 2);
    }

    /// Regression — pasted URLs with surrounding whitespace must be
    /// trimmed before the scheme check + HEAD probe. Without this, the
    /// row was emitted as `Unknown` (scheme reject) and the trimmed
    /// value never reached `head()`.
    #[tokio::test]
    async fn handle_check_online_trims_pasted_whitespace() {
        let mut responses = HashMap::new();
        responses.insert("https://a/".to_string(), response(200));
        let http = Arc::new(ScriptedHttp::new(responses, None));
        let event_bus = Arc::new(CapturingBus::new());
        let config = Arc::new(InMemoryConfig::new(AppConfig::default()));
        let bus = build_bus(http.clone(), event_bus.clone(), config);

        bus.handle_check_online(CheckOnlineCommand {
            urls: vec!["  https://a/  ".to_string()],
        })
        .await
        .expect("ok");

        // The probe must have run on the trimmed URL, not on the
        // whitespace-padded original.
        assert_eq!(http.calls(), vec!["https://a/".to_string()]);
        let status = extract_status_for(&event_bus.snapshot(), "https://a/").unwrap();
        assert!(matches!(status, LinkStatus::Online { .. }));
    }
}
