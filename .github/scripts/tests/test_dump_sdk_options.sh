#!/usr/bin/env bash
# Exercises dump_sdk_options.py against synthetic google-antigravity SDKs
# (fake_sdk_wheel.py lays one out from the dump's own tables).
#
# The parity guard in tests/antigravity_scripted/parity.rs is only as wide
# as this dump: a name the SDK exports that the dump never looks at reads as
# "nothing new" forever. Each way of exposing a name is planted here once,
# and the dump must refuse it until it is placed.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPTS="$(dirname "$HERE")"
DUMP="$SCRIPTS/dump_sdk_options.py"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

failures=0

# A fresh synthetic SDK under $WORK/$1; extra `Class.field`s in $2..
fresh() {
  local name="$1"
  shift
  rm -rf "${WORK:?}/$name"
  python3 -I "$HERE/fake_sdk_wheel.py" "$DUMP" "$WORK/$name" "$@"
  PKG="$WORK/$name/google/antigravity"
}

# Runs the dump with "$@", keeping stdout, stderr and the status.
run_dump() {
  rc=0
  out=$(python3 -I "$DUMP" "$@" 2>"$WORK/stderr") || rc=$?
  err=$(cat "$WORK/stderr")
}

check() {
  local label="$1" expected="$2" actual="$3"
  if grep -qF -- "$expected" <<<"$actual"; then
    echo "ok   - $label"
  else
    echo "FAIL - $label"
    echo "       expected to find: $expected"
    echo "       in:"
    sed 's/^/         /' <<<"$actual"
    failures=$((failures + 1))
  fi
}

# Expects the last dump to have succeeded.
check_passed() {
  if [ "$rc" -eq 0 ]; then
    echo "ok   - $1"
  else
    echo "FAIL - $1: exited $rc: $err"
    failures=$((failures + 1))
  fi
}

# Expects the last dump to have failed, naming $2.
check_refused() {
  local label="$1" expected="$2"
  if [ "$rc" -eq 0 ]; then
    echo "FAIL - $label: the dump must fail"
    failures=$((failures + 1))
    return
  fi
  check "$label" "$expected" "$err"
}

# --- The synthetic SDK places everything: a clean dump of its keys.
fresh base
run_dump "$WORK/base"
check_passed "base: dumps cleanly"
check "base: a field key" "AgentConfig.option" "$out"
check "base: a method key" "Conversation.method" "$out"
check "base: an enum key" "ThinkingLevel.MEMBER" "$out"
check "base: a bare class key" "InteractionsAgentConfig" "$out"
check "base: a module function key" "types.function" "$out"
check "base: a hook decorator key" "hooks.on_event" "$out"

# --- A new field is a new key, which the parity table must then list.
fresh field AgentConfig.new_option
run_dump "$WORK/field"
check "new field: a new key" "AgentConfig.new_option" "$out"

# --- A function exported through __all__ from a module whose functions SPEC
#     does not read (the gap that hid `from_bytes`).
fresh exported
printf '\ndef new_helper():\n    pass\n' >> "$PKG/models.py"
printf '\nfrom google.antigravity.models import new_helper\n' >> "$PKG/__init__.py"
python3 -I - "$PKG/__init__.py" <<'PY'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
text = path.read_text()
path.write_text(text.replace("'beta']", "'beta', 'new_helper']", 1))
PY
run_dump "$WORK/exported"
check_refused "exported function: refused" \
  "exported name in neither SPEC nor CLASS_SKIP: new_helper (models.py, exported by __init__.py)"

# --- The same function, defined but exported by nobody: not an option.
fresh unexported
printf '\ndef new_helper():\n    pass\n' >> "$PKG/models.py"
run_dump "$WORK/unexported"
check_passed "unexported function: not an option"

