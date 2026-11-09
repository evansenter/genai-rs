#!/usr/bin/env bash
# Structural lint for a Keep a Changelog file (#458).
#
# Usage: check_changelog.sh <changelog.md> [version]
#
# `## [Unreleased]` is a merge serialization point: parallel PRs all insert
# at the same anchor, and "keep both" preserves every line but not the
# heading it was written under. The result ships verbatim as the release
# body (extract_changelog_section.sh), so these rules run before tag time:
#
#   1. No `###` heading appears twice within one `## [...]` section.
#      Checked for `[Unreleased]` and, if given, `[version]` only: older
#      sections predate this lint and their release bodies are already
#      published, so rewriting them would change nothing a reader saw.
#   2. No merge-conflict markers, anywhere in the file.
#   3. Every `##`/`###` heading after the first line is preceded by a blank
#      line — the artifact a hand-relocated section leaves behind.
#   4. No `## [...]` heading appears twice, anywhere. A keep-both merge can
#      produce two `## [Unreleased]`; rule 1 would then check each copy
#      separately, and extract_changelog_section.sh ships only one of them.
#   5. Every breaking entry (a list item that starts `- **Breaking`) says
#      what to do about it in a `Migration:` of its own (CLAUDE.md, D-007).
#      An entry runs from its `- ` to the next line that is neither blank
#      nor indented. Checked where rule 1 is.
#
# Fenced code blocks are skipped for all five. An unclosed fence is itself
# reported: otherwise everything after it would go unchecked and pass.
#
# Output: one `<file>:<line>: <problem>` per violation, on stdout.
# Exit codes:
#   0  clean
#   1  at least one violation
#   2  a caller or environment error: wrong arguments, or no such file
#
# No `::error::` prefix: each caller picks the severity.
set -euo pipefail

if [ $# -lt 1 ] || [ $# -gt 2 ]; then
    echo "usage: $0 <changelog.md> [version]" >&2
    exit 2
fi

changelog=$1
version=${2:-}

if [ ! -f "$changelog" ]; then
    echo "changelog not found: $changelog" >&2
    exit 2
fi

awk -v file="$changelog" -v want="## [$version]" -v have_version="${version:+1}" '
    function report(msg) { printf "%s:%d: %s\n", file, NR, msg; bad = 1 }

    # Rule 5: the breaking entry that just ended has its migration.
    function finish_entry() {
        if (breaking && !migration) {
            printf "%s:%d: breaking entry with no \"Migration:\": %s\n", file, entry_line, entry_head
            bad = 1
        }
        breaking = 0
        migration = 0
    }

    # Fences toggle on their own line; nothing inside one is structure.
    /^[[:space:]]*```/ {
        fenced = !fenced
        if (fenced) { fence_line = NR }
        prev = $0
        next
    }
    fenced { prev = $0; next }

    /^(<<<<<<< |<<<<<<<$|=======$|>>>>>>> |>>>>>>>$)/ {
        report("merge-conflict marker")
    }

    # A line that is neither blank nor indented ends a list entry; one that
    # starts a breaking entry where rule 1 checks begins the next.
    /^[^[:space:]]/ {
        finish_entry()
        if (checked && /^- \*\*Breaking/) {
            breaking = 1
            entry_line = NR
            entry_head = $0
        }
    }
    breaking && /Migration:/ { migration = 1 }

    /^###? / {
        if (NR > 1 && prev !~ /^[[:space:]]*$/) {
            report("heading not preceded by a blank line: " $0)
        }
    }

    /^## / {
        # Matched up to the closing bracket, as extract_changelog_section.sh
        # does, so a date suffix is ignored and `0.1` cannot match `[0.10.0]`.
        close_bracket = index($0, "]")
        head = close_bracket > 0 ? substr($0, 1, close_bracket) : $0
        checked = (head == "## [Unreleased]") || (have_version && head == want)
        section = head
        if (head in seen_section) {
            report("duplicate section \"" head "\" (first at line " seen_section[head] ")")
        } else {
            seen_section[head] = NR
        }
        delete seen
    }

    checked && /^### / {
        heading = $0
        sub(/[[:space:]]+$/, "", heading)
        if (heading in seen) {
            report("duplicate \"" heading "\" in " section " (first at line " seen[heading] ")")
        } else {
            seen[heading] = NR
        }
    }

    { prev = $0 }

    END {
        finish_entry()
        if (fenced) {
            printf "%s:%d: unclosed code fence; nothing after it was checked\n", file, fence_line
            bad = 1
        }
        exit bad ? 1 : 0
    }
' "$changelog"
