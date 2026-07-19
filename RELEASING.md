# Releasing Vortex

Checklist for cutting an app release (example: `v0.3.0-beta.1`). Each item maps to an
acceptance criterion R-01 → R-07 of the release ticket. Nothing here pushes a tag —
publication is the explicit human decision at the very end.

## 1. Prerequisites

- [ ] All blocking tickets of the release ticket are Done.
- [ ] `main` is green (CI merge gate: fmt, clippy `-D warnings`, `cargo test --workspace`,
      vitest, oxlint, 3-OS build matrix, E2E smoke, plugin CI). **(R-01)**
- [ ] Working tree clean, on a release-prep branch created from up-to-date `main`.

## 2. Version bump

Update the version in:

- `package.json` (+ `package-lock.json` via `npm install --package-lock-only --ignore-scripts`)
- `src-tauri/Cargo.toml` (+ `Cargo.lock` via `cargo update -p vortex --offline`)
- `src-tauri/tauri.conf.json` — `version`, and `bundle.windows.wix.version`
  (numeric `x.y.z.w`: keep the semver core, bump the 4th digit per beta iteration,
  e.g. `0.3.0-beta.1` → `0.3.0.1`)

Then verify:

```bash
scripts/check-release-versions.sh 0.3.0-beta.1
```

The same script runs in CI (`release.yml` → verify-tag), so a mismatch also fails the
tag build. **(R-02)**

## 3. Changelog and docs

- [ ] `CHANGELOG.md`: cut a `## [X.Y.Z] - YYYY-MM-DD` section from `[Unreleased]`,
      with Highlights, Known limitations and Upgrade notes.
- [ ] Release notes claim **only verified capabilities** — no CAPTCHA solving, no MEGA
      decryption, no remote access (REST/WS/Web UI) until they actually ship. **(R-06)**
- [ ] `README.md`: status line, install URLs/filenames, features heading, roadmap row,
      feedback links. Asset filenames are predictions until CI builds them — re-check
      against the real release assets after the tag build (step 6).

## 4. Plugins

- [ ] Every plugin in `registry/registry.toml` passes its own CI (WASM build
      `wasm32-wasip1` + ABI smoke). **(R-05)**
- [ ] `cargo test --workspace` includes `registry_coherence` — registry versions,
      checksums format and `min_vortex_version` compatibility are asserted there.
- [ ] Registry `checksum_sha256` values come from each plugin's CI release
      `SHA256SUMS`, never from a local wasm build (local builds are not
      byte-reproducible).

## 5. Pre-tag audit

- [ ] `cargo audit` / `npm audit` clean or triaged. **(R-01)**
- [ ] No secrets in the diff, no stale artifacts, no `specs/` files tracked
      (`git ls-files | grep -E '^specs/'` must be empty). **(R-07)**
- [ ] Full local suite one last time:

```bash
cargo clippy --workspace -- -D warnings
cargo test --workspace
npx vitest run
npx oxlint .
scripts/check-release-versions.sh <version>
```

## 6. Tag → CI does the rest

Pushing the tag **is** publishing. It is a deliberate human action, never automated:

```bash
git tag -a v0.3.0-beta.1 -m "v0.3.0-beta.1"
git push origin v0.3.0-beta.1
```

`release.yml` then: verifies versions (step 2's script), creates the GitHub Release
(prerelease if the tag contains `-`), builds Linux/macOS/Windows bundles, publishes
the flatpak, updates the updater manifest, and uploads `SHA256SUMS` +
`PROVENANCE.txt` (tagged commit + workflow run URL). **(R-03)**

## 7. Post-build verification

- [ ] Download one bundle per OS, `sha256sum --check --ignore-missing SHA256SUMS`. **(R-03)**
- [ ] Clean install boots to the Downloads view. **(R-04)**
- [ ] Upgrade over the previous version preserves `config.toml`, `vortex.db` and
      in-flight downloads (SQLite migrations run automatically). **(R-04)**
- [ ] README install commands match the actual asset filenames (fix forward if not).

If a check fails after the tag: fix on a branch, bump to the next iteration
(`-beta.2`), and go back to step 2. Never move or reuse a published tag.