# --- A name a package __init__ binds itself (an alias or a constant) and
#     exports. The __init__ defines no function or class, so it sits in
#     neither SPEC nor MODULE_SKIP; the name it binds is still SDK surface.
fresh init_alias
printf '\nDEFAULT_POLICY = int\n' >> "$PKG/__init__.py"
python3 -I - "$PKG/__init__.py" <<'PY'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
text = path.read_text()
path.write_text(text.replace("'beta']", "'beta', 'DEFAULT_POLICY']", 1))
PY
run_dump "$WORK/init_alias"
check_refused "alias bound in a package __init__: refused" \
  "exported name bound in a module neither SPEC nor MODULE_SKIP reads: DEFAULT_POLICY (__init__.py, exported by __init__.py)"

# --- A name a package re-exports (no __all__) from a module MODULE_SKIP skips.
fresh reexported
printf '\nclass NewResult:\n    pass\n' >> "$PKG/connections/local/types.py"
printf 'from google.antigravity.connections.local.types import NewResult\n' \
  >> "$PKG/connections/local/__init__.py"
run_dump "$WORK/reexported"
check_refused "re-export from a skipped module: refused" \
  "exported name in neither SPEC nor EXPORT_SKIP: NewResult (connections/local/types.py, exported by connections/local/__init__.py)"

# --- A new module, such as utils/otel.py was: refused until placed.
fresh module
mkdir -p "$PKG/utils"
printf 'def get_new_hooks():\n    pass\n' > "$PKG/utils/new_hooks.py"
printf 'def ignored():\n    pass\n' > "$PKG/utils/new_hooks_test.py"
run_dump "$WORK/module"
check_refused "new module: refused" "module in neither SPEC nor MODULE_SKIP: utils/new_hooks.py"
if grep -qF "new_hooks_test.py" <<<"$err"; then
  echo "FAIL - new module: a *_test.py module is not SDK surface"
  failures=$((failures + 1))
else
  echo "ok   - new module: tests are not SDK surface"
fi

# --- An @beta export of a function in a module SPEC reads.
fresh beta
printf '\n@_beta_lib.beta\ndef shiny():\n    pass\n' >> "$PKG/models.py"
run_dump "$WORK/beta"
check_refused "@beta export: refused" \
  "exported name in neither SPEC nor CLASS_SKIP: shiny (models.py, exported by models.py)"

# --- A new public class in a module SPEC reads (the original class check).
fresh class
printf '\nclass NewConfig:\n    knob: int = 0\n' >> "$PKG/types.py"
run_dump "$WORK/class"
check_refused "new class: refused" "class in neither SPEC nor CLASS_SKIP: NewConfig (types.py)"

# --- An export that names nothing.
fresh dangling
python3 -I - "$PKG/__init__.py" <<'PY'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
path.write_text(path.read_text().replace("'beta']", "'beta', 'Ghost']", 1))
PY
run_dump "$WORK/dangling"
check_refused "dangling export: refused" "__init__.py exports Ghost, which it neither defines nor imports"

# --- A layout change: a module SPEC reads is gone.
fresh layout
rm "$PKG/workflows.py"
run_dump "$WORK/layout"
check_refused "missing SPEC module: refused" "missing $PKG/workflows.py"

# --- --lenient, for the release watch: the keys it could read, the
#     problems on stderr, and success.
fresh lenient
printf '\nclass NewConfig:\n    knob: int = 0\n' >> "$PKG/types.py"
run_dump --lenient "$WORK/lenient"
check_passed "lenient: exits 0"
check "lenient: still dumps keys" "AgentConfig.option" "$out"
check "lenient: reports the problem" "class in neither SPEC nor CLASS_SKIP: NewConfig (types.py)" "$err"

# --- No package at all fails even when lenient: there is nothing to diff.
mkdir -p "$WORK/empty"
run_dump --lenient "$WORK/empty"
check_refused "no package: refused, even lenient" "no google/antigravity package"

if [ "$failures" -ne 0 ]; then
  echo "$failures check(s) failed"
  exit 1
fi
echo "all dump_sdk_options checks passed"
