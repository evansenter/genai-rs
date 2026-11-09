#!/usr/bin/env bash
# Watches the published OpenAPI specs against the committed snapshots the
# coverage oracle reads. For each snapshot it fetches the `source` URL the
# snapshot records, reduces what is served with snapshot_openapi.py (with
# the snapshot's own source and fetch date, so only the spec can differ),
# and compares the result with the committed file. A spec that drifted, or
# a URL that stops serving one, opens or updates one rolling issue, as the
# bindings sweep does when the SDK moves; a run that finds every spec
# matching closes it. The daily api-surface-sweep runs it.
#
# Usage: .github/scripts/watch_spec_urls.sh <spec snapshot>...
#
#   .github/scripts/watch_spec_urls.sh \
#       tests/fixtures/api_surface/openapi-v1beta.json \
#       tests/fixtures/api_surface/openapi-v1.json
#
# A URL that does not serve an OpenAPI document (an HTTP error, a timeout,
# a response cut off mid-body, a URL the HTTP client rejects, a page that is
# not JSON, JSON that is not a spec) is tried twice,
# SPEC_WATCH_RETRY_DELAY seconds apart (default 10), before it counts as
# dead. A snapshot whose source is not a URL (`manual`) is skipped. The
# issue is found by its exact title and the `api-sweep` label, closed
# included, so a closed one reopens rather than a duplicate being filed.
# Its body is rewritten on every run with a finding; a comment is added
# only when the findings changed (a marker in the body records them), so a
# URL that stays dead does not comment every morning.
#
# Exit status: 0 when the watch ran, whatever it found; 1 when a `gh` call
# failed; 2 on bad usage or a snapshot with no source or fetch date to read.
#
# Needs python3, jq and gh (GH_TOKEN with issues: write).

set -euo pipefail

usage() {
    awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0" >&2
}

