#!/bin/sh
# Exercise successful and rejected inputs for the repository's local hooks.
set -eu

root=$(cd "$(dirname "$0")/../.." && pwd)
temp=$(mktemp -d)
trap 'rm -rf "$temp"' EXIT HUP INT TERM

expect_failure() {
    if "$@" > /dev/null 2>&1; then
        printf 'Expected command to fail: %s\n' "$*" >&2
        exit 1
    fi
}

printf 'fix(ip): accept IPv6 multicast addresses\n' > "$temp/valid-message"
sh "$root/scripts/hooks/check-commit-message.sh" "$temp/valid-message"

printf 'changed networking\n' > "$temp/invalid-message"
expect_failure sh "$root/scripts/hooks/check-commit-message.sh" "$temp/invalid-message"

printf 'fix: remove fallback\n\nCo-authored-by: Claude <noreply@anthropic.com>\n' \
    > "$temp/generated-message"
expect_failure sh "$root/scripts/hooks/check-commit-message.sh" "$temp/generated-message"

git -C "$temp" init --quiet --initial-branch=fix/hook-contract
git -C "$temp" config user.name 'Hook Contract Test'
git -C "$temp" config user.email 'hook-test@example.invalid'
printf 'baseline\n' > "$temp/input.txt"
git -C "$temp" add input.txt
(
    cd "$temp"
    sh "$root/scripts/hooks/check-staged.sh"
)

git -C "$temp" branch --move main
expect_failure sh -c "cd '$temp' && sh '$root/scripts/hooks/check-staged.sh'"

git -C "$temp" branch --move fix/hook-contract
expect_failure env MAX_STAGED_BYTES=1 sh -c \
    "cd '$temp' && sh '$root/scripts/hooks/check-staged.sh'"

mkdir "$temp/target"
printf 'harmless build output\n' > "$temp/target/artifact.txt"
git -C "$temp" add --force target/artifact.txt
expect_failure sh -c "cd '$temp' && sh '$root/scripts/hooks/check-staged.sh'"

printf 'Hook contract tests passed.\n'
