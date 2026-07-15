use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::domain::error::DomainError;
use crate::domain::ports::driven::plugin_store_client::OfficialPluginProvenance;

use super::capabilities::HostFunctionGrants;

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
}

impl OfficialProvenanceStore {
    pub(super) fn new(path: PathBuf) -> Result<Self, DomainError> {
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
        self.persist(&updated_entries)?;
        *entries = updated_entries;
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
        HostFunctionGrants { ytdlp: verified }
    }

    pub(super) fn revoke(&self, name: &str) -> Result<(), DomainError> {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if entries.remove(name).is_some() {
            self.persist(&entries)?;
        }
        Ok(())
    }

    fn persist(&self, entries: &HashMap<String, PersistedProvenance>) -> Result<(), DomainError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                DomainError::PluginError(format!(
                    "failed to create plugin provenance directory '{}': {error}",
                    parent.display()
                ))
            })?;
        }
        let payload = serde_json::to_vec_pretty(&ProvenanceFile {
            plugins: entries.clone(),
        })
        .map_err(|error| DomainError::PluginError(format!("provenance encode failed: {error}")))?;
        let mut options = std::fs::OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&self.path).map_err(|error| {
            DomainError::PluginError(format!(
                "failed to write plugin provenance '{}': {error}",
                self.path.display()
            ))
        })?;
        file.write_all(&payload).map_err(|error| {
            DomainError::PluginError(format!(
                "failed to write plugin provenance '{}': {error}",
                self.path.display()
            ))
        })?;
        #[cfg(unix)]
        std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600)).map_err(
            |error| {
                DomainError::PluginError(format!(
                    "failed to secure plugin provenance '{}': {error}",
                    self.path.display()
                ))
            },
        )?;
        Ok(())
    }
}

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

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
    use tempfile::TempDir;

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
}
