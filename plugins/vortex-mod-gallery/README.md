# vortex-mod-gallery

Gallery WASM plugin for [Vortex](https://github.com/mpiton/vortex):
extracts direct image URLs from Imgur albums, Reddit galleries,
Flickr photosets, and a generic `<img>` fallback.

## Features

- **Imgur**: `imgur.com/a/<id>` and `imgur.com/gallery/<id>` via the v3
  Album API (Authorization: Client-ID header)
- **Reddit**: native gallery (`is_gallery: true` + `media_metadata`)
  and single-image submissions (preview source fallback) with
  `&amp;` unescaping
- **Flickr**: `flickr.photosets.getPhotos` REST API with `extras=url_o,url_l`
- **Generic**: `<img src=…>` scraping with scheme guard
  (`data:` / `javascript:` / `mailto:` / `blob:` rejected) and
  relative-URL resolution against the page origin
- Post-processing: dedupe, minimum-resolution filter, auto-naming
  (`<provider>_<album>_<idx>.<ext>`)

## Requirements

- Vortex plugin host ≥ 0.1.0 with `http_request` and `get_config`
  host functions enabled.
- Imgur API `client_id` (config key `imgur_client_id`).
- Flickr API key (config key `flickr_api_key`).

## Build

```bash
rustup target add wasm32-wasip1
cargo build --release
```

Resulting WASM: `target/wasm32-wasip1/release/vortex_mod_gallery.wasm`.

## Install

```bash
mkdir -p ~/.config/vortex/plugins/vortex-mod-gallery
cp plugin.toml ~/.config/vortex/plugins/vortex-mod-gallery/
cp target/wasm32-wasip1/release/vortex_mod_gallery.wasm \
   ~/.config/vortex/plugins/vortex-mod-gallery/vortex-mod-gallery.wasm
```

## Tests

```bash
cargo test --target x86_64-unknown-linux-gnu
```

Every provider parser is covered with hardcoded JSON fixtures. The
generic HTML scraper has dedicated tests for relative-URL resolution
and scheme filtering.
