#!/usr/bin/env python3
"""Parse-check the shell in every workflow `run:` block.

Workflow shell is the least-tested code in the repo. A `run:` body is
never parsed until the step executes, so a quoting bug in a job that only
runs on a schedule — or only on the branch that changed — surfaces days
later as a failed cron email rather than as a red check on the pull
request that introduced it.

That is not hypothetical: an unterminated double quote shipped in the API
surface sweep's report block, on the one path the job exists for, and the
no-op path it was verified against exited before bash ever parsed it.

`bash -n` parses without executing, so this is a syntax gate only. It does
not know what the commands mean, and it will not catch an unset variable,
a wrong flag, or a command that fails at runtime. It catches exactly the
class of defect above, which is the class that survives review.

It also parse-checks the standalone scripts under `.github/scripts/`,
which are invoked only from schedule-only jobs and carry the same latency,
and the `run:` steps of the composite actions under `.github/actions/`
(`runs.steps`), which run wherever a job uses them — including a
schedule-only job (#457).

A repo script a `run:` block runs directly (`.github/scripts/x.sh args`,
not `bash .github/scripts/x.sh`) must be executable: committed without the
bit, it fails with exit 126 only when the step runs, which for a
schedule-only workflow is days later (the flake counter shipped that way,
#492).

Limits worth stating, so a green check is not read as broader coverage than
it is:

- GitHub substitutes `${{ }}` expressions into the `run:` body textually,
  *before* bash parses it. This checks the template, not the script that
  runs. A block echoing an expression that holds a PR title parses clean
  here and still dies at runtime on a title containing a double quote —
  the same tokenization failure, injected at expansion time rather than
  authored in.
- `sh` steps are parsed with `bash -n`, which accepts bash-only syntax
  (`[[ ]]`, process substitution, arrays) that dash rejects at runtime.
  Nothing in this repo sets `shell: sh` today, so it is not reachable —
  but a pass on such a step is weaker than it looks.
- Only `.github/workflows/*.y*ml`, `.github/actions/*/action.y*ml` and
  `.github/scripts/*.sh` are scanned. A shell script living anywhere else
  (`scripts/`, the harnesses under `.github/scripts/tests/`) is covered by
  `make test-scripts`'s shellcheck instead, and a composite action nested
  deeper than `.github/actions/<name>/` is not covered.
- A step with no explicit `shell:` in a job whose matrix includes Windows
  runs under pwsh on the Windows leg and bash elsewhere. Such steps are
  still parsed as bash — all of them are plain `cargo ...` invocations
  today — and the summary line counts them, so the assumption is stated
  rather than hidden.

Exit status: 0 when every block parses and every script run directly is
executable, 1 otherwise.
"""

from __future__ import annotations

import glob
import os
import re
import subprocess
import sys
import tempfile

import yaml

# A repo script path in a `run:` block, at a command position or anywhere a
# word starts. Whether it is run directly is decided by the word before it.
SCRIPT_PATH = re.compile(r"(?:^|(?<=[\s;&|(`]))((?:\./)?(?:\.github|scripts)/[\w./-]+\.(?:sh|py))")
# Words that run a script themselves, so its own mode does not matter.
INTERPRETERS = {"bash", "sh", "zsh", "python", "python3", "source", ".", "exec"}

# `run:` defaults to bash on Linux; other explicit interpreters are skipped.
# Matched on the leading word, so `bash -euo pipefail {0}` stays covered.
SHELLS = {"bash", "sh"}


def _explicit_default_shell(document: dict, job: dict) -> str | None:
    """A `defaults.run.shell` from the job, else the workflow, else None.

    Both scopes matter: a workflow-level `defaults:` block sets the shell
    for every job, and reading only the job level would miss it.
    """
    for scope in (job, document):
        shell = ((scope.get("defaults") or {}).get("run") or {}).get("shell")
        if shell:
            return str(shell)
    return None


