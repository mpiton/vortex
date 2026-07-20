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
        let api_key = command.api_key.trim().to_string();
        if api_key.is_empty() || api_key.len() > MAX_ANTICAPTCHA_API_KEY_BYTES {
            return Err(AppError::Validation(
                "AntiCaptcha API key is empty or exceeds safety limits".into(),
            ));
        }
        let store = self.credential_store_arc();
        tokio::task::spawn_blocking(move || {
            store.store(
                CAPTCHA_SOLVER_ANTICAPTCHA,
                &Credential::new("api-key", api_key),
            )
        })
        .await
        .map_err(|error| AppError::Storage(format!("credential task failed: {error}")))??;
        Ok(())
    }

    pub async fn handle_delete_captcha_credential(
        &self,
        _command: DeleteCaptchaCredentialCommand,
    ) -> Result<(), AppError> {
        let store = self.credential_store_arc();
        tokio::task::spawn_blocking(move || store.delete(CAPTCHA_SOLVER_ANTICAPTCHA))
            .await
            .map_err(|error| AppError::Storage(format!("credential task failed: {error}")))??;
        Ok(())
    }

    pub async fn captcha_credential_configured(&self) -> Result<bool, AppError> {
        let store = self.credential_store_arc();
        let credential = tokio::task::spawn_blocking(move || store.get(CAPTCHA_SOLVER_ANTICAPTCHA))
            .await
            .map_err(|error| AppError::Storage(format!("credential task failed: {error}")))??;
        Ok(credential.is_some())
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

        assert!(bus.captcha_credential_configured().await.expect("status"));
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

        assert!(!bus.captcha_credential_configured().await.expect("status"));
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

    struct ThreadRecordingCredentialStore {
        inner: InMemoryCredentialStore,
        threads: std::sync::Mutex<Vec<std::thread::ThreadId>>,
    }

    impl ThreadRecordingCredentialStore {
        fn new() -> Self {
            Self {
                inner: InMemoryCredentialStore::new(),
                threads: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn record_thread(&self) {
            self.threads
                .lock()
                .expect("thread log")
                .push(std::thread::current().id());
        }
    }

    impl CredentialStore for ThreadRecordingCredentialStore {
        fn get(
            &self,
            service: &str,
        ) -> Result<Option<crate::domain::model::credential::Credential>, crate::domain::DomainError>
        {
            self.record_thread();
            self.inner.get(service)
        }

        fn store(
            &self,
            service: &str,
            credential: &crate::domain::model::credential::Credential,
        ) -> Result<(), crate::domain::DomainError> {
            self.record_thread();
            self.inner.store(service, credential)
        }

        fn delete(&self, service: &str) -> Result<(), crate::domain::DomainError> {
            self.record_thread();
            self.inner.delete(service)
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn credential_store_io_runs_outside_the_async_runtime_thread() {
        let credentials = Arc::new(ThreadRecordingCredentialStore::new());
        let bus = build_credential_bus(credentials.clone());
        let runtime_thread = std::thread::current().id();

        bus.handle_set_captcha_credential(SetCaptchaCredentialCommand {
            api_key: "secret-api-key".into(),
        })
        .await
        .expect("store credential");
        assert!(bus.captcha_credential_configured().await.expect("status"));
        bus.handle_delete_captcha_credential(DeleteCaptchaCredentialCommand)
            .await
            .expect("delete credential");

        let io_threads = credentials.threads.lock().expect("thread log");
        assert_eq!(io_threads.len(), 3);
        assert!(io_threads.iter().all(|thread| *thread != runtime_thread));
    }
}
