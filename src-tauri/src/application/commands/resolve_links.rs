//! Handler for the `ResolveLinksCommand`.
//!
//! Checks each URL via plugin loader and HTTP HEAD, returning
//! resolution metadata for the frontend link grabber view.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::application::services::account_rotator::NextAccountOutcome;
use crate::domain::error::DomainError;
use crate::domain::model::account::{Account, AccountStatus};
use crate::domain::model::credential::Credential;
use crate::domain::model::http::HttpResponse;
use crate::domain::model::plugin::PluginCategory;

use super::ResolveLinksCommand;

/// Resolution metadata for a single URL.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLinkDto {
    pub id: String,
    pub original_url: String,
    pub resolved_url: Option<String>,
    pub filename: Option<String>,
    pub size_bytes: Option<u64>,
    /// "checking" | "online" | "offline" | "error"
    pub status: String,
    pub error_message: Option<String>,
    pub module_name: String,
    pub account_id: Option<String>,
    pub is_media: bool,
    pub media_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HosterExtractResponse {
    files: Vec<HosterFile>,
}

#[derive(Debug, Deserialize)]
struct HosterFile {
    url: String,
    filename: Option<String>,
    size_bytes: Option<u64>,
    direct_url: Option<String>,
    traffic_used_bytes: Option<u64>,
    traffic_total_bytes: Option<u64>,
}

struct HosterResolution {
    resolved_url: String,
    filename: Option<String>,
    size_bytes: Option<u64>,
    account_id: Option<String>,
}

