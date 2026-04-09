//! Hot-reload file system watcher for plugins using `notify` + tokio.

use std::path::PathBuf;
use std::sync::Arc;

use notify::{EventKind, RecursiveMode, Watcher};
use tokio::sync::mpsc;

use crate::domain::error::DomainError;
use crate::domain::ports::driven::PluginLoader;

use super::extism_loader::ExtismPluginLoader;
use super::manifest::parse_manifest;

pub struct PluginWatcher {
    _watcher: notify::RecommendedWatcher,
}

impl PluginWatcher {
    pub fn start(
        plugins_dir: PathBuf,
        loader: Arc<ExtismPluginLoader>,
    ) -> Result<Self, DomainError> {
        let (tx, mut rx) = mpsc::unbounded_channel::<notify::Event>();

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        })
        .map_err(|e| DomainError::PluginError(format!("watcher init failed: {e}")))?;

        watcher
            .watch(&plugins_dir, RecursiveMode::Recursive)
            .map_err(|e| DomainError::PluginError(format!("watch failed: {e}")))?;

        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                handle_fs_event(&event, &loader);
            }
        });

        Ok(Self { _watcher: watcher })
    }
}

fn handle_fs_event(event: &notify::Event, loader: &ExtismPluginLoader) {
    match event.kind {
        EventKind::Create(_) | EventKind::Modify(_) => {
            for path in &event.paths {
                if (is_plugin_toml(path) || is_wasm_file(path))
                    && let Some(plugin_dir) = path.parent()
                {
                    tracing::info!(
                        "plugin file changed, attempting load from {}",
                        plugin_dir.display()
                    );
                    match parse_manifest(plugin_dir) {
                        Ok((manifest, _)) => {
                            let name = manifest.info().name().to_string();
                            // Unload first if present (reload case). Ignore NotFound.
                            let _ = loader.unload(&name);
                            if let Err(e) = loader.load(&manifest) {
                                tracing::warn!("failed to load plugin '{name}': {e}");
                            } else {
                                tracing::info!("plugin '{name}' loaded");
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                "failed to parse manifest at {}: {e}",
                                plugin_dir.display()
                            );
                        }
                    }
                }
            }
        }
        EventKind::Remove(_) => {
            // Convention: plugin directory name must match the plugin's `name` field in
            // plugin.toml. On removal, the toml may already be gone so we rely on dir name.
            for path in &event.paths {
                if (is_plugin_toml(path) || is_wasm_file(path))
                    && let Some(plugin_dir) = path.parent()
                    && let Some(name) = plugin_dir.file_name().and_then(|n| n.to_str())
                {
                    // Try unload directly — unload returns NotFound if not loaded,
                    // which is fine (avoids TOCTOU between contains and unload).
                    tracing::info!("plugin file removed, unloading '{name}'");
                    if let Err(e) = loader.unload(name) {
                        tracing::debug!("unload '{name}' after removal: {e}");
                    }
                }
            }
        }
        _ => {}
    }
}

fn is_plugin_toml(path: &std::path::Path) -> bool {
    path.file_name().and_then(|n| n.to_str()) == Some("plugin.toml")
}

