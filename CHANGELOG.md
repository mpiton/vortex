# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
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
