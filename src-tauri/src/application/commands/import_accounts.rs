//! Handler for [`ImportAccountsCommand`](super::ImportAccountsCommand).
//!
//! Reads the bundle previously written by
//! [`ExportAccountsCommand`](super::ExportAccountsCommand), decrypts
//! it with the user-supplied passphrase, validates every entry,
//! and persists each one — both the SQLite row and the keyring
//! password — in a single best-effort batch.
//!
//! A wrong passphrase or any payload-level corruption aborts before
//! any row or keyring entry is written, so the keyring never ends up
//! holding orphaned credentials.

use std::collections::HashSet;

use uuid::Uuid;

use super::ImportAccountsOutcome;
use super::export_accounts::{EXPORT_VERSION, ExportEnvelope};
use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;
use crate::domain::model::account::{Account, AccountId};

impl CommandBus {
    pub async fn handle_import_accounts(
        &self,
        cmd: super::ImportAccountsCommand,
    ) -> Result<ImportAccountsOutcome, AppError> {
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

        let path = cmd.path.clone();
        let bytes = tokio::task::spawn_blocking(move || std::fs::read(&path))
            .await
            .map_err(|e| AppError::Storage(format!("import read task failed: {e}")))?
            .map_err(|e| AppError::Storage(format!("import read failed: {e}")))?;

        let plaintext = codec.open(&cmd.passphrase, &bytes)?;
        let envelope: ExportEnvelope = serde_json::from_slice(&plaintext)
            .map_err(|e| AppError::Validation(format!("export bundle is not valid JSON: {e}")))?;
        if envelope.version != EXPORT_VERSION {
            return Err(AppError::Validation(format!(
                "unsupported export version: {} (expected {})",
                envelope.version, EXPORT_VERSION
            )));
        }

        // Validate every entry up-front so a malformed row aborts
        // before any side effects. Trim service / username so duplicate
        // detection matches the same normalisation used when accounts
        // are added through `add_account`.
        let mut prepared = Vec::with_capacity(envelope.accounts.len());
        for entry in &envelope.accounts {
            let service_name = entry.service_name.trim().to_string();
            let username = entry.username.trim().to_string();
            if service_name.is_empty() {
                return Err(AppError::Validation(
                    "import bundle has an account with empty service_name".into(),
                ));
            }
            if username.is_empty() {
                return Err(AppError::Validation(
                    "import bundle has an account with empty username".into(),
                ));
            }
            if entry.password.is_empty() {
                return Err(AppError::Validation(
                    "import bundle has an account with empty password".into(),
                ));
            }
            let kind = entry.parse_account_type()?;
            prepared.push((service_name, username, entry, kind));
        }

        // Seed the dedup set with every `(service, username)` pair
        // already in the repo so the first import iteration doesn't
        // touch them, and grow the set as we insert each new entry so
        // duplicates **inside the bundle itself** are also skipped.
        let mut seen: HashSet<(String, String)> = repo
            .list()?
            .into_iter()
            .map(|a| (a.service_name().to_string(), a.username().to_string()))
            .collect();
        let mut imported = 0u32;
        let mut skipped = 0u32;

        for (service_name, username, entry, kind) in prepared {
            if !seen.insert((service_name.clone(), username.clone())) {
                skipped += 1;
                continue;
            }

            let new_id = AccountId::new(Uuid::new_v4().to_string());
            let mut account =
                Account::new(new_id.clone(), service_name, username, kind, cmd.now_ms);
            if !entry.enabled {
                account.disable();
            }
            if let Some(t) = entry.traffic_left {
                account.set_traffic_left(t);
            }
            if let Some(t) = entry.traffic_total {
                account.set_traffic_total(t);
            }
            if let Some(v) = entry.valid_until {
                account.set_valid_until(v);
            }
            if let Some(v) = entry.last_validated {
                account.set_last_validated(v);
            }

            repo.save(&account)?;
            if let Err(e) = store.store_password(&new_id, &entry.password) {
                let _ = repo.delete(&new_id);
                return Err(e.into());
            }
            imported += 1;
        }

        self.event_bus()
            .publish(DomainEvent::AccountsImported { count: imported });

        Ok(ImportAccountsOutcome {
            path: cmd.path,
            imported,
            skipped_duplicates: skipped,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::TempDir;

    use super::super::{AddAccountCommand, ExportAccountsCommand, ImportAccountsCommand};
    use crate::application::commands::tests_support::{
        CapturingEventBus, FakeAccountCredentialStore, FakePassphraseCodec, InMemoryAccountRepo,
        build_account_bus,
    };
    use crate::application::error::AppError;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::account::AccountType;
    use crate::domain::ports::driven::{
        AccountCredentialStore, AccountRepository, PassphraseCodec,
    };

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
    async fn test_export_then_import_roundtrip_restores_every_account() {
        let dir = TempDir::new().unwrap();
        let bundle = dir.path().join("export.bin");

        // ── Source bus produces the bundle ──
        let src_repo = Arc::new(InMemoryAccountRepo::new());
        let src_creds = Arc::new(FakeAccountCredentialStore::new());
        let codec: Arc<dyn PassphraseCodec> = Arc::new(FakePassphraseCodec);
        let src_events = Arc::new(CapturingEventBus::new());
        let src = build_account_bus(
            src_repo.clone(),
            src_creds.clone(),
            src_events,
            None,
            Some(codec.clone()),
        );
        src.handle_add_account(add_command("real-debrid", "alice", "rd-pw"))
            .await
            .unwrap();
        src.handle_add_account(add_command("alldebrid", "bob", "ad-pw"))
            .await
            .unwrap();
        src.handle_export_accounts(ExportAccountsCommand {
            path: bundle.clone(),
            passphrase: "passw0rd".into(),
        })
        .await
        .unwrap();

        // ── Target bus imports the bundle ──
        let dst_repo = Arc::new(InMemoryAccountRepo::new());
        let dst_creds = Arc::new(FakeAccountCredentialStore::new());
        let dst_events = Arc::new(CapturingEventBus::new());
        let dst = build_account_bus(
            dst_repo.clone(),
            dst_creds.clone(),
            dst_events.clone(),
            None,
            Some(codec),
        );

        let outcome = dst
            .handle_import_accounts(ImportAccountsCommand {
                path: bundle,
                passphrase: "passw0rd".into(),
                now_ms: 2_000_000_000_000,
            })
            .await
            .expect("import ok");
        assert_eq!(outcome.imported, 2);
        assert_eq!(outcome.skipped_duplicates, 0);

        let imported = dst_repo.list().unwrap();
        assert_eq!(imported.len(), 2);
        let mut services: Vec<&str> = imported.iter().map(|a| a.service_name()).collect();
        services.sort();
        assert_eq!(services, vec!["alldebrid", "real-debrid"]);

        // Each imported account has its password landed in the keyring.
        for acc in &imported {
            let pw = dst_creds.get_password(acc.id()).unwrap();
            assert!(pw.is_some(), "password missing for {}", acc.id().as_str());
        }

        assert!(
            dst_events
                .snapshot()
                .iter()
                .any(|e| matches!(e, DomainEvent::AccountsImported { count: 2 }))
        );
    }

    #[tokio::test]
    async fn test_import_accounts_wrong_passphrase_fails_without_partial_insert() {
        let dir = TempDir::new().unwrap();
        let bundle = dir.path().join("export.bin");

        // Build a bundle with the correct passphrase.
        let src_repo = Arc::new(InMemoryAccountRepo::new());
        let src_creds = Arc::new(FakeAccountCredentialStore::new());
        let codec: Arc<dyn PassphraseCodec> = Arc::new(FakePassphraseCodec);
        let events = Arc::new(CapturingEventBus::new());
        let src = build_account_bus(
            src_repo,
            src_creds,
            events.clone(),
            None,
            Some(codec.clone()),
        );
        src.handle_add_account(add_command("real-debrid", "alice", "pw"))
            .await
            .unwrap();
        src.handle_export_accounts(ExportAccountsCommand {
            path: bundle.clone(),
            passphrase: "right-pass".into(),
        })
        .await
        .unwrap();

        // Try to import with the wrong passphrase.
        let dst_repo = Arc::new(InMemoryAccountRepo::new());
        let dst_creds = Arc::new(FakeAccountCredentialStore::new());
        let dst_events = Arc::new(CapturingEventBus::new());
        let dst = build_account_bus(
            dst_repo.clone(),
            dst_creds.clone(),
            dst_events,
            None,
            Some(codec),
        );

        let err = dst
            .handle_import_accounts(ImportAccountsCommand {
                path: bundle,
                passphrase: "wrong-pass".into(),
                now_ms: 0,
            })
            .await
            .expect_err("wrong passphrase");
        assert!(
            matches!(err, AppError::Domain(_) | AppError::Validation(_)),
            "expected crypto-style error, got {err:?}"
        );

        assert!(
            dst_repo.list().unwrap().is_empty(),
            "no row inserted on wrong passphrase"
        );
        assert_eq!(dst_creds.entry_count(), 0, "no keyring write either");
    }

    #[tokio::test]
    async fn test_import_accounts_skips_already_present_pairs() {
        let dir = TempDir::new().unwrap();
        let bundle = dir.path().join("export.bin");

        let src_repo = Arc::new(InMemoryAccountRepo::new());
        let src_creds = Arc::new(FakeAccountCredentialStore::new());
        let codec: Arc<dyn PassphraseCodec> = Arc::new(FakePassphraseCodec);
        let events = Arc::new(CapturingEventBus::new());
        let src = build_account_bus(
            src_repo,
            src_creds,
            events.clone(),
            None,
            Some(codec.clone()),
        );
        src.handle_add_account(add_command("real-debrid", "alice", "pw"))
            .await
            .unwrap();
        src.handle_export_accounts(ExportAccountsCommand {
            path: bundle.clone(),
            passphrase: "k".into(),
        })
        .await
        .unwrap();

        let dst_repo = Arc::new(InMemoryAccountRepo::new());
        let dst_creds = Arc::new(FakeAccountCredentialStore::new());
        let dst_events = Arc::new(CapturingEventBus::new());
        let dst = build_account_bus(
            dst_repo.clone(),
            dst_creds.clone(),
            dst_events,
            None,
            Some(codec),
        );
        // Pre-existing identical pair.
        dst.handle_add_account(add_command("real-debrid", "alice", "different"))
            .await
            .unwrap();

        let outcome = dst
            .handle_import_accounts(ImportAccountsCommand {
                path: bundle,
                passphrase: "k".into(),
                now_ms: 0,
            })
            .await
            .expect("import ok");
        assert_eq!(outcome.imported, 0);
        assert_eq!(outcome.skipped_duplicates, 1);
        assert_eq!(dst_repo.list().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_import_accounts_skips_in_bundle_duplicates() {
        use crate::application::commands::export_accounts::{
            EXPORT_VERSION, ExportEntry, ExportEnvelope,
        };
        use crate::domain::ports::driven::PassphraseCodec;

        // Hand-craft a bundle that contains two entries with the same
        // (service, username) pair so the dedup logic can be exercised
        // without going through `add_account`, which would refuse the
        // second entry up-front.
        let envelope = ExportEnvelope {
            version: EXPORT_VERSION,
            accounts: vec![
                ExportEntry {
                    service_name: "real-debrid".into(),
                    username: "alice".into(),
                    password: "pw1".into(),
                    account_type: "premium".into(),
                    enabled: true,
                    traffic_left: None,
                    traffic_total: None,
                    valid_until: None,
                    last_validated: None,
                },
                // Duplicate of the first entry — must be skipped.
                ExportEntry {
                    service_name: "  real-debrid  ".into(),
                    username: " alice ".into(),
                    password: "pw2".into(),
                    account_type: "premium".into(),
                    enabled: true,
                    traffic_left: None,
                    traffic_total: None,
                    valid_until: None,
                    last_validated: None,
                },
                ExportEntry {
                    service_name: "alldebrid".into(),
                    username: "bob".into(),
                    password: "pw3".into(),
                    account_type: "premium".into(),
                    enabled: true,
                    traffic_left: None,
                    traffic_total: None,
                    valid_until: None,
                    last_validated: None,
                },
            ],
        };
        let plaintext = serde_json::to_vec(&envelope).unwrap();
        let codec = FakePassphraseCodec;
        let ciphertext = codec.seal("k", &plaintext).unwrap();

        let dir = TempDir::new().unwrap();
        let bundle = dir.path().join("dup.bin");
        std::fs::write(&bundle, &ciphertext).unwrap();

        let dst_repo = Arc::new(InMemoryAccountRepo::new());
        let dst_creds = Arc::new(FakeAccountCredentialStore::new());
        let dst_events = Arc::new(CapturingEventBus::new());
        let dst = build_account_bus(
            dst_repo.clone(),
            dst_creds,
            dst_events,
            None,
            Some(Arc::new(FakePassphraseCodec) as Arc<dyn PassphraseCodec>),
        );

        let outcome = dst
            .handle_import_accounts(ImportAccountsCommand {
                path: bundle,
                passphrase: "k".into(),
                now_ms: 0,
            })
            .await
            .expect("import ok");

        assert_eq!(outcome.imported, 2, "first occurrence + alldebrid land");
        assert_eq!(
            outcome.skipped_duplicates, 1,
            "the second real-debrid/alice entry must be skipped"
        );
        assert_eq!(dst_repo.list().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_import_accounts_corrupted_payload_returns_validation() {
        let dir = TempDir::new().unwrap();
        let bundle = dir.path().join("garbage.bin");
        // Hand-craft a "valid" envelope under FakePassphraseCodec format
        // but with non-JSON plaintext so the JSON parse step fails.
        let codec = FakePassphraseCodec;
        let bytes = codec.seal("k", b"this is not json").unwrap();
        std::fs::write(&bundle, &bytes).unwrap();

        let dst_repo = Arc::new(InMemoryAccountRepo::new());
        let dst_creds = Arc::new(FakeAccountCredentialStore::new());
        let dst_events = Arc::new(CapturingEventBus::new());
        let dst = build_account_bus(
            dst_repo,
            dst_creds,
            dst_events,
            None,
            Some(Arc::new(FakePassphraseCodec) as Arc<dyn PassphraseCodec>),
        );

        let err = dst
            .handle_import_accounts(ImportAccountsCommand {
                path: bundle,
                passphrase: "k".into(),
                now_ms: 0,
            })
            .await
            .expect_err("corrupted");
        assert!(matches!(err, AppError::Validation(_)));
    }
}
