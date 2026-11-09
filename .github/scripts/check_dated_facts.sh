#!/usr/bin/env bash
# Enforces D-032 (one home per dated live fact) and keeps every Rust code
# block either compiled or marked with the reason it is not.
#
# Usage: .github/scripts/check_dated_facts.sh [ROOT]
#
# ROOT (default: the current directory) must be a git work tree; its tracked
# and untracked, not ignored, `*.md` and `*.rs` files are read, so a new file
# is checked before it is committed. A doc comment is any of rustdoc's
# forms: `///` and `//!` lines, `/** */` and `/*! */` blocks, and
# `#[doc = "..."]` and `#![doc = "..."]` attributes (a string literal,
# escapes and raw strings read as rustdoc reads them). Four rules:
#
# 1. A date is a dated live fact, and one lives only in its homes:
#    docs/ENUM_WIRE_FORMATS.md, docs/INTERACTIONS_API_GAP.md and
#    docs/ANTIGRAVITY_BRIDGE_DESIGN.md. A date is ISO (`2026-11-05`,
#    `2026-11-5`, or a month, `2026-07`), slashed (`2026/11/05`,
#    `11/05/2026`), dotted (`2026.11.05`, `05.11.2026`), dashed day last
#    (`11-05-2026`), or written with the month named (`November 5, 2026`,
#    `Nov 5, 2026`, `5 November 2026`, `November 2026`), on one line or
#    wrapped across two. D-032's exceptions are not read: DECISIONS.md and
#    CHANGELOG.md (history), any file under a `tests/` directory (each
#    test's evidence, fixtures included), and in Rust everything but the
#    comments of non-test code: the doc comments in src/ and
#    genai-rs-macros/src/, where a `//` comment beside the code an
#    observation justifies may carry its date, and every comment in
#    examples/ (examples/CLAUDE.md). Test code is a `*_tests.rs`,
#    `tests.rs`, `proptest_*.rs` or `test_subscriber.rs` file,
#    src/coverage_oracle/, and an item behind a `#[cfg(test)]` line. Not
#    dates: a timestamp, a date followed by a time with seconds
#    (`2026-01-01T10:30:45Z`, `21 Oct 2015 07:28:00 GMT`: example data); a
#    link's target (a home's heading anchor); and the API revision
#    `2026-05-20`, a version name. Not read: a date without a year
#    (`Nov 5`) or with two digits (`'26`).
# 2. A code block fenced `ignore` (`rust,ignore`, or `rust ignore`: rustdoc
#    splits the info string on commas, spaces and tabs) has a
#    `<!-- ignore: reason -->` line directly above its fence: in any
#    Markdown file, and in any doc comment of any Rust file, test code and
#    `tests/` included.
# 3. A Rust block that nothing compiles is `ignore`d, with rule 2's reason.
#    Rustdoc compiles the Markdown files src/lib.rs includes as docs
#    (`#[doc = include_str!("../<path>")]` in code, not in a comment or a
#    string, and under no `cfg`, on it or on a module around it, that CI's
#    doctest run, a debug build with every feature on Linux, leaves out)
#    and the doc comments of the library crates (src/
#    and genai-rs-macros/src/) outside test code and items compiled only
#    under such a `cfg`: a file module (`#[cfg(test)] mod x;`), an inline
#    module, a function or any other item, from the doc comments and
#    attributes above it to its end, and a file whose inner `#![cfg]` leaves
#    it out. Every `cfg` in the library's non-test code must be one the
#    check can evaluate: a name or key it does not know (`target_arch`,
#    say) is a finding, not a guess. Anywhere else
#    (tests/, examples/, fuzz/, a Markdown file not included), a block
#    rustdoc would read as Rust (a fence whose info string names `rust` or
#    a rustdoc attribute such as `no_run` or `edition2021`, or, in a doc
#    comment, an untagged one) never compiles. History is exempt:
#    CHANGELOG.md's blocks show the API of the release each entry
#    describes. A fence tagged `rs` or `Rust` is Rust to GitHub but another
#    language to rustdoc, which never compiles it, so it fails everywhere:
#    tag it `rust`. `tests/doc_guards/` requires every guide to be
#    included.
# 4. A `text` block (also `txt` or `plain`, and an untagged one in a
#    Markdown file rule 3 reads) holds no Rust: a line that is a `let`
#    (single or multi-line, destructuring, or `let ... else`) or `use`
#    statement, a field assignment, a `fn`, `struct`, `enum`, `trait` or
#    `impl` header, an `if let`, `while let` or `match` header with its
#    arms, a `#[derive]`, `#[test]` or `#[tokio::main]` attribute, or a
#    bare expression that ends in `?;` or `.await`. Fence the block as rust
#    so it compiles. Other bare expressions are not read.
#
# Exit status: 0 when the tree follows the rules, 1 with each finding
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
import bisect
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
# The library crates, whose doc comments rustdoc compiles.
DOCTESTED_ROOTS = ("src/", "genai-rs-macros/src/")

