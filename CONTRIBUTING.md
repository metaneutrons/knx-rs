# Contributing to knx-rs

## Development setup

Install the repository toolchain, Lefthook, Gitleaks, and the Cargo quality
tools, then install the hooks:

```bash
brew install lefthook gitleaks
cargo install cargo-nextest cargo-deny cargo-llvm-cov
make setup
```

Run the local quality gates before opening a pull request:

```bash
make check
```

The CI workflow additionally checks the MSRV, `no_std` targets, feature
combinations, documentation, the coverage floor, and the complete Git history
for credentials. The pre-push Clippy and test gates take about 21 seconds on a
warm Apple Silicon development build; the complete feature matrix stays in CI.

## Branches and commits

Use a short branch name with a descriptive prefix such as `feat/`, `fix/`,
`docs/`, or `ci/`. Commit messages and pull request titles must follow
[Conventional Commits](https://www.conventionalcommits.org/), for example:

```text
fix(knxip): reject truncated discovery responses
```

Keep each change focused. Add regression tests for behavioral changes and
document public APIs. Do not include generated attribution trailers in commits
or pull request descriptions.

## Pull requests

Describe the user-visible behavior, compatibility impact, and verification
performed. Pull requests are squash-merged after all required checks pass and
review conversations are resolved.

## Contributor License Agreement (CLA)

You retain copyright. Under the CLA, you license your contribution under
Apache-2.0. This lets Fabian Schmieder distribute it as part of knx-rs under
permissive or proprietary terms, subject to the Apache licence and its notice
requirements. The current knx-rs release uses GPL-3.0-only.
[English agreement](CONTRIBUTION-LICENSE.md) · [Deutsche Fassung](CONTRIBUTION-LICENSE.de.md)

1. Read the agreement and tick the versioned CLA box in your pull request.
   Identify third-party material, its source and its licence in the description.
2. If several people contributed, each signs their own commits with
   `git commit -s`. Each named co-author adds their own `Signed-off-by:` line
   with the same email used in their `Co-authored-by:` line. For this project
   the sign-off records acceptance of the CLA under section 8.
3. Before merging, check that any disclosed third-party licences permit the
   intended uses. Keep the PR declaration in the squash commit and retain the
   original sign-offs for multi-author PRs in its message.

The required **Contribution licence declaration** check runs again when the PR
description or its commits change. The maintainer and the project's dependency,
release and CI bots are exempt for their own submissions. The check examines
other authors on those PRs too.

For security issues, follow [SECURITY.md](SECURITY.md) instead of opening a
public pull request.
