# Releasing knx-rs

Production releases are created only by Release Please. Do not create, move, or
delete a `v*` tag manually. GitHub rules make those tags immutable.

## Production flow

1. Merge Conventional Commit pull requests to `main`.
2. Review the Release Please pull request, including `CHANGELOG.md`, all
   workspace versions, internal dependency requirements, and `Cargo.lock`.
3. Merge that pull request after every required check passes.
4. Release Please creates the production tag and draft GitHub release, then
   dispatches `.github/workflows/release.yml` at that exact tag.
5. The release workflow validates tag identity, tests the workspace, builds a
   deterministic source archive, and verifies its exact inventory and checksum.
6. A crates.io OIDC preflight must succeed before the GitHub draft becomes a
   public prerelease.
7. Cargo publishes the workspace exactly once with a short-lived Trusted
   Publishing token. Every crate version is then read back from crates.io.
8. Only after all crates are public is the unchanged GitHub prerelease promoted
   to the stable latest release.

The `release` environment accepts deployments only from `main` and `v*` tags.
Each publishable crate must bind its crates.io Trusted Publisher to owner
`metaneutrons`, repository `knx-rs`, workflow `release.yml`, and environment
`release`. No long-lived crates.io token belongs in GitHub Actions.

## Pipeline validation

A planned tag such as `v<manifest-version>-repo-standard.1` may exercise the
complete GitHub release path. Its version core must match both manifests. A
prerelease test never requests a crates.io token and never publishes packages.
The test tag is permanently consumed and must never be promoted or reused.

## Failure handling

Do not move or recreate a release tag. A failed prerelease test uses a new
suffix after the fix. A failed production publish may have uploaded only part
of the workspace because registries are not transactional; inspect crates.io
and recover with a new patch release, never by retrying the same version.
