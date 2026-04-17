# Clear Completed & Failed Downloads — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add two bulk-clear buttons (completed & failed) to the Downloads toolbar with an optional "also delete files from disk" checkbox gated by a prominent red warning panel. Provide success/error toasts.

**Architecture:** One parameterised domain Command (`ClearDownloadsByStateCommand`) handled on `CommandBus`. Two thin Tauri IPC handlers restricting the state to `Completed` or `Error`. Domain-level guard rejects any non-terminal state. Frontend adds a reusable `ClearDownloadsDialog` and two buttons in `ActionsBar`. Sonner is installed once and wrapped by `src/lib/toast.ts` to avoid leaking the library everywhere.

**Tech Stack:** Rust (Tauri 2, tokio, tracing, thiserror), React 19, TypeScript, TanStack Query v5, Zustand, Tailwind 4, shadcn/ui (Radix Dialog + Checkbox + Separator + Button), sonner, Vitest + Testing Library, react-i18next.

**Spec:** `docs/superpowers/specs/2026-04-17-clear-completed-and-failed-downloads-design.md`

---

## File structure

### Create

- `src-tauri/src/application/commands/clear_downloads_by_state.rs` — handler + unit tests
- `src/views/DownloadsView/ClearDownloadsDialog.tsx` — reusable confirmation dialog
- `src/views/DownloadsView/__tests__/ClearDownloadsDialog.test.tsx`
- `src/lib/toast.ts` — sonner wrapper
- `src/views/DownloadsView/__tests__/ActionsBar.test.tsx` — upgraded (new suites added)

### Modify

- `src-tauri/src/application/commands/mod.rs` — declare new module + `ClearDownloadsByStateCommand` struct
- `src-tauri/src/adapters/driving/tauri_ipc.rs` — add `download_clear_completed` + `download_clear_failed`
- `src-tauri/src/lib.rs` — export new IPC handlers + register in `invoke_handler![...]`
- `src/views/DownloadsView/ActionsBar.tsx` — add buttons + separator + dialog state + toast calls
- `src/i18n/locales/en.json` + `src/i18n/locales/fr.json` — new keys under `downloads.actions.*` and `downloads.toast.*`
- `src/App.tsx` — mount `<Toaster />`
- `package.json` / `package-lock.json` — add `sonner`
- `CHANGELOG.md` — `[Unreleased] > Added` entry

---

## Task 1: Add `ClearDownloadsByStateCommand` struct

**Files:**
- Modify: `src-tauri/src/application/commands/mod.rs`

- [ ] **Step 1: Add the command struct**

Insert between `RemoveDownloadCommand` (line 114-118) and `ResolveLinksCommand`:

```rust
#[derive(Debug)]
pub struct ClearDownloadsByStateCommand {
    pub state: crate::domain::model::download::DownloadState,
    pub delete_files: bool,
}
impl Command for ClearDownloadsByStateCommand {}
```

Also add the module declaration near the top (after `mod remove_download;`):

```rust
mod clear_downloads_by_state;
```

- [ ] **Step 2: Compile-check**

Run: `cargo check -p vortex-core 2>&1 | tail -20`
Expected: compile error `unresolved module 'clear_downloads_by_state'` (we declare it next).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/application/commands/mod.rs
git commit -m "feat(download): declare ClearDownloadsByStateCommand

