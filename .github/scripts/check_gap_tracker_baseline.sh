#!/usr/bin/env bash
# Asserts the gap tracker's header names the same SDK version CI is swept to.
#
# The swept version lives in two places: `.github/last-swept-sdk-version`,
# which the sweep job reads, and the "Last swept against" row in
# `docs/INTERACTIONS_API_GAP.md`, which readers read. Only the first has a
# mechanical consequence, so bumping it without the second produces the
# best-looking wrong state: the sweep issue closes, the staleness signal
# clears, and the header still advertises an older snapshot with nothing
# saying otherwise.
#
# Same idea as `tests/model_literals.rs` — make the duplicated fact fail the
# build rather than rely on a checklist step.

set -euo pipefail

BASELINE_FILE=".github/last-swept-sdk-version"
TRACKER="docs/INTERACTIONS_API_GAP.md"

for file in "$BASELINE_FILE" "$TRACKER"; do
  if [ ! -f "$file" ]; then
    echo "::error::$file not found — run this from the repository root."
    exit 1
  fi
done

BASELINE=$(tr -d '[:space:]' < "$BASELINE_FILE")
if [ -z "$BASELINE" ]; then
  echo "::error::$BASELINE_FILE is empty."
  exit 1
fi

# The row is `| **Last swept against** | \`google-genai\` **2.18.1** |`.
# Anchored on the label so reflowing the table does not silently stop
# matching — an empty capture below is a failure, not a pass.
# The capture allows letters, `+` and `-`, not just digits and dots. PyPI
# reports whatever `info.version` says, pre-releases included, and step 4 of
# the sweep's next-steps tells a maintainer to bump the baseline to exactly
# that string. With a digits-and-dots-only capture, a `2.19.0rc1` baseline
# and a correctly-filled header row would yield an empty `$HEADER` and trip
# the branch below — whose message says the row "has moved or changed shape"
# and sends the reader to fix this script's parsing. The row would be
# exactly where it has always been. The label text is what anchors the
# match; the version pattern only has to be permissive enough to let the
# `!=` comparison below produce the accurate error instead.
HEADER=$(sed -n 's/.*\*\*Last swept against\*\*.*\*\*\([0-9][0-9A-Za-z.+-]*\)\*\*.*/\1/p' "$TRACKER" | head -1)

if [ -z "$HEADER" ]; then
  echo "::error file=$TRACKER::Could not find the 'Last swept against' version."
  echo "The row this check parses has moved or changed shape. Update"
  echo ".github/scripts/check_gap_tracker_baseline.sh to match it."
  exit 1
fi

if [ "$HEADER" != "$BASELINE" ]; then
  echo "::error file=$TRACKER::Gap tracker says $HEADER, $BASELINE_FILE says $BASELINE."
  echo
  echo "These must agree. The sweep workflow reads $BASELINE_FILE, and"
  echo "readers trust the tracker header — bumping one without the other"
  echo "clears the staleness signal while leaving a stale snapshot date."
  echo "Update the 'Last swept against' row (and the sweep date beside it)."
  exit 1
fi

echo "Gap tracker and CI baseline agree: google-genai $BASELINE."
