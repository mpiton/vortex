//! Classifies persisted downloads and resolves protected hoster capabilities.

use super::ResolveHosterSourceHandler;
use crate::application::services::download_source_policy::classify_download_module;
use crate::domain::error::DomainError;
use crate::domain::model::config::ResolutionTier;
use crate::domain::model::download::Download;
use crate::domain::model::plugin::PluginCategory;
use crate::domain::ports::driven::{
    DownloadSourceResolver, ExtractedHosterLink, ResolutionCancellation, ResolvedDownloadSource,
};

pub(super) fn resolved_protected_source(
    link: ExtractedHosterLink,
) -> Result<ResolvedDownloadSource, DomainError> {
    if let Some(captcha) = link.captcha {
        return Err(DomainError::CaptchaRequired {
            challenge_type: captcha.challenge_type,
            challenge_url: link.source_url,
            image_data: captcha.image_data,
        });
    }
    let direct_url = link
        .direct_url
        .filter(|url| !url.trim().is_empty())
        .ok_or(DomainError::HosterNoFile)?;
    Ok(ResolvedDownloadSource::protected(direct_url)
        .with_request_headers(link.request_headers)
        .with_metadata(link.filename, link.size_bytes, link.resumable))
}

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
            let error = match self.resolve_download(download, cancellation) {
                Ok(source) => return Ok(source),
                Err(error) => error,
            };
            if self.debrid_falls_through(download, cancellation) {
                return self.free_tier_source(download, error);
            }
            return Err(error);
        }
        cancellation.ensure_active()?;
        let service_name = download.module_name().ok_or_else(|| {
            DomainError::ValidationError("hoster download has no plugin association".into())
        })?;
        let link = self
            .plugins
            .extract_hoster_link(service_name, download.url().as_str(), None)?;
        cancellation.ensure_active()?;
        resolved_protected_source(link)
    }

    /// A debrid is one rung of the cascade, not the only route to the file.
    /// The link check picked it while it was healthy; by the time the engine
    /// asks for a URL the quota can be spent, the hoster dropped out of
    /// coverage, or the service down. R-04 wants the next rung tried with an
    /// explicit reason instead of a dead download.
    ///
    /// Only the free rung is retried. Premium is deliberately not: any
    /// premium account for the hoster was already offered this link at check
    /// time and lost to the debrid, so re-offering it replays a decision.
    /// A lookup that itself fails leaves the fall-through unproven, and then
    /// the debrid error stands.
    fn debrid_falls_through(
        &self,
        download: &Download,
        cancellation: &ResolutionCancellation,
    ) -> bool {
        let Some(module) = download.module_name() else {
            return false;
        };
        cancellation.ensure_active().is_ok()
            && self
                .config
                .get_config()
                .is_ok_and(|config| config.resolution_order.contains(&ResolutionTier::Free))
            && self.plugins.list_loaded().is_ok_and(|infos| {
                infos
                    .iter()
                    .any(|info| info.name() == module && info.category() == PluginCategory::Debrid)
            })
    }

    /// Anonymous extraction through the plugin that owns the URL, keeping the
    /// debrid's reason alongside the free one when neither rung delivers.
    fn free_tier_source(
        &self,
        download: &Download,
        debrid_error: DomainError,
    ) -> Result<ResolvedDownloadSource, DomainError> {
        let url = download.url().as_str();
        // `resolve_url` keeps debrid candidates and merely orders them last,
        // so the runner-up for a URL no hoster claims is another debrid.
        // Calling it without credentials is not a free extraction, and
        // reporting its refusal as "free" would name a rung never tried.
        let hoster = match self.plugins.resolve_url(url) {
            Ok(Some(info))
                if info.category() == PluginCategory::Hoster
                    && Some(info.name()) != download.module_name() =>
            {
                info.name().to_string()
            }
            _ => return Err(debrid_error),
        };
        self.plugins
            .extract_hoster_link(&hoster, url, None)
            .and_then(resolved_protected_source)
            .map_err(|free_error| match free_error {
                // The engine answers a challenge; burying it in a text
                // summary would strand the download instead.
                captcha @ DomainError::CaptchaRequired { .. } => captcha,
                free_error => DomainError::ResolutionExhausted(format!(
                    "debrid: {debrid_error}; free: {free_error}"
                )),
            })
    }
}

#[cfg(test)]
#[path = "hoster_download_source_tests.rs"]
mod tests;
