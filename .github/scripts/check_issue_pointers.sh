#!/usr/bin/env bash
# Flags issue pointers that name a closed issue (D-051), for the daily
# api-surface-sweep.
#
# Usage: .github/scripts/check_issue_pointers.sh [--print] [ROOT]
#
# D-051 tells a pointer from a provenance tag by what the sentence claims,
# which no script can read. This check is narrower: it finds the pointer
# phrasings CLAUDE.md lists, "tracked in #N" (or by, on, at), "see #N for"
# and "until #N", which D-051 requires a pointer to use, in ROOT's tracked
# files (default: the current directory, a git work tree), on one line or
# wrapped across two, and looks each N up with `gh`. DECISIONS.md is read:
# a decision's pointer goes stale like any other. Not read: CHANGELOG.md
# (a released section is not edited, so it holds no pointer), fixtures
# (any `fixtures/` directory) and the script harnesses'
# .github/scripts/tests/, whose strings are fixture data. A bare "#N item
# M" is not matched: D-051 uses that form for provenance tags too. A
# pointer whose issue is closed, or that names no issue, opens or updates
# one rolling issue (labeled `documentation`) that lists each; a run that
# finds none closes it. Whether a listed sentence is a stale pointer (state
# the outcome, drop the pointer) or a provenance tag worded like one
# (reword it) stays the writer's call.
#
# The issue is found by its exact title, closed included, so a closed one
# reopens rather than a duplicate being filed. Its body is rewritten on
# every run with a finding; a comment is added only when the findings
# changed (a marker in the body records them).
#
# --print: list every pointer found, with its issue's state, and exit 1 if
# any names a closed issue or none, without touching the rolling issue (a
# local run). Without it, only those are listed.
#
# Exit status: 0 when the check ran, whatever it found (with --print: when
# no pointer names a closed issue); 1 when a `gh` call failed (or, with
# --print, a pointer names a closed issue); 2 on bad usage or when ROOT is
# not a git work tree.
#
# Needs python3, git and gh (GH_TOKEN with issues: write).

set -euo pipefail

usage() {
    awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0" >&2
}

print_only=0
if [ "${1:-}" = --print ]; then
    print_only=1
    shift
