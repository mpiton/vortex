# Vortex Plugin Registry

## How to publish your plugin

### Prerequisites
- Your plugin is a `.wasm` binary built with the Extism PDK
- You have a GitHub repository for your plugin

### Step 1 — Release on GitHub

Create a GitHub Release tagged `v{version}` (e.g. `v1.0.0`) with two assets:
- `{name}.wasm` — your compiled WASM binary
- `plugin.toml` — your plugin manifest (capabilities, config schema)

### Step 2 — Get your checksum

```bash
sha256sum vortex-mod-example.wasm
```

### Step 3 — Open a PR

1. Fork this repository
2. Copy `registry/TEMPLATE.toml` and add a `[[plugin]]` entry to `registry/registry.toml`
3. Open a PR — the maintainer will verify your release exists and the checksum matches

---

## Download URL convention

Vortex constructs download URLs automatically:

```
{repository}/releases/download/v{version}/{name}.wasm
{repository}/releases/download/v{version}/plugin.toml
```

Make sure your GitHub Release assets follow this naming convention.
