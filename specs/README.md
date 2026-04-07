# Implementation Tasks Overview

## Project Summary
**From PRD**: Vortex est un gestionnaire de telechargements desktop open-source (GPLv3), successeur de JDownloader. Architecture plugin-first, Tauri 2 + Rust backend + React frontend. Cible les power users Linux/macOS/Windows.
**Tech Stack**: Tauri 2, Rust (tokio, sea-orm, axum, reqwest, extism), React 19, TypeScript, Zustand, TanStack Query/Table, shadcn/ui, Tailwind CSS 4
**Architecture**: Hexagonale (Ports & Adapters) + CQRS + TDD (Red-Green-Refactor)
**Current State**: Fondation initiale en place — scaffolding, CI/quality gates, et modeles de domaine implementes.

## Task Execution Guidelines

- Lire la tache complete avant de commencer
- Verifier que les dependances (taches precedentes) sont completes
- Suivre l'architecture hexagonale : domain (pur) → application (CQRS) → adapters
- TDD obligatoire : ecrire les tests RED d'abord, puis implementer GREEN, puis REFACTOR
- Valider contre les success criteria avant de marquer comme complete
- Respecter les conventions du CLAUDE.md (thiserror, pas de .unwrap(), oxlint, etc.)
- **Quality gates actifs des la tache 02** : chaque commit passe par les hooks pre-commit/pre-push

## MVP Tasks (specs/01-mvp/)

### Phase 1: Foundation
- [x] [`01-project-scaffolding.md`](01-mvp/01-project-scaffolding.md) — Tauri 2 + Vite + React + Tailwind + shadcn/ui + Nix flake
- [x] [`02-ci-quality-gates.md`](01-mvp/02-ci-quality-gates.md) — CI pipeline + lefthook pre-commit/pre-push + GitHub Actions + templates
- [x] [`03-domain-models.md`](01-mvp/03-domain-models.md) — Entites domaine, state machine, events, errors, value objects
- [x] [`04-domain-ports.md`](01-mvp/04-domain-ports.md) — Traits driven/driving (tous les ports hexagonaux)
- [ ] [`05-cqrs-infrastructure.md`](01-mvp/05-cqrs-infrastructure.md) — CommandBus, QueryBus, Command/Query traits, AppError, read models
- [ ] [`06-sqlite-persistence.md`](01-mvp/06-sqlite-persistence.md) — sea-orm entities, migrations, write + read repository adapters
- [ ] [`07-event-system.md`](01-mvp/07-event-system.md) — TokioEventBus, Tauri emit bridge, frontend subscription hooks

### Phase 2: Download Engine
- [ ] [`08-download-engine.md`](01-mvp/08-download-engine.md) — Moteur de telechargement segmente (reqwest, parallel segments)
- [ ] [`09-file-storage-resume.md`](01-mvp/09-file-storage-resume.md) — Pre-allocation disque, segment writes, .vortex-meta, resume
- [ ] [`10-queue-circuit-breaker.md`](01-mvp/10-queue-circuit-breaker.md) — File d'attente prioritaire, slots, circuit breaker, retry exponentiel

### Phase 3: CQRS Handlers
- [ ] [`11-download-commands.md`](01-mvp/11-download-commands.md) — StartDownload, Pause, Resume, Cancel, Retry, PauseAll + IPC wiring
- [ ] [`12-download-queries.md`](01-mvp/12-download-queries.md) — GetDownloads, GetDownloadDetail, CountByState + read repos optimises

### Phase 4: Plugin System
- [ ] [`13-plugin-infrastructure.md`](01-mvp/13-plugin-infrastructure.md) — Extism WASM loader, registry, manifest, hot-reload watcher
- [ ] [`14-plugin-host-functions.md`](01-mvp/14-plugin-host-functions.md) — Host functions (http, log, config, state, credential), capabilities
- [ ] [`15-core-http-module.md`](01-mvp/15-core-http-module.md) — Module HTTP/HTTPS/FTP built-in (natif Rust, catch-all)

