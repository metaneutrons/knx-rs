#!/usr/bin/env bash

set -euo pipefail

fail() {
  printf 'source-archive: %s\n' "$*" >&2
  exit 1
}

repo_root=$(git rev-parse --show-toplevel)
cd "$repo_root"

tag=${1:?usage: build-source-archive.sh TAG OUTPUT_DIRECTORY}
output_dir=${2:?usage: build-source-archive.sh TAG OUTPUT_DIRECTORY}

scripts/release/validate-release-tag.sh "$tag"

archive="knx-rs-${tag}.tar.gz"
archive_root="knx-rs-${tag}/"
mkdir -p "$output_dir"

temporary_dir=$(mktemp -d)
cleanup() {
  rm -rf "$temporary_dir"
}
trap cleanup EXIT

export SOURCE_DATE_EPOCH
SOURCE_DATE_EPOCH=$(git show -s --format=%ct "${tag}^{commit}")

git archive --format=tar --prefix="$archive_root" "${tag}^{commit}" \
  > "$temporary_dir/source.tar"
gzip -9 -n < "$temporary_dir/source.tar" > "$output_dir/$archive"

tar -tzf "$output_dir/$archive" > "$temporary_dir/archive.list"
[[ -s "$temporary_dir/archive.list" ]] || fail "archive is empty"
while IFS= read -r entry; do
  [[ "$entry" == "$archive_root"* ]] \
    || fail "archive entry is outside the expected root: $entry"
  [[ "$entry" != *'/../'* && "$entry" != '../'* ]] \
    || fail "archive entry contains parent traversal: $entry"
done < "$temporary_dir/archive.list"

(
  cd "$output_dir"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$archive" > SHA256SUMS
  else
    shasum -a 256 "$archive" > SHA256SUMS
  fi
)

printf 'Built %s with SOURCE_DATE_EPOCH=%s.\n' "$archive" "$SOURCE_DATE_EPOCH"