MONTH = (
    r"(?:Jan(?:uary)?|Feb(?:ruary)?|Mar(?:ch)?|Apr(?:il)?|May|June?|July?|Aug(?:ust)?"
    r"|Sep(?:t(?:ember)?)?|Oct(?:ober)?|Nov(?:ember)?|Dec(?:ember)?)"
)
DAY = r"(?:0?[1-9]|[12]\d|3[01])"
MONTH_NUM = r"(?:0?[1-9]|1[0-2])"
DAY_SUFFIX = r"(?:st|nd|rd|th)?"
# A time with seconds after a date makes it a timestamp, example data rather
# than a fact. A time without seconds is how prose dates an observation.
NOT_TIME = r"(?!\s*,?\s*\d{1,2}:\d\d:\d\d)"
DATE = re.compile(
    # ISO: 2026-11-05 (or 2026-11-5), or the month 2026-07.
    rf"(?<![\w.-])20\d\d-(?:(?:0[1-9]|1[0-2])(?!-\d)|{MONTH_NUM}-{DAY})(?![\w-]|T\d){NOT_TIME}"
    # Slashed: 2026/11/05, and 11/05/2026 or 05/11/2026.
    rf"|(?<![\w/.])20\d\d/{MONTH_NUM}/{DAY}(?![\w/]){NOT_TIME}"
    rf"|(?<![\w/.]){DAY}/{DAY}/20\d\d(?![\w/]){NOT_TIME}"
    # Dotted: 2026.11.05, and 05.11.2026; dashed with the year last:
    # 11-05-2026.
    rf"|(?<![\w.])20\d\d\.{MONTH_NUM}\.{DAY}(?!\w|\.\d){NOT_TIME}"
    rf"|(?<![\w.]){DAY}\.{DAY}\.20\d\d(?!\w|\.\d){NOT_TIME}"
    rf"|(?<![\w-]){DAY}-{DAY}-20\d\d(?![\w-]){NOT_TIME}"
    # The month named: November 5, 2026; Nov. 5 2026; 5 November 2026;
    # November 2026.
    rf"|\b{MONTH}\.? {DAY}{DAY_SUFFIX},? 20\d\d\b{NOT_TIME}"
    rf"|\b{DAY}{DAY_SUFFIX} {MONTH}\.?,? 20\d\d\b{NOT_TIME}"
    rf"|\b{MONTH}\.?,? 20\d\d\b{NOT_TIME}"
)
LINK_TARGET = re.compile(r"\]\([^)\s]*\)")
AUTOLINK = re.compile(r"<https?://[^>\s]*>")
BARE_URL = re.compile(r"https?://\S+")
LINK_DEFINITION = re.compile(r"^(\s*\[[^\]]+\]:\s*)\S+")
FENCE = re.compile(r"^\s*(`{3,}|~{3,})\s*([^`]*)")
# Rustdoc splits a fence's info string on commas, spaces and tabs.
INFO_SEP = re.compile(r"[,\s]+")
REASON = re.compile(r"^\s*<!--\s*ignore:\s*\S.*-->\s*$")
TEST_FILE = re.compile(r"(_tests|^tests|^proptest_\w+|^test_subscriber)\.rs$")
# One attribute, with one level of nested brackets, and a run of them.
ATTR = r'#\[(?:[^\[\]"]|"(?:\\.|[^"\\])*"|\[(?:[^\[\]"]|"(?:\\.|[^"\\])*")*\])*\]'
ATTR_RUN = re.compile(rf"(?:{ATTR}\s*)+")
ATTR_ONE = re.compile(ATTR)
# What src/lib.rs includes as docs, and so doctests.
DOC_INCLUDE = re.compile(r'#\[doc\s*=\s*include_str!\(\s*"\.\./([^"]+\.md)"\s*\)\s*\]')
CFG_ATTR = re.compile(r"#!?\[\s*cfg\s*\((.*)\)\s*\]$", re.S)
CFG_TOKEN = re.compile(r'\s*(?:([A-Za-z_]\w*)|("(?:\\.|[^"\\])*")|(\S))')
# What CI's doctest run sets and what it does not (see cfg_holds): a name
# in neither, or a key not in CFG_VALUES, is one the check cannot evaluate.
TRUE_CFGS = {"doctest", "unix", "debug_assertions"}
FALSE_CFGS = {"test", "windows", "miri", "fuzzing", "docsrs", "genai_strict_unknown"}
# The keys whose values it knows: the one value that holds. Every
# `feature = "..."` holds.
CFG_VALUES = {"target_os": '"linux"', "target_family": '"unix"'}
# An inner attribute at the top of a file, and a run of outer ones at the
# start of a line.
INNER_ATTR = re.compile(r"^" + ATTR.replace("#", "#!", 1), re.M)
INNER_ONE = re.compile(ATTR.replace("#", "#!", 1))
LINE_ATTR_RUN = re.compile(rf"^[ \t]*((?:{ATTR}\s*)+)", re.M)
# What the lexer stops at: a comment, a string or char literal (or a
# lifetime), and a doc attribute's value.
TOKEN = re.compile(r"""//|/\*|(?<!\w)[bc]?r(#*)"|"|'|#(!?)\[\s*doc\s*=\s*""")
ESCAPE = re.compile(r"\\(?:u\{([0-9a-fA-F]{1,6})\}|x([0-9a-fA-F]{2})|(.))")
ESCAPED = {"n": "\n", "t": "\t", "r": "\r", "0": "\0"}
# An item, whose end is its `;` or its body's `}`; anything else an
# attribute can gate (a field, a variant, a statement, a match arm) may
# also end at a `,`.
ITEM_KEYWORD = re.compile(
    r"(?:pub(?:\([^)]*\))?\s+)?(?:(?:async|const|unsafe|default|extern(?:\s+\"[^\"]*\")?)\s+)*"
    r"(?:fn|struct|enum|union|trait|impl|mod|const|static|type|use|extern|macro_rules!)\W"
)
PATH_ATTR = re.compile(r'#\[\s*path\s*=\s*"([^"]+)"\s*\]')
# A file module declaration after its attributes.
MOD_DECL = re.compile(rf"((?:{ATTR}\s*)*)(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;")
RAW_STRING = re.compile(r'(?<!\w)b?r(#*)"')
CHAR_LITERAL = re.compile(r"b?'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]{1,6}\}|.)|[^\\'\n])'")
# Info-string tokens that make rustdoc read a block as Rust.
RUSTDOC_ATTRS = {"rust", "no_run", "should_panic", "compile_fail", "test_harness", "standalone_crate"}
TEXT_INFOS = {"text", "txt", "plain"}
# A line of Rust, for rule 4: a statement ending in `;`, or an item header.
RUST_STATEMENT = re.compile(
    r"^\s*(?:let\s+(?:mut\s+)?[a-z_]\w*\b.*=.*|use\s+[a-z_]\w*::\S.*"
    r"|[a-z_]\w*(?:\.[a-z_]\w*)+\s*=[^=].*);\s*(?://.*)?$"
)
RUST_ITEM = re.compile(
    r"^\s*(?:(?:pub(?:\(\w+\))?\s+)?(?:async\s+)?fn\s+[a-z_]\w*\s*[<(]"
    r"|(?:pub(?:\(\w+\))?\s+)?(?:struct|enum|trait)\s+[A-Z]\w*"
    r"|impl\b.*\{\s*$"
    r"|#\[(?:derive\(|test\]|tokio::main))"
)
# Rule 4's bare expression: a line that ends in `?;` or `.await`.
RUST_TAIL = re.compile(r"(?:\?;|\.await[?;]*)\s*(?://.*)?$")
# Rule 4's statements that span lines: a `let` whose `;` comes on a later
# line (a builder chain), a destructuring `let` or a `let ... else`, and a
# header (`if let`, `while let`, `match`) whose body follows.
LET_OPEN = re.compile(
    r"^\s*let\s+(?:mut\s+)?(?:[a-z_]\w*|\(.*\)|[A-Z]\w*(?:::\w+)*\s*[({].*?[)}])"
    r"\s*(?::\s*[^=;]+)?=(?!=)"
)
ENDS_STATEMENT = re.compile(r";\s*(?://.*)?$")
LET_HEADER = re.compile(r"^\s*(?:if|while)\s+let\s+\S.*=.*\{\s*$")
MATCH_HEADER = re.compile(r"^\s*match\s+[^{}]+\{\s*$")
MATCH_ARM = re.compile(r"^\s*[^=\s][^=]*=>")

