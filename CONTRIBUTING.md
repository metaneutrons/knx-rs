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

For security issues, follow [SECURITY.md](SECURITY.md) instead of opening a
public pull request.
