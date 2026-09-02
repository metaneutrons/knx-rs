# Contributing to knx-rs

## Development setup

Install the Rust toolchain and run the local quality gates before opening a
pull request:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features -- --test-threads=1
```

The CI workflow also checks the MSRV, `no_std` targets, feature combinations,
documentation, and dependency advisories.

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
