//! Handler for [`ExportAccountsCommand`](super::ExportAccountsCommand).
//!
//! Serializes every persisted account (metadata + plaintext password
//! pulled from the keyring) into a JSON bundle, encrypts it via the
//! configured [`PassphraseCodec`], and writes the resulting opaque
//! blob to disk.
//!
//! The on-disk format is a single binary file. Plaintext passwords
//! never touch the filesystem outside the encrypted bundle.

use serde::{Deserialize, Serialize};

use super::ExportAccountsOutcome;
use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;
use crate::domain::model::account::{Account, AccountType};

/// Bundle format version. Incremented when the on-disk layout changes
/// in a backward-incompatible way (e.g. extra mandatory fields).
pub(crate) const EXPORT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ExportEnvelope {
    pub(crate) version: u32,
    pub(crate) accounts: Vec<ExportEntry>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ExportEntry {
    pub(crate) service_name: String,
    pub(crate) username: String,
    pub(crate) password: String,
    pub(crate) account_type: String,
    pub(crate) enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) traffic_left: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) traffic_total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) valid_until: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_validated: Option<u64>,
    /// Original `created_at` of the source account so a round-trip
    /// preserves chronology. Optional for backward compatibility with
    /// bundles produced by earlier versions of this code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) created_at: Option<u64>,
}

impl ExportEntry {
    pub(crate) fn from_account(account: &Account, password: String) -> Self {
        Self {
            service_name: account.service_name().to_string(),
            username: account.username().to_string(),
            password,
            account_type: account.account_type().to_string(),
            enabled: account.is_enabled(),
            traffic_left: account.traffic_left(),
            traffic_total: account.traffic_total(),
            valid_until: account.valid_until(),
            last_validated: account.last_validated(),
            created_at: Some(account.created_at()),
        }
    }

    pub(crate) fn parse_account_type(&self) -> Result<AccountType, AppError> {
        self.account_type
            .parse::<AccountType>()
            .map_err(AppError::Domain)
    }
}

