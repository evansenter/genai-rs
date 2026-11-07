#!/usr/bin/env bash
# Exercises `check_dated_facts.sh` (D-032's enforcement) against fixture
# trees.
#
# Each case builds a small git work tree holding the homes, the history
# files, a guide and some Rust, plants one file, and checks the exit status
# and the finding. The interesting parts are what the check must not read:
# D-032's exceptions (the homes, DECISIONS.md and CHANGELOG.md, test code,
# `//` comments in src/), a link to a home's dated heading, a timestamp, the
# API revision; and the test-code boundary, an inline `#[cfg(test)]` module
# that must not hide the code after it.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$(dirname "$HERE")/check_dated_facts.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

failures=0
cases=0

# A fresh tree that passes: dated facts only where D-032 allows them.
base_tree() {
  local tree="$1"
  rm -rf "$tree"
  mkdir -p "$tree/docs" "$tree/src" "$tree/tests/fixtures" "$tree/examples"
  git -C "$tree" init -q
  printf '%s\n' '- **A fact** (live 2026-11-05).' >"$tree/docs/INTERACTIONS_API_GAP.md"
  printf '%s\n' '| `Status` | `"active"` | verified live 2026-08-16 |' >"$tree/docs/ENUM_WIRE_FORMATS.md"
  printf '%s\n' 'Findings (0.1.23, 2026-11-07, scripted model):' >"$tree/docs/ANTIGRAVITY_BRIDGE_DESIGN.md"
  printf '%s\n' '## D-001 — A decision (2026-11-06)' >"$tree/DECISIONS.md"
  printf '%s\n' '## [0.13.0] - 2026-11-03' >"$tree/CHANGELOG.md"
  cat >"$tree/docs/GUIDE.md" <<'MD'
# A guide

The API accepts it ([gap tracker](INTERACTIONS_API_GAP.md#resource-residue-live-2026-11-05)).
Every request sends `Api-Revision: 2026-05-20`.

```text
[LOUD_WIRE] 2026-01-01T10:30:45Z [REQ#1] >>> POST
```

<!-- ignore: a fragment, not a program -->
```rust,ignore
let x = y;
```
MD
  printf '%s\n' 'Captured live 2026-11-05.' >"$tree/tests/fixtures/README.md"
  cat >"$tree/src/lib.rs" <<'RS'
//! The crate.

/// Reads a store. A store serves its counts as strings.
pub fn read() {
    // The API served `"1"` (live 2026-11-05), so this reads a string.
}

#[cfg(test)]
mod tests {
    /// The body as served live 2026-11-05.
    fn body() {}
}
RS
  printf '%s\n' '/// Captured live 2026-09-24.' 'fn fixture() {}' >"$tree/src/lib_tests.rs"
  printf '%s\n' '/// Observed 2026-11-04.' '#[test] fn t() {}' >"$tree/tests/live.rs"
  printf '%s\n' '//! Shows a store.' 'fn main() {}' >"$tree/examples/store.rs"
}

run_check() {
  rc=0
  out=$(bash "$SCRIPT" "$1" 2>&1) || rc=$?
}

expect() {
  local label="$1" want_rc="$2" needle="$3"
  cases=$((cases + 1))
  if [ "$rc" -eq "$want_rc" ] && grep -qF -- "$needle" <<<"$out"; then
    echo "ok   - $label (exit $rc)"
  else
    echo "FAIL - $label: expected exit $want_rc and '$needle', got exit $rc"
    sed 's/^/         /' <<<"$out"
    failures=$((failures + 1))
  fi
}

T="$WORK/tree"

base_tree "$T"
run_check "$T"
expect "the homes, history, tests, // comments, links, timestamps and the revision pass" 0 "No dated live fact"

# Rule 1 in a guide, in the README and in a link's text (not its target).
base_tree "$T"
printf '%s\n' '' 'Environments from 2026-07 were still there.' >>"$T/docs/GUIDE.md"
run_check "$T"
expect "a dated fact in a guide fails, naming its line" 1 "docs/GUIDE.md:15: dated live fact (2026-07)"

base_tree "$T"
printf '%s\n' 'Labels are echoed (verified live 2026-09-24).' >"$T/README.md"
run_check "$T"
expect "a dated fact in the README fails" 1 "README.md:1: dated live fact (2026-09-24)"

base_tree "$T"
printf '%s\n' 'See [the residue (2026-11-05)](INTERACTIONS_API_GAP.md#residue).' >>"$T/docs/GUIDE.md"
run_check "$T"
expect "a date in a link's text fails" 1 "dated live fact (2026-11-05)"

base_tree "$T"
printf '%s\n' '[residue]: INTERACTIONS_API_GAP.md#resource-residue-live-2026-11-05' >>"$T/docs/GUIDE.md"
run_check "$T"
expect "a reference link's target passes" 0 "No dated live fact"

# Rule 1 in Rust: a doc comment of non-test code, wherever it sits.
base_tree "$T"
printf '%s\n' '' '/// Accepted by the API (verified live 2026-10-30).' 'pub fn accepted() {}' >>"$T/src/lib.rs"
run_check "$T"
expect "a dated doc comment after an inline test module fails" 1 "src/lib.rs:14: dated live fact (2026-10-30) in a doc comment"

base_tree "$T"
mkdir -p "$T/src/http"
printf '%s\n' '//! Requests (live 2026-11-05).' >"$T/src/http/mod.rs"
run_check "$T"
expect "a dated module doc fails" 1 "src/http/mod.rs:1: dated live fact (2026-11-05)"

base_tree "$T"
mkdir -p "$T/genai-rs-macros/src"
printf '%s\n' '/// Generates a declaration (2026-11-01).' >"$T/genai-rs-macros/src/lib.rs"
run_check "$T"
expect "the macros crate's rustdoc is read too" 1 "genai-rs-macros/src/lib.rs:1"

base_tree "$T"
printf '%s\n' 'fn main() {' '    // The API answers 200 (2026-11-05).' '}' >"$T/examples/store.rs"
run_check "$T"
expect "a dated // comment in an example fails" 1 "examples/store.rs:2: dated live fact (2026-11-05) in an example"

base_tree "$T"
printf '%s\n' '//! Polls the store.' '//! Indexing took 2 s on 2026-08-16.' >"$T/examples/store.rs"
run_check "$T"
expect "a dated example header fails" 1 "examples/store.rs:2"

# Test code is exempt: a test-only file, the oracle, a gated item, a
# `////` line, and a `#[cfg(test)]` module declared out of line.
base_tree "$T"
mkdir -p "$T/src/coverage_oracle"
printf '%s\n' '/// Not served (2026-11-05).' >"$T/src/coverage_oracle/mod.rs"
printf '%s\n' '/// Seed (2026-11-04).' >"$T/src/proptest_steps.rs"
printf '%s\n' '#[cfg(test)]' '/// A helper (2026-11-05).' 'fn helper() {' '}' '//// A banner, 2026-11-05.' >>"$T/src/lib.rs"
run_check "$T"
expect "the oracle, property tests, a gated helper and //// lines pass" 0 "No dated live fact"

base_tree "$T"
printf '%s\n' '#[cfg(test)]' '#[path = "lib_tests.rs"]' 'mod tests;' '/// Stored (live 2026-11-01).' 'pub fn after() {}' >>"$T/src/lib.rs"
run_check "$T"
expect "an out-of-line test module hides only itself" 1 "src/lib.rs:16"

# Untracked files are read, so a new guide is checked before its commit.
base_tree "$T"
printf '%s\n' 'Observed 2026-11-05.' >"$T/docs/NEW.md"
run_check "$T"
expect "an untracked new file is read" 1 "docs/NEW.md:1"

# Ignored files are not.
base_tree "$T"
printf '%s\n' 'target/' >"$T/.gitignore"
mkdir -p "$T/target"
printf '%s\n' 'Observed 2026-11-05.' >"$T/target/NOTES.md"
run_check "$T"
expect "a gitignored file is not read" 0 "No dated live fact"

# Rule 2: the reason line, in docs and in rustdoc.
base_tree "$T"
printf '%s\n' '' '```rust,ignore' 'let x = y;' '```' >>"$T/docs/GUIDE.md"
run_check "$T"
expect "an ignore fence without a reason fails" 1 "docs/GUIDE.md:15: an ignore fence needs"

# Rustdoc splits the info string on spaces and tabs as well as commas.
base_tree "$T"
printf '%s\n' '' '```rust ignore' 'let x = y;' '```' >>"$T/docs/GUIDE.md"
run_check "$T"
expect "an ignore after a space, as rustdoc reads it, fails" 1 "docs/GUIDE.md:15: an ignore fence needs"

base_tree "$T"
printf '%s\n' '' '/// ```rust,	ignore' '/// x' '/// ```' 'pub fn tabbed() {}' >>"$T/src/lib.rs"
run_check "$T"
expect "an ignore after a comma and a tab, in rustdoc, fails" 1 "src/lib.rs:14: an ignore fence needs a"

base_tree "$T"
printf '%s\n' '<!-- ignore: a fragment -->' '' '```ignore' 'x' '```' >>"$T/docs/ENUM_WIRE_FORMATS.md"
run_check "$T"
expect "a blank line between the reason and the fence fails, in a home too" 1 "docs/ENUM_WIRE_FORMATS.md:4"

base_tree "$T"
printf '%s\n' '<!-- ignore: -->' '```rust,ignore' 'x' '```' >>"$T/docs/GUIDE.md"
run_check "$T"
expect "an empty reason fails" 1 "an ignore fence needs"

base_tree "$T"
printf '%s\n' '````markdown' '```rust,ignore' 'x' '```' '````' '```rust' 'fn ignore() {}' '```' >>"$T/docs/GUIDE.md"
run_check "$T"
expect "a fence quoted inside a longer fence, and ignore as code, pass" 0 "No dated live fact"

base_tree "$T"
printf '%s\n' '' '/// ```ignore' '/// x' '/// ```' 'pub fn quoted() {}' >>"$T/src/lib.rs"
run_check "$T"
expect "a rustdoc ignore fence without a reason fails" 1 "src/lib.rs:14: an ignore fence needs a"

base_tree "$T"
printf '%s\n' '' '/// <!-- ignore: a fragment -->' '/// ```ignore' '/// x' '/// ```' 'pub fn quoted() {}' >>"$T/src/lib.rs"
run_check "$T"
expect "a rustdoc ignore fence with a reason passes" 0 "No dated live fact"

# Every finding is listed, not just the first.
base_tree "$T"
printf '%s\n' 'One (2026-11-01).' 'Two (2026-11-02).' >>"$T/docs/GUIDE.md"
run_check "$T"
expect "every finding is listed" 1 "2 finding(s)"

# Usage errors.
rc=0
out=$(bash "$SCRIPT" "$WORK/not-a-tree" 2>&1) || rc=$?
expect "a directory that is not a git work tree is a usage error" 2 "not a git work tree"
rc=0
out=$(bash "$SCRIPT" a b 2>&1) || rc=$?
expect "two arguments are a usage error" 2 "Usage:"

# The real tree, from the repo root: the default ROOT reads it.
rc=0
out=$(cd "$HERE/../../.." && bash "$SCRIPT" 2>&1) || rc=$?
expect "the checkout follows both rules" 0 "No dated live fact outside its home"

echo
if [ "$failures" -gt 0 ]; then
  echo "$failures of $cases check(s) failed."
  exit 1
fi
echo "All $cases checks passed."
