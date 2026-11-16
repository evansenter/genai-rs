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
It fails if the turn runs on more than 30 seconds after the halt, keeps no
answer text, or never streams enough to halt on before its 180-second
budget, listing on stderr what the turn streamed (events by kind,
thinking, tool actions and the harness's error steps).

## The part worth reading twice

**A cancelled turn does not fail.** From harness 0.1.10 through the pinned
0.1.23 a halt takes the trajectory to the same terminal state a natural
completion does, so the
turn resolves normally, carrying whatever text it had produced. Nothing in
the response distinguishes a halted turn from a finished one — record that
on your side if it matters.

(`AntigravityError::Turn` of kind `TurnFailure::Cancelled` is still the
outcome when the *harness* cancels a turn of its own accord. Different
event.)

## What it covers that the other examples don't

| Surface | Why it matters |
|---------|----------------|
| `cancel_handle()` from another task | The only way to reach the agent mid-turn |
| Cancelling on a condition | A timer races the model; real output does not |
| Partial output | The text produced before the halt is in the response |
| Contrast with `with_turn_timeout` | The timeout *errors* and discards the turn; cancel succeeds and keeps it |

## Running

```bash
pip install google-antigravity==0.1.23   # or set ANTIGRAVITY_HARNESS_PATH
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
- **Ask for the answer in the reply.** On harness 0.1.23 an agent with no
  tools enabled can still answer a request for long writing with a tool
  call. Nothing streams while the model writes it, the call fails as
  malformed minutes later, and the harness retries the model call, so a
  halt keyed on answer text waits the whole time. The prompt asks for the
  essay as the reply, in plain text with no tools or files, and sets no
  pace; `docs/ANTIGRAVITY_BRIDGE_DESIGN.md` records how that wording was
  measured.
- **Take the handle before starting the turn.**
- **Cancelling before any output is a no-op** — there is no turn to halt
  yet and the request is dropped.
- The harness's `context canceled` stderr line is the expected trace of a
  successful halt, not an error to alert on.