def _may_run_on_windows(job: dict) -> bool:
    """Whether any leg of this job lands on a Windows image.

    GitHub's implicit `run:` shell is `pwsh` on Windows and `bash`
    elsewhere, so a step with no explicit `shell:` in such a job is not
    bash on every leg. The matrix has to be consulted, not just `runs-on` —
    the cross-platform job is `runs-on: ${{ matrix.os }}`, which says
    nothing on its own while its matrix includes `windows-latest`.
    """
    haystack = [str(job.get("runs-on", ""))]
    matrix = (job.get("strategy") or {}).get("matrix") or {}
    haystack.append(str(matrix))
    return any("windows" in part.lower() for part in haystack)


def iter_run_steps(path: str):
    """Yield (job, index, name, run, assumed_bash) per shell `run:` block.

    `assumed_bash` marks a step whose shell is implicit in a job that can
    land on Windows — bash on some legs, pwsh on others. Those are still
    parsed as bash rather than skipped: every one of them today is a plain
    `cargo ...` invocation, and dropping them would trade real coverage for
    a hypothetical. The flag exists so `check` can say so out loud, and so
    that a genuinely PowerShell-only step added there fails with an
    explanation of what to add (`shell: pwsh`) rather than a bare parse
    error.
    """
    with open(path, encoding="utf-8") as handle:
        document = yaml.safe_load(handle)

    for job_name, job in (document.get("jobs") or {}).items():
        # Reusable-workflow calls (`uses:` at job level) have no steps.
        explicit_default = _explicit_default_shell(document, job)
        windows = explicit_default is None and _may_run_on_windows(job)

        for index, step in enumerate(job.get("steps") or []):
            run = step.get("run")
            if not run:
                continue

            shell = step.get("shell", explicit_default)
            assumed_bash = shell is None and windows
            if shell is None:
                shell = "bash"

            if str(shell).split(maxsplit=1)[0] not in SHELLS:
                continue
            yield job_name, index, step.get("name", "(unnamed)"), run, assumed_bash


def iter_action_run_steps(path: str):
    """Yield the same tuples as `iter_run_steps` for a composite action.

    A composite action's steps sit under `runs.steps`, not `jobs.*.steps`,
    and each `run:` step must name its `shell:` (GitHub rejects a composite
    step without one), so nothing is assumed. The "job" is `runs`. Other
    action kinds (`docker`, `node20`) have no shell steps.
    """
    with open(path, encoding="utf-8") as handle:
        document = yaml.safe_load(handle) or {}
    runs = document.get("runs") or {}
    if runs.get("using") != "composite":
        return
    for index, step in enumerate(runs.get("steps") or []):
        run = step.get("run")
        if not run:
            continue
        shell = str(step.get("shell") or "bash")
        if shell.split(maxsplit=1)[0] not in SHELLS:
            continue
        yield "runs", index, step.get("name", "(unnamed)"), run, False


def directly_run_scripts(run: str) -> list[str]:
    """The repo scripts `run` executes as commands rather than through an
    interpreter. Comment lines are skipped, and flags between an interpreter
    and the path (`python3 -I x.py`) are passed over."""
    found = []
    for line in run.splitlines():
        if line.lstrip().startswith("#"):
            continue
        for match in SCRIPT_PATH.finditer(line):
            words = line[: match.start(1)].split()
            while words and words[-1].startswith("-"):
                words.pop()
            if words and words[-1].lstrip("$(`") in INTERPRETERS:
                continue
            found.append(match.group(1))
    return found


def check_modes(path: str, job_name: str, step_name: str, run: str) -> int:
    """Fails each existing repo script `run` executes directly without the
    executable bit. Returns the failure count."""
    failures = 0
    for script in directly_run_scripts(run):
        if os.path.isfile(script) and not os.access(script, os.X_OK):
            failures += 1
            print(
                f"::error file={path}::{job_name} / {step_name}: {script} is run "
                "directly but is not executable (git update-index --chmod=+x)"
            )
            print(f"FAIL {path} :: {job_name} :: {step_name}: {script} is not executable")
    return failures


