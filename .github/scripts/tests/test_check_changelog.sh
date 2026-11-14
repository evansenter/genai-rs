#!/usr/bin/env bash
# Fixture tests for check_changelog.sh.
#
# Run directly: .github/scripts/tests/test_check_changelog.sh
#
# Every rule is pinned in both directions — it fires on its defect, at the
# right line, and stays quiet on the look-alike — since a lint that cannot
# fail is the same defect one level up.
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
CHECK="$script_dir/../check_changelog.sh"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

failures=0

fail() {
    echo "FAIL: $*" >&2
    failures=$((failures + 1))
}

# expect <label> <exit> <expected stdout> <file> [version]
# Output is compared whole, so a violation reported at the wrong line, or an
# extra one, fails as surely as a missing one.
expect() {
    local label=$1 want_rc=$2 want_out=$3
    shift 3
    local out rc=0
    out=$("$CHECK" "$@" 2>/dev/null) || rc=$?
    if [ "$rc" -ne "$want_rc" ]; then
        fail "$label: exit $rc, expected $want_rc
  output: [$out]"
    fi
    if [ "$out" != "$want_out" ]; then
        fail "$label: output mismatch
  expected: [$want_out]
  actual:   [$out]"
    fi
}

# --- clean ------------------------------------------------------------------

cat >"$work/clean.md" <<'EOF'
# Changelog

## [Unreleased]

### Added

- a

### Fixed

- b

## [0.2.0] - 2026-07-01

### Added

- c
EOF
expect "clean file" 0 "" "$work/clean.md"
expect "clean file, with version" 0 "" "$work/clean.md" 0.2.0

# --- rule 1: duplicate ### within a section ---------------------------------

cat >"$work/dup_unreleased.md" <<'EOF'
# Changelog

## [Unreleased]

### Added

- a

### Fixed

- b

### Added

- c
EOF
expect "duplicate in [Unreleased]" 1 \
    "$work/dup_unreleased.md:13: duplicate \"### Added\" in ## [Unreleased] (first at line 5)" \
    "$work/dup_unreleased.md"

# Trailing whitespace is not a different heading.
printf '# Changelog\n\n## [Unreleased]\n\n### Added\n\n- a\n\n### Added  \n\n- b\n' \
    >"$work/dup_trailing_space.md"
expect "duplicate differing only in trailing space" 1 \
    "$work/dup_trailing_space.md:9: duplicate \"### Added\" in ## [Unreleased] (first at line 5)" \
    "$work/dup_trailing_space.md"

# The same heading in two *different* sections is the normal shape.
cat >"$work/same_heading_two_sections.md" <<'EOF'
# Changelog

## [Unreleased]

### Added

- a

## [0.2.0] - 2026-07-01

### Added

- b
EOF
expect "same heading across sections" 0 "" "$work/same_heading_two_sections.md" 0.2.0

cat >"$work/dup_versioned.md" <<'EOF'
# Changelog

## [0.10.0] - 2026-08-16

### Fixed

- a

### Fixed

- b
EOF
expect "duplicate in the named version" 1 \
    "$work/dup_versioned.md:9: duplicate \"### Fixed\" in ## [0.10.0] (first at line 5)" \
    "$work/dup_versioned.md" 0.10.0
# Older, already-published sections are out of scope: without the version,
# or with a version that is only a prefix of it, nothing is checked.
expect "duplicate in an unnamed older section" 0 "" "$work/dup_versioned.md"
expect "version 0.1.0 does not select [0.10.0]" 0 "" "$work/dup_versioned.md" 0.1.0

# --- rule 2: conflict markers -----------------------------------------------

cat >"$work/conflict.md" <<'EOF'
# Changelog

## [Unreleased]

### Added

<<<<<<< HEAD
- ours
=======
- theirs
>>>>>>> branch
EOF
expect "conflict markers" 1 \
    "$work/conflict.md:7: merge-conflict marker
$work/conflict.md:9: merge-conflict marker
$work/conflict.md:11: merge-conflict marker" \
    "$work/conflict.md"

# --- rule 3: blank line before headings -------------------------------------

cat >"$work/no_blank.md" <<'EOF'
# Changelog

## [Unreleased]

### Added

- a
### Fixed

- b
## [0.2.0] - 2026-07-01
EOF
expect "headings with no blank line above" 1 \
    "$work/no_blank.md:8: heading not preceded by a blank line: ### Fixed
$work/no_blank.md:11: heading not preceded by a blank line: ## [0.2.0] - 2026-07-01" \
    "$work/no_blank.md"

