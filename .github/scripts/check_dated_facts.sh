#!/usr/bin/env bash
# Enforces D-032 (one home per dated live fact) and the ignore-reason line
# on code blocks that do not compile.
#
# Usage: .github/scripts/check_dated_facts.sh [ROOT]
#
# ROOT (default: the current directory) must be a git work tree; its tracked
# and untracked, not ignored, `*.md` and `*.rs` files are read, so a new file
# is checked before it is committed. Two rules:
#
# 1. A date (`2026-11-05`, or a month, `2026-07`) is a dated live fact, and
#    one lives only in its homes: docs/ENUM_WIRE_FORMATS.md,
#    docs/INTERACTIONS_API_GAP.md and docs/ANTIGRAVITY_BRIDGE_DESIGN.md.
#    D-032's exceptions are not read: DECISIONS.md and CHANGELOG.md (history),
#    any file under a `tests/` directory (each test's evidence, fixtures
#    included), and in Rust everything but the comments of non-test code:
#    the doc comments in src/ and genai-rs-macros/src/, where a `//` comment
#    beside the code an observation justifies may carry its date, and every
#    comment in examples/ (examples/CLAUDE.md). Test code is a `*_tests.rs`,
#    `tests.rs`, `proptest_*.rs` or `test_subscriber.rs` file,
#    src/coverage_oracle/, and an item behind a `#[cfg(test)]` line. Not
#    dates: a timestamp (`2026-01-01T10:30:45Z`, example data), a link's
#    target (a home's heading anchor), and the API revision `2026-05-20`, a
#    version name.
# 2. A code block fenced `ignore` (`rust,ignore`) in any Markdown file, or in
#    a doc comment of that non-test code, has a `<!-- ignore: reason -->`
#    line directly above its fence.
#
# Exit status: 0 when the tree follows both rules, 1 with each finding
# listed as `path:line: ...`, 2 on bad usage or when ROOT is not a git work
# tree. Needs python3 and git. `make test-scripts` runs it on the checkout.

set -euo pipefail