fi
if [ $# -gt 1 ] || [ "${1:-}" = -h ] || [ "${1:-}" = --help ]; then
    usage
    exit 2
fi
root=${1:-.}
if ! git -C "$root" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    echo "not a git work tree: $root" >&2
    exit 2
fi

python3 -I - "$root" "$print_only" <<'EOF'
import json
import re
import subprocess
import sys
import time

root, print_only = sys.argv[1], sys.argv[2] == "1"
TITLE = "Issue pointers name closed issues"
LABEL = "documentation"
HISTORY = {"CHANGELOG.md"}
POINTER = re.compile(
    r"\b(?:tracked\s+(?:in|by|on|at)\s+#(\d+)|see\s+#(\d+)\s+for\b|until\s+#(\d+))",
    re.I,
)
# What starts a comment or a quote, dropped from a wrapped line's next one:
# a marker followed by a space, so the `#12` a hard-wrapped Markdown line
# can start with stays.
LEAD = re.compile(r"^\s*(?:(?://[/!]?|#+|\*|>|--)(?=\s|$))?\s*")


def gh(*args):
    """Runs gh; returns its stdout, or exits 1 naming the call."""
    done = subprocess.run(["gh", *args], capture_output=True, text=True)
    if done.returncode != 0:
        print(f"gh {' '.join(args)} failed: {done.stderr.strip()}", file=sys.stderr)
        sys.exit(1)
    return done.stdout


def skipped(path):
    return (
        path in HISTORY
        or path.startswith(".github/scripts/tests/")
        or "/fixtures/" in "/" + path
    )


listed = subprocess.run(
    ["git", "-C", root, "ls-files", "-z"], check=True, capture_output=True
).stdout.decode()
pointers = []  # (path, line number, phrase, issue)
for path in sorted(p for p in listed.split("\0") if p and not skipped(p)):
    try:
        with open(f"{root}/{path}", encoding="utf-8") as f:
            lines = f.read().splitlines()
    except (UnicodeDecodeError, IsADirectoryError, FileNotFoundError):
        continue
    for k, line in enumerate(lines):
        nxt = LEAD.sub("", lines[k + 1]) if k + 1 < len(lines) else ""
        joined = line.rstrip() + " " + nxt
        for m in POINTER.finditer(joined):
            if m.start() >= len(line.rstrip()):
                continue  # the next line's own, found there
            number = int(next(g for g in m.groups() if g))
            pointers.append((path, k + 1, " ".join(m.group(0).split()), number))

states = {}
for number in sorted({p[3] for p in pointers}):
    done = subprocess.run(
        ["gh", "api", f"repos/{{owner}}/{{repo}}/issues/{number}"],
        capture_output=True,
        text=True,
    )
    if done.returncode != 0:
        if "HTTP 404" in done.stderr or "Not Found" in done.stderr:
            states[number] = ("missing", "")
            continue
        print(f"looking up #{number} failed: {done.stderr.strip()}", file=sys.stderr)
        sys.exit(1)
    issue = json.loads(done.stdout)
    states[number] = (issue["state"].lower(), issue.get("title", ""))

stale = [p for p in pointers if states[p[3]][0] != "open"]
print(f"{len(pointers)} pointer(s) to {len(states)} issue(s); {len(stale)} name a closed one.")
for path, n, phrase, number in pointers if print_only else stale:
    state, title = states[number]
    what = "no such issue" if state == "missing" else f"{state}: {title}"
    print(f"{path}:{n}: \"{phrase}\" names #{number} ({what})")
if print_only:
    sys.exit(1 if stale else 0)

# Assigned before any condition reads them, so a failed call exits 1.
issues = json.loads(
    gh("issue", "list", "--label", LABEL, "--state", "all", "--search", f'"{TITLE}" in:title',
       "--limit", "50", "--json", "number,title,state")
)
existing = next((i for i in issues if i["title"] == TITLE), None)

if not stale:
    if existing and existing["state"] == "OPEN":
        body = (
            "No pointer phrasing names a closed issue any more, so this is closed. "
            "It reopens on the next one.\n\n---\n_Generated by [Claude Code](https://claude.ai/code)_\n"
        )
        gh("issue", "comment", str(existing["number"]), "--body", body)
        gh("issue", "close", str(existing["number"]), "--reason", "completed")
        print(f"Closed #{existing['number']}.")
    sys.exit(0)

keys = "\n".join(sorted({f"{p} {phrase} {number}" for p, _, phrase, number in stale}))
marker = f"<!-- issue-pointers: {subprocess.run(['cksum'], input=keys, capture_output=True, text=True, check=True).stdout.split()[0]} -->"
rows = []
for path, n, phrase, number in stale:
    state, title = states[number]
    what = "names no issue" if state == "missing" else f"is closed ({title})"
    rows.append(f"- `{path}:{n}`: \"{phrase}\", and #{number} {what}")
body = "\n".join(
    [
        "## Issue pointers that name a closed issue",
        "",
        "The daily API surface sweep (`.github/scripts/check_issue_pointers.sh`) looks for",
        'the pointer phrasings D-051 lists ("tracked in #N", "see #N for", "until #N")',
        "outside CHANGELOG.md and fixtures, and looks each issue up. "
        f"Checked {time.strftime('%Y-%m-%d', time.gmtime())}:",
        "",
        *rows,
        "",
        "Each is a pointer left behind, or a provenance tag worded like one. For a",
        "pointer, state the outcome in place and drop it; for a provenance tag, reword",
        "it so it does not read as current status (D-051). The check reads wording only,",
        "so which one it is stays your call. The next run that finds none closes this issue.",
        "",
        "---",
        "_Generated by [Claude Code](https://claude.ai/code)_",
        "",
        marker,
        "",
    ]
)
print(body)

if existing is None:
    gh("issue", "create", "--title", TITLE, "--label", LABEL, "--body", body)
    sys.exit(0)
number = str(existing["number"])
previous = json.loads(gh("issue", "view", number, "--json", "body"))["body"]
if existing["state"] != "OPEN":
    gh("issue", "reopen", number)
gh("issue", "edit", number, "--body", body)
if marker in previous:
    print(f"#{number} already lists these pointers; body refreshed, no comment.")
else:
    gh(
        "issue", "comment", number, "--body",
        "Updated: pointers to closed issues changed. The issue body above lists the "
        "current ones.\n\n---\n_Generated by [Claude Code](https://claude.ai/code)_\n",
    )
    print(f"Updated #{number}.")
EOF