impl CommandBus {
    pub async fn handle_resolve_links(
        &self,
        cmd: ResolveLinksCommand,
    ) -> Result<Vec<ResolvedLinkDto>, AppError> {
        const MAX_URLS: usize = 500;
        if cmd.urls.len() > MAX_URLS {
            return Err(AppError::Validation(format!(
                "Too many URLs: {} (max {})",
                cmd.urls.len(),
                MAX_URLS
            )));
        }

        let mut results = Vec::with_capacity(cmd.urls.len());

        // TODO(perf): resolve URLs concurrently with bounded parallelism.
        // Current sequential approach is acceptable for ≤500 URLs but should
        // use tokio::task::spawn_blocking + Semaphore for production workloads.
        for url in &cmd.urls {
            let id = Uuid::new_v4().to_string();

            if !is_allowed_scheme(url) {
                results.push(ResolvedLinkDto {
                    id,
                    original_url: url.clone(),
                    resolved_url: None,
                    filename: None,
                    size_bytes: None,
                    status: "error".to_string(),
                    error_message: Some("URL scheme not allowed".to_string()),
                    module_name: "core-http".to_string(),
                    account_id: None,
                    is_media: false,
                    media_type: None,
                });
                continue;
            }

            if url.to_lowercase().starts_with("magnet:") {
                results.push(ResolvedLinkDto {
                    id,
                    original_url: url.clone(),
                    resolved_url: Some(url.clone()),
                    filename: None,
                    size_bytes: None,
                    status: "online".to_string(),
                    error_message: None,
                    module_name: "magnet".to_string(),
                    account_id: None,
                    is_media: false,
                    media_type: None,
                });
                continue;
            }

            let plugin_info = self.plugin_loader().resolve_url(url);
            let module_name = match &plugin_info {
                Ok(Some(info)) => info.name().to_string(),
                _ => "core-http".to_string(),
            };

            let is_hoster = matches!(
                plugin_info.as_ref().ok().and_then(Option::as_ref),
                Some(info)
                    if matches!(info.category(), PluginCategory::Hoster | PluginCategory::Debrid)
            );
            if is_hoster {
                match self.resolve_hoster_link(url, &module_name) {
                    Ok(resolved) => results.push(ResolvedLinkDto {
                        id,
                        original_url: url.clone(),
                        resolved_url: Some(resolved.resolved_url),
                        filename: resolved.filename,
                        size_bytes: resolved.size_bytes,
                        status: "online".to_string(),
                        error_message: None,
                        module_name,
                        account_id: resolved.account_id,
                        is_media: false,
                        media_type: None,
                    }),
                    Err(error) => {
                        tracing::debug!(module_name, "hoster link resolution failed");
                        results.push(ResolvedLinkDto {
                            id,
                            original_url: url.clone(),
                            resolved_url: None,
                            filename: None,
                            size_bytes: None,
                            status: "error".to_string(),
                            error_message: Some(sanitize_hoster_error(&error)),
                            module_name,
                            account_id: None,
                            is_media: false,
                            media_type: None,
                        });
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
                    results.push(ResolvedLinkDto {
                        id,
                        original_url: url.clone(),
                        resolved_url: Some(url.clone()),
                        filename,
                        size_bytes: size,
                        status: "online".to_string(),
                        error_message: None,
                        module_name,
                        account_id: None,
                        is_media,
                        media_type,
                    });
                }
                Ok(_) => {
                    results.push(ResolvedLinkDto {
                        id,
                        original_url: url.clone(),
                        resolved_url: None,
                        filename: None,
                        size_bytes: None,
                        status: "offline".to_string(),
                        error_message: None,
                        module_name,
                        account_id: None,
                        is_media,
                        media_type,
                    });
                }
                Err(e) => {
                    tracing::debug!(error = %e, "link resolution failed");
                    results.push(ResolvedLinkDto {
                        id,
                        original_url: url.clone(),
                        resolved_url: None,
                        filename: None,
                        size_bytes: None,
                        status: "error".to_string(),
                        error_message: Some(sanitize_resolve_error(&e)),
                        module_name,
                        account_id: None,
                        is_media,
                        media_type,
                    });
                }
            }
        }

        Ok(results)
    }

    fn resolve_hoster_link(
        &self,
        url: &str,
        service_name: &str,
    ) -> Result<HosterResolution, AppError> {
        if let (Some(repo), Some(store)) = (self.account_repo(), self.account_credential_store()) {
            let max_attempts = repo.list_by_service(service_name)?.len();
            for _ in 0..max_attempts {
                let Some(mut account) = self.next_hoster_account(service_name)? else {
                    break;
                };
                let Some(password) = store.get_password(account.id())? else {
                    account.set_status(AccountStatus::MissingCredential);
                    repo.save(&account)?;
                    continue;
                };
                let credential = Credential::new(account.username(), password);

                match self
                    .plugin_loader()
                    .extract_links_with_credential(url, &credential)
                {
                    Ok(payload) => {
                        let resolved =
                            parse_hoster_response(&payload, Some(account.id().as_str()))?;
                        if let Some(total) = resolved.traffic_total_bytes {
                            account.set_traffic_total(total);
                            if let Some(used) = resolved.traffic_used_bytes {
                                account.set_traffic_left(total.saturating_sub(used));
                            }
                        }
                        account.set_status(AccountStatus::Valid);
                        repo.save(&account)?;
                        return Ok(resolved.into_resolution());
                    }
                    Err(error) => {
                        let status = match error {
                            DomainError::AccountInvalidCredentials => {
                                AccountStatus::InvalidCredentials
                            }
                            DomainError::AccountExpired => AccountStatus::Expired,
                            DomainError::AccountCooldown => AccountStatus::Cooldown,
                            DomainError::AccountQuotaExceeded => AccountStatus::QuotaExhausted,
                            other => return Err(other.into()),
                        };
                        if status == AccountStatus::QuotaExhausted
                            && let Some(rotator) = self.account_rotator()
                        {
                            rotator.mark_exhausted(account.id(), service_name, 60)?;
                        } else if matches!(
                            status,
                            AccountStatus::QuotaExhausted | AccountStatus::Cooldown
                        ) {
                            account
                                .mark_unavailable(status, current_time_ms().saturating_add(60_000));
                            repo.save(&account)?;
                        } else {
                            account.set_status(status);
                            repo.save(&account)?;
                        }
                    }
                }
            }
        }

        let payload = self.plugin_loader().extract_links(url)?;
        Ok(parse_hoster_response(&payload, None)?.into_resolution())
    }

    fn next_hoster_account(&self, service_name: &str) -> Result<Option<Account>, AppError> {
        let Some(rotator) = self.account_rotator() else {
            return self.resolve_account_for(service_name);
        };
        let strategy = self.config_store().get_config()?.account_selection_strategy;
        match rotator.next_account(service_name, strategy)? {
            NextAccountOutcome::Picked(account) => Ok(Some(account)),
            NextAccountOutcome::NoneAvailable | NextAccountOutcome::AllExhausted { .. } => Ok(None),
        }
    }
}

struct ParsedHosterResponse {
    file: HosterFile,
    account_id: Option<String>,
    traffic_used_bytes: Option<u64>,
    traffic_total_bytes: Option<u64>,
}

impl ParsedHosterResponse {
    fn into_resolution(self) -> HosterResolution {
        let direct_url = self.file.direct_url;
        HosterResolution {
            resolved_url: direct_url.unwrap_or(self.file.url),
            filename: self.file.filename,
            size_bytes: self.file.size_bytes,
            account_id: self.account_id,
        }
    }
}

fn parse_hoster_response(
    payload: &str,
    account_id: Option<&str>,
) -> Result<ParsedHosterResponse, AppError> {
    let response: HosterExtractResponse = serde_json::from_str(payload)
        .map_err(|_| AppError::Plugin("hoster returned an invalid response".into()))?;
    let file = response
        .files
        .into_iter()
        .next()
        .ok_or_else(|| AppError::Plugin("hoster returned no file".into()))?;
    let selected_account = file.direct_url.as_ref().and(account_id).map(str::to_string);
    Ok(ParsedHosterResponse {
        traffic_used_bytes: file.traffic_used_bytes,
        traffic_total_bytes: file.traffic_total_bytes,
        file,
        account_id: selected_account,
    })
}

fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn sanitize_hoster_error(error: &AppError) -> String {
    match error {
        AppError::Domain(DomainError::AccountInvalidCredentials) => {
            "Account credentials were rejected".to_string()
        }
        AppError::Domain(DomainError::AccountExpired) => "Account is expired".to_string(),
        AppError::Domain(DomainError::AccountCooldown) => {
            "Account is temporarily rate-limited".to_string()
        }
        AppError::Domain(DomainError::AccountQuotaExceeded) => {
            "Account quota is exhausted".to_string()
        }
        _ => "Could not resolve hoster link".to_string(),
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
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::application::commands::tests_support::{
        CapturingEventBus, FakeAccountCredentialStore, InMemoryAccountRepo,
        build_account_bus_with_plugin_loader,
    };
    use crate::application::services::AccountSelector;
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
    }

    impl PremiumPluginLoader {
        fn new() -> Self {
            Self {
                credentials: Mutex::new(Vec::new()),
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

        fn extract_links_with_credential(
            &self,
            _: &str,
            credential: &Credential,
        ) -> Result<String, DomainError> {
            self.credentials
                .lock()
                .unwrap()
                .push(credential.password().to_string());
            if credential.password() == "expired-key" {
                return Err(DomainError::AccountExpired);
            }
            Ok(r#"{"kind":"file","mode":"premium","files":[{"id":"abc","url":"https://1fichier.com/?abc123","filename":"file.zip","size_bytes":42,"direct_url":"https://download.1fichier.com/token/file.zip","resumable":true,"wait_seconds":null,"requires_captcha":false,"traffic_used_bytes":1,"traffic_total_bytes":100}]}"#.into())
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

    #[tokio::test]
    async fn resolve_hoster_rotates_expired_account_and_returns_opaque_account_id() {
        let repo = Arc::new(InMemoryAccountRepo::new());
        let credentials = Arc::new(FakeAccountCredentialStore::new());
        let events = Arc::new(CapturingEventBus::new());
        let plugin = Arc::new(PremiumPluginLoader::new());
        let primary = premium_account("primary", 100);
        let backup = premium_account("backup", 50);
        repo.save(&primary).unwrap();
        repo.save(&backup).unwrap();
        credentials
            .store_password(primary.id(), "expired-key")
            .unwrap();
        credentials
            .store_password(backup.id(), "working-key")
            .unwrap();
        let selector = AccountSelector::new(repo.clone(), events.clone(), Arc::new(FixedClock));
        let bus = build_account_bus_with_plugin_loader(
            repo.clone(),
            credentials,
            events,
            None,
            None,
            plugin.clone(),
        )
        .with_account_selector(selector);

        let result = bus
            .handle_resolve_links(ResolveLinksCommand {
                urls: vec!["https://1fichier.com/?abc123".into()],
            })
            .await
            .expect("resolve succeeds");

        assert_eq!(
            result[0].resolved_url.as_deref(),
            Some("https://download.1fichier.com/token/file.zip")
        );
        assert_eq!(result[0].account_id.as_deref(), Some("backup"));
        assert_eq!(result[0].module_name, "vortex-mod-1fichier");
        assert_eq!(
            repo.find_by_id(primary.id()).unwrap().unwrap().status(),
            AccountStatus::Expired
        );
        assert_eq!(
            plugin.credentials.lock().unwrap().as_slice(),
            ["expired-key", "working-key"]
        );
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
        assert!(is_media_url("https://www.youtube.com/watch?v=abc"));
    }

    #[test]
    fn test_is_media_url_rejects_fake_youtube_domain() {
        assert!(!is_media_url("https://not-youtube.com/watch?v=abc"));
    }
}
