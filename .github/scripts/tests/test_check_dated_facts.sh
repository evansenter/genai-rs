#!/usr/bin/env bash
# Exercises `check_dated_facts.sh` (D-032's enforcement) against fixture
# trees.
#
# Each case builds a small git work tree holding the homes, the history
# files, a guide and some Rust, plants one file, and checks the exit status
# and the finding. The interesting parts are what the check must not read:
# D-032's exceptions (the homes, DECISIONS.md and CHANGELOG.md, test code,
# `//` comments in src/), a link to a home's dated heading, a timestamp, the
# API revision; the test-code boundary, an inline `#[cfg(test)]` module
# that must not hide the code after it; the date forms beyond ISO, a date
# wrapped across lines, and a time with and without seconds; and the fence
# rules: a reason on every `ignore`, test code included; no Rust block
# nothing compiles (in a file nothing doctests, behind an include that is
# commented out or under a `cfg` the doctest run leaves out, or in a doc
# comment outside the library's non-test code, an inline item or module
# under such a `cfg` included); a `cfg` the check cannot evaluate; no `rs`
# or `Rust` fence; and no Rust in a text block, a bare `?;` or `.await`
# line included. Every rule reads every doc-comment form: `///`, `//!`,
# `/** */`, `/*! */`, `#[doc = "..."]` and `#![doc = "..."]`.

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

# A gated item whose signature spans lines is skipped through its body,
# and the check resumes after it.
base_tree "$T"
printf '%s\n' '#[cfg(test)]' 'fn helper(' '    a: u8,' ') {' '    /// Seen live 2026-11-05.' '    let _ = a;' '}' >>"$T/src/lib.rs"
run_check "$T"
expect "a gated fn with a multi-line signature is skipped whole" 0 "No dated live fact"
printf '%s\n' '/// Stored (live 2026-11-01).' 'pub fn after() {}' >>"$T/src/lib.rs"
run_check "$T"
expect "the check resumes after a multi-line gated item" 1 "src/lib.rs:20"

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
printf '%s\n' '#[doc = include_str!("../docs/GUIDE.md")]' 'mod guide {}' >>"$T/src/lib.rs"
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

# Rule 1 reads a date however it is written, not only as ISO.
for form in 'November 5, 2026' 'Nov 5, 2026' 'Nov. 5 2026' '5 November 2026' \
  '2026/11/05' '11/05/2026' 'November 2026' '2026-11-5' '2026.11.05' '05.11.2026' '11-05-2026'; do
  base_tree "$T"
  printf '%s\n' '' "The API accepted it ($form)." >>"$T/docs/GUIDE.md"
  run_check "$T"
  expect "a date written as '$form' fails" 1 "docs/GUIDE.md:15: dated live fact ($form)"
done

base_tree "$T"
printf '%s\n' '' '/// Seen on Nov 5, 2026.' 'pub fn seen() {}' >>"$T/src/lib.rs"
run_check "$T"
expect "a named-month date in a doc comment fails" 1 "src/lib.rs:14: dated live fact (Nov 5, 2026) in a doc comment"

base_tree "$T"
printf '%s\n' '' 'A `Retry-After` of `Wed, 21 Oct 2015 07:28:00 GMT` waits until then.' \
  'Logged at 2026/11/05 10:30:45, 11/05/2026, 10:30:45 and 2026-11-05 10:30:45.' \
  'Version 1/2 of 3, release 0.14.0, address 10.11.12.13.' >>"$T/docs/GUIDE.md"
run_check "$T"
expect "an HTTP date, timestamps with seconds, a fraction and a version pass" 0 "No dated live fact"

# A time without seconds is how prose dates an observation, in any form.
base_tree "$T"
printf '%s\n' '' 'Observed November 5, 2026, 10:30 UTC.' 'Observed 2026/11/05 10:30 UTC.' \
  'Observed 2026-11-05 10:30 UTC.' >>"$T/docs/GUIDE.md"
run_check "$T"
expect "a named date with a time but no seconds fails" 1 "docs/GUIDE.md:15: dated live fact (November 5, 2026)"
expect "a slashed date with a time but no seconds fails" 1 "docs/GUIDE.md:16: dated live fact (2026/11/05)"
expect "an ISO date with a time but no seconds fails" 1 "docs/GUIDE.md:17: dated live fact (2026-11-05)"

