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
SHELLS = {"bash", "sh", "bash -e {0}", "bash -eo pipefail {0}"}


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
            if step.get("shell", default_shell) not in SHELLS:
                continue
            yield job_name, index, step.get("name", "(unnamed)"), run


def check(path: str) -> int:
    failures = 0
    for job_name, index, step_name, run in iter_run_steps(path):
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
    return failures


def main() -> int:
    paths = sorted(
        glob.glob(".github/workflows/*.yml") + glob.glob(".github/workflows/*.yaml")
    )
    if not paths:
        print("::error::No workflow files found — is this running from the repo root?")
        return 1

    failures = sum(check(path) for path in paths)
    if failures:
        print(f"\n{failures} run: block(s) failed to parse.")
        return 1

    print(f"All run: blocks parse across {len(paths)} workflow file(s).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
