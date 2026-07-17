//! Handler for the `ResolveLinksCommand`.
//!
//! Checks each URL via plugin loader and HTTP HEAD, returning
//! resolution metadata for the frontend link grabber view.

use serde::Serialize;
use uuid::Uuid;

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::application::services::account_rotator::NextAccountOutcome;
use crate::application::services::download_source_policy::is_protected_plugin_category;
use crate::domain::error::DomainError;
use crate::domain::model::http::HttpResponse;
use crate::domain::ports::driven::ExtractedHosterLink;

use super::ResolveLinksCommand;

/// Resolution metadata for a single URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LinkResolutionErrorKind {
    InvalidUrl,
    NoFile,
    AuthenticationRequired,
    Expired,
    AccountUnavailable,
    Plugin,
    Network,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLinkDto {
    pub id: String,
    pub original_url: String,
    pub resolved_url: Option<String>,
    pub filename: Option<String>,
    pub size_bytes: Option<u64>,
    pub resumable: Option<bool>,
    /// "checking" | "online" | "offline" | "error"
    pub status: String,
    pub error_message: Option<String>,
    pub error_kind: Option<LinkResolutionErrorKind>,
    pub module_name: String,
    pub account_id: Option<String>,
    pub is_media: bool,
    pub media_type: Option<String>,
    /// Whether the frontend should ask the backend for a live HTTP probe.
    pub requires_online_probe: bool,
}

struct HosterResolution {
    stable_url: String,
    filename: String,
    size_bytes: Option<u64>,
    resumable: Option<bool>,
    account_id: Option<String>,
}

