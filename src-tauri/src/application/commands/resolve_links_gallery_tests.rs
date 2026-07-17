use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::super::ResolveLinksCommand;
use super::super::resolve_links::{LinkResolutionErrorKind, ResolvedLinkDto};
use super::super::tests_support::build_download_bus_with_plugin_loader;
use crate::domain::error::DomainError;
use crate::domain::model::http::HttpResponse;
use crate::domain::model::plugin::{PluginCategory, PluginInfo, PluginManifest};
use crate::domain::ports::driven::{HttpClient, PluginLoader};

const GALLERY_URL: &str = "https://imgur.com/a/abc123";

struct OkHttpClient;

impl HttpClient for OkHttpClient {
    fn head(&self, _url: &str) -> Result<HttpResponse, DomainError> {
        Ok(HttpResponse {
            status_code: 200,
            headers: Default::default(),
            body: vec![],
        })
    }
    fn get_range(&self, _url: &str, _start: u64, _end: u64) -> Result<Vec<u8>, DomainError> {
        Ok(vec![])
    }
    fn supports_range(&self, _url: &str) -> Result<bool, DomainError> {
        Ok(false)
    }
}

struct CrawlerLoader {
    name: &'static str,
    /// `None` makes `extract_links` fail like a broken plugin.
    response: Option<&'static str>,
    extract_called: AtomicBool,
}

impl CrawlerLoader {
    fn new(name: &'static str, response: Option<&'static str>) -> Arc<Self> {
        Arc::new(Self {
            name,
            response,
            extract_called: AtomicBool::new(false),
        })
    }
}

impl PluginLoader for CrawlerLoader {
    fn load(&self, _: &PluginManifest) -> Result<(), DomainError> {
        Ok(())
    }
    fn unload(&self, _: &str) -> Result<(), DomainError> {
        Ok(())
    }
    fn resolve_url(&self, _: &str) -> Result<Option<PluginInfo>, DomainError> {
        Ok(Some(PluginInfo::new(
            self.name.into(),
            "1.1.0".into(),
            "gallery crawler".into(),
            "vortex".into(),
            PluginCategory::Crawler,
        )))
    }
    fn extract_links(&self, _: &str) -> Result<String, DomainError> {
        self.extract_called.store(true, Ordering::SeqCst);
        self.response
            .map(str::to_string)
            .ok_or_else(|| DomainError::PluginError("imgur API unreachable".into()))
    }
    fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
        Ok(vec![])
    }
    fn set_enabled(&self, _: &str, _: bool) -> Result<(), DomainError> {
        Ok(())
    }
}

async fn resolve_with(loader: Arc<CrawlerLoader>, url: &str) -> Vec<ResolvedLinkDto> {
    let (bus, _, _) = build_download_bus_with_plugin_loader(Arc::new(OkHttpClient), loader);
    bus.handle_resolve_links(ResolveLinksCommand {
        urls: vec![url.to_string()],
    })
    .await
    .expect("resolve succeeds")
}

#[tokio::test]
async fn test_resolve_gallery_url_expands_images_in_order_with_names_and_type() {
    let loader = CrawlerLoader::new(
        "vortex-mod-gallery",
        Some(
            r#"{"kind":"gallery","provider":"imgur","images":[
                {"url":"https://i.imgur.com/a.jpg","filename":"imgur_abc123_000.jpg"},
                {"url":"https://i.imgur.com/b.png"},
                {"url":"https://i.imgur.com/c.gif","filename":"imgur_abc123_002.gif"}
            ]}"#,
        ),
    );
    let rows = resolve_with(loader, GALLERY_URL).await;

    assert_eq!(rows.len(), 3);
    let urls: Vec<_> = rows.iter().map(|r| r.original_url.as_str()).collect();
    assert_eq!(
        urls,
        [
            "https://i.imgur.com/a.jpg",
            "https://i.imgur.com/b.png",
            "https://i.imgur.com/c.gif"
        ],
        "gallery order must be preserved"
    );
    assert_eq!(rows[0].filename.as_deref(), Some("imgur_abc123_000.jpg"));
    assert_eq!(
        rows[1].filename.as_deref(),
        Some("b.png"),
        "missing plugin filename falls back to the URL basename"
    );
    for row in &rows {
        assert_eq!(row.status, "online");
        assert_eq!(row.media_type.as_deref(), Some("image"));
        assert_eq!(row.module_name, "vortex-mod-gallery");
        assert!(!row.is_media, "image rows must not offer the media grabber");
        assert!(!row.requires_online_probe);
        assert_eq!(row.resolved_url.as_deref(), Some(row.original_url.as_str()));
    }
}

