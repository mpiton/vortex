use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::*;
use crate::application::commands::tests_support::{
    CapturingEventBus, FakeAccountCredentialStore, InMemoryAccountRepo,
    build_account_bus_with_plugin_loader,
};
use crate::application::services::{AccountRotator, AccountSelector};
use crate::domain::error::DomainError;
use crate::domain::model::account::{Account, AccountId, AccountStatus, AccountType};
use crate::domain::model::credential::Credential;
use crate::domain::model::plugin::{PluginCategory, PluginInfo, PluginManifest};
use crate::domain::ports::driven::{
    AccountCredentialStore, AccountRepository, Clock, PluginLoader,
};

struct FixedClock;

impl Clock for FixedClock {
    fn now_unix_secs(&self) -> u64 {
        1_700_000_000
    }
}

struct PremiumPluginLoader {
    credentials: Mutex<Vec<String>>,
    services: Mutex<Vec<String>>,
}

impl PremiumPluginLoader {
    fn new() -> Self {
        Self {
            credentials: Mutex::new(Vec::new()),
            services: Mutex::new(Vec::new()),
        }
    }

    fn plugin_info() -> PluginInfo {
        PluginInfo::new(
            "vortex-mod-1fichier".into(),
            "1.1.0".into(),
            "1fichier".into(),
            "vortex".into(),
            PluginCategory::Hoster,
        )
    }
}

impl PluginLoader for PremiumPluginLoader {
    fn load(&self, _: &PluginManifest) -> Result<(), DomainError> {
        Ok(())
    }

    fn unload(&self, _: &str) -> Result<(), DomainError> {
        Ok(())
    }

    fn resolve_url(&self, _: &str) -> Result<Option<PluginInfo>, DomainError> {
        Ok(Some(Self::plugin_info()))
    }

    fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
        Ok(vec![Self::plugin_info()])
    }

    fn set_enabled(&self, _: &str, _: bool) -> Result<(), DomainError> {
        Ok(())
    }

    fn extract_hoster_link(
        &self,
        service_name: &str,
        _: &str,
        credential: Option<&Credential>,
    ) -> Result<ExtractedHosterLink, DomainError> {
        self.services.lock().unwrap().push(service_name.to_string());
        let credential = credential.ok_or_else(|| {
            DomainError::NotFound("free hoster extraction is not configured".into())
        })?;
        self.credentials
            .lock()
            .unwrap()
            .push(credential.password().to_string());
        match credential.password() {
            "expired-key" => return Err(DomainError::AccountExpired),
            "invalid-key" => return Err(DomainError::AccountInvalidCredentials),
            "quota-key" => return Err(DomainError::AccountQuotaExceeded),
            "cooldown-key" => return Err(DomainError::AccountCooldown),
            _ => {}
        }
        Ok(ExtractedHosterLink {
            source_url: "https://1fichier.com/?abc123".into(),
            filename: Some("file.zip".into()),
            size_bytes: Some(42),
            direct_url: Some("https://download.1fichier.com/token/file.zip".into()),
            resumable: Some(true),
            request_headers: Vec::new(),
            traffic_used_bytes: Some(1),
            traffic_total_bytes: Some(100),
            captcha: None,
        })
    }
}

fn premium_account(id: &str, traffic_left: u64) -> Account {
    Account::reconstruct_with_status(
        AccountId::new(id),
        "vortex-mod-1fichier".into(),
        format!("user-{id}"),
        AccountType::Premium,
        true,
        Some(traffic_left),
        Some(100),
        Some(u64::MAX),
        Some(1),
        0,
        AccountStatus::Valid,
        None,
    )
}