findings = []


class Lexed:
    """One Rust file, lexed. `code` is its text with every comment and the
    inside of every string and char literal blanked (newlines and offsets
    kept), so a regex over it sees only code. `docs` maps a line index to
    the doc text it holds: a `///` or `//!` line, a line of a `/** */` or
    `/*! */` block, or of a `#[doc = "..."]` attribute's string (one line
    can hold several, from `\n` escapes). `comments` maps the index of a
    whole-line `//` comment to its text, and `inner` holds the lines of
    inner doc comments, which document the module around them."""

    def __init__(self, src):
        self.src = src
        self.starts = [0] + [m.end() for m in re.finditer("\n", src)]
        self.docs, self.comments, self.inner = {}, {}, set()
        out = list(src)
        n, i = len(src), 0

        def blank(a, b):
            for k in range(a, b):
                if out[k] != "\n":
                    out[k] = " "

        while True:
            m = TOKEN.search(src, i)
            if not m:
                break
            at, tok = m.start(), m.group(0)
            if tok == "//":
                end = src.find("\n", at)
                end = n if end < 0 else end
                line = self.line(at)
                body = src[at + 2 : end]
                if not src[self.starts[line] : at].strip():
                    if body.startswith("/") and not body.startswith("//"):
                        self.doc(line, [body[1:]], False)
                    elif body.startswith("!"):
                        self.doc(line, [body[1:]], True)
                    else:
                        self.comments[line] = body[1:] if body.startswith(" ") else body
                blank(at, end)
                i = end
            elif tok == "/*":
                depth, j = 1, at + 2
                while j < n and depth:
                    if src.startswith("/*", j):
                        depth, j = depth + 1, j + 2
                    elif src.startswith("*/", j):
                        depth, j = depth - 1, j + 2
                    else:
                        j += 1
                body = src[at + 2 : j - 2 if not depth else n]
                outer = body.startswith("*") and not body.startswith("**") and body != "*"
                if outer or body.startswith("!"):
                    self.block_doc(self.line(at), body[1:], not outer)
                blank(at, j)
                i = j
            elif tok.startswith("#"):
                literal = self.literal(m.end())
                if literal and re.match(r"\s*\]", src[literal[2] :]):
                    first, last, after, raw = literal
                    self.string_doc(first, src[first:last], raw, bool(m.group(2)))
                    blank(first, last)
                    i = after
                else:
                    i = m.end()
            elif tok == "'":
                char = CHAR_LITERAL.match(src, at)
                if char:
                    blank(at + 1, char.end() - 1)
                    i = char.end()
                else:
                    i = at + 1  # a lifetime
            else:
                first, last, after, _ = self.literal(at)
                blank(first, last)
                i = after
        self.code = "".join(out)

    def line(self, offset):
        return bisect.bisect_right(self.starts, offset) - 1

    def literal(self, at):
        """`(start, end, after, raw)` of the string literal whose quote or
        raw prefix is at `at`: its contents' span, and where it ends."""
        src, n = self.src, len(self.src)
        m = re.compile(r'[bc]?r(#*)"').match(src, at)
        if m:
            close = src.find('"' + m.group(1), m.end())
            last = n if close < 0 else close
            return m.end(), last, min(n, last + 1 + len(m.group(1))), True
        if at >= n or src[at] != '"':
            return None
        j = at + 1
        while j < n and src[j] != '"':
            j += 2 if src[j] == "\\" else 1
        return at + 1, min(j, n), min(j + 1, n), False

    def doc(self, line, texts, inner):
        self.docs.setdefault(line, []).extend(
            t[1:] if t.startswith(" ") else t for t in texts
        )
        if inner:
            self.inner.add(line)

    def block_doc(self, line, body, inner):
        """A `/** */` or `/*! */` comment's lines, with the leading `*` its
        lines all carry, if they do, removed as rustdoc does."""
        pieces = body.split("\n")
        rest = [p for p in pieces[1:] if p.strip()]
        if rest and all(p.lstrip().startswith("*") for p in rest):
            pieces = pieces[:1] + [re.sub(r"^\s*\*", "", p) for p in pieces[1:]]
        for k, piece in enumerate(pieces):
            self.doc(line + k, [piece], inner)

    def string_doc(self, at, text, raw, inner):
        """A doc attribute's string: each source line of it, unescaped, and
        split again at each `\n` escape."""
        line = self.line(at)
        if not raw:
            # A `\` at a line's end continues the string without the newline.
            text = re.sub(r"(?<!\\)((?:\\\\)*)\\\n[ \t]*", "\\1\n", text)
        for k, piece in enumerate(text.split("\n")):
            if not raw:
                piece = ESCAPE.sub(
                    lambda e: chr(int(e.group(1) or e.group(2), 16))
                    if e.group(1) or e.group(2)
                    else ESCAPED.get(e.group(3), e.group(3)),
                    piece,
                )
            self.doc(line + k, piece.split("\n"), inner)


