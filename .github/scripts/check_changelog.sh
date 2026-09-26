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
#
# Fenced code blocks are skipped for all three.
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

    # Fences toggle on their own line; nothing inside one is structure.
    /^[[:space:]]*```/ { fenced = !fenced; prev = $0; next }
    fenced { prev = $0; next }

    /^(<<<<<<< |<<<<<<<$|=======$|>>>>>>> |>>>>>>>$)/ {
        report("merge-conflict marker")
    }

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

    END { exit bad ? 1 : 0 }
' "$changelog"
