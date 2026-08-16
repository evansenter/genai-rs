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

# Fixture tests for the shell scripts in .github/scripts/. Needs only bash
# and jq, so it runs in about a second — worth having at the edit rather than
# only inside build-metrics, which starts with a `cargo clean`. Globbed, so a
# new harness is picked up by dropping the file in — run through `bash`
# rather than executed, so that claim holds without also needing chmod +x.
# A harness missing its exec bit would otherwise fail with "Permission
# denied", which reads as a failing test rather than a missing mode bit.
#
# An empty directory is a failure, not a pass. A check that runs nothing and
# reports success is the exact shape the harnesses themselves exist to catch,
# and every branch that has this target also has at least one harness — so
# an empty match means the files went missing, not that the state is legal.
test-scripts:
	@found=0; \
	for t in .github/scripts/tests/*.sh; do \
		[ -e "$$t" ] || continue; \
		found=1; \
		echo "==> $$t"; \
		bash "$$t" || exit 1; \
	done; \
	if [ "$$found" -eq 0 ]; then \
		echo "no harnesses matched .github/scripts/tests/*.sh — expected at least one" >&2; \
		exit 1; \
	fi

# Build documentation with warnings as errors (all features + the docs.rs
# feature set, which differ on strict-unknown). The mirror build uses its
# own target dir so target/doc stays browsable as the all-features build.
docs:
	RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --document-private-items
	RUSTDOCFLAGS="--cfg docsrs -D warnings" cargo doc --workspace --no-deps --features antigravity --target-dir target/doc-docsrs

# Clean build artifacts
clean:
	cargo clean
