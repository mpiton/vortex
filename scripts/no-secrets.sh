#!/usr/bin/env bash
# Reject staged or tracked files likely to contain secrets.
# Invoked at pre-commit via lefthook.yml and with --all in CI.
set -euo pipefail

# File patterns that typically contain secrets
SECRET_PATTERNS='\.(env|env\..+|pem|key|p12|pfx|secret|creds|aws|netrc)$|(^|/)(\.env|\.secrets|secrets)/|(^|/)\.pypirc$'
NPMRC_PATH_PATTERN='(^|/)\.npmrc$'
NPMRC_AUTH_PATTERNS='(^|[[:space:]])(_authToken|_password|_auth)[[:space:]]*=|//.+:(_authToken|_password|_auth)[[:space:]]*='
API_KEY_PATTERNS='AKIA[0-9A-Z]{16}|ASIA[0-9A-Z]{16}|sk-ant-[A-Za-z0-9_-]{20,}|sk-[A-Za-z0-9]{32,}|ghp_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{82}|glpat-[A-Za-z0-9_-]{20}|AIza[0-9A-Za-z_-]{35}'

FILES=()
case "${1:-}" in
    "")
        SCOPE=staged
        while IFS= read -r -d '' file; do
            FILES+=("$file")
        done < <(git diff --cached --diff-filter=d --name-only -z)
        ;;
    --all)
        SCOPE=tracked
        while IFS= read -r -d '' file; do
            FILES+=("$file")
        done < <(git ls-files -z)
        ;;
    *) echo "Usage: $0 [--all]" >&2; exit 2 ;;
esac

if [ "${#FILES[@]}" -eq 0 ]; then
    exit 0
fi

# Classify paths without line-delimited parsing: Git permits newlines in names.
SECRET_FILES=()
NPMRC_FILES=()
shopt -s nocasematch
for file in "${FILES[@]}"; do
    if [[ $file =~ $SECRET_PATTERNS ]]; then
        SECRET_FILES+=("$file")
    fi
done
shopt -u nocasematch
for file in "${FILES[@]}"; do
    if [[ $file =~ $NPMRC_PATH_PATTERN ]]; then
        NPMRC_FILES+=("$file")
    fi
done

if [ "${#SECRET_FILES[@]}" -gt 0 ]; then
    echo "BLOCKED: files possibly containing secrets are $SCOPE:"
    printf '  - %q\n' "${SECRET_FILES[@]}"
    echo ""
    echo "If intentional, add the file to .gitignore and use a .example variant instead."
    exit 1
fi

# .npmrc is allowed for non-secret config (registries, loglevel, hoisting).
# Scan the complete indexed file so findings are identical locally and in CI.
NPMRC_LEAKS=()
for file in "${NPMRC_FILES[@]}"; do
    # Do not use grep -q here: under pipefail its early exit can SIGPIPE
    # git-show and turn a real match in a large file into a false negative.
    if git show ":$file" | grep -iE "$NPMRC_AUTH_PATTERNS" >/dev/null; then
        NPMRC_LEAKS+=("$file")
    fi
done

if [ "${#NPMRC_LEAKS[@]}" -gt 0 ]; then
    echo "BLOCKED: npm authentication config detected in tracked .npmrc:"
    printf '  - %q\n' "${NPMRC_LEAKS[@]}"
    echo "Store npm credentials in the user-level ~/.npmrc or CI secrets instead."
    exit 1
fi

# Check content: grep known API key patterns in the diff
CONTENT_FOUND=false
if [ "$SCOPE" = "tracked" ]; then
    if git grep --cached -qEI "$API_KEY_PATTERNS" -- ':(exclude)*.lock'; then
        CONTENT_FOUND=true
    else
        GREP_STATUS=$?
        if [ "$GREP_STATUS" -ne 1 ]; then
            echo "Secret scan failed while reading the Git index." >&2
            exit "$GREP_STATUS"
        fi
    fi
else
    CONTENT_LEAK=$(git diff --cached -U0 | grep -E '^\+' | grep -vE '^\+\+\+ [ab]/' | grep -iE "$API_KEY_PATTERNS" || true)
    if [ -n "$CONTENT_LEAK" ]; then
        CONTENT_FOUND=true
    fi
fi

if [ "$CONTENT_FOUND" = true ]; then
    echo "BLOCKED: API key pattern detected; matched values were redacted."
    if [ "$SCOPE" = "tracked" ]; then
        MATCH_COUNT=0
        while IFS= read -r -d '' file \
            && IFS= read -r -d '' line \
            && IFS= read -r content; do
            if [ "$MATCH_COUNT" -lt 5 ]; then
                printf '  - %q:%s\n' "$file" "$line"
            fi
            MATCH_COUNT=$((MATCH_COUNT + 1))
        done < <(git grep --cached -z -nEI "$API_KEY_PATTERNS" -- ':(exclude)*.lock')
    fi
    echo "Remove the key and revoke it immediately if it has been committed even locally."
    exit 1
fi

exit 0
