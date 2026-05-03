#!/usr/bin/env bash
# Reject manual edits to Cargo.toml / package.json without an updated lock file.
# Forces use of `cargo add` / `npm install`.
# Invoked at pre-commit via lefthook.yml.
set -euo pipefail

STAGED=$(git diff --cached --name-only)

# === Cargo.toml ===
for cargo_toml in src-tauri/Cargo.toml Cargo.toml; do
    if echo "$STAGED" | grep -qF "$cargo_toml"; then
        if [ "$cargo_toml" = "Cargo.toml" ]; then
            lock="Cargo.lock"
        else
            lock="${cargo_toml%/Cargo.toml}/Cargo.lock"
        fi

        # Detect changes in [dependencies] / [dev-dependencies] / [build-dependencies]
        DEP_DIFF=$(git diff --cached -U0 "$cargo_toml" \
            | grep -E '^\+[^+]' \
            | grep -E '^\+[a-zA-Z0-9_-]+\s*=' || true)

        if [ -n "$DEP_DIFF" ]; then
            if ! echo "$STAGED" | grep -qF "$lock"; then
                echo "BLOCKED: $cargo_toml modified without updated $lock."
                echo ""
                echo "You added/modified dependencies:"
                echo "$DEP_DIFF" | sed 's/^/  /'
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
if echo "$STAGED" | grep -qF "package.json"; then
    PKG_DEP_DIFF=$(git diff --cached -U0 package.json \
        | grep -E '^\+\s*"[^"]+"\s*:\s*"\^?[~=<>0-9]' || true)

    if [ -n "$PKG_DEP_DIFF" ]; then
        if ! echo "$STAGED" | grep -qF "package-lock.json"; then
            echo "BLOCKED: package.json modified without updated package-lock.json."
            echo ""
            echo "You added/modified dependencies:"
            echo "$PKG_DEP_DIFF" | sed 's/^/  /'
            echo ""
            echo "Correct procedure:"
            echo "  npm install <package>      # runtime dependency"
            echo "  npm install -D <package>   # devDependency"
            echo ""
            echo "This updates package-lock.json automatically."
            exit 1
        fi
    fi

    # Reject pnpm/yarn lock files (vortex uses npm only)
    if echo "$STAGED" | grep -qE '(pnpm-lock\.yaml|yarn\.lock)'; then
        echo "BLOCKED: pnpm-lock.yaml or yarn.lock detected."
        echo "Vortex uses npm exclusively. Remove this lock file and use package-lock.json."
        exit 1
    fi
fi

exit 0
