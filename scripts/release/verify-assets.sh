#!/usr/bin/env bash

set -euo pipefail

fail() {
  printf 'asset-verification: %s\n' "$*" >&2
  exit 1
}

asset_dir=${1:?usage: verify-assets.sh ASSET_DIRECTORY ARCHIVE_NAME}
archive=${2:?usage: verify-assets.sh ASSET_DIRECTORY ARCHIVE_NAME}

[[ -d "$asset_dir" ]] || fail "asset directory does not exist"
[[ -f "$asset_dir/$archive" ]] || fail "archive is missing: $archive"
[[ -f "$asset_dir/SHA256SUMS" ]] || fail "SHA256SUMS is missing"

actual_inventory=$(find "$asset_dir" -mindepth 1 -maxdepth 1 -type f \
  -exec basename {} \; | LC_ALL=C sort)
expected_inventory=$(printf '%s\n%s\n' SHA256SUMS "$archive" | LC_ALL=C sort)
[[ "$actual_inventory" == "$expected_inventory" ]] \
  || fail "asset inventory does not match the release contract"

line_count=$(awk 'END { print NR }' "$asset_dir/SHA256SUMS")
[[ "$line_count" == "1" ]] || fail "SHA256SUMS must contain exactly one entry"

checksum_line=$(<"$asset_dir/SHA256SUMS")
digest=${checksum_line%% *}
[[ "$digest" =~ ^[0-9a-f]{64}$ ]] || fail "checksum is not lowercase SHA-256"
[[ "$checksum_line" == "$digest  $archive" ]] \
  || fail "SHA256SUMS names an unexpected payload"

if command -v sha256sum >/dev/null 2>&1; then
  actual_digest=$(sha256sum "$asset_dir/$archive")
else
  actual_digest=$(shasum -a 256 "$asset_dir/$archive")
fi
actual_digest=${actual_digest%% *}
[[ "$actual_digest" == "$digest" ]] || fail "checksum mismatch for $archive"

archive_root=${archive%.tar.gz}/
listing=$(tar -tzf "$asset_dir/$archive") || fail "archive cannot be read"
[[ -n "$listing" ]] || fail "archive is empty"
while IFS= read -r entry; do
  [[ "$entry" == "$archive_root"* ]] \
    || fail "archive entry is outside the expected root: $entry"
  [[ "$entry" != *'/../'* && "$entry" != '../'* ]] \
    || fail "archive entry contains parent traversal: $entry"
done <<< "$listing"

printf 'Verified archive and exact asset inventory for %s.\n' "$archive"