def check(path: str, steps=iter_run_steps) -> tuple[int, int, int]:
    """Returns (blocks checked, blocks assumed bash, failures)."""
    checked = 0
    assumed = 0
    failures = 0
    for job_name, index, step_name, run, assumed_bash in steps(path):
        checked += 1
        if assumed_bash:
            assumed += 1
        failures += check_modes(path, job_name, step_name, run)
        handle = tempfile.NamedTemporaryFile(
            "w", suffix=".sh", delete=False, encoding="utf-8"
        )
        try:
            handle.write(run)
            handle.close()
            result = subprocess.run(
                ["bash", "-n", handle.name],
                capture_output=True,
                text=True,
                check=False,
            )
        finally:
            os.unlink(handle.name)

        if result.returncode != 0:
            failures += 1
            # `::error::` renders inline on the workflow run summary.
            detail = result.stderr.strip().replace("\n", " ")
            print(f"::error file={path}::{job_name} / {step_name}: {detail}")
            print(f"FAIL {path} :: {job_name} :: steps[{index}] {step_name}")
            for line in result.stderr.strip().splitlines():
                print(f"     {line}")
            if assumed_bash:
                print(
                    "     NOTE: this step has no explicit `shell:` and its job "
                    "can run on Windows, where the implicit shell is pwsh. If "
                    "this body is PowerShell, add `shell: pwsh` to the step."
                )
    return checked, assumed, failures


def check_script(path: str) -> int:
    """Parse-checks a standalone shell script. Returns the failure count."""
    result = subprocess.run(
        ["bash", "-n", path], capture_output=True, text=True, check=False
    )
    if result.returncode == 0:
        return 0

    detail = result.stderr.strip().replace("\n", " ")
    print(f"::error file={path}::{detail}")
    print(f"FAIL {path}")
    for line in result.stderr.strip().splitlines():
        print(f"     {line}")
    return 1


def main() -> int:
    paths = sorted(
        glob.glob(".github/workflows/*.yml") + glob.glob(".github/workflows/*.yaml")
    )
    if not paths:
        print("::error::No workflow files found — is this running from the repo root?")
        return 1

    checked = 0
    assumed = 0
    failures = 0
    for path in paths:
        path_checked, path_assumed, path_failures = check(path)
        checked += path_checked
        assumed += path_assumed
        failures += path_failures

    # Composite actions: their steps run inside whichever job uses them.
    actions = sorted(
        glob.glob(".github/actions/*/action.yml")
        + glob.glob(".github/actions/*/action.yaml")
    )
    action_checked = 0
    for action in actions:
        path_checked, _, path_failures = check(action, iter_action_run_steps)
        action_checked += path_checked
        failures += path_failures

    # Scripts too: they have the same discovery latency. Counted separately, so
    # the inert-gate check below still fires if run-block extraction yields
    # nothing.
    scripts = sorted(glob.glob(".github/scripts/*.sh"))
    script_checked = 0
    for script in scripts:
        script_checked += 1
        failures += check_script(script)

    total = checked + action_checked + script_checked

    if failures:
        print(f"\n{failures} problem(s) in {total} shell block(s).")
        return 1

    # Fail on zero: an inert gate must not look like a pass.
    if checked == 0:
        print("::error::No workflow `run:` blocks were checked — the gate is inert.")
        return 1
    if script_checked == 0:
        print("::error::No scripts were checked — the glob matched nothing.")
        return 1

    summary = (
        f"All {total} shell blocks parse "
        f"({checked} workflow `run:` block(s) across {len(paths)} file(s), "
        f"{action_checked} composite action step(s) across {len(actions)} "
        f"action(s), {script_checked} script(s))"
    )
    print(
        f"{summary}; {assumed} assumed bash on a Windows-capable job."
        if assumed
        else f"{summary}."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
