# Antigravity Harness Guide

`genai_rs::antigravity` is a native Rust client for the `localharness` agent
runtime that ships with Google's [`google-antigravity`](https://pypi.org/project/google-antigravity/)
Python SDK. The harness binary *is* the agent: model calls, streaming,
history/compaction, built-in tools (shell, file edits, grep, web search,
image generation), MCP, subagents, and trajectory persistence all run inside
it. This module speaks its protocol directly — no Python in the loop — so
your tools, hooks, and policies are ordinary Rust.

> **Note**: All code blocks in this guide use `rust,ignore` because the
> `antigravity` feature is off by default and doctests run without it.
> The same snippets are exercised (compiled and run) by
> `examples/antigravity_agent.rs` and `tests/antigravity_harness.rs`.

## Setup

Enable the feature:

```toml
[dependencies]
genai-rs = { version = "0.7", features = ["antigravity"] }
```

Install the harness binary (it ships inside the platform-specific wheel):

```bash
pip install google-antigravity==0.1.5
```

### Version pinning

The harness wire protocol is internal to Google's SDK and changes across
0.1.x releases. Each genai-rs release is verified against exactly one wheel
version, exposed as `antigravity::SUPPORTED_HARNESS_VERSION` (currently
`0.1.5`) — pin that version. Newer harnesses degrade gracefully rather than
erroring: unknown events, fields, and enum values are preserved in `Unknown`
variants and `extra` maps (the crate's Evergreen philosophy), but only the
pinned version is tested end-to-end.

### Binary discovery

`spawn()` finds the binary in this order:

1. `AgentBuilder::with_harness_path(...)` — explicit path.
2. `ANTIGRAVITY_HARNESS_PATH` environment variable (the same variable the
   Python SDK honors).
3. `google/antigravity/bin/localharness` inside `python3`'s site-packages.
4. `localharness` on `PATH`.

A miss returns `AntigravityError::HarnessNotFound` listing every location
searched.

## Quick start

```rust,ignore
use genai_rs::antigravity::{AntigravityAgent, policy};

let mut agent = AntigravityAgent::builder()
    .with_api_key(std::env::var("GEMINI_API_KEY")?)
    .with_model("gemini-3-flash-preview")
    .with_system_instructions("You are a code-review assistant.")
    .add_workspace("/path/to/repo")
    .add_policy(policy::deny_all())
    .add_policy(policy::allow("view_file"))
    .spawn()
    .await?;

let response = agent.chat("Summarize the layout of this repo.").await?;
println!("{}", response.text());

agent.shutdown().await?;   // graceful: the harness persists its trajectory
```

`spawn()` launches the harness, performs the stdio handshake, connects to
its localhost WebSocket, and initializes the conversation. `shutdown()`
closes the WebSocket and stdin (the harness's graceful-exit signal), then
escalates to SIGTERM/SIGKILL only if it lingers. Dropping the agent without
`shutdown()` kills the harness immediately — no zombie, but no trajectory
persistence either.

## Built-in tools (capabilities)

The harness executes its own tool suite; you choose which tools the agent
sees. The default is the **read-only** set (`list_directory`,
`search_directory`, `find_file`, `view_file`, `finish`), matching the Python
SDK:

```rust,ignore
use genai_rs::antigravity::{BuiltinTool, Capabilities};

// Read-only plus shell access:
let caps = Capabilities::read_only().enable(BuiltinTool::RunCommand);

// Everything (requires a policy — see below):
let caps = Capabilities::all();

// Custom tools only:
let caps = Capabilities::none();

let builder = AntigravityAgent::builder().with_capabilities(caps);
```

### Safety gate

Enabling any write-capable builtin (`run_command`, `edit_file`,
`create_file`, `generate_image`, `search_web`, `start_subagent`) or any MCP
server **without a policy or pre-tool hook** is an error at `spawn()` time —
the same guard the Python SDK enforces. Add `policy::allow_all()` for
autonomous agents, or a deny-by-default rule set.

## Policies

Policies are declarative allow/deny/confirm rules over tool names, evaluated
**in Rust before every dispatch decision** — defense in depth on top of the
harness's own enforcement:

```rust,ignore
use genai_rs::antigravity::policy;

let agent = AntigravityAgent::builder()
    // deny everything, then allow specific tools:
    .add_policy(policy::deny_all())
    .add_policy(policy::allow("view_file"))
    .add_policy(policy::allow("get_weather"))       // a custom tool
    .add_policy(policy::confirm("run_command"))     // defer to on_pre_tool
    .on_pre_tool(|call| {
        if call.args["commandLine"].as_str().unwrap_or("").contains("rm ") {
            genai_rs::antigravity::PreToolDecision::deny("no deletions")
        } else {
            genai_rs::antigravity::PreToolDecision::Allow
        }
    })
    // ...
    ;
```

Rules:

- **Exact-name rules beat wildcards** (`"*"`), so registration order between
  `deny_all()` and `allow("x")` doesn't matter; within the same specificity
  tier, the first matching rule wins.
- No matching rule = allow (default open, like the Python SDK), still
  subject to the `on_pre_tool` hook.
- `confirm(name)` defers to `on_pre_tool`; with no hook configured the call
  is **denied** (fail closed).
- Targets: builtin wire names (`run_command`, `edit_file`, ...), custom tool
  names, and MCP tools as `mcp_<server>_<tool>`.

`on_post_tool` observes completed custom tool calls (and harness-side
post-tool hook callbacks) for audit logging.

## Custom tools — the same `#[tool]` functions as the Interactions API

Tool declarations are the crate's ordinary `FunctionDeclaration`; dispatch
reuses the global `#[tool]` registry and `ToolService`:

```rust,ignore
use genai_rs::CallableFunction;
use genai_rs_macros::tool;

/// Returns the current weather for a city.
#[tool(city(description = "The city to get weather for"))]
fn get_weather(city: String) -> String {
    format!("Sunny and 22C in {city}")
}

let agent = AntigravityAgent::builder()
    .add_tool(GetWeatherCallable.declaration())     // #[tool] machinery
    .with_tool_service(my_service)                  // stateful ToolService
    // ...
    ;
```

When the model calls a custom tool, the crate checks policies, executes your
function, and replies to the harness automatically. Failures become
`{"error": ...}` results the model can react to — the turn is never
deadlocked by a failing tool.

## MCP servers

```rust,ignore
use genai_rs::antigravity::McpServer;

let agent = AntigravityAgent::builder()
    .add_mcp_server(McpServer::stdio("uvx", ["mcp-server-git"]).with_name("git"))
    .add_mcp_server(McpServer::http("http://localhost:8931/mcp").with_name("tickets"))
    .add_policy(policy::deny_all())
    .add_policy(policy::allow("mcp_git_status"))    // per-tool policy target
    // ...
    ;
```

The harness owns the MCP connections and tool execution; your policies see
the calls as `mcp_<server>_<tool>`.

## Streaming

```rust,ignore
use futures_util::StreamExt;
use genai_rs::antigravity::AgentEvent;

let mut stream = agent.send_streaming("Refactor src/lib.rs").await?;
while let Some(event) = stream.next().await {
    match event? {
        AgentEvent::TextDelta(t) => print!("{t}"),
        AgentEvent::ThinkingDelta(_) => {}
        AgentEvent::ToolAction(a) => eprintln!("[harness tool] {a:?}"),
        AgentEvent::ToolCallDispatched { name, .. } => eprintln!("[custom tool] {name}"),
        AgentEvent::Finished(response) => { println!(); break; }
        AgentEvent::Error(e) => eprintln!("[error] {e}"),
        _ => {} // non-exhaustive
    }
}
```

The stream borrows the agent mutably for the turn. To cancel from another
task, take a handle first:

```rust,ignore
let cancel = agent.cancel_handle();
// ... later, from any task:
cancel.cancel().await?;   // the in-flight turn fails with AntigravityError::Turn
```

`with_turn_timeout(Duration)` bounds each turn's wall-clock time.

## Structured output

```rust,ignore
let mut agent = AntigravityAgent::builder()
    .with_response_schema(serde_json::json!({
        "type": "object",
        "properties": {"severity": {"type": "string"}},
        "required": ["severity"]
    }))
    // ...
    .spawn().await?;

let response = agent.chat("Audit this repo.").await?;
if let Some(value) = response.structured_output() {
    println!("severity = {}", value["severity"]);
}
```

## Session persistence and resume

```rust,ignore
// First run:
let agent = AntigravityAgent::builder()
    .with_save_dir("/var/lib/myapp/agent-sessions")
    .spawn().await?;
let id = agent.conversation_id().unwrap().to_string();
agent.shutdown().await?;   // shutdown() persists the trajectory

// Later:
let agent = AntigravityAgent::builder()
    .with_save_dir("/var/lib/myapp/agent-sessions")
    .with_conversation_id(id)
    .spawn().await?;
println!("restored {} steps", agent.initial_history().len());
```

## Debugging

The Antigravity client feeds the crate's canonical wire-inspection layer
(`genai_rs::wire`). `LOUD_WIRE=1` pretty-prints everything to stderr:

- `HARNESS <path> (pid N)` — process spawn,
- `WS Send` / `WS Receive` — every proto-JSON message,
- `STDERR:` — every harness diagnostic line.

```bash
LOUD_WIRE=1 cargo run --example antigravity_agent --features antigravity
```

For programmatic capture, register inspectors on the builder — the
`WireEvent` variants are `HarnessSpawn`, `WsSend`, `WsReceive`, and
`HarnessStderr`, sharing one correlation id per harness session:

```rust,ignore
use genai_rs::wire::TracingForwarder;
use std::sync::Arc;

let agent = AntigravityAgent::builder()
    .add_wire_inspector(Arc::new(TracingForwarder::new()))  // RUST_LOG=genai_rs::wire=debug
    // ...
    ;
```

Spawn- and init-time errors (`HandshakeFailed`, `InitFailed`,
`ConnectionClosed`) carry the tail of the harness's stderr — that is where
the harness explains itself (e.g. `no text model configuration provided`).

## Errors

`AntigravityError` is structural (`#[non_exhaustive]`, thiserror): match on
variants, never on message text. Key variants: `HarnessNotFound{searched}`,
`HandshakeFailed`/`InitFailed`/`ConnectionClosed` (with `stderr`),
`Config` (spawn-time validation, including the safety gate), `Turn`
(cancellation, pre-turn denial, fatal model-backend errors), `Timeout`,
`ToolDispatch`, `WebSocket`, `Protocol`, `Io`, `Json`.

## Current limitations (follow-ups)

- **Triggers**: `antigravity::triggers` holds the config type only; the
  interval scheduler that injects `automated_trigger` events is not wired
  into the builder yet.
- **Subagents**: the wire types (`CustomAgent`) exist, but the builder does
  not expose custom subagent registration yet.
- **User questions**: the `ask_question` builtin is answered "unanswered"
  automatically (never deadlocks); interactive question hooks are not
  exposed. Disable the builtin if this matters.
- **Hooks are synchronous**: `on_pre_tool` / `on_post_tool` are sync
  closures; async hooks are a follow-up.
- **Vertex endpoints**: wire types exist; the tested path is the Gemini API
  key endpoint.