if [ $# -gt 1 ]; then
    awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0" >&2
    exit 2
fi
root=${1:-.}
for tool in git python3; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "$tool is required" >&2
        exit 2
    fi
done
if ! git -C "$root" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    echo "not a git work tree: $root" >&2
    exit 2
fi

python3 -I - "$root" <<'EOF'
import os
import re
import subprocess
import sys

root = sys.argv[1]

HOMES = {
    "docs/ENUM_WIRE_FORMATS.md",
    "docs/INTERACTIONS_API_GAP.md",
    "docs/ANTIGRAVITY_BRIDGE_DESIGN.md",
}
HISTORY = {"DECISIONS.md", "CHANGELOG.md"}
# Dates that name something rather than date an observation.
NAMES = {"2026-05-20"}  # the API revision (D-032)
RUST_ROOTS = ("src/", "genai-rs-macros/src/", "examples/")

DATE = re.compile(
    r"(?<![\w.-])20\d\d-(?:0[1-9]|1[0-2])(?:-(?:0[1-9]|[12]\d|3[01]))?(?![\w-]|T\d)"
)
LINK_TARGET = re.compile(r"\]\([^)\s]*\)")
AUTOLINK = re.compile(r"<https?://[^>\s]*>")
BARE_URL = re.compile(r"https?://\S+")
LINK_DEFINITION = re.compile(r"^(\s*\[[^\]]+\]:\s*)\S+")
FENCE = re.compile(r"^\s*(`{3,}|~{3,})\s*([^\s`]*)")
REASON = re.compile(r"^\s*<!--\s*ignore:\s*\S.*-->\s*$")
# A whole-line comment; group 1 is `/` or `!` for a doc comment.
COMMENT = re.compile(r"^\s*//(/(?!/)|!)? ?(.*)$")
TEST_FILE = re.compile(r"(_tests|^tests|^proptest_\w+|^test_subscriber)\.rs$")

findings = []


def dates(text):
    text = LINK_DEFINITION.sub(r"\1", text)
    for pattern in (LINK_TARGET, AUTOLINK, BARE_URL):
        text = pattern.sub("", text)
    return [d for d in DATE.findall(text) if d not in NAMES]


class Fences:
    """Tracks one Markdown text's fences, line by line."""

    def __init__(self):
        self.open = None  # the opening run of backticks or tildes

    def step(self, line):
        """True when `line` opens an `ignore` fence."""
        m = FENCE.match(line)
        if self.open is not None:
            closes = m and m.group(1)[0] == self.open[0] and len(m.group(1)) >= len(self.open)
            if closes and not line.strip().strip(m.group(1)[0]):
                self.open = None
            return False
        if not m:
            return False
        self.open = m.group(1)
        return "ignore" in m.group(2).split(",")


def check_markdown(path, text):
    dated = path not in HOMES and path not in HISTORY
    fences = Fences()
    prev = ""
    for n, line in enumerate(text.splitlines(), 1):
        if fences.step(line) and not REASON.match(prev):
            findings.append(
                f"{path}:{n}: an ignore fence needs a `<!-- ignore: reason -->` line directly above it"
            )
        if dated:
            for d in dates(line):
                findings.append(
                    f"{path}:{n}: dated live fact ({d}) outside its home: record it in "
                    "docs/ENUM_WIRE_FORMATS.md or docs/INTERACTIONS_API_GAP.md (Antigravity: "
                    "docs/ANTIGRAVITY_BRIDGE_DESIGN.md) and state the current behavior here (D-032)"
                )
        prev = line


def test_items(lines):
    """Indexes of the lines that belong to an item behind `#[cfg(test)]`."""
    skip = set()
    i = 0
    while i < len(lines):
        if lines[i].strip() != "#[cfg(test)]":
            i += 1
            continue
        indent = lines[i][: len(lines[i]) - len(lines[i].lstrip())]
        j = i + 1
        while j < len(lines) and lines[j].strip().startswith(("#[", "///", "//")):
            j += 1
        if j >= len(lines):
            break
        end = j
        if lines[j].rstrip().endswith("{"):
            close = indent + "}"
            end = next((k for k in range(j + 1, len(lines)) if lines[k].rstrip() == close), len(lines) - 1)
        skip.update(range(i, end + 1))
        i = end + 1
    return skip


def check_rust(path, text):
    every_comment = path.startswith("examples/")
    lines = text.splitlines()
    skip = test_items(lines)
    fences = Fences()
    prev = None  # the previous doc line's text; None outside a doc block
    for n, line in enumerate(lines, 1):
        m = COMMENT.match(line) if n - 1 not in skip else None
        if not m or not m.group(1):
            prev = None
            fences = Fences()
            if m and every_comment:
                for d in dates(m.group(2)):
                    findings.append(
                        f"{path}:{n}: dated live fact ({d}) in an example: state what the API "
                        "does now and keep the dated observation in a home (D-032)"
                    )
            continue
        doc = m.group(2)
        if fences.step(doc) and not REASON.match(prev or ""):
            findings.append(
                f"{path}:{n}: an ignore fence needs a `<!-- ignore: reason -->` doc line directly above it"
            )
        for d in dates(doc):
            findings.append(
                f"{path}:{n}: dated live fact ({d}) in a doc comment: state the current behavior "
                "without the date, and keep the dated evidence in a home or in a `//` comment "
                "beside the code it justifies (D-032)"
            )
        prev = doc


listed = subprocess.run(
    ["git", "-C", root, "ls-files", "-z", "--cached", "--others", "--exclude-standard",
     "--", "*.md", "*.rs"],
    check=True,
    capture_output=True,
).stdout.decode()
paths = sorted({p for p in listed.split("\0") if p})
checked = 0
for path in paths:
    full = os.path.join(root, path)
    if not os.path.isfile(full) or "/tests/" in "/" + path:
        continue
    with open(full, encoding="utf-8") as f:
        text = f.read()
    if path.endswith(".md"):
        check_markdown(path, text)
        checked += 1
    elif (
        path.startswith(RUST_ROOTS)
        and not path.startswith("src/coverage_oracle/")
        and not TEST_FILE.search(os.path.basename(path))
    ):
        check_rust(path, text)
        checked += 1

for finding in findings:
    print(finding)
if findings:
    print(
        f"\n{len(findings)} finding(s). D-032 (DECISIONS.md) lists the homes and the "
        "exceptions; CONTRIBUTING.md has the ignore-reason convention.",
        file=sys.stderr,
    )
    sys.exit(1)
print(f"No dated live fact outside its home, and every ignore fence has a reason ({checked} files).")
EOF
