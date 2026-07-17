use std::sync::{Arc, Mutex};

use super::*;
use crate::application::commands::tests_support::{
    CapturingEventBus, FakeAccountCredentialStore, InMemoryAccountRepo,
    build_account_bus_with_plugin_loader,
};
use crate::domain::error::DomainError;
use crate::domain::model::credential::Credential;
use crate::domain::model::plugin::{PluginCategory, PluginInfo, PluginManifest};
use crate::domain::ports::driven::PluginLoader;

struct FreeHosterPluginLoader {
    services: Mutex<Vec<String>>,
}

impl FreeHosterPluginLoader {
    fn new() -> Self {
        Self {
            services: Mutex::new(Vec::new()),
        }
    }

    fn plugin_info(name: &str) -> PluginInfo {
        PluginInfo::new(
            name.to_string(),
            "1.0.0".into(),
            name.to_string(),
            "vortex".into(),
            PluginCategory::Hoster,
        )
    }

    fn service_for_url(url: &str) -> &'static str {
        if url.contains("mediafire.com") {
            "vortex-mod-mediafire"
        } else if url.contains("pixeldrain.com") {
            "vortex-mod-pixeldrain"
        } else {
            "vortex-mod-gofile"
        }
    }
}

impl PluginLoader for FreeHosterPluginLoader {
    fn load(&self, _: &PluginManifest) -> Result<(), DomainError> {
        Ok(())
    }

    fn unload(&self, _: &str) -> Result<(), DomainError> {
        Ok(())
    }

    fn resolve_url(&self, url: &str) -> Result<Option<PluginInfo>, DomainError> {
        Ok(Some(Self::plugin_info(Self::service_for_url(url))))
    }

    fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
        Ok([
            "vortex-mod-mediafire",
            "vortex-mod-pixeldrain",
            "vortex-mod-gofile",
        ]
        .into_iter()
        .map(Self::plugin_info)
        .collect())
    }

    fn set_enabled(&self, _: &str, _: bool) -> Result<(), DomainError> {
        Ok(())
    }

    fn extract_hoster_links(
        &self,
        service_name: &str,
        url: &str,
        credential: Option<&Credential>,
    ) -> Result<Vec<ExtractedHosterLink>, DomainError> {
        assert!(credential.is_none());
        self.services.lock().unwrap().push(service_name.to_string());
        if url.contains("missing") {
            return Err(DomainError::HosterNoFile);
        }
        if url.contains("empty") {
            return Ok(Vec::new());
        }
        if service_name == "vortex-mod-gofile" && url.contains("fanout500") {
            return Ok((0..500)
                .map(|index| ExtractedHosterLink {
                    source_url: format!("https://gofile.io/d/fanout500/file-{index}"),
                    filename: Some(format!("file-{index}.bin")),
                    size_bytes: Some(1),
                    direct_url: Some(format!("https://cdn.example/file-{index}?token=secret")),
                    resumable: Some(true),
                    request_headers: Vec::new(),
                    traffic_used_bytes: None,
                    traffic_total_bytes: None,
                })
                .collect());
        }
        let files = if service_name == "vortex-mod-gofile" && url.contains("single1") {
            vec![("file-a", "a.zip", 10_u64)]
        } else if service_name == "vortex-mod-gofile" {
            vec![("file-a", "a.zip", 10_u64), ("file-b", "b.zip", 20_u64)]
        } else {
            vec![("file", "archive.zip", 42_u64)]
        };
        Ok(files
            .into_iter()
            .map(|(id, filename, size_bytes)| ExtractedHosterLink {
                source_url: if url.contains("unsafe-source") || url.contains("unsafesource") {
                    "https://cdn.example/file?stable-token=leaked".into()
                } else if service_name == "vortex-mod-gofile" {
                    let folder = url
                        .split("/d/")
                        .nth(1)
                        .and_then(|path| path.split(['?', '#']).next())
                        .expect("test Gofile URL contains a folder");
                    format!("https://gofile.io/d/{folder}/{id}")
                } else {
                    url.to_string()
                },
                filename: if url.contains("unsafe-source") || url.contains("unsafesource") {
                    None
                } else {
                    Some(filename.into())
                },
                size_bytes: Some(size_bytes),
                direct_url: Some(format!("https://cdn.example/{id}?token=secret")),
                resumable: Some(true),
                request_headers: vec![("Referer".into(), url.into())],
                traffic_used_bytes: None,
                traffic_total_bytes: None,
            })
            .collect())
    }
}

async fn resolve_free_hoster(url: &str) -> (Vec<ResolvedLinkDto>, Arc<FreeHosterPluginLoader>) {
    let repo = Arc::new(InMemoryAccountRepo::new());
    let credentials = Arc::new(FakeAccountCredentialStore::new());
    let events = Arc::new(CapturingEventBus::new());
    let plugin = Arc::new(FreeHosterPluginLoader::new());
    let bus =
        build_account_bus_with_plugin_loader(repo, credentials, events, None, None, plugin.clone());
    let result = bus
        .handle_resolve_links(ResolveLinksCommand {
            urls: vec![url.into()],
        })
        .await
        .expect("free hoster resolution succeeds");
    (result, plugin)
}

