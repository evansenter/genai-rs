# Lifecycle Hooks Example

A definition of done the agent cannot skip, via `on_stop`.

## Overview

The tool hooks see single calls; the lifecycle hooks see the session and
its turns. The harness asks the stop hook before a turn ends, and
`StopDecision::continue_with(instruction)` keeps the turn going with that
instruction. This example asks for a summary of a CSV and uses the stop
hook to require that the summary is also saved to `SUMMARY.md`, which the
user's message never mentions. It prints each hook as it fires and fails
unless the hook kept the turn going and the file exists.

## What it covers that the other examples don't

| Surface | Why it matters |
|---------|----------------|
| `on_stop` / `StopDecision::continue_with` | Keeps a turn going until a condition of yours holds |
| `StopRequest::continuation_count` | The bound that stops a hook from running a turn forever |
| `on_session_start`, `on_pre_turn`, `on_post_turn`, `on_session_end` | The order the harness calls them in, printed |

## Running

```bash
pip install google-antigravity==0.1.23   # or set ANTIGRAVITY_HARNESS_PATH
export GEMINI_API_KEY=...
cargo run --example lifecycle_hooks --features antigravity
```

## Gotchas

- **Bound the stop hook.** A condition the model cannot meet keeps
  continuing until the turn deadline; stop past a `continuation_count` of
  your own.
- **`on_session_end` runs in `shutdown()`**, so dropping the agent skips it.
- **Hooks are synchronous**, each on a thread of its own and bounded by
  the turn deadline.
- **`on_pre_turn` can refuse a turn** (`TurnDecision::deny`); the turn
  fails with `AntigravityError::Turn` of kind `TurnFailure::Denied`, and
  the agent stays usable.
