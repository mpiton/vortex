# vortex-mod-vimeo

Vimeo WASM plugin for [Vortex](https://github.com/mpiton/vortex).

## Features

- Single public video extraction via the oEmbed endpoint
- Private-link videos (`vimeo.com/<id>/<hash>`) recognised and proxied
  through the same oEmbed call
- Quality variants parsed from the player config JSON
  (`player.vimeo.com/video/<id>/config`), including HLS adaptive fallback
- Audio-only preference (`extract_audio_only` config) preserves HLS and
  drops progressive MP4 variants
- Quality selection helper with `2K → 1440p` and `4K → 2160p` mapping

## Requirements

- Vortex plugin host ≥ 0.1.0 with `http_request` and `get_config`
  host functions enabled.

## Build

```bash
rustup target add wasm32-wasip1
cargo build --release
```

Resulting WASM: `target/wasm32-wasip1/release/vortex_mod_vimeo.wasm`.

## Install

```bash
mkdir -p ~/.config/vortex/plugins/vortex-mod-vimeo
cp plugin.toml ~/.config/vortex/plugins/vortex-mod-vimeo/
cp target/wasm32-wasip1/release/vortex_mod_vimeo.wasm \
   ~/.config/vortex/plugins/vortex-mod-vimeo/vortex-mod-vimeo.wasm
```

## Tests

```bash
cargo test --target x86_64-unknown-linux-gnu
```

Pure parsing modules (`url_matcher`, `parser`, response builders) are
covered natively with hardcoded oEmbed and player-config fixtures.