# A named-month date has spaces, so hard-wrapped prose breaks it across
# lines: the finding names its first line.
base_tree "$T"
printf '%s\n' '' 'The API began sending it as of November' '5, 2026, so the helper reads it.' >>"$T/docs/GUIDE.md"
run_check "$T"
expect "a date wrapped across two lines fails, on its first" 1 "docs/GUIDE.md:15: dated live fact (November 5, 2026)"

base_tree "$T"
printf '%s\n' '' '/// As of November' '/// 5, 2026 the API sets it.' 'pub fn set() {}' >>"$T/src/lib.rs"
run_check "$T"
expect "a date wrapped across two doc lines fails" 1 "src/lib.rs:14: dated live fact (November 5, 2026) in a doc comment"

base_tree "$T"
printf '%s\n' 'fn main() {' '    // The API answers 200 as of Nov' '    // 5, 2026.' '}' >"$T/examples/store.rs"
run_check "$T"
expect "a date wrapped across two example comments fails" 1 "examples/store.rs:2: dated live fact (Nov 5, 2026) in an example"

# Rule 2 reaches every doc comment: test code and tests/ too, where rule 1
# does not look.
base_tree "$T"
printf '%s\n' '//! Shared helpers.' '//! ```ignore' '//! mod common;' '//! ```' >"$T/tests/helpers.rs"
run_check "$T"
expect "an ignore fence in a tests/ helper needs a reason" 1 "tests/helpers.rs:2: an ignore fence needs a"

base_tree "$T"
printf '%s\n' '//! Shared helpers (2026-11-05).' '//! <!-- ignore: a test crate is not doctested -->' \
  '//! ```ignore' '//! mod common;' '//! ```' >"$T/tests/helpers.rs"
run_check "$T"
expect "a tests/ helper's reasoned ignore fence, and its date, pass" 0 "No dated live fact"

base_tree "$T"
printf '%s\n' '/// ```ignore' '/// x' '/// ```' 'fn fixture() {}' >"$T/src/lib_tests.rs"
run_check "$T"
expect "an ignore fence in a unit-test file needs a reason" 1 "src/lib_tests.rs:1: an ignore fence needs a"

base_tree "$T"
printf '%s\n' '#[cfg(test)]' '/// ```rust,ignore' '/// x' '/// ```' 'fn helper() {' '}' >>"$T/src/lib.rs"
run_check "$T"
expect "an ignore fence on a gated test item needs a reason" 1 "src/lib.rs:14: an ignore fence needs a"

# Rule 3: a Rust block in Markdown that nothing doctests.
base_tree "$T"
printf '%s\n' '' '```rust' 'let x = 1;' '```' >>"$T/docs/GUIDE.md"
run_check "$T"
expect "a rust fence in a guide src/lib.rs does not include fails" 1 "docs/GUIDE.md:15: a Rust block here never compiles"

base_tree "$T"
printf '%s\n' '' '```rust,no_run' 'let x = 1;' '```' >>"$T/docs/GUIDE.md"
printf '%s\n' '' '```no_run' 'let x = 1;' '```' >"$T/README.md"
run_check "$T"
expect "a no_run fence is Rust too" 1 "docs/GUIDE.md:15: a Rust block here never compiles"
expect "a bare rustdoc attribute is Rust too" 1 "README.md:2: a Rust block here never compiles"

base_tree "$T"
printf '%s\n' '' '```rust' 'let x = 1;' '```' >>"$T/docs/GUIDE.md"
printf '%s\n' '' '#[cfg(doctest)]' 'mod doc_tests {' '    #[doc = include_str!("../docs/GUIDE.md")]' '    mod guide {}' '}' >>"$T/src/lib.rs"
printf '%s\n' '' '```rust' 'client.old_call();' '```' >>"$T/CHANGELOG.md"
run_check "$T"
expect "a doctested guide's rust fence, and CHANGELOG's, pass" 0 "No dated live fact"