class UnknownCfg(Exception):
    """A cfg predicate the check cannot evaluate."""


def cfg_holds(predicate):
    """Whether a `cfg(...)` predicate holds in CI's doctest run, a debug
    build with every feature on Linux: `doctest`, `unix`,
    `debug_assertions`, every `feature = "..."`, `target_os = "linux"` and
    `target_family = "unix"` do; `test`, `windows`, `miri`, `fuzzing`,
    `docsrs`, `genai_strict_unknown`, another `target_os` or
    `target_family`, and an empty `any()` do not. Raises UnknownCfg on any
    other name or key, such as `target_arch`, rather than guess."""
    tokens = [m.group(1) or m.group(2) or m.group(3) for m in CFG_TOKEN.finditer(predicate)]
    tokens = [t for t in tokens if t]

    def parse(i):
        name = tokens[i]
        if name in ("all", "any", "not") and tokens[i + 1] == "(":
            i += 2
            values = []
            while tokens[i] != ")":
                value, i = parse(i)
                values.append(value)
                if tokens[i] == ",":
                    i += 1
            i += 1
            if name == "all":
                return all(values), i
            if name == "any":
                return any(values), i
            if len(values) != 1:
                raise UnknownCfg("it cannot parse it")
            return not values[0], i
        if i + 1 < len(tokens) and tokens[i + 1] == "=":
            value = tokens[i + 2]
            if name == "feature":
                return True, i + 3
            if name in CFG_VALUES and value.startswith('"'):
                return value == CFG_VALUES[name], i + 3
            raise UnknownCfg(f"it does not know `{name}`")
        if name in TRUE_CFGS or name in FALSE_CFGS:
            return name in TRUE_CFGS, i + 1
        raise UnknownCfg(f"it does not know `{name}`")

    try:
        value, end = parse(0)
    except IndexError:
        raise UnknownCfg("it cannot parse it") from None
    if end != len(tokens):
        raise UnknownCfg("it cannot parse it")
    return value


