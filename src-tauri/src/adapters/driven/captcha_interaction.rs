use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::domain::error::DomainError;
use crate::domain::model::captcha::CaptchaChallenge;
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
}

fn browser_window_label(challenge_id: &str) -> String {
    let mut hasher = DefaultHasher::new();
    challenge_id.hash(&mut hasher);
    format!("captcha-browser-{:016x}", hasher.finish())
}

fn browser_window_path(challenge_id: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(challenge_id.as_bytes()).collect();
    format!("index.html?captchaWindow={encoded}")
}

fn window_error(_: tauri::Error) -> DomainError {
    DomainError::PluginError("could not open the CAPTCHA browser window".into())
}

#[cfg(test)]
mod tests {
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
    }
}