base_tree "$T"
printf '%s\n' '' '```rust' 'let x = 1;' '```' >>"$T/docs/GUIDE.md"
printf '%s\n' '' '#[cfg(doctest)]' 'mod doc_tests {' '    #[cfg(feature = "antigravity")]' \
  '    #[doc = include_str!("../docs/GUIDE.md")]' '    mod guide {}' '}' >>"$T/src/lib.rs"
run_check "$T"
expect "an include behind a feature, which the all-features run has, counts" 0 "No dated live fact"

# An include that is commented out, or behind a cfg the doctest run never
# sets, doctests nothing.
for include in '// #[doc = include_str!("../docs/GUIDE.md")]' \
  '/* #[doc = include_str!("../docs/GUIDE.md")] */' \
  '#[cfg(any())] #[doc = include_str!("../docs/GUIDE.md")]' \
  '#[doc = include_str!("../docs/GUIDE.md")] #[cfg(not(doctest))]'; do
  base_tree "$T"
  printf '%s\n' '' '```rust' 'let x = 1;' '```' >>"$T/docs/GUIDE.md"
  printf '%s\n' '' '#[cfg(doctest)]' 'mod doc_tests {' "    $include" '    mod guide {}' '}' >>"$T/src/lib.rs"
  run_check "$T"
  expect "an include written as '$include' doctests nothing" 1 "docs/GUIDE.md:15: a Rust block here never compiles"
done

# `rs` and `Rust` are Rust to GitHub and another language to rustdoc, so
# nothing compiles them, even in a doctested guide; an `edition` token is
# Rust to rustdoc.
base_tree "$T"
printf '%s\n' '' '```rs' 'let x = 1;' '```' >>"$T/docs/GUIDE.md"
printf '%s\n' '' '#[cfg(doctest)]' 'mod doc_tests {' '    #[doc = include_str!("../docs/GUIDE.md")]' '    mod guide {}' '}' >>"$T/src/lib.rs"
printf '%s\n' '' '```Rust' 'let x = 1;' '```' >"$T/README.md"
printf '%s\n' '' '```edition2021' 'let x = 1;' '```' >"$T/docs/OTHER.md"
run_check "$T"
expect "an rs fence in a doctested guide fails" 1 "docs/GUIDE.md:15: a \`rs\` fence is Rust to GitHub"
expect "a Rust fence fails" 1 "README.md:2: a \`Rust\` fence is Rust to GitHub"
expect "an edition fence in a guide nothing doctests fails" 1 "docs/OTHER.md:2: a Rust block here never compiles"

# In Rust, rustdoc compiles the library's doc comments only: not tests/,
# examples/, test files, test items, or a module compiled only in tests.
# There an untagged block is Rust too.
base_tree "$T"
printf '%s\n' '//! Shared helpers.' '//! ```rust' '//! let x = 1;' '//! ```' >"$T/tests/helpers.rs"
printf '%s\n' '//! Shows a store.' '//! ```' '//! let x = 1;' '//! ```' 'fn main() {}' >"$T/examples/store.rs"
printf '%s\n' '/// ```' '/// let x = 1;' '/// ```' 'fn fixture() {}' >"$T/src/lib_tests.rs"
run_check "$T"
expect "a Rust block in a tests/ helper's doc fails" 1 "tests/helpers.rs:2: a Rust block in this doc comment never compiles"
expect "an untagged block in an example's doc fails" 1 "examples/store.rs:2: a Rust block in this doc comment"
expect "an untagged block in a unit-test file fails" 1 "src/lib_tests.rs:1: a Rust block in this doc comment"

base_tree "$T"
printf '%s\n' '#[cfg(test)]' '/// <!-- ignore: a fragment -->' '/// ```rust,ignore' '/// x' '/// ```' \
  '/// ```no_run' '/// let x = 1;' '/// ```' 'fn helper() {' '}' '#[cfg(test)]' 'mod oracle;' >>"$T/src/lib.rs"
printf '%s\n' '/// ```rust' '/// let x = 1;' '/// ```' 'pub fn reach() {}' >"$T/src/oracle.rs"
run_check "$T"
expect "a Rust block on a gated test item fails" 1 "src/lib.rs:18: a Rust block in this doc comment"
expect "a Rust block in a module compiled only in tests fails" 1 "src/oracle.rs:1: a Rust block in this doc comment"

