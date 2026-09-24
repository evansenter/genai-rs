.PHONY: check fmt clippy test test-all test-scripts docs clean

# Pre-push gate: format check + lint + unit tests + the CI script harnesses
check: fmt clippy test test-scripts

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

# Fixture tests for the shell scripts in .github/scripts/ and scripts/, plus
# shellcheck when installed (at -S warning, so an unpinned shellcheck bump on
# the runner can't redden unrelated PRs). jq and python3 are preflighted so a
# missing interpreter is named rather than reported as a failing assertion.
# Note: test_setup_dev.sh temporarily rewrites the gitignored
# .cargo/config.toml and restores it on exit (#455). An empty harness glob is
# an error, not a pass.
test-scripts:
	@for tool in jq python3; do \
		command -v "$$tool" >/dev/null 2>&1 || { \
			echo "$$tool is required by these harnesses (apt install $$tool / brew install $$tool)" >&2; \
			exit 1; \
		}; \
	done; \
	rc=0; \
	if command -v shellcheck >/dev/null 2>&1; then \
		echo "==> shellcheck"; \
		shellcheck -S warning .github/scripts/*.sh .github/scripts/tests/*.sh scripts/*.sh || rc=1; \
	else \
		echo "==> shellcheck not installed, skipping lint"; \
	fi; \
	found=0; \
	for t in .github/scripts/tests/*.sh; do \
		[ -e "$$t" ] || continue; \
		found=1; \
		echo "==> $$t"; \
		bash "$$t" || rc=1; \
	done; \
	if [ "$$found" -eq 0 ]; then \
		echo "no harnesses matched .github/scripts/tests/*.sh — expected at least one" >&2; \
		exit 1; \
	fi; \
	exit "$$rc"

# Build documentation with warnings as errors (all features + the docs.rs
# feature set, which differ on strict-unknown). The mirror build uses its
# own target dir so target/doc stays browsable as the all-features build.
docs:
	RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --document-private-items
	RUSTDOCFLAGS="--cfg docsrs -D warnings" cargo doc --workspace --no-deps --features antigravity --target-dir target/doc-docsrs

# Clean build artifacts
clean:
	cargo clean
