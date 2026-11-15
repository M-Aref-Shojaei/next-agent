.PHONY: test test-ts check-workspace check-rust-structure check-boundaries fmt lint topology

test: test-ts check

check:
	node scripts/check-workspace.mjs
	node scripts/check-rust-structure.mjs
	node scripts/check-policy-parity.mjs
	node scripts/check-boundaries.mjs
	node scripts/check-no-coucou-assets.mjs
	node scripts/check-design-tokens.mjs
	node scripts/gen-capabilities.mjs --check
	node scripts/gen-topology.mjs --check

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

# Regenerate docs/architecture/Project Topology.md (Mermaid project graph).
topology:
	node scripts/gen-topology.mjs
