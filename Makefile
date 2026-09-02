.PHONY: setup check

setup:
	command -v lefthook >/dev/null
	command -v gitleaks >/dev/null
	lefthook install

check:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
	cargo nextest run --workspace --all-features --locked
	cargo deny --all-features check
