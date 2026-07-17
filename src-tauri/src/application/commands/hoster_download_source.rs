//! Classifies persisted downloads and resolves protected hoster capabilities.

use super::ResolveHosterSourceHandler;
use crate::application::services::download_source_policy::classify_download_module;
use crate::domain::error::DomainError;
use crate::domain::model::download::Download;
use crate::domain::ports::driven::{
    DownloadSourceResolver, ResolutionCancellation, ResolvedDownloadSource,
};

impl DownloadSourceResolver for ResolveHosterSourceHandler {
    fn requires_resolution(&self, download: &Download) -> Result<bool, DomainError> {
        if download.account_id().is_some() {
            return Ok(true);
        }
        Ok(classify_download_module(self.plugins.as_ref(), download.module_name())?.is_protected())
    }

    fn resolve(&self, download: &Download) -> Result<ResolvedDownloadSource, DomainError> {
        self.resolve_source(download, &ResolutionCancellation::default())
    }

    fn resolve_cancellable(
        &self,
        download: &Download,
        cancellation: &ResolutionCancellation,
    ) -> Result<ResolvedDownloadSource, DomainError> {
        self.resolve_source(download, cancellation)
    }
}

impl ResolveHosterSourceHandler {
    fn resolve_source(
        &self,
        download: &Download,
        cancellation: &ResolutionCancellation,
    ) -> Result<ResolvedDownloadSource, DomainError> {
        if download.account_id().is_some() {
            return self.resolve_download(download, cancellation);
        }
        cancellation.ensure_active()?;
        let service_name = download.module_name().ok_or_else(|| {
            DomainError::ValidationError("hoster download has no plugin association".into())
        })?;
        let link = self
            .plugins
            .extract_hoster_link(service_name, download.url().as_str(), None)?;
        cancellation.ensure_active()?;
        let direct_url = link
            .direct_url
            .filter(|url| !url.trim().is_empty())
            .ok_or(DomainError::HosterNoFile)?;
        Ok(ResolvedDownloadSource::protected(direct_url)
            .with_request_headers(link.request_headers)
            .with_metadata(link.filename, link.size_bytes, link.resumable))
    }
}
