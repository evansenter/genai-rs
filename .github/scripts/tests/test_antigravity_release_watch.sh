#!/usr/bin/env bash
# Exercises antigravity_release_watch.sh and dump_harness_descriptor.py
# against a stubbed `gh` and two synthetic wheels.
#
# The watch only runs on a schedule, so a broken branch would surface days
# later, or never: a lookup that matched nothing would file a duplicate every
# morning, and a dump that found no descriptors would diff as "unchanged"
# forever. Both are asserted here.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPTS="$(dirname "$HERE")"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

failures=0

# Writes a synthetic unpacked wheel under $1 whose localharness_pb2.py
# registers a descriptor with a HarnessSideTools message; extra field names
# in $2.. are added to it. The encoder writes protobuf wire format by hand,
# the same subset the dump script reads.
make_wheel() {
  local dir="$1"
  shift
  mkdir -p "$dir/google/antigravity/proto"
  python3 -I - "$dir/google/antigravity/proto/localharness_pb2.py" "$@" <<'PY'
import sys

def varint(n):
    out = b""
    while True:
        byte = n & 0x7F
        n >>= 7
        out += bytes([byte | (0x80 if n else 0)])
        if not n:
            return out

def ld(number, payload):
    return varint(number << 3 | 2) + varint(len(payload)) + payload

def vi(number, value):
    return varint(number << 3) + varint(value)

def field(name, number, type_name):
    return ld(1, name.encode()) + vi(3, number) + vi(4, 1) + vi(5, 11) + ld(6, type_name.encode())

toggle = ld(1, b"ToolToggle") + ld(2, ld(1, b"enabled") + vi(3, 1) + vi(4, 1) + vi(5, 8))
fields = [field("view_file", 1, ".antigravity.localharness.ToolToggle")]
for i, name in enumerate(sys.argv[2:]):
    fields.append(field(name, 20 + i, ".antigravity.localharness.ToolToggle"))
tools = ld(1, b"HarnessSideTools") + b"".join(ld(2, f) for f in fields)
state = ld(1, b"State") + ld(2, ld(1, b"STATE_IDLE") + vi(2, 1))
update = ld(1, b"TrajectoryStateUpdate") + ld(4, state)
descriptor = (ld(1, b"google/antigravity/proto/localharness.proto")
              + ld(2, b"antigravity.localharness")
              + ld(4, toggle) + ld(4, tools) + ld(4, update))
with open(sys.argv[1], "w") as out:
    out.write("from google.protobuf import descriptor_pool as _descriptor_pool\n")
    out.write("DESCRIPTOR = _descriptor_pool.Default().AddSerializedFile(%r)\n" % descriptor)
PY
}

make_wheel "$WORK/pinned"
make_wheel "$WORK/latest" run_workflow
mkdir -p "$WORK/latest/google/antigravity/connections"
echo "      self, *, use_interactions_api: bool = False" \
  > "$WORK/latest/google/antigravity/connections/local_connection.py"

