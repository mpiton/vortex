use std::sync::Arc;
use std::time::Instant;

use crate::domain::error::DomainError;
use crate::domain::model::credential::Credential;
use crate::domain::ports::driven::{AccountValidator, PluginLoader, ValidationOutcome};

/// Validates account credentials through the plugin that owns the service.
pub struct PluginAccountValidator {
    loader: Arc<dyn PluginLoader>,
}

impl PluginAccountValidator {
    pub fn new(loader: Arc<dyn PluginLoader>) -> Self {
        Self { loader }
    }
}

impl AccountValidator for PluginAccountValidator {
    fn validate(
        &self,
        service_name: &str,
        username: &str,
        password: &str,
    ) -> Result<ValidationOutcome, DomainError> {
        let started_at = Instant::now();
        let credential = Credential::new(username, password);
        let mut outcome = self.loader.validate_account(service_name, &credential)?;
        if outcome.latency_ms.is_none() {
            outcome.latency_ms =
                Some(u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX));
        }
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use crate::domain::error::DomainError;
    use crate::domain::model::account::AccountStatus;
    use crate::domain::model::credential::Credential;
    use crate::domain::model::plugin::{PluginInfo, PluginManifest};
    use crate::domain::ports::driven::{AccountValidator, PluginLoader, ValidationOutcome};

    use super::PluginAccountValidator;

    struct CapturingLoader {
        captured: Mutex<Option<(String, Credential)>>,
        outcome: ValidationOutcome,
    }

    impl PluginLoader for CapturingLoader {
        fn load(&self, _: &PluginManifest) -> Result<(), DomainError> {
            Ok(())
        }

        fn unload(&self, _: &str) -> Result<(), DomainError> {
            Ok(())
        }

        fn resolve_url(&self, _: &str) -> Result<Option<PluginInfo>, DomainError> {
            Ok(None)
        }

        fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
            Ok(Vec::new())
        }

        fn set_enabled(&self, _: &str, _: bool) -> Result<(), DomainError> {
            Ok(())
        }

        fn validate_account(
            &self,
            service_name: &str,
            credential: &Credential,
        ) -> Result<ValidationOutcome, DomainError> {
            *self.captured.lock().expect("capture mutex") =
                Some((service_name.to_string(), credential.clone()));
            Ok(self.outcome.clone())
        }
    }

    #[test]
    fn validate_scopes_exact_credentials_and_records_host_latency() {
        let loader = Arc::new(CapturingLoader {
            captured: Mutex::new(None),
            outcome: ValidationOutcome::ok(),
        });
        let validator = PluginAccountValidator::new(loader.clone());

        let outcome = validator
            .validate("vortex-mod-1fichier", "alice", "api-key")
            .expect("validation succeeds");

        assert_eq!(outcome.status, AccountStatus::Valid);
        assert!(outcome.latency_ms.is_some());
        let captured = loader.captured.lock().expect("capture mutex");
        let (service, credential) = captured.as_ref().expect("credential captured");
        assert_eq!(service, "vortex-mod-1fichier");
        assert_eq!(credential.username(), "alice");
        assert_eq!(credential.password(), "api-key");
    }

    #[test]
    fn validate_preserves_plugin_supplied_latency_and_typed_status() {
        let loader = Arc::new(CapturingLoader {
            captured: Mutex::new(None),
            outcome: ValidationOutcome {
                latency_ms: Some(42),
                ..ValidationOutcome::rejected(
                    AccountStatus::InvalidCredentials,
                    "credential rejected",
                )
            },
        });
        let validator = PluginAccountValidator::new(loader);

        let outcome = validator
            .validate("vortex-mod-1fichier", "alice", "bad-key")
            .expect("typed rejection is an outcome");

        assert!(!outcome.valid);
        assert_eq!(outcome.status, AccountStatus::InvalidCredentials);
        assert_eq!(outcome.latency_ms, Some(42));
    }
}
