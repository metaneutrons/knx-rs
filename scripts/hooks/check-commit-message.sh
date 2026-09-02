#!/bin/sh
# Validate Conventional Commits and reject generated attribution trailers.
set -eu

msg_file=${1:?commit message path is required}
msg=$(cat "$msg_file")

# Remove comment lines and the verbose diff appended by `git commit -v`.
body=$(printf '%s\n' "$msg" | sed -e '/^#/d' -e '/^diff --git /,$d')
subject=$(printf '%s\n' "$body" | sed -e '/^[[:space:]]*$/d' -e 1q)

fail() {
    printf '\033[31mCommit rejected:\033[0m %s\n' "$1" >&2
    shift
    for line in "$@"; do printf '  %s\n' "$line" >&2; done
    exit 1
}

case "$subject" in
    "Merge "*|"Revert \""*|"fixup!"*|"squash!"*|"amend!"*) exit 0 ;;
esac

types='feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert'
if ! printf '%s' "$subject" |
    grep -Eq "^($types)(\([a-z0-9._/-]+\))?!?: .+"; then
    fail "The subject does not follow Conventional Commits." \
        "Actual:   $subject" \
        "Expected: <type>[(scope)][!]: <description>" \
        "Types: feat fix docs style refactor perf test build ci chore revert"
fi

if [ "${#subject}" -gt 100 ]; then
    fail "The subject is ${#subject} characters; the limit is 100."
fi

if printf '%s\n' "$body" |
    grep -Eiq '^[[:space:]]*co-authored-by:.*(claude|anthropic|noreply@anthropic\.com)'; then
    fail "The message contains a generated attribution trailer." \
        "Remove the Co-Authored-By line."
fi

if printf '%s\n' "$body" | grep -Eiq 'generated with .*claude code|🤖 generated with'; then
    fail "The message contains a generated attribution line."
fi

exit 0
