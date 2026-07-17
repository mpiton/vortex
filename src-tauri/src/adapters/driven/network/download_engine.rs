use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::watch;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::domain::error::DomainError;
use crate::domain::event::DomainEvent;
use crate::domain::model::download::{Download, DownloadId};
use crate::domain::model::meta::{DownloadMeta, SegmentMeta};
use crate::domain::ports::driven::{
    DownloadEngine, DownloadSourceResolver, EventBus, FileStorage, ResolutionCancellation,
};

use super::segment_worker::{SegmentError, SegmentParams, download_segment};
use super::{format_error_chain, is_html_content_type, restricted_download_client};

struct ActiveDownload {
    cancel_token: CancellationToken,
    resolution_cancellation: ResolutionCancellation,
    pause_sender: watch::Sender<bool>,
}

struct RemoteMetadata {
    content_length: u64,
    accepts_ranges: bool,
    content_type: Option<String>,
    status: reqwest::StatusCode,
}

/// Minimum age and downloaded bytes a segment must have before it is
/// eligible for split. Without this gate a fresh split child (downloaded == 0,
/// elapsed ≈ 0) would compute as 0 B/s, become the guaranteed "slowest"
/// candidate, and be re-split immediately on the next completion event —
/// cascading fragmentation of the newest range without any real slow-tail
/// signal.
const MIN_SPLIT_SAMPLE_DURATION: std::time::Duration = std::time::Duration::from_millis(500);

/// Runtime state of one in-flight segment, tracked by the engine so it can
/// shrink the segment's range and observe its throughput for dynamic split.
struct SegmentRuntimeState {
    end_tx: watch::Sender<u64>,
    progress: Arc<AtomicU64>,
    started_at: std::time::Instant,
    start_byte: u64,
    initial_end: u64,
    /// Set by the coordinator when the worker for this slot returns `Ok(_)`.
    /// Completed slots stay in `active_segments` (instead of being cleared)
    /// so `persist_split_meta` records their byte range with `completed: true`
    /// — otherwise a crash right after a split would leave the resume meta
    /// without any record that those bytes are already on disk.
    completed: bool,
}

/// Pick the slowest active segment whose remaining range is large enough
/// to benefit from a split. Returns the slot index and the byte at which
/// to split (midpoint of the remaining range).
fn pick_split_target(
    segments: &[SegmentRuntimeState],
    min_remaining_bytes: u64,
) -> Option<(usize, u64)> {
    let mut slowest: Option<(usize, f64, u64)> = None;
    for (idx, state) in segments.iter().enumerate() {
        if state.completed {
            continue;
        }
        if state.initial_end == u64::MAX {
            continue; // unbounded segments cannot be split
        }
        let downloaded = state.progress.load(Ordering::Relaxed);
        if downloaded == 0 {
            continue; // no throughput sample yet
        }
        let elapsed = state.started_at.elapsed();
        if elapsed < MIN_SPLIT_SAMPLE_DURATION {
            continue; // worker hasn't run long enough to produce a meaningful bps
        }
        let current_offset = state.start_byte.saturating_add(downloaded);
        if current_offset >= state.initial_end {
            continue; // already at end — completion event will fire shortly
        }
        let remaining = state.initial_end - current_offset;
        if remaining < min_remaining_bytes.max(1) {
            continue;
        }
        let split_at = current_offset.saturating_add(remaining / 2);
        if split_at <= current_offset || split_at >= state.initial_end {
            continue;
        }
        let bps = downloaded as f64 / elapsed.as_secs_f64().max(1e-3);
        match slowest {
            None => slowest = Some((idx, bps, split_at)),
            Some((_, prev_bps, _)) if bps < prev_bps => {
                slowest = Some((idx, bps, split_at));
            }
            _ => {}
        }
    }
    slowest.map(|(idx, _, split_at)| (idx, split_at))
}

/// Atomically rewrite `.vortex-meta` after a dynamic split so resume after a
/// crash sees the updated segment topology. A failure here only logs — the
/// in-memory split is still valid for the live download.
async fn persist_split_meta(
    file_storage: &Arc<dyn FileStorage>,
    dest_path: &Path,
    download_id: DownloadId,
    url: &str,
    total_size: u64,
    active_segments: &[SegmentRuntimeState],
) {
    // Snapshot every slot — including completed ones — so a crash right
    // after a split does not lose the record of byte ranges already on
    // disk. Completed segments report their full range as downloaded so
    // resume does not re-fetch them.
    let segments_meta: Vec<SegmentMeta> = active_segments
        .iter()
        .enumerate()
        .map(|(i, st)| {
            let downloaded = if st.completed {
                st.initial_end.saturating_sub(st.start_byte)
            } else {
                st.progress.load(Ordering::Relaxed)
            };
            SegmentMeta {
                id: i as u32,
                start_byte: st.start_byte,
                end_byte: st.initial_end,
                downloaded_bytes: downloaded,
                completed: st.completed,
            }
        })
        .collect();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let file_name = dest_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    let snapshot = DownloadMeta {
        download_id,
        url: url.to_string(),
        file_name,
        total_bytes: Some(total_size),
        segments: segments_meta,
        checksum_expected: None,
        created_at: now,
        updated_at: now,
    };
    let storage = file_storage.clone();
    let path = dest_path.to_path_buf();
    let join = tokio::task::spawn_blocking(move || storage.write_meta(&path, &snapshot)).await;
    match join {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::warn!(
            download_id = download_id.0,
            error = %e,
            "persist meta after split failed (download still proceeds)"
        ),
        Err(e) => tracing::warn!(
            download_id = download_id.0,
            error = %e,
            "persist meta after split task panicked"
        ),
    }
}

pub struct SegmentedDownloadEngine {
    client: reqwest::Client,
    file_storage: Arc<dyn FileStorage>,
    event_bus: Arc<dyn EventBus>,
    default_segments: u32,
    min_segment_bytes: u64,
    dynamic_split_enabled: Arc<AtomicBool>,
    dynamic_split_min_remaining_bytes: Arc<AtomicU64>,
    active_downloads: Arc<Mutex<HashMap<DownloadId, ActiveDownload>>>,
    source_resolver: Option<Arc<dyn DownloadSourceResolver>>,
}

impl SegmentedDownloadEngine {
    pub fn new(
        client: reqwest::Client,
        file_storage: Arc<dyn FileStorage>,
        event_bus: Arc<dyn EventBus>,
        default_segments: u32,
    ) -> Self {
        Self {
            client,
            file_storage,
            event_bus,
            default_segments: default_segments.max(1),
            min_segment_bytes: 64 * 1024,
            dynamic_split_enabled: Arc::new(AtomicBool::new(true)),
            dynamic_split_min_remaining_bytes: Arc::new(AtomicU64::new(4 * 1024 * 1024)),
            active_downloads: Arc::new(Mutex::new(HashMap::new())),
            source_resolver: None,
        }
    }

    pub fn with_min_segment_bytes(mut self, min_bytes: u64) -> Self {
        self.min_segment_bytes = min_bytes.max(1);
        self
    }

    pub fn with_source_resolver(mut self, resolver: Arc<dyn DownloadSourceResolver>) -> Self {
        self.source_resolver = Some(resolver);
        self
    }

    /// Configure runtime re-splitting of slow segments. PRD §7.1.
    /// `min_remaining_mb == 0` disables the size gate entirely; the engine
    /// then only refuses to split if the candidate has 0 bytes left.
    pub fn with_dynamic_split(self, enabled: bool, min_remaining_mb: u64) -> Self {
        self.set_dynamic_split(enabled, min_remaining_mb);
        self
    }

    /// Update dynamic-split runtime parameters live. Used by the engine
    /// config bridge so settings changes from the UI take effect on
    /// already-running and newly-started downloads without restart.
    pub fn set_dynamic_split(&self, enabled: bool, min_remaining_mb: u64) {
        self.dynamic_split_enabled.store(enabled, Ordering::Relaxed);
        self.dynamic_split_min_remaining_bytes.store(
            min_remaining_mb.saturating_mul(1024 * 1024),
            Ordering::Relaxed,
        );
    }

    /// Read back the current dynamic-split parameters as `(enabled, min_remaining_bytes)`.
    /// Lets the bridge tests prove that a `SettingsUpdated` event actually
    /// reaches the engine; also useful for diagnostics on a running download.
    pub fn dynamic_split_state(&self) -> (bool, u64) {
        (
            self.dynamic_split_enabled.load(Ordering::Relaxed),
            self.dynamic_split_min_remaining_bytes
                .load(Ordering::Relaxed),
        )
    }

