//! Shared HTTP client built from the persisted network settings (MAT-136).
//!
//! Applied at startup only — changing these settings requires an app
//! restart. `dns_over_https` is not consumed yet (planned).

use std::time::Duration;

use tracing::warn;

use crate::domain::model::config::AppConfig;

/// Build the app-wide reqwest client from user network settings.
///
/// Invalid proxy values fall back to a direct connection instead of
/// failing startup: the app must stay usable so the user can fix the
/// setting.
pub fn client_from_config(config: &AppConfig) -> Result<reqwest::Client, reqwest::Error> {
    let mut builder = reqwest::Client::builder()
        .user_agent(effective_user_agent(config))
        .connect_timeout(Duration::from_secs(effective_connect_timeout_secs(config)));
    if let Some(proxy_url) = configured_proxy_uri(config) {
        match reqwest::Proxy::all(&proxy_url) {
            Ok(proxy) => builder = builder.proxy(proxy),
            Err(err) => warn!(proxy_url, error = %err, "invalid proxy setting ignored"),
        }
    }
    builder.build()
}

fn effective_user_agent(config: &AppConfig) -> String {
    let trimmed = config.user_agent.trim();
    if trimmed.is_empty() {
        AppConfig::default().user_agent
    } else if reqwest::header::HeaderValue::from_str(trimmed).is_err() {
        warn!(user_agent = trimmed, "invalid user_agent setting ignored");
        AppConfig::default().user_agent
    } else {
        trimmed.to_string()
    }
}

fn effective_connect_timeout_secs(config: &AppConfig) -> u64 {
    if config.connection_timeout_seconds == 0 {
        u64::from(AppConfig::default().connection_timeout_seconds)
    } else {
        u64::from(config.connection_timeout_seconds)
    }
}

/// Proxy URI to use, or `None` for a direct connection. Prepends the
/// selected proxy type as scheme when the user typed a bare `host:port`.
fn configured_proxy_uri(config: &AppConfig) -> Option<String> {
    if config.proxy_type == "none" {
        return None;
    }
    let url = config.proxy_url.as_deref().map(str::trim)?;
    if url.is_empty() {
        return None;
    }
    if url.contains("://") {
        Some(url.to_string())
    } else {
        Some(format!("{}://{}", config.proxy_type, url))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_client_from_config_sends_configured_user_agent() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/probe"))
            .and(header("user-agent", "TestAgent/9"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock_server)
            .await;

        let config = AppConfig {
            user_agent: "TestAgent/9".to_string(),
            ..AppConfig::default()
        };
        let client = client_from_config(&config).expect("client builds");
        let response = client
            .get(format!("{}/probe", mock_server.uri()))
            .send()
            .await
            .expect("request succeeds");

        assert_eq!(response.status(), 200);
    }

    #[test]
    fn test_client_from_config_with_invalid_proxy_url_falls_back_to_direct() {
        let config = AppConfig {
            proxy_type: "http".to_string(),
            proxy_url: Some("::not a proxy url::".to_string()),
            ..AppConfig::default()
        };

        assert!(client_from_config(&config).is_ok());
    }

    #[test]
    fn test_client_from_config_with_control_chars_in_user_agent_still_builds() {
        // A hand-edited config.toml can hold a value that is not a valid
        // HTTP header; startup must not fail on it.
        let config = AppConfig {
            user_agent: "Bad\nAgent".to_string(),
            ..AppConfig::default()
        };

        assert!(client_from_config(&config).is_ok());
    }

    #[test]
    fn test_effective_user_agent_with_blank_value_uses_default() {
        let config = AppConfig {
            user_agent: "   ".to_string(),
            ..AppConfig::default()
        };

        assert_eq!(effective_user_agent(&config), "Vortex/1.0");
    }

    #[test]
    fn test_effective_connect_timeout_with_zero_uses_default() {
        let config = AppConfig {
            connection_timeout_seconds: 0,
            ..AppConfig::default()
        };

        assert_eq!(effective_connect_timeout_secs(&config), 30);
    }

    #[test]
    fn test_configured_proxy_uri_with_none_type_returns_none() {
        assert_eq!(configured_proxy_uri(&AppConfig::default()), None);
    }

    #[test]
    fn test_configured_proxy_uri_with_bare_host_prepends_scheme() {
        let config = AppConfig {
            proxy_type: "socks5".to_string(),
            proxy_url: Some("127.0.0.1:1080".to_string()),
            ..AppConfig::default()
        };

        assert_eq!(
            configured_proxy_uri(&config),
            Some("socks5://127.0.0.1:1080".to_string())
        );
    }

    #[test]
    fn test_configured_proxy_uri_with_explicit_scheme_keeps_url() {
        let config = AppConfig {
            proxy_type: "http".to_string(),
            proxy_url: Some("http://proxy.local:3128".to_string()),
            ..AppConfig::default()
        };

        assert_eq!(
            configured_proxy_uri(&config),
            Some("http://proxy.local:3128".to_string())
        );
    }
}
