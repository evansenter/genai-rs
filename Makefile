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

# Fixture tests for the shell scripts in .github/scripts/, plus shellcheck
# when it is installed. The fixtures observe what the scripts *do* on the
# inputs they supply; shellcheck covers the class they structurally cannot —
# quoting slips, `set -e` interaction with command substitution, word
# splitting — which matters here because a quoting slip that made a branch
# unreachable would leave every existing assertion green. Skipped rather than
# required when absent, so it does not become a second hard dependency for
# contributors; ubuntu-latest ships it, so CI runs it with no workflow edit.
#
# `-S warning` because the runner's shellcheck is unpinned: new releases add
# checks and promote optional ones, so at the default severity a runner image
# bump could redden this job on a PR that touched no shell at all — the same
# fires-on-toolchain-drift shape the size ceiling this PR replaces had. The
# info/style tier is where most new checks land; everything indicating a real
# defect is `warning` or above.
#
# Dependencies are per-harness rather than a fixed set. jq and python3 are
# preflighted here because a harness that dies on a missing interpreter
# reports as a failing assertion instead of naming the tool — the exact
# misdiagnosis these harnesses exist to catch elsewhere. Individual harnesses
# may need more: `test_setup_dev.sh` resolves the real `cc` to probe
# `-fuse-ld=mold`, which is why this target takes a few seconds rather than
# one.
#
# One side effect is worth knowing before running the pre-push gate:
# `test_setup_dev.sh` rewrites the checkout's real `.cargo/config.toml` and
# restores it from a temp copy on an EXIT trap. That file is gitignored, so
# git cannot bring it back if the harness is killed outright. Tracked in #455.
#
# Globbed, so a new harness is picked up by dropping the file in — run through
# `bash` rather than executed, so that claim holds without also needing
# chmod +x.
# A harness missing its exec bit would otherwise fail with "Permission
# denied", which reads as a failing test rather than a missing mode bit.
#
# An empty directory is a failure, not a pass. A check that runs nothing and
# reports success is the exact shape the harnesses themselves exist to catch,
# and every branch that has this target also has at least one harness — so
# an empty match means the files went missing, not that the state is legal.
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
