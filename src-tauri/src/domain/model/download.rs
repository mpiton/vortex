use crate::domain::error::DomainError;
use crate::domain::event::DomainEvent;
use crate::domain::model::checksum::ChecksumAlgorithm;
use crate::domain::model::mirror::{Mirror, sort_by_priority as sort_mirrors_by_priority};
use crate::domain::model::queue::Priority;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DownloadId(pub u64);

#[derive(Clone, PartialEq, Eq)]
pub struct Url {
    raw: String,
    scheme: String,
    host: String,
}

impl std::fmt::Debug for Url {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Redact path/query/fragment to avoid leaking tokens in logs
        write!(f, "Url({}://{}/<redacted>)", self.scheme, self.host)
    }
}

impl Url {
    pub fn new(s: &str) -> Result<Url, DomainError> {
        if s.is_empty() {
            return Err(DomainError::InvalidUrl("URL must not be empty".to_string()));
        }

        let scheme = if s.starts_with("https://") {
            "https"
        } else if s.starts_with("http://") {
            "http"
        } else if s.starts_with("ftp://") {
            "ftp"
        } else {
            return Err(DomainError::InvalidUrl(format!(
                "URL must start with http, https, or ftp: {s}"
            )));
        };

        let after_scheme = &s[scheme.len() + 3..]; // skip "scheme://"
        if after_scheme.is_empty() {
            return Err(DomainError::InvalidUrl(format!("URL has no host: {s}")));
        }

        // Extract authority (everything before first '/', '?', '#' or end)
        let authority = after_scheme.split(['/', '?', '#']).next().unwrap_or("");

        // Strip userinfo (user:pass@) if present
        let host_port = if let Some(at_pos) = authority.rfind('@') {
            &authority[at_pos + 1..]
        } else {
            authority
        };

        // Strip port if present
        let host = if let Some(colon_pos) = host_port.rfind(':') {
            let after_colon = &host_port[colon_pos + 1..];
            if after_colon.chars().all(|c| c.is_ascii_digit()) {
                &host_port[..colon_pos]
            } else {
                host_port
            }
        } else {
            host_port
        };

        if host.is_empty() {
            return Err(DomainError::InvalidUrl(format!("URL has no host: {s}")));
        }

        Ok(Url {
            raw: s.to_string(),
            scheme: scheme.to_string(),
            host: host.to_string(),
        })
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    pub fn host(&self) -> &str {
        &self.host
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FileSize(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Speed(pub f64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DownloadState {
    Queued,
    Downloading,
    Paused,
    Waiting,
    Retry,
    Error,
    Extracting,
    Completed,
    Checking,
}

impl std::str::FromStr for DownloadState {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Queued" => Ok(DownloadState::Queued),
            "Downloading" => Ok(DownloadState::Downloading),
            "Paused" => Ok(DownloadState::Paused),
            "Waiting" => Ok(DownloadState::Waiting),
            "Retry" => Ok(DownloadState::Retry),
            "Error" => Ok(DownloadState::Error),
            "Extracting" => Ok(DownloadState::Extracting),
            "Completed" => Ok(DownloadState::Completed),
            "Checking" => Ok(DownloadState::Checking),
            _ => Err(DomainError::ValidationError(format!(
                "Unknown download state: {s}"
            ))),
        }
    }
}

impl std::fmt::Display for DownloadState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DownloadState::Queued => write!(f, "Queued"),
            DownloadState::Downloading => write!(f, "Downloading"),
            DownloadState::Paused => write!(f, "Paused"),
            DownloadState::Waiting => write!(f, "Waiting"),
            DownloadState::Retry => write!(f, "Retry"),
            DownloadState::Error => write!(f, "Error"),
            DownloadState::Extracting => write!(f, "Extracting"),
            DownloadState::Completed => write!(f, "Completed"),
            DownloadState::Checking => write!(f, "Checking"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Download {
    id: DownloadId,
    url: Url,
    file_name: String,
    file_size: Option<FileSize>,
    downloaded_bytes: u64,
    state: DownloadState,
    priority: Priority,
    queue_position: i64,
    retry_count: u32,
    max_retries: u32,
    segments_count: u32,
    checksum_expected: Option<String>,
    checksum_computed: Option<String>,
    checksum_algorithm: Option<ChecksumAlgorithm>,
    source_hostname: String,
    protocol: String,
    resume_supported: bool,
    module_name: Option<String>,
    account_id: Option<u64>,
    destination_path: String,
    /// Alternative source URLs (Metalink mirrors). Sorted by priority
    /// descending; the engine starts with `mirrors[current_mirror_index]`
    /// and falls back to the next entry on persistent fetch failure.
    /// Empty when the download has a single source — in that case
    /// [`Download::active_url`] returns the canonical `url` field.
    mirrors: Vec<Mirror>,
    /// Index into `mirrors` of the candidate currently in flight. Always
    /// `0` when `mirrors` is empty. Bumped by [`Download::advance_mirror`]
    /// on adapter-level failover.
    current_mirror_index: u32,
    created_at: u64,
    updated_at: u64,
}

impl Download {
    pub fn new(id: DownloadId, url: Url, file_name: String, destination_path: String) -> Self {
        let protocol = url.scheme().to_string();
        let source_hostname = url.host().to_string();

        Download {
            id,
            url,
            file_name,
            file_size: None,
            downloaded_bytes: 0,
            state: DownloadState::Queued,
            priority: Priority::default(),
            queue_position: 0,
            retry_count: 0,
            max_retries: 5,
            segments_count: 1,
            checksum_expected: None,
            checksum_computed: None,
            checksum_algorithm: None,
            source_hostname,
            protocol,
            resume_supported: false,
            module_name: None,
            account_id: None,
            destination_path,
            mirrors: Vec::new(),
            current_mirror_index: 0,
            created_at: 0,
            updated_at: 0,
        }
    }

    /// Reconstruct a Download from persistence storage.
    ///
    /// Bypasses state machine validation because the data was validated
    /// when first created and is assumed to be consistent.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn reconstruct(
        id: DownloadId,
        url: Url,
        file_name: String,
        file_size: Option<FileSize>,
        downloaded_bytes: u64,
        state: DownloadState,
        priority: Priority,
        queue_position: i64,
        retry_count: u32,
        max_retries: u32,
        segments_count: u32,
        checksum_expected: Option<String>,
        checksum_computed: Option<String>,
        checksum_algorithm: Option<ChecksumAlgorithm>,
        source_hostname: String,
        protocol: String,
        resume_supported: bool,
        module_name: Option<String>,
        account_id: Option<u64>,
        destination_path: String,
        mirrors: Vec<Mirror>,
        current_mirror_index: u32,
        created_at: u64,
        updated_at: u64,
    ) -> Self {
        // Clamp the persisted index to a valid slot — a corrupted DB row
        // pointing past the end of the list must not panic the engine.
        let clamped_index = if mirrors.is_empty() {
            0
        } else {
            current_mirror_index.min((mirrors.len() - 1) as u32)
        };
        Download {
            id,
            url,
            file_name,
            file_size,
            downloaded_bytes,
            state,
            priority,
            queue_position,
            retry_count,
            max_retries,
            segments_count,
            checksum_expected,
            checksum_computed,
            checksum_algorithm,
            source_hostname,
            protocol,
            resume_supported,
            module_name,
            account_id,
            destination_path,
            mirrors,
            current_mirror_index: clamped_index,
            created_at,
            updated_at,
        }
    }

    pub fn with_max_retries(mut self, n: u32) -> Self {
        self.max_retries = n;
        self
    }

    pub fn with_priority(mut self, p: Priority) -> Self {
        self.priority = p;
        self
    }

    pub fn with_queue_position(mut self, position: i64) -> Self {
        self.queue_position = position;
        self
    }

    pub fn set_queue_position(&mut self, position: i64) {
        self.queue_position = position;
    }

    pub fn with_source_hostname(mut self, hostname: String) -> Self {
        self.source_hostname = hostname;
        self
    }

    pub fn with_segments_count(mut self, n: u32) -> Self {
        self.segments_count = n;
        self
    }

    pub fn with_module_name(mut self, name: String) -> Self {
        self.module_name = Some(name);
        self
    }

    pub fn with_account_id(mut self, id: u64) -> Self {
        self.account_id = Some(id);
        self
    }

    pub fn with_created_at(mut self, ts: u64) -> Self {
        self.created_at = ts;
        self.updated_at = ts;
        self
    }

    /// Replace the absolute filesystem destination path.
    ///
    /// Used by `ChangeDirectoryCommand` after the underlying file has been
    /// successfully relocated. Callers are responsible for ensuring the path
    /// points to the new on-disk location before persisting.
    pub fn with_destination_path(mut self, path: String) -> Self {
        self.destination_path = path;
        self
    }

    pub fn set_destination_path(&mut self, path: String) {
        self.destination_path = path;
    }

    /// Replace the mirror list. The list is sorted highest-priority first
    /// and the cursor is reset to slot 0 so the next [`Download::active_url`]
    /// resolves to the best available source. Pass an empty `Vec` to clear
    /// the alternatives and fall back to the canonical `url`.
    pub fn with_mirrors(mut self, mirrors: Vec<Mirror>) -> Self {
        self.set_mirrors(mirrors);
        self
    }

    /// In-place equivalent of [`Download::with_mirrors`].
    pub fn set_mirrors(&mut self, mut mirrors: Vec<Mirror>) {
        sort_mirrors_by_priority(&mut mirrors);
        self.mirrors = mirrors;
        self.current_mirror_index = 0;
    }

    /// Move the cursor to the next mirror. Returns the freshly selected
    /// mirror's URL on success, or [`DomainError::NotFound`] when every
    /// mirror has already been visited (the engine raises
    /// `DownloadFailed` in that case).
    pub fn advance_mirror(&mut self) -> Result<&Url, DomainError> {
        if self.mirrors.is_empty() {
            return Err(DomainError::NotFound("no mirrors configured".to_string()));
        }
        let next = self.current_mirror_index.saturating_add(1);
        if next as usize >= self.mirrors.len() {
            return Err(DomainError::NotFound("all mirrors exhausted".to_string()));
        }
        self.current_mirror_index = next;
        Ok(self.mirrors[next as usize].url())
    }

    /// Reset the cursor back to the highest-priority mirror. Called by
    /// the manual-retry path so a user-driven retry restarts from the
    /// best candidate, matching the behaviour of the automatic-retry
    /// circuit-breaker reset.
    pub fn reset_mirror_cursor(&mut self) {
        self.current_mirror_index = 0;
    }

    /// Sorted alternative sources. Empty when the download has no Metalink
    /// metadata attached.
    pub fn mirrors(&self) -> &[Mirror] {
        &self.mirrors
    }

    /// Index into [`Download::mirrors`] of the candidate currently in use.
    /// Always `0` when no mirrors are configured.
    pub fn current_mirror_index(&self) -> u32 {
        self.current_mirror_index
    }

    /// URL the engine should hit for the next fetch. Resolves to
    /// `mirrors[current_mirror_index]` when a mirror list is configured;
    /// falls back to the canonical [`Download::url`] otherwise so single-
    /// source downloads keep their pre-existing behaviour.
    pub fn active_url(&self) -> &Url {
        match self.mirrors.get(self.current_mirror_index as usize) {
            Some(mirror) => mirror.url(),
            None => &self.url,
        }
    }

    pub fn touch(&mut self, now: u64) {
        self.updated_at = now;
    }

    pub fn update_progress(&mut self, downloaded_bytes: u64) {
        self.downloaded_bytes = downloaded_bytes;
    }

    pub fn set_file_size(&mut self, bytes: u64) {
        self.file_size = Some(FileSize(bytes));
        // Also marks download as 100% complete (file already exists locally).
        self.downloaded_bytes = bytes;
    }

    // --- Getters ---

    pub fn id(&self) -> DownloadId {
        self.id
    }

    pub fn url(&self) -> &Url {
        &self.url
    }

    pub fn state(&self) -> DownloadState {
        self.state
    }

    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    pub fn file_size(&self) -> Option<FileSize> {
        self.file_size
    }

    pub fn downloaded_bytes(&self) -> u64 {
        self.downloaded_bytes
    }

    pub fn priority(&self) -> &Priority {
        &self.priority
    }

    pub fn queue_position(&self) -> i64 {
        self.queue_position
    }

    pub fn retry_count(&self) -> u32 {
        self.retry_count
    }

    pub fn max_retries(&self) -> u32 {
        self.max_retries
    }

    pub fn segments_count(&self) -> u32 {
        self.segments_count
    }

    pub fn checksum_expected(&self) -> Option<&str> {
        self.checksum_expected.as_deref()
    }

    pub fn checksum_computed(&self) -> Option<&str> {
        self.checksum_computed.as_deref()
    }

    pub fn checksum_algorithm(&self) -> Option<ChecksumAlgorithm> {
        self.checksum_algorithm
    }

    /// Set the expected checksum (used when the upstream resolver knows it).
    /// The algorithm is detected from the hash format and stored alongside.
    /// Returns `Err(UnsupportedChecksumFormat)` when the format cannot be
    /// recognised so callers can either skip validation or surface the error.
    pub fn with_expected_checksum(mut self, value: String) -> Result<Self, DomainError> {
        let algo = ChecksumAlgorithm::detect_from_hex(&value)
            .ok_or_else(|| DomainError::UnsupportedChecksumFormat(value.clone()))?;
        self.checksum_expected = Some(value);
        self.checksum_algorithm = Some(algo);
        Ok(self)
    }

    pub fn source_hostname(&self) -> &str {
        &self.source_hostname
    }

    pub fn protocol(&self) -> &str {
        &self.protocol
    }

    pub fn resume_supported(&self) -> bool {
        self.resume_supported
    }

    pub fn module_name(&self) -> Option<&str> {
        self.module_name.as_deref()
    }

    pub fn account_id(&self) -> Option<u64> {
        self.account_id
    }

    pub fn destination_path(&self) -> &str {
        &self.destination_path
    }

    pub fn created_at(&self) -> u64 {
        self.created_at
    }

    pub fn updated_at(&self) -> u64 {
        self.updated_at
    }

    pub fn progress_percentage(&self) -> u64 {
        match self.file_size {
            Some(FileSize(total)) if total > 0 => {
                ((self.downloaded_bytes as f64 / total as f64 * 100.0) as u64).min(100)
            }
            _ => 0,
        }
    }

    // --- State machine ---

    pub fn start(&mut self) -> Result<DomainEvent, DomainError> {
        match self.state {
            DownloadState::Queued => {
                self.state = DownloadState::Downloading;
                Ok(DomainEvent::DownloadStarted { id: self.id })
            }
            DownloadState::Retry => {
                self.state = DownloadState::Downloading;
                Ok(DomainEvent::DownloadStarted { id: self.id })
            }
            _ => Err(DomainError::InvalidTransition {
                from: self.state,
                to: DownloadState::Downloading,
            }),
        }
    }

    pub fn pause(&mut self) -> Result<DomainEvent, DomainError> {
        if self.state != DownloadState::Downloading {
            return Err(DomainError::InvalidTransition {
                from: self.state,
                to: DownloadState::Paused,
            });
        }
        self.state = DownloadState::Paused;
        Ok(DomainEvent::DownloadPaused { id: self.id })
    }

    pub fn resume(&mut self) -> Result<DomainEvent, DomainError> {
        if self.state != DownloadState::Paused {
            return Err(DomainError::InvalidTransition {
                from: self.state,
                to: DownloadState::Downloading,
            });
        }
        self.state = DownloadState::Downloading;
        Ok(DomainEvent::DownloadResumed { id: self.id })
    }

    pub fn complete(&mut self) -> Result<DomainEvent, DomainError> {
        match self.state {
            DownloadState::Downloading | DownloadState::Checking | DownloadState::Extracting => {
                self.state = DownloadState::Completed;
                Ok(DomainEvent::DownloadCompleted { id: self.id })
            }
            _ => Err(DomainError::InvalidTransition {
                from: self.state,
                to: DownloadState::Completed,
            }),
        }
    }

    pub fn retry(&mut self) -> Result<DomainEvent, DomainError> {
        if self.state != DownloadState::Error {
            return Err(DomainError::InvalidTransition {
                from: self.state,
                to: DownloadState::Retry,
            });
        }

        if self.retry_count >= self.max_retries {
            self.state = DownloadState::Error;
            return Err(DomainError::MaxRetriesExceeded {
                download_id: self.id.0,
            });
        }

        self.retry_count += 1;
        self.state = DownloadState::Retry;
        Ok(DomainEvent::DownloadRetrying {
            id: self.id,
            attempt: self.retry_count,
        })
    }

    pub fn retry_manually(&mut self) -> Result<DomainEvent, DomainError> {
        if self.state != DownloadState::Error {
            return Err(DomainError::InvalidTransition {
                from: self.state,
                to: DownloadState::Retry,
            });
        }

        // A manual retry is an explicit user action, so it reopens the
        // automatic retry circuit breaker and starts a fresh retry cycle.
        self.retry_count = 1;
        self.state = DownloadState::Retry;
        Ok(DomainEvent::DownloadRetrying {
            id: self.id,
            attempt: self.retry_count,
        })
    }

    pub fn fail(&mut self, error: String) -> Result<DomainEvent, DomainError> {
        match self.state {
            DownloadState::Downloading
            | DownloadState::Waiting
            | DownloadState::Checking
            | DownloadState::Extracting => {
                self.state = DownloadState::Error;
                Ok(DomainEvent::DownloadFailed { id: self.id, error })
            }
            _ => Err(DomainError::InvalidTransition {
                from: self.state,
                to: DownloadState::Error,
            }),
        }
    }

    pub fn wait(&mut self) -> Result<DomainEvent, DomainError> {
        if self.state != DownloadState::Downloading {
            return Err(DomainError::InvalidTransition {
                from: self.state,
                to: DownloadState::Waiting,
            });
        }
        self.state = DownloadState::Waiting;
        Ok(DomainEvent::DownloadWaiting { id: self.id })
    }

    pub fn resume_from_wait(&mut self) -> Result<DomainEvent, DomainError> {
        if self.state != DownloadState::Waiting {
            return Err(DomainError::InvalidTransition {
                from: self.state,
                to: DownloadState::Downloading,
            });
        }
        self.state = DownloadState::Downloading;
        Ok(DomainEvent::DownloadResumedFromWait { id: self.id })
    }

    pub fn start_checking(&mut self) -> Result<DomainEvent, DomainError> {
        if self.state != DownloadState::Downloading {
            return Err(DomainError::InvalidTransition {
                from: self.state,
                to: DownloadState::Checking,
            });
        }
        self.state = DownloadState::Checking;
        Ok(DomainEvent::DownloadChecking { id: self.id })
    }

    /// Re-enter `Checking` from `Completed` for a manual verify-checksum
    /// request. Distinct from `start_checking` (which only walks
    /// `Downloading → Checking`) so the post-download flow stays disciplined
    /// while still allowing on-demand re-verification.
    pub fn start_checking_from_completed(&mut self) -> Result<DomainEvent, DomainError> {
        if self.state != DownloadState::Completed {
            return Err(DomainError::InvalidTransition {
                from: self.state,
                to: DownloadState::Checking,
            });
        }
        self.state = DownloadState::Checking;
        Ok(DomainEvent::DownloadChecking { id: self.id })
    }

    /// Record a successful checksum verification and transition to `Completed`.
    ///
    /// Stores the computed value + algorithm so the detail panel can compare
    /// against `checksum_expected`. Caller must already be in `Checking`.
    pub fn record_checksum_match(
        &mut self,
        algorithm: ChecksumAlgorithm,
        computed: String,
    ) -> Result<DomainEvent, DomainError> {
        if self.state != DownloadState::Checking {
            return Err(DomainError::InvalidTransition {
                from: self.state,
                to: DownloadState::Completed,
            });
        }
        self.checksum_algorithm = Some(algorithm);
        self.checksum_computed = Some(computed.clone());
        self.state = DownloadState::Completed;
        Ok(DomainEvent::ChecksumVerified {
            id: self.id,
            algorithm: algorithm.to_string(),
            checksum: computed,
        })
    }

    /// Record a checksum mismatch and transition to `Error`. Persists the
    /// computed value alongside the expected one for forensic display.
    pub fn record_checksum_mismatch(
        &mut self,
        algorithm: ChecksumAlgorithm,
        expected: String,
        computed: String,
    ) -> Result<DomainEvent, DomainError> {
        if self.state != DownloadState::Checking {
            return Err(DomainError::InvalidTransition {
                from: self.state,
                to: DownloadState::Error,
            });
        }
        self.checksum_algorithm = Some(algorithm);
        self.checksum_computed = Some(computed.clone());
        self.state = DownloadState::Error;
        Ok(DomainEvent::ChecksumMismatch {
            id: self.id,
            algorithm: algorithm.to_string(),
            expected,
            computed,
        })
    }

    pub fn start_extracting(&mut self) -> Result<DomainEvent, DomainError> {
        match self.state {
            DownloadState::Downloading | DownloadState::Checking | DownloadState::Completed => {
                self.state = DownloadState::Extracting;
                Ok(DomainEvent::DownloadExtracting { id: self.id })
            }
            _ => Err(DomainError::InvalidTransition {
                from: self.state,
                to: DownloadState::Extracting,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_download() -> Download {
        let id = DownloadId(1);
        let url = Url::new("https://example.com/file.zip").unwrap();
        Download::new(id, url, "file.zip".to_string(), "/tmp".to_string())
    }

    #[test]
    fn test_download_new_starts_queued() {
        let d = make_download();
        assert_eq!(d.state(), DownloadState::Queued);
        assert_eq!(d.retry_count(), 0);
        assert_eq!(d.priority(), &Priority::default());
        assert_eq!(d.max_retries(), 5);
    }

    #[test]
    fn test_download_start_from_queued_succeeds() {
        let mut d = make_download();
        let event = d.start().unwrap();
        assert_eq!(d.state(), DownloadState::Downloading);
        assert_eq!(event, DomainEvent::DownloadStarted { id: DownloadId(1) });
    }

    #[test]
    fn test_download_pause_from_downloading_succeeds() {
        let mut d = make_download();
        d.start().unwrap();
        let event = d.pause().unwrap();
        assert_eq!(d.state(), DownloadState::Paused);
        assert_eq!(event, DomainEvent::DownloadPaused { id: DownloadId(1) });
    }

    #[test]
    fn test_download_pause_from_completed_fails() {
        let mut d = make_download();
        d.start().unwrap();
        d.complete().unwrap();
        let result = d.pause();
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            DomainError::InvalidTransition {
                from: DownloadState::Completed,
                to: DownloadState::Paused
            }
        );
    }

    #[test]
    fn test_download_resume_from_paused_succeeds() {
        let mut d = make_download();
        d.start().unwrap();
        d.pause().unwrap();
        let event = d.resume().unwrap();
        assert_eq!(d.state(), DownloadState::Downloading);
        assert_eq!(event, DomainEvent::DownloadResumed { id: DownloadId(1) });
    }

    #[test]
    fn test_download_retry_increments_count() {
        let mut d = make_download();
        d.start().unwrap();
        d.fail("network error".to_string()).unwrap();
        let event = d.retry().unwrap();
        assert_eq!(d.retry_count(), 1);
        assert_eq!(d.state(), DownloadState::Retry);
        assert_eq!(
            event,
            DomainEvent::DownloadRetrying {
                id: DownloadId(1),
                attempt: 1
            }
        );
    }

    #[test]
    fn test_download_retry_circuit_breaker_after_max() {
        let mut d = make_download().with_max_retries(2);
        d.start().unwrap();
        d.fail("error".to_string()).unwrap();
        // cycle 1: Error → retry → Retry → start → Downloading → fail → Error
        d.retry().unwrap(); // attempt 1
        d.start().unwrap();
        d.fail("error".to_string()).unwrap();
        // cycle 2: Error → retry → Retry → start → Downloading → fail → Error
        d.retry().unwrap(); // attempt 2
        d.start().unwrap();
        d.fail("error".to_string()).unwrap();
        // cycle 3: count (2) >= max (2) → MaxRetriesExceeded
        let result = d.retry();
        assert_eq!(
            result.unwrap_err(),
            DomainError::MaxRetriesExceeded { download_id: 1 }
        );
        assert_eq!(d.state(), DownloadState::Error);
    }

    #[test]
    fn test_download_manual_retry_reopens_circuit_breaker() {
        let mut d = make_download().with_max_retries(1);
        d.start().unwrap();
        d.fail("error".to_string()).unwrap();
        d.retry().unwrap();
        d.start().unwrap();
        d.fail("error".to_string()).unwrap();

        let event = d.retry_manually().unwrap();

        assert_eq!(d.retry_count(), 1);
        assert_eq!(d.state(), DownloadState::Retry);
        assert_eq!(
            event,
            DomainEvent::DownloadRetrying {
                id: DownloadId(1),
                attempt: 1
            }
        );
    }

    #[test]
    fn test_download_complete_from_downloading_succeeds() {
        let mut d = make_download();
        d.start().unwrap();
        let event = d.complete().unwrap();
        assert_eq!(d.state(), DownloadState::Completed);
        assert_eq!(event, DomainEvent::DownloadCompleted { id: DownloadId(1) });
    }

    #[test]
    fn test_download_state_all_valid_transitions() {
        // Queued -> Downloading -> Paused -> Downloading -> Completed
        let mut d = make_download();
        assert!(d.start().is_ok());
        assert!(d.pause().is_ok());
        assert!(d.resume().is_ok());
        assert!(d.complete().is_ok());

        // Downloading -> Waiting -> Downloading
        let mut d2 = make_download();
        d2.start().unwrap();
        assert!(d2.wait().is_ok());
        assert!(d2.resume_from_wait().is_ok());

        // Downloading -> Checking -> Completed
        let mut d3 = make_download();
        d3.start().unwrap();
        assert!(d3.start_checking().is_ok());
        assert!(d3.complete().is_ok());

        // Downloading -> Extracting -> Completed
        let mut d4 = make_download();
        d4.start().unwrap();
        assert!(d4.start_extracting().is_ok());
        assert!(d4.complete().is_ok());
    }

    #[test]
    fn test_download_state_all_invalid_transitions() {
        // Can't pause from Queued
        let mut d = make_download();
        assert!(d.pause().is_err());

        // Can't complete from Queued
        assert!(d.complete().is_err());

        // Can't resume from Queued
        assert!(d.resume().is_err());

        // Can't retry from Queued
        assert!(d.retry().is_err());
    }

    #[test]
    fn test_url_validation() {
        assert!(Url::new("https://example.com").is_ok());
        assert!(Url::new("http://example.com").is_ok());
        assert!(Url::new("ftp://example.com").is_ok());
        assert!(Url::new("").is_err());
        assert!(Url::new("ssh://example.com").is_err());
        assert!(Url::new("invalid").is_err());
    }

    #[test]
    fn test_file_size_ordering() {
        assert!(FileSize(100) < FileSize(200));
        assert!(FileSize(0) < FileSize(1));
        assert_eq!(FileSize(50), FileSize(50));
    }

    #[test]
    fn test_progress_percentage() {
        let mut d = make_download();
        // No file_size => 0
        assert_eq!(d.progress_percentage(), 0);

        d.file_size = Some(FileSize(200));
        d.update_progress(100);
        assert_eq!(d.progress_percentage(), 50);

        d.update_progress(200);
        assert_eq!(d.progress_percentage(), 100);
    }

    #[test]
    fn test_with_max_retries_builder() {
        let d = make_download().with_max_retries(10);
        assert_eq!(d.max_retries(), 10);
    }

    #[test]
    fn test_with_priority_builder() {
        let p = Priority::new(9).unwrap();
        let d = make_download().with_priority(p);
        assert_eq!(d.priority(), &Priority::new(9).unwrap());
    }

    #[test]
    fn test_queue_position_default_is_zero() {
        let d = make_download();
        assert_eq!(d.queue_position(), 0);
    }

    #[test]
    fn test_with_queue_position_builder() {
        let d = make_download().with_queue_position(42);
        assert_eq!(d.queue_position(), 42);
    }

    #[test]
    fn test_set_queue_position_mutator() {
        let mut d = make_download();
        d.set_queue_position(-3);
        assert_eq!(d.queue_position(), -3);
    }

    #[test]
    fn test_retry_to_downloading_transition() {
        let mut d = make_download();
        d.start().unwrap();
        d.fail("network error".to_string()).unwrap();
        d.retry().unwrap();
        assert_eq!(d.state(), DownloadState::Retry);
        let event = d.start().unwrap();
        assert_eq!(d.state(), DownloadState::Downloading);
        assert_eq!(event, DomainEvent::DownloadStarted { id: DownloadId(1) });
    }

    #[test]
    fn test_start_extracting_from_checking() {
        let mut d = make_download();
        d.start().unwrap();
        d.start_checking().unwrap();
        assert_eq!(d.state(), DownloadState::Checking);
        let event = d.start_extracting().unwrap();
        assert_eq!(d.state(), DownloadState::Extracting);
        assert_eq!(event, DomainEvent::DownloadExtracting { id: DownloadId(1) });
    }

    #[test]
    fn test_start_extracting_from_completed() {
        let mut d = make_download();
        d.start().unwrap();
        d.complete().unwrap();
        assert_eq!(d.state(), DownloadState::Completed);
        let event = d.start_extracting().unwrap();
        assert_eq!(d.state(), DownloadState::Extracting);
        assert_eq!(event, DomainEvent::DownloadExtracting { id: DownloadId(1) });
    }

    #[test]
    fn test_with_expected_checksum_detects_sha256() {
        let d = make_download()
            .with_expected_checksum(
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
            )
            .unwrap();
        assert_eq!(d.checksum_algorithm(), Some(ChecksumAlgorithm::Sha256));
        assert!(d.checksum_expected().is_some());
    }

    #[test]
    fn test_with_expected_checksum_detects_md5() {
        let d = make_download()
            .with_expected_checksum("d41d8cd98f00b204e9800998ecf8427e".to_string())
            .unwrap();
        assert_eq!(d.checksum_algorithm(), Some(ChecksumAlgorithm::Md5));
    }

    #[test]
    fn test_with_expected_checksum_rejects_unsupported() {
        let result = make_download().with_expected_checksum("abc".to_string());
        assert!(matches!(
            result,
            Err(DomainError::UnsupportedChecksumFormat(_))
        ));
    }

    #[test]
    fn test_record_checksum_match_transitions_checking_to_completed() {
        let mut d = make_download();
        d.start().unwrap();
        d.start_checking().unwrap();

        let event = d
            .record_checksum_match(
                ChecksumAlgorithm::Sha256,
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
            )
            .unwrap();

        assert_eq!(d.state(), DownloadState::Completed);
        assert_eq!(d.checksum_algorithm(), Some(ChecksumAlgorithm::Sha256));
        assert_eq!(
            d.checksum_computed(),
            Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
        );
        assert!(matches!(event, DomainEvent::ChecksumVerified { .. }));
    }

    #[test]
    fn test_record_checksum_match_outside_checking_fails() {
        let mut d = make_download();
        d.start().unwrap();
        let result = d.record_checksum_match(ChecksumAlgorithm::Sha256, "deadbeef".to_string());
        assert!(matches!(result, Err(DomainError::InvalidTransition { .. })));
    }

    #[test]
    fn test_record_checksum_mismatch_transitions_to_error_with_event() {
        let mut d = make_download()
            .with_expected_checksum("d41d8cd98f00b204e9800998ecf8427e".to_string())
            .unwrap();
        d.start().unwrap();
        d.start_checking().unwrap();

        let event = d
            .record_checksum_mismatch(
                ChecksumAlgorithm::Md5,
                "d41d8cd98f00b204e9800998ecf8427e".to_string(),
                "00000000000000000000000000000000".to_string(),
            )
            .unwrap();

        assert_eq!(d.state(), DownloadState::Error);
        assert_eq!(
            d.checksum_computed(),
            Some("00000000000000000000000000000000")
        );
        match event {
            DomainEvent::ChecksumMismatch {
                expected,
                computed,
                algorithm,
                ..
            } => {
                assert_eq!(expected, "d41d8cd98f00b204e9800998ecf8427e");
                assert_eq!(computed, "00000000000000000000000000000000");
                assert_eq!(algorithm, "MD5");
            }
            other => panic!("expected ChecksumMismatch, got {other:?}"),
        }
    }

    #[test]
    fn test_with_source_hostname_overrides_derived_hostname() {
        // Regression for YouTube downloads: the CDN URL hostname
        // (e.g. "rr1---sn-n4g-cvq6.googlevideo.com") should be overridable
        // so that "youtube.com" is stored instead.
        let d = make_download();
        assert_eq!(d.source_hostname(), "example.com");
        let d = d.with_source_hostname("youtube.com".to_string());
        assert_eq!(d.source_hostname(), "youtube.com");
    }

    #[test]
    fn test_with_segments_count_overrides_default() {
        let d = make_download();
        assert_eq!(d.segments_count(), 1);
        let d = d.with_segments_count(8);
        assert_eq!(d.segments_count(), 8);
    }

    #[test]
    fn test_with_module_name_stores_plugin_owner() {
        let d = make_download();
        assert_eq!(d.module_name(), None);
        let d = d.with_module_name("vortex-mod-youtube".to_string());
        assert_eq!(d.module_name(), Some("vortex-mod-youtube"));
    }

    #[test]
    fn test_with_account_id_stores_account_link() {
        let d = make_download();
        assert_eq!(d.account_id(), None);
        let d = d.with_account_id(42);
        assert_eq!(d.account_id(), Some(42));
    }

    fn mk_mirror(host: &str, priority: u8) -> Mirror {
        let url = Url::new(&format!("https://{host}/file.zip")).unwrap();
        Mirror::new(url, priority, None).unwrap()
    }

    #[test]
    fn test_with_mirrors_sorts_highest_priority_first() {
        let mirrors = vec![
            mk_mirror("low.example.com", 10),
            mk_mirror("high.example.com", 80),
            mk_mirror("mid.example.com", 50),
        ];
        let d = make_download().with_mirrors(mirrors);
        let active = d.active_url();
        assert_eq!(active.host(), "high.example.com");
    }

    #[test]
    fn test_active_url_falls_back_to_canonical_url_without_mirrors() {
        let d = make_download();
        assert_eq!(d.active_url().host(), "example.com");
    }

    #[test]
    fn test_advance_mirror_walks_through_priority_descending_list() {
        let mirrors = vec![
            mk_mirror("low.example.com", 10),
            mk_mirror("high.example.com", 80),
            mk_mirror("mid.example.com", 50),
        ];
        let mut d = make_download().with_mirrors(mirrors);
        assert_eq!(d.active_url().host(), "high.example.com");
        let next = d.advance_mirror().unwrap();
        assert_eq!(next.host(), "mid.example.com");
        let next = d.advance_mirror().unwrap();
        assert_eq!(next.host(), "low.example.com");
    }

    #[test]
    fn test_advance_mirror_returns_not_found_when_exhausted() {
        let mirrors = vec![
            mk_mirror("a.example.com", 50),
            mk_mirror("b.example.com", 30),
        ];
        let mut d = make_download().with_mirrors(mirrors);
        d.advance_mirror().unwrap();
        let err = d.advance_mirror().unwrap_err();
        assert!(matches!(err, DomainError::NotFound(_)));
    }

    #[test]
    fn test_advance_mirror_returns_not_found_when_no_mirrors_configured() {
        let mut d = make_download();
        let err = d.advance_mirror().unwrap_err();
        assert!(matches!(err, DomainError::NotFound(_)));
    }

    #[test]
    fn test_reset_mirror_cursor_returns_to_highest_priority() {
        let mirrors = vec![
            mk_mirror("low.example.com", 10),
            mk_mirror("high.example.com", 80),
        ];
        let mut d = make_download().with_mirrors(mirrors);
        d.advance_mirror().unwrap();
        assert_eq!(d.active_url().host(), "low.example.com");
        d.reset_mirror_cursor();
        assert_eq!(d.active_url().host(), "high.example.com");
    }

    #[test]
    fn test_mirrors_getter_returns_sorted_list() {
        let mirrors = vec![
            mk_mirror("low.example.com", 10),
            mk_mirror("high.example.com", 80),
        ];
        let d = make_download().with_mirrors(mirrors);
        assert_eq!(d.mirrors().len(), 2);
        assert_eq!(d.mirrors()[0].priority(), 80);
        assert_eq!(d.mirrors()[1].priority(), 10);
    }
}
