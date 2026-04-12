# vortex-mod-soundcloud

SoundCloud WASM plugin for [Vortex](https://github.com/mpiton/vortex).

## Features

- Single track resolution with title, artist, duration, artwork
- Playlist / album extraction (`/sets/`), plus `/likes`, `/tracks`, `/albums`
- Artwork upgraded from the 100×100 default to the 500×500 variant when
  the CDN URL follows the `-large` marker convention
- `client_id` is read from host config (`get_config` → `client_id`) so
  that the user can supply their own without rebuilding the plugin

## Requirements

- Vortex plugin host ≥ 0.1.0 with `http_request` and `get_config`
  host functions enabled.

## Build

```bash
rustup target add wasm32-wasip1
cargo build --release --target wasm32-wasip1
```

The resulting WASM binary is at
`target/wasm32-wasip1/release/vortex_mod_soundcloud.wasm`.

> Note: the crate ships a `.cargo/config.toml` that sets
> `target = "wasm32-wasip1"`, so `cargo build --release` alone also
> works inside the crate directory. The explicit flag above is given
> so that the command works from any working directory.

## Install

The Vortex plugin loader enforces two rules:

1. The plugin directory name must match the `name` field in `plugin.toml`.
2. The directory must contain exactly one `.wasm` file.

```bash
mkdir -p ~/.config/vortex/plugins/vortex-mod-soundcloud
cp plugin.toml ~/.config/vortex/plugins/vortex-mod-soundcloud/
cp target/wasm32-wasip1/release/vortex_mod_soundcloud.wasm \
   ~/.config/vortex/plugins/vortex-mod-soundcloud/vortex-mod-soundcloud.wasm
```

## Tests

```bash
cargo test --target x86_64-unknown-linux-gnu
```

All URL classification, JSON parsing, and IPC helpers are
native-testable — tests use hardcoded JSON fixtures so they run without
a WASM runtime or a live SoundCloud account.
