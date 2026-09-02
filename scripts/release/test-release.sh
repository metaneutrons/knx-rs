#!/usr/bin/env bash

set -euo pipefail

fail() {
  printf 'release-contract-test: %s\n' "$*" >&2
  exit 1
}

repo_root=$(git rev-parse --show-toplevel)
cd "$repo_root"

version=$(jq -er '."."' .release-please-manifest.json)
suffix="repo-standard.$$"
valid_tag="v${version}-${suffix}"
wrong_target_tag="v${version}-${suffix}.wrong"
temporary_dir=$(mktemp -d)

cleanup() {
  if git rev-parse --verify --quiet "refs/tags/$valid_tag" >/dev/null; then
    git tag --delete "$valid_tag" >/dev/null
  fi
  if git rev-parse --verify --quiet "refs/tags/$wrong_target_tag" >/dev/null; then
    git tag --delete "$wrong_target_tag" >/dev/null
  fi
  rm -rf "$temporary_dir"
}
trap cleanup EXIT

git tag "$valid_tag" HEAD
scripts/release/validate-release-tag.sh "$valid_tag" >/dev/null
scripts/release/build-source-archive.sh "$valid_tag" "$temporary_dir/first" >/dev/null
scripts/release/build-source-archive.sh "$valid_tag" "$temporary_dir/second" >/dev/null
archive="knx-rs-${valid_tag}.tar.gz"
scripts/release/verify-assets.sh "$temporary_dir/first" "$archive" >/dev/null
cmp "$temporary_dir/first/$archive" "$temporary_dir/second/$archive"
cmp "$temporary_dir/first/SHA256SUMS" "$temporary_dir/second/SHA256SUMS"

if output=$(scripts/release/validate-release-tag.sh "v01.2.3-invalid" 2>&1); then
  fail "non-canonical tag unexpectedly passed"
fi
grep -Fq 'tag is not canonical SemVer' <<< "$output" \
  || fail "non-canonical tag failed for the wrong reason"

wrong_target_commit=$(
  printf 'synthetic wrong release target\n' \
    | GIT_AUTHOR_NAME='Release Contract Test' \
      GIT_AUTHOR_EMAIL='release-contract@example.invalid' \
      GIT_AUTHOR_DATE='2000-01-01T00:00:00Z' \
      GIT_COMMITTER_NAME='Release Contract Test' \
      GIT_COMMITTER_EMAIL='release-contract@example.invalid' \
      GIT_COMMITTER_DATE='2000-01-01T00:00:00Z' \
      git commit-tree "$(git rev-parse 'HEAD^{tree}')"
)
git tag "$wrong_target_tag" "$wrong_target_commit"
if output=$(scripts/release/validate-release-tag.sh "$wrong_target_tag" 2>&1); then
  fail "wrong tag target unexpectedly passed"
fi
grep -Fq 'tag target does not match the checked-out commit' <<< "$output" \
  || fail "wrong tag target failed for the wrong reason"

cp -R "$temporary_dir/first" "$temporary_dir/tampered"
printf 'x' >> "$temporary_dir/tampered/$archive"
if output=$(scripts/release/verify-assets.sh "$temporary_dir/tampered" "$archive" 2>&1); then
  fail "tampered archive unexpectedly passed"
fi
grep -Fq 'checksum mismatch' <<< "$output" \
  || fail "tampered archive failed for the wrong reason"

cp -R "$temporary_dir/first" "$temporary_dir/extra-asset"
touch "$temporary_dir/extra-asset/unexpected.txt"
if output=$(scripts/release/verify-assets.sh "$temporary_dir/extra-asset" "$archive" 2>&1); then
  fail "unexpected asset unexpectedly passed"
fi
grep -Fq 'asset inventory does not match' <<< "$output" \
  || fail "unexpected asset failed for the wrong reason"

mkdir -p "$temporary_dir/bin"
cat > "$temporary_dir/bin/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
url=''
for argument in "$@"; do
  url=$argument
done
case "$url" in
  */"$FAKE_CRATE_VERSION")
    printf '{"version":{"num":"%s"}}\n' "$FAKE_CRATE_VERSION"
    ;;
  *)
    exit 22
    ;;
esac
EOF
chmod +x "$temporary_dir/bin/curl"

FAKE_CRATE_VERSION="$version" PATH="$temporary_dir/bin:$PATH" \
  CRATES_IO_VERIFY_ATTEMPTS=1 CRATES_IO_VERIFY_DELAY_SECONDS=0 \
  scripts/release/verify-crates-io.sh "$version" >/dev/null

if output=$(FAKE_CRATE_VERSION="$version" PATH="$temporary_dir/bin:$PATH" \
  CRATES_IO_VERIFY_ATTEMPTS=1 CRATES_IO_VERIFY_DELAY_SECONDS=0 \
  scripts/release/verify-crates-io.sh 99.99.99 2>&1); then
  fail "missing registry version unexpectedly passed"
fi
grep -Fq 'is not visible from the public registry' <<< "$output" \
  || fail "missing registry version failed for the wrong reason"

printf 'Release contract positive and negative probes passed.\n'
