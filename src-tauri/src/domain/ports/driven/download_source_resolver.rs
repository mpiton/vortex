//! Resolves a persisted source URL into an ephemeral download capability.

use std::sync::{Arc, Mutex};

use crate::domain::error::DomainError;
use crate::domain::model::download::Download;

#[derive(Clone, PartialEq, Eq)]
pub struct ResolvedDownloadSource {
    request_url: String,
}

impl std::fmt::Debug for ResolvedDownloadSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ResolvedDownloadSource(<redacted>)")
    }
}

impl ResolvedDownloadSource {
    pub fn sensitive(request_url: String) -> Self {
        Self { request_url }
    }

    pub fn request_url(&self) -> &str {
        &self.request_url
    }
}

/// Linearizes cancellation with resolver-side persistence. The engine can
/// return promptly while a blocking plugin call is still unwinding, and the
/// resolver uses `run_if_active` around each later write so that detached work
/// cannot commit after cancellation won.
#[derive(Clone, Default)]
pub struct ResolutionCancellation {
    cancelled: Arc<Mutex<bool>>,
}

impl ResolutionCancellation {
    pub fn cancel(&self) {
        let mut cancelled = self
            .cancelled
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *cancelled = true;
    }

    pub fn is_cancelled(&self) -> bool {
        *self
            .cancelled
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn ensure_active(&self) -> Result<(), DomainError> {
        if self.is_cancelled() {
            Err(cancelled_error())
        } else {
            Ok(())
        }
    }

    pub fn run_if_active<T>(
        &self,
        operation: impl FnOnce() -> Result<T, DomainError>,
    ) -> Result<T, DomainError> {
        let cancelled = self.cancelled.lock().map_err(|_| {
            DomainError::PluginError("premium source cancellation state unavailable".into())
        })?;
        if *cancelled {
            return Err(cancelled_error());
        }
        operation()
    }
}

fn cancelled_error() -> DomainError {
    DomainError::PluginError("premium source resolution cancelled".into())
}

/// Called by the download engine immediately before opening the connection.
/// Implementations must never persist or log the returned URL.
pub trait DownloadSourceResolver: Send + Sync {
    fn resolve(&self, download: &Download) -> Result<ResolvedDownloadSource, DomainError>;

    fn resolve_cancellable(
        &self,
        download: &Download,
        cancellation: &ResolutionCancellation,
    ) -> Result<ResolvedDownloadSource, DomainError> {
        cancellation.ensure_active()?;
        let source = self.resolve(download)?;
        cancellation.ensure_active()?;
        Ok(source)
    }
}
