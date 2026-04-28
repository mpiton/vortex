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
use crate::domain::ports::driven::{AccountCredentialStore, AccountRepository};

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
        // Track every entry we successfully persist so a later failure
        // can roll the whole batch back. The reviewer specifically
        // flagged that returning mid-loop after a `repo.save` or
        // keyring failure left earlier accounts persisted, which made
        // retries non-deterministic (later attempts saw the partial
        // entries as duplicates).
        let mut imported_ids: Vec<AccountId> = Vec::new();
        let mut skipped = 0u32;

        for (service_name, username, entry, kind) in prepared {
            if !seen.insert((service_name.clone(), username.clone())) {
                skipped += 1;
                continue;
            }

            let new_id = AccountId::new(Uuid::new_v4().to_string());
            // Preserve the original `created_at` when the bundle
            // carries one so an export → import round-trip keeps the
            // chronology. Bundles produced by earlier versions omit
            // the field; fall back to `cmd.now_ms` in that case.
            let created_at = entry.created_at.unwrap_or(cmd.now_ms);
            let mut account =
                Account::new(new_id.clone(), service_name, username, kind, created_at);
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

            if let Err(e) = repo.save(&account) {
                rollback_imports(repo, store, &imported_ids);
                return Err(e.into());
            }
            // Track the id BEFORE attempting the keyring write so a
            // backend that partially writes the secret before failing
            // is still cleaned up by `rollback_imports`. The trait
            // contract for `store_password` does not promise "no side
            // effects on `Err`", so we treat any failed write as
            // potentially having left a stale entry behind.
            imported_ids.push(new_id.clone());
            if let Err(e) = store.store_password(&new_id, &entry.password) {
                rollback_imports(repo, store, &imported_ids);
                return Err(e.into());
            }
        }

        let imported = imported_ids.len() as u32;
        self.event_bus()
            .publish(DomainEvent::AccountsImported { count: imported });

        Ok(ImportAccountsOutcome {
            path: cmd.path,
            imported,
            skipped_duplicates: skipped,
        })
    }
}

/// Best-effort rollback of every account already imported in the
/// current batch. Failures are logged but never propagated — the
/// caller is already in an error path and we don't want a logging
/// failure to mask the real cause.
fn rollback_imports(
    repo: &dyn AccountRepository,
    store: &dyn AccountCredentialStore,
    ids: &[AccountId],
) {
    for id in ids {
        if let Err(e) = repo.delete(id) {
            tracing::warn!(
                account_id = %id.as_str(),
                error = %e,
                "failed to roll back imported account row after later import failure"
            );
        }
        if let Err(e) = store.delete_password(id) {
            tracing::warn!(
                account_id = %id.as_str(),
                error = %e,
                "failed to roll back imported keyring entry after later import failure"
            );
        }
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

        // The export bundle carries the source `created_at`, so the
        // importer must preserve it instead of stamping `cmd.now_ms`
        // (`2_000_000_000_000`) on every restored row.
        for acc in &imported {
            assert_eq!(
                acc.created_at(),
                1_700_000_000_000,
                "created_at must round-trip; source was 1.7e12, now_ms was 2e12"
            );
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
                    created_at: None,
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
                    created_at: None,
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
                    created_at: None,
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
    async fn test_import_accounts_rolls_back_all_entries_on_partial_failure() {
        use crate::application::commands::export_accounts::{
            EXPORT_VERSION, ExportEntry, ExportEnvelope,
        };
        use crate::domain::ports::driven::PassphraseCodec;

        // Build a bundle with three distinct (service, username) pairs.
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
                    created_at: None,
                },
                ExportEntry {
                    service_name: "alldebrid".into(),
                    username: "bob".into(),
                    password: "pw2".into(),
                    account_type: "premium".into(),
                    enabled: true,
                    traffic_left: None,
                    traffic_total: None,
                    valid_until: None,
                    last_validated: None,
                    created_at: None,
                },
                ExportEntry {
                    service_name: "uploaded".into(),
                    username: "carol".into(),
                    password: "pw3".into(),
                    account_type: "premium".into(),
                    enabled: true,
                    traffic_left: None,
                    traffic_total: None,
                    valid_until: None,
                    last_validated: None,
                    created_at: None,
                },
            ],
        };
        let plaintext = serde_json::to_vec(&envelope).unwrap();
        let codec = FakePassphraseCodec;
        let bytes = codec.seal("k", &plaintext).unwrap();

        let dir = TempDir::new().unwrap();
        let bundle = dir.path().join("partial.bin");
        std::fs::write(&bundle, &bytes).unwrap();

        // Keyring fails after the first successful write so entry 2's
        // `store_password` call returns an error mid-loop.
        let dst_repo = Arc::new(InMemoryAccountRepo::new());
        let dst_creds = Arc::new(FakeAccountCredentialStore::new().failing_after(1));
        let dst_events = Arc::new(CapturingEventBus::new());
        let dst = build_account_bus(
            dst_repo.clone(),
            dst_creds.clone(),
            dst_events.clone(),
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
            .expect_err("partial keyring failure must surface");
        assert!(
            matches!(err, AppError::Domain(_)),
            "expected storage error, got {err:?}"
        );

        assert!(
            dst_repo.list().unwrap().is_empty(),
            "all imported rows must be rolled back when one entry fails"
        );
        assert_eq!(
            dst_creds.entry_count(),
            0,
            "all imported keyring entries must be rolled back too"
        );
        assert!(
            !dst_events
                .snapshot()
                .iter()
                .any(|e| matches!(e, DomainEvent::AccountsImported { .. })),
            "no AccountsImported event when the import fails atomically"
        );
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
