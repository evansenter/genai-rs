#!/usr/bin/env bash
# Exercises `scripts/setup-dev.sh` against stub compilers.
#
# It is the one piece of new shell in this work with no automated coverage,
# and it has the same property its siblings do: `bash -n` cannot see what it
# gets wrong. The `command -v mold` guard that shipped in an earlier round was
# valid shell meaning the wrong thing, and it wrote a config that broke every
# subsequent build — the #428 failure, reached through the control built to
# prevent it.
#
# The `linker` pin is the sharpest case, because it is a *file format* claim
# rather than a control-flow one: appending a bare `linker = ...` line is
# correct only while `.cargo/config.toml.example` ends inside
# `[target.x86_64-unknown-linux-gnu]`. Add a second table to that file later
# and the line lands under the wrong one, which cargo accepts and then
# ignores for the target that matters.
#
# Depends on bash, and on python3 only for the TOML parse — which is the
# assertion that would otherwise be a `grep` proving nothing about structure.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../../.." && pwd)"
SCRIPT="$ROOT/scripts/setup-dev.sh"
CONFIG="$ROOT/.cargo/config.toml"
WORK="$(mktemp -d)"

# The script writes into the real checkout, so restore whatever was there.
had_config=0
[ -f "$CONFIG" ] && { had_config=1; cp "$CONFIG" "$WORK/pre-existing"; }
restore() {
  rm -f "$CONFIG"
  [ "$had_config" -eq 1 ] && cp "$WORK/pre-existing" "$CONFIG"
  rm -rf "$WORK"
}
trap restore EXIT

failures=0

# A stand-in for a compiler that can drive mold: it *drops* the flag and
# links with the default linker.
#
# Not "rewrite it to -fuse-ld=lld", which is what the first version did — lld
# is no more installed by default than mold is, so this harness passed on a
# box that happened to have it and failed on the CI runner with
# `cannot find 'ld'`, exercising the probe-fails branch twice and the success
# branch never. The stub stands in for a driver that *accepts* the flag; it
# is not here to exercise a linker.
#
# The real compiler is resolved *now* and baked in as an absolute path. The
# stub gets installed as `cc` further down to test the default-CC path, and a
# stub named `cc` that execs `cc` finds itself — an infinite loop, which is
# how the first version of that case hung rather than failed.
mkdir -p "$WORK/bin"
REAL_CC="$(command -v cc)"
cat > "$WORK/bin/mold-capable-cc" <<STUB
#!/usr/bin/env bash
args=()
for a in "\$@"; do
  [ "\$a" = "-fuse-ld=mold" ] && continue
  args+=("\$a")
done
exec "$REAL_CC" "\${args[@]}"
STUB
chmod +x "$WORK/bin/mold-capable-cc"

run() {
  rc=0
  out=$(cd "$ROOT" && "$@" bash "$SCRIPT" 2>&1) || rc=$?
}

expect_rc() {
  local label="$1" want="$2"
  if [ "$rc" -eq "$want" ]; then
    echo "ok   - $label (exit $rc)"
  else
    echo "FAIL - $label: expected exit $want, got $rc"
    sed 's/^/         /' <<<"$out"
    failures=$((failures + 1))
  fi
}

check() {
  local label="$1" needle="$2"
  if grep -q -- "$needle" <<<"$out"; then
    echo "ok   - $label"
  else
    echo "FAIL - $label: expected '$needle'"
    sed 's/^/         /' <<<"$out"
    failures=$((failures + 1))
  fi
}

pass() { echo "ok   - $1"; }
fail() { echo "FAIL - $1"; failures=$((failures + 1)); }

# --- The probe fails: explain and exit 0, writing nothing. Building without
#     mold is fine, just slower — so this must never be fatal, and must never
#     leave a config behind that would break the next build.
rm -f "$CONFIG"
run env CC=/bin/false
expect_rc "probe fails: exits 0, not fatal" 0
check     "probe fails: says the config was not enabled" "was NOT enabled"
if [ -f "$CONFIG" ]; then
  fail "probe fails: wrote a config anyway — this is the #428 shape"
else
  pass "probe fails: writes no config"
fi

# --- A compiler that can drive mold: config written, and `linker` pinned to
#     the driver that passed the probe. Without the pin, cargo links through
#     plain `cc` — which is a different driver whenever CC was overridden,
#     and following this script's own `CC=gcc-12` advice would then break
#     every build.
rm -f "$CONFIG"
run env "PATH=$WORK/bin:$PATH" CC=mold-capable-cc
expect_rc "probe passes: exits 0" 0
check     "probe passes: reports the pin" "Pinned linker"
if [ -f "$CONFIG" ]; then
  pass "probe passes: writes the config"
  # Status captured off the heredoc, not left to `set -e`. A failing assert
  # inside python would otherwise kill this harness before it could report —
  # which is how the two mutations that should have failed here came back
  # green the first time I checked.
  toml_rc=0
  toml_err=$(python3 - "$CONFIG" <<'PY' 2>&1
import sys, tomllib
d = tomllib.load(open(sys.argv[1], "rb"))
t = d.get("target", {}).get("x86_64-unknown-linux-gnu")
assert t is not None, "no [target.x86_64-unknown-linux-gnu] table"
linker = t.get("linker")
assert linker is not None, f"no linker key under the target table: {t}"
assert linker.endswith("mold-capable-cc"), f"linker is not the probed driver: {linker}"
assert any("mold" in f for f in t.get("rustflags", [])), f"rustflags lost: {t}"
PY
  ) || toml_rc=$?
  if [ "$toml_rc" -eq 0 ]; then
    pass "probe passes: linker lands INSIDE the target table, rustflags intact"
  else
    fail "probe passes: $(tail -1 <<<"$toml_err")"
  fi
else
  fail "probe passes: wrote no config"
fi

# --- Idempotent: a second run must leave the first alone rather than
#     appending a second `linker` line.
before=$(cat "$CONFIG")
run env "PATH=$WORK/bin:$PATH" CC=mold-capable-cc
expect_rc "second run: exits 0" 0
check     "second run: says it left the file alone" "already exists"
if [ "$before" = "$(cat "$CONFIG")" ]; then
  pass "second run: config unchanged"
else
  fail "second run: config was modified"
fi

# --- The default path must not pin anything: with CC unset the probe uses the
#     same `cc` cargo will, so a `linker` key would be noise at best.
#
#     Shimmed as `cc` rather than just unsetting CC, so the probe actually
#     succeeds and the no-pin assertion means something. Without the shim
#     this reduces to "no config was written, therefore no pin" on any box
#     without mold — true, and vacuous.
cp "$WORK/bin/mold-capable-cc" "$WORK/bin/cc"
rm -f "$CONFIG"
# `-u CC` because a dev box may well export it, which would silently turn
# this into a second copy of the case above.
run env -u CC "PATH=$WORK/bin:$PATH"
expect_rc "default CC: exits 0" 0
if [ ! -f "$CONFIG" ]; then
  fail "default CC: probe should have passed through the cc shim, but no config was written"
elif grep -q "^linker" "$CONFIG"; then
  fail "default CC: pinned a linker it did not need to"
else
  pass "default CC: config written, no linker pin (probe used the driver cargo will)"
fi

echo
if [ "$failures" -gt 0 ]; then
  echo "$failures check(s) failed."
  exit 1
fi
echo "All checks passed."
