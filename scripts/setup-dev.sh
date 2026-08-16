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

if ! command -v mold >/dev/null 2>&1; then
  cat <<'MSG'
mold is not installed, so the fast-linker config was NOT enabled.

That is fine — builds work without it, just slower. If you want it:

  Debian/Ubuntu   sudo apt install mold
  Fedora          sudo dnf install mold
  Arch            sudo pacman -S mold
  Nix             nix profile install nixpkgs#mold

then re-run this script. (No macOS entry: mold's macOS port, sold, was
discontinued upstream, and the config below is x86_64-linux-only regardless.)
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
HOST=$(rustc -vV 2>/dev/null | sed -n 's/^host: //p')
if [ -n "$HOST" ] && [ "$HOST" != "x86_64-unknown-linux-gnu" ]; then
  echo
  echo "Note: your host is $HOST, and the config only covers"
  echo "x86_64-unknown-linux-gnu — so builds will not actually use mold."
fi
