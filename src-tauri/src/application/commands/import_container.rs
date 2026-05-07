//! Handler for [`ImportContainerCommand`](super::ImportContainerCommand).

use std::path::Path;

use serde::Deserialize;

use crate::application::command_bus::CommandBus;
use crate::application::commands::CreatePackageCommand;
use crate::application::error::AppError;
use crate::domain::model::package::{PackageId, PackageSourceType};

const ALLOWED_EXTENSIONS: &[&str] = &["dlc", "ccf", "rsdf", "metalink", "meta4"];

/// Cap so a hostile caller cannot force the host to allocate an
/// arbitrarily large plugin input buffer.
pub const MAX_CONTAINER_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ImportContainerOutcome {
    pub format: String,
    pub urls: Vec<String>,
    pub package_id: PackageId,
    pub package_name: String,
}

#[derive(Debug, Deserialize)]
struct DecryptResponse {
    format: String,
    #[serde(default)]
    links: Vec<DecryptLink>,
}

#[derive(Debug, Deserialize)]
struct DecryptLink {
    url: String,
}

impl CommandBus {
    pub async fn handle_import_container(
        &self,
        cmd: super::ImportContainerCommand,
    ) -> Result<ImportContainerOutcome, AppError> {
        let file_name = cmd.file_name.trim();
        if file_name.is_empty() {
            return Err(AppError::Validation(
                "container file name must not be empty".into(),
            ));
        }
        if !has_allowed_extension(file_name) {
            return Err(AppError::Validation(format!(
                "unsupported container extension for '{file_name}'"
            )));
        }
        if cmd.file_bytes.is_empty() {
            return Err(AppError::Validation("container file is empty".into()));
        }
        if cmd.file_bytes.len() > MAX_CONTAINER_BYTES {
            return Err(AppError::Validation(format!(
                "container file too large: {} bytes (max {MAX_CONTAINER_BYTES})",
                cmd.file_bytes.len()
            )));
        }

        let json = self
            .plugin_loader()
            .decrypt_container(&cmd.file_bytes)
            .map_err(AppError::from)?;
        let response: DecryptResponse = serde_json::from_str(&json)
            .map_err(|e| AppError::Plugin(format!("invalid container plugin response: {e}")))?;

        let urls: Vec<String> = response
            .links
            .into_iter()
            .filter_map(|l| {
                let trimmed = l.url.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            })
            .collect();
        if urls.is_empty() {
            return Err(AppError::Plugin("container decoded with zero links".into()));
        }

        let package_name = file_name.to_string();
        let package_id = self
            .handle_create_package(CreatePackageCommand {
                name: package_name.clone(),
                source_type: PackageSourceType::Container,
                folder_path: None,
                created_at_ms: cmd.created_at_ms,
            })
            .await?;

        Ok(ImportContainerOutcome {
            format: response.format,
            urls,
            package_id,
            package_name,
        })
    }
}

