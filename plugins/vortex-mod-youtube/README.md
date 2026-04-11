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

The Vortex plugin loader expects the binary filename to match the plugin name
from `plugin.toml` (directory naming convention enforced in
`src-tauri/src/adapters/driven/plugin/manifest.rs`). Copy the manifest and
rename the build artifact to match the directory:

```bash
mkdir -p ~/.config/vortex/plugins/vortex-mod-youtube
cp plugin.toml ~/.config/vortex/plugins/vortex-mod-youtube/
cp target/wasm32-wasip1/release/vortex_mod_youtube.wasm \
   ~/.config/vortex/plugins/vortex-mod-youtube/vortex-mod-youtube.wasm
```

Final layout:

```
~/.config/vortex/plugins/vortex-mod-youtube/
  ├── plugin.toml
  └── vortex-mod-youtube.wasm
```

## Tests

```bash
cargo test --target x86_64-unknown-linux-gnu
```

Pure logic modules (`url_matcher`, `metadata`, `quality_manager`) are native-testable without WASM runtime.
