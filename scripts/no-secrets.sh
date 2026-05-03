#!/usr/bin/env bash
# Reject staged files likely to contain secrets.
# Invoked at pre-commit via lefthook.yml.
set -euo pipefail

# File patterns that typically contain secrets
SECRET_PATTERNS='\.(env|env\..+|pem|key|p12|pfx|secret|creds|aws|netrc)$|\.(env|secrets)/|^\.npmrc$|^\.pypirc$'

# List staged files (excluding deletions)
STAGED=$(git diff --cached --diff-filter=d --name-only)

if [ -z "$STAGED" ]; then
    exit 0
fi

# Check filenames
SECRET_FILES=$(echo "$STAGED" | grep -iE "$SECRET_PATTERNS" || true)

if [ -n "$SECRET_FILES" ]; then
    echo "BLOCKED: files possibly containing secrets staged:"
    echo "$SECRET_FILES" | sed 's/^/  - /'
    echo ""
    echo "If intentional, add the file to .gitignore and use a .example variant instead."
    exit 1
fi

# Check content: grep known API key patterns in the diff
CONTENT_LEAK=$(git diff --cached -U0 | grep -E '^\+' | grep -iE \
    -e 'AKIA[0-9A-Z]{16}' \
    -e 'sk-ant-[A-Za-z0-9_-]{20,}' \
    -e 'sk-[A-Za-z0-9]{32,}' \
    -e 'ghp_[A-Za-z0-9]{36}' \
    -e 'github_pat_[A-Za-z0-9_]{82}' \
    -e 'glpat-[A-Za-z0-9_-]{20}' \
    -e 'AIza[0-9A-Za-z_-]{35}' \
    || true)

if [ -n "$CONTENT_LEAK" ]; then
    echo "BLOCKED: API key pattern detected in diff:"
    echo "$CONTENT_LEAK" | head -5
    echo ""
    echo "Remove the key and revoke it immediately if it has been committed even locally."
    exit 1
fi

exit 0
