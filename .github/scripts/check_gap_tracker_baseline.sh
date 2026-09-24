#!/usr/bin/env bash
# Asserts the gap tracker's header names the same SDK version CI is swept to.
#
# The version lives in `.github/last-swept-sdk-version` (read by the sweep) and
# in the tracker header (read by people). Bumping only the first clears the
# staleness signal while the header still shows an older snapshot.

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
# Anchored on the label, so a reflowed table fails loudly. The version class
# admits pre-releases like `2.19.0rc1`, so a mismatch reports as a mismatch
# rather than as a parse failure.
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
