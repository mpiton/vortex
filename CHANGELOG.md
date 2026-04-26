# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- `copy_then_delete_io` now treats a `NotFound` error from the final source unlink as success: by that point the destination has already been written and size-verified, so discarding it would lose the only complete copy and break `move_meta`'s "missing source = no-op" contract (coderabbit critical, PR #107).
- `FsFileStorage::move_meta` now probes the source sidecar when destination reservation fails with `AlreadyExists`: if the source is missing, the call returns `Ok(())` per the sidecar contract instead of surfacing "destination already exists" — that error would otherwise roll back an already-completed body move in the change-directory handler. When the source is present and the destination is occupied, the error still propagates so unrelated metadata isn't silently clobbered (cubic P2, PR #107).
- `FsFileStorage::move_meta` no longer pre-checks the sidecar existence with `try_exists` before attempting the move: the probe + move pair was a TOCTOU race where another actor deleting the sidecar between the two calls would cause the move to fail and roll back an already-successful body move in the change-directory handler. The function now attempts the move directly and treats `NotFound` on the source as the no-op the `FileStorage` contract promises (coderabbit major, PR #107).
- Change-directory cross-FS fallback no longer reopens the TOCTOU race it was meant to close: the reserved-destination placeholder is now kept in place when `fs::rename` returns EXDEV — `fs::copy` truncates and overwrites it with the source bytes — instead of being deleted before the copy runs (cubic P1, PR #107).
- Move-to-folder partial-failure flow now updates `selectedDownloadId` together with `selectedDownloadIds`, so the details panel doesn't keep showing a download that just moved successfully when only failed rows remain selected (coderabbit minor + cubic P2, PR #107).
- Change-directory handler no longer reports a successful move as failed when the post-persistence engine resume errors out: the `DownloadDirectoryChanged` event now publishes before the resume attempt and resume failures log a warning instead of propagating, so bulk callers no longer misclassify the row as failed and the frontend always invalidates its caches. Sidecar rollback failures during DB-save recovery are now logged loudly so metadata/body divergence is observable. The production `move_file` path replaces its racy `to.exists()` check with a `create_new` placeholder reservation, closing the TOCTOU window where a concurrent process could squeeze a different file into the destination before our rename. The `FileStorage` port's `file_exists` now returns `Result<bool, DomainError>` and uses `Path::try_exists` so I/O errors (permission denied, broken symlink loop) surface as `Err` instead of being silently coerced into `false`. The `move_file` and `move_meta` defaults now return an explicit "not implemented" error so a future adapter that forgets to override surfaces the gap loudly instead of silently succeeding while leaving the file behind. Frontend `MoveDialog` now receives the first selected download's `destinationPath` so the "current location" pill renders and the OS folder picker opens at the file's parent directory; `deriveDefaultDir` handles root-level paths (`/file.bin` → `/`, `C:\file.bin` → `C:\`) instead of returning `null`/`C:`. The bulk-move outcome's failed-rows handler now coerces the IPC's numeric ids back to strings before writing to the UI store, matching the store's `string[]` contract. (PR #107 review)

### Added

- Dynamic segment splitting (PRD-v2 P0.17, task 17): when a parallel segment finishes before its peers, the engine now re-evaluates the still-running segments, picks the slowest one whose remaining range exceeds `dynamic_split_min_remaining_mb` (default 4 MiB) and shrinks it in place — a fresh worker takes the upper half so the tail of the download accelerates instead of stalling on a single slow connection. Backend ships a domain-pure `Segment::split(at_byte, new_id)` validation method (state must be `Downloading`, split point strictly inside the unfetched range, caller-provided id must differ from the original — IDs are allocated by the engine's monotonic `next_segment_id` counter, never invented inside the domain), a new `DomainEvent::SegmentSplit { download_id, original_segment_id, new_segment_id, split_at }` forwarded as the `segment-split` Tauri event and logged in the per-download log store, two new `AppConfig` / `ConfigPatch` / `SettingsDto` fields `dynamic_split_enabled` (default `true`) and `dynamic_split_min_remaining_mb` (default `4`) wired through the toml config store, the Tauri IPC `SettingsDto`/`ConfigPatchDto` (so the frontend can both read and write them) and the new `application::services::engine_config_bridge` subscriber so live `settings_update` calls reconfigure already-running engines without a restart. `SegmentedDownloadEngine` stores `dynamic_split_enabled` / `dynamic_split_min_remaining_bytes` in `Arc<AtomicBool>` / `Arc<AtomicU64>` and exposes a `set_dynamic_split(enabled, min_remaining_mb)` setter consumed by the bridge. After a split, the engine updates the original slot's `initial_end` to `split_at` immediately on successful `end_tx.send`, so a subsequent `pick_split_target` evaluation cannot expand the worker's range past the shrunk boundary and `persist_split_meta` records the post-split topology rather than the stale one (closes coderabbit P1 + greptile P1 race). Each segment task now returns `(slot_idx, Result<u64>)`; on success the engine flips a `completed: bool` flag on the slot — `pick_split_target` skips completed slots so they cannot be re-picked, and `persist_split_meta` keeps the entry with `completed: true` and a full-range `downloaded_bytes` so a crash right after a split never loses the record of byte ranges already on disk. `pick_split_target` also gates on a 500 ms / non-zero-progress sample window: a fresh split child cannot be picked again until it has actually produced a throughput sample, preventing cascading fragmentation of the newest range. The segment worker accepts the upper bound through a `tokio::sync::watch::Receiver<u64>` instead of a frozen `u64`, re-reads it before each chunk fetch and again after every successful network read so a mid-flight shrink clamps the next write to the new boundary; per-segment progress is exposed via an `Arc<AtomicU64>` so the engine can pick the slowest candidate by throughput (`downloaded / elapsed`). After every split, the engine atomically rewrites `.vortex-meta` with the updated segment topology so resume after a crash mid-split sees a consistent state. (task 17, PR #111 review)
- "Report broken plugin" action (PRD-v2 P0.16, task 16): plugins listed in *Plugins → Plugin Store* now expose a *Report broken plugin* item in their kebab menu. Clicking it opens the user's default browser at a pre-filled GitHub issue on the plugin's repository, with diagnostic metadata (plugin name + version, Vortex version, OS, optional URL under test, last 50 log lines) inlined into the issue body. Backend adds a `repository_url` field to `domain::model::plugin::PluginInfo` (parsed from the new `[plugin].repository` key in `plugin.toml`), a `domain::ports::driven::UrlOpener` port plus its platform-native `SystemUrlOpener` adapter (`xdg-open` / `open` / `cmd start`, `http(s)://` only by validation), the std-only `domain::model::plugin::build_report_broken_url` URL builder (RFC 3986 unreserved-set percent encoder, last 50 log lines, GitHub-only repository hosts, accepts `.git` suffix, rejects malformed URLs with `DomainError::ValidationError`), and a `ReportBrokenPluginCommand` handler that returns `AppError::Validation` when a manifest carries no `repository_url`. New Tauri IPC `plugin_report_broken(pluginName, logLines?, testedUrl?) → string` returns the issue URL so the UI can fall back to clipboard copy if the launcher fails. i18n (en/fr): `plugins.action.reportBroken`, `plugins.toast.reportBrokenSuccess`, `plugins.toast.reportBrokenError`. (task 16)
- Dynamic plugin configuration UI (PRD-v2 P0.15, task 15): plugins declaring a `[config]` block in their `plugin.toml` now expose their schema at runtime. Backend adds `ConfigField` / `ConfigFieldType` / `PluginConfigSchema` to `domain/model/plugin.rs` (typed validation, enum options, `min`/`max` bounds, regex via a std-only matcher — no external import in the domain), a `PluginConfigStore` port (`get_values` / `set_value` / `list_all` / `delete_all`) implemented by `SqlitePluginConfigRepo` backed by the new `plugin_configs (plugin_name, key, value)` table (migration `m20260425_000005_create_plugin_configs`, composite primary key). The manifest parser (`adapters/driven/plugin/manifest.rs`) now extracts `type`, `default`, `options`, `description`, `min`, `max`, `regex` on top of the existing defaults, and rejects defaults that fail their own field validation. CQRS gains `UpdatePluginConfigCommand` (validates against the schema, applies the runtime first then persists, rolls back on failure) and `GetPluginConfigQuery` (returns the schema plus persisted values, dropping any persisted entry that no longer matches the current schema and falling back to manifest defaults). `PluginLoader` is extended with `get_manifest()` and `set_runtime_config()`; `ExtismPluginLoader` implements both by reading from `PluginRegistry` and writing to `SharedHostResources::plugin_configs`, so `get_config(key)` calls from the WASM plugin observe the new value without a reload. At startup, `lib.rs` replays persisted configs onto the in-memory map before plugins are loaded. Frontend adds two components: `PluginConfigField.tsx` (dispatcher renderer: `string` → text input, `boolean` → shadcn switch, `integer`/`float` → numeric input with bounds, `url` → url input, `enum` (and `string` with options) → shadcn select; `aria-describedby` on the control points to the error message) and `PluginConfigDialog.tsx` (loads the schema via `useQuery`, validates each field on the UI side (rejects empty floats, validates JSON arrays) before sending, persists changed values sequentially, guards the schema-reset effect while a save is in flight to avoid clobbering the draft, invalidates the query on success). `PluginsView` queries `plugin_config_get` for each installed plugin (keyed off the unfiltered installed list to avoid churn while typing in search) to decide whether the *Configure* button (Settings icon, next to the *More* menu) should render: a plugin without `[config]` exposes no button. New IPC commands `plugin_config_get(name) → PluginConfigView` and `plugin_config_update(name, key, value)`. i18n (en/fr): `plugins.action.configure`, `plugins.config.{title,description,loading,error,noFields,toast.{saveSuccess,validationFailed}}`. (task 15)
- History retention with automatic daily purge (PRD-v2 P0.14, task 14): new `history_retention_days` setting (default 30, presets 7 / 30 / 90 / 365 / `0 = unlimited`) exposed in the *General* Settings tab as a `Select` dropdown wired to `settings_update`. Backend ships a `Clock` domain port (`SystemClock` adapter under `adapters/driven/scheduler/`) and a `HistoryPurgeWorker` daemon spawned during Tauri setup that hard-deletes `history` rows where `completed_at < now - retention_days * 86_400`. The worker persists its last run as a Unix-epoch timestamp inside `<app_data_dir>/.history_purge_state` (sentinel filename `HISTORY_PURGE_STATE_FILE`). On startup, the daemon reads the sentinel and either runs immediately (missing/stale) or sleeps for `SECS_PER_DAY - elapsed` so the first post-launch purge stays anchored to the previous successful run instead of drifting up to ~47h after a restart; the recurring loop then ticks every 24h via `tokio::time::interval` with `MissedTickBehavior::Skip`. `retention_days <= 0` is a no-op that does not write the sentinel, so the next run re-fires the moment the user re-enables retention; corrupt sentinels are treated as "never ran" so a stuck file never blocks the scheduler. The worker shares the same `Arc<dyn HistoryRepository>` and `Arc<dyn ConfigStore>` the IPC layer already mutates, so a settings change is observed without restart. Domain helper `normalize_history_retention_days` clamps negatives back to `0` and is now applied at every write boundary — `apply_patch` (so a crafted `settings_update` payload cannot persist a negative) and `From<ConfigDto> for AppConfig` (so a hand-edited `config.toml` is normalized at load) — plus the worker itself for defense-in-depth. (task 14)
- Change-directory action that moves a download's on-disk file (and its `.vortex-meta` sidecar when present) into a new destination folder (PRD-v2 P0.13, task 13). New Tauri IPC commands `download_change_directory(id, newDestinationDir)` and `download_change_directory_bulk(ids, newDestinationDir)` are backed by `ChangeDirectoryCommand` / `ChangeDirectoryBulkCommand` in the application layer; the bulk variant returns a structured `{ moved: number[], failed: { id, message }[] }` outcome so the UI can keep failed rows selected for retry instead of swallowing partial errors. The handler pauses the download engine for `Downloading` items, relocates the body and the `.vortex-meta` sidecar, persists the new path, then resumes — segments survive the move so the engine picks up exactly where it left off. `Extracting` and `Checking` downloads are rejected because another worker is actively reading the file. The `FileStorage` port grows `move_file`, `move_meta` and `file_exists`; the production `FsFileStorage` adapter prefers `fs::rename` for same-filesystem moves and falls back to copy + size-verify + delete-source for cross-device cases (EXDEV / `ErrorKind::CrossesDevices`), with rollback on any partial failure so the source file always stays intact. New `DomainEvent::DownloadDirectoryChanged { id, newDestinationPath }` is forwarded to the frontend as the `download-directory-changed` event. Frontend ships a reusable `<MoveDialog>` (folder picker via `useBrowseFolder`, current path + selected path preview, confirm disabled until a folder is picked) and a `Move to...` action in the downloads `ActionsBar` selection toolbar that wires the bulk IPC, surfaces success / partial-failure / error toasts and clears or re-narrows the selection accordingly. New i18n keys `downloads.actions.moveSelected`, `downloads.moveDialog.*` and `downloads.toast.{moveSucceeded,movePartial,moveError}` (en/fr). (task 13)
- Queue reordering via drag & drop and Move-to-Top / Move-to-Bottom (PRD-v2 P0.12, task 12): new Tauri IPC commands `download_move_to_top(id)`, `download_move_to_bottom(id)`, `download_reorder_queue(orderedIds)` backed by `MoveToTopCommand` / `MoveToBottomCommand` / `ReorderQueueCommand` in the application layer. A new `queue_position` column (migration `m20260425_000004_add_queue_position`, `BIGINT NOT NULL DEFAULT 0`, index `idx_downloads_queue_position`) persists the manual ordering so drag-reorders survive restart. `QueueManager` now sorts candidates by priority desc → `queue_position` asc → `created_at` asc, and also subscribes to two new domain events (`DownloadPrioritySet`, `QueueReordered`) so changing priority triggers immediate rescheduling — a high-priority item starts as soon as a slot is free. The default `download_list` sort uses `queue_position` ASC → `created_at` DESC so fresh downloads (position 0) still appear newest-first while manually-moved rows stick. Frontend integration in `DownloadsTable` adds `@dnd-kit/core` + `@dnd-kit/sortable` with a drag handle column (enabled only for Queued/Retry/Waiting rows), a `SortableContext` around the virtualized rows, and a `computeReorderedIds` helper that filters non-reorderable IDs from the new order before invoking `download_reorder_queue`. Row dropdown menu gets Move to top / Move to bottom items for reorderable rows. New i18n keys `downloads.table.actions.moveToTop` / `moveToBottom` (en/fr). `DownloadView` / `DownloadViewDto` now expose `priority` + `queuePosition`. (task 12)
- Global `Ctrl/Cmd+V` paste-to-link-grabber shortcut and a dedicated *Keyboard shortcuts* Settings tab (PRD-v2 P0.11, task 11): pressing `Ctrl`/`Cmd`+`V` anywhere outside a text field reads the system clipboard via `navigator.clipboard.readText`, navigates to `/link-grabber` with `location.state = { focusPaste: true, pasteContent, pasteToken }`, then `PasteZone` consumes `pasteContent` through a new `initialValue` prop by pre-filling the textarea and auto-triggering `link_resolve` on the extracted URLs. Replay is keyed off a navigation-scoped `pasteToken` (a fresh `Date.now()`+random string per shortcut press) instead of the raw clipboard text, so pressing the shortcut twice with identical clipboard contents still re-resolves; `handleClear()` also resets the guard. Focus is preserved by the existing `data-shortcut-target="link-grabber-paste"` handler. The `AppLayout.isEditableTarget` guard still short-circuits the shortcut when focus is on an `<input>`, `<textarea>`, or `contenteditable`, so native paste keeps working inside editors; a `.catch` on `readText()` surfaces a `linkGrabber.toast.clipboardReadFailed` toast instead of an unhandled rejection when permission is denied. A new `ShortcutsSection` component (`SettingTab = 'shortcuts'`, Keyboard icon) renders the ten PRD §8 combos in a `<kbd>` table and substitutes `Cmd` on macOS via the new `src/lib/platform.ts` helper (also used by `AppLayout`) so the displayed modifier always matches the actual handler. i18n under `settings.shortcuts.*` (`columns.shortcut/action`, `rows.pasteUrls/selectAll/pauseResume/deleteSelection/toggleClipboard/navigateViews/focusSearch/addUrlsDialog/openSettings/closePanel`) translated for en/fr. Covered by new Vitest cases in `AppLayout.test.tsx` (`Ctrl/Cmd+V` reads clipboard + navigates with `pasteContent`+`pasteToken`, clipboard read failure shows a toast, `Ctrl+1` ignored on textarea, `Ctrl+V` not intercepted on textarea, `Ctrl+2..6` nav), `LinkGrabberView.test.tsx` (textarea pre-filled + `link_resolve` called with the pasted URLs), and `SettingsView.test.tsx` (seven tabs with exact count, shortcuts tab lists ten rows). (task 11)
- Clipboard monitoring toggle now live in the Link Grabber header (PRD-v2 P0.10, task 10): the `Switch` is no longer `disabled`, `onCheckedChange` is wired through the existing `useClipboardMonitoring` hook so a click invokes the `clipboard_toggle` IPC and also subscribes to the `clipboard-monitoring-changed` Tauri event. Initial state is seeded from `settingsStore.config.clipboardMonitoring`, so the toggle matches the persisted config as soon as the store hydrates. A 7×7 status dot (success on, border off) sits between the label and the switch, and the wrapper `title` swaps between `statusBar.clipboardActive` ("Clipboard monitoring active") and `statusBar.clipboardPaused` ("Clipboard monitoring paused") — the same copy used by the status-bar `ClipboardIndicator`, so both views stay in sync through the shared event. Backend persistence is untouched: `handle_toggle_clipboard` still writes the new value via `ConfigStore::update_config` and rolls back if the observer `start`/`stop` fails, so the state survives restart. The orphan `linkGrabber.clipboardComingSoon` i18n key was removed from `en.json` / `fr.json`. (task 10)
- Re-download action on completed downloads and history entries (PRD-v2 P0.9, task 09): new Tauri IPC command `download_redownload(sourceKind, sourceId, overwriteMode?)` that clones either a `Download` aggregate or a `HistoryEntry` into a brand-new Download with a fresh `DownloadId`, preserving the URL, filename, destination, and — for the `Download` source — segments count, priority, source hostname, module name and account id. Return type is a tagged union: `{ kind: "created", id }` on success or `{ kind: "fileExists", originalPath, suggestedPath }` when the destination already exists; the UI re-invokes with `overwriteMode: "overwrite"` or `"rename"` (the latter resolves a non-colliding `name (N).ext` via the existing `unique_destination` helper). Backend `RedownloadCommand` + `RedownloadSource` (application layer) and new command handler `application/commands/redownload.rs` that `load_template`s the source before calling `Download::new` with the new id from `next_download_id`. Domain `Download` gains `with_segments_count`, `with_module_name`, `with_account_id` builder methods so the handler can carry forward options the history row does not retain. Frontend ships a reusable `<OverwriteDialog>` (Overwrite / Keep both / Cancel) and a `useRedownload` hook returning `{ trigger, dialog, isPending }`; both `DownloadsTable` (Completed rows only) and `HistoryView` render the dialog and invalidate `downloads.lists`, `downloads.countByState` and `history.lists` on success. New i18n keys `common.overwriteDialog.*` and `downloads.table.{actions.redownload,toast.redownload*}` (en/fr). (task 09)
- Open file / Open folder actions on completed downloads (PRD-v2 P0.8, task 08): two new Tauri IPC commands `download_open_file(id)` and `download_open_folder(id)` launch the OS default app or reveal the file in the host file manager. Driven by a new `FileOpener` port (`domain/ports/driven/file_opener.rs`) with a `SystemFileOpener` adapter that dispatches per-OS: `xdg-open` on Linux, `open` / `open -R` on macOS, `explorer` / `explorer /select,<path>` on Windows. Application handlers `open_download_file` and `open_download_folder` look up the download by id, refuse non-`Completed` state with `AppError::Validation`, and surface `DomainError::NotFound` when the destination file is gone — the frontend `useTauriMutation` `errorMessage` mapper translates that to a localized "File not found" toast (en/fr). UI exposes the actions in the row dropdown (Completed rows only) and as buttons in the detail panel's File info section. `CommandBus::with_file_opener` wires the adapter optionally (matching the `with_checksum_computer` pattern) so existing test fixtures do not need new mocks. (task 08)
- Browse-folder dialog in General settings (PRD-v2 P0.7, task 07): the `Browse` button next to *Download directory* is now wired to a native OS folder picker via `tauri-plugin-dialog`, replacing the previously `disabled` placeholder. Two async Tauri IPC commands back the UI: `browse_folder(default_path?)` and `browse_file(filters?, default_path?)` — both return `Option<String>` so a cancelled dialog persists nothing and does not raise an error toast. The implementation bridges the plugin's callback-based `pick_folder` / `pick_file` to async with a `tokio::sync::oneshot` channel; the passed `default_path` is validated (directory must exist) before being forwarded as `set_directory`, and for `browse_file` the anchor falls back to the parent when a file path is provided. `GeneralSection` now consumes a new reusable `useBrowseFolder` / `useBrowseFile` hook pair from `src/hooks/useBrowseFolder.ts`, ready to be reused for package destinations, export paths and other future path pickers. Selected folder goes through the existing `settings_update` mutation so persistence and toast feedback stay on one path. (task 07)
- Checksum integrity validation (PRD-v2 P0.6, task 06): post-download SHA-256 / MD5 verification driven by the `Downloading → Checking → Completed | Error` flow when `checksum_expected` is set and `verify_checksums` is on. Algorithm auto-detected from the hash format (32 hex chars → MD5, 64 → SHA-256). `compute_file_checksum` streams files in 8 MB chunks via `sha2` + `md-5` to handle multi-GB downloads without memory pressure. Mismatches transition to `Error` with a `ChecksumMismatch { expected, computed, algorithm }` event published on the bus and a descriptive `error_message` persisted alongside; matches transition to `Completed` with `checksum_computed` + `checksum_algorithm` columns durable in SQLite (migration `m20260424_000003_add_checksum_columns`). New IPC command `download_verify_checksum(id)` re-runs validation on demand even for already-completed downloads. New domain port `ChecksumComputer` with `StreamingChecksumComputer` adapter; new application service `ChecksumValidatorService` orchestrating validation + persistence + event publishing. `IntegritySection` in the detail panel now shows the algorithm, expected hash, computed hash, match indicator (✓ / ✗) and a "Verify" button wired to the IPC. Setting `verify_checksums = false` bypasses validation entirely so downloads complete directly. (task 06)
- Statistics view (`#/statistics`) now replaces the placeholder: period selector (7d/30d/all-time tablist), seven KPI cards (total volume, total files, avg/peak speed, success rate, cumulative download time, CAPTCHA placeholder), four Recharts visualizations (daily volume bar, top hosts donut, type breakdown horizontal bar, average speed line) plus a Top-5 modules ranking. New `useStatsQuery(period)` aggregates `stats_get`, `stats_top_modules` (limit 5) and `history_list`; type breakdown and speed series are derived client-side from history (extension parsing + UTC-day grouping). Charts pull their primary color from `var(--color-accent)` so the user's accent setting is respected, fall back to a fixed palette for multi-series, expose `role="img"` + axis `label` props for screen readers, and render an empty hint when the period yields no data. Recharts dependency added (`npm i recharts`). New `i18n` namespace `statistics.*` (en/fr). (task 04)
- History view (`#/history`) now replaces the placeholder: entries grouped by local day with sticky date headers and a proper `<thead>` row (Name, Host, Size, Duration, Completed, Status, Avg speed, Module, Account, Actions); filter tabs All/Completed/Failed/Cancelled with live counts (Failed/Cancelled resolve to 0 until the backend persists those states); debounced search (300 ms) that swaps `history_list` → `history_search`; per-row actions Re-download (invokes `download_start` with the original URL), Copy URL, Delete entry, Open folder (invokes the new `reveal_in_folder` IPC to reveal the destination in the OS file manager via xdg-open/open/explorer); Export CSV / JSON via a native save dialog (wrapped in try/catch with error toast) that pipes the chosen path through `history_export`. New `useHistoryQuery` TanStack wrapper and `useDebouncedValue` hook. `HistoryViewDto` serializes `entryId` (u64) as a string so 64-bit IDs survive JavaScript number precision; `history_delete_entry` / `history_get_by_id` accept a string id and parse it server-side. `exportSuccess` toast uses i18next plural forms (`_one` / `_other`). Bundled `tauri-plugin-dialog` (Cargo + npm) with a scoped `dialog:allow-save` capability; no open-dialog exposure. (task 03)
- Statistics IPC surface: `stats_get(period)` (`"7d" | "30d" | "all"`) returns `StatsViewDto` with period-bounded totals, daily volumes, success rate and top hosts; `stats_top_modules(limit)` returns the most-used resolving modules (name, download count, total bytes) capped at 50. Period filtering uses `statistics.date >= ?` for the daily rollup and `downloads.created_at >= ?` for success rate / top hosts so cutoffs line up with the data source. New `StatsPeriod` enum and `ModuleStats` domain view; `StatsRepository::get_stats` now takes a period argument and gained `top_modules(limit)`. `ModuleStatsDto` serializes `moduleName`/`downloadCount`/`totalBytes` camelCase for the frontend. (task 02)
- History IPC surface: queries `history_list(dateFrom, dateTo, hostname, sortField, sortDirection, limit, offset)`, `history_search(q)`, `history_get_by_id(id)` and commands `history_export(format, path)` (CSV RFC 4180 with spreadsheet-formula guard, or JSON pretty-printed), `history_delete_entry(id)`, `history_clear`, `history_purge_older_than(days)` — `days == 0` is rejected to avoid a full-table wipe. Results are capped at 500 rows per request; `list` supports `offset` for pagination. `HistoryViewDto` exposes the primary key as `entryId` so the frontend can target individual rows. The `HistoryRepository` port gained `list` (with date range + exact hostname match against the URL host), `search` (case-insensitive over file name / URL / destination), `find_by_id`, `delete_by_id` and `delete_all`, implemented by `SqliteHistoryRepo`. (task 01)
- `useTauriMutation` now accepts `silentError` (opt-out of the default toast) and `errorMessage` (remap the error message before toasting) options. (#74)

### Changed

- Plugins view refreshed to match the design mockup: a header with enabled/disabled counters and a "Check updates" action, a segmented category filter replacing the dropdown, grouped sections per category with uppercase labels, monogram icons with accent coloring for crawlers/extractors, a toggle for installed plugins, and a kebab menu hosting the destructive "Uninstall" action. Installable plugins keep a single `Install` button; pending updates surface as an inline pill on the row.
- Default settings values now match PRD §6.10 (fresh installs only — existing `config.toml` files are not migrated): `autoExtract` on, `maxConcurrentDownloads=4`, `maxRetries=5`, `retryDelaySeconds=10`, `minFileSizeMb=1.0`, `verifyChecksums` on, `webInterfacePort=9876`, REST API and WebSocket enabled by default. `downloadDir` now resolves to the OS default Downloads directory on first launch, and `apiKey` is generated as a random UUIDv4 so the REST/WS protocols never start with an empty credential. (#67)
- Every IPC mutation now surfaces an error toast by default via `useTauriMutation`; migrated all call sites (downloads, settings, plugins, link grabber, clipboard monitoring) to rely on this default. Inline error state removed from the link grabber. (#74)

### Fixed

- `QueueManager` now seeds `max_concurrent` from persisted `config.max_concurrent_downloads` at startup instead of the hardcoded `4`, and listens for `SettingsUpdated` events through a new `queue_config_bridge` subscriber so raising the limit in the UI takes effect immediately without a restart. Both paths route through a new `domain::model::config::normalize_max_concurrent` helper that clamps the raw `u32` to 1-20, so a manually-edited `config.toml` with `0` or an out-of-range value can no longer stall the scheduler. The `// TODO: read max_concurrent from config` in `lib.rs` is gone. (task 05)
- Statistics view type breakdown and speed curve could silently truncate data: `history_list` was called without filters, so the backend's 500-row cap clipped the dataset for users with large histories while KPI cards (sourced from `stats_get`) reflected the full DB. `useStatsQuery` now passes `dateFrom` matching the selected period cutoff and an explicit `limit: 500`, keeping KPI and chart data consistent within 7d/30d windows. Top-modules card title now reads "Top modules (all time)" to document that the backend `stats_top_modules` ranking is not period-bounded. Inline error banner surfaces partial failures instead of replacing the entire dashboard when `stats_get` succeeded but `history_list` or `stats_top_modules` failed. `PeriodSelector` tabs now expose proper `tabIndex` semantics (`0` on selected, `-1` otherwise). `formatDurationFromSeconds` returns `"< 1min"` for positive sub-minute durations instead of `"0min"`. `useStatsQuery.refetch` rethrows the first failure from `Promise.all` so callers can react to refetch errors.
- YouTube `get_media_metadata` surfaced 144p, 240p, and other non-canonical heights in the quality selector even though `vortex-mod-youtube` does not support them. Picking one produced a raw yt-dlp "Requested format is not available" error because the plugin only bypasses its pre-merged-HTTPS path for heights >=720 on the canonical ladder. `parse_ytdlp_json` now filters `available_qualities` against the plugin's supported set `{360, 480, 720, 1080, 1440, 2160, 4320}`, kept in sync with `plugin.toml :: default_quality.options`. The filter is scoped to YouTube sources (detected via yt-dlp's `extractor_key` / `webpage_url_domain`) so Vimeo, SoundCloud and other providers keep their own ladders (e.g. Vimeo's 540p).
- Completed downloads stayed stuck on `Downloading` in the UI until a manual reload: `QueueManager::handle_download_completed` persisted `state = Completed` to SQLite but never published the `DownloadCompletedPersisted` event the Tauri bridge forwards as `download-completed`, so `useDownloadEvents` never invalidated the TanStack Query cache. Now emitted after the save (and also for pre-persisted completions from `RegisterLocalFileCommand`/`ExtractArchive`), gated on the repo state being `Completed` so late events after cancel/fail do not mislead the UI.
- Frontend briefly showing stale state after a download completed: `DownloadCompleted` fired before `QueueManager` persisted `state = Completed` to SQLite, so a re-fetch triggered by the event could read the previous state. New `DownloadCompletedPersisted` event emitted _after_ the save; the Tauri bridge maps it to the same `download-completed` frontend event so existing invalidation logic is reused without changes.
- `downloaded_bytes` stayed at 0 in SQLite for downloads that finished in under 500 ms (the `DownloadProgress` throttle window): `segment_worker` now emits one final `DownloadProgress` right before `SegmentCompleted` so `progress_bridge` always observes the real byte count.
- State-transition saves could regress `downloaded_bytes` back to a stale lower value when racing with `progress_bridge` writes. `SqliteDownloadRepo.save` now excludes `downloaded_bytes` from the UPSERT column list and uses `MAX(excluded.downloaded_bytes, COALESCE(downloads.downloaded_bytes, 0))` so only larger values win.
- `DownloadView` / `DownloadDetailView` now expose `source_hostname` so the UI can show the origin host (e.g. `www.youtube.com`) rather than the CDN host (`rr1---sn-n4g-cvq6.googlevideo.com`) that the download URL resolves to.
- YouTube downloads silently downgrading to 360p when 1080p was requested but only
  DASH streams were available.
- YouTube `download_to_file` returning `HTTP Error 403: Forbidden` on protected
  videos (VEVO music, age-gated content). Bumped `vortex-mod-youtube` to 1.2.3
  which passes `--extractor-args "youtube:player_client=default,web_safari,android_vr,tv"`,
  `--retries 3`, `--fragment-retries 3`, and `--quiet` to yt-dlp.
- Completed downloads showed ~96% progress instead of 100%: the last `DownloadProgress` event (throttled to 500ms) could arrive before the final chunk was written; `compute_progress_percent()` now forces 100.0 when `state == "Completed"`
- Progress values showed excessive decimal places (e.g. "96.247262...%"); rounded to one decimal place using `(v * 10.0).round() / 10.0`
- Downloads never transitioned to Completed state: queue_manager received DownloadCompleted events but never persisted the state change; added `handle_download_completed()` analogous to `handle_download_failed()` to load the aggregate, call `.complete()`, and save it
- `progressPercent` always showed 0: `DownloadProgress` events carry `total_bytes` but the progress_bridge was discarding it; now writes `total_bytes` to the downloads row on first progress event (COALESCE so existing values are never overwritten)
- Downloads stalling indefinitely mid-transfer: `response.chunk().await` had no idle timeout, so a server stalling mid-stream would block the segment task forever; added a 30-second idle timeout that triggers `SegmentFailed` and allows the engine to fail-fast and retry
- `create_file` failed with "file exists" after app restart: engine now checks for orphaned download files (no `.vortex-meta` sidecar) and removes them before calling `create_new(true)`
- Default download destination was `./` (current working directory, usually the Tauri binary dir); now uses `config.download_dir` or `dirs::download_dir()` XDG fallback (fixes #59)
- Download directory was not created automatically; `create_file` now calls `std::fs::create_dir_all(parent)` before opening the file
- Pause button was shown for Queued state downloads, causing a silent IPC error since the backend only allows Downloading → Paused; button now correctly only shows for Downloading state (fixes #58)
- Bulk toggle (Space shortcut) no longer attempts to pause Queued downloads, aligning with the domain state machine
- Orphaned downloads from previous session (stuck in Downloading/Waiting/Checking/Extracting state) are now recovered to Error on startup so the user can retry; Queued/Retry downloads are re-scheduled automatically (fixes #57)
- `maxConcurrentDownloads` validation now enforces the PRD §6.10 limit of 1–20 (was incorrectly accepting up to 100) in both backend validation and the settings UI input
- Download engine was double-joining `file_name` onto `destination_path`, producing a path like `/Downloads/file.bin/file.bin` and causing all downloads to fail silently with a "Not a directory" I/O error before any bytes were fetched (fixes #54)
- `SegmentStarted` event now carries `start_byte` and `end_byte` so downstream consumers can identify which byte range a segment covers

### Added

- **Clear completed / Clear failed downloads**: two new toolbar buttons in the Downloads view, separated from the bulk actions by a vertical separator. Each opens a confirmation dialog with an optional "Also delete files from disk" checkbox gated behind a prominent red warning panel. Success and error outcomes are reported via toasts.
- `sonner` toast notifications (new dependency) mounted globally in `App.tsx`, with a thin `src/lib/toast.ts` wrapper so components do not depend on the library directly.
- YouTube 1080p+ support: when `resolve_stream_url` returns `AdaptiveStreamOnly`,
  `download_media_start` now falls back to `download_to_file` which delegates the
  full DASH download + ffmpeg merge to yt-dlp. The merged file is moved to the
  downloads folder and registered as a completed download.
- `download_media_start` IPC command: resolves the direct CDN stream URL via the WASM plugin that claims the URL (`resolve_stream_url` export), then starts the download — fixes the retry loop where the engine received a YouTube/Vimeo/SoundCloud page URL instead of a downloadable CDN URL
- `resolve_stream_url` method on `PluginLoader` trait: delegates URL resolution to WASM plugins; implemented in `ExtismPluginLoader` via `registry.call_plugin`; default impl returns `NotFound` for loaders that don't support it
- `command_get_media_metadata` IPC command: invokes `yt-dlp --dump-single-json --flat-playlist` and returns video title, thumbnail, duration, deduplicated quality options (sorted by height), video/audio container formats, subtitles (excluding live_chat), and playlist entries — fixes the "Failed to load media metadata" error in the Media Grabber Options dialog
- Error message display: failed downloads now show the error reason in a popover tooltip on the Status column (Popover component from shadcn/ui)
- `error_message` column added to `downloads` table (migration m20260415_000002); exposed in `DownloadView` read model and IPC response
- `DownloadRepository::save_failed(download, error)` — persists Error state and error text atomically, replacing the previous pattern of calling `save()` then updating separately
- Plugin store: browse, refresh, and install official plugins from the built-in registry; plugins verified by SHA-256 checksum and `min_vortex_version` constraint
- `spawn_sqlite_progress_bridge` — new event bridge that persists live download state to SQLite (`downloads.downloaded_bytes`, `download_segments` rows) so the read model reflects real progress instead of always showing 0%
- `SqliteStatsRepo` — persistent download statistics backed by SQLite (replaces in-memory stub)
- Project scaffolding: Tauri 2 + React 19 + TypeScript + Tailwind CSS 4 + shadcn/ui
- Nix flake for reproducible development environment
- Hexagonal architecture folder structure for Rust backend
- CI pipeline with GitHub Actions (lint, test, build matrix)
- Lefthook pre-commit and pre-push hooks
- EditorConfig for cross-editor consistency
- Contributor documentation (CONTRIBUTING.md, issue/PR templates)
- Domain models: Download, Segment, Package, Account, Plugin entities with state machines
- Domain ports: repository traits, event bus, engine, storage, and credential ports
- CQRS infrastructure: CommandBus, QueryBus, AppError, read model DTOs
- SQLite persistence: sea-orm adapter with WAL mode, migrations, and 3 repository implementations
  - `SqliteDownloadRepo` (write: save, find_by_id, delete, find_by_state)
  - `SqliteDownloadReadRepo` (read: filtered/sorted list, detail with segments, count by state)
  - `SqliteHistoryRepo` (record, find_recent, find_by_download, delete_older_than)
  - Initial migration creating 6 tables with indexes and foreign keys
- Event system: TokioEventBus (broadcast), Tauri bridge, useTauriEvent React hook
- Segmented download engine: parallel HTTP Range downloads with pause/cancel
  - `ReqwestHttpClient` adapter implementing HttpClient port (HEAD, GET range, range detection)
  - `SegmentedDownloadEngine` adapter implementing DownloadEngine port (start, pause, cancel)
  - Segment worker with streaming HTTP chunks, progress throttling (500ms), and CancellationToken
  - Configurable segment count with 64KB minimum segment size
  - Single-segment fallback when server doesn't support Range requests
- File storage adapter: `FsFileStorage` implementing `FileStorage` port
  - Sparse file pre-allocation via `set_len()` (no disk space wasted)
  - Segment writes at arbitrary byte offsets with seek + write_all
  - `.vortex-meta` bincode persistence for download resume state
  - Atomic meta writes (write-to-tmp + rename) to prevent corruption on crash
  - Graceful handling of corrupted `.vortex-meta` files (log warning, restart download)
- Queue manager: `QueueManager` application service for download scheduling
  - Configurable max concurrent downloads with `AtomicUsize` slot tracking
  - Priority-based queue ordering (highest priority first, FIFO within same priority)
  - Automatic next-download scheduling when a slot frees (completion, failure, pause)
  - Exponential backoff retry: 10s, 20s, 40s, 80s, 160s (capped at 300s)
  - Circuit breaker integration: respects `Download::retry()` / `MaxRetriesExceeded`
  - Retry cancellation via `CancellationToken` (e.g., when download is deleted)
  - EventBus sync-to-async bridge using bounded `mpsc::channel(1024)` with lifecycle event filtering
  - Idempotent `on_slot_freed()` via `tokio::sync::Mutex` scheduling lock
  - Event-driven scheduling for DownloadCreated, DownloadResumed, and DownloadRetrying events
- Download command handlers: 9 CQRS command handlers on CommandBus
  - `StartDownloadCommand`: HEAD metadata, URL validation, Download entity creation, event-driven queue scheduling
  - `PauseDownloadCommand` / `ResumeDownloadCommand`: state machine transitions with engine control
  - `CancelDownloadCommand`: engine cancellation, DB cleanup, `.vortex-meta` removal
  - `RetryDownloadCommand`: circuit breaker integration via domain `retry()` state machine
  - `PauseAllDownloadsCommand` / `ResumeAllDownloadsCommand`: batch operations on active/paused downloads
  - `SetPriorityCommand`: priority update (1-10) for queue reordering
  - `RemoveDownloadCommand`: full cleanup with optional file deletion
- Tauri IPC driving adapter: 9 `#[tauri::command]` functions with `AppState` wiring
  - Convention: `download_{action}` naming (`download_start`, `download_pause`, etc.)
- Download query handlers: 3 CQRS query handlers on QueryBus
  - `GetDownloadsQuery`: filtered, sorted, paginated download list via read repository
  - `GetDownloadDetailQuery`: full detail view with segments, NotFound handling
  - `CountDownloadsByStateQuery`: state-grouped counts for UI filter badges
  - Tauri IPC: `download_list` (filter/sort/search/pagination), `download_detail`, `download_count_by_state`
  - String-based filter/sort parsing in IPC layer (DownloadState, SortField, SortDirection)
- Plugin infrastructure: WASM plugin system via Extism with hot-reload
  - `plugin.toml` manifest parser with category/capabilities/version validation
  - `PluginRegistry` backed by DashMap for concurrent in-memory tracking
  - `ExtismPluginLoader` implementing `PluginLoader` port: load, unload, resolve_url, list
  - Hot-reload file watcher via `notify` crate with tokio integration
  - `InstallPluginCommand` / `UninstallPluginCommand` CQRS handlers with domain events
  - `EnablePluginCommand` / `DisablePluginCommand` handlers (validation-only for MVP)
  - `ListPluginsQuery` handler returning `PluginViewDto` read models
  - Tauri IPC: `plugin_install`, `plugin_uninstall`, `plugin_enable`, `plugin_disable`, `plugin_list`
  - Path traversal protection on plugin_install IPC (canonicalize + prefix check)
  - WASM file size limit (100 MB) to prevent OOM
  - Atomic insert via DashMap entry API to prevent TOCTOU races
  - `Container` and `Notifier` plugin categories added to domain model
  - Plugin host functions: http_request, log, get_config/set_config, get_state/set_state, get_credential, run_subprocess
  - Capability-based security for plugin host function access
- Downloads View: main table UI with TanStack Table + Virtual virtualization
  - Virtualized table rendering 10k+ rows with `@tanstack/react-virtual` (estimateSize: 48px, overscan: 10)
  - 9 columns: checkbox, state dot, filename (tooltip URL), type badge, host, progress bar, speed, ETA, actions
  - Sortable columns via TanStack Table `getSortedRowModel` (click header for asc/desc toggle)
  - FilterBar with state tabs: All | Active | Queued | Done | Failed (counts from `download_count_by_state`)
  - SearchBar: case-insensitive search across filename, URL, and hostname
  - Multi-select: Ctrl/Cmd+click toggles selection, single click selects for detail panel
  - ActionsBar: Pause All / Resume All when no selection; Cancel Selected / Clear when items selected
  - Per-row actions: Pause/Resume/Retry buttons + DropdownMenu (Set Priority, Remove)
  - Real-time progress: ProgressCell, SpeedCell, EtaCell read from Zustand `downloadStore.progressMap`
  - Speed color coding: green (>10 MB/s), blue (1-10 MB/s), muted (<1 MB/s)
  - Format utilities: `formatEta`, `formatSpeed`, `formatBytes` in `src/lib/format.ts`
  - uiStore extended with `selectedDownloadIds` for multi-select state
  - Fixed `useTauriQuery` to support custom `queryKey` (query cache invalidation now works correctly)
  - Fixed `useDownloadEvents` to invalidate `downloadQueries.all()` (covers list + count queries)
- Download Details Panel: right sidebar showing detailed info for the selected download
  - 8 sections: File Info, Metrics, Segments, Speed History, Source, Integrity, Module, Logs
  - Real-time metrics from Zustand `downloadStore.progressMap` (speed, ETA, downloaded/total)
  - Segment visualization with colored progress bars per segment
  - Speed sparkline: SVG polyline chart with 2-minute history sampled every 2 seconds
  - File info with MIME type detection, tooltips on long filenames/paths/URLs
  - Integrity section with SHA-256 checksum status
  - Scrollable logs section fetching last 20 log lines via IPC query
  - Auto-opens when a download is selected, closeable via X button
  - `useDownloadDetail` hook wrapping TanStack Query with 500ms staleTime
  - `useSpeedHistory` hook sampling speed from store at 2s intervals
- Link Grabber View: paste zone, URL validation, link analysis pipeline, package grouping
  - PasteZone with textarea + drag-and-drop URL input
  - FilterBar: All | Online | Offline | Media filter tabs
  - PackageGrouping: group by hostname, extension, or type
  - ActionsBar: Select All, Start Selected, Start All Online, Clear
  - LinkRow with status icons, filename/URL display, media badge
  - ResolvedLinksSection with grouping, multi-select, and group toggle
  - URL validation for http/https/ftp/magnet protocols (case-insensitive)
  - `useTauriMutation` for `link_resolve` and `download_start` commands
- Media Grabber UI: modal dialog for media download options (Task 21)
  - MediaGrabberDialog: orchestrates quality/format/audio/subtitle/playlist selection
  - QualitySelector: grid of video qualities (360p-4K) with resolution, fps, bitrate
  - AudioOnlySection: toggle + audio format selection (M4A, MP3, OGG, WAV, OPUS)
  - SubtitleSelector: multi-select checkbox list of available languages
  - PlaylistSection: scrollable list with individual/bulk select for playlist items
  - SizeEstimate: real-time download size estimation based on quality and duration
  - MediaPreview: thumbnail + title display with broken image fallback
  - `useMediaMetadata` hook fetching metadata via Tauri IPC
  - Integration in LinkGrabberView via clickable media button in LinkRow
  - shadcn/ui Dialog, Card, Skeleton components added to UI library
- Clipboard Observer & System Tray (Task 22)
  - Clipboard monitoring adapter: polls system clipboard every 500ms via `tauri-plugin-clipboard-manager`
  - URL extraction from clipboard text via regex (http, https, ftp, magnet protocols)
  - Duplicate URL detection with seen-set deduplication
  - Toggle clipboard monitoring via `clipboard_toggle` IPC command with config persistence
  - `ClipboardUrlDetected` domain event for frontend notification of detected URLs
  - System tray with menu: Pause All, Resume All, Clipboard Monitoring toggle, Open Window, Quit
  - Tray icon click opens/focuses main window
  - Desktop notification bridge: notifies on download completion and failure via `tauri-plugin-notification`
  - `ClipboardIndicator` component in StatusBar showing monitoring state with toggle
  - `useClipboardMonitoring` React hook with server-confirmed state updates
  - Vitest test infrastructure: jsdom environment, setup file, Tauri API mocks
  - Frontend tests for ClipboardIndicator (4 tests) and useClipboardMonitoring (4 tests)
- Settings View (Task 23)
  - Expanded `AppConfig` domain model from 9 to 32 fields across 6 categories
  - `ConfigPatch` with `apply_patch()` utility for partial config updates
  - `TomlConfigStore` adapter: read/write `~/.config/vortex/config.toml` with atomic writes and auto-defaults
  - `UpdateConfigCommand` handler with input validation (proxy_type, theme, port bounds, limits)
  - `SettingsUpdated` domain event with Tauri bridge forwarding
  - Settings IPC: `settings_get` query + `settings_update` command with camelCase DTO serialization
  - SettingsView with 6-tab sidebar layout (General, Downloads, Network, Remote Access, Browser, Appearance)
  - Shared `SettingToggle` and `SettingNumberInput` field components
  - GeneralSection: download directory, 7 toggle settings
  - DownloadsSection: 5 numeric settings with MB/s speed limit conversion, 2 toggles
  - NetworkSection: proxy type selector with conditional URL, user-agent, DoH, timeout
  - RemoteAccessSection: security warning, web interface/REST API/WebSocket toggles, API key display with show/hide/copy/regenerate
  - BrowserSection: min file size, excluded domains/extensions via comma-separated textarea
  - AppearanceSection: theme selector, 6 accent color presets, compact mode, language selector
  - Event-based cache invalidation for settings changes from external sources
  - 35 frontend tests (SettingsView + 6 sections + settingsStore)
- YouTube WASM Plugin (Task 24)
  - New `plugins/vortex-mod-youtube/` crate targeting `wasm32-wasip1` via `extism-pdk` 1.4
  - `plugin.toml` manifest declaring `subprocess = ["yt-dlp"]` (least-privilege, no HTTP capability)
  - `url_matcher` module: regex-based classification of youtube.com / youtu.be / shorts / playlist / channel URLs with video and playlist ID extraction
  - `metadata` module: serde-based parsing of `yt-dlp --dump-json` (single video) and `--flat-playlist` (JSONL + envelope formats) with automatic audio-only / video-only / muxed classification via `vcodec`/`acodec` inspection
  - `quality_manager` module: format selection by target resolution (360p → 4320p + `Best`) with height-bucket fallback and user container preference (mp4/webm/mkv); audio-only picks highest `abr`
  - `extractor` module: pure helpers to build yt-dlp subprocess requests with `--` sentinel (defense-in-depth against option injection) and parse `SubprocessResponse` envelopes with UTF-8-safe stderr truncation
  - `plugin_api` module (WASM-only, gated behind `cfg(target_family = "wasm")`): `#[plugin_fn]` exports for `can_handle`, `supports_playlist`, `extract_links`, `get_media_variants`, `extract_playlist`; `#[host_fn] extern "ExtismHost"` import of `run_subprocess` with documented safety invariants
  - `PluginError` enum with `thiserror`: `SerdeJson(#[from])` preserves error source chain; dedicated variants for parse errors, subprocess failures, host response errors, unsupported URLs, and quality mismatches
  - 77 native unit tests covering all pure-logic modules (url_matcher, metadata, quality_manager, extractor, ipc handlers) — runs on `x86_64-unknown-linux-gnu` without a WASM runtime
  - Release WASM binary: 1.2 MB stripped with LTO + `opt-level = "z"`
- SoundCloud, Vimeo, and Gallery WASM plugins (Task 25)
  - New crates under `plugins/`: `vortex-mod-soundcloud`, `vortex-mod-vimeo`, `vortex-mod-gallery`, all targeting `wasm32-wasip1` via `extism-pdk` 1.4 and delegating network I/O to the host `http_request` capability
  - **SoundCloud plugin**: `/resolve` API client (api-v2.soundcloud.com) with tagged enum `ResolveResponse` (Track / Playlist / User / Unknown), `classify_url` router covering `soundcloud.com`, `m.soundcloud.com`, `on.soundcloud.com` (single-segment short-links treated as Track), plus `sets/`, `likes`, `reposts`, `tracks`, `albums` paths. Fragment-safe path normalisation (`#recent` no longer misclassifies), artwork upgrade from `-large` to `-t500x500` (handles `.ext`, extensionless, and query-string variants), `client_id` forwarded via host `get_config`. Artist profiles are intentionally rejected by `can_handle` / `supports_playlist` / `ensure_soundcloud_url` until artist pagination is implemented, avoiding a false-positive capability claim. 51 native unit tests.
  - **Vimeo plugin**: oEmbed JSON client (`vimeo.com/api/oembed.json`) for metadata + player config client (`player.vimeo.com/video/<id>/config`) for quality variants (progressive MP4 + HLS). Balanced-brace HTML fallback with single- and double-quoted string tracking, plus a word-boundary marker (`window.playerConfig` / `playerConfig =`) so similarly named variables like `window.playerConfigVersion` cannot derail extraction. Deterministic HLS CDN fallback (lexicographic key order when `default_cdn` is missing). `pick_variant_for_quality` with `2K → 1440` / `4K → 2160` mapping, `filter_audio_only` preserving HLS, plus `default_quality` config honoured by hoisting the matching variant to the head of the returned list. Private-share URLs (`vimeo.com/<id>/<hash>`) are preserved verbatim in the response so the auth token is not dropped. Showcase URLs are rejected by `can_handle` / `supports_playlist` / `extract_links` until token-gated showcase extraction lands. Anchored showcase/album regex rejects malformed trailing segments. 57 native unit tests.
  - **Gallery plugin**: 3 provider backends with dedicated JSON shapes — Imgur album API v3 (Authorization: Client-ID), Reddit submission JSON (native `is_gallery` + single-image preview fallback) with `&amp;` unescaping and deterministic URL-sorted output (single-image fallback accepts `.jpg`/`.png`/… URLs with query strings and fragments). Flickr `flickr.photosets.getPhotos` handles both numeric and string `width_o`/`height_o`, and `{"stat":"fail"}` envelopes surface as a `PluginError::HttpStatus` with the Flickr error `code`/`message` instead of a JSON parse failure. Generic `<img src>` HTML fallback behind a separate `extract_generic` export; relative URLs now resolve against the **page directory** (preserving `gallery/` context), protocol-relative URLs inherit the **page scheme** (no forced `https:`), and `UrlContext` strips `?`/`#` when computing the origin and base directory. `has_non_http_scheme` guard blocks `data:`/`javascript:`/`mailto:`/`blob:` from resolution. Fragment-stripping URL normaliser; `extract_reddit_permalink` no longer double-appends `.json` when the input already ends in `.json`. Post-processing pipeline: `dedupe_links` → `filter_by_min_resolution` (now drops images with a single known dimension below the threshold, not just both-known cases) → `auto_name` (zero-padded `<provider>_<album>_<idx>.<ext>` with album-id sanitisation). Canonical `Provider` enum lives in `url_matcher.rs` and is re-exported from `link.rs`, eliminating the duplicated type surface. Runtime `min_resolution` fallback (`800x600`) now matches the manifest default. 49 native unit tests.
  - Shared host-function envelope pattern: every plugin models `HttpRequest`/`HttpResponse` to mirror `src-tauri/src/adapters/driven/plugin/host_functions.rs`, with `HttpResponse::into_success_body()` mapping 401/403 → `PluginError::Private` and other non-2xx → `PluginError::HttpStatus`
  - `PluginError` per crate via `thiserror` with `SerdeJson(#[from])`, no `.unwrap()` in production paths, no `#[allow(dead_code)]`, no `unsafe` outside documented `#[host_fn]` call sites
  - Release WASM binaries: SoundCloud ~250 KB, Vimeo ~1.12 MB, Gallery ~1.14 MB (all stripped with LTO + `opt-level = "z"`)
- Archive extractor module: native Rust extraction for ZIP, RAR, 7z, TAR (Task 26)
  - Domain types: `ArchiveFormat` enum (9 variants), `ExtractSummary`, `ArchiveEntry`, `ExtractionConfig`
  - `ArchiveExtractor` port trait with detect, extract, list_contents, detect_segments
  - Format detection by magic bytes (ZIP, RAR v4/v5, 7z, TAR ustar) with extension fallback
  - ZIP handler: `zip` crate with AES password support, path traversal protection via `enclosed_name()`
  - TAR handler: plain TAR + GZ/BZ2/XZ/ZSTD compression via `flate2`/`bzip2`/`xz2`/`zstd`
  - RAR handler: `unrar` crate for v4/v5 with password support and graceful error recovery
  - 7z handler: `sevenz-rust2` (pure Rust) with password support and path safety validation
  - Split archive detection: RAR parts (.partNN.rar), 7z segments (.7z.NNN), ZIP spans
  - `VortexArchiveExtractor` composite: format routing, recursive extraction with configurable depth
  - `ExtractArchiveCommand` CQRS handler with `spawn_blocking` for CPU-bound extraction
  - `ListArchiveContentsQuery` handler for archive preview without extraction
- i18n & advanced theming (Task 27)
  - `react-i18next` + `i18next` + `i18next-browser-languagedetector` installed for internationalisation
  - `src/i18n/i18n.ts`: i18next instance initialized with `LanguageDetector` (localStorage → navigator fallback)
  - `src/i18n/locales/en.json` and `src/i18n/locales/fr.json`: complete English and French translations covering navigation, all settings sections, downloads search, and media grabber dialogs
  - `src/hooks/useLanguage.ts`: `useLanguage()` hook for language switching — calls `i18n.changeLanguage()` and persists `locale` to backend config via `settingsStore.updateConfig()`
  - `src/hooks/useAppEffects.ts`: `useAppEffects()` hook applying DOM side-effects on config changes — toggles `compact-mode` class on `<body>` and sets `--color-accent` CSS variable on `:root`
  - All hardcoded UI strings replaced with `t('key')` calls: navigation labels (Sidebar), settings tabs and all 6 settings sections, downloads search bar, media grabber dialog
  - `src/types/layout.ts`: `RouteConfig.label` renamed to `labelKey` (i18n translation key), Sidebar uses `t(route.labelKey)`
  - `src/App.tsx`: `import './i18n/i18n'` added as first import to ensure i18n is initialized before rendering
  - `src/layouts/AppLayout.tsx`: loads `settings_get` on mount, feeds result to `settingsStore` for `useAppEffects` to pick up initial compact mode and accent color
  - `src/index.css`: `body.compact-mode` selector with reduced font size, line height, and spacing overrides
  - Accent color runtime: changing accent color preset updates `--color-accent` CSS variable immediately without reload
  - `UpdateConfigCommand` locale validation: rejects locales not in `["en", "fr", "de", "es", "ja", "zh"]`
  - `src/test-setup.ts`: global `react-i18next` mock returning English translation values via key lookup so all existing tests continue passing
  - 17 new frontend tests: `useLanguage` (4), `useAppEffects` (5), translation key parity en↔fr (8)
  - 2 new Rust tests: `test_handle_update_config_rejects_invalid_locale`, `test_handle_update_config_accepts_valid_locale`
- Release & distribution pipeline (Task 28)
  - `.github/workflows/release.yml`: triggered on `v*.*.*` tags, 6 jobs
    - `create-release`: extracts changelog body from CHANGELOG.md, creates GitHub Release
    - `build-tauri-linux`: builds .deb and .rpm, uploads to release
    - `build-tauri-macos`: builds .dmg with code signing + notarization via xcrun notarytool, uploads to release
    - `build-tauri-windows`: builds .msi with certificate import, uploads to release
    - `publish-flatpak`: builds Flatpak bundle from manifest, uploads to release
    - `update-updater`: generates `latest.json` updater manifest and uploads to release
  - Tauri in-app updater configured in `tauri.conf.json` (plugins.updater, endpoint → GitHub Releases)
  - `tauri-plugin-updater` added to Cargo.toml dependencies
  - `contrib/vortex.service` — systemd user unit for headless/autostart scenarios
  - `contrib/vortex.desktop` — Freedesktop .desktop entry (MimeType magnet + uri-list)
  - `contrib/flatpak/org.vortex.Vortex.yml` — Flatpak manifest (runtime 23.08, Rust + Node 22 SDK)
  - `contrib/icons/README.md` — icon generation instructions via `npx tauri icon`
  - `contrib/winget/Vortex.yaml` — Winget manifest template (TODO placeholders for future submission)
  - `contrib/homebrew/vortex.rb` — Homebrew cask template (TODO placeholders for future submission)

### Changed

- `KeyringCredentialStore` replaces `NoopCredentialStore` as the default credential adapter (#35)
  - Credentials now persist in the OS keychain (macOS Keychain, Linux Secret Service/keyutils, Windows Credential Manager)
  - `NoopCredentialStore` remains available for tests

### Fixed

- HTML `lang` attribute now updates when the app locale changes — screen readers and browser features use the correct language pronunciation rules (#33)
- Link Grabber now shows an inline error message when `link_resolve` fails, instead of silently resetting after "Analyze Links" (#29)
- Settings view now displays the actual backend error message when `settings_get` fails, instead of only a generic "Failed to load settings" (#28)
- **CRITICAL**: All 22 IPC commands now work — `AppState` is constructed and registered via `.manage()` in the Tauri setup closure (#27)
  - Database connection (SQLite WAL mode) with migrations run at startup
  - All driven adapters wired: event bus, file storage, HTTP client, config store, clipboard observer, plugin loader, download engine, archive extractor
  - CQRS buses (CommandBus + QueryBus) assembled from 15 driven ports
  - Event bridges (Tauri webview + desktop notifications) connected to domain event bus
  - Plugin hot-reload watcher started with tracing on failure
  - Shared `reqwest::Client` between HTTP metadata port and download engine
  - `NoopCredentialStore` stub for tests (replaced by `KeyringCredentialStore` as default in #35)
  - `InMemoryStatsRepository` stub for unit tests (replaced by `SqliteStatsRepo` as default in #36)
- Status bar now shows real available disk space instead of `-- GB free` — new `status_bar_get` Tauri IPC command reads available bytes via `statvfs` (Unix) or `GetDiskFreeSpaceExW` (Windows) from the configured download directory, with fallback to the system Downloads folder then the current directory (#32)
- Status bar text now follows the UI language — `AppLayout` syncs `settings_get.locale` into i18next on startup so all status bar strings (`statusBar.*`) render in the active language; English and French translations are complete (#32)
