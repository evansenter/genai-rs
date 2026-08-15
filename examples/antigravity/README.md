# Antigravity Harness Examples

Examples that drive Google's Antigravity `localharness` agent runtime
through this crate's native client. Grouped here, mirroring
[`src/antigravity/`](../../src/antigravity/), because they share a setup
story the rest of `examples/` does not: every one needs the `antigravity`
cargo feature **and** the harness binary on your machine.

```bash
pip install google-antigravity==0.1.10   # version must match SUPPORTED_HARNESS_VERSION
export GEMINI_API_KEY=your_api_key
cargo run --example antigravity_agent --features antigravity
```

Start with `agent.rs`. The rest are complete projects, each with its own
README explaining what it demonstrates and what to look for in the output.

| Example | What it's for | Key surface |
|---------|---------------|-------------|
| [`agent.rs`](./agent.rs) (`--example antigravity_agent`) | The starter — spawn a harness, take a turn, read the result | `AntigravityAgent::builder()`, Rust `#[tool]` functions, `on_questions` |
| [`repo_auditor/`](./repo_auditor/) | Agentic security audit of a checked-in fixture repo | Subagents, policies + hooks, structured report |
| [`session_resume/`](./session_resume/) | An agent that remembers across process restarts | Trajectory persistence, `conversation_id` round trip, `initial_history` |
| [`workspace_explorer/`](./workspace_explorer/) | Watching an agent work, and gating it live | Workspaces, typed `ToolAction` stream, content-based `on_pre_tool` deny |
| [`mcp_toolbelt/`](./mcp_toolbelt/) | Giving an agent tools it didn't ship with | `add_mcp_server` (stdio), `mcp_<server>_<tool>` policy targets |
| [`proactive_agent/`](./proactive_agent/) | Work that starts without a user turn | `add_trigger`, observing deliveries via a wire inspector |
| [`cancellable_turn/`](./cancellable_turn/) | Stopping an agent mid-thought | `cancel_handle` from another task, partial output kept |

Every one of these is smoke-run in CI on each push, so a change that
breaks one is caught by the run rather than by the next person to try it.

## Seeing the wire

The harness protocol is a stdio handshake plus proto-JSON over a
localhost WebSocket. `LOUD_WIRE` prints it:

```bash
LOUD_WIRE=1 cargo run --example antigravity_agent --features antigravity
LOUD_WIRE=stepUpdate cargo run --example workspace_explorer --features antigravity
```

Selectors match a message's oneof key, and — on received frames — the
action nested inside a `stepUpdate`, so `LOUD_WIRE=mcpTool` shows only
the MCP calls. See [docs/ANTIGRAVITY.md](../../docs/ANTIGRAVITY.md#wire-inspection).

## Further reading

- [docs/ANTIGRAVITY.md](../../docs/ANTIGRAVITY.md) — the guide: setup,
  workspaces, policies, subagents, streaming, triggers, resume, debugging
- [docs/ANTIGRAVITY_BRIDGE_DESIGN.md](../../docs/ANTIGRAVITY_BRIDGE_DESIGN.md)
  — why this is a native protocol client rather than a PyO3 binding
