# Cancellable Turn Example

Stopping an agent mid-thought with `cancel_handle()`, and keeping what it
had already produced.

## Overview

`send_streaming` borrows the agent mutably for the whole turn, so the
streaming loop itself cannot call `cancel()`. `cancel_handle()` returns a
cheap, cloneable handle that escapes that borrow — hand copies to a UI
button, a signal handler, and a watchdog at once.

This example asks for a long essay and halts once 200 characters of
*answer text* have streamed, then shows the partial answer that survived.

## The part worth reading twice

**A cancelled turn does not fail.** On harness 0.1.10 a halt takes the
trajectory to the same terminal state a natural completion does, so the
turn resolves normally, carrying whatever text it had produced. Nothing in
the response distinguishes a halted turn from a finished one — record that
on your side if it matters.

(`AntigravityError::Turn` is still the outcome when the *harness* cancels
a turn of its own accord. Different event.)

## What it covers that the other examples don't

| Surface | Why it matters |
|---------|----------------|
| `cancel_handle()` from another task | The only way to reach the agent mid-turn |
| Cancelling on a condition | A timer races the model; real output does not |
| Partial output | The text produced before the halt is in the response |
| Contrast with `with_turn_timeout` | The timeout *errors* and discards the turn; cancel succeeds and keeps it |

## Running

```bash
pip install google-antigravity==0.1.10   # or set ANTIGRAVITY_HARNESS_PATH
export GEMINI_API_KEY=...
cargo run --example cancellable_turn --features antigravity

# Watch the halt and the terminal state it produces:
LOUD_WIRE=haltRequest,trajectoryStateUpdate cargo run --example cancellable_turn --features antigravity
```

## Gotchas

- **Thinking deltas are not answer text.** Halting mid-thought keeps
  nothing. Trigger on `TextDelta` if partial output is the point — an
  earlier draft of this example halted after 404 characters of *thinking*
  and kept an empty string.
- **Take the handle before starting the turn.**
- **Cancelling before any output is a no-op** — there is no turn to halt
  yet and the request is dropped.
- The harness's `context canceled` stderr line is the expected trace of a
  successful halt, not an error to alert on.