async fn resolve_with_primary_credential(
    primary_password: Option<&str>,
    include_backup: bool,
    temporary_status: Option<AccountStatus>,
) -> (
    Vec<ResolvedLinkDto>,
    Arc<InMemoryAccountRepo>,
    Arc<PremiumPluginLoader>,
    Account,
) {
    let repo = Arc::new(InMemoryAccountRepo::new());
    let credentials = Arc::new(FakeAccountCredentialStore::new());
    let events = Arc::new(CapturingEventBus::new());
    let plugin = Arc::new(PremiumPluginLoader::new());
    let mut primary = premium_account("primary", 100);
    let backup = premium_account("backup", 50);
    match temporary_status {
        Some(AccountStatus::QuotaExhausted) => primary.mark_exhausted(1_700_000_060_000),
        Some(AccountStatus::Cooldown) => primary.mark_cooldown(1_700_000_060_000),
        _ => {}
    }
    repo.save(&primary).unwrap();
    if include_backup {
        repo.save(&backup).unwrap();
    }
    if let Some(password) = primary_password {
        credentials.store_password(primary.id(), password).unwrap();
    }
    if include_backup {
        credentials
            .store_password(backup.id(), "working-key")
            .unwrap();
    }
    let clock: Arc<dyn Clock> = Arc::new(FixedClock);
    let selector = AccountSelector::new(repo.clone(), events.clone(), clock.clone());
    let rotator = AccountRotator::new(selector.clone(), repo.clone(), events.clone(), clock);
    let bus = build_account_bus_with_plugin_loader(
        repo.clone(),
        credentials,
        events,
        None,
        None,
        plugin.clone(),
    )
    .with_account_selector(selector)
    .with_account_rotator(rotator);

    let result = bus
        .handle_resolve_links(ResolveLinksCommand {
            urls: vec!["https://1fichier.com/?abc123".into()],
        })
        .await
        .expect("resolve succeeds");
    (result, repo, plugin, primary)
}

#[tokio::test]
async fn resolve_hoster_selects_account_without_reading_secret_or_issuing_token() {
    let (result, repo, plugin, primary) =
        resolve_with_primary_credential(Some("working-key"), true, None).await;

    assert_eq!(
        result[0].resolved_url.as_deref(),
        Some("https://1fichier.com/?abc123")
    );
    assert!(
        !serde_json::to_string(&result)
            .expect("serialize resolved links")
            .contains("download.1fichier.com/token"),
        "short-lived direct capabilities must not cross IPC"
    );
    assert_eq!(result[0].account_id.as_deref(), Some("primary"));
    assert_eq!(result[0].module_name, "vortex-mod-1fichier");
    assert_eq!(
        repo.find_by_id(primary.id()).unwrap().unwrap().status(),
        AccountStatus::Valid
    );
    assert!(plugin.credentials.lock().unwrap().is_empty());
    assert!(plugin.services.lock().unwrap().is_empty());
}

#[tokio::test]
async fn resolve_hoster_surfaces_persisted_exhaustion_without_free_fallback() {
    let (result, _, plugin, _) = resolve_with_primary_credential(
        Some("quota-key"),
        false,
        Some(AccountStatus::QuotaExhausted),
    )
    .await;

    assert_eq!(result[0].status, "error");
    assert_eq!(
        result[0].error_kind,
        Some(LinkResolutionErrorKind::AccountUnavailable)
    );
    assert_eq!(
        result[0].error_message.as_deref(),
        Some("Account quota is exhausted")
    );
    assert!(plugin.credentials.lock().unwrap().is_empty());
    assert!(plugin.services.lock().unwrap().is_empty());
}

#[tokio::test]
async fn resolve_hoster_preserves_persisted_cooldown_without_free_fallback() {
    let (result, _, plugin, _) =
        resolve_with_primary_credential(Some("cooldown-key"), false, Some(AccountStatus::Cooldown))
            .await;

    assert_eq!(result[0].status, "error");
    assert_eq!(
        result[0].error_kind,
        Some(LinkResolutionErrorKind::AccountUnavailable)
    );
    assert_eq!(
        result[0].error_message.as_deref(),
        Some("Account is temporarily rate-limited")
    );
    assert!(plugin.credentials.lock().unwrap().is_empty());
    assert!(plugin.services.lock().unwrap().is_empty());
}

#[test]
fn test_extract_filename_from_url_returns_last_path_segment() {
    assert_eq!(
        extract_filename_from_url("https://example.com/files/archive.zip"),
        Some("archive.zip".to_string())
    );
}

#[test]
fn test_extract_filename_from_url_strips_query_string() {
    assert_eq!(
        extract_filename_from_url("https://example.com/file.pdf?token=abc"),
        Some("file.pdf".to_string())
    );
}

#[test]
fn test_extract_filename_from_url_returns_none_for_bare_host() {
    assert_eq!(extract_filename_from_url("https://example.com/"), None);
}