fn is_wasm_file(path: &std::path::Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("wasm")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::plugin::{PluginCategory, PluginInfo, PluginManifest};
    use notify::event::{CreateKind, RemoveKind};
    use std::io::Write;
    use tempfile::TempDir;

    fn make_loader(plugins_dir: &std::path::Path) -> Arc<ExtismPluginLoader> {
        Arc::new(ExtismPluginLoader::new(plugins_dir.to_path_buf()))
    }

    fn setup_plugin_dir(plugins_dir: &std::path::Path, name: &str) {
        let plugin_dir = plugins_dir.join(name);
        std::fs::create_dir_all(&plugin_dir).unwrap();

        let toml_content = format!(
            r#"[plugin]
name = "{name}"
version = "1.0.0"
category = "utility"
author = "tester"
description = "Test plugin"
"#
        );
        let mut f = std::fs::File::create(plugin_dir.join("plugin.toml")).unwrap();
        f.write_all(toml_content.as_bytes()).unwrap();

        let wasm_bytes: &[u8] = &[0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
        let mut wf = std::fs::File::create(plugin_dir.join(format!("{name}.wasm"))).unwrap();
        wf.write_all(wasm_bytes).unwrap();
    }

    fn make_manifest_for(name: &str) -> PluginManifest {
        let info = PluginInfo::new(
            name.to_string(),
            "1.0.0".to_string(),
            "desc".to_string(),
            "author".to_string(),
            PluginCategory::Utility,
        );
        PluginManifest::new(info)
    }

    #[test]
    fn test_handle_fs_event_create_loads_plugin() {
        let tmp = TempDir::new().unwrap();
        setup_plugin_dir(tmp.path(), "new-plugin");
        let loader = make_loader(tmp.path());

        let toml_path = tmp.path().join("new-plugin").join("plugin.toml");
        let event = notify::Event {
            kind: EventKind::Create(CreateKind::File),
            paths: vec![toml_path],
            attrs: Default::default(),
        };

        handle_fs_event(&event, &loader);
        assert!(loader.registry().contains("new-plugin"));
    }

    #[test]
    fn test_handle_fs_event_create_reloads_existing_plugin() {
        let tmp = TempDir::new().unwrap();
        setup_plugin_dir(tmp.path(), "hot-plugin");
        let loader = make_loader(tmp.path());

        // Pre-load
        loader.load(&make_manifest_for("hot-plugin")).unwrap();

        let toml_path = tmp.path().join("hot-plugin").join("plugin.toml");
        let event = notify::Event {
            kind: EventKind::Create(CreateKind::File),
            paths: vec![toml_path],
            attrs: Default::default(),
        };

        handle_fs_event(&event, &loader);
        // Should still be loaded after reload
        assert!(loader.registry().contains("hot-plugin"));
    }

    #[test]
    fn test_handle_fs_event_remove_unloads_plugin() {
        let tmp = TempDir::new().unwrap();
        setup_plugin_dir(tmp.path(), "doomed-plugin");
        let loader = make_loader(tmp.path());

        loader.load(&make_manifest_for("doomed-plugin")).unwrap();
        assert!(loader.registry().contains("doomed-plugin"));

        let wasm_path = tmp.path().join("doomed-plugin").join("doomed-plugin.wasm");
        let event = notify::Event {
            kind: EventKind::Remove(RemoveKind::File),
            paths: vec![wasm_path],
            attrs: Default::default(),
        };

        handle_fs_event(&event, &loader);
        assert!(!loader.registry().contains("doomed-plugin"));
    }

    #[test]
    fn test_handle_fs_event_unrelated_path_ignored() {
        let tmp = TempDir::new().unwrap();
        let loader = make_loader(tmp.path());

        let event = notify::Event {
            kind: EventKind::Create(CreateKind::File),
            paths: vec![tmp.path().join("some-dir").join("readme.txt")],
            attrs: Default::default(),
        };

        handle_fs_event(&event, &loader);
        assert!(loader.list_loaded().unwrap().is_empty());
    }

    #[test]
    fn test_is_plugin_toml() {
        assert!(is_plugin_toml(std::path::Path::new("/foo/bar/plugin.toml")));
        assert!(!is_plugin_toml(std::path::Path::new("/foo/bar/other.toml")));
        assert!(!is_plugin_toml(std::path::Path::new(
            "/foo/bar/plugin.wasm"
        )));
    }

    #[test]
    fn test_is_wasm_file() {
        assert!(is_wasm_file(std::path::Path::new("/foo/plugin.wasm")));
        assert!(!is_wasm_file(std::path::Path::new("/foo/plugin.toml")));
        assert!(!is_wasm_file(std::path::Path::new("/foo/plugin.wat")));
    }
}
