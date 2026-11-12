.PHONY: test test-ts check-workspace check-rust-structure check-boundaries fmt lint

test: test-ts check

check:
	node scripts/check-workspace.mjs
	node scripts/check-rust-structure.mjs
	node scripts/check-policy-parity.mjs
	node scripts/check-boundaries.mjs

test-ts:
	node scripts/test.mjs

check-workspace:
	node scripts/check-workspace.mjs

test-rust:
	cargo test --workspace --all-targets

fmt:
	cargo fmt --all

lint:
	cargo clippy --workspace --all-targets -- -D warnings