#[test]
fn test_is_media_url_detects_youtube() {
    assert!(is_media_url("https://www.youtube.com/watch?v=abc"));
}

#[test]
fn test_is_media_url_detects_vimeo() {
    assert!(is_media_url("https://vimeo.com/12345678"));
}

#[test]
fn test_is_media_url_detects_soundcloud() {
    assert!(is_media_url("https://soundcloud.com/artist/track"));
}

#[test]
fn test_is_media_url_detects_soundcloud_artist_profile() {
    assert!(is_media_url("https://soundcloud.com/forss"));
}

#[test]
fn test_is_media_url_detects_soundcloud_playlist() {
    assert!(is_media_url("https://soundcloud.com/forss/sets/soulhack"));
}

#[test]
fn test_is_media_url_returns_false_for_regular_url() {
    assert!(!is_media_url("https://example.com/file.zip"));
}

#[test]
fn test_detect_media_type_returns_video_for_youtube() {
    assert_eq!(
        detect_media_type("https://www.youtube.com/watch?v=abc"),
        Some("video".to_string())
    );
}

#[test]
fn test_detect_media_type_returns_audio_for_soundcloud() {
    assert_eq!(
        detect_media_type("https://soundcloud.com/artist/track"),
        Some("audio".to_string())
    );
}

#[test]
fn test_detect_media_type_returns_audio_for_soundcloud_artist_profile() {
    assert_eq!(
        detect_media_type("https://soundcloud.com/forss"),
        Some("audio".to_string())
    );
}

#[test]
fn test_detect_media_type_returns_audio_for_soundcloud_playlist() {
    assert_eq!(
        detect_media_type("https://soundcloud.com/forss/sets/soulhack"),
        Some("audio".to_string())
    );
}

#[test]
fn test_detect_media_type_returns_none_for_non_media() {
    assert_eq!(detect_media_type("https://example.com/file.zip"), None);
}

#[test]
fn test_extract_content_length_reads_header() {
    let mut headers = HashMap::new();
    headers.insert("content-length".to_string(), vec!["1024".to_string()]);
    let response = HttpResponse {
        status_code: 200,
        headers,
        body: vec![],
    };
    assert_eq!(extract_content_length(&response), Some(1024));
}

#[test]
fn test_extract_content_length_returns_none_when_absent() {
    let response = HttpResponse {
        status_code: 200,
        headers: HashMap::new(),
        body: vec![],
    };
    assert_eq!(extract_content_length(&response), None);
}

#[test]
fn test_is_allowed_scheme_accepts_http() {
    assert!(is_allowed_scheme("http://example.com/file.zip"));
}

#[test]
fn test_is_allowed_scheme_accepts_https() {
    assert!(is_allowed_scheme("https://example.com/file.zip"));
}

#[test]
fn test_is_allowed_scheme_accepts_ftp() {
    assert!(is_allowed_scheme("ftp://example.com/file.zip"));
}

#[test]
fn test_is_allowed_scheme_accepts_magnet() {
    assert!(is_allowed_scheme(
        "magnet:?xt=urn:btih:c12fe1c06bba254a9dc9f519b335aa7c1367a88a"
    ));
}

#[test]
fn test_is_allowed_scheme_rejects_file() {
    assert!(!is_allowed_scheme("file:///etc/passwd"));
}

#[test]
fn test_is_allowed_scheme_rejects_javascript() {
    assert!(!is_allowed_scheme("javascript:alert(1)"));
}

#[test]
fn test_is_allowed_scheme_rejects_container() {
    assert!(!is_allowed_scheme("container://some-image"));
}

#[test]
fn test_is_media_url_detects_subdomain_youtube() {
    assert!(is_media_url("https://music.youtube.com/watch?v=abc"));
}

#[test]
fn hoster_network_failures_keep_the_network_error_kind() {
    let (kind, message) = hoster_error_details(&AppError::Domain(DomainError::NetworkError(
        "connection refused".into(),
    )));

    assert_eq!(kind, LinkResolutionErrorKind::Network);
    assert_eq!(message, "Could not reach the hoster");
}

#[test]
fn test_is_media_url_rejects_fake_youtube_domain() {
    assert!(!is_media_url("https://not-youtube.com/watch?v=abc"));
}
