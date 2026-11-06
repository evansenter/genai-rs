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
| `add_trigger` / `TriggerConfig` | Work on a timer, with no request behind it (`add_custom_trigger` takes any other source) |
| Wire inspector on `automatedTrigger` | Counts deliveries whether or not anything drives their turns |
| The discard boundary | A `chat` after a trigger answers *your* message, never the trigger's |

## Running

```bash
pip install google-antigravity==0.1.23   # or set ANTIGRAVITY_HARNESS_PATH
export GEMINI_API_KEY=...
cargo run --example proactive_agent --features antigravity

# Just the deliveries:
LOUD_WIRE=automatedTrigger,summary cargo run --example proactive_agent --features antigravity
```

## Gotchas

- **The example opens with a real turn before any trigger can fire.**
  Harness 0.1.23 runs a trigger in an empty conversation normally, but on
  0.1.10 and 0.1.18 one crashed the run (`earliest step index is out of
  bounds: 0 vs 0`) and closed the WebSocket, killing the session. The
  example still opens with a turn, which also gives the trigger turns a
  conversation to build on.
- **A trigger's turn is surfaced only on request.** Unless
  `next_trigger_turn` drives it (see `file_watcher`), its text never
  reaches you; the next `chat`/`send_streaming` halts it and discards its
  events.
- **Delivery is idle-only.** A firing due mid-turn is deferred, and missed
  intervals collapse into one — there is no backlog.
- **The first firing is after one interval**, not at spawn.
- **Intervals must be non-zero**; `spawn()` validates this.
- Trigger tasks stop on `shutdown()` and on drop, so no timers leak.