base_tree "$T"
printf '%s\n' '' '/// ```' '/// let x = 1;' '/// ```' 'pub fn shown() {}' '#[cfg(unix)]' 'mod tree;' >>"$T/src/lib.rs"
printf '%s\n' '/// ```rust' '/// let x = 1;' '/// ```' 'pub fn kill() {}' >"$T/src/tree.rs"
printf '%s\n' '//! <!-- ignore: a test crate is not doctested -->' '//! ```rust,ignore' '//! mod common;' '//! ```' \
  '//! ```text' '//! a/b' '//! ```' >"$T/tests/helpers.rs"
run_check "$T"
expect "library doc blocks, a Linux-only module's included, and tests/ ignore and text blocks pass" 0 "No dated live fact"

# An inline item or module under a cfg the doctest run leaves out is never
# doctested either, its doc comments above or below the attribute
# included; the code after it is again.
base_tree "$T"
printf '%s\n' '' '/// ```' '/// let x = 1;' '/// ```' '#[cfg(windows)]' 'pub fn kill() {}' \
  '/// ```' '/// let y = 2;' '/// ```' 'pub fn after() {}' >>"$T/src/lib.rs"
run_check "$T"
expect "a Rust block on an inline cfg(windows) fn fails" 1 "src/lib.rs:14: a Rust block in this doc comment never compiles"
expect "only the gated item's block fails" 1 "1 finding(s)"

base_tree "$T"
printf '%s\n' '' '#[cfg(all(test, unix))]' 'mod helpers {' '    /// Builds a store.' '    ///' \
  '    /// ```rust' '    /// let x = 1;' '    /// ```' '    pub fn build() {}' '}' \
  '/// ```' '/// let y = 2;' '/// ```' 'pub fn after() {}' >>"$T/src/lib.rs"
run_check "$T"
expect "a Rust block in an inline cfg(all(test, unix)) module fails" 1 "src/lib.rs:18: a Rust block in this doc comment never compiles"
expect "the code after the gated module is doctested again" 1 "1 finding(s)"

base_tree "$T"
printf '%s\n' '' '#[cfg(all(' '    unix,' '    not(target_os = "linux"),' '))]' '/// ```' '/// let x = 1;' '/// ```' \
  'pub fn bsd() {}' '/// ```' '/// let y = 2;' '/// ```' '#[cfg(any(target_os = "linux", not(windows)))]' \
  'pub fn linux() {}' >>"$T/src/lib.rs"
run_check "$T"
expect "a multi-line cfg the doctest run leaves out gates its item" 1 "src/lib.rs:18: a Rust block in this doc comment never compiles"
expect "a cfg that holds does not" 1 "1 finding(s)"

base_tree "$T"
printf '%s\n' '' '```rust' 'let x = 1;' '```' >>"$T/docs/GUIDE.md"
printf '%s\n' '' '#[cfg(doctest)]' 'mod doc_tests {' '    #[cfg(windows)]' '    mod windows_only {' \
  '        #[doc = include_str!("../docs/GUIDE.md")]' '        mod guide {}' '    }' '}' >>"$T/src/lib.rs"
run_check "$T"
expect "an include inside a gated inline module doctests nothing" 1 "docs/GUIDE.md:15: a Rust block here never compiles"

# A cfg name the check does not know fails, rather than reading as false
# (which `not(..)` would turn into true), in the library's non-test code;
# test code and tests/ are not evaluated.
base_tree "$T"
printf '%s\n' '' '#[cfg(not(target_arch = "x86_64"))]' 'pub fn other() {}' '#[cfg(target_pointer_width = "64")]' \
  'mod wide;' '#[cfg(sanitize)]' 'pub fn sanitized() {}' >>"$T/src/lib.rs"
