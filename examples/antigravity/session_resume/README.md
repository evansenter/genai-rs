# Session Resume Example

An agent that remembers across process restarts, using the harness's
trajectory persistence — the shape a CLI assistant actually needs.

## Overview

Runs two *separate* agent lifecycles against one persisted conversation:

1. **Run 1** starts fresh with `with_save_dir`, plants a fact, captures
   `conversation_id()`, writes it to disk, and `shutdown()`s.
2. **Run 2** spawns a brand-new agent with the same save dir plus
   `with_conversation_id`, inspects `initial_history()` to prove the
   history came back, and answers a question that can only be answered
   from run 1's turn.

Nothing is carried between the two in memory — only what the harness wrote
to disk.

## What it covers that the other examples don't

| Surface | Why it matters |
|---------|----------------|
| `with_save_dir` + `conversation_id()` | The two halves you must persist together |
| `with_conversation_id` | Selecting *which* trajectory to restore |
| `initial_history()` | The only programmatic evidence a resume resumed |
| `shutdown()` vs drop | Only `shutdown()` makes the trajectory durable |

## The failure mode this exists to make visible

**Resuming an unknown id is not an error.** It comes back empty and the
agent carries on with a fresh conversation — which looks identical to a
working resume until someone notices the agent has amnesia. That is why
run 2 checks `initial_history()` and fails loudly rather than assuming.

The same check is asserted in
`tests/antigravity_harness.rs::test_antigravity_session_resume_restores_history`.

## Running

```bash
pip install google-antigravity==0.1.10   # ships the localharness binary
export GEMINI_API_KEY=...

cargo run --example session_resume --features antigravity
LOUD_WIRE=1 cargo run --example session_resume --features antigravity
```

With `LOUD_WIRE=1` the resume is visible on the wire: run 1 sends an empty
`cascadeId` and gets one back; run 2 sends that id and the
`initializeConversationResponse` comes back carrying `history`.

## Production notes

- Persist **both** the save directory and the conversation id. Either alone
  silently starts fresh.
- Always `shutdown()`. Dropping the agent kills the harness without writing
  the trajectory.
- The save directory grows per conversation — prune on your own schedule.
- Restored history counts toward the context window like any other history,
  so long-lived conversations still compact.
