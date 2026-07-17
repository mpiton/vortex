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

use super::download_artifact_lifecycle::{
    AttemptFailure, AttemptOutcome, cleanup_download_artifacts, ownership_metadata,
    resume_metadata_matches,
};
use super::download_source_preparation::prepare_sources;
use super::segment_worker::{SegmentError, SegmentParams, download_segment};
use super::{SourcePolicy, format_error_chain, safe_source_failure};

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
                download.clone(),
                source_resolver.clone(),
                client.clone(),
                cancel_token.clone(),
                resolution_cancellation.clone(),
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
                            error: safe_source_failure(&error),
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
            let mut download_client = prepared.client;
            let mut mirror_urls = prepared.urls;
            let mut resume_url = prepared.resume_url;
            let mut source_policy = prepared.policy;
            let mut size_hint = prepared.size_hint;
            let mut resume_supported = prepared.resume_supported;
            let mut resolved_source = prepared.resolved;
            if cancel_token.is_cancelled() {
                event_bus.publish(DomainEvent::DownloadCancelled { id: download_id });
                active_downloads
                    .lock()
                    .expect("active_downloads lock poisoned")
                    .remove(&download_id);
                return;
            }
            let mut mirror_idx = prepared.initial_index;
            let mut source_refreshes = 0;
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
                    client: download_client.clone(),
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
                    source_policy,
                    size_hint,
                    resume_supported,
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
                    AttemptOutcome::Failed(failure) => {
                        if failure.retryable_with_mirror && resolved_source && source_refreshes == 0
                        {
                            if cancel_token.is_cancelled() {
                                event_bus
                                    .publish(DomainEvent::DownloadCancelled { id: download_id });
                                break;
                            }
                            if failure.owns_artifacts
                                && let Err(error) =
                                    cleanup_download_artifacts(&file_storage, &dest_path).await
                            {
                                tracing::warn!(
                                    download_id = download_id.0,
                                    error = %error,
                                    "failed to reset download artifacts before source refresh"
                                );
                                event_bus.publish(DomainEvent::DownloadFailed {
                                    id: download_id,
                                    error:
                                        "failed to reset the partial download before source refresh"
                                            .into(),
                                });
                                break;
                            }
                            let refreshed = match prepare_sources(
                                download.clone(),
                                source_resolver.clone(),
                                client.clone(),
                                cancel_token.clone(),
                                resolution_cancellation.clone(),
                            )
                            .await
                            {
                                Ok(refreshed) => refreshed,
                                Err(error) => {
                                    let event = if cancel_token.is_cancelled() {
                                        DomainEvent::DownloadCancelled { id: download_id }
                                    } else {
                                        DomainEvent::DownloadFailed {
                                            id: download_id,
                                            error: safe_source_failure(&error),
                                        }
                                    };
                                    event_bus.publish(event);
                                    break;
                                }
                            };
                            download_client = refreshed.client;
                            mirror_urls = refreshed.urls;
                            mirror_idx = refreshed.initial_index;
                            resume_url = refreshed.resume_url;
                            source_policy = refreshed.policy;
                            size_hint = refreshed.size_hint;
                            resume_supported = refreshed.resume_supported;
                            resolved_source = refreshed.resolved;
                            source_refreshes += 1;
                            continue;
                        }
                        let next = mirror_idx + 1;
                        if failure.retryable_with_mirror && next < mirror_urls.len() {
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
                            if source_policy.is_protected() {
                                tracing::info!(
                                    download_id = download_id.0,
                                    new_mirror_index = mirror_idx,
                                    "switching protected source after failure"
                                );
                            } else {
                                tracing::info!(
                                    download_id = download_id.0,
                                    new_mirror_index = mirror_idx,
                                    new_url = %mirror_urls[mirror_idx],
                                    previous_error = %failure.message,
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
                            if failure.owns_artifacts
                                && let Err(error) =
                                    cleanup_download_artifacts(&file_storage, &dest_path).await
                            {
                                tracing::warn!(
                                    download_id = download_id.0,
                                    error = %error,
                                    "failed to reset download artifacts before mirror retry"
                                );
                                event_bus.publish(DomainEvent::DownloadFailed {
                                    id: download_id,
                                    error:
                                        "failed to reset the partial download before mirror retry"
                                            .into(),
                                });
                                break;
                            }
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
                                new_url: if source_policy.is_protected() {
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
                        if failure.retryable_with_mirror {
                            event_bus.publish(DomainEvent::AllMirrorsExhausted { id: download_id });
                        }
                        event_bus.publish(DomainEvent::DownloadFailed {
                            id: download_id,
                            error: failure.message,
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
    source_policy: SourcePolicy,
    size_hint: Option<u64>,
    resume_supported: Option<bool>,
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
        source_policy,
        size_hint,
        resume_supported,
    } = params;
    let metadata = match SegmentedDownloadEngine::probe_remote_metadata(&client, &url).await {
        Ok(metadata) => metadata,
        Err(e) => {
            if source_policy.is_protected() {
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
            return AttemptOutcome::Failed(AttemptFailure::retryable(
                if source_policy.is_protected() {
                    "metadata probe failed".into()
                } else {
                    format!("metadata probe failed: {}", format_error_chain(&e))
                },
                false,
            ));
        }
    };

    if let Some(error) =
        source_policy.response_error(metadata.status, metadata.content_type.as_deref())
    {
        return AttemptOutcome::Failed(AttemptFailure::retryable(
            safe_source_failure(&error),
            false,
        ));
    }
    if metadata.content_length > 0
        && size_hint.is_some_and(|hint| hint > 0 && hint != metadata.content_length)
    {
        return AttemptOutcome::Failed(AttemptFailure::retryable(
            "remote size conflicts with resolved download metadata".into(),
            false,
        ));
    }
    let total_size = if metadata.content_length > 0 {
        metadata.content_length
    } else {
        size_hint.unwrap_or(0)
    };
    let supports_range = metadata.accepts_ranges && resume_supported != Some(false);

    if user_cancel_token.is_cancelled() {
        return AttemptOutcome::Cancelled;
    }

    let created_by_attempt = {
        let storage = file_storage.clone();
        let path = dest_path.clone();
        let stable_url = resume_url.clone();
        match tokio::task::spawn_blocking(move || {
            if storage.file_exists(&path)? {
                match storage.read_meta(&path)? {
                    Some(metadata)
                        if resume_metadata_matches(
                            &metadata,
                            download_id,
                            &stable_url,
                            &path,
                            total_size,
                        ) =>
                    {
                        return Ok(false);
                    }
                    Some(metadata) if metadata.download_id != download_id => {
                        return Err(DomainError::AlreadyExists(
                            "destination belongs to another Vortex download".into(),
                        ));
                    }
                    Some(_) => storage.delete_download_artifacts(&path)?,
                    None => {
                        return Err(DomainError::AlreadyExists(
                            "destination file has no Vortex resume metadata".into(),
                        ));
                    }
                }
            }
            storage.create_file(&path, total_size)?;
            let metadata = ownership_metadata(download_id, stable_url, &path, total_size);
            if let Err(error) = storage.write_meta(&path, &metadata) {
                if let Err(cleanup_error) = storage.delete_download_artifacts(&path) {
                    tracing::warn!(
                        download_id = download_id.0,
                        error = %cleanup_error,
                        "failed to roll back a download whose ownership metadata could not be written"
                    );
                }
                return Err(error);
            }
            Ok(true)
        })
        .await
        {
            Err(e) => {
                tracing::error!(
                    download_id = download_id.0,
                    error = %e,
                    "spawn_blocking for create_file panicked"
                );
                return AttemptOutcome::Failed(AttemptFailure::terminal(
                    format!("file pre-allocation failed: {e}"),
                    false,
                ));
            }
            Ok(Err(e)) => {
                return AttemptOutcome::Failed(AttemptFailure::terminal(
                    format!("file pre-allocation failed: {e}"),
                    false,
                ));
            }
            Ok(Ok(created)) => created,
        }
    };

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

    let cancelled = user_cancel_token.is_cancelled();
    if source_policy.is_protected()
        && created_by_attempt
        && cancelled
        && let Err(error) = cleanup_download_artifacts(&file_storage, &dest_path).await
    {
        tracing::warn!(
            download_id = download_id.0,
            error = %error,
            "failed to clean up a cancelled protected download attempt"
        );
    }
    if cancelled {
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
            source_policy,
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
                        source_policy,
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

    let cancelled = user_cancel_token.is_cancelled();
    let cleanup_failed =
        if source_policy.is_protected() && created_by_attempt && (cancelled || failed) {
            cleanup_download_artifacts(&file_storage, &dest_path)
                .await
                .inspect_err(|error| {
                    tracing::warn!(
                        download_id = download_id.0,
                        error = %error,
                        "failed to clean up a protected download attempt"
                    );
                })
                .is_err()
        } else {
            false
        };
    if cancelled {
        return AttemptOutcome::Cancelled;
    }
    if failed {
        if cleanup_failed {
            return AttemptOutcome::Failed(AttemptFailure::terminal(
                "failed to clean up the protected download attempt".into(),
                true,
            ));
        }
        return AttemptOutcome::Failed(AttemptFailure::retryable(error_msg, true));
    }
    let storage = file_storage.clone();
    let path = dest_path.clone();
    match tokio::task::spawn_blocking(move || storage.delete_meta(&path)).await {
        Ok(Ok(())) => AttemptOutcome::Completed,
        Ok(Err(error)) => {
            tracing::warn!(
                download_id = download_id.0,
                error = %error,
                "failed to finalize download ownership metadata"
            );
            AttemptOutcome::Failed(AttemptFailure::terminal(
                "failed to finalize the completed download".into(),
                true,
            ))
        }
        Err(error) => {
            tracing::warn!(
                download_id = download_id.0,
                error = %error,
                "download metadata finalization task stopped"
            );
            AttemptOutcome::Failed(AttemptFailure::terminal(
                "failed to finalize the completed download".into(),
                true,
            ))
        }
    }
}

#[cfg(test)]
#[path = "download_engine_test_support.rs"]
mod test_support;

#[cfg(test)]
#[path = "download_engine_hoster_tests.rs"]
mod hoster_tests;

#[cfg(test)]
#[path = "download_engine_tests.rs"]
mod tests;