Adds the CQRS command struct for the upcoming bulk clear handler. The module
file is created in task 2."
```

---

## Task 2: RED — failing test for the "completed" happy path

**Files:**
- Create: `src-tauri/src/application/commands/clear_downloads_by_state.rs`

- [ ] **Step 1: Scaffold the file with test-only content**

Create the file with:

```rust
// Handler lives here. Test module sits at the bottom.

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use crate::application::command_bus::CommandBus;
    use crate::application::commands::ClearDownloadsByStateCommand;
    use crate::application::error::AppError;
    use crate::domain::error::DomainError;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::config::{AppConfig, ConfigPatch};
    use crate::domain::model::credential::Credential;
    use crate::domain::model::download::{Download, DownloadId, DownloadState, Url};
    use crate::domain::model::http::HttpResponse;
    use crate::domain::model::meta::DownloadMeta;
    use crate::domain::model::plugin::{PluginInfo, PluginManifest};
    use crate::domain::ports::driven::{
        ArchiveExtractor, ClipboardObserver, ConfigStore, CredentialStore, DownloadEngine,
        DownloadRepository, EventBus, FileStorage, HttpClient, PluginLoader,
    };

    // ---------- Mocks (copied from remove_download.rs — kept inline to stay
    // consistent with the existing test style in this crate) ----------

    struct MockDownloadRepo {
        store: Mutex<HashMap<u64, Download>>,
    }
    impl MockDownloadRepo {
        fn new() -> Self { Self { store: Mutex::new(HashMap::new()) } }
        fn with(self, dl: Download) -> Self {
            self.store.lock().unwrap().insert(dl.id().0, dl);
            self
        }
    }
    impl DownloadRepository for MockDownloadRepo {
        fn find_by_id(&self, id: DownloadId) -> Result<Option<Download>, DomainError> {
            Ok(self.store.lock().unwrap().get(&id.0).cloned())
        }
        fn save(&self, d: &Download) -> Result<(), DomainError> {
            self.store.lock().unwrap().insert(d.id().0, d.clone()); Ok(())
        }
        fn delete(&self, id: DownloadId) -> Result<(), DomainError> {
            self.store.lock().unwrap().remove(&id.0); Ok(())
        }
        fn find_by_state(&self, s: DownloadState) -> Result<Vec<Download>, DomainError> {
            Ok(self.store.lock().unwrap().values().filter(|d| d.state() == s).cloned().collect())
        }
    }

    struct MockDownloadEngine;
    impl DownloadEngine for MockDownloadEngine {
        fn start(&self, _: &Download) -> Result<(), DomainError> { Ok(()) }
        fn pause(&self, _: DownloadId) -> Result<(), DomainError> { Ok(()) }
        fn resume(&self, _: DownloadId) -> Result<(), DomainError> { Ok(()) }
        fn cancel(&self, _: DownloadId) -> Result<(), DomainError> { Ok(()) }
    }

    struct MockEventBus { events: Mutex<Vec<DomainEvent>> }
    impl MockEventBus { fn new() -> Self { Self { events: Mutex::new(Vec::new()) } } }
    impl EventBus for MockEventBus {
        fn publish(&self, e: DomainEvent) { self.events.lock().unwrap().push(e); }
        fn subscribe(&self, _: Box<dyn Fn(&DomainEvent) + Send + Sync>) {}
    }

    struct MockFileStorage { deleted_metas: Mutex<Vec<String>> }
    impl MockFileStorage { fn new() -> Self { Self { deleted_metas: Mutex::new(Vec::new()) } } }
    impl FileStorage for MockFileStorage {
        fn create_file(&self, _: &Path, _: u64) -> Result<(), DomainError> { Ok(()) }
        fn write_segment(&self, _: &Path, _: u64, _: &[u8]) -> Result<(), DomainError> { Ok(()) }
        fn read_meta(&self, _: &Path) -> Result<Option<DownloadMeta>, DomainError> { Ok(None) }
        fn write_meta(&self, _: &Path, _: &DownloadMeta) -> Result<(), DomainError> { Ok(()) }
        fn delete_meta(&self, p: &Path) -> Result<(), DomainError> {
            self.deleted_metas.lock().unwrap().push(p.to_string_lossy().into_owned()); Ok(())
        }
    }

    struct MockHttpClient;
    impl HttpClient for MockHttpClient {
        fn head(&self, _: &str) -> Result<HttpResponse, DomainError> {
            Ok(HttpResponse { status_code: 200, headers: HashMap::new(), body: vec![] })
        }
        fn get_range(&self, _: &str, _: u64, _: u64) -> Result<Vec<u8>, DomainError> { Ok(vec![]) }
        fn supports_range(&self, _: &str) -> Result<bool, DomainError> { Ok(true) }
    }

    struct MockPluginLoader;
    impl PluginLoader for MockPluginLoader {
        fn load(&self, _: &PluginManifest) -> Result<(), DomainError> { Ok(()) }
        fn unload(&self, _: &str) -> Result<(), DomainError> { Ok(()) }
        fn resolve_url(&self, _: &str) -> Result<Option<PluginInfo>, DomainError> { Ok(None) }
        fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> { Ok(vec![]) }
        fn set_enabled(&self, _: &str, _: bool) -> Result<(), DomainError> { Ok(()) }
    }

    struct MockConfigStore;
    impl ConfigStore for MockConfigStore {
        fn get_config(&self) -> Result<AppConfig, DomainError> { Ok(AppConfig::default()) }
        fn update_config(&self, _: ConfigPatch) -> Result<AppConfig, DomainError> { Ok(AppConfig::default()) }
    }

    struct MockCredentialStore;
    impl CredentialStore for MockCredentialStore {
        fn get(&self, _: &str) -> Result<Option<Credential>, DomainError> { Ok(None) }
        fn store(&self, _: &str, _: &Credential) -> Result<(), DomainError> { Ok(()) }
        fn delete(&self, _: &str) -> Result<(), DomainError> { Ok(()) }
    }

    struct MockClipboardObserver;
    impl ClipboardObserver for MockClipboardObserver {
        fn start(&self) -> Result<(), DomainError> { Ok(()) }
        fn stop(&self) -> Result<(), DomainError> { Ok(()) }
        fn get_urls(&self) -> Result<Vec<String>, DomainError> { Ok(vec![]) }
    }

    struct FakeArchiveExtractor;
    impl ArchiveExtractor for FakeArchiveExtractor {
        fn detect_format(&self, _: &Path) -> Result<Option<crate::domain::model::archive::ArchiveFormat>, DomainError> { Ok(None) }
        fn can_extract(&self, _: &Path) -> Result<bool, DomainError> { Ok(false) }
        fn extract(&self, _: &Path, _: &Path, _: Option<&str>) -> Result<crate::domain::model::archive::ExtractSummary, DomainError> {
            Ok(crate::domain::model::archive::ExtractSummary { extracted_files: 0, extracted_bytes: 0, duration_ms: 0, warnings: vec![] })
        }
        fn list_contents(&self, _: &Path, _: Option<&str>) -> Result<Vec<crate::domain::model::archive::ArchiveEntry>, DomainError> { Ok(vec![]) }
        fn detect_segments(&self, _: &Path) -> Result<Option<Vec<std::path::PathBuf>>, DomainError> { Ok(None) }
    }

    // ---------- Fixture helpers ----------

    fn completed_download(id: u64, path: &str) -> Download {
        let mut d = Download::new(
            DownloadId(id),
            Url::new("http://example.com/f.zip").unwrap(),
            format!("f{id}.zip"),
            path.to_string(),
        );
        d.start().unwrap();
        // Drive the state machine to Completed. The domain exposes `mark_completed`
        // (see domain/model/download.rs — confirm the exact method name when running
        // the test; if the API differs, adjust here).
        d.mark_completed().unwrap();
        d
    }

    fn errored_download(id: u64, path: &str) -> Download {
        let mut d = Download::new(
            DownloadId(id),
            Url::new("http://example.com/f.zip").unwrap(),
            format!("f{id}.zip"),
            path.to_string(),
        );
        d.start().unwrap();
        d.mark_error("boom".to_string()).unwrap();
        d
    }

    struct TestHarness {
        bus: CommandBus,
        event_bus: Arc<MockEventBus>,
        file_storage: Arc<MockFileStorage>,
    }

    fn make_harness(repo: MockDownloadRepo) -> TestHarness {
        let event_bus = Arc::new(MockEventBus::new());
        let file_storage = Arc::new(MockFileStorage::new());
        let bus = CommandBus::new(
            Arc::new(repo),
            Arc::new(MockDownloadEngine),
            event_bus.clone(),
            file_storage.clone(),
            Arc::new(MockHttpClient),
            Arc::new(MockPluginLoader),
            Arc::new(MockConfigStore),
            Arc::new(MockCredentialStore),
            Arc::new(MockClipboardObserver),
            Arc::new(FakeArchiveExtractor),
            None,
        );
        TestHarness { bus, event_bus, file_storage }
    }

    // ---------- Tests ----------

    #[tokio::test]
    async fn test_clear_completed_returns_count_and_deletes_from_db() {
        let repo = MockDownloadRepo::new()
            .with(completed_download(1, "/tmp/a.zip"))
            .with(completed_download(2, "/tmp/b.zip"));
        let h = make_harness(repo);

        let cmd = ClearDownloadsByStateCommand {
            state: DownloadState::Completed,
            delete_files: false,
        };
        let count = h.bus.handle_clear_downloads_by_state(cmd).await.unwrap();

        assert_eq!(count, 2);
        assert!(h.bus.download_repo().find_by_id(DownloadId(1)).unwrap().is_none());
        assert!(h.bus.download_repo().find_by_id(DownloadId(2)).unwrap().is_none());
    }
}
```

**Important caveat:** the helpers `mark_completed()` and `mark_error("boom")` must match the real API of `Download`. If the names differ (e.g. `complete()`, `fail(msg)`, or state transitions via a different method), adjust *only these two lines* by reading `src-tauri/src/domain/model/download.rs`. Do not invent a new API.

- [ ] **Step 2: Run the test, confirm it fails**

Run: `cargo test -p vortex-core test_clear_completed_returns_count_and_deletes_from_db 2>&1 | tail -30`
Expected: FAIL — the method `handle_clear_downloads_by_state` does not exist on `CommandBus`.

- [ ] **Step 3: Do NOT commit yet. Continue to Task 3.**

---

## Task 3: GREEN — minimal happy-path implementation

**Files:**
- Modify: `src-tauri/src/application/commands/clear_downloads_by_state.rs`

- [ ] **Step 1: Add the handler on top of the test module**

Insert at the top of the file (before `#[cfg(test)] mod tests`):