impl CommandBus {
    pub async fn handle_resolve_links(
        &self,
        cmd: ResolveLinksCommand,
    ) -> Result<Vec<ResolvedLinkDto>, AppError> {
        const MAX_URLS: usize = 500;
        const MAX_URL_BYTES: usize = 8 * 1024;
        if cmd.urls.len() > MAX_URLS {
            return Err(AppError::Validation(format!(
                "Too many URLs: {} (max {})",
                cmd.urls.len(),
                MAX_URLS
            )));
        }
        if cmd.urls.iter().any(|url| url.len() > MAX_URL_BYTES) {
            return Err(AppError::Validation(format!(
                "URL exceeds the {MAX_URL_BYTES}-byte limit"
            )));
        }

        let mut results = Vec::with_capacity(cmd.urls.len());

        // TODO(perf): resolve URLs concurrently with bounded parallelism.
        // Current sequential approach is acceptable for ≤500 URLs but should
        // use tokio::task::spawn_blocking + Semaphore for production workloads.
        for url in &cmd.urls {
            let id = Uuid::new_v4().to_string();

            if !is_allowed_scheme(url) {
                push_bounded_result(
                    &mut results,
                    ResolvedLinkDto {
                        id,
                        original_url: url.clone(),
                        resolved_url: None,
                        filename: None,
                        size_bytes: None,
                        resumable: None,
                        status: "error".to_string(),
                        error_message: Some("URL scheme not allowed".to_string()),
                        error_kind: Some(LinkResolutionErrorKind::InvalidUrl),
                        module_name: "core-http".to_string(),
                        account_id: None,
                        is_media: false,
                        media_type: None,
                        requires_online_probe: false,
                    },
                    MAX_URLS,
                )?;
                continue;
            }

            if url.to_lowercase().starts_with("magnet:") {
                push_bounded_result(
                    &mut results,
                    ResolvedLinkDto {
                        id,
                        original_url: url.clone(),
                        resolved_url: Some(url.clone()),
                        filename: None,
                        size_bytes: None,
                        resumable: None,
                        status: "online".to_string(),
                        error_message: None,
                        error_kind: None,
                        module_name: "magnet".to_string(),
                        account_id: None,
                        is_media: false,
                        media_type: None,
                        requires_online_probe: false,
                    },
                    MAX_URLS,
                )?;
                continue;
            }

            let plugin_info = self.plugin_loader().resolve_url(url);
            let module_name = match &plugin_info {
                Ok(Some(info)) => info.name().to_string(),
                _ => "core-http".to_string(),
            };

            let is_hoster = matches!(
                plugin_info.as_ref().ok().and_then(Option::as_ref),
                Some(info) if is_protected_plugin_category(info.category())
            );
            if is_hoster {
                match self.resolve_hoster_links(url, &module_name).await {
                    Ok(resolutions) => {
                        for resolved in resolutions {
                            push_bounded_result(
                                &mut results,
                                ResolvedLinkDto {
                                    id: Uuid::new_v4().to_string(),
                                    original_url: resolved.stable_url.clone(),
                                    resolved_url: Some(resolved.stable_url),
                                    filename: Some(resolved.filename),
                                    size_bytes: resolved.size_bytes,
                                    resumable: resolved.resumable,
                                    status: "online".to_string(),
                                    error_message: None,
                                    error_kind: None,
                                    module_name: module_name.clone(),
                                    account_id: resolved.account_id,
                                    is_media: false,
                                    media_type: None,
                                    requires_online_probe: false,
                                },
                                MAX_URLS,
                            )?;
                        }
                    }
                    Err(error) => {
                        tracing::debug!(module_name, "hoster link resolution failed");
                        let (error_kind, error_message) = hoster_error_details(&error);
                        push_bounded_result(
                            &mut results,
                            ResolvedLinkDto {
                                id,
                                original_url: url.clone(),
                                resolved_url: None,
                                filename: None,
                                size_bytes: None,
                                resumable: None,
                                status: "error".to_string(),
                                error_message: Some(error_message),
                                error_kind: Some(error_kind),
                                module_name,
                                account_id: None,
                                is_media: false,
                                media_type: None,
                                requires_online_probe: false,
                            },
                            MAX_URLS,
                        )?;
                    }
                }
                continue;
            }

            let is_media = is_media_url(url);
            let media_type = if is_media {
                detect_media_type(url)
            } else {
                None
            };

            match self.http_client().head(url) {
                Ok(response) if response.is_success() => {
                    let filename = extract_filename_from_url(url);
                    let size = extract_content_length(&response);
                    push_bounded_result(
                        &mut results,
                        ResolvedLinkDto {
                            id,
                            original_url: url.clone(),
                            resolved_url: Some(url.clone()),
                            filename,
                            size_bytes: size,
                            resumable: None,
                            status: "online".to_string(),
                            error_message: None,
                            error_kind: None,
                            module_name,
                            account_id: None,
                            is_media,
                            media_type,
                            requires_online_probe: true,
                        },
                        MAX_URLS,
                    )?;
                }
                Ok(_) => {
                    push_bounded_result(
                        &mut results,
                        ResolvedLinkDto {
                            id,
                            original_url: url.clone(),
                            resolved_url: None,
                            filename: None,
                            size_bytes: None,
                            resumable: None,
                            status: "offline".to_string(),
                            error_message: None,
                            error_kind: None,
                            module_name,
                            account_id: None,
                            is_media,
                            media_type,
                            requires_online_probe: true,
                        },
                        MAX_URLS,
                    )?;
                }
                Err(e) => {
                    tracing::debug!(error = %e, "link resolution failed");
                    push_bounded_result(
                        &mut results,
                        ResolvedLinkDto {
                            id,
                            original_url: url.clone(),
                            resolved_url: None,
                            filename: None,
                            size_bytes: None,
                            resumable: None,
                            status: "error".to_string(),
                            error_message: Some(sanitize_resolve_error(&e)),
                            error_kind: Some(LinkResolutionErrorKind::Network),
                            module_name,
                            account_id: None,
                            is_media,
                            media_type,
                            requires_online_probe: true,
                        },
                        MAX_URLS,
                    )?;
                }
            }
        }

        Ok(results)
    }

