#!/usr/bin/env bash

set -euo pipefail

fail() {
  printf 'crates-io-verification: %s\n' "$*" >&2
  exit 1
}

version=${1:?usage: verify-crates-io.sh VERSION}
[[ "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] \
  || fail "version is not a stable semantic version: $version"

api_base=${CRATES_IO_API_BASE:-https://crates.io/api/v1}
attempts=${CRATES_IO_VERIFY_ATTEMPTS:-12}
delay=${CRATES_IO_VERIFY_DELAY_SECONDS:-10}
[[ "$attempts" =~ ^[1-9][0-9]*$ ]] || fail "attempt count must be positive"
[[ "$delay" =~ ^[0-9]+$ ]] || fail "retry delay must be non-negative"

metadata=$(cargo metadata --no-deps --format-version 1 --locked)
packages=$(jq -r '
  .workspace_members as $members
  | .packages[]
  | select(.id as $id | $members | index($id))
  | select(.publish != [])
  | .name
' <<< "$metadata" | LC_ALL=C sort)
[[ -n "$packages" ]] || fail "workspace has no publishable crates"

while IFS= read -r package; do
  attempt=1
  verified=false
  while [[ "$attempt" -le "$attempts" ]]; do
    if response=$(curl --fail --silent --show-error --location \
      --user-agent "knx-rs-release-verifier/$version" \
      "$api_base/crates/$package/$version"); then
      if jq -e --arg version "$version" '.version.num == $version' \
        >/dev/null <<< "$response"; then
        verified=true
        break
      fi
    fi

    if [[ "$attempt" -lt "$attempts" ]]; then
      sleep "$delay"
    fi
    attempt=$((attempt + 1))
  done

  [[ "$verified" == "true" ]] \
    || fail "$package $version is not visible from the public registry"
  printf 'Verified crates.io package %s %s.\n' "$package" "$version"
done <<< "$packages"
