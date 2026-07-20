use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::adapters::captcha_browser::browser_window_label;
use crate::domain::error::DomainError;
use crate::domain::model::captcha::{CaptchaChallenge, CaptchaId};
use crate::domain::ports::driven::CaptchaInteraction;

pub struct TauriCaptchaInteraction {
    app: AppHandle,
}

impl TauriCaptchaInteraction {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl CaptchaInteraction for TauriCaptchaInteraction {
    fn request(&self, challenge: &CaptchaChallenge) -> Result<(), DomainError> {
        let label = browser_window_label(challenge.id().as_str());
        if let Some(window) = self.app.get_webview_window(&label) {
            window.show().map_err(window_error)?;
            window.set_focus().map_err(window_error)?;
            return Ok(());
        }

        WebviewWindowBuilder::new(
            &self.app,
            label,
            WebviewUrl::App(browser_window_path(challenge.id().as_str()).into()),
        )
        .title("Vortex CAPTCHA")
        .inner_size(560.0, 680.0)
        .resizable(true)
        .center()
        .build()
        .map_err(window_error)?;
        Ok(())
    }

    fn dismiss(&self, challenge_id: &CaptchaId) -> Result<(), DomainError> {
        let label = browser_window_label(challenge_id.as_str());
        if let Some(window) = self.app.get_webview_window(&label) {
            window.close().map_err(close_window_error)?;
        }
        Ok(())
    }
}

fn browser_window_path(challenge_id: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(challenge_id.as_bytes()).collect();
    format!("index.html?captchaWindow={encoded}")
}

fn window_error(_: tauri::Error) -> DomainError {
    DomainError::PluginError("could not open the CAPTCHA browser window".into())
}

fn close_window_error(_: tauri::Error) -> DomainError {
    DomainError::PluginError("could not close the CAPTCHA browser window".into())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn popup_path_encodes_the_challenge_id_and_label_is_capability_safe() {
        assert_eq!(
            browser_window_path("captcha / ?"),
            "index.html?captchaWindow=captcha+%2F+%3F"
        );
        assert!(
            browser_window_label("captcha / ?")
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-')
        );
        assert_eq!(
            browser_window_label("captcha / ?").len(),
            "captcha-browser-".len() + 64
        );
    }

    #[test]
    fn popup_uses_a_dedicated_minimal_capability() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let default: serde_json::Value = serde_json::from_slice(
            &std::fs::read(manifest_dir.join("capabilities/default.json"))
                .expect("read default capability"),
        )
        .expect("parse default capability");
        let browser: serde_json::Value = serde_json::from_slice(
            &std::fs::read(manifest_dir.join("capabilities/captcha-browser.json"))
                .expect("read CAPTCHA browser capability"),
        )
        .expect("parse CAPTCHA browser capability");

        assert_eq!(default["windows"], serde_json::json!(["main"]));
        assert!(
            default["permissions"]
                .as_array()
                .expect("default permissions")
                .contains(&serde_json::json!("main-window-commands"))
        );
        assert_eq!(browser["windows"], serde_json::json!(["captcha-browser-*"]));
        assert_eq!(
            browser["permissions"],
            serde_json::json!(["captcha-browser-commands", "core:window:allow-close"])
        );

        let permission_source = std::fs::read_to_string(manifest_dir.join("permissions/app.toml"))
            .expect("read application permissions");
        let permissions: toml::Value =
            toml::from_str(&permission_source).expect("parse application permissions");
        let browser_commands = permissions["permission"]
            .as_array()
            .expect("permission entries")
            .iter()
            .find(|permission| {
                permission["identifier"].as_str() == Some("captcha-browser-commands")
            })
            .expect("CAPTCHA browser command permission");
        assert_eq!(
            browser_commands["commands"]["allow"],
            toml::Value::Array(
                [
                    "captcha_get_pending",
                    "captcha_solve",
                    "captcha_skip",
                    "captcha_retry",
                ]
                .into_iter()
                .map(|command| toml::Value::String(command.to_string()))
                .collect()
            )
        );
    }
}
