#!/usr/bin/env bash

set -euo pipefail

fail() {
  printf 'github-release-promote: %s\n' "$*" >&2
  exit 1
}

tag=${1:?usage: promote-github-release.sh TAG}
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"
: "${GH_TOKEN:?GH_TOKEN is required}"

[[ "$tag" != *-* ]] || fail "a prerelease tag cannot be promoted to latest"
scripts/release/validate-release-tag.sh "$tag"

release_json=$(gh release view "$tag" --repo "$GITHUB_REPOSITORY" \
  --json tagName,isDraft,isPrerelease)
jq -e --arg tag "$tag" '
  .tagName == $tag and .isDraft == false and .isPrerelease == true
' >/dev/null <<< "$release_json" || fail "release is not a staged prerelease"

gh release edit "$tag" --repo "$GITHUB_REPOSITORY" \
  --draft=false --prerelease=false --latest

release_json=$(gh release view "$tag" --repo "$GITHUB_REPOSITORY" \
  --json tagName,isDraft,isPrerelease)
jq -e --arg tag "$tag" '
  .tagName == $tag and .isDraft == false and .isPrerelease == false
' >/dev/null <<< "$release_json" || fail "release was not promoted to stable"

latest_tag=$(gh api "repos/$GITHUB_REPOSITORY/releases/latest" --jq .tag_name)
[[ "$latest_tag" == "$tag" ]] || fail "promoted release is not latest"

printf 'Promoted and verified GitHub release %s as latest.\n' "$tag"