reported = set()


def attrs_hold(lexed, start, end, path, attrs=ATTR_ONE):
    """Whether every `cfg` attribute (`attrs`: outer, or inner) in
    `lexed.code[start:end]` holds in the doctest run. One the check cannot
    evaluate is a finding (when `path` is given) and counts as not
    holding."""
    holds = True
    for attr in attrs.finditer(lexed.code, start, end):
        m = CFG_ATTR.match(lexed.src[attr.start() : attr.end()])
        if not m:
            continue
        try:
            holds = cfg_holds(m.group(1)) and holds
        except UnknownCfg as e:
            holds = False
            line = lexed.line(attr.start()) + 1
            if path and (path, line) not in reported:
                reported.add((path, line))
                findings.append(
                    f"{path}:{line}: this check cannot evaluate `cfg({m.group(1).strip()})` "
                    f"({e}): teach cfg_holds, here and in tests/doc_guards/, whether CI's "
                    "doctest run (a debug build with every feature on Linux) sets it"
                )
    return holds


def item_end(code, at):
    """The offset where what an attribute run gates, starting at `at` in
    `code`, ends: an item at its `;` or the `}` that closes its body, and
    anything else (a field, a variant, a statement, a match arm) also at a
    `,`, or where the block around it closes."""
    at = re.compile(r"\s*").match(code, at).end()
    item = ITEM_KEYWORD.match(code, at)
    depth, body = 0, False
    for k in range(at, len(code)):
        c = code[k]
        if c in "([{":
            body = body or (c == "{" and depth == 0)
            depth += 1
        elif c in ")]}":
            depth -= 1
            if depth < 0 or (depth == 0 and c == "}" and body):
                return k
        elif depth == 0 and (c == ";" or (c == "," and not item)):
            return k
    return len(code)


