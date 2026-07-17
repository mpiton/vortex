use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::domain::error::DomainError;
use crate::domain::model::download::Download;
use crate::domain::ports::driven::{DownloadSourceResolver, ResolutionCancellation};

use super::{SourcePolicy, allows_html_filename};

pub(super) type ResolvedClientFactory =
    fn(&reqwest::Url, &[(String, String)]) -> Result<reqwest::Client, DomainError>;

pub(super) struct PreparedSources {
    pub(super) urls: Vec<String>,
    pub(super) initial_index: usize,
    pub(super) client: reqwest::Client,
    pub(super) resume_url: String,
    pub(super) policy: SourcePolicy,
    pub(super) size_hint: Option<u64>,
    pub(super) resume_supported: Option<bool>,
    pub(super) resolved: bool,
}

pub(super) async fn prepare_sources(
    download: Download,
    resolver: Option<Arc<dyn DownloadSourceResolver>>,
    client: reqwest::Client,
    cancel_token: CancellationToken,
    resolution_cancellation: ResolutionCancellation,
    resolved_client_factory: ResolvedClientFactory,
) -> Result<PreparedSources, DomainError> {
    if cancel_token.is_cancelled() {
        return Err(DomainError::PluginError(
            "download source resolution cancelled".into(),
        ));
    }
    let resume_url = download.url().as_str().to_string();
    let allow_html = allows_html_filename(download.file_name());
    let size_hint = download.file_size().map(|size| size.0);
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
        let policy = if source.is_protected() {
            SourcePolicy::Protected {
                allow_html: source.filename().map_or(allow_html, allows_html_filename),
            }
        } else {
            SourcePolicy::Direct
        };
        let size_hint = source.size_bytes().or(size_hint);
        let resume_supported = source.resumable();
        let protected = source.is_protected();
        let request_url = source.request_url().to_string();
        let request_headers = source.request_headers().to_vec();
        let direct_client = client.clone();
        let mut safety_task = tokio::task::spawn_blocking(move || {
            let parsed = reqwest::Url::parse(&request_url)
                .map_err(|_| DomainError::NetworkError("plugin returned an invalid URL".into()))?;
            let client = if protected || !request_headers.is_empty() {
                resolved_client_factory(&parsed, &request_headers)?
            } else {
                direct_client
            };
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
            policy,
            size_hint,
            resume_supported,
            resolved: true,
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
        policy: SourcePolicy::Direct,
        size_hint,
        resume_supported: None,
        resolved: false,
    })
}
