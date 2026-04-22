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
                handle_upsert_event(path, loader);
            }
        }
        EventKind::Remove(_) => {
            for path in &event.paths {
                handle_remove_event(path, loader);
            }
        }
        _ => {}
    }
}

/// Identify the plugin name a filesystem event refers to, if any.
///
/// Only two path shapes qualify, both anchored directly under
/// `plugins_dir` so an event deeper in the tree can't unload a plugin
/// whose leaf name coincidentally matches some nested directory:
///
/// - **File event**: `plugins_dir/<name>/plugin.toml` or
///   `plugins_dir/<name>/<something>.wasm`. The plugin name is the
///   parent directory's file name.
/// - **Folder event**: `plugins_dir/<name>` itself. The plugin name is
///   the path's own file name. Used for backends that emit a single
///   `Remove(Folder)` when a plugin directory is deleted (macOS
///   FSEvents, sometimes Windows ReadDirectoryChangesW).
///
/// Any other path (nested subdirectories, sibling files in
/// `plugins_dir`, paths outside `plugins_dir`) returns `None`.
fn plugin_name_from_event_path<'a>(
    path: &'a std::path::Path,
    plugins_dir: &std::path::Path,
) -> Option<&'a str> {
    if is_plugin_toml(path) || is_wasm_file(path) {
        let parent = path.parent()?;
        if parent.parent() == Some(plugins_dir) {
            return parent.file_name().and_then(|n| n.to_str());
        }
    }
    if path.parent() == Some(plugins_dir) {
        return path.file_name().and_then(|n| n.to_str());
    }
    None
}

/// Handle a CREATE/MODIFY event for a plugin file.
///
/// Reloads the plugin whose directory contains the changed file. Paths
/// that aren't a plugin file directly under `plugins_dir/<name>/` are
/// ignored.
fn handle_upsert_event(path: &std::path::Path, loader: &ExtismPluginLoader) {
    let plugins_dir = loader.plugins_dir();
    let Some(name) = plugin_name_from_event_path(path, plugins_dir) else {
        return;
    };

    if loader.is_install_in_progress(name) {
        tracing::debug!(
            plugin = %name,
            "skipping watcher upsert while install is in flight",
        );
        return;
    }

    let plugin_dir = plugins_dir.join(name);
    tracing::info!(
        "plugin file changed, attempting load from {}",
        plugin_dir.display()
    );
    match parse_manifest(&plugin_dir) {
        Ok((manifest, _)) => {
            let manifest_name = manifest.info().name().to_string();
            // Unload first if present (reload case). Ignore NotFound.
            let _ = loader.unload(&manifest_name);
            if let Err(e) = loader.load(&manifest) {
                tracing::warn!("failed to load plugin '{manifest_name}': {e}");
            } else {
                tracing::info!("plugin '{manifest_name}' loaded");
            }
        }
        Err(e) => {
            tracing::warn!("failed to parse manifest at {}: {e}", plugin_dir.display());
        }
    }
}