printf '%s\n' '#[cfg(target_arch = "x86_64")]' 'fn helper() {}' >"$T/src/lib_tests.rs"
printf '%s\n' '#![cfg(target_arch = "x86_64")]' '#[test] fn t() {}' >"$T/tests/live.rs"
run_check "$T"
expect "an unknown cfg key fails, under not() too" 1 "src/lib.rs:14: this check cannot evaluate \`cfg(not(target_arch = \"x86_64\"))\` (it does not know \`target_arch\`)"
expect "an unknown cfg on a file module fails" 1 "src/lib.rs:16: this check cannot evaluate \`cfg(target_pointer_width = \"64\")\`"
expect "an unknown cfg name fails" 1 "src/lib.rs:18: this check cannot evaluate \`cfg(sanitize)\` (it does not know \`sanitize\`)"
expect "test code and tests/ are not evaluated" 1 "3 finding(s)"

# Nor is a cfg inside an item the doctest run already leaves out: an
# inline test module's, on a test or a file module, or a windows-only
# module's.
base_tree "$T"
printf '%s\n' '' '#[cfg(test)]' 'mod more_tests {' '    #[cfg(target_arch = "x86_64")]' '    #[test]' \
  '    fn only_x86() {}' '    #[cfg(target_arch = "x86_64")]' '    mod x86;' '}' '#[cfg(windows)]' \
  'mod win {' '    #[cfg(target_arch = "x86_64")]' '    pub fn f() {}' '}' >>"$T/src/lib.rs"
run_check "$T"
expect "a cfg inside a gated item is not evaluated" 0 "No dated live fact"

base_tree "$T"
printf '%s\n' '' '#[cfg(doctest)]' 'mod doc_tests {' '    #[cfg(target_env = "gnu")]' \
  '    #[doc = include_str!("../docs/GUIDE.md")]' '    mod guide {}' '}' >>"$T/src/lib.rs"
run_check "$T"
expect "an unknown cfg on an include fails" 1 "src/lib.rs:16: this check cannot evaluate \`cfg(target_env = \"gnu\")\`"

base_tree "$T"
mkdir -p "$T/src/http"
printf '%s\n' '#![cfg(windows)]' '/// ```' '/// let x = 1;' '/// ```' 'pub fn win() {}' >"$T/src/http/mod.rs"
run_check "$T"
expect "a file whose inner cfg the doctest run leaves out is not doctested" 1 "src/http/mod.rs:2: a Rust block in this doc comment never compiles"

# Rules 1 to 4 read every doc-comment form: `/** */`, `/*! */`, and
# `#[doc = "..."]` and `#![doc = "..."]`, `\n` escapes and raw strings
# included; a string or a plain comment that looks like one is not read.
base_tree "$T"
printf '%s\n' '' '/**' ' * Reads a store.' ' *' ' * ```ignore' ' * x' ' * ```' ' */' 'pub fn block() {}' \
  '/** Seen live 2026-11-05. */' 'pub fn dated() {}' >>"$T/src/lib.rs"
printf '%s\n' '/*!' 'Helpers.' '' '```rust' 'let x = 1;' '```' '*/' >"$T/tests/helpers.rs"
run_check "$T"
expect "an ignore fence in a /** */ comment needs a reason" 1 "src/lib.rs:17: an ignore fence needs a"
expect "a date in a /** */ comment fails" 1 "src/lib.rs:22: dated live fact (2026-11-05) in a doc comment"
expect "a Rust block in a /*! */ comment of a tests/ helper fails" 1 "tests/helpers.rs:4: a Rust block in this doc comment never compiles"

base_tree "$T"
printf '%s\n' '' '#[doc = "```ignore"]' '#[doc = "x"]' '#[doc = "```"]' 'pub fn attr() {}' \
  '#[doc = "Reads a store.\n\n```text\nlet x = y;\n```"]' 'pub fn escaped() {}' \
  '#[doc = r#"Stored (live 2026-11-01)."#]' 'pub fn raw() {}' >>"$T/src/lib.rs"
printf '%s\n' '#![doc = "```rust"]' '#![doc = "let x = 1;"]' '#![doc = "```"]' >"$T/tests/helpers.rs"
run_check "$T"
expect "an ignore fence in #[doc] text needs a reason" 1 "src/lib.rs:14: an ignore fence needs a"
expect "Rust in a text fence behind \\n escapes fails" 1 "src/lib.rs:18: Rust in a text block never compiles"
expect "a date in a raw #[doc] string fails" 1 "src/lib.rs:20: dated live fact (2026-11-01) in a doc comment"
expect "a Rust block in #![doc] text of a tests/ helper fails" 1 "tests/helpers.rs:1: a Rust block in this doc comment never compiles"

