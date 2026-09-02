.PHONY: setup check release-check

setup:
	command -v lefthook >/dev/null
	command -v gitleaks >/dev/null
	lefthook install

check:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
	cargo nextest run --workspace --all-features --locked
	cargo deny --all-features check
	$(MAKE) release-check

release-check:
	scripts/release/test-release.sh
