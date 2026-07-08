# Antigravity Bridge Design

Status: **Implemented (phase 1)** — `src/antigravity/`, feature `antigravity`,
user guide `docs/ANTIGRAVITY.md`. Deviations from this design in phase 1:

- ~~`triggers.rs` is a stub~~ — implemented: `AgentBuilder::add_trigger`
  spawns per-trigger timer tasks sending `automated_trigger` events, gated
  on client-side idleness (deliveries defer while a turn is in flight and
  missed intervals collapse into one), aborted on shutdown/drop. Jitter was
  dropped from the plan (the reference SDK's `every()` has none).
- ~~Custom subagent registration not exposed~~ — implemented:
  `AgentBuilder::add_subagent(Subagent)` with spawn-time validation that
  referenced custom tools are registered on the parent (reference-SDK
  parity, including appended-style subagent instructions and force-disabled
  nested subagents).
- Hooks are synchronous closures (`on_pre_tool`/`on_post_tool`); async hooks
  and the `workspace_only` policy combinator are follow-ups. Policies support
  `allow`/`deny`/`confirm`/`allow_all`/`deny_all` with exact-name-over-wildcard
  priority (a simplification of the Python SDK's six-tier model).
- The session transport is driven sequentially by the turn loop (no
  background reader task); cancellation uses a shared sink handle. A mockable
  transport trait was deferred — protocol logic is unit-tested at the serde
  layer and against the real binary instead.
- Consequence of the sequential transport: **trigger-initiated harness turns
  run unobserved and their output is not surfaced.** The next
  `chat`/`send_streaming` halts a still-running trigger turn and drains its
  events before sending the user's input (same discipline as per-turn
  timeout recovery), so trigger turns cannot desync client-driven turns.
  Follow-up: a background consumer owning the read half (reference-SDK
  style) that drives trigger turns to completion and surfaces their output
  via a documented channel/callback.
- `IntoFuture` builder sugar was skipped in favor of an explicit
  `spawn().await`.

Scope: Add first-class support for building agentic applications on Google's
Antigravity harness, alongside the existing Interactions API client.

## Summary

`genai-rs` becomes a dual-mode library:

1. **Interactions API** (existing): direct HTTP/SSE client for Gemini model
   inference — `Client`, `InteractionBuilder`, function calling, streaming,
   multimodal, structured output.
2. **Antigravity harness** (new): a native Rust client for the `localharness`
   agent runtime that ships with Google's `google-antigravity` Python SDK —
   agents with workspaces, built-in tools (shell, file edit, web search),
   custom Rust tools, MCP servers, hooks/policies, subagents, triggers, and
   session persistence.

The two modes share the crate's existing strengths: the `#[tool]` macro and
`FunctionRegistry` for custom tools, Evergreen soft-typing at the wire layer,
builder-style APIs, and wire-level debuggability.

## Decision: native protocol client, not PyO3

We connect **directly to the `localharness` binary** using the same protocol
the Python SDK uses. We do **not** embed Python via PyO3.

### Evidence

The `google-antigravity` wheel (v0.1.5, Apache-2.0) was inspected at source
level and the protocol exercised end-to-end from a standalone script in this
repository's development environment (spawn → handshake → WebSocket →
conversation init succeeded without any Python SDK involvement).

Architecture facts that drove the decision:

- **The Go binary is the agent runtime.** The entire agentic loop lives in the
  ~30 MB `localharness` binary shipped inside the wheel: model calls,
  streaming, history/compaction, built-in tool execution (run_command,
  file edit/view/create, grep, list_dir, web search, image gen), the MCP
  client, skills loading, trajectory persistence/resume, and structured
  output. The Python layer never calls a model.
- **The Python layer is a protocol client** (~8.8k LOC incl. types): process
  spawn + handshake, a WebSocket event pump, custom-tool dispatch (run a
  Python callable, reply with JSON), a hook/policy engine that reduces to
  allow/deny decisions on the wire, and API sugar (`Agent`, `Conversation`).
  All of it is exactly the layer a Rust framework wants to own natively —
  tools as Rust closures via the existing `#[tool]` macro, hooks as Rust
  closures, policies as Rust types.
