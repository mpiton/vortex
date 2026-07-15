#!/usr/bin/env bash
set -euo pipefail

ROOT=$(git rev-parse --show-toplevel)
SCANNER="$ROOT/scripts/no-secrets.sh"
REPO=$(mktemp -d)

assert_rejected() {
    local repo=$1
    local label=$2
    local secret=$3
    local diagnostic=$4
    local output
    shift 4

    if output=$(cd "$repo" && "$SCANNER" "$@" 2>&1); then
        echo "$label was accepted"
        exit 1
    fi
    if [[ "$output" == *"$secret"* ]]; then
        echo "$label leaked in scanner output"
        exit 1
    fi
    if [[ -n "$diagnostic" && "$output" != *"$diagnostic"* ]]; then
        echo "$label diagnostic omitted $diagnostic"
        exit 1
    fi
}

git -C "$REPO" init -q
git -C "$REPO" config user.email test@example.com
git -C "$REPO" config user.name "Vortex Test"

printf 'loglevel=error\n' > "$REPO/.npmrc"
git -C "$REPO" add .npmrc
(cd "$REPO" && "$SCANNER" --all) \
    || { echo "safe .npmrc was rejected"; exit 1; }

NPM_TOKEN='npm-secret-value'
printf '//registry.npmjs.org/:_authToken=%s\n' "$NPM_TOKEN" > "$REPO/.npmrc"
git -C "$REPO" add .npmrc
assert_rejected "$REPO" "npm auth token" "$NPM_TOKEN" ".npmrc" --all

printf 'loglevel=error\n' > "$REPO/.npmrc"
API_TOKEN="ghp_$(printf 'a%.0s' {1..36})"
printf '%s\n' "$API_TOKEN" > "$REPO/credential.txt"
git -C "$REPO" add .npmrc credential.txt
assert_rejected "$REPO" "API token" "$API_TOKEN" "credential.txt:1" --all
assert_rejected "$REPO" "staged API token" "$API_TOKEN" ""

LARGE_REPO=$(mktemp -d)
git -C "$LARGE_REPO" init -q
printf '//registry.npmjs.org/:_authToken=%s\n' "$NPM_TOKEN" > "$LARGE_REPO/.npmrc"
head -c 1048576 /dev/zero | tr '\0' x >> "$LARGE_REPO/.npmrc"
git -C "$LARGE_REPO" add .npmrc
assert_rejected "$LARGE_REPO" "large npm auth token" "$NPM_TOKEN" ".npmrc" --all
assert_rejected "$LARGE_REPO" "large staged npm auth token" "$NPM_TOKEN" ".npmrc"

WEIRD_REPO=$(mktemp -d)
WEIRD_DIR=$'odd\nname'
git -C "$WEIRD_REPO" init -q
mkdir -p "$WEIRD_REPO/$WEIRD_DIR"
printf '//registry.npmjs.org/:_authToken=%s\n' "$NPM_TOKEN" > "$WEIRD_REPO/$WEIRD_DIR/.npmrc"
git -C "$WEIRD_REPO" add -- "$WEIRD_DIR/.npmrc"
assert_rejected "$WEIRD_REPO" "newline-path npm auth token" "$NPM_TOKEN" ".npmrc" --all
assert_rejected "$WEIRD_REPO" "newline-path staged npm auth token" "$NPM_TOKEN" ".npmrc"

COLON_REPO=$(mktemp -d)
git -C "$COLON_REPO" init -q
mkdir -p "$COLON_REPO/dir:part"
printf '%s\n' "$API_TOKEN" > "$COLON_REPO/dir:part/credential.txt"
git -C "$COLON_REPO" add -- "dir:part/credential.txt"
assert_rejected "$COLON_REPO" "colon-path API token" "$API_TOKEN" "dir:part/credential.txt:1" --all

echo "no-secrets checks passed"
