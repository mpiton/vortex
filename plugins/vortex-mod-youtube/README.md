# vortex-mod-youtube

YouTube WASM plugin for [Vortex](https://github.com/mpiton/vortex).

## Features

- Videos, playlists, channels, YouTube Shorts
- Quality selection (360p to 4320p / 8K)
- Video formats: MP4, WEBM, MKV
- Audio-only: M4A, MP3, OGG, OPUS, WAV
- Subtitles (auto-generated + manual), SRT/VTT
- Thumbnails and metadata extraction

## Requirements

- `yt-dlp` installed and available on `PATH` (host-side).
- Vortex plugin host ≥ 0.1.0.

## Build

```bash
rustup target add wasm32-wasip1
cargo build --release
```

The resulting WASM binary is at `target/wasm32-wasip1/release/vortex_mod_youtube.wasm`.

## Install

The Vortex plugin loader enforces two rules (see
`src-tauri/src/adapters/driven/plugin/manifest.rs` for the exact validation):

1. The plugin directory name must match the `name` field in `plugin.toml`.
2. The directory must contain exactly one `.wasm` file — the filename itself
   is not pinned, so `vortex_mod_youtube.wasm` and `vortex-mod-youtube.wasm`
   are both accepted as long as there is only one of them.

Cargo produces `target/wasm32-wasip1/release/vortex_mod_youtube.wasm`
(underscores — Cargo's default crate-name-to-artifact mapping). The
instructions below rename it to match the directory purely as a convention;
you may leave the underscore form if you prefer.

```bash
mkdir -p ~/.config/vortex/plugins/vortex-mod-youtube
cp plugin.toml ~/.config/vortex/plugins/vortex-mod-youtube/
cp target/wasm32-wasip1/release/vortex_mod_youtube.wasm \
   ~/.config/vortex/plugins/vortex-mod-youtube/vortex-mod-youtube.wasm
```

Final layout:

```text
~/.config/vortex/plugins/vortex-mod-youtube/
  ├── plugin.toml
  └── vortex-mod-youtube.wasm
```

## Tests

```bash
cargo test --target x86_64-unknown-linux-gnu
```

Pure logic modules (`url_matcher`, `metadata`, `quality_manager`) are native-testable without WASM runtime.
