# File Watcher Example

An agent that reacts to files, via `triggers::on_file_change`, and hands
back what it did, via `next_trigger_turn`.

## Overview

The watch polls a directory and passes each batch of changes to a
callback; the message the callback returns starts a turn of the agent's
own. `next_trigger_turn` waits for that turn, drives it and returns its
response. This example watches an inbox, drops a request file into it,
and prints the agent's reading of it. It fails unless the change started a
turn and that turn's response came back.

## What it covers that the other examples don't

| Surface | Why it matters |
|---------|----------------|
| `triggers::on_file_change` | Event-driven work, where `add_trigger` is a timer |
| `add_custom_trigger` | Takes any trigger built on `TriggerContext` |
| `next_trigger_turn` | Returns a trigger turn's response instead of discarding it |

## Running

```bash
pip install google-antigravity==0.1.23   # or set ANTIGRAVITY_HARNESS_PATH
export GEMINI_API_KEY=...
cargo run --example file_watcher --features antigravity
```

## Gotchas

- **The watch polls**, comparing sizes and modification times, and skips
  `.git`, `node_modules` and similar directories.
- **The baseline is taken when `on_file_change` is called**, so a file
  written right after `spawn()` is reported. If part of the tree cannot be
  read then, the first poll that reads all of it is the baseline instead.
- **A poll that cannot read part of the tree is skipped**, with a warning
  naming what it could not read, so an unreadable directory holds back
  every change until it reads again.
- **Changes made during a turn arrive as one batch after it**: the
  callback runs once the agent is idle, with the net change since the
  last batch. Return `None` from it to skip a batch.
- **Without `next_trigger_turn`**, a trigger's turn runs unobserved and
  the next `chat` discards its events (see `proactive_agent`).
