.PHONY: check fmt clippy test test-all docs clean

# Pre-push gate: format check + lint + unit tests
check: fmt clippy test

# Check formatting
fmt:
	cargo fmt --all -- --check

# Lint with warnings as errors
clippy:
	cargo clippy --workspace --all-targets --all-features -- -D warnings

# Unit tests only (doctests run in CI - excluded locally for speed)
test:
	cargo nextest run

# Full test suite including integration tests (requires GEMINI_API_KEY)
# Doctests excluded locally - they add compile overhead and CI catches them
test-all:
	cargo nextest run --run-ignored all

# Build documentation with warnings as errors (all features + the docs.rs
# feature set, which differ on strict-unknown)
docs:
	RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --document-private-items
	RUSTDOCFLAGS="--cfg docsrs -D warnings" cargo doc --workspace --no-deps --features antigravity

# Clean build artifacts
clean:
	cargo clean