def gated_lines(path, lexed, report):
    """The indexes of the lines `lexed` holds that the doctest run never
    compiles: every item under a `cfg` that run leaves out (an inline
    module, a function, an impl, a field), from the doc comments and
    attributes above it to its end, and the whole file when an inner
    `#![cfg]` at its top leaves it out. `report`: a `cfg` the check
    cannot evaluate is a finding."""
    where = path if report else None
    for m in INNER_ATTR.finditer(lexed.code):
        if not attrs_hold(lexed, m.start(), m.end(), where, INNER_ONE):
            return set(range(len(lexed.starts)))
    code_lines = lexed.code.split("\n")
    gated = set()
    for m in LINE_ATTR_RUN.finditer(lexed.code):
        if attrs_hold(lexed, m.start(1), m.end(1), where):
            continue
        first = lexed.line(m.start(1))
        last = lexed.line(item_end(lexed.code, m.end(1)))
        # Up over the doc comments, comments and blank lines above it.
        while first > 0 and not code_lines[first - 1].strip() and first - 1 not in lexed.inner:
            first -= 1
        gated.update(range(first, last + 1))
    return gated


def doc_includes(lib_rs, report_path):
    """The Markdown files `lib_rs` includes as docs in code that the
    doctest run compiles: not in a comment or a string, and not in or on an
    item under a `cfg` that run leaves out."""
    lexed = Lexed(lib_rs)
    gated = gated_lines(report_path, lexed, True)
    included = set()
    for attr in ATTR_ONE.finditer(lexed.code):
        m = DOC_INCLUDE.match(lib_rs[attr.start() : attr.end()])
        if m and lexed.line(attr.start()) not in gated:
            included.add(m.group(1))
    return included


def gated_modules(path, lexed, gated, report):
    """Path prefixes of the file modules `path` declares under a `cfg` the
    doctest run leaves out (`#[cfg(test)] mod x;`), or inside an item
    that is (`gated`, its lines): their doc comments are never
    doctested."""
    here = os.path.dirname(path)
    base = os.path.basename(path)
    own = here if base in ("lib.rs", "main.rs", "mod.rs") else os.path.join(here, base[:-3])
    modules = []
    for m in MOD_DECL.finditer(lexed.code):
        holds = attrs_hold(lexed, m.start(1), m.end(1), path if report else None)
        if holds and lexed.line(m.start(2)) not in gated:
            continue
        found = None
        for attr in ATTR_ONE.finditer(lexed.code, m.start(1), m.end(1)):
            found = PATH_ATTR.match(lexed.src[attr.start() : attr.end()]) or found
        if found:
            file = os.path.normpath(os.path.join(here, found.group(1)))
        else:
            file = os.path.join(own, m.group(2) + ".rs")
        modules.append(file)
        # Its own modules: beside a mod.rs, under its stem otherwise.
        modules.append(os.path.join(own, m.group(2)) + "/")
        modules.append(file[: -len(".rs")] + "/")
    return modules


def clean(text):
    """`text` without what holds a date that is not a fact: link targets
    and URLs."""
    text = LINK_DEFINITION.sub(r"\1", text)
    for pattern in (LINK_TARGET, AUTOLINK, BARE_URL):
        text = pattern.sub("", text)
    return text


def dates(text):
    return [d for d in DATE.findall(clean(text)) if d not in NAMES]


def wrapped_dates(first, second):
    """Dates that start on line `first` and end on line `second`: prose is
    hard-wrapped, and a named-month date has spaces to break at."""
    head = clean(first).rstrip()
    joined = head + " " + clean(second).lstrip()
    return [
        m.group(0)
        for m in DATE.finditer(joined)
        if m.start() < len(head) < m.end() and m.group(0) not in NAMES
    ]


def info_tokens(info):
    return [t for t in INFO_SEP.split(info.strip()) if t]