# A `gh` stub: `issue list` answers from $1 through the caller's --jq,
# `issue view` from $2, and every other call is logged as `GH-CALL: ...`
# (with `--body-file` contents as `GH-BODY: ...`). The JSON goes through
# files, since the title's apostrophe would break a quoted heredoc.
make_gh_stub() {
  printf '%s\n' "$1" > "$WORK/list.json"
  printf '%s\n' "${2:-{\}}" > "$WORK/view.json"
  cat > "$WORK/gh" <<STUB
#!/usr/bin/env bash
if [ "\$1 \$2" = "issue list" ]; then
  prev=""; filter=""; state=""
  for a in "\$@"; do
    [ "\$prev" = "--jq" ] && filter="\$a"
    [ "\$prev" = "--state" ] && state="\$a"
    prev="\$a"
  done
  echo "GH-LIST: state=\$state" >&2
  jq "\$filter" "$WORK/list.json"
  exit 0
fi
if [ "\$1 \$2" = "issue view" ]; then
  cat "$WORK/view.json"
  exit 0
fi
echo "GH-CALL: \$*" >&2
prev=""
for a in "\$@"; do
  if [ "\$prev" = "--body-file" ]; then
    echo "GH-BODY: \$(cat "\$a")" >&2
  fi
  prev="\$a"
done
STUB
  chmod +x "$WORK/gh"
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

check_absent() {
  local label="$1" unexpected="$2" actual="$3"
  if grep -qF -- "$unexpected" <<<"$actual"; then
    echo "FAIL - $label"
    echo "       did not expect: $unexpected"
    sed 's/^/         /' <<<"$actual"
    failures=$((failures + 1))
  else
    echo "ok   - $label"
  fi
}

# Runs the watch under the stub, keeping its output and status in $out/$rc.
run_watch() {
  rc=0
  out=$(PATH="$WORK:$PATH" PINNED_WHEEL_DIR="$WORK/pinned" LATEST_WHEEL_DIR="$WORK/latest" \
    "$@" bash "$SCRIPTS/antigravity_release_watch.sh" 2>&1) || rc=$?
}

TITLE="Antigravity release watch: PyPI's google-antigravity differs from the pinned harness"

# --- The dump decodes fields and enum values, oneof-free.
dump=$(python3 -I "$SCRIPTS/dump_harness_descriptor.py" "$WORK/latest")
check "dump: a message field with its type" \
  "localharness.proto: message antigravity.localharness.HarnessSideTools: 20 run_workflow: antigravity.localharness.ToolToggle" "$dump"
check "dump: a nested enum value" \
  "localharness.proto: enum antigravity.localharness.TrajectoryStateUpdate.State: STATE_IDLE = 1" "$dump"

# --- The dump fails rather than reporting nothing.
mkdir -p "$WORK/empty/google/antigravity"
if python3 -I "$SCRIPTS/dump_harness_descriptor.py" "$WORK/empty" >/dev/null 2>&1; then
  echo "FAIL - dump: a wheel with no descriptors must fail, not dump nothing"
  failures=$((failures + 1))
else
  echo "ok   - dump: no descriptors is an error"
fi

# --- The pin is current and nothing is open: a no-op.
make_gh_stub '[]'
run_watch env PINNED_VERSION=0.1.23 LATEST_VERSION=0.1.23
check "current: exits cleanly" "nothing to report" "$out"
check_absent "current: files nothing" "issue create" "$out"

# --- Without an override the pin is read from the source: the constant's
#     line still has the shape the sed expects.
make_gh_stub '[]'
PIN=$(sed -n 's/^pub const SUPPORTED_HARNESS_VERSION: &str = "\(.*\)";$/\1/p' \
  "$SCRIPTS/../../src/antigravity/config.rs")
run_watch env LATEST_VERSION="$PIN"
check "pin: read from src/antigravity/config.rs" "Pinned: $PIN. Latest on PyPI: $PIN." "$out"

# --- The pin caught up: the open issue is closed.
make_gh_stub "[{\"title\":\"$TITLE\",\"number\":41}]"
run_watch env PINNED_VERSION=0.1.24 LATEST_VERSION=0.1.24
check "caught up: closes the issue" "GH-CALL: issue close 41" "$out"
check "caught up: looks only at open issues" "GH-LIST: state=open" "$out"

# --- A new release and no issue: one is filed, labelled, with the diff.
make_gh_stub '[]'
run_watch env PINNED_VERSION=0.1.23 LATEST_VERSION=0.1.24
[ "$rc" -eq 0 ] || { echo "FAIL - new release: exited $rc"; failures=$((failures + 1)); }
check "new release: files an issue" "issue create --title $TITLE" "$out"
check "new release: labels it for the lookup" "--label antigravity-release" "$out"
check "new release: carries the descriptor diff" \
  "> localharness.proto: message antigravity.localharness.HarnessSideTools: 20 run_workflow" "$out"
check "new release: names both versions" "**0.1.24** is on PyPI; this crate pins **0.1.23**" "$out"
check "new release: shows the Python transport default" "use_interactions_api: bool = False" "$out"
check "new release: records the release it describes" "<!-- watch-latest: 0.1.24 -->" "$out"

# --- Same release again: the body is refreshed, no repeat comment, and the
#     maintainer's notes below the marker survive.
PREV_BODY='old body\n<!-- watch-latest: 0.1.24 -->\n<!-- Generated above; anything you add below this line is preserved. -->\nmaintainer note 7731'
make_gh_stub "[{\"title\":\"$TITLE\",\"number\":42}]" "{\"state\":\"OPEN\",\"body\":\"$PREV_BODY\"}"
run_watch env PINNED_VERSION=0.1.23 LATEST_VERSION=0.1.24
check "same release: edits the body" "GH-CALL: issue edit 42" "$out"
check "same release: keeps the maintainer's notes" "maintainer note 7731" "$out"
check_absent "same release: no repeat comment" "issue comment" "$out"
check_absent "same release: no duplicate issue" "issue create" "$out"

# --- A newer release on a closed issue: reopened, edited, commented.
PREV_BODY='old body\n<!-- watch-latest: 0.1.24 -->'
make_gh_stub "[{\"title\":\"$TITLE\",\"number\":43}]" "{\"state\":\"CLOSED\",\"body\":\"$PREV_BODY\"}"
run_watch env PINNED_VERSION=0.1.23 LATEST_VERSION=0.1.25
check "newer release: reopens a closed issue" "GH-CALL: issue reopen 43" "$out"
check "newer release: comments once" "GH-CALL: issue comment 43" "$out"
check "newer release: looks at closed issues too" "GH-LIST: state=all" "$out"

# --- An unreadable wheel fails the run (and so escalates), not a green no-op.
make_gh_stub '[]'
rc=0
out=$(PATH="$WORK:$PATH" PINNED_WHEEL_DIR="$WORK/pinned" LATEST_WHEEL_DIR="$WORK/empty" \
  PINNED_VERSION=0.1.23 LATEST_VERSION=0.1.24 \
  bash "$SCRIPTS/antigravity_release_watch.sh" 2>&1) || rc=$?
if [ "$rc" -eq 0 ]; then
  echo "FAIL - unreadable wheel: the run must fail"
  failures=$((failures + 1))
else
  echo "ok   - unreadable wheel: the run fails"
fi
check_absent "unreadable wheel: files nothing" "issue create" "$out"

if [ "$failures" -ne 0 ]; then
  echo "$failures check(s) failed"
  exit 1
fi
echo "all antigravity release watch checks passed"
