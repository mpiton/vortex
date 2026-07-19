# Vortex

[![Release](https://img.shields.io/github/v/release/mpiton/vortex?include_prereleases&label=release)](https://github.com/mpiton/vortex/releases/latest)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-24c8db?logo=tauri)](https://tauri.app)
[![Rust](https://img.shields.io/badge/rust-1.95-orange?logo=rust)](src-tauri/Cargo.toml)
[![CI](https://github.com/mpiton/vortex/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/mpiton/vortex/actions/workflows/ci.yml)
[![CodSpeed](https://img.shields.io/endpoint?url=https://codspeed.io/badge.json)](https://codspeed.io/mpiton/vortex?utm_source=badge)

Open-source desktop download manager — successor to JDownloader. Tauri 2 + Rust backend + React 19 frontend, hexagonal architecture, CQRS, WASM plugin system (Extism).

> **Status: v0.3.0-beta.1** (2026-07-19). Phase 1 of the v2 roadmap shipped — premium accounts (keyring-backed) wired end to end, file hosters (MediaFire, PixelDrain, Gofile), container files (DLC/CCF/RSDF/Metalink), per-image gallery rows in the Link Grabber, and a three-OS CI merge gate with a blocking E2E smoke test. Targeted at testers; CAPTCHA solving, MEGA decryption, REST API and browser extension are deferred to v0.4+. See [CHANGELOG.md](CHANGELOG.md) for the full feature list and [PRD-v2.md](PRD-v2.md) for the v1.0 roadmap.

## Install (v0.3.0-beta.1)

> ⚠️ Beta binaries on macOS and Windows ship **unsigned**. First-launch Gatekeeper / SmartScreen warnings are expected — see the per-platform notes below to bypass them.

### Linux

| Format | Command |
|--------|---------|
| Debian / Ubuntu (`.deb`) | `wget https://github.com/mpiton/vortex/releases/download/v0.3.0-beta.1/Vortex_0.3.0-beta.1_amd64.deb && sudo dpkg -i Vortex_0.3.0-beta.1_amd64.deb` |
| Fedora / RHEL (`.rpm`) | `sudo rpm -i https://github.com/mpiton/vortex/releases/download/v0.3.0-beta.1/Vortex-0.3.0-beta.1-1.x86_64.rpm` |
| Portable (`.AppImage`) | `wget https://github.com/mpiton/vortex/releases/download/v0.3.0-beta.1/Vortex_0.3.0-beta.1_amd64.AppImage && chmod +x Vortex_0.3.0-beta.1_amd64.AppImage && ./Vortex_0.3.0-beta.1_amd64.AppImage` |
| Flatpak | `flatpak install --user vortex.flatpak` (download from the [release page](https://github.com/mpiton/vortex/releases/tag/v0.3.0-beta.1)) |

### macOS (universal — Apple Silicon + Intel)

```bash
curl -LO https://github.com/mpiton/vortex/releases/download/v0.3.0-beta.1/Vortex_0.3.0-beta.1_universal.dmg
open Vortex_0.3.0-beta.1_universal.dmg
# Drag Vortex.app to /Applications
# First launch: right-click Vortex.app → Open → Open (bypasses Gatekeeper)
```

If macOS blocks with "cannot be opened because the developer cannot be verified":
```bash
xattr -dr com.apple.quarantine /Applications/Vortex.app
```

### Windows

| Format | Notes |
|--------|-------|
| MSI installer | [`Vortex_0.3.0-beta.1_x64_en-US.msi`](https://github.com/mpiton/vortex/releases/download/v0.3.0-beta.1/Vortex_0.3.0-beta.1_x64_en-US.msi) — recommended for system-wide install |
| NSIS setup | [`Vortex_0.3.0-beta.1_x64-setup.exe`](https://github.com/mpiton/vortex/releases/download/v0.3.0-beta.1/Vortex_0.3.0-beta.1_x64-setup.exe) — per-user install |

SmartScreen will warn "Windows protected your PC" → click *More info* → *Run anyway*.

## Features (v0.3.0-beta.1)

- **Segmented downloads** — parallel HTTP Range workers with dynamic split (slow-tail rebalancing) and `.vortex-meta` resume across restarts
- **Premium accounts** — add hoster credentials (stored in the OS keyring), validated and injected into plugin requests end to end
- **File hosters** — MediaFire, PixelDrain and Gofile link resolution; MEGA and 1fichier-free links are detected and refused with an explicit reason (no CAPTCHA solver, no host-side decryption yet)
- **Container files** — DLC / CCF / RSDF / Metalink import in the Link Grabber
- **Queue manager** — drag-and-drop reorder, Move-to-top / -bottom, priority-aware scheduling, configurable concurrency (1-20)
- **Integrity** — SHA-256 / MD5 verification on completion, mismatch surfaces with expected vs. computed hash
- **History view** — group-by-day, filter tabs, debounced search, CSV / JSON export, retention purge worker (7 / 30 / 90 / 365 / unlimited days)
- **Statistics view** — daily volume, top hosts, type breakdown, average speed, top modules — over 7d / 30d / all-time periods
- **WASM plugin system** — install / hot-reload / configure crawlers from a built-in registry (YouTube, Vimeo, SoundCloud, Gallery shipped); typed config UI per plugin
- **Tray** — pulse animation while transfers active; Pause / Resume / Quit shortcuts
- **Notifications** — completion shows `{filename} · {size}`, failure shows `{filename} · Error: {reason}`, 5s grouper for bursts
- **Auto-updater** — Tauri-signed updater bundle published on every release; in-app dialog prompts when a new version ships

The full PRD-v2 §P0 acceptance matrix is in [`docs/PRD.md`](docs/PRD.md) and the v1.0 roadmap in [`PRD-v2.md`](PRD-v2.md).

## Architecture

Hexagonal + CQRS, dependency rule `adapters/ → application/ → domain/`:

```
src-tauri/src/
├── domain/          # Pure entities (Download, Segment, Package…), state machine,
│                    # domain events, ports (traits). ZERO external deps.
├── application/     # Command Handlers (mutations) + Query Handlers (reads),
│                    # CommandBus / QueryBus, async services.
└── adapters/
    ├── driving/     # Tauri IPC, REST axum (planned), CLI (planned)
    └── driven/      # SQLite (sea-orm), filesystem, reqwest, Extism, keyring,
                     # clipboard, tray, notifications, scheduler
```

Plugin runtime: **Extism** sandbox. Each plugin is a `.wasm` + `plugin.toml` manifest declaring capabilities. Plugins live in the Tauri app data dir — `~/.local/share/dev.vortex.app/plugins/<name>/` on Linux, `~/Library/Application Support/dev.vortex.app/plugins/<name>/` on macOS, `%APPDATA%\dev.vortex.app\plugins\<name>\` on Windows — and hot-reload via filesystem watcher.

See [`ARCHI.md`](ARCHI.md) for the full architecture documentation.

## Build from source

```bash
# Prerequisites
rustup install 1.95.0  # or use the rust-toolchain.toml pin
node --version          # 22.x recommended
sudo apt install libwebkit2gtk-4.1-dev build-essential libssl-dev libappindicator3-dev librsvg2-dev  # Linux

# Clone + dev
git clone https://github.com/mpiton/vortex.git
cd vortex
npm install
npm run tauri dev

# Release build
npm run tauri build
# Output: src-tauri/target/release/bundle/{deb,rpm,appimage,dmg,msi,nsis}/
```

A Nix flake is provided for a reproducible toolchain (`nix develop`).

## Plugin development

Plugins are independent crates compiled to `wasm32-wasip1`. The four official plugins live in sibling repos:

- [`vortex-mod-youtube`](https://github.com/mpiton/vortex-mod-youtube) — YouTube via the host's typed yt-dlp broker
- [`vortex-mod-vimeo`](https://github.com/mpiton/vortex-mod-vimeo) — Vimeo crawler
- [`vortex-mod-soundcloud`](https://github.com/mpiton/vortex-mod-soundcloud) — SoundCloud crawler
- [`vortex-mod-gallery`](https://github.com/mpiton/vortex-mod-gallery) — generic image gallery extractor

Quickstart for a new plugin:

```bash
cargo new --lib vortex-mod-myhost
cd vortex-mod-myhost
# Add Cargo.toml: crate-type = ["cdylib", "rlib"] + extism-pdk dep
# Implement #[plugin_fn] can_handle / extract_links / get_media_variants / …
cargo build --target wasm32-wasip1 --release
cp target/wasm32-wasip1/release/vortex_mod_myhost.wasm ~/.local/share/dev.vortex.app/plugins/vortex-mod-myhost/plugin.wasm
cp plugin.toml ~/.local/share/dev.vortex.app/plugins/vortex-mod-myhost/
# Hot-reload picks it up; check Plugins view → Installed.
```

A typed SDK crate (`vortex-plugin-sdk`) is on the v1.0 roadmap to make this faster.

## Roadmap

| Version | Target | Theme |
|---------|--------|-------|
| ✅ **v0.2.0-beta** | 2026-04-28 | Phase 0 — every placeholder view replaced, integrity, queue UX, plugin config UI |
| ✅ **v0.3.0-beta.1** | 2026-07-19 | Phase 1 — Accounts (premium), file hosters (MediaFire, PixelDrain, Gofile), containers (DLC/CCF/RSDF/Metalink), gallery rows, CI merge gate + E2E smoke |
| v0.4 | 2026-09-15 | Phase 2 — CAPTCHA pipeline, Real-Debrid / AllDebrid, Scheduler, automation rules, reconnect IP |
| v1.0 | 2026-12-15 | Phase 3 — REST API + WebSocket, Web UI, browser extension, Click'n'Load, headless mode, i18n, Flathub |

See [`PRD-v2.md`](PRD-v2.md) for the per-task breakdown.

## Feedback

This is a **beta release** — bugs, rough edges and missing flows are expected. Two channels:

- **Bugs** → [open an issue](https://github.com/mpiton/vortex/issues/new?template=bug_report.yml) with the *Vortex version* dropdown set to `v0.3.0-beta.1`
- **Discussions** → [GitHub Discussions](https://github.com/mpiton/vortex/discussions) for general impressions, missing features, plugin requests

## License

[GPL-3.0-only](LICENSE) — same as JDownloader's spirit, source-available + copyleft.

## Acknowledgements

Built on the shoulders of [Tauri](https://tauri.app), [Extism](https://extism.org), [yt-dlp](https://github.com/yt-dlp/yt-dlp), [shadcn/ui](https://ui.shadcn.com), [TanStack Query/Table](https://tanstack.com), and the GNOME / freedesktop runtimes.
