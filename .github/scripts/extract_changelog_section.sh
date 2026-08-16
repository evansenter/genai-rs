#!/usr/bin/env bash
# Print the body of one version's section from a Keep a Changelog file.
#
# Usage: extract_changelog_section.sh <changelog.md> <version>
#
# `version` is bare (0.10.0), not tag-shaped (v0.10.0) — the caller strips
# the prefix, because the tag and the heading do not have to agree on it.
#
# Known limitation: a line starting `## ` ends the section wherever it
# appears, including inside a fenced code block. A future entry quoting
# markdown, or a shell snippet with a comment at column zero, would truncate
# the body silently — the extract still exits 0, so neither caller's fallback
# fires. Pinned by a fixture in the harness so the behaviour is documented
# rather than discovered. Not worth a fence state machine for a Keep a
# Changelog file; if it ever bites, that is the fix.
#
# Exit codes:
#
#   0  the section was found and printed
#   1  the file exists but has no such section — a recoverable case
#   2  a caller or environment error: wrong arguments, or no such file
#
# 1 is reserved strictly for "the file is fine, the section is not there",
# because that is the only case a caller should fall back on. A missing file
# is the same class of problem as a bad argument list, so it shares exit 2 —
# otherwise `release.yml` would annotate a vanished CHANGELOG as
# "no [X.Y.Z] section in CHANGELOG.md", naming a section in a file that does
# not exist and shipping a commit-list body over it.
#
# No `::error::` prefix on any of them: both callers treat the recoverable
# case as recoverable and annotate it themselves, so the script would
# otherwise stamp a red annotation on a run that succeeded by design. The
# script cannot know how its caller weighs a missing section, so it does not
# pick the severity.
set -euo pipefail

if [ $# -ne 2 ]; then
    echo "usage: $0 <changelog.md> <version>" >&2
    exit 2
fi

changelog=$1
version=$2

if [ ! -f "$changelog" ]; then
    echo "changelog not found: $changelog" >&2
    exit 2
fi

# Compared against the heading up to and including the closing bracket, so
# the date suffix cannot affect the match: `## [0.10.0]`,
# `## [0.10.0] - 2026-08-16` and `## [0.10.0] — 2026-08-16` are all the same
# section. Only the bracketed version is load-bearing.
#
# The brackets are what keep `0.1` and `0.1.0` from finding `[0.10.0]` —
# `## [0.1]` is not a prefix of `## [0.10.0]`, because the character after
# `0.1` is `0` and not `]`. A bare-version search would match, which is why
# the delimiter is part of the compared string rather than stripped off.
#
# One awk pass, including the blank-line trimming, rather than piping through
# `tac`: `tac` is GNU coreutils, absent on macOS and BSD, and this script has
# a harness whose whole point is a fast local loop on the machine most likely
# to run it.
section=$(awk -v want="## [$version]" '
    # A heading ends the section we are in, whichever version it names —
    # including an identical one, which would mean a duplicated section.
    /^## / {
        if (collecting) { exit }
        # $0 up to and including the closing bracket, so a trailing date or
        # dash cannot affect the comparison.
        close_bracket = index($0, "]")
        if (close_bracket > 0 && substr($0, 1, close_bracket) == want) {
            collecting = 1
            next
        }
    }
    collecting {
        # Blank lines are held rather than printed: any still pending at EOF
        # (or at the next heading) are the trailing run and are dropped, and
        # none is ever flushed before the first non-blank line, which drops
        # the leading run. A Keep a Changelog section always opens with one,
        # and a release body should not.
        if ($0 ~ /^[[:space:]]*$/) {
            if (started) { pending = pending $0 "\n" }
            next
        }
        if (pending != "") { printf "%s", pending; pending = "" }
        print
        started = 1
    }
' "$changelog")

if [ -z "$section" ]; then
    echo "no [$version] section in $changelog" >&2
    exit 1
fi

printf '%s\n' "$section"