class Fences:
    """Tracks one Markdown text's fences, line by line."""

    def __init__(self):
        self.open = None  # the opening run of backticks or tildes
        self.tokens = []  # the open fence's info string, split
        self.start = 0  # the open fence's line number
        self.body = []  # the open fence's lines so far
        self.numbers = []  # the line number of each, which can repeat in Rust

    def step(self, line, n):
        """Returns `("open", tokens)` on a fence's first line, `("close",
        tokens, start, body, numbers)` on its last, and None otherwise."""
        m = FENCE.match(line)
        if self.open is not None:
            closes = m and m.group(1)[0] == self.open[0] and len(m.group(1)) >= len(self.open)
            if closes and not line.strip().strip(m.group(1)[0]):
                self.open = None
                return ("close", self.tokens, self.start, self.body, self.numbers)
            self.body.append(line)
            self.numbers.append(n)
            return None
        if not m:
            return None
        self.open = m.group(1)
        self.tokens = info_tokens(m.group(2))
        self.start = n
        self.body = []
        self.numbers = []
        return ("open", self.tokens)


def rust_in_text(body):
    """The index in `body` of the first line rule 4 reads as Rust, or None."""
    for k, line in enumerate(body):
        if (
            RUST_STATEMENT.match(line)
            or RUST_ITEM.match(line)
            or LET_HEADER.match(line)
            or RUST_TAIL.search(line)
        ):
            return k
        if LET_OPEN.match(line) and any(ENDS_STATEMENT.search(rest) for rest in body[k:]):
            return k
        if MATCH_HEADER.match(line) and any(MATCH_ARM.match(rest) for rest in body[k + 1 :]):
            return k
    return None


def check_fence(path, n, event, prev, doc_line, compiled):
    """Rules 2 to 4 for one fence event in `path`. `doc_line`: the fence is
    in a doc comment, where rustdoc reads an untagged block as Rust.
    `compiled`: whether rustdoc compiles a Rust block here."""
    if event is None:
        return
    if event[0] == "open":
        tokens = event[1]
        if "ignore" in tokens and not REASON.match(prev or ""):
            where = "doc line" if doc_line else "line"
            findings.append(
                f"{path}:{n}: an ignore fence needs a `<!-- ignore: reason -->` {where} directly above it"
            )
        aliases = [t for t in tokens if t.lower() in ("rs", "rust") and t != "rust"]
        if aliases:
            findings.append(
                f"{path}:{n}: a `{aliases[0]}` fence is Rust to GitHub but another language to "
                "rustdoc, which never compiles it: fence the block as `rust`"
            )
            return
        rusty = (
            RUSTDOC_ATTRS.intersection(tokens)
            or any(t.startswith("edition") for t in tokens)
            or (doc_line and not tokens)
        )
        if not compiled and "ignore" not in tokens and rusty:
            if doc_line:
                findings.append(
                    f"{path}:{n}: a Rust block in this doc comment never compiles, because rustdoc "
                    "doctests only the library crates' non-test code: mark it `ignore` with a "
                    "`<!-- ignore: reason -->` doc line above it, or fence it as `text` if it is "
                    "not Rust"
                )
            else:
                findings.append(
                    f"{path}:{n}: a Rust block here never compiles, because src/lib.rs does not "
                    f"include {path} in its doc_tests: include the file there, or mark the block "
                    "`ignore` with a `<!-- ignore: reason -->` line above it"
                )
        return
    _, tokens, start, body, numbers = event
    texty = (tokens[:1] and tokens[0] in TEXT_INFOS) or (not tokens and not compiled and not doc_line)
    if texty:
        k = rust_in_text(body)
        if k is not None:
            findings.append(
                f"{path}:{numbers[k]}: Rust in a text block never compiles: fence the block "
                f"(line {start}) as rust, or as `rust,ignore` with a reason"
            )


def dated_fact(path, n, d, where=""):
    if where == "doc":
        findings.append(
            f"{path}:{n}: dated live fact ({d}) in a doc comment: state the current behavior "
            "without the date, and keep the dated evidence in a home or in a `//` comment "
            "beside the code it justifies (D-032)"
        )
    elif where == "example":
        findings.append(
            f"{path}:{n}: dated live fact ({d}) in an example: state what the API "
            "does now and keep the dated observation in a home (D-032)"
        )
    else:
        findings.append(
            f"{path}:{n}: dated live fact ({d}) outside its home: record it in "
            "docs/ENUM_WIRE_FORMATS.md or docs/INTERACTIONS_API_GAP.md (Antigravity: "
            "docs/ANTIGRAVITY_BRIDGE_DESIGN.md) and state the current behavior here (D-032)"
        )