impl CommandBus {
    pub async fn handle_export_accounts(
        &self,
        cmd: super::ExportAccountsCommand,
    ) -> Result<ExportAccountsOutcome, AppError> {
        if cmd.passphrase.is_empty() {
            return Err(AppError::Validation("passphrase must not be empty".into()));
        }
        let repo = self
            .account_repo()
            .ok_or_else(|| AppError::Validation("account repository not configured".into()))?;
        let store = self.account_credential_store().ok_or_else(|| {
            AppError::Validation("account credential store not configured".into())
        })?;
        let codec = self
            .passphrase_codec()
            .ok_or_else(|| AppError::Validation("passphrase codec not configured".into()))?;

        let accounts = repo.list()?;
        let mut entries = Vec::with_capacity(accounts.len());
        for account in &accounts {
            let password = store.get_password(account.id())?.ok_or_else(|| {
                AppError::Storage(format!(
                    "no stored password for account {}",
                    account.id().as_str()
                ))
            })?;
            entries.push(ExportEntry::from_account(account, password));
        }

        let envelope = ExportEnvelope {
            version: EXPORT_VERSION,
            accounts: entries,
        };
        let plaintext = serde_json::to_vec(&envelope)
            .map_err(|e| AppError::Storage(format!("serialise export: {e}")))?;
        let ciphertext = codec.seal(&cmd.passphrase, &plaintext)?;

        let path = cmd.path.clone();
        let bytes = ciphertext.clone();
        tokio::task::spawn_blocking(move || std::fs::write(&path, &bytes))
            .await
            .map_err(|e| AppError::Storage(format!("export write task failed: {e}")))?
            .map_err(|e| AppError::Storage(format!("export write failed: {e}")))?;

        let count = accounts.len() as u32;
        self.event_bus()
            .publish(DomainEvent::AccountsExported { count });

        Ok(ExportAccountsOutcome {
            path: cmd.path,
            count,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::TempDir;

    use super::super::{AddAccountCommand, ExportAccountsCommand};
    use super::ExportEnvelope;
    use crate::application::commands::tests_support::{
        CapturingEventBus, FakeAccountCredentialStore, FakePassphraseCodec, InMemoryAccountRepo,
        build_account_bus,
    };
    use crate::application::error::AppError;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::account::AccountType;
    use crate::domain::ports::driven::{AccountCredentialStore, PassphraseCodec};

    fn add_command(service: &str, user: &str, pw: &str) -> AddAccountCommand {
        AddAccountCommand {
            service_name: service.into(),
            username: user.into(),
            password: pw.into(),
            account_type: AccountType::Premium,
            created_at_ms: 1_700_000_000_000,
        }
    }

    #[tokio::test]
    async fn test_export_accounts_writes_encrypted_bundle_with_all_entries() {
        let repo = Arc::new(InMemoryAccountRepo::new());
        let creds = Arc::new(FakeAccountCredentialStore::new());
        let codec: Arc<dyn PassphraseCodec> = Arc::new(FakePassphraseCodec);
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_account_bus(
            repo.clone(),
            creds,
            events.clone(),
            None,
            Some(codec.clone()),
        );

        bus.handle_add_account(add_command("real-debrid", "alice", "rd-pw"))
            .await
            .unwrap();
        bus.handle_add_account(add_command("alldebrid", "bob", "ad-pw"))
            .await
            .unwrap();

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("accounts.vortex.bin");

        let outcome = bus
            .handle_export_accounts(ExportAccountsCommand {
                path: path.clone(),
                passphrase: "secret-pass".into(),
            })
            .await
            .expect("export ok");
        assert_eq!(outcome.count, 2);
        assert_eq!(outcome.path, path);

        let bytes = std::fs::read(&path).expect("file present");
        let decrypted = codec.open("secret-pass", &bytes).expect("decode");
        let envelope: ExportEnvelope = serde_json::from_slice(&decrypted).unwrap();
        assert_eq!(envelope.accounts.len(), 2);
        let services: Vec<&str> = envelope
            .accounts
            .iter()
            .map(|e| e.service_name.as_str())
            .collect();
        assert!(services.contains(&"real-debrid"));
        assert!(services.contains(&"alldebrid"));

        // Wrong passphrase must fail at the codec layer — the bundle is
        // not readable without the original key.
        let wrong = codec.open("not-the-pass", &bytes);
        assert!(wrong.is_err());

        assert!(
            events
                .snapshot()
                .iter()
                .any(|e| matches!(e, DomainEvent::AccountsExported { count: 2 }))
        );
    }

    #[tokio::test]
    async fn test_export_accounts_empty_passphrase_rejected() {
        let repo = Arc::new(InMemoryAccountRepo::new());
        let creds = Arc::new(FakeAccountCredentialStore::new());
        let codec: Arc<dyn PassphraseCodec> = Arc::new(FakePassphraseCodec);
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_account_bus(repo, creds, events, None, Some(codec));

        let err = bus
            .handle_export_accounts(ExportAccountsCommand {
                path: std::env::temp_dir().join("vortex-export.bin"),
                passphrase: "".into(),
            })
            .await
            .expect_err("empty pass");
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[tokio::test]
    async fn test_export_accounts_missing_password_aborts_with_storage_error() {
        let repo = Arc::new(InMemoryAccountRepo::new());
        let creds = Arc::new(FakeAccountCredentialStore::new());
        let codec: Arc<dyn PassphraseCodec> = Arc::new(FakePassphraseCodec);
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_account_bus(repo.clone(), creds.clone(), events, None, Some(codec));

        // Persist one account, then evict its keyring password.
        let id = bus
            .handle_add_account(add_command("real-debrid", "alice", "pw"))
            .await
            .unwrap();
        creds.delete_password(&id).unwrap();

        let dir = TempDir::new().unwrap();
        let err = bus
            .handle_export_accounts(ExportAccountsCommand {
                path: dir.path().join("accounts.bin"),
                passphrase: "any".into(),
            })
            .await
            .expect_err("password missing");
        assert!(matches!(err, AppError::Storage(_)));
        // The bundle file must NOT exist when export fails mid-flight.
        assert!(!dir.path().join("accounts.bin").exists());
    }
}
