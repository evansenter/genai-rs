#!/usr/bin/env bash
# Probes the published OpenAPI spec URLs, the `source` each committed spec
# snapshot records, and logs what each serves. When one serves an OpenAPI
# document, it says so once on the issue given as its first argument, while
# that issue is open. The daily api-surface-sweep runs it.
#
# Usage: .github/scripts/watch_spec_urls.sh <issue number> <spec snapshot>...
#
#   .github/scripts/watch_spec_urls.sh 511 \
#       tests/fixtures/api_surface/openapi-v1beta.json \
#       tests/fixtures/api_surface/openapi-v1.json
#
# A URL that answers with anything but an OpenAPI document (a 404, a page
# that is not JSON, a timeout) is the state the issue tracks: the script
# logs it and exits 0. A snapshot whose source is not a URL (`manual`) is
# skipped. When a URL serves, the script comments on the issue with the URL,
# where it redirected, and the commands that refresh the snapshot from it,
# unless the issue is closed or a comment already carries that URL's
# marker, so it comments once per URL rather than every morning.
#
# Exit status: 0 when the probe ran, whatever it found; 1 when a `gh` call
# failed; 2 on bad usage or a snapshot with no source to read.
#
# Needs python3, jq and gh (GH_TOKEN with issues: write).

set -euo pipefail

usage() {
    awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0" >&2
}

if [ $# -lt 2 ]; then
    usage
    exit 2
fi
issue=$1
shift
case "$issue" in
    '' | *[!0-9]*)
        echo "not an issue number: $issue" >&2
        exit 2
        ;;
esac

# One line per URL that serves: source URL, final URL, the spec's
# `info.version`, its path count and the snapshot's API version, tab-separated.
serving=()
for snapshot in "$@"; do
    if ! meta=$(python3 -I -c '
import json, sys
with open(sys.argv[1], encoding="utf-8") as f:
    snapshot = json.load(f)
print(snapshot["source"] + "\t" + snapshot.get("version", ""))
' "$snapshot" 2>/dev/null); then
        echo "cannot read a source URL from $snapshot" >&2
        exit 2
    fi
    url=${meta%%$'\t'*}
    api_version=${meta#*$'\t'}
    case "$url" in
        http://* | https://*) ;;
        *)
            echo "$snapshot: its source ($url) is not a URL; skipped"
            continue
            ;;
    esac
    # `timeout=` so a connection that is accepted and then stalls fails the
    # probe instead of hanging the job.
    result=$(python3 -I - "$url" <<'EOF'
import json, sys, urllib.error, urllib.request

url = sys.argv[1]
request = urllib.request.Request(url, headers={"User-Agent": "genai-rs-spec-watch"})
try:
    with urllib.request.urlopen(request, timeout=30) as response:
        final, body = response.geturl(), response.read()
except urllib.error.HTTPError as e:
    print(f"dead\tHTTP {e.code} at {e.geturl()}")
    sys.exit(0)
except (urllib.error.URLError, OSError) as e:
    print(f"dead\t{e}")
    sys.exit(0)
try:
    spec = json.loads(body)
except ValueError:
    print(f"dead\tnot JSON, at {final}")
    sys.exit(0)
if not (isinstance(spec, dict) and "openapi" in spec and isinstance(spec.get("paths"), dict)):
    print(f"dead\tJSON but not an OpenAPI document, at {final}")
    sys.exit(0)
info = spec.get("info") if isinstance(spec.get("info"), dict) else {}
print(f"serves\t{final}\t{info.get('version', '?')}\t{len(spec['paths'])}")
EOF
)
    case "$result" in
        serves$'\t'*)
            echo "$url serves an OpenAPI document: ${result#serves$'\t'}"
            serving+=("$url"$'\t'"${result#serves$'\t'}"$'\t'"$api_version")
            ;;
        *)
            echo "$url does not serve the spec: ${result#dead$'\t'}"
            ;;
    esac
done

if [ ${#serving[@]} -eq 0 ]; then
    echo "No published spec URL serves an OpenAPI document; #$issue stays as it is."
    exit 0
fi

# Assigned, not inlined into a condition, so a failed call trips `set -e`.
view=$(gh issue view "$issue" --json state,comments)
state=$(jq -r .state <<<"$view")
if [ "$state" != "OPEN" ]; then
    echo "#$issue is $state; not commenting."
    exit 0
fi
comments=$(jq -r '.comments[].body' <<<"$view")

body=$(mktemp)
trap 'rm -f "$body"' EXIT
announced=0
{
    echo "A published OpenAPI spec URL serves an OpenAPI document again (checked $(date -u +%Y-%m-%d) by the API surface sweep):"
    echo
} >"$body"
refresh=()
for entry in "${serving[@]}"; do
    IFS=$'\t' read -r url final version paths api_version <<<"$entry"
    marker="<!-- spec-url-serves: $url -->"
    if grep -qF -- "$marker" <<<"$comments"; then
        echo "#$issue already has the comment for $url."
        continue
    fi
    announced=$((announced + 1))
    {
        if [ "$final" = "$url" ]; then
            echo "- $url: \`info.version\` $version, $paths paths"
        else
            echo "- $url, served from $final: \`info.version\` $version, $paths paths"
        fi
        echo "$marker"
    } >>"$body"
    flag=""
    case "$api_version" in
        v1beta | v1) flag="--spec-$api_version" ;;
    esac
    refresh+=("curl -fsSL -o spec-$api_version.json '$url'"
        ".github/scripts/refresh_api_snapshot.sh ${flag:-"--spec-<version>"} spec-$api_version.json --spec-source '$url'")
done
if [ "$announced" -eq 0 ]; then
    exit 0
fi
{
    echo
    echo "Refresh the committed snapshot (\`tests/fixtures/api_surface/\`) from it, then run"
    echo "the coverage oracle, update the URL's record in \`docs/INTERACTIONS_API_GAP.md\`"
    echo "(\"How to verify API surface\"), and close this issue:"
    echo
    echo '```sh'
    printf '%s\n' "${refresh[@]}"
    echo "cargo nextest run -E 'test(/coverage_oracle/) | binary(request_wire_oracle) | binary(ci_coverage)'"
    echo '```'
    echo
    echo "---"
    echo "_Generated by [Claude Code](https://claude.ai/code)_"
} >>"$body"
gh issue comment "$issue" --body-file "$body"
echo "Commented on #$issue: $announced URL(s) serve the spec."