#[tokio::test]
async fn test_resolve_empty_gallery_reports_no_file_error() {
    let loader = CrawlerLoader::new(
        "vortex-mod-gallery",
        Some(r#"{"kind":"gallery","provider":"imgur","images":[]}"#),
    );
    let rows = resolve_with(loader, GALLERY_URL).await;

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, "error");
    assert_eq!(rows[0].error_kind, Some(LinkResolutionErrorKind::NoFile));
    assert_eq!(rows[0].original_url, GALLERY_URL);
}

#[tokio::test]
async fn test_resolve_partially_invalid_gallery_keeps_valid_images_and_flags_bad_one() {
    let loader = CrawlerLoader::new(
        "vortex-mod-gallery",
        Some(
            r#"{"kind":"gallery","provider":"imgur","images":[
                {"url":"https://i.imgur.com/a.jpg"},
                {"url":"","title":"broken slide"},
                {"url":"https://i.imgur.com/c.jpg"}
            ]}"#,
        ),
    );
    let rows = resolve_with(loader, GALLERY_URL).await;

    assert_eq!(rows.len(), 3, "invalid entries must not cancel the gallery");
    assert_eq!(rows[0].status, "online");
    assert_eq!(rows[2].status, "online");
    assert_eq!(rows[1].status, "error");
    assert_eq!(rows[1].error_kind, Some(LinkResolutionErrorKind::Plugin));
    assert!(
        rows[1]
            .error_message
            .as_deref()
            .unwrap_or_default()
            .contains("broken slide"),
        "error row should name the offending item"
    );
}

#[tokio::test]
async fn test_resolve_gallery_extraction_failure_reports_single_plugin_error() {
    let loader = CrawlerLoader::new("vortex-mod-gallery", None);
    let rows = resolve_with(loader.clone(), GALLERY_URL).await;

    assert!(loader.extract_called.load(Ordering::SeqCst));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, "error");
    assert_eq!(rows[0].error_kind, Some(LinkResolutionErrorKind::Plugin));
}

#[tokio::test]
async fn test_resolve_non_gallery_crawler_payload_falls_back_to_head_probe() {
    let loader = CrawlerLoader::new(
        "vortex-mod-future",
        Some(r#"{"kind":"article","items":[]}"#),
    );
    let rows = resolve_with(loader, "https://example.com/some/page").await;

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, "online");
    assert!(
        rows[0].requires_online_probe,
        "fallback path keeps the probe"
    );
    assert_eq!(rows[0].media_type, None);
}

#[tokio::test]
async fn test_resolve_media_host_url_never_calls_extract_links() {
    let loader = CrawlerLoader::new("vortex-mod-youtube", Some(r#"{"kind":"gallery"}"#));
    let rows = resolve_with(loader.clone(), "https://youtube.com/watch?v=xyz").await;

    assert!(
        !loader.extract_called.load(Ordering::SeqCst),
        "media-host crawlers (yt-dlp) must keep the cheap resolve path"
    );
    assert_eq!(rows.len(), 1);
    assert!(rows[0].is_media);
    assert_eq!(rows[0].media_type.as_deref(), Some("video"));
}
