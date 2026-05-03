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
# version of the file pointed to by `<ref>:<path>` (e.g. `:src-tauri/Cargo.toml`
# for the staged blob, `HEAD:src-tauri/Cargo.toml` for the index parent).
# Handles `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]`,
# `[workspace.dependencies]`, and `[target.*.dependencies]` (plus dev-/build-
# variants).
cargo_dep_section_ranges() {
    local ref_path="$1"
    # `git show` exits non-zero for new files (no HEAD blob) or unstaged paths.
    # Funnel through `|| true` so the awk pipe still runs on empty input under
    # `set -euo pipefail` and the function returns 0 with no output.
    { git show "$ref_path" 2>/dev/null || true; } | awk '
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

# Emit the new-file line numbers of every added (`+`) line in the staged diff.
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

# Emit the old-file line numbers of every removed (`-`) line in the staged diff.
# Numbers refer to HEAD, so they should be tested against
# `cargo_dep_section_ranges "HEAD:<file>"`.
removed_line_numbers() {
    git diff --cached -U0 "$1" | awk '
        /^---/ { next }
        /^@@/ {
            if (match($0, /-[0-9]+/)) {
                cur = substr($0, RSTART + 1, RLENGTH - 1) + 0
            }
            next
        }
        /^-/ { print cur; cur++ }
    '
}

# Returns 0 if any added line falls in a dep-section range of the staged blob,
# OR any removed line falls in a dep-section range of the HEAD blob.
# Catches both additions and deletions so a manual `cargo remove` followed by
# a stage of Cargo.toml-only is also blocked when Cargo.lock isn't updated.
cargo_dep_change_detected() {
    local file="$1"
    local new_ranges old_ranges added removed line s e

    new_ranges=$(cargo_dep_section_ranges ":${file}")
    if [ -n "$new_ranges" ]; then
        added=$(added_line_numbers "$file")
        for line in $added; do
            for r in $new_ranges; do
                s="${r%-*}"
                e="${r#*-}"
                if [ "$line" -ge "$s" ] && [ "$line" -le "$e" ]; then
                    return 0
                fi
            done
        done
    fi

    old_ranges=$(cargo_dep_section_ranges "HEAD:${file}")
    if [ -n "$old_ranges" ]; then
        removed=$(removed_line_numbers "$file")
        for line in $removed; do
            for r in $old_ranges; do
                s="${r%-*}"
                e="${r#*-}"
                if [ "$line" -ge "$s" ] && [ "$line" -le "$e" ]; then
                    return 0
                fi
            done
        done
    fi

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
                echo "  cargo remove <crate>         # for deletions"
                echo ""
                echo "This updates $lock automatically."
                echo "Revert your manual change then use the cargo command above."
                exit 1
            fi
        fi
    fi
done

# === package.json ===
# Fail closed if jq is missing — the policy is non-negotiable, so a tooling gap
# must block the commit instead of silently letting manual dep edits through.
require_jq() {
    if ! command -v jq >/dev/null 2>&1; then
        echo "BLOCKED: jq is required for the no-manual-deps hook." >&2
        echo "Install jq before committing:" >&2
        echo "  Linux:   sudo apt install jq    # or your distro equivalent" >&2
        echo "  macOS:   brew install jq" >&2
        echo "  Windows: scoop install jq       # or choco install jq" >&2
        exit 1
    fi
}

# Detect dependency-table changes via jq diff against HEAD. Compares the
# four standard sections so a top-level "version" / "name" bump never triggers.
pkg_dep_change_detected() {
    require_jq
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
            echo "  npm uninstall <package>    # for deletions"
            echo ""
            echo "This updates package-lock.json automatically."
            exit 1
        fi
    fi
fi

exit 0
