# Proactive Agent Example

Work that starts without a user turn, via `add_trigger`.

## Overview

Every other example is request/response. Triggers invert that: a message
is injected on a fixed interval with no user behind it — the shape you
want for a watcher that polls a queue, re-checks a build, or summarizes
what changed.

This example registers a 2s trigger, waits for the first delivery, then
issues a user turn that interrupts the trigger's turn.

## What it covers that the other examples don't

| Surface | Why it matters |
|---------|----------------|
| `add_trigger` / `TriggerConfig` | The only non-request/response entry point |
| Wire inspector on `automatedTrigger` | The **only** way to observe a delivery today — there is no agent-stream event |
| The discard boundary | A `chat` after a trigger answers *your* message, never the trigger's |

## Running

```bash
pip install google-antigravity==0.1.10   # or set ANTIGRAVITY_HARNESS_PATH
export GEMINI_API_KEY=...
cargo run --example proactive_agent --features antigravity

# Just the deliveries:
LOUD_WIRE=automatedTrigger,summary cargo run --example proactive_agent --features antigravity
```

## Gotchas

- **Open with a real turn before any trigger can fire.** On harness
  0.1.10 a trigger delivered into a conversation with *no history* crashes
  the harness process — its pre-invocation hook asks for "tokens since the
  last checkpoint", finds no steps, and aborts the run
  (`earliest step index is out of bounds: 0 vs 0`). The session dies with
  it. This example opens with a turn for exactly that reason.
- **A trigger's turn is not surfaced.** Its text never reaches your
  stream; the next `chat`/`send_streaming` halts it and discards its
  events. Use triggers for side effects, and read results with a normal
  turn afterwards.
- **Delivery is idle-only.** A firing due mid-turn is deferred, and missed
  intervals collapse into one — there is no backlog.
- **The first firing is after one interval**, not at spawn.
- **Intervals must be non-zero**; `spawn()` validates this.
- Trigger tasks stop on `shutdown()` and on drop, so no timers leak.