# --- fences -----------------------------------------------------------------

# Inside a fence, heading-, marker- and duplicate-shaped lines are content.
# Unindented on purpose: every rule is anchored at column 0, so an indented
# fence (inside a list item, say) would pass with the fence skip deleted.
cat >"$work/fenced.md" <<'EOF'
# Changelog

## [Unreleased]

### Added

- a migration example follows.

```text
### Added
<<<<<<< HEAD
=======
## not a heading
```

- b
EOF
expect "structure inside a fence is ignored" 0 "" "$work/fenced.md"

# --- rule 4: duplicate section headings ------------------------------------

# A keep-both merge that re-adds `## [Unreleased]`. Each copy alone is clean,
# so only the section-level rule can see it.
cat >"$work/dup_section.md" <<'EOF'
# Changelog

## [Unreleased]

### Added

- a

## [Unreleased]

### Added

- b

## [0.2.0] - 2026-07-01

- c

## [0.2.0] - 2026-07-02

- d
EOF
expect "duplicate section headings, including released ones" 1 \
    "$work/dup_section.md:9: duplicate section \"## [Unreleased]\" (first at line 3)
$work/dup_section.md:19: duplicate section \"## [0.2.0]\" (first at line 15)" \
    "$work/dup_section.md"

# --- unclosed fence ---------------------------------------------------------

# A merge that drops one side of a fence would otherwise switch every rule
# off for the rest of the file, conflict markers included, and exit 0.
cat >"$work/unclosed_fence.md" <<'EOF'
# Changelog

## [Unreleased]

