#!/bin/sh
# Fast pre-commit guards that do not compile the workspace.
set -eu

max_bytes=${MAX_STAGED_BYTES:-5242880}
status=0

fail() { printf '\033[31mCommit rejected:\033[0m %s\n' "$1" >&2; status=1; }

branch=$(git symbolic-ref --quiet --short HEAD 2>/dev/null || echo '')
default=$(git symbolic-ref --quiet --short refs/remotes/origin/HEAD 2>/dev/null |
    sed 's#^origin/##')
default=${default:-main}
if [ "$branch" = "$default" ] && [ "${ALLOW_COMMIT_ON_DEFAULT:-}" != '1' ]; then
    fail "Direct commit on '$default'. Create a branch or set ALLOW_COMMIT_ON_DEFAULT=1."
fi

git -c core.quotePath=false diff --cached --name-only --diff-filter=AM | while IFS= read -r file; do
    [ -f "$file" ] || continue
    size=$(wc -c < "$file" | tr -d ' ')
    if [ "$size" -gt "$max_bytes" ]; then
        printf '\033[31mCommit rejected:\033[0m %s is %s bytes (limit %s).\n' \
            "$file" "$size" "$max_bytes" >&2
        exit 1
    fi
done || status=1

if git -c core.quotePath=false diff --cached --name-only |
    grep -Eq '(^|/)(node_modules|target|dist|build|\.next|coverage)/'; then
    fail "A build or dependency directory is staged. Check .gitignore."
fi

exit "$status"