```rust
use std::path::Path;

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;
use crate::domain::model::download::DownloadState;

impl CommandBus {
    pub async fn handle_clear_downloads_by_state(
        &self,
        cmd: super::ClearDownloadsByStateCommand,
    ) -> Result<u32, AppError> {
        if !matches!(cmd.state, DownloadState::Completed | DownloadState::Error) {
            return Err(AppError::Validation(
                "state must be Completed or Error".into(),
            ));
        }

        let downloads = self.download_repo().find_by_state(cmd.state)?;
        let mut count: u32 = 0;

        for download in downloads {
            if cmd.delete_files {
                let dest = Path::new(download.destination_path());
                if dest.exists() {
                    if let Err(e) = std::fs::remove_file(dest) {
                        tracing::warn!(
                            path = %dest.display(),
                            error = %e,
                            "failed to delete download file"
                        );
                    }
                }
                let meta_path = format!("{}.vortex-meta", download.destination_path());
                if let Err(e) = self.file_storage().delete_meta(Path::new(&meta_path)) {
                    tracing::warn!(
                        path = %meta_path,
                        error = %e,
                        "failed to delete .vortex-meta sidecar"
                    );
                }
            }

            if let Err(e) = self.download_repo().delete(download.id()) {
                tracing::error!(
                    id = download.id().0,
                    error = %e,
                    "failed to delete download from repository"
                );
                continue;
            }

            self.event_bus()
                .publish(DomainEvent::DownloadRemoved { id: download.id() });
            count += 1;
        }

        Ok(count)
    }
}
```

- [ ] **Step 2: Run the test, confirm it passes**

Run: `cargo test -p vortex-core test_clear_completed_returns_count_and_deletes_from_db 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 3: Full workspace tests still green**

Run: `cargo test --workspace 2>&1 | tail -5`
Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/application/commands/clear_downloads_by_state.rs
git commit -m "feat(download): add handle_clear_downloads_by_state

Bulk-clears downloads in terminal states (Completed, Error) with optional
on-disk file deletion. Emits one DownloadRemoved event per successfully
cleared download. Non-terminal states are rejected with AppError::Validation.
Filesystem failures are logged and ignored (best-effort)."
```

---

## Task 4: RED → GREEN — error state + validation + event emission + idempotence

**Files:**
- Modify: `src-tauri/src/application/commands/clear_downloads_by_state.rs`

Append these tests to the `tests` module:

- [ ] **Step 1: Write the extra test suite**

```rust
#[tokio::test]
async fn test_clear_failed_returns_count() {
    let repo = MockDownloadRepo::new()
        .with(errored_download(1, "/tmp/a.zip"))
        .with(completed_download(2, "/tmp/b.zip"));
    let h = make_harness(repo);

    let cmd = ClearDownloadsByStateCommand {
        state: DownloadState::Error,
        delete_files: false,
    };
    let count = h.bus.handle_clear_downloads_by_state(cmd).await.unwrap();

    assert_eq!(count, 1);
    // The completed one must remain untouched.
    assert!(h.bus.download_repo().find_by_id(DownloadId(2)).unwrap().is_some());
}

#[tokio::test]
async fn test_clear_non_terminal_state_returns_validation_error() {
    let h = make_harness(MockDownloadRepo::new());
    let cmd = ClearDownloadsByStateCommand {
        state: DownloadState::Downloading,
        delete_files: false,
    };
    let err = h.bus.handle_clear_downloads_by_state(cmd).await.unwrap_err();
    assert!(matches!(err, AppError::Validation(_)));
}

#[tokio::test]
async fn test_clear_emits_one_removed_event_per_cleared_download() {
    let repo = MockDownloadRepo::new()
        .with(completed_download(1, "/tmp/a.zip"))
        .with(completed_download(2, "/tmp/b.zip"));
    let h = make_harness(repo);

    let cmd = ClearDownloadsByStateCommand {
        state: DownloadState::Completed,
        delete_files: false,
    };
    h.bus.handle_clear_downloads_by_state(cmd).await.unwrap();

    let events = h.event_bus.events.lock().unwrap();
    let removed: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            DomainEvent::DownloadRemoved { id } => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(removed.len(), 2);
    assert!(removed.contains(&DownloadId(1)));
    assert!(removed.contains(&DownloadId(2)));
}

#[tokio::test]
async fn test_clear_with_delete_files_calls_filestorage_delete_meta() {
    let repo = MockDownloadRepo::new().with(completed_download(1, "/tmp/a.zip"));
    let h = make_harness(repo);

    let cmd = ClearDownloadsByStateCommand {
        state: DownloadState::Completed,
        delete_files: true,
    };
    h.bus.handle_clear_downloads_by_state(cmd).await.unwrap();

    let metas = h.file_storage.deleted_metas.lock().unwrap();
    assert_eq!(metas.len(), 1);
    assert_eq!(metas[0], "/tmp/a.zip.vortex-meta");
}

#[tokio::test]
async fn test_clear_without_delete_files_skips_filestorage() {
    let repo = MockDownloadRepo::new().with(completed_download(1, "/tmp/a.zip"));
    let h = make_harness(repo);

    let cmd = ClearDownloadsByStateCommand {
        state: DownloadState::Completed,
        delete_files: false,
    };
    h.bus.handle_clear_downloads_by_state(cmd).await.unwrap();

    assert!(h.file_storage.deleted_metas.lock().unwrap().is_empty());
}

#[tokio::test]
async fn test_clear_missing_file_is_idempotent() {
    // Path that surely does not exist on disk.
    let repo = MockDownloadRepo::new()
        .with(completed_download(1, "/nonexistent/definitely/not/here.zip"));
    let h = make_harness(repo);

    let cmd = ClearDownloadsByStateCommand {
        state: DownloadState::Completed,
        delete_files: true,
    };
    let count = h.bus.handle_clear_downloads_by_state(cmd).await.unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn test_clear_empty_returns_zero() {
    let h = make_harness(MockDownloadRepo::new());
    let cmd = ClearDownloadsByStateCommand {
        state: DownloadState::Completed,
        delete_files: true,
    };
    let count = h.bus.handle_clear_downloads_by_state(cmd).await.unwrap();
    assert_eq!(count, 0);
    assert!(h.event_bus.events.lock().unwrap().is_empty());
}
```

