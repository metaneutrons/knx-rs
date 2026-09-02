#!/usr/bin/env bash

set -euo pipefail

fail() {
  printf 'github-release-stage: %s\n' "$*" >&2
  exit 1
}

tag=${1:?usage: stage-github-release.sh TAG CANDIDATE_DIRECTORY}
candidate_dir=${2:?usage: stage-github-release.sh TAG CANDIDATE_DIRECTORY}
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"
: "${GH_TOKEN:?GH_TOKEN is required}"

archive="knx-rs-${tag}.tar.gz"
scripts/release/verify-assets.sh "$candidate_dir" "$archive"

is_prerelease=false
[[ "$tag" == *-* ]] && is_prerelease=true

if release_json=$(gh release view "$tag" --repo "$GITHUB_REPOSITORY" \
  --json isDraft,isPrerelease,assets 2>/dev/null); then
  jq -e '.isDraft == true' >/dev/null <<< "$release_json" \
    || fail "release already exists in a published state"
else
  [[ "$is_prerelease" == "true" ]] \
    || fail "production tag has no release-please draft"
  gh release create "$tag" --repo "$GITHUB_REPOSITORY" --verify-tag --draft \
    --title "knx-rs $tag (pipeline validation)" \
    --notes "Prerelease used to validate the release pipeline. It is never published to package registries."
  release_json=$(gh release view "$tag" --repo "$GITHUB_REPOSITORY" \
    --json isDraft,isPrerelease,assets)
fi

existing_assets=$(jq -r '.assets[].name' <<< "$release_json" | LC_ALL=C sort)
expected_assets=$(printf '%s\n%s\n' SHA256SUMS "$archive" | LC_ALL=C sort)
if [[ -n "$existing_assets" && "$existing_assets" != "$expected_assets" ]]; then
  fail "draft contains assets outside the release contract"
fi

gh release upload "$tag" --repo "$GITHUB_REPOSITORY" --clobber \
  "$candidate_dir/$archive" "$candidate_dir/SHA256SUMS"

download_dir=$(mktemp -d)
cleanup() {
  rm -rf "$download_dir"
}
trap cleanup EXIT

gh release download "$tag" --repo "$GITHUB_REPOSITORY" --dir "$download_dir"
scripts/release/verify-assets.sh "$download_dir" "$archive"
cmp "$candidate_dir/$archive" "$download_dir/$archive"
cmp "$candidate_dir/SHA256SUMS" "$download_dir/SHA256SUMS"

gh release edit "$tag" --repo "$GITHUB_REPOSITORY" \
  --draft=false --prerelease --latest=false

release_json=$(gh release view "$tag" --repo "$GITHUB_REPOSITORY" \
  --json tagName,isDraft,isPrerelease,assets)
jq -e --arg tag "$tag" --arg archive "$archive" '
  .tagName == $tag
  and .isDraft == false
  and .isPrerelease == true
  and ([.assets[].name] | sort) == (["SHA256SUMS", $archive] | sort)
' >/dev/null <<< "$release_json" || fail "published prerelease state is invalid"

latest_tag=$(gh api "repos/$GITHUB_REPOSITORY/releases/latest" --jq .tag_name)
[[ "$latest_tag" != "$tag" ]] || fail "staged prerelease was marked as latest"

printf 'Staged and verified GitHub prerelease %s.\n' "$tag"
