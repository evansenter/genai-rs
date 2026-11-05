#!/usr/bin/env bash
# Exercises `check_workflow_shell.py` in a temp tree (#457).
#
# The checker globs paths relative to the working directory, so each case
# builds a small `.github/` and runs the real script from inside it: a
# composite action's `runs.steps`, which it used not to read, and a
# workflow, to show both paths still fail on what they exist to catch.
#
# The checker needs PyYAML. Without it this harness skips with a named
# message, except under CI (`CI=true`), where a skip would let the gate go
# unexercised, so it fails instead.

set -euo pipefail

command -v python3 >/dev/null 2>&1 || {
  echo "python3 is required to run this harness (brew install python3 / apt install python3)" >&2
  exit 1
}
if ! python3 -c 'import yaml' 2>/dev/null; then
  if [ "${CI:-}" = "true" ]; then
    echo "FAIL - PyYAML is not installed, so check_workflow_shell.py cannot run (pip install pyyaml)" >&2
    exit 1
  fi
  echo "skip - PyYAML is not installed (pip install pyyaml); check_workflow_shell.py needs it"
  exit 0
fi

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$(dirname "$HERE")/check_workflow_shell.py"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

failures=0

# A minimal tree that passes: one workflow, one script, one composite action.
make_tree() {
  local dir="$1"
  mkdir -p "$dir/.github/workflows" "$dir/.github/scripts" "$dir/.github/actions/demo"
  cat > "$dir/.github/workflows/ci.yml" <<'YAML'
name: CI
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - run: echo "built"
YAML
  printf '#!/usr/bin/env bash\necho ok\n' > "$dir/.github/scripts/tool.sh"
  cat > "$dir/.github/actions/demo/action.yml" <<'YAML'
name: Demo
runs:
  using: composite
  steps:
    - name: Greet
      shell: bash
      run: |
        if [ -n "${RUNNER_OS:-}" ]; then
          echo "on $RUNNER_OS"
        fi
YAML
}

run_checker() {
  rc=0
  out=$(cd "$1" && python3 "$SCRIPT" 2>&1) || rc=$?
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

expect_output() {
  local label="$1" needle="$2"
  if grep -qF -- "$needle" <<<"$out"; then
    echo "ok   - $label"
  else
    echo "FAIL - $label: expected to find '$needle'"
    sed 's/^/         /' <<<"$out"
    failures=$((failures + 1))
  fi
}

# --- A clean tree passes, and the summary counts the composite step.
make_tree "$WORK/clean"
run_checker "$WORK/clean"
expect_rc     "clean tree: passes" 0
expect_output "clean tree: counts the composite action's step" \
  "1 composite action step(s) across 1 action(s)"

# --- An unterminated quote in a composite action's bash step fails, naming
#     the action file and the step.
make_tree "$WORK/broken-action"
cat > "$WORK/broken-action/.github/actions/demo/action.yml" <<'YAML'
name: Demo
runs:
  using: composite
  steps:
    - name: Broken greeting
      shell: bash
      run: echo "unterminated
YAML
run_checker "$WORK/broken-action"
expect_rc     "broken composite step: fails" 1
expect_output "broken composite step: names the action and the step" \
  "FAIL .github/actions/demo/action.yml :: runs :: steps[0] Broken greeting"

# --- A composite step in another shell is not parsed as bash.
make_tree "$WORK/pwsh-action"
cat > "$WORK/pwsh-action/.github/actions/demo/action.yml" <<'YAML'
name: Demo
runs:
  using: composite
  steps:
    - shell: pwsh
      run: |
        Write-Host "not bash $(Get-Date"
YAML
run_checker "$WORK/pwsh-action"
expect_rc     "pwsh composite step: skipped, not parsed as bash" 0
expect_output "pwsh composite step: not counted" "0 composite action step(s)"

# --- A non-composite action has no shell steps to read.
make_tree "$WORK/docker-action"
cat > "$WORK/docker-action/.github/actions/demo/action.yml" <<'YAML'
name: Demo
runs:
  using: docker
  image: Dockerfile
YAML
run_checker "$WORK/docker-action"
expect_rc "docker action: passes with nothing to check" 0

# --- The workflow path still fails on the same defect.
make_tree "$WORK/broken-workflow"
cat > "$WORK/broken-workflow/.github/workflows/ci.yml" <<'YAML'
name: CI
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - name: Broken
        run: echo "unterminated
YAML
run_checker "$WORK/broken-workflow"
expect_rc     "broken workflow step: fails" 1
expect_output "broken workflow step: names the job and step" \
  "FAIL .github/workflows/ci.yml :: build :: steps[0] Broken"

echo
if [ "$failures" -gt 0 ]; then
  echo "$failures check(s) failed."
  exit 1
fi
echo "All checks passed."
