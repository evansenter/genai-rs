# Workspace Explorer Example

Points the harness's read-only builtins at a real directory and observes
every move the agent makes — the **control and observability** surface
underneath an agent run.

Where [`repo_auditor`](../repo_auditor/) shows an agent producing a
structured verdict, this one is about what you can *see* and what you can
*stop*.

## Overview

1. Seeds a small workspace (a README, two source files, and a planted
   `secrets.env`).
2. Runs the agent with `Capabilities::read_only()` and a name-based policy.
3. Streams the turn, matching `AgentEvent::ToolAction` to build an audit
   trail of every harness-executed file tool, with its allow/deny decision.
4. Gates custom tool calls with an `on_pre_tool` hook that refuses **by
   content** — the thing a name-based policy cannot express.
5. Records what each call returned via `on_post_tool`.

## What it covers that the other examples don't

| Surface | Why it matters |
|---------|----------------|
| `add_workspace` with real content | Builtins actually run against real files, not merely enabled |
| `AgentEvent::ToolAction` | The typed action stream — what the agent touched, in order |
| `ToolDecision` on each action | Allowed or denied, per action |
| `on_pre_tool` returning `Deny` | Content-based refusal, live |
| `on_post_tool` | What each call actually returned |

## Policies gate by name; hooks gate by content

This distinction is the point of the example. `policy::allow("record_finding")`
permits the *tool*; it cannot tell a benign argument from a credential. The
`on_pre_tool` hook inspects the arguments and refuses the specific call,
and the model sees the refusal reason as a tool response — so a good reason
string steers the next attempt rather than stalling it.

## A bug this example found

Running it surfaced a real defect: the harness sends `"error": ""`
(protobuf's default for an unset string) on tool calls that **succeeded**,
and the bridge passed that through as `Some("")`. Every successful
harness-executed builtin was therefore reported to `on_post_tool` as a
failure — with `error.is_some()` being exactly the check the field's docs
invite.

Fixed, and pinned by
`tests/antigravity_harness.rs::test_antigravity_workspace_actions_and_post_tool_success`
plus a unit test on the normalization itself. Worth noting that it was
invisible to every existing test and only appeared when someone actually
watched an agent work.

## Running

```bash
pip install google-antigravity==0.1.10   # ships the localharness binary
export GEMINI_API_KEY=...

cargo run --example workspace_explorer --features antigravity
LOUD_WIRE=1 cargo run --example workspace_explorer --features antigravity
```

## Production notes

- Use policies **and** hooks: names for shape, content for judgement.
- Hooks run inline on the event pump — keep them non-blocking, and never
  await a human decision inside one.
- `ToolAction` carries `trajectory_id`; with subagents running, that is what
  distinguishes the parent's actions from a subagent's.
- Workspace announcement is on by default so the model knows the root path;
  disable it only if you ground the model yourself.