if [ $# -lt 1 ]; then
    usage
    exit 2
fi

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
title='API surface sweep: a published OpenAPI spec drifted or stopped serving'
label='api-sweep'
retry_delay=${SPEC_WATCH_RETRY_DELAY:-10}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Fetches a URL into a file. Prints `serves<TAB>final URL` or
# `dead<TAB>reason`.
probe() {
    python3 -I - "$1" "$2" <<'EOF'
import http.client, json, sys, urllib.error, urllib.request

url, out = sys.argv[1], sys.argv[2]
# `timeout=` so a connection that is accepted and then stalls fails the
# probe instead of hanging the job.
try:
    request = urllib.request.Request(url, headers={"User-Agent": "genai-rs-spec-watch"})
    with urllib.request.urlopen(request, timeout=30) as response:
        final, body = response.geturl(), response.read()
except urllib.error.HTTPError as e:
    print(f"dead\tHTTP {e.code} at {e.geturl()}")
    sys.exit(0)
# A connection that fails, a response cut off mid-body (IncompleteRead is an
# HTTPException, not an OSError) and a URL the client rejects (ValueError)
# all mean the URL serves no spec: reported, never a crashed run.
except (urllib.error.URLError, OSError, http.client.HTTPException, ValueError) as e:
    print(f"dead\t{type(e).__name__}: {e}")
    sys.exit(0)
try:
    spec = json.loads(body)
except ValueError:
    print(f"dead\tnot JSON, at {final}")
    sys.exit(0)
if not (isinstance(spec, dict) and "openapi" in spec and isinstance(spec.get("paths"), dict)):
    print(f"dead\tJSON but not an OpenAPI document, at {final}")
    sys.exit(0)
with open(out, "wb") as f:
    f.write(body)
print(f"serves\t{final}")
EOF
}

# One section per finding, appended to $work/findings.md; one line per
# finding's identity (URL and what it found) to $work/keys, for the marker.
: >"$work/findings.md"
: >"$work/keys"
refresh=()
n=0
for snapshot in "$@"; do
    n=$((n + 1))
    if ! meta=$(python3 -I -c '
import json, sys
with open(sys.argv[1], encoding="utf-8") as f:
    snapshot = json.load(f)
print("\t".join([snapshot["source"], snapshot["fetched"], snapshot.get("version") or ""]))
' "$snapshot" 2>/dev/null); then
        echo "cannot read a source URL and fetch date from $snapshot" >&2
        exit 2
    fi
    IFS=$'\t' read -r url fetched api_version <<<"$meta"
    case "$url" in
        http://* | https://*) ;;
        *)
            echo "$snapshot: its source ($url) is not a URL; skipped"
            continue
            ;;
    esac
    served="$work/served-$n.json"
    result=$(probe "$url" "$served")
    if [ "${result%%$'\t'*}" = dead ] && [ "$retry_delay" != 0 ]; then
        sleep "$retry_delay"
    fi
    if [ "${result%%$'\t'*}" = dead ]; then
        result=$(probe "$url" "$served")
    fi
    if [ "${result%%$'\t'*}" = dead ]; then
        reason=${result#dead$'\t'}
        echo "$url does not serve the spec: $reason"
        printf '%s dead\n' "$url" >>"$work/keys"
        {
            echo "### $url stopped serving the spec"
            echo
            echo "It answered: $reason. \`$snapshot\` was fetched from it on $fetched."
            echo "Find where the spec moved, or record that it is gone, in"
            echo "\`docs/INTERACTIONS_API_GAP.md\` (\"How to verify API surface\")."
            echo
        } >>"$work/findings.md"
        continue
    fi
    final=${result#serves$'\t'}
    reduced="$work/reduced-$n.json"
    if ! python3 -I "$script_dir/snapshot_openapi.py" "$served" "$url" "$fetched" \
        >"$reduced" 2>"$work/reduce.err"; then
        reason=$(tail -n 1 "$work/reduce.err")
        echo "$url serves a spec snapshot_openapi.py cannot reduce: $reason"
        printf '%s unreadable\n' "$url" >>"$work/keys"
        {
            echo "### $url serves a spec the snapshot script cannot read"
            echo
            echo "\`snapshot_openapi.py\` failed on it: $reason"
            echo
        } >>"$work/findings.md"
        continue
    fi
    if cmp -s "$reduced" "$snapshot"; then
        echo "$url serves the committed snapshot ($snapshot)."
        continue
    fi
    echo "$url has drifted from $snapshot."
    printf '%s drift %s\n' "$url" "$(cksum <"$reduced")" >>"$work/keys"
    # `|| [ $? -eq 1 ]`: diff exits 1 for "differences found".
    diff -u --label "$snapshot (committed)" --label "$url (served)" \
        "$snapshot" "$reduced" >"$work/diff" || [ $? -eq 1 ]
    {
        if [ "$final" = "$url" ]; then
            echo "### $url has drifted from \`$snapshot\`"
        else
            echo "### $url (served from $final) has drifted from \`$snapshot\`"
        fi
        echo
        echo "Reduced with \`snapshot_openapi.py\`, as the committed copy was:"
        echo
        echo '<details><summary>Diff</summary>'
        echo
        # Tilde fence: a backtick run in the spec cannot close it early.
        echo '~~~diff'
        head -c 20000 "$work/diff"
        if [ "$(wc -c <"$work/diff")" -gt 20000 ]; then
            echo
            echo "... truncated; run the refresh below to see the rest ..."
        fi
        echo '~~~'
        echo
        echo '</details>'
        echo
    } >>"$work/findings.md"
    flag=""
    case "$api_version" in
        v1beta | v1) flag="--spec-$api_version" ;;
    esac
    refresh+=("curl -fsSL -o spec-$n.json '$url'"
        ".github/scripts/refresh_api_snapshot.sh ${flag:-"--spec-<version>"} spec-$n.json --spec-source '$url'")
done

# `--state all`: a closed issue reopens rather than a duplicate being filed.
# Assigned, not inlined into a condition, so a failed call trips `set -e`.
issues=$(gh issue list --label "$label" --state all --limit 100 --json number,title,state)
existing=$(jq -r --arg title "$title" \
    '[.[] | select(.title == $title)] | first // empty | "\(.number)\t\(.state)"' <<<"$issues")

if [ ! -s "$work/keys" ]; then
    echo "Every published spec matches its committed snapshot."
    if [ -n "$existing" ] && [ "${existing#*$'\t'}" = OPEN ]; then
        number=${existing%%$'\t'*}
        cat >"$work/close.md" <<'EOF'
Every published OpenAPI spec serves again and matches its committed snapshot, so this is closed. It reopens on the next drift or dead URL.

---
_Generated by [Claude Code](https://claude.ai/code)_
EOF
        gh issue comment "$number" --body-file "$work/close.md"
        gh issue close "$number" --reason completed
        echo "Closed #$number."
    fi
    exit 0
fi

marker="<!-- spec-watch: $(sort "$work/keys" | cksum | tr -d ' \t') -->"
{
    echo "## A published OpenAPI spec no longer matches its snapshot"
    echo
    echo "The daily API surface sweep (\`.github/scripts/watch_spec_urls.sh\`) reduces"
    echo "each spec its snapshot in \`tests/fixtures/api_surface/\` was fetched from"
    echo "and compares the two. Checked $(date -u +%Y-%m-%d):"
    echo
    cat "$work/findings.md"
    if [ ${#refresh[@]} -gt 0 ]; then
        echo "### Next steps"
        echo
        echo "Refresh the drifted snapshot from its URL, run the coverage oracle (a new"
        echo "field, schema or endpoint fails there until it is handled or listed with a"
        echo "reason), and update the URL's record in \`docs/INTERACTIONS_API_GAP.md\`"
        echo "(\"How to verify API surface\"). The next clean run closes this issue."
        echo
        echo '```sh'
        printf '%s\n' "${refresh[@]}"
        echo "cargo nextest run -E 'test(/coverage_oracle/) | binary(request_wire_oracle) | binary(ci_coverage)'"
        echo '```'
        echo
    fi
    echo "---"
    echo "_Generated by [Claude Code](https://claude.ai/code)_"
    echo
    echo "$marker"
} >"$work/body.md"
cat "$work/body.md"

if [ -z "$existing" ]; then
    gh label create "$label" --description "New upstream API surface detected" \
        --color "1d76db" 2>/dev/null || true
    gh issue create --title "$title" --label "$label" --body-file "$work/body.md"
    exit 0
fi
number=${existing%%$'\t'*}
view=$(gh issue view "$number" --json body)
previous=$(jq -r .body <<<"$view")
if [ "${existing#*$'\t'}" != OPEN ]; then
    gh issue reopen "$number"
fi
gh issue edit "$number" --body-file "$work/body.md"
if grep -qF -- "$marker" <<<"$previous"; then
    echo "#$number already reports these findings; body refreshed, no comment."
else
    cat >"$work/comment.md" <<'EOF'
Updated: the published OpenAPI specs differ from their snapshots in a new way. The issue body above has the current findings.

---
_Generated by [Claude Code](https://claude.ai/code)_
EOF
    gh issue comment "$number" --body-file "$work/comment.md"
    echo "Updated #$number."
fi
