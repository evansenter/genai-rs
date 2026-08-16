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

Two further limits worth stating, so a green check is not read as broader
coverage than it is:

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


def iter_run_steps(path: str):
    """Yield (job, index, name, run) for each shell `run:` block."""
    with open(path, encoding="utf-8") as handle:
        document = yaml.safe_load(handle)

    for job_name, job in (document.get("jobs") or {}).items():
        # Reusable-workflow calls (`uses:` at job level) have no steps.
        default_shell = (
            (job.get("defaults") or {}).get("run") or {}
        ).get("shell") or "bash"
        for index, step in enumerate(job.get("steps") or []):
            run = step.get("run")
            if not run:
                continue
            shell = str(step.get("shell", default_shell)).split(maxsplit=1)[0]
            if shell not in SHELLS:
                continue
            yield job_name, index, step.get("name", "(unnamed)"), run


def check(path: str) -> tuple[int, int]:
    """Returns (blocks checked, failures)."""
    checked = 0
    failures = 0
    for job_name, index, step_name, run in iter_run_steps(path):
        checked += 1
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
    return checked, failures


def main() -> int:
    paths = sorted(
        glob.glob(".github/workflows/*.yml") + glob.glob(".github/workflows/*.yaml")
    )
    if not paths:
        print("::error::No workflow files found — is this running from the repo root?")
        return 1

    checked = 0
    failures = 0
    for path in paths:
        path_checked, path_failures = check(path)
        checked += path_checked
        failures += path_failures

    if failures:
        print(f"\n{failures} of {checked} run: block(s) failed to parse.")
        return 1

    # Report blocks, not files, and fail on zero. This gate exists for a
    # defect class that is otherwise invisible until a cron email arrives,
    # so it going inert — an unrecognized `shell:` value, an unexpected YAML
    # shape, a refactor of iter_run_steps that drops blocks — must not look
    # identical to it passing.
    if checked == 0:
        print("::error::No run: blocks were checked — the gate is inert.")
        return 1

    print(f"All {checked} run: blocks parse across {len(paths)} workflow file(s).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