#[tokio::test]
async fn free_mediafire_and_pixeldrain_resolution_preserves_download_metadata() {
    for (url, service) in [
        (
            "https://www.mediafire.com/file/abc/archive.zip/file",
            "vortex-mod-mediafire",
        ),
        ("https://pixeldrain.com/u/abc", "vortex-mod-pixeldrain"),
    ] {
        let (result, plugin) = resolve_free_hoster(url).await;

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].module_name, service);
        assert_eq!(result[0].filename.as_deref(), Some("archive.zip"));
        assert_eq!(result[0].size_bytes, Some(42));
        assert_eq!(result[0].resumable, Some(true));
        assert_eq!(result[0].resolved_url.as_deref(), Some(url));
        assert!(!result[0].requires_online_probe);
        assert_eq!(*plugin.services.lock().unwrap(), vec![service.to_string()]);
        assert!(
            !serde_json::to_string(&result)
                .unwrap()
                .contains("token=secret")
        );
    }
}

#[tokio::test]
async fn free_gofile_folder_expands_to_one_stable_row_per_file() {
    let folder = "https://gofile.io/d/folder";
    let (result, plugin) = resolve_free_hoster(folder).await;

    assert_eq!(result.len(), 2);
    assert_eq!(result[0].original_url, format!("{folder}/file-a"));
    assert_eq!(result[1].original_url, format!("{folder}/file-b"));
    assert_eq!(result[0].filename.as_deref(), Some("a.zip"));
    assert_eq!(result[1].size_bytes, Some(20));
    assert_eq!(
        plugin.services.lock().unwrap().as_slice(),
        ["vortex-mod-gofile"]
    );
    assert!(
        !serde_json::to_string(&result)
            .unwrap()
            .contains("token=secret")
    );
}

#[tokio::test]
async fn one_file_gofile_folder_keeps_its_stable_child_identity() {
    let folder = "https://gofile.io/d/single1";
    let (result, _) = resolve_free_hoster(folder).await;

    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0].resolved_url.as_deref(),
        Some("https://gofile.io/d/single1/file-a")
    );
}

#[tokio::test]
async fn gofile_child_identity_accepts_official_alias_and_https_canonicalization() {
    for folder in [
        "https://www.gofile.io/d/folder",
        "http://gofile.io/d/folder",
        "https://gofile.io/d/folder?foo=bar#section",
    ] {
        let (result, _) = resolve_free_hoster(folder).await;

        assert_eq!(result.len(), 2, "{folder}");
        assert_eq!(
            result[0].resolved_url.as_deref(),
            Some("https://gofile.io/d/folder/file-a"),
            "{folder}"
        );
    }
}

#[tokio::test]
async fn gofile_child_identity_rejects_unsafe_requested_origins() {
    for folder in [
        "https://user:pass@gofile.io/d/folder",
        "https://gofile.io:444/d/folder",
    ] {
        let (result, plugin) = resolve_free_hoster(folder).await;

        assert_eq!(result.len(), 1, "{folder}");
        assert_eq!(result[0].status, "error", "{folder}");
        assert_eq!(
            result[0].error_kind,
            Some(LinkResolutionErrorKind::Plugin),
            "{folder}"
        );
        assert!(result[0].resolved_url.is_none(), "{folder}");
        assert!(
            plugin.services.lock().unwrap().is_empty(),
            "unsafe Gofile origins must be rejected before plugin invocation"
        );
    }
}

#[tokio::test]
async fn empty_hoster_result_is_reported_as_no_file() {
    let (result, _) = resolve_free_hoster("https://gofile.io/d/empty-folder").await;

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].status, "error");
    assert_eq!(result[0].error_kind, Some(LinkResolutionErrorKind::NoFile));
}

#[tokio::test]
async fn typed_no_file_error_is_returned_to_link_grabber() {
    let (result, _) = resolve_free_hoster("https://gofile.io/d/missing").await;

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].status, "error");
    assert_eq!(result[0].error_kind, Some(LinkResolutionErrorKind::NoFile));
    assert_eq!(
        result[0].error_message.as_deref(),
        Some("No downloadable file was found")
    );
    assert!(!result[0].requires_online_probe);
}

#[tokio::test]
async fn single_hoster_keeps_the_requested_url_and_synthesizes_a_safe_filename() {
    let requested = "https://www.mediafire.com/file/unsafe-source";
    let (result, _) = resolve_free_hoster(requested).await;

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].original_url, requested);
    assert_eq!(result[0].resolved_url.as_deref(), Some(requested));
    assert!(
        result[0]
            .filename
            .as_deref()
            .is_some_and(|name| !name.is_empty())
    );
    assert!(
        !serde_json::to_string(&result)
            .unwrap()
            .contains("stable-token=leaked")
    );
}

#[tokio::test]
async fn multi_file_hoster_rejects_plugin_sources_outside_the_requested_origin() {
    let (result, _) = resolve_free_hoster("https://gofile.io/d/unsafesource").await;

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].status, "error");
    assert_eq!(result[0].error_kind, Some(LinkResolutionErrorKind::Plugin));
    assert!(
        !serde_json::to_string(&result)
            .unwrap()
            .contains("stable-token=leaked")
    );
}

#[tokio::test]
async fn resolved_output_limit_applies_to_rows_after_a_hoster_fanout() {
    let repo = Arc::new(InMemoryAccountRepo::new());
    let credentials = Arc::new(FakeAccountCredentialStore::new());
    let events = Arc::new(CapturingEventBus::new());
    let plugin = Arc::new(FreeHosterPluginLoader::new());
    let bus = build_account_bus_with_plugin_loader(repo, credentials, events, None, None, plugin);

    let result = bus
        .handle_resolve_links(ResolveLinksCommand {
            urls: vec![
                "https://gofile.io/d/fanout500".into(),
                "invalid-scheme://extra".into(),
            ],
        })
        .await;

    assert!(matches!(result, Err(AppError::Validation(_))));
}