- [ ] **Step 2: Run all the handler's tests**

Run: `cargo test -p vortex-core clear_downloads_by_state 2>&1 | tail -20`
Expected: all 7 tests PASS.

- [ ] **Step 3: Full workspace**

Run: `cargo test --workspace 2>&1 | tail -5`
Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/application/commands/clear_downloads_by_state.rs
git commit -m "test(download): exhaustive tests for clear-by-state handler

Covers: Error state, validation guard against non-terminal states, event
emission, file deletion gating, idempotence on missing files, and empty
result."
```

---

## Task 5: Tauri IPC handlers

**Files:**
- Modify: `src-tauri/src/adapters/driving/tauri_ipc.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add the two IPC handlers**

Open `tauri_ipc.rs`, find the `download_remove` handler (line 138-153). Immediately after it, insert:

```rust
#[tauri::command]
pub async fn download_clear_completed(
    state: State<'_, AppState>,
    delete_files: bool,
) -> Result<u32, String> {
    let cmd = ClearDownloadsByStateCommand {
        state: crate::domain::model::download::DownloadState::Completed,
        delete_files,
    };
    state
        .command_bus
        .handle_clear_downloads_by_state(cmd)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn download_clear_failed(
    state: State<'_, AppState>,
    delete_files: bool,
) -> Result<u32, String> {
    let cmd = ClearDownloadsByStateCommand {
        state: crate::domain::model::download::DownloadState::Error,
        delete_files,
    };
    state
        .command_bus
        .handle_clear_downloads_by_state(cmd)
        .await
        .map_err(|e| e.to_string())
}
```

Locate the imports at the top of `tauri_ipc.rs` (anywhere `RemoveDownloadCommand` is brought in) and add `ClearDownloadsByStateCommand` to the `use` list, e.g.:

```rust
use crate::application::commands::{
    // ...existing imports...
    ClearDownloadsByStateCommand,
    // ...
};
```

- [ ] **Step 2: Export the new handlers and register them in the Tauri builder**

In `src-tauri/src/lib.rs` (line 55-63) update the `pub use` statement to add `download_clear_completed, download_clear_failed` to the exported identifiers (keep alphabetical order):

```rust
pub use adapters::driving::tauri_ipc::{
    self, AppState, clipboard_state, clipboard_toggle, command_get_media_metadata, download_cancel,
    download_clear_completed, download_clear_failed,
    download_count_by_state, download_detail, download_list, download_logs, download_media_start,
    download_pause, download_pause_all, download_remove, download_resume, download_resume_all,
    download_retry, download_set_priority, download_start, link_resolve, plugin_disable,
    plugin_enable, plugin_install, plugin_list, plugin_store_install, plugin_store_list,
    plugin_store_refresh, plugin_store_update, plugin_uninstall, settings_get, settings_update,
    status_bar_get,
};
```

Then find `.invoke_handler(tauri::generate_handler![...])` (around line 270-300 — search for `download_pause_all,`) and add `download_clear_completed, download_clear_failed,` to the list (again keep alphabetical / section grouping with existing `download_*` entries).

- [ ] **Step 3: Compile check**

Run: `cargo check --workspace 2>&1 | tail -10`
Expected: no errors, no warnings.

- [ ] **Step 4: Run full test suite**

Run: `cargo test --workspace 2>&1 | tail -5`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/adapters/driving/tauri_ipc.rs src-tauri/src/lib.rs
git commit -m "feat(download): expose clear-completed and clear-failed IPC

Two distinct Tauri commands restrict the target state to Completed or Error
respectively. Non-terminal states cannot be reached from the frontend."
```

---

## Task 6: Install sonner and mount the Toaster

**Files:**
- Modify: `package.json` (via `npm install`)
- Create: `src/lib/toast.ts`
- Modify: `src/App.tsx`

- [ ] **Step 1: Install sonner**

Run: `npm install sonner`
Expected: `package.json` `dependencies.sonner` present; `package-lock.json` updated.

- [ ] **Step 2: Create the toast wrapper**

Create `src/lib/toast.ts`:

```ts
import { toast as sonnerToast } from 'sonner';

export const toast = {
  success: (message: string) => sonnerToast.success(message),
  error: (message: string) => sonnerToast.error(message),
};
```

- [ ] **Step 3: Mount the Toaster**

Edit `src/App.tsx`. Import sonner's Toaster at the top:

```tsx
import { Toaster } from 'sonner';
```

Place the `<Toaster />` component inside `<QueryClientProvider>` and outside `<BrowserRouter>`, e.g. just before the `</TooltipProvider>` closing tag:

```tsx
<TooltipProvider>
  <BrowserRouter>
    {/* ...routes... */}
  </BrowserRouter>
  <Toaster richColors position="bottom-right" closeButton />
</TooltipProvider>
```

- [ ] **Step 4: Verify build**

Run: `npm run build 2>&1 | tail -10`
Expected: success.

- [ ] **Step 5: Commit**

```bash
git add package.json package-lock.json src/lib/toast.ts src/App.tsx
git commit -m "feat(ui): add sonner toast infrastructure