    async fn probe_remote_metadata(
        client: &reqwest::Client,
        url: &str,
    ) -> Result<RemoteMetadata, reqwest::Error> {
        let response = match client.head(url).send().await {
            Ok(response) if response.status().is_success() => response,
            Ok(response) => {
                tracing::warn!(
                    status = %response.status(),
                    "HEAD probe returned non-success status, falling back to GET metadata probe"
                );
                client.get(url).send().await?
            }
            Err(_) => {
                tracing::warn!("HEAD probe failed, falling back to GET metadata probe");
                client.get(url).send().await?
            }
        };

        let content_length = response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let accepts_ranges = response
            .headers()
            .get("accept-ranges")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.eq_ignore_ascii_case("bytes"))
            .unwrap_or(false);
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let status = response.status();

        Ok(RemoteMetadata {
            content_length,
            accepts_ranges,
            content_type,
            status,
        })
    }
}

/// Outcome of a single mirror attempt inside the failover loop.
///
/// The outer engine task converts this into a `DomainEvent`:
/// - `Completed` → `DownloadCompleted`
/// - `Cancelled` → `DownloadCancelled`
/// - `Failed(_)` → either `MirrorSwitched` + retry the next mirror, or
///   `DownloadFailed` if no mirror remains.
enum AttemptOutcome {
    Completed,
    Cancelled,
    Failed(String),
}

struct PreparedSources {
    urls: Vec<String>,
    initial_index: usize,
    client: reqwest::Client,
    resume_url: String,
    sensitive: bool,
}

async fn prepare_sources(
    download: Download,
    resolver: Option<Arc<dyn DownloadSourceResolver>>,
    client: reqwest::Client,
    cancel_token: CancellationToken,
    resolution_cancellation: ResolutionCancellation,
) -> Result<PreparedSources, DomainError> {
    if cancel_token.is_cancelled() {
        return Err(DomainError::PluginError(
            "download source resolution cancelled".into(),
        ));
    }
    let resume_url = download.url().as_str().to_string();
    let requires_resolution = match resolver.as_ref() {
        Some(resolver) => resolver.requires_resolution(&download)?,
        None => download.account_id().is_some(),
    };
    if requires_resolution {
        let resolver = resolver.ok_or_else(|| {
            DomainError::PluginError("download source resolver is not configured".into())
        })?;
        let mut resolve_task = tokio::task::spawn_blocking(move || {
            resolver.resolve_cancellable(&download, &resolution_cancellation)
        });
        let source = tokio::select! {
            biased;
            _ = cancel_token.cancelled() => {
                return Err(DomainError::PluginError("download source resolution cancelled".into()));
            }
            result = &mut resolve_task => {
                result
                    .map_err(|_| DomainError::PluginError("download source resolver stopped".into()))??
            }
        };
        let request_url = source.request_url().to_string();
        let request_headers = source.request_headers().to_vec();
        let mut safety_task = tokio::task::spawn_blocking(move || {
            let parsed = reqwest::Url::parse(&request_url)
                .map_err(|_| DomainError::NetworkError("plugin returned an invalid URL".into()))?;
            let client = restricted_download_client(&parsed, &request_headers)?;
            Ok::<_, DomainError>((request_url, client))
        });
        let (request_url, client) = tokio::select! {
            biased;
            _ = cancel_token.cancelled() => {
                return Err(DomainError::PluginError("download source resolution cancelled".into()));
            }
            result = &mut safety_task => {
                result
                    .map_err(|_| DomainError::NetworkError("URL safety check stopped".into()))??
            }
        };
        return Ok(PreparedSources {
            urls: vec![request_url],
            initial_index: 0,
            client,
            resume_url,
            sensitive: true,
        });
    }
    let urls = if download.mirrors().is_empty() {
        vec![resume_url.clone()]
    } else {
        download
            .mirrors()
            .iter()
            .map(|mirror| mirror.url().as_str().to_string())
            .collect::<Vec<_>>()
    };
    let initial_index =
        (download.current_mirror_index() as usize).min(urls.len().saturating_sub(1));
    Ok(PreparedSources {
        urls,
        initial_index,
        client,
        resume_url,
        sensitive: false,
    })
}

fn source_failure_message(error: &DomainError) -> String {
    match error {
        DomainError::AccountInvalidCredentials => "Account credentials were rejected",
        DomainError::AccountExpired => "Account is expired",
        DomainError::AccountCooldown => "Account is temporarily rate-limited",
        DomainError::AccountQuotaExceeded => "Account quota is exhausted",
        DomainError::HosterNoFile => "No downloadable file was found",
        DomainError::HosterAuthenticationRequired => "Hoster authentication is required",
        DomainError::HosterDirectUrlExpired => "The direct download URL has expired",
        DomainError::HosterUnexpectedHtml => "Hoster returned an HTML page instead of file content",
        _ => "Download source could not be resolved",
    }
    .to_string()
}

