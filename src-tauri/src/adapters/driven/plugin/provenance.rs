use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::domain::error::DomainError;
use crate::domain::ports::driven::plugin_store_client::OfficialPluginProvenance;

use super::capabilities::HostFunctionGrants;
use super::tesseract_broker::OCR_PLUGIN_NAME;

type ParentSync = fn(&Path) -> std::io::Result<()>;

enum PersistResult {
    Committed,
    CommittedWithDurabilityError(DomainError),
}

impl PersistResult {
    fn warn_if_not_durable(self) {
        if let Self::CommittedWithDurabilityError(error) = self {
            tracing::warn!(
                %error,
                "plugin provenance record committed without durable directory sync"
            );
        }
    }

    fn into_result(self) -> Result<(), DomainError> {
        match self {
            Self::Committed => Ok(()),
            Self::CommittedWithDurabilityError(error) => Err(error),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistedProvenance {
    version: String,
    wasm_sha256: String,
    manifest_sha256: String,
}

#[derive(Default, Deserialize, Serialize)]
struct ProvenanceFile {
    plugins: HashMap<String, PersistedProvenance>,
}

/// Host-owned trust state. Its path is supplied by the loader and must live
/// outside the directory writable through plugin installation/hot reload.
pub(super) struct OfficialProvenanceStore {
    path: PathBuf,
    entries: Mutex<HashMap<String, PersistedProvenance>>,
    sync_parent: ParentSync,
}

impl OfficialProvenanceStore {
    pub(super) fn new(path: PathBuf) -> Result<Self, DomainError> {
        Self::new_with_parent_sync(path, sync_parent_directory)
    }

    fn new_with_parent_sync(path: PathBuf, sync_parent: ParentSync) -> Result<Self, DomainError> {
        let entries = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<ProvenanceFile>(&bytes) {
                Ok(file) => file.plugins,
                Err(error) => {
                    tracing::warn!(
                        path = %path.display(),
                        %error,
                        "ignoring invalid plugin provenance state"
                    );
                    HashMap::new()
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(error) => {
                return Err(DomainError::PluginError(format!(
                    "failed to read plugin provenance '{}': {error}",
                    path.display()
                )));
            }
        };
        Ok(Self {
            path,
            entries: Mutex::new(entries),
            sync_parent,
        })
    }

    pub(super) fn record(&self, provenance: &OfficialPluginProvenance) -> Result<(), DomainError> {
        validate_digest(&provenance.wasm_sha256)?;
        validate_digest(&provenance.manifest_sha256)?;
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut updated_entries = entries.clone();
        updated_entries.insert(
            provenance.name.clone(),
            PersistedProvenance {
                version: provenance.version.clone(),
                wasm_sha256: provenance.wasm_sha256.to_ascii_lowercase(),
                manifest_sha256: provenance.manifest_sha256.to_ascii_lowercase(),
            },
        );
        let persist_result = self.persist(&updated_entries)?;
        *entries = updated_entries;
        // If directory sync fails after rename, startup checksum revalidation
        // still makes a lost record fail closed. Do not abort an install whose
        // files and trust record are already committed. Revocation remains
        // stricter because it runs before an untrusted local replacement.
        persist_result.warn_if_not_durable();
        Ok(())
    }

    pub(super) fn grants_for(
        &self,
        name: &str,
        version: &str,
        wasm_bytes: &[u8],
        manifest_bytes: &[u8],
    ) -> HostFunctionGrants {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let verified = entries.get(name).is_some_and(|entry| {
            entry.version == version
                && entry.wasm_sha256 == digest(wasm_bytes)
                && entry.manifest_sha256 == digest(manifest_bytes)
        });
        HostFunctionGrants {
            ytdlp: verified,
            tesseract: verified && name == OCR_PLUGIN_NAME,
        }
    }

    pub(super) fn revoke(&self, name: &str) -> Result<(), DomainError> {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut updated_entries = entries.clone();
        if updated_entries.remove(name).is_some() {
            let persist_result = self.persist(&updated_entries)?;
            *entries = updated_entries;
            return persist_result.into_result();
        }
        Ok(())
    }

    fn persist(
        &self,
        entries: &HashMap<String, PersistedProvenance>,
    ) -> Result<PersistResult, DomainError> {
        let parent = self.path.parent().ok_or_else(|| {
            DomainError::PluginError(format!(
                "plugin provenance path '{}' has no parent",
                self.path.display()
            ))
        })?;
        std::fs::create_dir_all(parent).map_err(|error| {
            DomainError::PluginError(format!(
                "failed to create plugin provenance directory '{}': {error}",
                parent.display()
            ))
        })?;
        let payload = serde_json::to_vec_pretty(&ProvenanceFile {
            plugins: entries.clone(),
        })
        .map_err(|error| DomainError::PluginError(format!("provenance encode failed: {error}")))?;
        let file_name = self.path.file_name().ok_or_else(|| {
            DomainError::PluginError(format!(
                "plugin provenance path '{}' has no file name",
                self.path.display()
            ))
        })?;
        let temp_path = parent.join(format!(
            ".{}.{}.tmp",
            file_name.to_string_lossy(),
            Uuid::new_v4()
        ));
        let result = self.persist_atomically(&temp_path, parent, &payload);
        if result.is_err() {
            let _ = std::fs::remove_file(&temp_path);
        }
        result
    }

    fn persist_atomically(
        &self,
        temp_path: &std::path::Path,
        parent: &std::path::Path,
        payload: &[u8],
    ) -> Result<PersistResult, DomainError> {
        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        {
            let mut file = options.open(temp_path).map_err(|error| {
                DomainError::PluginError(format!(
                    "failed to create plugin provenance temp file '{}': {error}",
                    temp_path.display()
                ))
            })?;
            file.write_all(payload).map_err(|error| {
                DomainError::PluginError(format!(
                    "failed to write plugin provenance temp file '{}': {error}",
                    temp_path.display()
                ))
            })?;
            file.sync_all().map_err(|error| {
                DomainError::PluginError(format!(
                    "failed to sync plugin provenance temp file '{}': {error}",
                    temp_path.display()
                ))
            })?;
        }
        std::fs::rename(temp_path, &self.path).map_err(|error| {
            DomainError::PluginError(format!(
                "failed to replace plugin provenance '{}': {error}",
                self.path.display()
            ))
        })?;
        match (self.sync_parent)(parent) {
            Ok(()) => Ok(PersistResult::Committed),
            Err(error) => Ok(PersistResult::CommittedWithDurabilityError(
                DomainError::PluginError(format!(
                    "failed to sync plugin provenance directory '{}': {error}",
                    parent.display()
                )),
            )),
        }
    }
}

#[cfg(unix)]
fn sync_parent_directory(parent: &Path) -> std::io::Result<()> {
    std::fs::File::open(parent).and_then(|directory| directory.sync_all())
}

#[cfg(not(unix))]
fn sync_parent_directory(_: &Path) -> std::io::Result<()> {
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn validate_digest(value: &str) -> Result<(), DomainError> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(DomainError::ValidationError(
            "official plugin provenance requires a 64-character SHA-256 digest".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::io::Read;
    use tempfile::TempDir;

    fn fail_parent_sync(_: &Path) -> std::io::Result<()> {
        Err(std::io::Error::other("injected parent sync failure"))
    }

    #[test]
    fn test_verified_provenance_persists_and_grants_unchanged_plugin() {
        let temp = TempDir::new().unwrap();
        let state_path = temp
            .path()
            .join("host-state")
            .join("plugin-provenance.json");
        let wasm = b"official wasm";
        let manifest = b"official manifest";
        let provenance = OfficialPluginProvenance {
            name: "vortex-mod-youtube".into(),
            version: "1.0.0".into(),
            wasm_sha256: digest(wasm),
            manifest_sha256: digest(manifest),
        };

        OfficialProvenanceStore::new(state_path.clone())
            .unwrap()
            .record(&provenance)
            .unwrap();
        let reloaded = OfficialProvenanceStore::new(state_path).unwrap();

        let grants = reloaded.grants_for("vortex-mod-youtube", "1.0.0", wasm, manifest);
        assert!(grants.ytdlp);
    }

    #[test]
    fn test_verified_provenance_denies_tampered_wasm() {
        let temp = TempDir::new().unwrap();
        let state_path = temp.path().join("plugin-provenance.json");
        let wasm = b"official wasm";
        let manifest = b"official manifest";
        let provenance = OfficialPluginProvenance {
            name: "vortex-mod-youtube".into(),
            version: "1.0.0".into(),
            wasm_sha256: digest(wasm),
            manifest_sha256: digest(manifest),
        };
        let store = OfficialProvenanceStore::new(state_path).unwrap();
        store.record(&provenance).unwrap();

        let grants = store.grants_for("vortex-mod-youtube", "1.0.0", b"tampered wasm", manifest);

        assert!(!grants.ytdlp);
    }

    #[test]
    fn test_verified_provenance_denies_tampered_manifest() {
        let temp = TempDir::new().unwrap();
        let state_path = temp.path().join("plugin-provenance.json");
        let wasm = b"official wasm";
        let manifest = b"official manifest";
        let provenance = OfficialPluginProvenance {
            name: "vortex-mod-youtube".into(),
            version: "1.0.0".into(),
            wasm_sha256: digest(wasm),
            manifest_sha256: digest(manifest),
        };
        let store = OfficialProvenanceStore::new(state_path).unwrap();
        store.record(&provenance).unwrap();

        let grants = store.grants_for("vortex-mod-youtube", "1.0.0", wasm, b"tampered manifest");

        assert!(!grants.ytdlp);
    }

    #[test]
    fn test_failed_persistence_does_not_leave_an_in_memory_grant() {
        let temp = TempDir::new().unwrap();
        let state_dir = temp.path().join("host-state");
        std::fs::create_dir(&state_dir).unwrap();
        let state_path = state_dir.join("plugin-provenance.json");
        let wasm = b"official wasm";
        let manifest = b"official manifest";
        let provenance = OfficialPluginProvenance {
            name: "vortex-mod-youtube".into(),
            version: "1.0.0".into(),
            wasm_sha256: digest(wasm),
            manifest_sha256: digest(manifest),
        };
        let store = OfficialProvenanceStore::new(state_path).unwrap();
        std::fs::rename(&state_dir, temp.path().join("moved-host-state")).unwrap();
        std::fs::write(&state_dir, b"file").unwrap();

        let result = store.record(&provenance);

        assert!(result.is_err());
        assert!(
            !store
                .grants_for("vortex-mod-youtube", "1.0.0", wasm, manifest)
                .ytdlp
        );
    }

    #[test]
    fn test_failed_revoke_keeps_in_memory_and_persisted_grant_consistent() {
        let temp = TempDir::new().unwrap();
        let state_dir = temp.path().join("host-state");
        std::fs::create_dir(&state_dir).unwrap();
        let state_path = state_dir.join("plugin-provenance.json");
        let wasm = b"official wasm";
        let manifest = b"official manifest";
        let provenance = OfficialPluginProvenance {
            name: "vortex-mod-youtube".into(),
            version: "1.0.0".into(),
            wasm_sha256: digest(wasm),
            manifest_sha256: digest(manifest),
        };
        let store = OfficialProvenanceStore::new(state_path).unwrap();
        store.record(&provenance).unwrap();
        let moved_state_dir = temp.path().join("moved-host-state");
        std::fs::rename(&state_dir, &moved_state_dir).unwrap();
        std::fs::write(&state_dir, b"file").unwrap();

        let result = store.revoke("vortex-mod-youtube");

        assert!(result.is_err());
        assert!(
            store
                .grants_for("vortex-mod-youtube", "1.0.0", wasm, manifest)
                .ytdlp
        );
        let reloaded =
            OfficialProvenanceStore::new(moved_state_dir.join("plugin-provenance.json")).unwrap();
        assert!(
            reloaded
                .grants_for("vortex-mod-youtube", "1.0.0", wasm, manifest)
                .ytdlp
        );
    }

    #[test]
    fn test_committed_sync_errors_allow_record_but_fail_revoke_without_divergence() {
        let temp = TempDir::new().unwrap();
        let state_path = temp.path().join("plugin-provenance.json");
        let wasm = b"official wasm";
        let manifest = b"official manifest";
        let provenance = OfficialPluginProvenance {
            name: "vortex-mod-youtube".into(),
            version: "1.0.0".into(),
            wasm_sha256: digest(wasm),
            manifest_sha256: digest(manifest),
        };
        let store =
            OfficialProvenanceStore::new_with_parent_sync(state_path.clone(), fail_parent_sync)
                .unwrap();

        let record_result = store.record(&provenance);

        assert!(record_result.is_ok());
        assert!(
            store
                .grants_for("vortex-mod-youtube", "1.0.0", wasm, manifest)
                .ytdlp
        );
        let reloaded = OfficialProvenanceStore::new(state_path.clone()).unwrap();
        assert!(
            reloaded
                .grants_for("vortex-mod-youtube", "1.0.0", wasm, manifest)
                .ytdlp
        );

        let revoke_result = store.revoke("vortex-mod-youtube");

        assert!(revoke_result.is_err());
        assert!(
            !store
                .grants_for("vortex-mod-youtube", "1.0.0", wasm, manifest)
                .ytdlp
        );
        let reloaded = OfficialProvenanceStore::new(state_path).unwrap();
        assert!(
            !reloaded
                .grants_for("vortex-mod-youtube", "1.0.0", wasm, manifest)
                .ytdlp
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_persist_replaces_live_state_atomically() {
        let temp = TempDir::new().unwrap();
        let state_path = temp.path().join("plugin-provenance.json");
        let store = OfficialProvenanceStore::new(state_path.clone()).unwrap();
        let first = OfficialPluginProvenance {
            name: "vortex-mod-youtube".into(),
            version: "1.0.0".into(),
            wasm_sha256: digest(b"first wasm"),
            manifest_sha256: digest(b"first manifest"),
        };
        store.record(&first).unwrap();
        let mut open_snapshot = std::fs::File::open(&state_path).unwrap();
        let second = OfficialPluginProvenance {
            name: first.name.clone(),
            version: "2.0.0".into(),
            wasm_sha256: digest(b"second wasm"),
            manifest_sha256: digest(b"second manifest"),
        };

        store.record(&second).unwrap();

        let mut snapshot_json = String::new();
        open_snapshot.read_to_string(&mut snapshot_json).unwrap();
        let snapshot: ProvenanceFile = serde_json::from_str(&snapshot_json).unwrap();
        assert_eq!(snapshot.plugins["vortex-mod-youtube"].version, "1.0.0");
        let current = OfficialProvenanceStore::new(state_path).unwrap();
        assert!(
            current
                .grants_for(
                    "vortex-mod-youtube",
                    "2.0.0",
                    b"second wasm",
                    b"second manifest"
                )
                .ytdlp
        );
    }
}
