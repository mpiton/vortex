//! Resolves a persisted source URL into an ephemeral download capability.

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

/// Called by the download engine immediately before opening the connection.
/// Implementations must never persist or log the returned URL.
pub trait DownloadSourceResolver: Send + Sync {
    fn resolve(&self, download: &Download) -> Result<ResolvedDownloadSource, DomainError>;
}
