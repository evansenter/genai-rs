#!/usr/bin/env bash
# Enables the opt-in fast-linker config, if the linker is actually present.
#
# `.cargo/config.toml` used to be checked in, which made every build on a
# machine without mold fail before compiling anything — with an error naming
# `ld`, a binary that *is* installed, rather than mold, which is not (#428).
# So the config is now an example, and this script is the safe way to enable
# it: it checks first and explains rather than leaving a landmine.
#
# Idempotent. Safe to run on a machine with no mold — it says so and exits 0,
# because building without mold is fine, just slower.

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

EXAMPLE=".cargo/config.toml.example"
CONFIG=".cargo/config.toml"

if [ ! -f "$EXAMPLE" ]; then
  echo "error: $EXAMPLE not found — run this from a clone of the repo." >&2
  exit 1
fi

if [ -f "$CONFIG" ]; then
  echo "$CONFIG already exists; leaving it alone."
  echo "Delete it and re-run if you want the current example instead."
  exit 0
fi

# `command -v mold` is the obvious check and it is the wrong one.
# `-fuse-ld=mold` is a compiler-driver flag, and GCC only learned it in 12.1
# (clang: 12+). On Ubuntu 22.04 LTS — default gcc 11.4, supported into 2027 —
# "apt install mold", the very command this script recommends, puts mold on
# PATH, so a PATH check passes, the config gets written, and every later
# cargo build dies at link time. That is the #428 failure mode reached
# through the script that exists to prevent it.
#
# So ask the compiler the question rustc will ask it: link something. That
# covers both conditions at once — missing mold and a driver too old to
# invoke it both come back as a failed link — and needs no version table.
CC=${CC:-cc}
probe_dir=$(mktemp -d)
trap 'rm -rf "$probe_dir"' EXIT
printf 'int main(void) { return 0; }\n' >"$probe_dir/probe.c"

if ! command -v "$CC" >/dev/null 2>&1; then
  probe_ok=0
  probe_err="$CC: not found (set CC to your compiler driver)"
elif probe_err=$("$CC" -fuse-ld=mold "$probe_dir/probe.c" -o "$probe_dir/probe" 2>&1); then
  probe_ok=1
else
  probe_ok=0
fi

if [ "$probe_ok" -eq 0 ]; then
  cat <<MSG
The fast-linker config was NOT enabled: "$CC -fuse-ld=mold" could not link a
trivial program, so cargo would have failed the same way at link time.

$(printf '%s\n' "$probe_err" | head -n 3 | sed 's/^/  /')

That is fine — builds work without it, just slower. Two things it can be:

  1. mold is not installed:

       Debian/Ubuntu   sudo apt install mold
       Fedora          sudo dnf install mold
       Arch            sudo pacman -S mold
       Nix             nix profile install nixpkgs#mold

  2. mold is installed but your compiler cannot drive it. -fuse-ld=mold
     needs GCC 12.1+ or clang 12+, and Ubuntu 22.04 still defaults to
     gcc 11. Install a newer one and re-run as e.g. CC=gcc-12 $0

Re-run this script afterwards. (No macOS entry: mold's macOS port, sold, was
discontinued upstream, and the config is x86_64-linux-only regardless.)
MSG
  exit 0
fi

cp "$EXAMPLE" "$CONFIG"
echo "Enabled the mold linker via $CONFIG (git-ignored)."
echo "Delete that file to go back to the default linker."

# The config only sets [target.x86_64-unknown-linux-gnu], so on any other
# host it is inert. Saying "enabled" without this would be a claim the
# config cannot back: someone on aarch64 installs mold, gets congratulated,
# and sees no change with nothing to explain why.
# `|| true` is load-bearing under `set -euo pipefail`: with no rustc on PATH
# the substitution exits 127, `pipefail` carries that past the successful
# `sed`, and a simple assignment takes `set -e` — so the script would die
# here, silently and non-zero, *after* announcing success two lines up. That
# is reachable on a fresh container where you install mold, clone, and run
# this before sourcing the rustup env. The `-n "$HOST"` guard below already
# encodes the intent: no answer means skip the note.
HOST=$(rustc -vV 2>/dev/null | sed -n 's/^host: //p' || true)
if [ -n "$HOST" ] && [ "$HOST" != "x86_64-unknown-linux-gnu" ]; then
  echo
  echo "Note: your host is $HOST, and the config only covers"
  echo "x86_64-unknown-linux-gnu — so builds will not actually use mold."
fi