base_tree "$T"
printf '%s\n' '' '#[doc = "<!-- ignore: a fragment -->"]' '#[doc = "```ignore"]' '#[doc = "x"]' '#[doc = "```"]' \
  'pub fn reasoned() {}' 'pub fn strings() {' '    let _ = "/** ```ignore */";' \
  '    let _ = "#[doc = \"```ignore\"]";' '    let _ = r#"' '/// Seen live 2026-11-05.' '"#;' \
  '    /* /** ```ignore */ */' '    let _ = '"'"'"'"'"';' '}' '/*** 2026-11-05 ***/' >>"$T/src/lib.rs"
run_check "$T"
expect "a reasoned #[doc] ignore passes; literals and plain comments are not doc text" 0 "No dated live fact"

# Rule 4: Rust in a text block.
base_tree "$T"
printf '%s\n' '' '```text' 'let mut response = InteractionResponse::default();' \
  'response.status = InteractionStatus::Completed;' '```' >>"$T/docs/GUIDE.md"
run_check "$T"
expect "a let statement in a text fence fails, naming its line" 1 "docs/GUIDE.md:16: Rust in a text block never compiles"

base_tree "$T"
printf '%s\n' '' '```text' 'response.status = InteractionStatus::Completed;' '```' >>"$T/docs/GUIDE.md"
printf '%s\n' '' '```' 'use genai_rs::Client;' '```' >"$T/README.md"
run_check "$T"
expect "a field assignment in a text fence fails" 1 "docs/GUIDE.md:16: Rust in a text block"
expect "an untagged fence in an undoctested file is read as text" 1 "README.md:3: Rust in a text block"

base_tree "$T"
printf '%s\n' '' '/// ```text' '/// #[derive(Debug)]' '/// ```' 'pub fn shown() {}' >>"$T/src/lib.rs"
run_check "$T"
expect "Rust in a rustdoc text fence fails" 1 "src/lib.rs:15: Rust in a text block"

# Statements that span lines or bind a pattern, and headers with a body.
for body in $'let response = client\n    .interaction()\n    .create()\n    .await?;' \
  'let (text, usage) = split(response);' \
  'let Some(text) = response.text() else { return };' \
  $'match chunk {\n    StreamChunk::Completed { .. } => done(),\n}' \
  $'if let Some(text) = response.text() {\n    println!("{text}");\n}'; do
  base_tree "$T"
  printf '%s\n' '' '```text' "$body" '```' >>"$T/docs/GUIDE.md"
  run_check "$T"
  expect "a text fence holding '${body%%$'\n'*}' fails" 1 "docs/GUIDE.md:16: Rust in a text block never compiles"
done

# A bare expression: a line that ends in `?;` or `.await`.
for line in 'client.interaction().with_text("x").create().await?;' 'agent.chat("hi").await' \
  'stream.next().await;' 'serde_json::to_string(&request)?;'; do
  base_tree "$T"
  printf '%s\n' '' '```text' "$line" '```' >>"$T/docs/GUIDE.md"
  run_check "$T"
  expect "a text fence holding '$line' fails" 1 "docs/GUIDE.md:16: Rust in a text block never compiles"
done

base_tree "$T"
printf '%s\n' '' '```text' 'src/' '  mod.rs        Re-exports the public API' '  agent.rs      fn-free notes' \
  'with_content() set? -> Yes' 'Harness up. conversation_id=Some("...")' 'events => created, delta;' \
  'let it settle; then read' 'match: 3 of 4 {' '```' >>"$T/docs/GUIDE.md"
run_check "$T"
expect "a file tree and program output in a text fence pass" 0 "No dated live fact"

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
expect "the checkout follows every rule" 0 "No dated live fact outside its home"

echo
if [ "$failures" -gt 0 ]; then
  echo "$failures of $cases check(s) failed."
  exit 1
fi
echo "All $cases checks passed."