- **The protocol is mechanically portable.**
  - Stdio handshake: 4-byte little-endian length-prefixed **binary protobuf**.
    Client writes `InputConfig{storage_directory, port, bind_address,
    client_info}`; harness replies `OutputConfig{port, api_key}` on stdout.
    Two trivial messages — encoded with hand-rolled protobuf (field numbers
    verified against the shipped descriptors), no protoc/prost build dep.
  - Everything else: **proto-JSON over a localhost WebSocket** (header
    `x-goog-api-key: <OutputConfig.api_key>` — a per-process token, not the
    Gemini key). Proto-JSON means `serde` structs with camelCase field names
    and unknown-field preservation — a perfect match for the crate's
    Evergreen philosophy.
- **PyO3 costs with no offsetting benefit**: embedding CPython, asyncio↔tokio
  bridging, pydantic marshaling, Python packaging/venv discovery, GIL
  contention — all to reach a protocol that is simpler than the Python code
  wrapping it, wrapped by an SDK that is itself alpha (0.1.x, weekly
  releases). There is no stable Python API to amortize against.

### Risk: the protocol is internal and in flux

The wire protocol is not publicly documented and changes across 0.1.x
releases. Mitigations:

1. **Pin a supported harness version per genai-rs release** (documented
   constant, e.g. `SUPPORTED_HARNESS_VERSION = "0.1.5"`); integration tests
   run against exactly that wheel.
2. **Evergreen at the wire layer**: every event envelope deserializes unknown
   oneof variants into `Unknown { event_type, data }` and continues; unknown
   fields are preserved for roundtrip. Protocol drift degrades gracefully
   instead of breaking.
3. **`ClientInfo{language: "rust", version: <crate version>}`** is sent
   honestly so the harness can version-gate if it grows that capability.

## Binary acquisition & discovery

The binary ships only inside platform-specific PyPI wheels (macOS arm64,
Linux x86_64/aarch64, Windows amd64/arm64). Discovery order (mirrors the
Python SDK, extended):

1. Explicit path: `AntigravityAgent::builder().with_harness_path(...)`.
2. `ANTIGRAVITY_HARNESS_PATH` env var (same var the Python SDK honors).
3. Scan installed `google-antigravity` pip package for
   `google/antigravity/bin/localharness[.exe]` (site-packages of `python3`
   on PATH, plus common user-site locations).
4. `localharness` on `PATH`.

If not found, the error message tells the user to
`pip install google-antigravity` or set `ANTIGRAVITY_HARNESS_PATH`.
We do **not** vendor the 30 MB binary in the crate. A `harness fetch` helper
(downloading the matching wheel from PyPI and extracting the binary to a
cache dir) is a possible follow-up, not v1.

Licensing: SDK and wheel are Apache-2.0; ported protocol-handling logic
carries Google LLC attribution in file headers where derived.

## Lessons adopted from prior art (agy-bridge / llm-tool)

The existing PyO3 bridge (`domenukk/agy-bridge`) was reviewed in depth. Its
transport layer confirms the native decision by counter-example (a ~900-line
Python init script monkeypatching five SDK internals, process-global
registries to cross the FFI boundary, comment-enforced GIL invariants,
POSIX-only venv discovery). Its **API vocabulary**, however, is strong and we
adopt these ideas natively:

- **`IntoFuture` agent builder** — `bridge.agent(cfg).tools(...).await`.
- **Take-once typed streaming handles** — a unified event stream plus
  optional split text/thought/tool-call streams on the response handle.
- **Structural error classification, never string matching** — with unit
  tests asserting that name/message sniffing is rejected.
- **Defense in depth for policies** — declarative policy set evaluated
  Rust-side before every custom-tool dispatch, even though the harness also
  enforces hook decisions.
- **Mockable runtime boundary** — session transport behind a trait so the
  agent API is unit-testable without spawning the binary.
- **Macro ergonomics** (for `#[tool]` evolution, separate workstream):
  compile-time-mandatory doc comments per parameter, `Option<T>` →
  `serde(default)`, flexible return types, trybuild compile-fail tests.
- **Zombie hygiene** — explicit cleanup on init-timeout so leaked harness
  processes cannot accumulate; layered timeouts (inner operation + outer
  safety margin).

## Relationship to the Interactions API `agent` field

The Interactions API exposes hosted agents including
`agent: "antigravity-preview-05-2026"` — the same harness run server-side.
genai-rs therefore offers the Antigravity harness at two altitudes:

- **Hosted**: `client.interaction().with_agent("antigravity-preview-05-2026")`
  (plus Environments/Agents resources — see `docs/INTERACTIONS_API_GAP.md`).