    async fn resolve_hoster_links(
        &self,
        url: &str,
        service_name: &str,
    ) -> Result<Vec<HosterResolution>, AppError> {
        if service_name == "vortex-mod-gofile" {
            validate_gofile_requested_origin(url)?;
        }
        if self.account_repo().is_some() {
            match self.next_hoster_account(service_name)? {
                NextAccountOutcome::Picked(account) => {
                    return Ok(vec![HosterResolution {
                        stable_url: url.to_string(),
                        filename: extract_filename_from_url(url)
                            .unwrap_or_else(|| "download".into()),
                        size_bytes: None,
                        resumable: None,
                        account_id: Some(account.id().as_str().to_string()),
                    }]);
                }
                NextAccountOutcome::AllExhausted { reason, .. } => {
                    return Err(reason.into_domain_error().into());
                }
                NextAccountOutcome::NoneAvailable => {}
            }
        }

        let links = self
            .plugin_loader()
            .extract_hoster_links(service_name, url, None)?;
        if links.is_empty() {
            return Err(DomainError::HosterNoFile.into());
        }
        links
            .into_iter()
            .map(|link| into_hoster_resolution(link, url, service_name))
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    fn next_hoster_account(&self, service_name: &str) -> Result<NextAccountOutcome, AppError> {
        let Some(rotator) = self.account_rotator() else {
            return Ok(match self.resolve_account_for(service_name)? {
                Some(account) => NextAccountOutcome::Picked(account),
                None => NextAccountOutcome::NoneAvailable,
            });
        };
        let strategy = self.config_store().get_config()?.account_selection_strategy;
        rotator.next_account(service_name, strategy)
    }
}

fn push_bounded_result(
    results: &mut Vec<ResolvedLinkDto>,
    result: ResolvedLinkDto,
    max_results: usize,
) -> Result<(), AppError> {
    if results.len() >= max_results {
        return Err(AppError::Validation(format!(
            "Resolved link output exceeds the {max_results}-item limit"
        )));
    }
    results.push(result);
    Ok(())
}

fn into_hoster_resolution(
    link: ExtractedHosterLink,
    requested_url: &str,
    service_name: &str,
) -> Result<HosterResolution, DomainError> {
    if link.source_url.trim().is_empty()
        || link
            .direct_url
            .as_deref()
            .is_none_or(|url| url.trim().is_empty())
    {
        return Err(DomainError::HosterNoFile);
    }
    if service_name == "vortex-mod-gofile" {
        validate_gofile_requested_origin(requested_url)?;
    }
    let stable_url = if link.source_url.trim() == requested_url {
        requested_url.to_string()
    } else if service_name == "vortex-mod-gofile" {
        validated_gofile_child_source(requested_url, &link.source_url)?
    } else {
        requested_url.to_string()
    };
    let filename = link
        .filename
        .filter(|name| !name.trim().is_empty())
        .or_else(|| extract_filename_from_url(&stable_url))
        .unwrap_or_else(|| "download".into());
    Ok(HosterResolution {
        stable_url,
        filename,
        size_bytes: link.size_bytes,
        resumable: link.resumable,
        account_id: None,
    })
}

fn validate_gofile_requested_origin(requested_url: &str) -> Result<(), DomainError> {
    let requested = reqwest::Url::parse(requested_url)
        .map_err(|_| DomainError::PluginError("hoster received an invalid source URL".into()))?;
    if !is_supported_gofile_origin(&requested, false)
        || !requested.username().is_empty()
        || requested.password().is_some()
    {
        return Err(DomainError::PluginError(
            "hoster received an unsafe source URL".into(),
        ));
    }
    Ok(())
}

fn validated_gofile_child_source(
    requested_url: &str,
    candidate_url: &str,
) -> Result<String, DomainError> {
    let requested = reqwest::Url::parse(requested_url)
        .map_err(|_| DomainError::PluginError("hoster returned an invalid source URL".into()))?;
    let candidate = reqwest::Url::parse(candidate_url.trim())
        .map_err(|_| DomainError::PluginError("hoster returned an invalid source URL".into()))?;
    let requested_segments = path_segments(&requested);
    let candidate_segments = path_segments(&candidate);
    let canonical_child = matches!(
        (requested_segments.as_slice(), candidate_segments.as_slice()),
        (["d", folder], ["d", candidate_folder, child])
            if folder == candidate_folder && is_gofile_id(folder, false) && is_gofile_id(child, true)
    );
    if !is_supported_gofile_origin(&requested, false)
        || !is_supported_gofile_origin(&candidate, true)
        || !candidate.username().is_empty()
        || candidate.password().is_some()
        || candidate.query().is_some()
        || candidate.fragment().is_some()
        || !canonical_child
    {
        return Err(DomainError::PluginError(
            "hoster returned an unsafe source URL".into(),
        ));
    }
    Ok(candidate.to_string())
}

fn is_supported_gofile_origin(url: &reqwest::Url, require_https: bool) -> bool {
    let scheme_allowed = if require_https {
        url.scheme() == "https"
    } else {
        matches!(url.scheme(), "http" | "https")
    };
    let default_port = match url.scheme() {
        "http" => url.port_or_known_default() == Some(80),
        "https" => url.port_or_known_default() == Some(443),
        _ => false,
    };
    scheme_allowed && default_port && matches!(url.host_str(), Some("gofile.io" | "www.gofile.io"))
}

fn path_segments(url: &reqwest::Url) -> Vec<&str> {
    url.path()
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn is_gofile_id(value: &str, allow_separator: bool) -> bool {
    value.len() >= 6
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || (allow_separator && matches!(byte, b'_' | b'-'))
        })
}