Installs sonner (~5 kB gzipped), mounts the Toaster in App, and adds a thin
src/lib/toast wrapper so components depend on our abstraction rather than
the sonner API directly."
```

---

## Task 7: i18n keys

**Files:**
- Modify: `src/i18n/locales/en.json`
- Modify: `src/i18n/locales/fr.json`

- [ ] **Step 1: Edit `en.json`**

Find the `"downloads"` block, locate the `"actions"` object at line 133-137 and extend it. Also append a sibling `"toast"` object and a `"clearDialog"` object.

Replace the current `"actions"` block and the following lines with:

```json
"actions": {
  "pauseAll": "Pause All",
  "resumeAll": "Resume All",
  "cancelSelected": "Cancel Selected",
  "clearCompleted": "Clear completed",
  "clearFailed": "Clear failed"
},
"clearDialog": {
  "titleCompleted_one": "Clear {{count}} completed download?",
  "titleCompleted_other": "Clear {{count}} completed downloads?",
  "titleFailed_one": "Clear {{count}} failed download?",
  "titleFailed_other": "Clear {{count}} failed downloads?",
  "description": "This removes the download entries from Vortex. They will no longer appear in the list.",
  "deleteFilesLabel": "Also delete files from disk",
  "warningTitle": "Permanent deletion",
  "warningBody": "Files will be removed from your disk. This action cannot be undone.",
  "confirm": "Clear",
  "confirmWithFiles": "Clear and delete files",
  "cancel": "Cancel"
},
"toast": {
  "clearedCompleted_one": "{{count}} completed download cleared",
  "clearedCompleted_other": "{{count}} completed downloads cleared",
  "clearedFailed_one": "{{count}} failed download cleared",
  "clearedFailed_other": "{{count}} failed downloads cleared",
  "clearError": "Failed to clear downloads: {{error}}"
},
```

- [ ] **Step 2: Edit `fr.json`**

Apply the symmetric French translations. Example values:

```json
"actions": {
  "pauseAll": "Tout mettre en pause",
  "resumeAll": "Tout reprendre",
  "cancelSelected": "Annuler la sélection",
  "clearCompleted": "Effacer terminés",
  "clearFailed": "Effacer en erreur"
},
"clearDialog": {
  "titleCompleted_one": "Effacer {{count}} téléchargement terminé ?",
  "titleCompleted_other": "Effacer {{count}} téléchargements terminés ?",
  "titleFailed_one": "Effacer {{count}} téléchargement en erreur ?",
  "titleFailed_other": "Effacer {{count}} téléchargements en erreur ?",
  "description": "Les entrées seront retirées de Vortex. Elles n'apparaîtront plus dans la liste.",
  "deleteFilesLabel": "Également supprimer les fichiers du disque",
  "warningTitle": "Suppression définitive",
  "warningBody": "Les fichiers seront supprimés de votre disque. Cette action est irréversible.",
  "confirm": "Effacer",
  "confirmWithFiles": "Effacer et supprimer les fichiers",
  "cancel": "Annuler"
},
"toast": {
  "clearedCompleted_one": "{{count}} téléchargement terminé effacé",
  "clearedCompleted_other": "{{count}} téléchargements terminés effacés",
  "clearedFailed_one": "{{count}} téléchargement en erreur effacé",
  "clearedFailed_other": "{{count}} téléchargements en erreur effacés",
  "clearError": "Échec de l'effacement des téléchargements : {{error}}"
},
```

**Preserve any pre-existing keys in `fr.json`** that are not listed above (e.g., the original `pauseAll` French value if it already existed). Only the three blocks listed here are affected.

- [ ] **Step 3: Validate JSON**

Run: `node -e "require('./src/i18n/locales/en.json'); require('./src/i18n/locales/fr.json'); console.log('ok')"`
Expected: `ok`.

- [ ] **Step 4: Commit**

```bash
git add src/i18n/locales/en.json src/i18n/locales/fr.json
git commit -m "i18n(download): add keys for clear completed/failed dialog and toasts"
```

---

## Task 8: `ClearDownloadsDialog` component — RED

**Files:**
- Create: `src/views/DownloadsView/__tests__/ClearDownloadsDialog.test.tsx`

- [ ] **Step 1: Write the failing tests**

```tsx
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { I18nextProvider } from 'react-i18next';
import i18n from '@/i18n/i18n';
import { ClearDownloadsDialog } from '@/views/DownloadsView/ClearDownloadsDialog';

function renderDialog(overrides: Partial<Parameters<typeof ClearDownloadsDialog>[0]> = {}) {
  const props = {
    open: true,
    onOpenChange: vi.fn(),
    targetState: 'completed' as const,
    count: 3,
    onConfirm: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
  render(
    <I18nextProvider i18n={i18n}>
      <ClearDownloadsDialog {...props} />
    </I18nextProvider>,
  );
  return props;
}

describe('ClearDownloadsDialog', () => {
  beforeEach(() => vi.clearAllMocks());

  it('renders the completed title with the provided count', () => {
    renderDialog({ targetState: 'completed', count: 3 });
    expect(screen.getByText(/Clear 3 completed downloads\?/i)).toBeInTheDocument();
  });

  it('renders the failed title when targetState is error', () => {
    renderDialog({ targetState: 'error', count: 2 });
    expect(screen.getByText(/Clear 2 failed downloads\?/i)).toBeInTheDocument();
  });

  it('does not show the warning panel by default', () => {
    renderDialog();
    expect(screen.queryByText(/Permanent deletion/i)).not.toBeInTheDocument();
  });

  it('reveals the warning panel when the checkbox is checked', async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(screen.getByRole('checkbox', { name: /also delete files from disk/i }));
    expect(screen.getByText(/Permanent deletion/i)).toBeInTheDocument();
  });

  it('primary button label switches when the checkbox is checked', async () => {
    const user = userEvent.setup();
    renderDialog();
    expect(screen.getByRole('button', { name: /^clear$/i })).toBeInTheDocument();
    await user.click(screen.getByRole('checkbox', { name: /also delete files from disk/i }));
    expect(
      screen.getByRole('button', { name: /clear and delete files/i }),
    ).toBeInTheDocument();
  });

  it('calls onConfirm with deleteFiles:false when the box is not checked', async () => {
    const user = userEvent.setup();
    const props = renderDialog();
    await user.click(screen.getByRole('button', { name: /^clear$/i }));
    expect(props.onConfirm).toHaveBeenCalledWith(false);
  });

  it('calls onConfirm with deleteFiles:true when the box is checked', async () => {
    const user = userEvent.setup();
    const props = renderDialog();
    await user.click(screen.getByRole('checkbox', { name: /also delete files from disk/i }));
    await user.click(screen.getByRole('button', { name: /clear and delete files/i }));
    expect(props.onConfirm).toHaveBeenCalledWith(true);
  });

  it('calls onOpenChange(false) when cancel is clicked', async () => {
    const user = userEvent.setup();
    const props = renderDialog();
    await user.click(screen.getByRole('button', { name: /cancel/i }));
    expect(props.onOpenChange).toHaveBeenCalledWith(false);
  });
});
```

- [ ] **Step 2: Run, confirm RED**

Run: `npx vitest run src/views/DownloadsView/__tests__/ClearDownloadsDialog.test.tsx 2>&1 | tail -15`
Expected: FAIL — module not found (`ClearDownloadsDialog`).

---

## Task 9: `ClearDownloadsDialog` component — GREEN

**Files:**
- Create: `src/views/DownloadsView/ClearDownloadsDialog.tsx`

- [ ] **Step 1: Write the component**

```tsx
import { useEffect, useState } from 'react';
import { AlertTriangle } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';

