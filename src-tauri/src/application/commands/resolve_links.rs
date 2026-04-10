//! Handler for the `ResolveLinksCommand`.
//!
//! Checks each URL via plugin loader and HTTP HEAD, returning
//! resolution metadata for the frontend link grabber view.

use serde::Serialize;
use uuid::Uuid;

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::model::http::HttpResponse;

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
    pub is_media: bool,
    pub media_type: Option<String>,
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
                        is_media,
                        media_type,
                    });
                }
                Err(e) => {
                    results.push(ResolvedLinkDto {
                        id,
                        original_url: url.clone(),
                        resolved_url: None,
                        filename: None,
                        size_bytes: None,
                        status: "error".to_string(),
                        error_message: Some(e.to_string()),
                        module_name,
                        is_media,
                        media_type,
                    });
                }
            }
        }

        Ok(results)
    }
}

fn is_allowed_scheme(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://") || url.starts_with("ftp://")
}

fn extract_filename_from_url(url: &str) -> Option<String> {
    // Strip query string and fragment
    let path = url.split('?').next().unwrap_or(url);
    let path = path.split('#').next().unwrap_or(path);
    // Extract the path component after the scheme + authority (e.g. after "https://host")
    let path_only = if let Some(after_scheme) = path.find("://") {
        let after = &path[after_scheme + 3..];
        match after.find('/') {
            Some(slash) => &after[slash + 1..],
            None => return None,
        }
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

    use super::*;

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