impl DownloadEngine for SegmentedDownloadEngine {
    fn start(&self, download: &Download) -> Result<(), DomainError> {
        let download_id = download.id();
        // destination_path already contains the complete file path (dir + filename).
        // Do NOT join file_name again — that would produce "dir/file.bin/file.bin".
        let dest_path = PathBuf::from(download.destination_path());
        let segments_count = if download.segments_count() == 0 {
            self.default_segments
        } else {
            download.segments_count()
        };

        let cancel_token = CancellationToken::new();
        let resolution_cancellation = ResolutionCancellation::default();
        let (pause_tx, pause_rx) = watch::channel(false);

        {
            let mut map = self
                .active_downloads
                .lock()
                .expect("active_downloads lock poisoned");
            if map.contains_key(&download_id) {
                return Err(DomainError::AlreadyExists(format!(
                    "download {}",
                    download_id.0
                )));
            }
            map.insert(
                download_id,
                ActiveDownload {
                    cancel_token: cancel_token.clone(),
                    resolution_cancellation: resolution_cancellation.clone(),
                    pause_sender: pause_tx,
                },
            );
        }

        let client = self.client.clone();
        let file_storage = self.file_storage.clone();
        let event_bus = self.event_bus.clone();
        let active_downloads = self.active_downloads.clone();
        let min_segment_bytes = self.min_segment_bytes;
        let dynamic_split_enabled = self.dynamic_split_enabled.clone();
        let dynamic_split_min_remaining_bytes = self.dynamic_split_min_remaining_bytes.clone();
        let source_resolver = self.source_resolver.clone();
        let download = download.clone();

        tokio::spawn(async move {
            let prepared = match prepare_sources(
                download,
                source_resolver,
                client,
                cancel_token.clone(),
                resolution_cancellation,
            )
            .await
            {
                Ok(prepared) => prepared,
                Err(error) => {
                    let event = if cancel_token.is_cancelled() {
                        DomainEvent::DownloadCancelled { id: download_id }
                    } else {
                        DomainEvent::DownloadFailed {
                            id: download_id,
                            error: source_failure_message(&error),
                        }
                    };
                    event_bus.publish(event);
                    active_downloads
                        .lock()
                        .expect("active_downloads lock poisoned")
                        .remove(&download_id);
                    return;
                }
            };
            let client = prepared.client;
            let mirror_urls = prepared.urls;
            let resume_url = prepared.resume_url;
            let sensitive_url = prepared.sensitive;
            if cancel_token.is_cancelled() {
                event_bus.publish(DomainEvent::DownloadCancelled { id: download_id });
                active_downloads
                    .lock()
                    .expect("active_downloads lock poisoned")
                    .remove(&download_id);
                return;
            }
            let mut mirror_idx = prepared.initial_index;
            loop {
                let url = mirror_urls[mirror_idx].clone();
                // Each attempt gets a fresh attempt-scoped child token so
                // tearing down peer segments after a failure does not also
                // mark a user-cancel. The user-cancel signal lives on
                // `cancel_token`; the attempt token cascades from it via
                // `child_token()` so a real cancel still aborts segments.
                let attempt_token = cancel_token.child_token();
                let outcome = run_mirror_attempt(MirrorAttemptParams {
                    url,
                    download_id,
                    segments_count,
                    client: client.clone(),
                    file_storage: file_storage.clone(),
                    event_bus: event_bus.clone(),
                    dest_path: dest_path.clone(),
                    pause_rx: pause_rx.clone(),
                    user_cancel_token: cancel_token.clone(),
                    attempt_token,
                    min_segment_bytes,
                    dynamic_split_enabled: dynamic_split_enabled.clone(),
                    dynamic_split_min_remaining_bytes: dynamic_split_min_remaining_bytes.clone(),
                    resume_url: resume_url.clone(),
                    sensitive_url,
                })
                .await;

                match outcome {
                    AttemptOutcome::Completed => {
                        event_bus.publish(DomainEvent::DownloadCompleted { id: download_id });
                        break;
                    }
                    AttemptOutcome::Cancelled => {
                        event_bus.publish(DomainEvent::DownloadCancelled { id: download_id });
                        break;
                    }
                    AttemptOutcome::Failed(err) => {
                        let next = mirror_idx + 1;
                        if next < mirror_urls.len() {
                            // A user-cancel that landed after the attempt
                            // returned `Failed` (or while the cleanup runs
                            // below) must not be silently upgraded into a
                            // mirror switch — `MirrorSwitched` persists the
                            // cursor through the bridge, so a cancel in this
                            // window would leave a future retry resuming
                            // from a slot the user never asked for.
                            if cancel_token.is_cancelled() {
                                event_bus
                                    .publish(DomainEvent::DownloadCancelled { id: download_id });
                                break;
                            }
                            mirror_idx = next;
                            if sensitive_url {
                                tracing::info!(
                                    download_id = download_id.0,
                                    new_mirror_index = mirror_idx,
                                    "switching sensitive source after failure"
                                );
                            } else {
                                tracing::info!(
                                    download_id = download_id.0,
                                    new_mirror_index = mirror_idx,
                                    new_url = %mirror_urls[mirror_idx],
                                    previous_error = %err,
                                    "switching to next mirror after failure"
                                );
                            }
                            // Wipe the previous mirror's partial file + meta so
                            // the next attempt starts clean. The pre-allocation
                            // step uses `create_new(true)` and would otherwise
                            // collide with the existing file when meta is
                            // present, sinking the retry before it ever opens
                            // a connection. Bytes from mirror N are not safe to
                            // splice with mirror N+1 anyway (the servers may
                            // serve subtly different payloads).
                            let storage = file_storage.clone();
                            let path = dest_path.clone();
                            let _ = tokio::task::spawn_blocking(move || {
                                if let Err(e) = storage.delete_meta(&path) {
                                    tracing::debug!(
                                        path = %path.display(),
                                        error = %e,
                                        "failed to delete stale meta before mirror retry"
                                    );
                                }
                                if path.exists()
                                    && let Err(e) = std::fs::remove_file(&path)
                                {
                                    tracing::warn!(
                                        path = %path.display(),
                                        error = %e,
                                        "failed to remove stale file before mirror retry"
                                    );
                                }
                            })
                            .await;
                            // Re-check after cleanup since the user may have
                            // hit cancel while it was running.
                            if cancel_token.is_cancelled() {
                                event_bus
                                    .publish(DomainEvent::DownloadCancelled { id: download_id });
                                break;
                            }
                            event_bus.publish(DomainEvent::MirrorSwitched {
                                id: download_id,
                                new_mirror_index: mirror_idx as u32,
                                new_url: if sensitive_url {
                                    resume_url.clone()
                                } else {
                                    mirror_urls[mirror_idx].clone()
                                },
                            });
                            continue;
                        }
                        // Publish exhaustion before the generic terminal
                        // event so the persistence bridge can distinguish
                        // mirror-driven failure from post-download failures
                        // (extract / verify / domain `fail()`) and reset the
                        // cursor only on this signal.
                        event_bus.publish(DomainEvent::AllMirrorsExhausted { id: download_id });
                        event_bus.publish(DomainEvent::DownloadFailed {
                            id: download_id,
                            error: err,
                        });
                        break;
                    }
                }
            }

            active_downloads
                .lock()
                .expect("active_downloads lock poisoned")
                .remove(&download_id);
        });

        Ok(())
    }

    fn pause(&self, id: DownloadId) -> Result<(), DomainError> {
        {
            let map = self
                .active_downloads
                .lock()
                .expect("active_downloads lock poisoned");
            let active = map
                .get(&id)
                .ok_or_else(|| DomainError::NotFound(format!("download {}", id.0)))?;
            let _ = active.pause_sender.send(true);
        }
        // Guard dropped — safe to publish without deadlock risk
        self.event_bus.publish(DomainEvent::DownloadPaused { id });
        Ok(())
    }

    fn resume(&self, id: DownloadId) -> Result<(), DomainError> {
        {
            let map = self
                .active_downloads
                .lock()
                .expect("active_downloads lock poisoned");
            let active = map
                .get(&id)
                .ok_or_else(|| DomainError::NotFound(format!("download {}", id.0)))?;
            let _ = active.pause_sender.send(false);
        }
        // Guard dropped — safe to publish without deadlock risk
        self.event_bus.publish(DomainEvent::DownloadResumed { id });
        Ok(())
    }

    fn cancel(&self, id: DownloadId) -> Result<(), DomainError> {
        // Don't remove from map — the spawned task removes itself on exit.
        // This prevents a new start() for the same ID from racing with
        // the old task that is still shutting down.
        let map = self
            .active_downloads
            .lock()
            .expect("active_downloads lock poisoned");
        let active = map
            .get(&id)
            .ok_or_else(|| DomainError::NotFound(format!("download {}", id.0)))?;
        active.resolution_cancellation.cancel();
        active.cancel_token.cancel();
        Ok(())
    }
}

/// Run one mirror attempt: probe metadata, plan segments, dispatch
/// workers, await completion. Returns the outcome so the caller can
/// either fall back to the next mirror or finalise the download.
struct MirrorAttemptParams {
    url: String,
    download_id: DownloadId,
    segments_count: u32,
    client: reqwest::Client,
    file_storage: Arc<dyn FileStorage>,
    event_bus: Arc<dyn EventBus>,
    dest_path: PathBuf,
    pause_rx: watch::Receiver<bool>,
    user_cancel_token: CancellationToken,
    attempt_token: CancellationToken,
    min_segment_bytes: u64,
    dynamic_split_enabled: Arc<AtomicBool>,
    dynamic_split_min_remaining_bytes: Arc<AtomicU64>,
    resume_url: String,
    sensitive_url: bool,
}