- **Local**: `genai_rs::antigravity` spawning `localharness` with local
  workspaces, local tool execution, and local policy enforcement (this doc).

## Module layout

New top-level module, feature-gated:

```text
src/antigravity/
  mod.rs           Public API: AntigravityAgent, AgentBuilder, AgentSession
  config.rs        AgentConfig, CapabilitiesConfig, ModelTarget, McpServer,
                   SubagentConfig, WorkspaceConfig (serde, Evergreen)
  protocol.rs      Wire types: InputEvent/OutputEvent envelopes, StepUpdate,
                   ToolCall/ToolResponse, HarnessConfig (proto-JSON serde,
                   Unknown variants throughout)
  handshake.rs     Binary-proto InputConfig/OutputConfig encode/decode
                   (hand-rolled, ~100 LOC, exhaustively unit-tested)
  process.rs       Spawn, stdin-EOF shutdown ordering, SIGTERM/SIGKILL
                   escalation, stderr drain task (pipe-deadlock avoidance)
  session.rs       WebSocket pump: reader task, step routing, idle tracking,
                   confirmation-request debouncing
  tools.rs         Custom tool dispatch — bridges harness ToolCall to the
                   existing FunctionRegistry / ToolService / #[tool] macro
  hooks.rs         Hook + Policy types (pre/post turn/tool, on_tool_error);
                   policy combinators: allow_all, deny_all, allow, deny,
                   confirm, workspace_only
  triggers.rs      Client-side timers -> automated_trigger events
  streaming.rs     Step/delta stream types (text_delta, thinking_delta,
                   tool actions), AgentEvent public enum
```