fn has_allowed_extension(file_name: &str) -> bool {
    Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let lower = e.to_ascii_lowercase();
            ALLOWED_EXTENSIONS.iter().any(|ext| *ext == lower)
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::super::ImportContainerCommand;
    use super::has_allowed_extension;
    use crate::application::commands::tests_support::{
        CapturingEventBus, InMemoryCredentialStore, InMemoryDownloadRepo, InMemoryPackageRepo,
        StubPluginLoader, build_package_bus_with_plugin_loader,
    };
    use crate::application::error::AppError;
    use crate::domain::error::DomainError;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::package::PackageSourceType;
    use crate::domain::ports::driven::{PackageRepository, PluginLoader};

    struct FakeContainerPluginLoader {
        response: Mutex<Result<String, DomainError>>,
        last_input: Mutex<Option<Vec<u8>>>,
    }

    impl FakeContainerPluginLoader {
        fn ok(json: &str) -> Arc<Self> {
            Arc::new(Self {
                response: Mutex::new(Ok(json.to_string())),
                last_input: Mutex::new(None),
            })
        }

        fn err(err: DomainError) -> Arc<Self> {
            Arc::new(Self {
                response: Mutex::new(Err(err)),
                last_input: Mutex::new(None),
            })
        }
    }

    impl PluginLoader for FakeContainerPluginLoader {
        fn load(
            &self,
            _: &crate::domain::model::plugin::PluginManifest,
        ) -> Result<(), DomainError> {
            Ok(())
        }
        fn unload(&self, _: &str) -> Result<(), DomainError> {
            Ok(())
        }
        fn resolve_url(
            &self,
            _: &str,
        ) -> Result<Option<crate::domain::model::plugin::PluginInfo>, DomainError> {
            Ok(None)
        }
        fn list_loaded(
            &self,
        ) -> Result<Vec<crate::domain::model::plugin::PluginInfo>, DomainError> {
            Ok(vec![])
        }
        fn set_enabled(&self, _: &str, _: bool) -> Result<(), DomainError> {
            Ok(())
        }
        fn decrypt_container(&self, bytes: &[u8]) -> Result<String, DomainError> {
            *self.last_input.lock().unwrap() = Some(bytes.to_vec());
            match &*self.response.lock().unwrap() {
                Ok(s) => Ok(s.clone()),
                Err(e) => Err(e.clone()),
            }
        }
    }

    fn metalink_response_json() -> &'static str {
        r#"{
            "format": "metalink",
            "links": [
                {"url": "https://primary.example.com/file.iso", "filename": "file.iso", "mirrors": ["https://mirror.example.com/file.iso"]},
                {"url": "https://second.example.com/extra.bin"}
            ]
        }"#
    }

    fn cmd(file_name: &str, bytes: Vec<u8>) -> ImportContainerCommand {
        ImportContainerCommand {
            file_name: file_name.into(),
            file_bytes: bytes,
            created_at_ms: 1_700_000_000_000,
        }
    }

    struct Fixture {
        bus: crate::application::command_bus::CommandBus,
        pkg_repo: Arc<InMemoryPackageRepo>,
        events: Arc<CapturingEventBus>,
    }

    fn fixture(loader: Arc<dyn PluginLoader>) -> Fixture {
        let pkg_repo = Arc::new(InMemoryPackageRepo::new());
        let creds = Arc::new(InMemoryCredentialStore::new());
        let dl_repo = Arc::new(InMemoryDownloadRepo::new());
        let events = Arc::new(CapturingEventBus::new());
        let bus = build_package_bus_with_plugin_loader(
            pkg_repo.clone(),
            creds,
            events.clone(),
            dl_repo,
            loader,
        );
        Fixture {
            bus,
            pkg_repo,
            events,
        }
    }

    fn stub_fixture() -> Fixture {
        fixture(Arc::new(StubPluginLoader))
    }

    #[tokio::test]
    async fn test_import_container_creates_package_and_returns_urls() {
        let loader = FakeContainerPluginLoader::ok(metalink_response_json());
        let f = fixture(loader.clone());

        let outcome = f
            .bus
            .handle_import_container(cmd("Apache.metalink", b"<metalink/>".to_vec()))
            .await
            .expect("import ok");

        assert_eq!(outcome.format, "metalink");
        assert_eq!(outcome.package_name, "Apache.metalink");
        assert_eq!(
            outcome.urls,
            vec![
                "https://primary.example.com/file.iso",
                "https://second.example.com/extra.bin"
            ]
        );
        let stored = f.pkg_repo.find_by_id(&outcome.package_id).unwrap().unwrap();
        assert_eq!(stored.source_type(), PackageSourceType::Container);
        assert_eq!(stored.name(), "Apache.metalink");
        let captured = loader.last_input.lock().unwrap();
        assert_eq!(captured.as_deref(), Some(b"<metalink/>".as_slice()));
        let snapshot = f.events.snapshot();
        assert!(snapshot.iter().any(|e| matches!(
            e,
            DomainEvent::PackageCreated { id, name } if id == &outcome.package_id && name == "Apache.metalink"
        )));
    }

    #[tokio::test]
    async fn test_import_container_rejects_blank_file_name() {
        let err = stub_fixture()
            .bus
            .handle_import_container(cmd("   ", b"any".to_vec()))
            .await
            .expect_err("blank rejected");
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[tokio::test]
    async fn test_import_container_rejects_unsupported_extension() {
        let err = stub_fixture()
            .bus
            .handle_import_container(cmd("malicious.exe", b"data".to_vec()))
            .await
            .expect_err("rejected");
        assert!(
            matches!(&err, AppError::Validation(msg) if msg.contains("unsupported")),
            "wrong error: {err:?}"
        );
    }

    #[tokio::test]
    async fn test_import_container_rejects_empty_bytes() {
        let err = stub_fixture()
            .bus
            .handle_import_container(cmd("foo.dlc", vec![]))
            .await
            .expect_err("rejected");
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[tokio::test]
    async fn test_import_container_rejects_oversize_payload() {
        let huge = vec![0u8; super::MAX_CONTAINER_BYTES + 1];
        let err = stub_fixture()
            .bus
            .handle_import_container(cmd("big.dlc", huge))
            .await
            .expect_err("rejected");
        assert!(matches!(&err, AppError::Validation(msg) if msg.contains("too large")));
    }

    #[tokio::test]
    async fn test_import_container_propagates_plugin_not_found() {
        let loader =
            FakeContainerPluginLoader::err(DomainError::NotFound("no container plugin".into()));
        let err = fixture(loader)
            .bus
            .handle_import_container(cmd("foo.dlc", b"DLC".to_vec()))
            .await
            .expect_err("propagates");
        assert!(matches!(err, AppError::Domain(DomainError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_import_container_rejects_zero_link_response() {
        let loader = FakeContainerPluginLoader::ok(r#"{"format":"dlc","links":[]}"#);
        let f = fixture(loader);
        let err = f
            .bus
            .handle_import_container(cmd("empty.dlc", b"DLC".to_vec()))
            .await
            .expect_err("zero links rejected");
        assert!(matches!(&err, AppError::Plugin(msg) if msg.contains("zero links")));
        assert!(f.pkg_repo.list().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_import_container_rejects_invalid_plugin_json() {
        let loader = FakeContainerPluginLoader::ok("not json");
        let err = fixture(loader)
            .bus
            .handle_import_container(cmd("foo.metalink", b"<x/>".to_vec()))
            .await
            .expect_err("rejected");
        assert!(matches!(err, AppError::Plugin(_)));
    }

    #[test]
    fn test_has_allowed_extension_accepts_each_format() {
        for ext in ["dlc", "ccf", "rsdf", "metalink", "meta4"] {
            assert!(has_allowed_extension(&format!("foo.{ext}")));
            assert!(has_allowed_extension(&format!(
                "FOO.{}",
                ext.to_ascii_uppercase()
            )));
        }
    }

    #[test]
    fn test_has_allowed_extension_rejects_unrelated() {
        assert!(!has_allowed_extension("foo.txt"));
        assert!(!has_allowed_extension("dlc"));
        assert!(!has_allowed_extension(""));
    }
}
