#!/usr/bin/env bash

set -euo pipefail

fail() {
  printf 'release-tag: %s\n' "$*" >&2
  exit 1
}

repo_root=$(git rev-parse --show-toplevel)
cd "$repo_root"

tag=${1:-${GITHUB_REF_NAME:-}}
[[ -n "$tag" ]] || fail "tag is required"

semver='^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-([0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*))?$'
[[ "$tag" =~ $semver ]] || fail "tag is not canonical SemVer: $tag"

prerelease=${BASH_REMATCH[5]:-}
if [[ -n "$prerelease" ]]; then
  IFS='.' read -r -a identifiers <<< "$prerelease"
  for identifier in "${identifiers[@]}"; do
    if [[ "$identifier" =~ ^[0-9]+$ && ${#identifier} -gt 1 && "$identifier" == 0* ]]; then
      fail "numeric prerelease identifier has a leading zero: $identifier"
    fi
  done
fi

tag_commit=$(git rev-parse --verify "${tag}^{commit}" 2>/dev/null) \
  || fail "tag does not resolve to a commit: $tag"
head_commit=$(git rev-parse --verify HEAD)
[[ "$tag_commit" == "$head_commit" ]] \
  || fail "tag target does not match the checked-out commit"

version=${tag#v}
version=${version%%-*}
manifest_version=$(jq -er '."."' .release-please-manifest.json) \
  || fail "release-please manifest does not contain the workspace version"
[[ "$manifest_version" == "$version" ]] \
  || fail "tag core $version does not match release-please manifest $manifest_version"

metadata=$(cargo metadata --no-deps --format-version 1 --locked)
if ! jq -e --arg version "$version" '
  .workspace_members as $members
  | [.packages[]
      | select(.id as $id | $members | index($id))
      | select(.publish != [])] as $publishable
  | ($publishable | length) > 0
    and all($publishable[]; .version == $version)
' >/dev/null <<< "$metadata"; then
  fail "publishable workspace crates do not all match tag core $version"
fi

is_prerelease=false
[[ -n "$prerelease" ]] && is_prerelease=true
archive="knx-rs-${tag}.tar.gz"

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  {
    printf 'tag=%s\n' "$tag"
    printf 'version=%s\n' "$version"
    printf 'is_prerelease=%s\n' "$is_prerelease"
    printf 'archive=%s\n' "$archive"
  } >> "$GITHUB_OUTPUT"
fi

printf 'Validated %s at %s (version %s, prerelease=%s).\n' \
  "$tag" "$head_commit" "$version" "$is_prerelease"