/// Handle a REMOVE event. Uses the same direct-under-`plugins_dir`
/// constraint as the upsert branch so a remove of a nested directory
/// whose leaf name happens to match a loaded plugin can't unload it.
///
/// Accepts both file-level removes (`plugins_dir/<name>/plugin.toml`
/// etc. — inotify) and folder-level removes (`plugins_dir/<name>` —
/// FSEvents, sometimes ReadDirectoryChangesW).
fn handle_remove_event(path: &std::path::Path, loader: &ExtismPluginLoader) {
    let plugins_dir = loader.plugins_dir();
    let Some(name) = plugin_name_from_event_path(path, plugins_dir) else {
        return;
    };

    if loader.is_install_in_progress(name) {
        tracing::debug!(
            plugin = %name,
            "skipping watcher remove while install is in flight",
        );
        return;
    }

    match loader.unload(name) {
        Ok(()) => tracing::info!("plugin removed, unloaded '{name}'"),
        Err(e) => tracing::debug!("unload '{name}' after removal: {e}"),
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
        use crate::adapters::driven::plugin::capabilities::SharedHostResources;
        Arc::new(
            ExtismPluginLoader::new(
                plugins_dir.to_path_buf(),
                Arc::new(SharedHostResources::new()),
            )
            .expect("test HTTP client"),
        )
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
    fn test_handle_fs_event_remove_nested_path_does_not_unload_plugin() {
        // A remove event for a path nested deeper than
        // `plugins_dir/<name>/` — even if its leaf happens to match a
        // loaded plugin — must NOT unload that plugin. The risk comes
        // from `RecursiveMode::Recursive`: any subdirectory that gets
        // created then deleted inside a plugin dir could coincidentally
        // share a plugin's name.
        let tmp = TempDir::new().unwrap();
        setup_plugin_dir(tmp.path(), "safe-plugin");
        let loader = make_loader(tmp.path());
        loader.load(&make_manifest_for("safe-plugin")).unwrap();
        assert!(loader.registry().contains("safe-plugin"));

        // plugins/safe-plugin/subdir/safe-plugin — leaf matches the
        // loaded plugin but the path isn't a direct child of plugins_dir.
        let nested_path = tmp
            .path()
            .join("safe-plugin")
            .join("subdir")
            .join("safe-plugin");
        let event = notify::Event {
            kind: EventKind::Remove(RemoveKind::Folder),
            paths: vec![nested_path],
            attrs: Default::default(),
        };

        handle_fs_event(&event, &loader);
        assert!(
            loader.registry().contains("safe-plugin"),
            "nested folder remove must not unload a plugin whose name it coincidentally shares"
        );
    }

    #[test]
    fn test_handle_fs_event_remove_folder_unloads_plugin() {
        // macOS FSEvents (and occasionally Windows ReadDirectoryChangesW) emit
        // a single Remove(Folder) event for the plugin directory itself rather
        // than per-file removes. The watcher must still unload the plugin in
        // that case.
        let tmp = TempDir::new().unwrap();
        setup_plugin_dir(tmp.path(), "folder-doomed");
        let loader = make_loader(tmp.path());

        loader.load(&make_manifest_for("folder-doomed")).unwrap();
        assert!(loader.registry().contains("folder-doomed"));

        let plugin_dir = tmp.path().join("folder-doomed");
        let event = notify::Event {
            kind: EventKind::Remove(RemoveKind::Folder),
            paths: vec![plugin_dir],
            attrs: Default::default(),
        };

        handle_fs_event(&event, &loader);
        assert!(!loader.registry().contains("folder-doomed"));
    }

    #[test]
    fn test_handle_fs_event_skips_create_when_install_in_progress() {
        // With the install flag set, the watcher must NOT load the plugin
        // even though the filesystem looks ready — the install handler
        // is the owner of the load/unload lifecycle in that window.
        let tmp = TempDir::new().unwrap();
        setup_plugin_dir(tmp.path(), "busy-plugin");
        let loader = make_loader(tmp.path());

        loader.mark_install_in_progress_for_testing("busy-plugin");

        let toml_path = tmp.path().join("busy-plugin").join("plugin.toml");
        let event = notify::Event {
            kind: EventKind::Create(CreateKind::File),
            paths: vec![toml_path],
            attrs: Default::default(),
        };

        handle_fs_event(&event, &loader);
        assert!(
            !loader.registry().contains("busy-plugin"),
            "watcher must not load a plugin while its install is in flight"
        );
    }

    #[test]
    fn test_handle_fs_event_skips_remove_when_install_in_progress() {
        // Critical path for the race that motivated the suppression:
        // `load_from_dir` calls `remove_dir_all` on the destination, which
        // fires REMOVE events after the handler has already re-inserted
        // the new version. If the watcher acts on them, it unloads what
        // the install just loaded — leaving the user with "success toast,
        // plugin not installed" state.
        let tmp = TempDir::new().unwrap();
        setup_plugin_dir(tmp.path(), "busy-plugin");
        let loader = make_loader(tmp.path());
        loader.load(&make_manifest_for("busy-plugin")).unwrap();
        assert!(loader.registry().contains("busy-plugin"));

        loader.mark_install_in_progress_for_testing("busy-plugin");

        let wasm_path = tmp.path().join("busy-plugin").join("busy-plugin.wasm");
        let event = notify::Event {
            kind: EventKind::Remove(RemoveKind::File),
            paths: vec![wasm_path],
            attrs: Default::default(),
        };

        handle_fs_event(&event, &loader);
        assert!(
            loader.registry().contains("busy-plugin"),
            "watcher must not unload a plugin while its install is in flight"
        );
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