```text
example

<<<<<<< HEAD
- ours
EOF
expect "unclosed fence" 1 \
    "$work/unclosed_fence.md:5: unclosed code fence; nothing after it was checked" \
    "$work/unclosed_fence.md"

# --- rule 5: a breaking entry has a migration -------------------------------

# A breaking entry needs its own `Migration:`, on its first line or a later
# one; the next entry's does not count, nor does a paragraph's after it.
# Older sections are checked only when named, as rule 1 is.
cat >"$work/migration.md" <<'EOF'
# Changelog

## [Unreleased]

### Changed

- **Breaking: `a` is renamed `b`.** Migration: rename it.
- **Breaking: `c` takes a `&str`.** A `String` needs a borrow.
- **Breaking: `d` is gone.** Nothing replaces it:
  it never worked.

  Migration: drop the call.
- Not breaking. Migration: none.

- **Breaking: `e` is boxed.**

Migration: dereference it.

### Fixed

- **Breaking: `f` is fixed.**
  ```text
  Migration: inside a fence
  ```

## [0.2.0] - 2026-07-01

### Changed

- **Breaking: `g` moved.** Callers must follow.
EOF
expect "breaking entries with no migration of their own" 1 \
    "$work/migration.md:8: breaking entry with no \"Migration:\": - **Breaking: \`c\` takes a \`&str\`.** A \`String\` needs a borrow.
$work/migration.md:15: breaking entry with no \"Migration:\": - **Breaking: \`e\` is boxed.**
$work/migration.md:21: breaking entry with no \"Migration:\": - **Breaking: \`f\` is fixed.**" \
    "$work/migration.md"
expect "breaking entries in a named older section" 1 \
    "$work/migration.md:8: breaking entry with no \"Migration:\": - **Breaking: \`c\` takes a \`&str\`.** A \`String\` needs a borrow.
$work/migration.md:15: breaking entry with no \"Migration:\": - **Breaking: \`e\` is boxed.**
$work/migration.md:21: breaking entry with no \"Migration:\": - **Breaking: \`f\` is fixed.**
$work/migration.md:30: breaking entry with no \"Migration:\": - **Breaking: \`g\` moved.** Callers must follow." \
    "$work/migration.md" 0.2.0

# The last entry in the file is checked too.
printf '# Changelog\n\n## [Unreleased]\n\n### Changed\n\n- **Breaking: `h`.**\n' >"$work/migration_last.md"
expect "a breaking entry at the end of the file" 1 \
    "$work/migration_last.md:7: breaking entry with no \"Migration:\": - **Breaking: \`h\`.**" \
    "$work/migration_last.md"

# --- rule 6: no entry twice in a section ------------------------------------

# A keep-both merge that puts an entry's old text back beside its rewrite:
# the same first line under another heading, and the same bold title with
# a different body, wrapped, with its `Breaking:` marker dropped. A pair
# that matches both ways is reported once. Sub-items, bare markers and a
# fence are not entries; [0.2.0] repeats [Unreleased] only across
# sections, and is checked only when named.
cat >"$work/dup_entries.md" <<'EOF'
# Changelog

## [Unreleased]

### Changed

- **Breaking: `Delta` carries `extra`.** Migration: read it.
- `wait()` polls at most every 10 ms.
  - a sub-item
- **Breaking:** one change. Migration: one.
- **Breaking: `a`
  is a struct.** Migration: match its fields.
- **Same twice.** Body.

### Fixed

- `wait()` polls at most every 10 ms.
  - a sub-item
- **Breaking:** another change. Migration: another.
- **`Delta` carries
  `extra`.** It still carries the `StepDelta` alone.
- **`a` is a struct.** The old text.
- **Same twice.** Body.

```text
- **Same twice.** Body.
```

## [0.2.0] - 2026-07-01

### Changed

- `wait()` polls at most every 10 ms.
- **`a` is a struct.** Released.
- **`a` is a struct.** Released twice.
EOF
unreleased_dups="$work/dup_entries.md:17: duplicate entry in ## [Unreleased] (first at line 8): - \`wait()\` polls at most every 10 ms.
$work/dup_entries.md:20: two entries open alike in ## [Unreleased] (first at line 7): \"\`Delta\` carries \`extra\`.\"
$work/dup_entries.md:22: two entries open alike in ## [Unreleased] (first at line 11): \"\`a\` is a struct.\"
$work/dup_entries.md:23: duplicate entry in ## [Unreleased] (first at line 13): - **Same twice.** Body."
expect "duplicate entries in [Unreleased]" 1 "$unreleased_dups" "$work/dup_entries.md"
expect "duplicate entries in a named older section" 1 \
    "$unreleased_dups
$work/dup_entries.md:35: two entries open alike in ## [0.2.0] (first at line 34): \"\`a\` is a struct.\"" \
    "$work/dup_entries.md" 0.2.0

# The keep-both shapes rule 6 once let through (#504, K-13), each a real
# pair from this CHANGELOG with its body trimmed, beside a rewrite: two
# Breaking entries a rewrite retitled, keeping the subject and most of the
# words (Changed); three plain Fixed entries promoted to Breaking, whose
# first sentence is matched with the new title (one reorders its words,
# and its rewrite's title runs to a second sentence); a pair that differs
# in case alone, twice, the second where no dropped one-letter word can
# hide it; and an entry whose bold is only the marker, beside its plain
# copy.
cat >"$work/keep_both.md" <<'EOF'
# Changelog

## [Unreleased]

### Changed

- **Breaking: `GenerationConfig` has two new public fields, `extra` and
  `speech_config_extra`.** `speech_config_extra` keeps the keys a
  `{"speakers": [...]}` form of `speech_config` carries beside the list.
  Migration: a struct literal needs `extra: Default::default()` and
  `speech_config_extra: Default::default()`, or `..Default::default()`.
- **Breaking: `AntigravityError::InitFailed` and `HandshakeFailed` have a
  `source` field, no Antigravity error repeats its source's text in
  `Display`, and every failure of `spawn()` after the handshake is
  `InitFailed`.** Migration: match `InitFailed { message, stderr, .. }`.
- **Breaking: `GenerationConfig` has a new public field, `extra`.**
  Migration: a struct literal needs `extra: Default::default()`, or
  `..Default::default()`.
- **Breaking: `AntigravityError::InitFailed` has a `source` field, and no
  Antigravity error repeats its source's text in `Display`.** A WebSocket
  connect that fails during `spawn()` is `InitFailed`. Migration: match
  `InitFailed { message, stderr, .. }`.

### Fixed

- **Breaking: no message to the Antigravity harness can wait forever. A
  failed initialization send fails `spawn()` with `InitFailed`, not
  `WebSocket`, and a send in a turn with no timeout gives up with
  `Timeout`.** Migration: match `InitFailed { .. }` where you matched a
  `WebSocket` error from `spawn()`.
- **Breaking: an Antigravity harness that dies mid-turn, or while
  `next_trigger_turn` waits, fails the call with
  `AntigravityError::ConnectionClosed`, not `WebSocket`.** Migration:
  match `ConnectionClosed { message, stderr }`.
- **Breaking: an Antigravity turn whose model call failed after the agent
  delegated to a subagent fails with `AntigravityError::Turn`, not `Ok`.**
  Migration: handle `Err(AntigravityError::Turn { kind, .. })`.
- An Antigravity turn whose model call failed after the agent delegated to
  a subagent (a 503 with no retries left) fails with
  `AntigravityError::Turn`, as one without a subagent does. Harness 0.1.23
  reports that error before the turn's final state.
- Antigravity: no message to the harness can wait forever. `chat()` and
  `send_streaming()` start the turn deadline before sending the message.
- Antigravity: a harness that dies mid-turn, or while `next_trigger_turn`
  waits, fails the call with `AntigravityError::ConnectionClosed` and the
  tail of the harness's stderr, as that variant says. A connection that
  ended without a close frame read as a `WebSocket` error with no stderr.
- **A bold one.** Its old text.
- **Breaking: a bold one.** Migration: none.
- **Streaming keeps media deltas apart.** The old text.
- **Breaking: streaming keeps media deltas apart.** Migration: none.
- **Breaking:** `Role` is a plain enum. Migration: drop its serde.
- `Role` is a plain enum. It was not.
EOF
expect "keep-both copies of a retitled or newly bold entry" 1 \
    "$work/keep_both.md:16: two entries open alike in ## [Unreleased] (first at line 7): \"\`GenerationConfig\` has a new public field, \`extra\`.\"
$work/keep_both.md:19: two entries open alike in ## [Unreleased] (first at line 12): \"\`AntigravityError::InitFailed\` has a \`source\` field, and no Antigravity error repeats its source's text in \`Display\`.\"
$work/keep_both.md:38: two entries open alike in ## [Unreleased] (first at line 35): \"An Antigravity turn whose model call failed after the agent delegated to a subagent (a 503 with no retries left) fails with \`AntigravityError::Turn\`, as one without a subagent does.\"
$work/keep_both.md:42: two entries open alike in ## [Unreleased] (first at line 26): \"Antigravity: no message to the harness can wait forever.\"
$work/keep_both.md:44: two entries open alike in ## [Unreleased] (first at line 31): \"Antigravity: a harness that dies mid-turn, or while \`next_trigger_turn\` waits, fails the call with \`AntigravityError::ConnectionClosed\` and the tail of the harness's stderr, as that variant says.\"
$work/keep_both.md:49: two entries open alike in ## [Unreleased] (first at line 48): \"a bold one.\"
$work/keep_both.md:51: two entries open alike in ## [Unreleased] (first at line 50): \"streaming keeps media deltas apart.\"
$work/keep_both.md:53: two entries open alike in ## [Unreleased] (first at line 52): \"\`Role\` is a plain enum.\"" \
    "$work/keep_both.md"

# Look-alikes that are different entries, each a shape from this
# CHANGELOG: the same sentence about different code (the first code span
# differs); two that share most words, but not 85% of the shorter's; two
# short openings whose words a longer one holds, with many more of its own
# (under 45% of both); an opening of two words beside one that holds both;
# and an opening that its sub-item, which is not part of it, would make a
# copy of another.
cat >"$work/look_alikes.md" <<'EOF'
# Changelog

## [Unreleased]

### Added

- `StepDelta` keeps every unmodeled field a delta carries in `extra`.
- `Annotation` keeps every unmodeled field a delta carries in `extra`.
- Three more antigravity worked examples.
- Two antigravity worked examples.

### Fixed

- `Debug` no longer prints the secrets a value holds. Each prints
  `[REDACTED]` in the secret's place.
- The `Debug` output of the Antigravity types that hold a secret no longer
  prints it: the header and environment variable values print as
  `"[REDACTED]"`, their names kept.
- `wait_until_active` (files and file-search documents):
  - It honors the `Retry-After` of a 429 it polls through.
- `wait_until_active` (files and file-search documents) never runs past its
  deadline: each status request is cut off at the deadline, and a poll
  that would start past it is not made.
- Lone words.
- Lone words, and many more beside them.
- `Files` changed:
  - uploads retry a 503 and resume where they stopped.
- `Files` uploads retry a 503 and resume where they stopped.
EOF
expect "look-alike entries that differ" 0 "" "$work/look_alikes.md"

# A title left open by the end of its entry is abandoned, not carried into
# the next one, and bold text after an entry's start is no title.
cat >"$work/open_title.md" <<'EOF'
# Changelog

## [Unreleased]

### Added

- **An unclosed title

- **An unclosed title** closed here.
- text **An unclosed title** later.
EOF
expect "an unclosed bold title" 0 "" "$work/open_title.md"

# --- usage ------------------------------------------------------------------

expect "no arguments" 2 ""
expect "too many arguments" 2 "" "$work/clean.md" 0.2.0 extra
expect "missing file" 2 "" "$work/does-not-exist.md"

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)" >&2
    exit 1
fi
echo "All checks passed."
