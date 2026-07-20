use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::model::config::CAPTCHA_SOLVER_ANTICAPTCHA;
use crate::domain::model::credential::Credential;

use super::{DeleteCaptchaCredentialCommand, SetCaptchaCredentialCommand};

const MAX_ANTICAPTCHA_API_KEY_BYTES: usize = 1_024;

impl CommandBus {
    pub async fn handle_set_captcha_credential(
        &self,
        command: SetCaptchaCredentialCommand,
    ) -> Result<(), AppError> {
        let api_key = command.api_key.trim();
        if api_key.is_empty() || api_key.len() > MAX_ANTICAPTCHA_API_KEY_BYTES {
            return Err(AppError::Validation(
                "AntiCaptcha API key is empty or exceeds safety limits".into(),
            ));
        }
        self.credential_store().store(
            CAPTCHA_SOLVER_ANTICAPTCHA,
            &Credential::new("api-key", api_key),
        )?;
        Ok(())
    }

    pub async fn handle_delete_captcha_credential(
        &self,
        _command: DeleteCaptchaCredentialCommand,
    ) -> Result<(), AppError> {
        self.credential_store().delete(CAPTCHA_SOLVER_ANTICAPTCHA)?;
        Ok(())
    }

    pub fn captcha_credential_configured(&self) -> Result<bool, AppError> {
        Ok(self
            .credential_store()
            .get(CAPTCHA_SOLVER_ANTICAPTCHA)?
            .is_some())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::application::commands::tests_support::{
        InMemoryCredentialStore, build_credential_bus,
    };
    use crate::application::commands::{
        DeleteCaptchaCredentialCommand, SetCaptchaCredentialCommand,
    };
    use crate::domain::model::config::CAPTCHA_SOLVER_ANTICAPTCHA;
    use crate::domain::ports::driven::CredentialStore;

    #[tokio::test]
    async fn anti_captcha_api_key_is_stored_only_in_the_scoped_credential_store() {
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let bus = build_credential_bus(credentials.clone());

        bus.handle_set_captcha_credential(SetCaptchaCredentialCommand {
            api_key: "secret-api-key".into(),
        })
        .await
        .expect("store credential");

        assert!(bus.captcha_credential_configured().expect("status"));
        let stored = credentials
            .get(CAPTCHA_SOLVER_ANTICAPTCHA)
            .expect("read credential")
            .expect("configured credential");
        assert_eq!(stored.username(), "api-key");
        assert_eq!(stored.password(), "secret-api-key");
    }

    #[tokio::test]
    async fn anti_captcha_api_key_can_be_deleted_and_is_redacted_from_debug() {
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let bus = build_credential_bus(credentials);
        let command = SetCaptchaCredentialCommand {
            api_key: "never-log-me".into(),
        };
        assert!(!format!("{command:?}").contains("never-log-me"));
        bus.handle_set_captcha_credential(command)
            .await
            .expect("store credential");

        bus.handle_delete_captcha_credential(DeleteCaptchaCredentialCommand)
            .await
            .expect("delete credential");

        assert!(!bus.captcha_credential_configured().expect("status"));
    }

    #[tokio::test]
    async fn empty_anti_captcha_api_key_is_rejected_before_keyring_access() {
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let bus = build_credential_bus(credentials.clone());

        let result = bus
            .handle_set_captcha_credential(SetCaptchaCredentialCommand {
                api_key: "  ".into(),
            })
            .await;

        assert!(result.is_err());
        assert_eq!(credentials.entry_count(), 0);
    }
}
