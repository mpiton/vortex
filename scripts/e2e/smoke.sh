#!/usr/bin/env bash
# Desktop E2E smoke (MAT-135): drives the real debug binary (tauri-plugin-pilot
# is compiled in on debug builds) through the critical path — add link, resolve,
# download from a local fixture server, verify content, restart, verify persistence.
set -euo pipefail

cd "$(dirname "$0")/../.."

VORTEX_BIN="${VORTEX_BIN:-src-tauri/target/debug/vortex}"
PORT="${FIXTURE_PORT:-8642}"
FIXTURE_URL="http://127.0.0.1:$PORT/fixture.bin"
ART="e2e-artifacts"
# Printed by `fixture-server.py --self-test` (4 MiB of bytes(range(256)) repeated).
EXPECTED_SHA="2b07811057df887086f06a67edc6ebf911de8b6741156e7a2eb1416a4b8b1b2e"
EXPECTED_SIZE=4194304

if [ ! -x "$VORTEX_BIN" ]; then
  echo "missing $VORTEX_BIN — run: npx tauri build --debug --no-bundle --config scripts/e2e/tauri.e2e.conf.json" >&2
  exit 1
fi

rm -rf "$ART"
mkdir -p "$ART"

# Empty, isolated profile (R-02). XDG_RUNTIME_DIR also isolates the pilot
# socket, so a locally running dev instance cannot collide with the test.
# The profile must live directly under /tmp: the socket path is capped at
# ~108 bytes (SUN_LEN) and a deep directory overflows it.
PROFILE=$(mktemp -d /tmp/vxe2e.XXXXXX)
mkdir -p "$PROFILE/Downloads" "$PROFILE/.config" "$PROFILE/.local/share" \
  "$PROFILE/.cache" "$PROFILE/runtime"
chmod 700 "$PROFILE/runtime"
printf 'XDG_DOWNLOAD_DIR="$HOME/Downloads"\n' > "$PROFILE/.config/user-dirs.dirs"

export HOME="$PROFILE"
export XDG_CONFIG_HOME="$PROFILE/.config"
export XDG_DATA_HOME="$PROFILE/.local/share"
export XDG_CACHE_HOME="$PROFILE/.cache"
export XDG_RUNTIME_DIR="$PROFILE/runtime"
export GDK_BACKEND=x11
export TAURI_PILOT_SOCKET="$XDG_RUNTIME_DIR/tauri-pilot-dev.vortex.app.sock"

app_pid=""
server_pid=""

collect_artifacts() {
  tauri-pilot screenshot "$ART/failure.png" 2>/dev/null || true
  tauri-pilot html > "$ART/page.html" 2>&1 || true
  tauri-pilot logs > "$ART/console.log" 2>&1 || true
  cp -r "$XDG_DATA_HOME/dev.vortex.app" "$ART/profile-data" 2>/dev/null || true
}

cleanup() {
  status=$?
  if [ "$status" -ne 0 ]; then
    collect_artifacts
    echo "SMOKE FAILED — diagnostics in $ART/" >&2
  fi
  if [ -n "$app_pid" ]; then kill "$app_pid" 2>/dev/null || true; fi
  if [ -n "$server_pid" ]; then kill "$server_pid" 2>/dev/null || true; fi
  wait 2>/dev/null || true
  # WebKit helper processes may still write to the profile briefly after the
  # app is killed; retry once instead of failing the run on the race.
  rm -rf "$PROFILE" 2>/dev/null || { sleep 2; rm -rf "$PROFILE" || true; }
  exit "$status"
}
trap cleanup EXIT

launch_app() {
  "$VORTEX_BIN" > "$ART/$1" 2>&1 &
  app_pid=$!
  for _ in $(seq 1 60); do
    if tauri-pilot ping >/dev/null 2>&1; then return 0; fi
    if ! kill -0 "$app_pid" 2>/dev/null; then
      echo "app exited during startup — see $ART/$1" >&2
      return 1
    fi
    sleep 1
  done
  echo "pilot socket never became ready — see $ART/$1" >&2
  return 1
}

# Poll for a CSS selector via eval. The pilot `wait` command relies on an
# in-page promise that silently times out when the element appears after the
# call, so polling is the reliable primitive here.
wait_for() { # <selector> <timeout-seconds>
  local sel=$1 deadline=$(($(date +%s) + $2))
  while [ "$(date +%s)" -lt "$deadline" ]; do
    if [ "$(tauri-pilot eval "document.querySelector('$sel') ? 'yes' : 'no'" 2>/dev/null)" = "yes" ]; then
      return 0
    fi
    sleep 1
  done
  echo "timeout waiting for: $sel" >&2
  return 1
}

echo "--- fixture server"
python3 scripts/e2e/fixture-server.py --self-test > "$ART/fixture-selftest.log" 2>&1
python3 scripts/e2e/fixture-server.py "$PORT" > "$ART/fixture-server.log" 2>&1 &
server_pid=$!
for _ in $(seq 1 20); do
  curl -sfI "$FIXTURE_URL" >/dev/null 2>&1 && break
  sleep 0.5
done
curl -sfI "$FIXTURE_URL" >/dev/null

echo "--- launch app (empty profile)"
launch_app app.log
wait_for 'a[href="/link-grabber"]' 30

echo "--- add link + resolve in Link Grabber"
tauri-pilot click 'a[href="/link-grabber"]'
wait_for '[data-testid=paste-input]' 15
# `fill` uses the HTMLInputElement value setter, which throws on a <textarea>;
# the paste zone is uncontrolled (ref-read), so a plain value assignment works.
tauri-pilot eval "document.querySelector('[data-testid=paste-input]').value = '$FIXTURE_URL'"
tauri-pilot click '[data-testid=analyze-links]'
wait_for '[data-testid^="link-row-"][data-status="online"]' 30

echo "--- start download, wait for completion"
tauri-pilot click '[data-testid=start-all-online]'
# Starting stays on the Link Grabber; the Downloads view is where completion shows.
tauri-pilot click 'a[href="/downloads"]'
wait_for '[data-testid=download-row][data-state=Completed]' 120

echo "--- verify downloaded file (R-03)"
FILE="$HOME/Downloads/fixture.bin"
if [ ! -f "$FILE" ]; then
  echo "downloaded file missing: $FILE" >&2
  exit 1
fi
size=$(stat -c%s "$FILE")
if [ "$size" != "$EXPECTED_SIZE" ]; then
  echo "size mismatch: got $size, expected $EXPECTED_SIZE" >&2
  exit 1
fi
echo "$EXPECTED_SHA  $FILE" | sha256sum -c -

echo "--- restart app, verify persistence (R-04)"
kill -TERM "$app_pid"
wait "$app_pid" 2>/dev/null || true
app_pid=""
launch_app app-restart.log
wait_for '[data-testid=download-row][data-state=Completed]' 30
tauri-pilot eval "document.querySelector('[data-testid=download-row][data-state=Completed]').textContent" \
  | grep -q "fixture.bin"

echo "SMOKE OK"
