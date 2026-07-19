#!/usr/bin/env bash
# Verify that every version declaration agrees before tagging a release.
# Usage: scripts/check-release-versions.sh [expected-version]
# Without an argument, package.json is the reference. Invoked locally from
# RELEASING.md and in CI by the release.yml verify-tag job.
set -euo pipefail

cd "$(dirname "$0")/.."

fail=0
err() {
  echo "::error::$*" >&2
  fail=1
}

pkg_version=$(node -p "require('./package.json').version")
cargo_version=$(grep -m1 '^version' src-tauri/Cargo.toml | cut -d'"' -f2)
tauri_version=$(node -p "require('./src-tauri/tauri.conf.json').version")
wix_version=$(node -p "require('./src-tauri/tauri.conf.json').bundle.windows.wix.version")

expected="${1:-$pkg_version}"

[ "$pkg_version" = "$expected" ] || err "package.json version ($pkg_version) != expected ($expected)"
[ "$cargo_version" = "$expected" ] || err "src-tauri/Cargo.toml version ($cargo_version) != expected ($expected)"
[ "$tauri_version" = "$expected" ] || err "src-tauri/tauri.conf.json version ($tauri_version) != expected ($expected)"

# Windows MSI needs a strictly numeric x.y.z.w version whose x.y.z prefix
# matches the semver core of the release (prerelease suffix stripped).
core="${expected%%-*}"
if ! [[ "$wix_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ && "$wix_version" == "$core".* ]]; then
  err "tauri.conf.json wix version ($wix_version) must be numeric x.y.z.w starting with $core."
fi

# Fixed-string match: a `.` in an unescaped regex would match any character.
if ! grep -qF "## [${expected}]" CHANGELOG.md; then
  err "CHANGELOG.md has no section for version ${expected}"
fi

if [ "$fail" -ne 0 ]; then
  exit 1
fi
echo "OK: all release version declarations agree on ${expected} (wix: ${wix_version})"