fn hoster_error_details(error: &AppError) -> (LinkResolutionErrorKind, String) {
    match error {
        AppError::Domain(DomainError::AccountInvalidCredentials) => (
            LinkResolutionErrorKind::AuthenticationRequired,
            "Account credentials were rejected".to_string(),
        ),
        AppError::Domain(DomainError::HosterAuthenticationRequired) => (
            LinkResolutionErrorKind::AuthenticationRequired,
            "Hoster authentication is required".to_string(),
        ),
        AppError::Domain(DomainError::AccountExpired) => (
            LinkResolutionErrorKind::Expired,
            "Account is expired".to_string(),
        ),
        AppError::Domain(DomainError::HosterDirectUrlExpired) => (
            LinkResolutionErrorKind::Expired,
            "The direct download URL has expired".to_string(),
        ),
        AppError::Domain(DomainError::HosterNoFile) => (
            LinkResolutionErrorKind::NoFile,
            "No downloadable file was found".to_string(),
        ),
        AppError::Domain(DomainError::AccountCooldown) => (
            LinkResolutionErrorKind::AccountUnavailable,
            "Account is temporarily rate-limited".to_string(),
        ),
        AppError::Domain(DomainError::AccountQuotaExceeded) => (
            LinkResolutionErrorKind::AccountUnavailable,
            "Account quota is exhausted".to_string(),
        ),
        AppError::Domain(DomainError::NetworkError(_)) => (
            LinkResolutionErrorKind::Network,
            "Could not reach the hoster".to_string(),
        ),
        _ => (
            LinkResolutionErrorKind::Plugin,
            "Could not resolve hoster link".to_string(),
        ),
    }
}

fn is_allowed_scheme(url: &str) -> bool {
    let lower = url.to_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("ftp://")
        || lower.starts_with("magnet:")
}

fn sanitize_resolve_error(_e: &crate::domain::DomainError) -> String {
    // All errors map to a generic user-facing message.
    // Add variant-specific messages here when needed.
    "Could not check link status".to_string()
}

fn extract_filename_from_url(url: &str) -> Option<String> {
    // Strip query string and fragment
    let path = url.split('?').next().unwrap_or(url);
    let path = path.split('#').next().unwrap_or(path);
    // Extract the path component after the scheme + authority (e.g. after "https://host")
    let path_only = if let Some(after_scheme) = path.find("://") {
        let after = &path[after_scheme + 3..];
        let slash = after.find('/')?;
        &after[slash + 1..]
    } else {
        path
    };
    let last = path_only.split('/').rfind(|s| !s.is_empty())?;
    Some(last.to_string())
}

fn extract_content_length(response: &HttpResponse) -> Option<u64> {
    response.content_length()
}

fn extract_host(url: &str) -> &str {
    let lower_url = url;
    let after_scheme = lower_url
        .strip_prefix("https://")
        .or_else(|| lower_url.strip_prefix("http://"))
        .or_else(|| lower_url.strip_prefix("ftp://"))
        .unwrap_or(lower_url);
    let host_and_port = after_scheme.split('/').next().unwrap_or("");
    host_and_port.split(':').next().unwrap_or("")
}

fn is_media_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    let host = extract_host(&lower);
    let media_hosts = [
        "youtube.com",
        "youtu.be",
        "vimeo.com",
        "soundcloud.com",
        "dailymotion.com",
        "twitch.tv",
        "tiktok.com",
    ];
    media_hosts
        .iter()
        .any(|&h| host == h || host.ends_with(&format!(".{h}")))
}

fn detect_media_type(url: &str) -> Option<String> {
    let lower = url.to_lowercase();
    let host = extract_host(&lower);
    if host == "soundcloud.com" || host.ends_with(".soundcloud.com") {
        Some("audio".to_string())
    } else if [
        "youtube.com",
        "youtu.be",
        "vimeo.com",
        "dailymotion.com",
        "twitch.tv",
        "tiktok.com",
    ]
    .iter()
    .any(|&h| host == h || host.ends_with(&format!(".{h}")))
    {
        Some("video".to_string())
    } else {
        None
    }
}

#[cfg(test)]
#[path = "resolve_links_hoster_tests.rs"]
mod hoster_tests;

#[cfg(test)]
#[path = "resolve_links_tests.rs"]
mod tests;