Feature flag: `antigravity` (default **off** initially; revisit after
stabilization). Dependencies added under the flag: `tokio-tungstenite`
(rustls, consistent with the crate's TLS stance). No protobuf crate — the two
binary messages are hand-encoded; everything else is serde JSON.

## Public API sketch

```rust,ignore
use genai_rs::antigravity::{AntigravityAgent, policy};

let mut agent = AntigravityAgent::builder()
    .with_api_key(std::env::var("GEMINI_API_KEY")?)   // or with_model_target(...)
    .with_model("gemini-3-flash-preview")
    .with_system_instructions("You are a code-review assistant.")
    .with_workspace("/path/to/repo")
    .add_tool(get_weather_declaration())               // same #[tool] fns as Interactions
    .add_mcp_server(McpServer::stdio("uvx", ["mcp-server-git"]))
    .add_policy(policy::deny_all())
    .add_policy(policy::allow("get_weather"))
    .on_pre_tool(|call| { tracing::info!(?call); Decision::Allow })
    .spawn()                                           // launches harness, handshakes, inits
    .await?;

// Simple: one-shot chat
let response = agent.chat("Summarize the diff in HEAD~1").await?;
println!("{}", response.text());

// Advanced: stream agent steps
let mut steps = agent.send_streaming("Refactor src/lib.rs").await?;
while let Some(event) = steps.next().await {
    match event? {
        AgentEvent::TextDelta(t) => print!("{t}"),
        AgentEvent::ToolAction { action, decision, .. } => {
            eprintln!("[tool] {action:?} ({decision:?})");
        }
        AgentEvent::Finished(r) => break,
        _ => {}
    }
}

agent.shutdown().await?;   // close stdin -> graceful harness exit
```

Design rules carried over from the Interactions side:

- `with_*` configures, `add_*` accumulates (BUILDER_API.md conventions).
- The **same `#[tool]` functions work in both modes** — the macro's
  `FunctionDeclaration` maps 1:1 onto the harness `Tool{name, description,
  parameters_json_schema}`.
- Safety parity with the Python SDK: enabling write tools or MCP servers
  without any policy/pre-tool hook is a builder error at `spawn()` time.
- Session resume: `with_save_dir(...)` + `with_conversation_id(...)`;
  `agent.conversation_id()` exposes the id for later resume.

## Wire debugging (shared with LOUD_WIRE redesign)

The LOUD_WIRE mechanism is redesigned around a structured event type so both
transports feed one canonical debugging surface:

- `WireEvent` enum: `Request`, `ResponseStatus`, `ResponseBody`, `ErrorBody`,
  `SseFrame`, `UploadStart/Complete` (Interactions) plus `HarnessSpawn`,
  `WsSend`, `WsReceive`, `HarnessStderr` (Antigravity).
- `ClientBuilder::with_wire_inspector(Arc<dyn WireInspector>)` /
  `AgentBuilder::with_wire_inspector(...)` for programmatic, per-client
  capture (snapshot tests, bug reports).
- Built-in sinks: the existing colored stderr printer (`LOUD_WIRE=1` env
  sugar — zero behavior change for current users) and a `tracing` forwarder
  emitting to target `genai_rs::wire` (`RUST_LOG=genai_rs::wire=trace`).
- Fixes folded in: error response bodies logged, UTF-8-safe truncation,
  no request serialization when disabled.

## Testing strategy

- **Unit**: handshake encode/decode golden tests; proto-JSON roundtrips for
  every protocol type (proptest, matching existing roundtrip suite);
  policy-engine decision tables; Unknown-variant preservation.
- **Harness-integration** (`#[ignore = "Requires localharness binary"]`):
  spawn/handshake/init against the pinned wheel binary — **no API key
  needed** (verified: init succeeds with a placeholder key), so these run in
  CI wherever the wheel installs.
- **Full-integration** (`#[ignore = "Requires API key"]`): real chat, tool
  round-trip, policy confirmation flow, structured output, resume.
- CI: a new integration matrix group `antigravity`; a pipeline step
  `pip install google-antigravity==<pinned>`.

## Example projects (tracked in docs/EXAMPLE_PROJECTS.md)

Flagship examples demonstrating the dual-mode value:

1. **repo-auditor** — Antigravity agent with a repo workspace + read-only
   built-ins + custom `#[tool]` severity classifier; walks a codebase and
   files a structured report (structured output mode).
2. **release-notes-bot** — Interactions API for summarization + Antigravity
   agent for git archaeology; shows both modes sharing one tool set.
3. **doc-gardener** — trigger-driven agent that watches a docs/ tree and
   keeps cross-references fresh (triggers + file edit tools + policies).
4. **support-triage** — MCP server integration (ticket system) + hooks for
   audit logging + subagents for parallel investigation.

## Out of scope (v1)

- Wheel auto-download (`harness fetch`) — discovery-only in v1.
- Vertex endpoint config (types are present; tested path is Gemini API key).
- Windows process-management edge cases beyond what CI covers.
- OAuth (the SDK itself is API-key only today).

## Follow-ups

### Done (repo_auditor ergonomics pass)

The five ergonomics follow-ups discovered while building `repo_auditor` are
now implemented:

- ~~**Announce workspace roots to the model automatically**~~ — done. The
  wire protocol has *no* native announcement field (`FilesystemWorkspace`
  carries only `directory`; `PermissionsConfig.enforce_workspace_validation`
  governs enforcement, not disclosure — confirmed against the shipped 0.1.5
  descriptor), so this is prompt injection: `spawn()` appends a delimited
  workspace note to the effective system instructions (the stored string is
  never mutated), opt-outable via `with_workspace_announcement(false)`. The
  same note is appended to every subagent's instructions.
- ~~**Trajectory identity on `AgentEvent`s**~~ — done. The `ToolAction`
  event is now a struct variant carrying `trajectory_id: Option<String>`.
  `ToolAction::InvokeSubagent` gained a typed `name` field + a
  `subagent_name()` accessor; harness 0.1.5 emits an empty `invokeSubagent`
  action on the wire (verified via `LOUD_WIRE`), so the name is `None` there
  and the field is forward-compatible (Evergreen `extra` preserves anything
  a future harness adds).
- ~~**Accepted/denied marker on `ToolAction` events**~~ — done. The
  `ToolAction` event carries a `ToolDecision` (`Allowed` / `Denied{reason}`),
  wired from the confirmation/policy decision path.
- ~~**Unwrap `ToolOutcome.result` from the wire envelope**~~ — done. Post-tool
  hooks receive the inner value, not the `{"result": ...}` envelope.
- ~~**Surface harness-internal noise errors distinctly**~~ — done.
  `AgentEvent::Error` is now `{ message, severity }` with an `ErrorSeverity`
  (`Transient` / `Terminal`); turn-ending failures still go through the
  `AntigravityError::Turn` path.

### Still open

- **Async hooks** + the `workspace_only` policy combinator.
- **Background consumer for trigger turns** (surface trigger-turn output via
  a documented channel/callback instead of halt-and-drain).
- **Mockable transport trait** (unit-test protocol logic without the binary).
- **`harness fetch`** (download + extract the matching wheel binary).
