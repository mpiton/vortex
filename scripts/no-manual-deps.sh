#!/usr/bin/env bash
# Reject manual edits to Cargo.toml / package.json *dependency tables* without
# an updated lock file. Forces use of `cargo add` / `npm install`.
# Invoked at pre-commit via lefthook.yml.
set -euo pipefail

STAGED=$(git diff --cached --name-only)

# === Reject pnpm-lock.yaml / yarn.lock unconditionally (vortex = npm only) ===
# Runs even on lockfile-only commits, so a stray pnpm-lock.yaml stage attempt
# is blocked even when no package.json change accompanies it.
if echo "$STAGED" | grep -qE '(^|/)(pnpm-lock\.yaml|yarn\.lock)$'; then
    echo "BLOCKED: pnpm-lock.yaml or yarn.lock detected."
    echo "Vortex uses npm exclusively. Remove this lock file and use package-lock.json."
    exit 1
fi

# Compute the inclusive line ranges occupied by Cargo dependency tables in the
# staged version of the file. Handles `[dependencies]`, `[dev-dependencies]`,
# `[build-dependencies]`, `[workspace.dependencies]`, and `[target.*.dependencies]`
# (plus their dev-/build- variants).
cargo_dep_section_ranges() {
    git show ":${1}" 2>/dev/null | awk '
        /^\[/ {
            if (in_dep && start > 0) print start "-" (NR - 1)
            in_dep = ($0 ~ /^\[(dev-|build-)?dependencies\][[:space:]]*$/ \
                  || $0 ~ /^\[workspace\.(dev-|build-)?dependencies\][[:space:]]*$/ \
                  || $0 ~ /^\[target\.[^]]+\.(dev-|build-)?dependencies\][[:space:]]*$/)
            start = (in_dep ? NR + 1 : 0)
            next
        }
        END { if (in_dep && start > 0) print start "-" NR }
    '
}

# Emit the new-file line numbers of every `+` line in the staged diff.
# Skips diff metadata (+++ headers).
added_line_numbers() {
    git diff --cached -U0 "$1" | awk '
        /^\+\+\+/ { next }
        /^@@/ {
            if (match($0, /\+[0-9]+/)) {
                cur = substr($0, RSTART + 1, RLENGTH - 1) + 0
            }
            next
        }
        /^\+/ { print cur; cur++ }
    '
}

# Returns 0 if any added line falls inside a dep-section range.
cargo_dep_change_detected() {
    local file="$1"
    local ranges added line s e
    ranges=$(cargo_dep_section_ranges "$file") || return 1
    [ -z "$ranges" ] && return 1
    added=$(added_line_numbers "$file")
    [ -z "$added" ] && return 1
    for line in $added; do
        for r in $ranges; do
            s="${r%-*}"
            e="${r#*-}"
            if [ "$line" -ge "$s" ] && [ "$line" -le "$e" ]; then
                return 0
            fi
        done
    done
    return 1
}

# === Cargo.toml ===
for cargo_toml in src-tauri/Cargo.toml Cargo.toml; do
    if echo "$STAGED" | grep -qF "$cargo_toml"; then
        if [ "$cargo_toml" = "Cargo.toml" ]; then
            lock="Cargo.lock"
        else
            lock="${cargo_toml%/Cargo.toml}/Cargo.lock"
        fi

        if cargo_dep_change_detected "$cargo_toml"; then
            if ! echo "$STAGED" | grep -qF "$lock"; then
                echo "BLOCKED: $cargo_toml dependency table modified without updated $lock."
                echo ""
                echo "Correct procedure:"
                echo "  cargo add <crate>            # or cargo add --dev / --build"
                echo "  cargo add <crate>@<version>  # explicit constraint if needed"
                echo ""
                echo "This updates $lock automatically."
                echo "Revert your manual change then use cargo add."
                exit 1
            fi
        fi
    fi
done

# === package.json ===
# Detect dependency-table changes via jq diff against HEAD. Avoids the
# false positives that hit a top-level "version" / "name" bump.
pkg_dep_change_detected() {
    command -v jq >/dev/null 2>&1 || return 1
    local sect old new
    for sect in dependencies devDependencies peerDependencies optionalDependencies; do
        old=$(git show "HEAD:package.json" 2>/dev/null \
            | jq -S --arg s "$sect" '.[$s] // {}' 2>/dev/null || echo '{}')
        new=$(git show ":package.json" 2>/dev/null \
            | jq -S --arg s "$sect" '.[$s] // {}' 2>/dev/null || echo '{}')
        if [ "$old" != "$new" ]; then
            return 0
        fi
    done
    return 1
}

if echo "$STAGED" | grep -qF "package.json"; then
    if pkg_dep_change_detected; then
        if ! echo "$STAGED" | grep -qF "package-lock.json"; then
            echo "BLOCKED: package.json dependency table modified without updated package-lock.json."
            echo ""
            echo "Correct procedure:"
            echo "  npm install <package>      # runtime dependency"
            echo "  npm install -D <package>   # devDependency"
            echo ""
            echo "This updates package-lock.json automatically."
            exit 1
        fi
    fi
fi

exit 0