def check_markdown(path, text, doctested):
    dated = path not in HOMES and path not in HISTORY and "/tests/" not in "/" + path
    compiled = path in doctested or path in HISTORY
    fences = Fences()
    prev = ""
    for n, line in enumerate(text.splitlines(), 1):
        check_fence(path, n, fences.step(line, n), prev, False, compiled)
        if dated:
            for d in wrapped_dates(prev, line) if n > 1 else []:
                dated_fact(path, n - 1, d)
            for d in dates(line):
                dated_fact(path, n, d)
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
        # A signature can span lines: the body opens on the first line that
        # ends in `{`, unless a `;` ends the item first.
        end = next((k for k in range(j, len(lines)) if lines[k].rstrip().endswith(("{", ";"))), j)
        if lines[end].rstrip().endswith("{"):
            close = indent + "}"
            end = next((k for k in range(end + 1, len(lines)) if lines[k].rstrip() == close), len(lines) - 1)
        skip.update(range(i, end + 1))
        i = end + 1
    return skip


def check_rust(path, lexed, dated, uncompiled):
    """Rules 2 to 4 in every doc comment, whatever its form; rule 1, when
    `dated`, in the comments of non-test code. `uncompiled`: the indexes
    of the lines whose doc comments rustdoc does not compile, or None when
    it compiles none in this file."""
    every_comment = path.startswith("examples/")
    lines = lexed.src.splitlines()
    tests = test_items(lines)
    skip = tests if dated else set(range(len(lines)))
    fences = Fences()
    prev = None  # the previous doc line's text; None outside a doc block
    carry = None  # (line number, kind, text) of the last comment line rule 1 read
    for i in range(len(lines)):
        n = i + 1
        docs = lexed.docs.get(i)
        if docs is None:
            prev = None
            fences = Fences()
        else:
            compiled = uncompiled is not None and i not in uncompiled
            for doc in docs:
                check_fence(path, n, fences.step(doc, n), prev, True, compiled)
                prev = doc
        comment = lexed.comments.get(i)
        if docs is not None:
            kind, texts = "doc", docs
        elif comment is not None and every_comment:
            kind, texts = "example", [comment]
        else:
            kind, texts = None, []
        if kind is None or i in skip:
            carry = None
            continue
        for text in texts:
            if carry and carry[1] == kind:
                for d in wrapped_dates(carry[2], text):
                    dated_fact(path, carry[0], d, kind)
            for d in dates(text):
                dated_fact(path, n, d, kind)
            carry = (n, kind, text)


def read(path):
    with open(os.path.join(root, path), encoding="utf-8") as f:
        return f.read()


listed = subprocess.run(
    ["git", "-C", root, "ls-files", "-z", "--cached", "--others", "--exclude-standard",
     "--", "*.md", "*.rs"],
    check=True,
    capture_output=True,
).stdout.decode()
paths = sorted({p for p in listed.split("\0") if p and os.path.isfile(os.path.join(root, p))})


def is_test_file(path):
    return bool(
        "/tests/" in "/" + path
        or path.startswith("src/coverage_oracle/")
        or TEST_FILE.search(os.path.basename(path))
    )


doctested = set()
if os.path.isfile(os.path.join(root, "src", "lib.rs")):
    doctested = doc_includes(read("src/lib.rs"), "src/lib.rs")
# The library's non-test files: lexed, and the lines whose doc comments the
# doctest run never compiles. A cfg the check cannot evaluate fails here.
lexed, uncompiled, gated = {}, {}, []
for path in paths:
    if path.endswith(".rs") and path.startswith(DOCTESTED_ROOTS):
        lexed[path] = Lexed(read(path))
        test_file = is_test_file(path)
        lines = gated_lines(path, lexed[path], True) if not test_file else set()
        uncompiled[path] = lines | test_items(lexed[path].src.splitlines())
        gated.extend(gated_modules(path, lexed[path], lines, not test_file))
checked = 0
for path in paths:
    if path.endswith(".md"):
        check_markdown(path, read(path), doctested)
    else:
        test_file = is_test_file(path)
        dated = path.startswith(RUST_ROOTS) and not test_file
        doctested_file = (
            path.startswith(DOCTESTED_ROOTS)
            and not test_file
            and not any(path == g or (g.endswith("/") and path.startswith(g)) for g in gated)
        )
        check_rust(
            path,
            lexed.get(path) or Lexed(read(path)),
            dated,
            uncompiled[path] if doctested_file else None,
        )
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
print(
    "No dated live fact outside its home, every ignore fence has a reason, and every "
    f"Rust block compiles or says why not ({checked} files)."
)
EOF
