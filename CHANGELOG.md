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
  - EventBus sync-to-async bridge using `mpsc::unbounded_channel`
  - Idempotent `on_slot_freed()` via `tokio::sync::Mutex` scheduling lock
