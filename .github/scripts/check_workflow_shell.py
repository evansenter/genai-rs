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
which are invoked only from schedule-only jobs and carry the same latency.

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
- Only `.github/workflows/*.y*ml` and `.github/scripts/*.sh` are scanned.
  A shell script living anywhere else is not covered.
- A step with no explicit `shell:` in a job whose matrix includes Windows
  runs under pwsh on the Windows leg and bash elsewhere. Such steps are
  still parsed as bash — all of them are plain `cargo ...` invocations
  today — and the summary line counts them, so the assumption is stated
  rather than hidden.

Exit status: 0 when every block parses, 1 otherwise.
"""

from __future__ import annotations

import glob
import os
import subprocess
import sys
import tempfile

import yaml

# `run:` defaults to bash on Linux runners. Anything explicitly set to
# another interpreter is not ours to parse.
#
# Matched on the leading word rather than the whole value, so custom
# templates like `bash -euo pipefail {0}` — one flag away from the
# documented ones — stay covered. An exact-match allowlist would opt such a
# step out of this gate with nothing in the output saying so.
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


def check(path: str) -> tuple[int, int, int]:
    """Returns (blocks checked, blocks assumed bash, failures)."""
    checked = 0
    assumed = 0
    failures = 0
    for job_name, index, step_name, run, assumed_bash in iter_run_steps(path):
        checked += 1
        if assumed_bash:
            assumed += 1
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

    # The scripts a `run:` block invokes have the same discovery latency as
    # the block itself — `.github/scripts/*.sh` are called only from the
    # daily flakiness report, so a quoting bug in one arrives as a failed
    # cron email. To the loop above they are a one-line command that parses
    # fine, which would make a green check read as broader than it is.
    #
    # This pass is no longer *unique* coverage for those paths. Since this
    # check moved into the `shell-scripts` job, `make test-scripts` runs
    # `shellcheck -S warning` over the same glob in the same job, and
    # shellcheck reports syntax errors too. Kept anyway: it keeps this
    # script meaningful when run on its own, and it keeps the reported
    # block count honest about what was examined. It is redundancy now
    # rather than the only line of defence, and worth knowing as such
    # before treating a green run here as covering these files alone.
    # Counted separately from the workflow blocks above, deliberately. The
    # inertness guard below asks whether the `run:` extraction still yields
    # anything, and there are eight scripts here — so folding both into one
    # total means that if `iter_run_steps` yielded nothing at all (an
    # unexpected YAML shape, a `shell:` value that stops matching SHELLS, a
    # refactor), the count would still be 8, `== 0` would be false, and the
    # summary would report "All 8 shell blocks parse" with the half of the
    # gate the workflows depend on silently off. Two populations, two
    # counters, and the guard applies to the one it is about.
    scripts = sorted(glob.glob(".github/scripts/*.sh"))
    script_checked = 0
    for script in scripts:
        script_checked += 1
        failures += check_script(script)

    total = checked + script_checked

    if failures:
        print(f"\n{failures} of {total} shell block(s) failed to parse.")
        return 1

    # Report blocks, not files, and fail on zero. This gate exists for a
    # defect class that is otherwise invisible until a cron email arrives,
    # so it going inert — an unrecognized `shell:` value, an unexpected YAML
    # shape, a refactor of iter_run_steps that drops blocks — must not look
    # identical to it passing.
    if checked == 0:
        print("::error::No workflow `run:` blocks were checked — the gate is inert.")
        return 1
    if script_checked == 0:
        print("::error::No scripts were checked — the glob matched nothing.")
        return 1

    summary = (
        f"All {total} shell blocks parse "
        f"({checked} workflow `run:` block(s) across {len(paths)} file(s), "
        f"{script_checked} script(s))"
    )
    print(
        f"{summary}; {assumed} assumed bash on a Windows-capable job."
        if assumed
        else f"{summary}."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