### Phase 5: Frontend Foundation
- [ ] [`16-frontend-layout.md`](01-mvp/16-frontend-layout.md) — Sidebar, StatusBar, AppLayout, routing 10 vues, theme light/dark
- [ ] [`17-tauri-ipc-data-layer.md`](01-mvp/17-tauri-ipc-data-layer.md) — Typed invoke/listen, TanStack Query hooks, Zustand stores
- [ ] [`18-downloads-view.md`](01-mvp/18-downloads-view.md) — TanStack Table virtualise, filtres, tri, recherche, actions groupees
- [ ] [`19-download-details-panel.md`](01-mvp/19-download-details-panel.md) — Sidebar droite, segments bar, sparkline vitesse, file info
- [ ] [`20-link-grabber-view.md`](01-mvp/20-link-grabber-view.md) — Paste zone, link analysis pipeline, package grouping
- [ ] [`21-media-grabber-ui.md`](01-mvp/21-media-grabber-ui.md) — Selecteur qualite/format/sous-titres pour modules crawler

### Phase 6: System Features
- [ ] [`22-clipboard-tray.md`](01-mvp/22-clipboard-tray.md) — Clipboard observer, URL detection, system tray, notifications
- [ ] [`23-settings.md`](01-mvp/23-settings.md) — config.toml CRUD, Settings view UI (General, Downloads, Network, Appearance)

### Phase 7: First Plugins
- [ ] [`24-youtube-plugin.md`](01-mvp/24-youtube-plugin.md) — YouTube WASM plugin (yt-dlp subprocess, video/playlist/quality)
- [ ] [`25-media-plugins.md`](01-mvp/25-media-plugins.md) — SoundCloud + Vimeo + Gallery WASM plugins
- [ ] [`26-archive-extractor.md`](01-mvp/26-archive-extractor.md) — ZIP, RAR, 7z, TAR.* extraction, auto-extract, split archives

### Phase 8: Polish
- [ ] [`27-i18n-theming.md`](01-mvp/27-i18n-theming.md) — react-i18next, en/fr, accent color system, compact mode
- [ ] [`28-release-distribution.md`](01-mvp/28-release-distribution.md) — Tauri updater, .deb/.rpm/.msi packaging, CHANGELOG workflow

## Dependency Map

```
                    ┌──────────────────┐
                    │ 01 Scaffolding   │
                    └────────┬─────────┘
              ┌──────────────┼──────────────┐
              ▼              ▼              ▼
       ┌────────────┐ ┌──────────┐   ┌──────────┐
       │02 CI+Hooks │ │16 Layout │   │28 Release│
       │pre-commit  │ └────┬─────┘   └──────────┘
       │pre-push    │      ▼
       └────────────┘ ┌──────────┐
              │       │17 IPC    │
              ▼       │ Layer    │
       ┌──────────┐   └────┬─────┘
       │03 Domain  │   ┌───┼───┬───┬───┬───┐
       │ Models    │   ▼   ▼   ▼   ▼   ▼   ▼
       └────┬─────┘  18  19  20  21  22  23
            ▼         ▼   ▼
       ┌──────────┐  19  21
       │04 Ports  │
       └────┬─────┘
  ┌────┬────┼────┬───┐
  ▼    ▼    ▼    ▼   ▼
┌──┐ ┌──┐ ┌──┐ ┌──┐┌──┐
│05│ │06│ │07│ │08││13│
└┬─┘ └──┘ └┬─┘ └┬─┘└┬─┘
 │          ▼    ▼   ▼
 │       ┌──┐ ┌──┐ ┌──┐
 │       │09│ │10│ │14│
 │       └──┘ └──┘ └┬─┘
 ▼                ┌─┼──┐
┌──────────┐      │ ▼  ▼
│11 Cmds   │    ┌──┐┌──┐┌──┐
│12 Queries│    │15││24││25│
└──────────┘    └──┘└──┘└──┘
                      ▼
                    ┌──┐
                    │26│
                    └──┘
```

## PRD Phase 1 Coverage

- PRD §7.1 Moteur de telechargement : Tasks 08, 09, 10
- PRD §2 Architecture modulaire : Tasks 13, 14, 15
- PRD §2.4 Module API (traits) : Task 04
- PRD §6.1 Vue Downloads : Tasks 18, 19
- PRD §6.2 Link Grabber : Tasks 20, 21
- PRD §6.10 Settings : Task 23
- PRD §7.3 Clipboard Observer : Task 22
- PRD §7.5 System Tray : Task 22
- PRD §3.6 Module HTTP : Task 15
- PRD §3.1 Module YouTube : Task 24
- PRD §3.3 Module SoundCloud : Task 25
- PRD §3.2 Module Vimeo : Task 25
- PRD §3.7 Module Extract : Task 26
- PRD §3.8 Module Gallery : Task 25
- PRD §1.2 Themes : Tasks 16, 27

## Total Estimated Time: 56-84 hours (28 tasks x 2-3h)