async fn run_mirror_attempt(params: MirrorAttemptParams) -> AttemptOutcome {
    let MirrorAttemptParams {
        url,
        download_id,
        segments_count,
        client,
        file_storage,
        event_bus,
        dest_path,
        pause_rx,
        user_cancel_token,
        attempt_token,
        min_segment_bytes,
        dynamic_split_enabled,
        dynamic_split_min_remaining_bytes,
        resume_url,
        sensitive_url,
    } = params;
    let metadata = match SegmentedDownloadEngine::probe_remote_metadata(&client, &url).await {
        Ok(metadata) => metadata,
        Err(e) => {
            if sensitive_url {
                tracing::warn!(download_id = download_id.0, "metadata probe failed");
            } else {
                tracing::warn!(
                    download_id = download_id.0,
                    url = %url,
                    error = %format_error_chain(&e),
                    "metadata probe failed (mirror attempt)"
                );
            }
            if user_cancel_token.is_cancelled() {
                return AttemptOutcome::Cancelled;
            }
            return AttemptOutcome::Failed(if sensitive_url {
                "metadata probe failed".into()
            } else {
                format!("metadata probe failed: {}", format_error_chain(&e))
            });
        }
    };

    if sensitive_url {
        let source_error = match metadata.status {
            reqwest::StatusCode::UNAUTHORIZED => Some(DomainError::HosterAuthenticationRequired),
            reqwest::StatusCode::FORBIDDEN | reqwest::StatusCode::GONE => {
                Some(DomainError::HosterDirectUrlExpired)
            }
            status if !status.is_success() => Some(DomainError::NetworkError(
                "hoster direct URL returned an unsuccessful status".into(),
            )),
            _ if metadata
                .content_type
                .as_deref()
                .is_some_and(is_html_content_type) =>
            {
                Some(DomainError::HosterUnexpectedHtml)
            }
            _ => None,
        };
        if let Some(error) = source_error {
            return AttemptOutcome::Failed(source_failure_message(&error));
        }
    }
    let total_size = metadata.content_length;
    let supports_range = metadata.accepts_ranges;

    if user_cancel_token.is_cancelled() {
        return AttemptOutcome::Cancelled;
    }

    if total_size > 0 {
        let storage = file_storage.clone();
        let path = dest_path.clone();
        match tokio::task::spawn_blocking(move || {
            if path.exists() {
                let has_meta = storage.read_meta(&path).ok().flatten().is_some();
                if !has_meta {
                    if let Err(e) = std::fs::remove_file(&path) {
                        tracing::warn!(
                            path = %path.display(),
                            error = %e,
                            "failed to remove stale download file; create_file may fail"
                        );
                    } else {
                        tracing::debug!(
                            path = %path.display(),
                            "removed orphaned download file before re-creating"
                        );
                    }
                }
            }
            storage.create_file(&path, total_size)
        })
        .await
        {
            Err(e) => {
                tracing::error!(
                    download_id = download_id.0,
                    error = %e,
                    "spawn_blocking for create_file panicked"
                );
                return AttemptOutcome::Failed(format!("file pre-allocation failed: {e}"));
            }
            Ok(Err(e)) => {
                return AttemptOutcome::Failed(format!("file pre-allocation failed: {e}"));
            }
            Ok(Ok(())) => {}
        }
    }

    let num_segments = if supports_range && total_size > 0 {
        segments_count
            .min((total_size / min_segment_bytes).max(1) as u32)
            .max(1)
    } else {
        1
    };

    let segments: Vec<(u64, u64)> = if supports_range && total_size > 0 && num_segments > 1 {
        let segment_size = total_size / num_segments as u64;
        (0..num_segments)
            .map(|i| {
                let start = i as u64 * segment_size;
                let end = if i == num_segments - 1 {
                    total_size
                } else {
                    (i as u64 + 1) * segment_size
                };
                (start, end)
            })
            .collect()
    } else if supports_range && total_size > 0 {
        vec![(0, total_size)]
    } else {
        vec![(0, u64::MAX)]
    };

    if user_cancel_token.is_cancelled() {
        return AttemptOutcome::Cancelled;
    }

    event_bus.publish(DomainEvent::DownloadStarted { id: download_id });

    let shared_downloaded = Arc::new(AtomicU64::new(0));
    let mut join_set: JoinSet<(usize, Result<u64, SegmentError>)> = JoinSet::new();
    let mut active_segments: Vec<SegmentRuntimeState> = Vec::with_capacity(segments.len());
    for (index, (start, end)) in segments.iter().enumerate() {
        let (end_tx, end_rx) = watch::channel(*end);
        let progress = Arc::new(AtomicU64::new(0));
        active_segments.push(SegmentRuntimeState {
            end_tx,
            progress: progress.clone(),
            started_at: std::time::Instant::now(),
            start_byte: *start,
            initial_end: *end,
            completed: false,
        });
        let params = SegmentParams {
            client: client.clone(),
            file_storage: file_storage.clone(),
            event_bus: event_bus.clone(),
            download_id,
            segment_index: index as u32,
            url: url.clone(),
            start_byte: *start,
            end_byte_rx: end_rx,
            already_downloaded: 0,
            total_file_size: total_size,
            dest_path: dest_path.clone(),
            pause_rx: pause_rx.clone(),
            cancel_token: attempt_token.clone(),
            shared_downloaded: shared_downloaded.clone(),
            segment_progress: progress,
            sensitive_url,
        };
        let slot_idx = index;
        join_set.spawn(async move { (slot_idx, download_segment(params).await) });
    }

    let mut failed = false;
    let mut error_msg = String::new();
    let mut next_segment_id: u32 = segments.len() as u32;

    while let Some(result) = join_set.join_next().await {
        match result {
            Ok((slot_idx, Ok(_bytes))) => {
                if slot_idx < active_segments.len() {
                    active_segments[slot_idx].completed = true;
                }

                if dynamic_split_enabled.load(Ordering::Relaxed)
                    && !attempt_token.is_cancelled()
                    && let Some((idx, split_at)) = pick_split_target(
                        &active_segments,
                        dynamic_split_min_remaining_bytes.load(Ordering::Relaxed),
                    )
                {
                    let new_id = next_segment_id;
                    next_segment_id += 1;
                    let initial_end = active_segments[idx].initial_end;
                    let signal_sent = active_segments[idx].end_tx.send(split_at).is_ok();
                    if signal_sent {
                        active_segments[idx].initial_end = split_at;
                    } else {
                        tracing::warn!(
                            download_id = download_id.0,
                            original_segment_id = idx as u32,
                            "split skipped: target worker no longer listening"
                        );
                        continue;
                    }
                    event_bus.publish(DomainEvent::SegmentSplit {
                        download_id,
                        original_segment_id: idx as u32,
                        new_segment_id: new_id,
                        split_at,
                    });

                    let new_progress = Arc::new(AtomicU64::new(0));
                    let (new_end_tx, new_end_rx) = watch::channel(initial_end);
                    let new_slot_idx = active_segments.len();
                    let params = SegmentParams {
                        client: client.clone(),
                        file_storage: file_storage.clone(),
                        event_bus: event_bus.clone(),
                        download_id,
                        segment_index: new_id,
                        url: url.clone(),
                        start_byte: split_at,
                        end_byte_rx: new_end_rx,
                        already_downloaded: 0,
                        total_file_size: total_size,
                        dest_path: dest_path.clone(),
                        pause_rx: pause_rx.clone(),
                        cancel_token: attempt_token.clone(),
                        shared_downloaded: shared_downloaded.clone(),
                        segment_progress: new_progress.clone(),
                        sensitive_url,
                    };
                    join_set.spawn(async move { (new_slot_idx, download_segment(params).await) });
                    active_segments.push(SegmentRuntimeState {
                        end_tx: new_end_tx,
                        progress: new_progress,
                        started_at: std::time::Instant::now(),
                        start_byte: split_at,
                        initial_end,
                        completed: false,
                    });

                    persist_split_meta(
                        &file_storage,
                        &dest_path,
                        download_id,
                        &resume_url,
                        total_size,
                        &active_segments,
                    )
                    .await;
                }
            }
            Ok((_slot_idx, Err(e))) => {
                match e {
                    SegmentError::Cancelled => {
                        attempt_token.cancel();
                    }
                    _ => {
                        if failed {
                            tracing::warn!(
                                download_id = download_id.0,
                                previous_error = %error_msg,
                                "additional segment failure (overwriting previous error)"
                            );
                        }
                        error_msg = format!("{e:?}");
                        failed = true;
                        // Tear down peers via the attempt-scoped token so the
                        // user-cancel signal is not raised. The outer loop
                        // distinguishes user-cancel from internal failure by
                        // inspecting `user_cancel_token` after this drains.
                        attempt_token.cancel();
                    }
                }
            }
            Err(e) => {
                error_msg = format!("segment task panicked: {e}");
                failed = true;
                attempt_token.cancel();
            }
        }
    }

    if user_cancel_token.is_cancelled() {
        return AttemptOutcome::Cancelled;
    }
    if failed {
        return AttemptOutcome::Failed(error_msg);
    }
    AttemptOutcome::Completed
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering as AtomicOrdering};
    use std::sync::{Condvar, Mutex};
    use std::time::Duration;

    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::domain::model::account::AccountId;
    use crate::domain::model::download::{Download, DownloadId, Url};
    use crate::domain::model::meta::DownloadMeta;
    use crate::domain::ports::driven::{
        DownloadSourceResolver, EventBus, FileStorage, ResolvedDownloadSource,
    };

    // --- Mock types ---

    type WriteRecord = (PathBuf, u64, Vec<u8>);

    struct MockFileStorage {
        writes: Arc<Mutex<Vec<WriteRecord>>>,
    }

    impl MockFileStorage {
        fn new() -> Self {
            Self {
                writes: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl FileStorage for MockFileStorage {
        fn create_file(&self, _path: &Path, _size: u64) -> Result<(), DomainError> {
            Ok(())
        }

        fn write_segment(&self, path: &Path, offset: u64, data: &[u8]) -> Result<(), DomainError> {
            self.writes
                .lock()
                .unwrap()
                .push((path.to_path_buf(), offset, data.to_vec()));
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

    struct CollectingEventBus {
        events: Arc<Mutex<Vec<DomainEvent>>>,
    }

    impl CollectingEventBus {
        fn new() -> Self {
            Self {
                events: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn collected(&self) -> Vec<DomainEvent> {
            self.events.lock().unwrap().clone()
        }

        async fn wait_for_event_async<F>(&self, predicate: F, timeout: Duration) -> bool
        where
            F: Fn(&DomainEvent) -> bool,
        {
            let deadline = tokio::time::Instant::now() + timeout;
            loop {
                if self.collected().iter().any(&predicate) {
                    return true;
                }
                if tokio::time::Instant::now() >= deadline {
                    return false;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
    }

    impl EventBus for CollectingEventBus {
        fn publish(&self, event: DomainEvent) {
            self.events.lock().unwrap().push(event);
        }

        fn subscribe(&self, _handler: Box<dyn Fn(&DomainEvent) + Send + Sync + 'static>) {}
    }

    fn make_download(id: u64, url: &str) -> Download {
        let download_id = DownloadId(id);
        let parsed_url = Url::new(url).unwrap();
        // destination_path must be the full file path (dir + filename),
        // matching how StartDownloadCommand builds it in production.
        Download::new(
            download_id,
            parsed_url,
            "test_file.bin".to_string(),
            "/tmp/test_file.bin".to_string(),
        )
    }

    fn make_engine(
        storage: Arc<dyn FileStorage>,
        bus: Arc<dyn EventBus>,
    ) -> SegmentedDownloadEngine {
        SegmentedDownloadEngine::new(reqwest::Client::new(), storage, bus, 4)
    }

    struct LoopbackSourceResolver {
        calls: AtomicUsize,
    }

    struct RotatingHosterSourceResolver {
        calls: AtomicUsize,
    }

    struct BlockingSourceResolver {
        entered: Mutex<Option<std::sync::mpsc::Sender<()>>>,
        release: Arc<(Mutex<bool>, Condvar)>,
        committed: AtomicBool,
        finished: AtomicBool,
    }

    impl DownloadSourceResolver for BlockingSourceResolver {
        fn resolve(&self, _: &Download) -> Result<ResolvedDownloadSource, DomainError> {
            Err(DomainError::PluginError(
                "cancellable path was not used".into(),
            ))
        }

        fn resolve_cancellable(
            &self,
            _: &Download,
            cancellation: &ResolutionCancellation,
        ) -> Result<ResolvedDownloadSource, DomainError> {
            if let Some(entered) = self.entered.lock().unwrap().take() {
                entered.send(()).unwrap();
            }
            let (released, condition) = &*self.release;
            let mut released = released.lock().unwrap();
            while !*released {
                released = condition.wait(released).unwrap();
            }
            let result = cancellation.run_if_active(|| {
                self.committed.store(true, AtomicOrdering::SeqCst);
                Ok(ResolvedDownloadSource::sensitive(
                    "https://1.1.1.1/late-token".into(),
                ))
            });
            self.finished.store(true, AtomicOrdering::SeqCst);
            result
        }
    }

    impl DownloadSourceResolver for LoopbackSourceResolver {
        fn resolve(&self, _: &Download) -> Result<ResolvedDownloadSource, DomainError> {
            self.calls.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(ResolvedDownloadSource::sensitive(
                "https://127.0.0.1/secret-token".into(),
            ))
        }
    }

    impl DownloadSourceResolver for RotatingHosterSourceResolver {
        fn requires_resolution(&self, _: &Download) -> Result<bool, DomainError> {
            Ok(true)
        }

        fn resolve(&self, _: &Download) -> Result<ResolvedDownloadSource, DomainError> {
            let call = self.calls.fetch_add(1, AtomicOrdering::SeqCst) + 1;
            Ok(
                ResolvedDownloadSource::sensitive(format!(
                    "https://1.1.1.1/file?capability={call}"
                ))
                .with_request_headers(vec![(
                    "Authorization".into(),
                    format!("Bearer token-{call}"),
                )]),
            )
        }
    }

    #[tokio::test]
    async fn premium_source_is_resolved_jit_and_blocked_before_private_network_access() {
        let storage = Arc::new(MockFileStorage::new());
        let bus = Arc::new(CollectingEventBus::new());
        let resolver = Arc::new(LoopbackSourceResolver {
            calls: AtomicUsize::new(0),
        });
        let engine = make_engine(storage, bus.clone()).with_source_resolver(resolver.clone());
        let download = make_download(99, "https://1fichier.com/?abc123")
            .with_module_name("vortex-mod-1fichier".into())
            .with_account_id(AccountId::new("account-1"));

        engine.start(&download).expect("spawn download");

        assert!(
            bus.wait_for_event_async(
                |event| matches!(event, DomainEvent::DownloadFailed { id, .. } if id.0 == 99),
                Duration::from_secs(2),
            )
            .await
        );
        assert_eq!(resolver.calls.load(AtomicOrdering::SeqCst), 1);
        let serialized = format!("{:?}", bus.collected());
        assert!(!serialized.contains("secret-token"));
        assert_eq!(download.url().as_str(), "https://1fichier.com/?abc123");
    }

    #[tokio::test]
    async fn every_hoster_engine_start_resolves_a_fresh_download_capability() {
        for (index, service) in [
            "vortex-mod-mediafire",
            "vortex-mod-pixeldrain",
            "vortex-mod-gofile",
        ]
        .into_iter()
        .enumerate()
        {
            let resolver = Arc::new(RotatingHosterSourceResolver {
                calls: AtomicUsize::new(0),
            });
            let download = make_download(200 + index as u64, "https://hoster.example/page")
                .with_module_name(service.into());

            let first = prepare_sources(
                download.clone(),
                Some(resolver.clone()),
                reqwest::Client::new(),
                CancellationToken::new(),
                ResolutionCancellation::default(),
            )
            .await
            .expect("first start resolves");
            let retry = prepare_sources(
                download,
                Some(resolver.clone()),
                reqwest::Client::new(),
                CancellationToken::new(),
                ResolutionCancellation::default(),
            )
            .await
            .expect("retry resolves again");

            assert_eq!(resolver.calls.load(AtomicOrdering::SeqCst), 2, "{service}");
            assert_ne!(first.urls, retry.urls, "{service}");
            assert!(first.sensitive);
            assert!(retry.sensitive);
        }
    }

    #[tokio::test]
    async fn sensitive_hoster_attempt_rejects_an_unexpected_html_page() {
        let server = MockServer::start().await;
        Mock::given(method("HEAD"))
            .and(path("/download"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/html; charset=utf-8")
                    .insert_header("content-length", "18"),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/download"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/html; charset=utf-8")
                    .set_body_string("<html>expired</html>"),
            )
            .mount(&server)
            .await;
        let storage: Arc<dyn FileStorage> = Arc::new(MockFileStorage::new());
        let events: Arc<dyn EventBus> = Arc::new(CollectingEventBus::new());

        let outcome = run_mirror_attempt(MirrorAttemptParams {
            url: format!("{}/download", server.uri()),
            download_id: DownloadId(299),
            segments_count: 1,
            client: reqwest::Client::new(),
            file_storage: storage,
            event_bus: events,
            dest_path: PathBuf::from("/tmp/vortex-hoster-html-test.bin"),
            pause_rx: watch::channel(false).1,
            user_cancel_token: CancellationToken::new(),
            attempt_token: CancellationToken::new(),
            min_segment_bytes: 1,
            dynamic_split_enabled: Arc::new(AtomicBool::new(false)),
            dynamic_split_min_remaining_bytes: Arc::new(AtomicU64::new(1)),
            resume_url: "https://hoster.example/page".into(),
            sensitive_url: true,
        })
        .await;

        match outcome {
            AttemptOutcome::Failed(error) => assert!(error.contains("HTML"), "{error}"),
            _ => panic!("hoster HTML must be rejected"),
        }
    }

    #[tokio::test]
    async fn sensitive_hoster_attempt_rejects_html_disguised_as_binary() {
        let server = MockServer::start().await;
        let body = "<!doctype html><html>expired</html>";
        Mock::given(method("HEAD"))
            .and(path("/download"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/octet-stream")
                    .insert_header("content-length", body.len().to_string()),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/download"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/octet-stream")
                    .set_body_string(body),
            )
            .mount(&server)
            .await;

        let outcome = run_mirror_attempt(MirrorAttemptParams {
            url: format!("{}/download", server.uri()),
            download_id: DownloadId(300),
            segments_count: 1,
            client: reqwest::Client::new(),
            file_storage: Arc::new(MockFileStorage::new()),
            event_bus: Arc::new(CollectingEventBus::new()),
            dest_path: PathBuf::from("/tmp/vortex-hoster-disguised-html-test.bin"),
            pause_rx: watch::channel(false).1,
            user_cancel_token: CancellationToken::new(),
            attempt_token: CancellationToken::new(),
            min_segment_bytes: 1,
            dynamic_split_enabled: Arc::new(AtomicBool::new(false)),
            dynamic_split_min_remaining_bytes: Arc::new(AtomicU64::new(1)),
            resume_url: "https://hoster.example/page".into(),
            sensitive_url: true,
        })
        .await;

        match outcome {
            AttemptOutcome::Failed(error) => assert!(error.contains("HTML"), "{error}"),
            _ => panic!("disguised hoster HTML must be rejected"),
        }
    }

    #[tokio::test]
    async fn sensitive_hoster_attempt_never_deletes_an_unowned_destination() {
        let server = MockServer::start().await;
        Mock::given(method("HEAD"))
            .and(path("/download"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/octet-stream")
                    .insert_header("content-length", "4"),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/download"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"data"))
            .mount(&server)
            .await;
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("existing.bin");
        std::fs::write(&destination, b"user-owned").unwrap();

        let outcome = run_mirror_attempt(MirrorAttemptParams {
            url: format!("{}/download", server.uri()),
            download_id: DownloadId(301),
            segments_count: 1,
            client: reqwest::Client::new(),
            file_storage: Arc::new(MockFileStorage::new()),
            event_bus: Arc::new(CollectingEventBus::new()),
            dest_path: destination.clone(),
            pause_rx: watch::channel(false).1,
            user_cancel_token: CancellationToken::new(),
            attempt_token: CancellationToken::new(),
            min_segment_bytes: 1,
            dynamic_split_enabled: Arc::new(AtomicBool::new(false)),
            dynamic_split_min_remaining_bytes: Arc::new(AtomicU64::new(1)),
            resume_url: "https://hoster.example/page".into(),
            sensitive_url: true,
        })
        .await;

        assert!(matches!(outcome, AttemptOutcome::Failed(_)));
        assert_eq!(std::fs::read(destination).unwrap(), b"user-owned");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancellation_wins_against_blocked_jit_resolution_without_late_commit() {
        let storage = Arc::new(MockFileStorage::new());
        let bus = Arc::new(CollectingEventBus::new());
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let resolver = Arc::new(BlockingSourceResolver {
            entered: Mutex::new(Some(entered_tx)),
            release: release.clone(),
            committed: AtomicBool::new(false),
            finished: AtomicBool::new(false),
        });
        let engine = make_engine(storage, bus.clone()).with_source_resolver(resolver.clone());
        let download = make_download(98, "https://1fichier.com/?abc123")
            .with_module_name("vortex-mod-1fichier".into())
            .with_account_id(AccountId::new("account-1"));
        engine.start(&download).expect("spawn download");
        tokio::task::spawn_blocking(move || entered_rx.recv_timeout(Duration::from_secs(1)))
            .await
            .unwrap()
            .expect("resolver entered");

        engine
            .cancel(download.id())
            .expect("cancel active download");
        assert!(
            bus.wait_for_event_async(
                |event| matches!(event, DomainEvent::DownloadCancelled { id } if id.0 == 98),
                Duration::from_millis(300),
            )
            .await,
            "cancellation must not wait for the blocking resolver"
        );

        let (released, condition) = &*release;
        *released.lock().unwrap() = true;
        condition.notify_all();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
        while !resolver.finished.load(AtomicOrdering::SeqCst)
            && tokio::time::Instant::now() < deadline
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(resolver.finished.load(AtomicOrdering::SeqCst));
        assert!(!resolver.committed.load(AtomicOrdering::SeqCst));
    }

    // --- Tests ---

    #[tokio::test]
    async fn test_start_spawns_download_and_completes() {
        let server = MockServer::start().await;
        let body = vec![b'a'; 1024];

        Mock::given(method("HEAD"))
            .and(path("/file"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-length", "1024")
                    .insert_header("accept-ranges", "bytes"),
            )
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/file"))
            .respond_with(ResponseTemplate::new(206).set_body_bytes(body))
            .mount(&server)
            .await;

        let storage = Arc::new(MockFileStorage::new());
        let bus = Arc::new(CollectingEventBus::new());
        let engine = make_engine(storage, bus.clone());

        let url = format!("{}/file", server.uri());
        let download = make_download(1, &url);

        engine.start(&download).unwrap();

        let found = bus
            .wait_for_event_async(
                |e| matches!(e, DomainEvent::DownloadCompleted { id } if id.0 == 1),
                Duration::from_secs(5),
            )
            .await;

        assert!(found, "DownloadCompleted not received");

        let events = bus.collected();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, DomainEvent::DownloadStarted { id } if id.0 == 1)),
            "DownloadStarted not published"
        );
    }

    #[tokio::test]
    async fn test_start_fallback_single_segment_no_range() {
        let server = MockServer::start().await;
        let body = vec![b'b'; 512];

        Mock::given(method("HEAD"))
            .and(path("/norange"))
            .respond_with(
                ResponseTemplate::new(200).insert_header("content-length", "512"),
                // No accept-ranges header → single segment fallback
            )
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/norange"))
            .respond_with(ResponseTemplate::new(206).set_body_bytes(body))
            .mount(&server)
            .await;

        let storage = Arc::new(MockFileStorage::new());
        let bus = Arc::new(CollectingEventBus::new());
        let engine = make_engine(storage, bus.clone());

        let url = format!("{}/norange", server.uri());
        let download = make_download(2, &url);

        engine.start(&download).unwrap();

        let found = bus
            .wait_for_event_async(
                |e| {
                    matches!(
                        e,
                        DomainEvent::DownloadCompleted { id } | DomainEvent::DownloadFailed { id, .. }
                        if id.0 == 2
                    )
                },
                Duration::from_secs(5),
            )
            .await;

        assert!(found, "download did not finish");

        let events = bus.collected();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, DomainEvent::DownloadCompleted { id } if id.0 == 2)),
            "expected DownloadCompleted, events: {events:?}"
        );
    }

    #[tokio::test]
    async fn test_start_falls_back_to_get_when_head_returns_non_success() {
        let server = MockServer::start().await;
        let body = vec![b'g'; 256];

        Mock::given(method("HEAD"))
            .and(path("/head-blocked"))
            .respond_with(ResponseTemplate::new(405))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/head-blocked"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-length", "256")
                    .set_body_bytes(body),
            )
            .mount(&server)
            .await;

        let storage = Arc::new(MockFileStorage::new());
        let bus = Arc::new(CollectingEventBus::new());
        let engine = make_engine(storage, bus.clone());

        let url = format!("{}/head-blocked", server.uri());
        let download = make_download(20, &url);

        engine.start(&download).unwrap();

        let found = bus
            .wait_for_event_async(
                |e| matches!(e, DomainEvent::DownloadCompleted { id } if id.0 == 20),
                Duration::from_secs(5),
            )
            .await;

        assert!(found, "download should complete via GET fallback");
    }

    #[tokio::test]
    async fn test_pause_sends_signal() {
        let server = MockServer::start().await;
        // Slow server to keep download active
        let body = vec![b'p'; 64 * 1024];

        Mock::given(method("HEAD"))
            .and(path("/slow"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-length", &(64 * 1024u64).to_string())
                    .insert_header("accept-ranges", "bytes"),
            )
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/slow"))
            .respond_with(
                ResponseTemplate::new(206)
                    .set_body_bytes(body)
                    .set_delay(Duration::from_secs(10)),
            )
            .mount(&server)
            .await;

        let storage = Arc::new(MockFileStorage::new());
        let bus = Arc::new(CollectingEventBus::new());
        let engine = make_engine(storage, bus.clone());

        let url = format!("{}/slow", server.uri());
        let download = make_download(3, &url);

        engine.start(&download).unwrap();

        // Wait for DownloadStarted before pausing
        bus.wait_for_event_async(
            |e| matches!(e, DomainEvent::DownloadStarted { id } if id.0 == 3),
            Duration::from_secs(3),
        )
        .await;

        let pause_result = engine.pause(DownloadId(3));
        assert!(pause_result.is_ok(), "pause should succeed");

        let events = bus.collected();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, DomainEvent::DownloadPaused { id } if id.0 == 3)),
            "DownloadPaused not published"
        );

        // Clean up
        let _ = engine.cancel(DownloadId(3));
    }

    #[tokio::test]
    async fn test_cancel_stops_download() {
        let server = MockServer::start().await;
        let body = vec![b'c'; 64 * 1024];

        Mock::given(method("HEAD"))
            .and(path("/cancel"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-length", &(64 * 1024u64).to_string())
                    .insert_header("accept-ranges", "bytes"),
            )
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/cancel"))
            .respond_with(
                ResponseTemplate::new(206)
                    .set_body_bytes(body)
                    .set_delay(Duration::from_secs(10)),
            )
            .mount(&server)
            .await;

        let storage = Arc::new(MockFileStorage::new());
        let bus = Arc::new(CollectingEventBus::new());
        let engine = make_engine(storage, bus.clone());

        let url = format!("{}/cancel", server.uri());
        let download = make_download(4, &url);

        engine.start(&download).unwrap();

        // Wait for DownloadStarted
        bus.wait_for_event_async(
            |e| matches!(e, DomainEvent::DownloadStarted { id } if id.0 == 4),
            Duration::from_secs(3),
        )
        .await;

        let cancel_result = engine.cancel(DownloadId(4));
        assert!(cancel_result.is_ok(), "cancel should succeed");

        // Cancel is idempotent — second call succeeds (task removes itself on exit)
        let cancel_again = engine.cancel(DownloadId(4));
        assert!(
            cancel_again.is_ok(),
            "second cancel should succeed (idempotent)"
        );
    }

    #[tokio::test]
    async fn test_dynamic_split_skipped_when_remaining_too_small() {
        // 2 KiB total, 4 segments, min_remaining 4 MiB → split must NOT trigger.
        let server = MockServer::start().await;
        let body = vec![b'a'; 2048];

        Mock::given(method("HEAD"))
            .and(path("/small"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-length", "2048")
                    .insert_header("accept-ranges", "bytes"),
            )
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/small"))
            .respond_with(ResponseTemplate::new(206).set_body_bytes(body))
            .mount(&server)
            .await;

        let storage = Arc::new(MockFileStorage::new());
        let bus = Arc::new(CollectingEventBus::new());
        let engine = SegmentedDownloadEngine::new(reqwest::Client::new(), storage, bus.clone(), 4)
            .with_min_segment_bytes(256)
            .with_dynamic_split(true, 4); // 4 MiB threshold blocks 2 KiB file

        let url = format!("{}/small", server.uri());
        let download = make_download(70, &url);
        engine.start(&download).unwrap();

        let found = bus
            .wait_for_event_async(
                |e| matches!(e, DomainEvent::DownloadCompleted { id } if id.0 == 70),
                Duration::from_secs(5),
            )
            .await;
        assert!(found, "download did not complete");

        let events = bus.collected();
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, DomainEvent::SegmentSplit { .. })),
            "no split should fire when remaining < threshold; got {events:?}"
        );
    }

    #[tokio::test]
    async fn test_dynamic_split_disabled_via_config_does_not_split() {
        let server = MockServer::start().await;
        let body = vec![b'x'; 64 * 1024];

        Mock::given(method("HEAD"))
            .and(path("/disabled"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-length", "65536")
                    .insert_header("accept-ranges", "bytes"),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/disabled"))
            .respond_with(ResponseTemplate::new(206).set_body_bytes(body))
            .mount(&server)
            .await;

        let storage = Arc::new(MockFileStorage::new());
        let bus = Arc::new(CollectingEventBus::new());
        let engine = SegmentedDownloadEngine::new(reqwest::Client::new(), storage, bus.clone(), 4)
            .with_min_segment_bytes(1024)
            .with_dynamic_split(false, 0);

        let url = format!("{}/disabled", server.uri());
        let download = make_download(71, &url);
        engine.start(&download).unwrap();

        let found = bus
            .wait_for_event_async(
                |e| matches!(e, DomainEvent::DownloadCompleted { id } if id.0 == 71),
                Duration::from_secs(5),
            )
            .await;
        assert!(found);
        let events = bus.collected();
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, DomainEvent::SegmentSplit { .. })),
            "split must not fire when disabled"
        );
    }

    #[test]
    fn test_pick_split_target_prefers_slowest_above_threshold() {
        let make = |start: u64, end: u64, downloaded: u64, age_ms: u64| SegmentRuntimeState {
            end_tx: watch::channel(end).0,
            progress: Arc::new(AtomicU64::new(downloaded)),
            started_at: std::time::Instant::now() - std::time::Duration::from_millis(age_ms),
            start_byte: start,
            initial_end: end,
            completed: false,
        };
        let segs = [
            // fast: 1 MiB downloaded in 1500 ms → ~700 KiB/s
            make(0, 16 * 1024 * 1024, 1024 * 1024, 1500),
            // slow: 100 KiB in 1000 ms → ~100 KiB/s, plenty of remaining
            make(16 * 1024 * 1024, 32 * 1024 * 1024, 100 * 1024, 1000),
            // tiny remaining → must be filtered
            make(32 * 1024 * 1024, 32 * 1024 * 1024 + 1024, 512, 600),
        ];
        let pick = pick_split_target(&segs, 4 * 1024 * 1024);
        assert_eq!(
            pick.map(|(i, _)| i),
            Some(1),
            "expected slot 1 (slowest with enough remaining), got {pick:?}"
        );
        let (_, split_at) = pick.unwrap();
        assert!(
            split_at > 16 * 1024 * 1024 + 100 * 1024,
            "split must be above current offset"
        );
        assert!(
            split_at < 32 * 1024 * 1024,
            "split must be below initial_end"
        );
    }

    #[test]
    fn test_pick_split_target_returns_none_when_all_below_threshold() {
        let make = |start: u64, end: u64, downloaded: u64| SegmentRuntimeState {
            end_tx: watch::channel(end).0,
            progress: Arc::new(AtomicU64::new(downloaded)),
            started_at: std::time::Instant::now() - std::time::Duration::from_millis(800),
            start_byte: start,
            initial_end: end,
            completed: false,
        };
        let segs = [make(0, 1024, 100), make(1024, 2048, 1), make(2048, 3072, 1)];
        let pick = pick_split_target(&segs, 4 * 1024 * 1024);
        assert!(pick.is_none(), "got {pick:?}");
    }

    #[test]
    fn test_pick_split_target_skips_fresh_segments() {
        // Brand-new split children should not be candidates: no throughput
        // sample yet (downloaded == 0) and elapsed below MIN_SPLIT_SAMPLE_DURATION.
        // A genuinely slow neighbor (1000 ms / 100 KiB) sits next to them.
        let mk = |start: u64, end: u64, downloaded: u64, age_ms: u64| SegmentRuntimeState {
            end_tx: watch::channel(end).0,
            progress: Arc::new(AtomicU64::new(downloaded)),
            started_at: std::time::Instant::now() - std::time::Duration::from_millis(age_ms),
            start_byte: start,
            initial_end: end,
            completed: false,
        };
        let segs = [
            // fresh child: 0 bytes, 50 ms — must be skipped despite being "slowest"
            mk(0, 16 * 1024 * 1024, 0, 50),
            // slightly older but still no sample — must be skipped
            mk(16 * 1024 * 1024, 32 * 1024 * 1024, 0, 200),
            // genuinely slow but mature
            mk(32 * 1024 * 1024, 48 * 1024 * 1024, 100 * 1024, 1000),
        ];
        let pick = pick_split_target(&segs, 4 * 1024 * 1024);
        assert_eq!(pick.map(|(i, _)| i), Some(2), "got {pick:?}");
    }

    #[test]
    fn test_pick_split_target_skips_completed_segments() {
        // A completed slot must never be picked even if its throughput was the
        // slowest before completion.
        let mk = |start: u64, end: u64, downloaded: u64, completed: bool| SegmentRuntimeState {
            end_tx: watch::channel(end).0,
            progress: Arc::new(AtomicU64::new(downloaded)),
            started_at: std::time::Instant::now() - std::time::Duration::from_millis(1000),
            start_byte: start,
            initial_end: end,
            completed,
        };
        let segs = [
            // completed slow segment — must be ignored
            mk(0, 16 * 1024 * 1024, 16 * 1024 * 1024, true),
            // live, slower in absolute terms but only it is eligible
            mk(16 * 1024 * 1024, 32 * 1024 * 1024, 100 * 1024, false),
        ];
        let pick = pick_split_target(&segs, 4 * 1024 * 1024);
        assert_eq!(pick.map(|(i, _)| i), Some(1));
    }

    #[tokio::test]
    async fn test_pause_unknown_id_returns_not_found() {
        let storage = Arc::new(MockFileStorage::new());
        let bus = Arc::new(CollectingEventBus::new());
        let engine = make_engine(storage, bus);

        let result = engine.pause(DownloadId(999));
        assert!(
            matches!(result, Err(DomainError::NotFound(_))),
            "expected NotFound, got {result:?}"
        );
    }

    #[tokio::test]
    async fn test_cancel_unknown_id_returns_not_found() {
        let storage = Arc::new(MockFileStorage::new());
        let bus = Arc::new(CollectingEventBus::new());
        let engine = make_engine(storage, bus);

        let result = engine.cancel(DownloadId(888));
        assert!(
            matches!(result, Err(DomainError::NotFound(_))),
            "expected NotFound, got {result:?}"
        );
    }

    fn make_download_with_mirrors(id: u64, mirrors: Vec<crate::domain::model::Mirror>) -> Download {
        let download_id = DownloadId(id);
        // The canonical URL is intentionally invalid for the mock server so
        // failing this fallback still surfaces a clear test signal — we want
        // every fetch to flow through `mirrors`.
        let parsed_url = Url::new("https://invalid-canonical.example.invalid/file.bin").unwrap();
        Download::new(
            download_id,
            parsed_url,
            "test_file.bin".to_string(),
            "/tmp/test_file.bin".to_string(),
        )
        .with_mirrors(mirrors)
    }

    #[tokio::test]
    async fn test_three_mirrors_first_404_triggers_failover_to_second() {
        // 3 mirrors. First returns 404 for the GET range request so the
        // attempt fails; second succeeds. Engine must publish
        // MirrorSwitched then DownloadCompleted.
        let server = MockServer::start().await;
        let body = vec![b'm'; 256];

        // Mirror 1 — HEAD 404 (probe fail) so attempt fails fast.
        Mock::given(method("HEAD"))
            .and(path("/m1"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/m1"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        // Mirror 2 — works.
        Mock::given(method("HEAD"))
            .and(path("/m2"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-length", "256")
                    .insert_header("accept-ranges", "bytes"),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/m2"))
            .respond_with(ResponseTemplate::new(206).set_body_bytes(body.clone()))
            .mount(&server)
            .await;

        // Mirror 3 — also works but should not be hit (m2 succeeds first).
        Mock::given(method("HEAD"))
            .and(path("/m3"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-length", "256")
                    .insert_header("accept-ranges", "bytes"),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/m3"))
            .respond_with(ResponseTemplate::new(206).set_body_bytes(body))
            .mount(&server)
            .await;

        let mirrors = vec![
            crate::domain::model::Mirror::new(
                Url::new(&format!("{}/m1", server.uri())).unwrap(),
                90, // highest priority — tried first
                None,
            )
            .unwrap(),
            crate::domain::model::Mirror::new(
                Url::new(&format!("{}/m2", server.uri())).unwrap(),
                70,
                None,
            )
            .unwrap(),
            crate::domain::model::Mirror::new(
                Url::new(&format!("{}/m3", server.uri())).unwrap(),
                50,
                None,
            )
            .unwrap(),
        ];

        let storage = Arc::new(MockFileStorage::new());
        let bus = Arc::new(CollectingEventBus::new());
        let engine = make_engine(storage, bus.clone());

        let download = make_download_with_mirrors(100, mirrors);
        engine.start(&download).unwrap();

        let completed = bus
            .wait_for_event_async(
                |e| {
                    matches!(
                        e,
                        DomainEvent::DownloadCompleted { id } | DomainEvent::DownloadFailed { id, .. }
                        if id.0 == 100
                    )
                },
                Duration::from_secs(10),
            )
            .await;
        assert!(completed, "engine did not finish");

        let events = bus.collected();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, DomainEvent::DownloadCompleted { id } if id.0 == 100)),
            "expected DownloadCompleted, events: {events:?}"
        );

        let switched: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                DomainEvent::MirrorSwitched {
                    id,
                    new_mirror_index,
                    new_url,
                } if id.0 == 100 => Some((*new_mirror_index, new_url.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            switched.len(),
            1,
            "exactly one mirror switch expected, got {switched:?}"
        );
        let (idx, url) = &switched[0];
        assert_eq!(*idx, 1, "switched to slot 1 (priority 70)");
        assert!(url.ends_with("/m2"), "expected /m2 mirror url, got {url}");
    }

    #[tokio::test]
    async fn test_all_mirrors_fail_publishes_download_failed() {
        let server = MockServer::start().await;

        for p in ["/all1", "/all2"] {
            Mock::given(method("HEAD"))
                .and(path(p))
                .respond_with(ResponseTemplate::new(500))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path(p))
                .respond_with(ResponseTemplate::new(500))
                .mount(&server)
                .await;
        }

        let mirrors = vec![
            crate::domain::model::Mirror::new(
                Url::new(&format!("{}/all1", server.uri())).unwrap(),
                80,
                None,
            )
            .unwrap(),
            crate::domain::model::Mirror::new(
                Url::new(&format!("{}/all2", server.uri())).unwrap(),
                40,
                None,
            )
            .unwrap(),
        ];

        let storage = Arc::new(MockFileStorage::new());
        let bus = Arc::new(CollectingEventBus::new());
        let engine = make_engine(storage, bus.clone());

        let download = make_download_with_mirrors(101, mirrors);
        engine.start(&download).unwrap();

        let done = bus
            .wait_for_event_async(
                |e| matches!(e, DomainEvent::DownloadFailed { id, .. } if id.0 == 101),
                Duration::from_secs(10),
            )
            .await;
        assert!(done, "expected DownloadFailed after all mirrors exhausted");

        let events = bus.collected();
        // Exactly one MirrorSwitched (between mirror 1 and mirror 2).
        let switches: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, DomainEvent::MirrorSwitched { id, .. } if id.0 == 101))
            .collect();
        assert_eq!(switches.len(), 1, "switched once before final failure");
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, DomainEvent::DownloadCompleted { id } if id.0 == 101)),
            "must not emit DownloadCompleted on full mirror exhaustion"
        );
    }

    #[tokio::test]
    async fn test_priority_respected_highest_first() {
        // Priority order: low (10) < mid (50) < high (90). The engine
        // must pick `high` first; we confirm by failing only `high` and
        // observing exactly one MirrorSwitched ending on `mid`.
        let server = MockServer::start().await;
        let body = vec![b'p'; 128];

        // high — fails
        Mock::given(method("HEAD"))
            .and(path("/high"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/high"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        // mid — succeeds
        Mock::given(method("HEAD"))
            .and(path("/mid"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-length", "128")
                    .insert_header("accept-ranges", "bytes"),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/mid"))
            .respond_with(ResponseTemplate::new(206).set_body_bytes(body))
            .mount(&server)
            .await;
        // low — must not be reached
        Mock::given(method("HEAD"))
            .and(path("/low"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        // Insert in non-priority-order to assert the engine sorts.
        let mirrors = vec![
            crate::domain::model::Mirror::new(
                Url::new(&format!("{}/low", server.uri())).unwrap(),
                10,
                None,
            )
            .unwrap(),
            crate::domain::model::Mirror::new(
                Url::new(&format!("{}/high", server.uri())).unwrap(),
                90,
                None,
            )
            .unwrap(),
            crate::domain::model::Mirror::new(
                Url::new(&format!("{}/mid", server.uri())).unwrap(),
                50,
                None,
            )
            .unwrap(),
        ];

        let storage = Arc::new(MockFileStorage::new());
        let bus = Arc::new(CollectingEventBus::new());
        let engine = make_engine(storage, bus.clone());

        let download = make_download_with_mirrors(102, mirrors);
        engine.start(&download).unwrap();

        let done = bus
            .wait_for_event_async(
                |e| matches!(e, DomainEvent::DownloadCompleted { id } if id.0 == 102),
                Duration::from_secs(10),
            )
            .await;
        assert!(done, "expected DownloadCompleted via mid mirror");

        let events = bus.collected();
        let switches: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                DomainEvent::MirrorSwitched { id, new_url, .. } if id.0 == 102 => {
                    Some(new_url.clone())
                }
                _ => None,
            })
            .collect();
        assert_eq!(switches.len(), 1, "exactly one switch (high → mid)");
        assert!(
            switches[0].ends_with("/mid"),
            "expected switch to /mid, got {}",
            switches[0]
        );
    }
}