export type ClearDownloadsTarget = 'completed' | 'error';

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  targetState: ClearDownloadsTarget;
  count: number;
  onConfirm: (deleteFiles: boolean) => Promise<void> | void;
}

export function ClearDownloadsDialog({
  open,
  onOpenChange,
  targetState,
  count,
  onConfirm,
}: Props) {
  const { t } = useTranslation();
  const [deleteFiles, setDeleteFiles] = useState(false);
  const [submitting, setSubmitting] = useState(false);

  // Reset checkbox every time the dialog opens so the destructive option is
  // never pre-selected.
  useEffect(() => {
    if (open) setDeleteFiles(false);
  }, [open]);

  const titleKey =
    targetState === 'completed'
      ? 'downloads.clearDialog.titleCompleted'
      : 'downloads.clearDialog.titleFailed';

  const confirmLabel = deleteFiles
    ? t('downloads.clearDialog.confirmWithFiles')
    : t('downloads.clearDialog.confirm');

  const handleConfirm = async () => {
    if (submitting) return;
    setSubmitting(true);
    try {
      await onConfirm(deleteFiles);
      onOpenChange(false);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t(titleKey, { count })}</DialogTitle>
          <DialogDescription>
            {t('downloads.clearDialog.description')}
          </DialogDescription>
        </DialogHeader>

        <label className="flex items-center gap-2 text-sm">
          <Checkbox
            checked={deleteFiles}
            onCheckedChange={(v) => setDeleteFiles(Boolean(v))}
          />
          <span>{t('downloads.clearDialog.deleteFilesLabel')}</span>
        </label>

        {deleteFiles && (
          <div
            role="alert"
            className="rounded-md border border-destructive/40 bg-destructive/10 p-3 flex gap-2 items-start"
          >
            <AlertTriangle
              className="h-5 w-5 shrink-0 text-destructive"
              aria-hidden="true"
            />
            <div>
              <p className="font-semibold text-destructive">
                {t('downloads.clearDialog.warningTitle')}
              </p>
              <p className="text-sm text-destructive/90">
                {t('downloads.clearDialog.warningBody')}
              </p>
            </div>
          </div>
        )}

        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={submitting}
          >
            {t('downloads.clearDialog.cancel')}
          </Button>
          <Button
            variant={deleteFiles ? 'destructive' : 'default'}
            onClick={handleConfirm}
            disabled={submitting}
          >
            {confirmLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
```

- [ ] **Step 2: Run the dialog tests**

Run: `npx vitest run src/views/DownloadsView/__tests__/ClearDownloadsDialog.test.tsx 2>&1 | tail -15`
Expected: all 8 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add src/views/DownloadsView/ClearDownloadsDialog.tsx \
       src/views/DownloadsView/__tests__/ClearDownloadsDialog.test.tsx
git commit -m "feat(downloads): add ClearDownloadsDialog component

Reusable confirmation dialog for bulk clearing. Optional 'also delete files'
checkbox toggles a prominent red warning panel and switches the primary
button to the destructive variant with a matching label."
```

---

## Task 10: `ActionsBar` integration — RED

**Files:**
- Create: `src/views/DownloadsView/__tests__/ActionsBar.test.tsx`

- [ ] **Step 1: Inspect the existing ActionsBar test (if any)**

Run: `ls src/views/DownloadsView/__tests__/ 2>&1`
If an existing `ActionsBar.test.tsx` already exists, open it and append the new suites below rather than creating a new file.

- [ ] **Step 2: Write the tests**

```tsx
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { I18nextProvider } from 'react-i18next';
import i18n from '@/i18n/i18n';
import { ActionsBar } from '@/views/DownloadsView/ActionsBar';

const invokeMock = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

const toastMock = { success: vi.fn(), error: vi.fn() };
vi.mock('@/lib/toast', () => ({ toast: toastMock }));

function wrap(ui: React.ReactElement) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return (
    <QueryClientProvider client={qc}>
      <I18nextProvider i18n={i18n}>{ui}</I18nextProvider>
    </QueryClientProvider>
  );
}

// The view pre-seeds the count-by-state query; we do the same here.
function seedCounts(qc: QueryClient, counts: Record<string, number>) {
  qc.setQueryData(['downloads', 'countByState'], counts);
}

describe('ActionsBar — clear completed/failed', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    toastMock.success.mockReset();
    toastMock.error.mockReset();
  });

  it('disables "Clear completed" when Completed count is 0', () => {
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    seedCounts(qc, { Completed: 0, Error: 3 });
    render(
      <QueryClientProvider client={qc}>
        <I18nextProvider i18n={i18n}><ActionsBar /></I18nextProvider>
      </QueryClientProvider>,
    );
    expect(screen.getByRole('button', { name: /clear completed/i })).toBeDisabled();
  });

  it('disables "Clear failed" when Error count is 0', () => {
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    seedCounts(qc, { Completed: 1, Error: 0 });
    render(
      <QueryClientProvider client={qc}>
        <I18nextProvider i18n={i18n}><ActionsBar /></I18nextProvider>
      </QueryClientProvider>,
    );
    expect(screen.getByRole('button', { name: /clear failed/i })).toBeDisabled();
  });

  it('invokes download_clear_completed with deleteFiles:false and shows success toast', async () => {
    invokeMock.mockResolvedValueOnce(3);
    const user = userEvent.setup();
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    seedCounts(qc, { Completed: 3, Error: 0 });

    render(
      <QueryClientProvider client={qc}>
        <I18nextProvider i18n={i18n}><ActionsBar /></I18nextProvider>
      </QueryClientProvider>,
    );
    await user.click(screen.getByRole('button', { name: /clear completed/i }));
    await user.click(await screen.findByRole('button', { name: /^clear$/i }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('download_clear_completed', {
        deleteFiles: false,
      });
    });
    await waitFor(() => {
      expect(toastMock.success).toHaveBeenCalledWith(
        expect.stringContaining('3'),
      );
    });
  });

  it('invokes download_clear_failed with deleteFiles:true when checkbox checked', async () => {
    invokeMock.mockResolvedValueOnce(2);
    const user = userEvent.setup();
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    seedCounts(qc, { Completed: 0, Error: 2 });

    render(
      <QueryClientProvider client={qc}>
        <I18nextProvider i18n={i18n}><ActionsBar /></I18nextProvider>
      </QueryClientProvider>,
    );
    await user.click(screen.getByRole('button', { name: /clear failed/i }));
    await user.click(await screen.findByRole('checkbox', { name: /also delete files from disk/i }));
    await user.click(screen.getByRole('button', { name: /clear and delete files/i }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('download_clear_failed', {
        deleteFiles: true,
      });
    });
  });

  it('shows error toast when the mutation rejects', async () => {
    invokeMock.mockRejectedValueOnce(new Error('boom'));
    const user = userEvent.setup();
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    seedCounts(qc, { Completed: 1, Error: 0 });

    render(
      <QueryClientProvider client={qc}>
        <I18nextProvider i18n={i18n}><ActionsBar /></I18nextProvider>
      </QueryClientProvider>,
    );
    await user.click(screen.getByRole('button', { name: /clear completed/i }));
    await user.click(await screen.findByRole('button', { name: /^clear$/i }));

    await waitFor(() => {
      expect(toastMock.error).toHaveBeenCalledWith(
        expect.stringContaining('boom'),
      );
    });
  });
});
```

- [ ] **Step 3: Run, confirm RED**

Run: `npx vitest run src/views/DownloadsView/__tests__/ActionsBar.test.tsx 2>&1 | tail -20`
Expected: FAIL — buttons not rendered / `@/lib/toast` not used by ActionsBar yet.

---

## Task 11: `ActionsBar` integration — GREEN

**Files:**
- Modify: `src/views/DownloadsView/ActionsBar.tsx`

- [ ] **Step 1: Rewrite `ActionsBar.tsx`**

Replace the file with:

```tsx
import { useRef, useState } from 'react';
import { CheckCheck, Pause, Play, X, XCircle } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useQueryClient } from '@tanstack/react-query';
import { Button } from '@/components/ui/button';
import { Separator } from '@/components/ui/separator';
import { useTauriMutation } from '@/api/hooks';
import { downloadQueries } from '@/api/queries';
import { useUiStore } from '@/stores/uiStore';
import { toast } from '@/lib/toast';
import {
  ClearDownloadsDialog,
  type ClearDownloadsTarget,
} from './ClearDownloadsDialog';

const INVALIDATE_KEYS = [
  downloadQueries.lists(),
  downloadQueries.countByState(),
] as const;

export function ActionsBar() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const selectedDownloadIds = useUiStore((s) => s.selectedDownloadIds);
  const setSelectedDownloadIds = useUiStore((s) => s.setSelectedDownloadIds);
  const clearSelection = useUiStore((s) => s.clearSelection);

  const pauseAll = useTauriMutation<void, void>('download_pause_all', {
    invalidateKeys: INVALIDATE_KEYS,
  });

  const resumeAll = useTauriMutation<void, void>('download_resume_all', {
    invalidateKeys: INVALIDATE_KEYS,
  });

  const cancelDownload = useTauriMutation<void, { id: number }>('download_cancel', {
    invalidateKeys: INVALIDATE_KEYS,
  });

  const clearCompleted = useTauriMutation<number, { deleteFiles: boolean }>(
    'download_clear_completed',
    {
      invalidateKeys: INVALIDATE_KEYS,
      onSuccess: (count) => {
        toast.success(
          t('downloads.toast.clearedCompleted', { count }),
        );
      },
      onError: (err) => {
        toast.error(t('downloads.toast.clearError', { error: err.message }));
      },
    },
  );

  const clearFailed = useTauriMutation<number, { deleteFiles: boolean }>(
    'download_clear_failed',
    {
      invalidateKeys: INVALIDATE_KEYS,
      onSuccess: (count) => {
        toast.success(t('downloads.toast.clearedFailed', { count }));
      },
      onError: (err) => {
        toast.error(t('downloads.toast.clearError', { error: err.message }));
      },
    },
  );

  const cancellingRef = useRef(false);
  const handleCancelSelected = async () => {
    if (cancellingRef.current) return;
    cancellingRef.current = true;
    const snapshot = [...selectedDownloadIds];
    try {
      const results = await Promise.allSettled(
        snapshot.map((id) => cancelDownload.mutateAsync({ id: Number(id) })),
      );
      const failedIds = snapshot.filter((_, i) => results[i].status === 'rejected');
      const currentIds = useUiStore.getState().selectedDownloadIds;
      const unchanged =
        currentIds.length === snapshot.length
        && currentIds.every((id, i) => id === snapshot[i]);
      if (unchanged) {
        if (failedIds.length === 0) clearSelection();
        else setSelectedDownloadIds(failedIds);
      }
    } finally {
      cancellingRef.current = false;
    }
  };

  const hasSelection = selectedDownloadIds.length > 0;

  // Counts. We read the cache directly so the bar is fully reactive even
  // when the DownloadsView does not explicitly pass counts down.
  const counts =
    queryClient.getQueryData<Record<string, number>>(
      downloadQueries.countByState(),
    ) ?? {};
  const completedCount = counts.Completed ?? 0;
  const errorCount = counts.Error ?? 0;

  const [dialogTarget, setDialogTarget] =
    useState<ClearDownloadsTarget | null>(null);
  const dialogOpen = dialogTarget !== null;
  const dialogCount = dialogTarget === 'completed' ? completedCount : errorCount;

  const handleDialogConfirm = async (deleteFiles: boolean) => {
    if (dialogTarget === 'completed') {
      await clearCompleted.mutateAsync({ deleteFiles });
    } else if (dialogTarget === 'error') {
      await clearFailed.mutateAsync({ deleteFiles });
    }
  };

  return (
    <div
      className={`flex items-center gap-2 min-h-[36px] ${hasSelection ? 'rounded-md bg-muted/50 px-3 py-1' : ''}`}
    >
      {hasSelection ? (
        <>
          <span className="text-sm text-muted-foreground">
            {t('downloads.selectedCount', { count: selectedDownloadIds.length })}
          </span>
          <Button variant="ghost" size="sm" onClick={handleCancelSelected}>
            <X className="mr-1 h-4 w-4" />
            {t('downloads.actions.cancelSelected')}
          </Button>
          <Button variant="ghost" size="sm" onClick={clearSelection}>
            {t('common.clear')}
          </Button>
        </>
      ) : (
        <>
          <Button variant="ghost" size="sm" onClick={() => pauseAll.mutate()}>
            <Pause className="mr-1 h-4 w-4" />
            {t('downloads.actions.pauseAll')}
          </Button>
          <Button variant="ghost" size="sm" onClick={() => resumeAll.mutate()}>
            <Play className="mr-1 h-4 w-4" />
            {t('downloads.actions.resumeAll')}
          </Button>

          <Separator orientation="vertical" className="mx-1 h-4" />

          <Button
            variant="ghost"
            size="sm"
            disabled={completedCount === 0}
            onClick={() => setDialogTarget('completed')}
          >
            <CheckCheck className="mr-1 h-4 w-4" />
            {t('downloads.actions.clearCompleted')}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            disabled={errorCount === 0}
            onClick={() => setDialogTarget('error')}
          >
            <XCircle className="mr-1 h-4 w-4" />
            {t('downloads.actions.clearFailed')}
          </Button>
        </>
      )}

      {dialogTarget !== null && (
        <ClearDownloadsDialog
          open={dialogOpen}
          onOpenChange={(o) => !o && setDialogTarget(null)}
          targetState={dialogTarget}
          count={dialogCount}
          onConfirm={handleDialogConfirm}
        />
      )}
    </div>
  );
}
```

- [ ] **Step 2: Run the ActionsBar tests**

Run: `npx vitest run src/views/DownloadsView/__tests__/ActionsBar.test.tsx 2>&1 | tail -20`
Expected: all 5 new tests PASS.

- [ ] **Step 3: Run full frontend suite**

Run: `npx vitest run 2>&1 | tail -5`
Expected: all pass.

- [ ] **Step 4: Lint and type-check**

Run: `npx oxlint . 2>&1 | tail -10`
Run: `npx tsc --noEmit 2>&1 | tail -10`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add src/views/DownloadsView/ActionsBar.tsx \
       src/views/DownloadsView/__tests__/ActionsBar.test.tsx
git commit -m "feat(downloads): wire Clear completed/failed buttons in ActionsBar

Adds two new toolbar buttons separated from bulk actions by a vertical
Separator. Each opens the ClearDownloadsDialog and fires the corresponding
Tauri mutation, followed by a success or error toast."
```

---

## Task 12: CHANGELOG update

**Files:**
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Edit**

Under `[Unreleased] > Added`, append:

```markdown
- Clear completed and clear failed downloads from the Downloads toolbar, with
  an optional "also delete files from disk" confirmation guarded by a
  prominent warning. Each action reports its outcome via a toast.
- Sonner-based toast notifications (new library dependency).
```

- [ ] **Step 2: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs: changelog entry for clear completed/failed downloads"
```

---

## Task 13: Manual smoke test (not scripted)

- [ ] **Step 1:** Run `npm run tauri dev`. In a second shell, trigger real downloads so some finish and some fail (for example, one legit URL + one 404).

- [ ] **Step 2:** Click **Clear completed** without checking the box → confirm toast, list shrinks, files still on disk.

- [ ] **Step 3:** Click **Clear failed** with the box checked → confirm warning appears red, confirm destructive variant, click confirm → files gone from disk.

- [ ] **Step 4:** Trigger an artificial error (e.g. flip the DB into read-only mode for a second, or forge a mutation rejection via `tauri-pilot ipc`) → confirm the error toast shows the backend message.

- [ ] **Step 5:** Record findings in the PR description.

---

## Task 14: Adversarial review

- [ ] **Step 1:** Dispatch four agents **in parallel**:
  - `rust-reviewer` — focus on the new command, guard, and idempotence.
  - `typescript-reviewer` — focus on `ClearDownloadsDialog` and `ActionsBar`, hooks usage, invalidation correctness.
  - `security-reviewer` — focus on the file-deletion path: can anything outside the intended file be removed? Is the IPC boundary safe?
  - `code-reviewer` — general quality, over-engineering, unused code.

- [ ] **Step 2:** Fix any finding that rates "must fix" in its own commit (`fix(...): address <reviewer>: <issue>`).

- [ ] **Step 3:** Re-run `cargo test --workspace` and `npx vitest run` after fixes.

---

## Task 15: Optional — PR and follow-up issue

- [ ] **Step 1:** `/git-create-pr` (or `gh pr create`) with title `feat(downloads): clear completed and failed downloads` and a body summarising the change and linking the spec.

- [ ] **Step 2:** `/issue-create` (or `gh issue create`) — "Migrate all success/error feedback app-wide to sonner toasts". Mention that this feature seeded the toast infrastructure and the rest of the app should follow.

---

## Self-review notes

- Every spec section has at least one task covering it (dialog, button+separator, i18n, sonner, adversarial review, follow-up issue).
- No TBDs. All code blocks are complete and self-contained.
- Method signatures are consistent: `handle_clear_downloads_by_state(cmd) -> Result<u32, AppError>` used identically across the handler file, IPC file, and the frontend expectation (`Promise<number>`).
- Types match across the boundary: `{ deleteFiles: boolean }` (camelCase) in TS, `delete_files: bool` in Rust (serde auto-derives camelCase on the `#[tauri::command]` parameter, per Tauri defaults).
- The `mark_completed` / `mark_error` helpers used in tests are flagged as "verify-API-before-use" so the implementer adjusts them if the domain exposes a different name.
