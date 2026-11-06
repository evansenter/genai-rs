# Command Guard Example

Policies that read the call, not just its name, via `Policy::when` and
`with_reason`.

## Overview

The agent gets a shell and three rules: plain read-only commands are
allowed, deletions are denied with a reason that tells the model to carry
on, and anything else is denied with a reason listing what may run. The
user asks it to delete a file and count another's lines. The example
prints each command with its verdict and fails unless a deletion was
denied, a read-only command ran, and the file is still there.

## What it covers that the other examples don't

| Surface | Why it matters |
|---------|----------------|
| `Policy::when` | A rule for some calls to a tool, judged on their arguments |
| `with_reason` | What the model is told, so it adapts rather than retries |
| Precedence | Named tool beats `"*"`; within a tier deny beats ask beats allow |
| `ToolAction::Denied` | The denied call on the stream, named, with its arguments |

## Running

```bash
pip install google-antigravity==0.1.23   # or set ANTIGRAVITY_HARNESS_PATH
export GEMINI_API_KEY=...
cargo run --example command_guard --features antigravity
```

## Gotchas

- **Order does not decide precedence.** Within a tier, deny beats allow
  wherever it was registered; a rule whose condition rejects a call does
  not apply to it, and the call falls through.
- **Conditions read the tool's own argument names** (`CommandLine` for
  `run_command`). Treat a missing argument as a match in a deny rule.
- **Check the whole command line.** A first-word allowlist alone lets
  `ls; rm -rf .` through; this example also refuses shell operators.
