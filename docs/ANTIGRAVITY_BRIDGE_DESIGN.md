# Antigravity Bridge Design

Status: **Implemented (phase 1)** — `src/antigravity/`, feature `antigravity`,
user guide `docs/ANTIGRAVITY.md`.

> **Pre-implementation design.** This document was written before the
> bridge was built. Its [public API sketch](#public-api-sketch),
> wire-debugging plan and example list are that plan, not a description of
> what shipped: names in the sketch differ from the real API. Its [module
> layout](#module-layout) began as the plan too and has been updated by hand
> since, so check it against `src/antigravity/`. What shipped is the code in
> `src/antigravity/` and its rustdoc, and the user guide
> `docs/ANTIGRAVITY.md`; trust those over this file. What *is* kept current here is the record: the harness upgrade
> table and its findings, the [Python SDK parity
> table](#python-sdk-parity-google-antigravity-0123), the
> [#494 decision](#interactions-mode-transport-494-decision), and the
> lessons near the end. Dated harness findings live here, keyed by harness
> version (D-032); the guide and the rustdoc name the version only.

Deviations from this design in phase 1:

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
- Hooks are synchronous closures, each run on a thread of its own; one with
  async work blocks on the runtime there (`Handle::current().block_on`).
  Every Python SDK lifecycle hook is modeled (#493). Policies follow the
  harness evaluator's precedence (named over server-wide over wildcard,
  then deny over ask over allow) and support conditions, ask-user handlers
  and the harness-side auto policy; they are evaluated client-side in the
  pre-tool callback (D-022). `workspace_only` is
  `with_workspace_containment`, on by default.
- The session transport is driven sequentially by the turn loop (no
  background reader task); cancellation uses a shared sink handle. A mockable
  transport trait was deferred — protocol logic is unit-tested at the serde
  layer and against the real binary instead.
- Consequence of the sequential transport: trigger-initiated harness turns
  are surfaced only when the caller consumes them with
  `AntigravityAgent::next_trigger_turn` (a pull, not a background reader).
  Otherwise the next `chat`/`send_streaming` halts a still-running trigger
  turn and drains its events before sending the user's input (same
  discipline as per-turn timeout recovery), so trigger turns cannot desync
  client-driven turns.
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
   constant `SUPPORTED_HARNESS_VERSION`, currently `"0.1.23"`); integration
   tests run against exactly that wheel.
2. **Evergreen at the wire layer**: every event envelope deserializes unknown
   oneof variants into `Unknown { event_type, data }` and continues; unknown
   fields are preserved for roundtrip. Protocol drift degrades gracefully
   instead of breaking.
3. **`ClientInfo{language: "rust", version: <crate version>}`** is sent
   honestly so the harness can version-gate if it grows that capability.
4. **Check the version at spawn** and **watch for releases** (#499). The
   binary has no version of its own, so `spawn()` reads the wheel's
   dist-info beside it and warns on a mismatch (`harness_version()`
   exposes it; init failures and turn timeouts name a known mismatch). It
   warns rather than refuses: the failures a mismatch causes are already
   loud, and a deliberate newer wheel is how a bump is tested. The
   `antigravity-release-watch.yml` workflow diffs PyPI's latest wheel's
   protobuf descriptors against the pin's daily and keeps an
   `antigravity-release` issue open until the pin catches up, so a release
   no longer waits for someone to notice it.

## Binary acquisition & discovery

The binary ships only inside platform-specific PyPI wheels (0.1.23: macOS
arm64/x86_64, Linux x86_64/aarch64, Windows amd64/arm64). Discovery order
(mirrors the Python SDK, extended):

1. Explicit path: `AntigravityAgent::builder().with_harness_path(...)`.
2. `ANTIGRAVITY_HARNESS_PATH` env var (same var the Python SDK honors).
3. Scan installed `google-antigravity` pip package for
   `google/antigravity/bin/localharness[.exe]` (site-packages of `python3`
   on PATH, then on Windows of `python`, which python.org's installer
   provides instead, plus their user-site directories).
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
`agent: "antigravity-preview-09-2026"` (`DEFAULT_ANTIGRAVITY_AGENT`) — the same harness run server-side.
genai-rs therefore offers the Antigravity harness at two altitudes:

- **Hosted**: `client.interaction().with_agent(DEFAULT_ANTIGRAVITY_AGENT)`
  (plus Environments/Agents resources — see `docs/INTERACTIONS_API_GAP.md`).
- **Local**: `genai_rs::antigravity` spawning `localharness` with local
  workspaces, local tool execution, and local policy enforcement (this doc).

## Module layout

> Pre-implementation plan, later updated by hand. `src/antigravity/` is the
> authority; check the file list there before relying on this one.

Feature-gated top-level module. The original sketch had one file per
concern; the Python SDK parity work (#493) split the builder and the
protocol into directories (D-021). The layout as of 0.1.23 parity:

```text
src/antigravity/
  mod.rs           Re-exports the public API
  agent.rs         AntigravityAgent: chat, streaming turns, trigger turns,
                   session start/end, shutdown
  builder/         AgentBuilder: mod.rs (setters, spawn), hooks.rs (hook
                   setters), options.rs (model, subagent and harness option
                   setters), validate.rs (spawn-time checks), wire.rs (the
                   HarnessConfig it sends)
  config.rs        Capabilities, BuiltinTool, ModelTarget, workspaces
  instructions.rs  SystemInstructions: appended, edited or custom prompt
  prompt.rs        Prompt, Media, SlashCommand: what a turn sends
  options.rs       Budget, RetryConfig, RunCommandOptions, InlineSkill,
                   SubagentSkills, SessionContinuation
  subagent.rs      Subagent (static subagent definitions)
  mcp.rs           McpServer
  hooks.rs         Hook argument and verdict types, LifecycleHooks
  policies.rs      Policy, the policy module's constructors, PolicyEngine
                   (harness precedence, evaluated client-side; D-022)
  lifecycle.rs     Answers to the lifecycle hooks (turn, stop, tool error,
                   compaction, session start/end)
  turn.rs          One turn: step routing, deltas, terminal steps, deadline
  turn_requests.rs Tool calls, pre/post-tool hooks, auto-policy decision
                   requests, confirmations and questions within a turn
  hook_mapping.rs  Harness step/action to ToolInvocation mapping
  streaming.rs     AgentEvent, ToolAction, ChatResponse
  triggers.rs      Interval, custom and file-change triggers; TriggerContext
  tools.rs         Custom tool dispatch onto FunctionRegistry / ToolService
  session.rs       WebSocket transport: connect, send/receive, wire inspection
  handshake.rs     Binary-proto InputConfig/OutputConfig encode/decode
  process.rs       Discovery, spawn, ordered shutdown
  harness_stderr.rs The stderr drain and its bounded tail
  process_tree.rs  Signals the harness's process group and its children's
  error.rs         AntigravityError
  protocol/        Proto-JSON wire types: config.rs (HarnessConfig),
                   options.rs (budget, retry, compaction, skills, policy
                   rules), hook_messages.rs (callHookRequest,
                   policyDecisionRequest and answers), input.rs, output.rs,
                   step_update.rs, enums.rs
```

Unit tests sit beside their module as `<module>_tests.rs` (D-014).

Feature flag: `antigravity` (default **off** initially; revisit after
stabilization). Dependencies added under the flag: `tokio-tungstenite`
(rustls, consistent with the crate's TLS stance). No protobuf crate — the two
binary messages are hand-encoded; everything else is serde JSON.

## Public API sketch

> Pre-implementation: the shipped API differs (for example the pre-tool
> hook returns `PreToolDecision`, and there is no `with_model_target`). See
> `docs/ANTIGRAVITY.md` for code that compiles.

<!-- ignore: a pre-implementation sketch; its names are not the shipped API, and this file is not a doctest -->
```rust,ignore
use genai_rs::antigravity::{AntigravityAgent, policy};

let mut agent = AntigravityAgent::builder()
    .with_api_key(std::env::var("GEMINI_API_KEY")?)   // or with_model_target(...)
    .with_model(genai_rs::DEFAULT_MODEL)
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
- `ClientBuilder::add_wire_inspector(Arc<dyn WireInspector>)` /
  `AgentBuilder::add_wire_inspector(...)` for programmatic, per-client
  capture (snapshot tests, bug reports).
- Built-in sinks: the existing colored stderr printer (`LOUD_WIRE=1` env
  sugar — zero behavior change for current users) and a `tracing` forwarder
  emitting at `DEBUG` to target `genai_rs::wire` (selected with
  `RUST_LOG=genai_rs::wire=debug` in a program that installs a `tracing`
  subscriber).
- Fixes folded in: error response bodies logged, UTF-8-safe truncation,
  no request serialization when disabled.

## Testing strategy

- **Unit**: handshake encode/decode golden tests; proto-JSON roundtrips for
  every protocol type (proptest, matching existing roundtrip suite);
  policy-engine decision tables; Unknown-variant preservation.
- **Harness-integration** (`#[ignore = "Requires localharness binary"]`):
  spawn/handshake/init against the pinned wheel binary — **no API key
  needed** (init succeeds with a placeholder key, and none of these runs a
  turn), so they run in CI wherever the wheel installs. Tests that run a
  turn without a real model are scripted (next item).
- **Scripted harness** (`tests/antigravity_scripted.rs`, same ignore
  reason): the real harness against a scripted local model reached through
  `with_gemini_base_url`, so turns run end to end with no key and a
  deterministic "model" — builtins through the pre-tool hook, custom tools
  through policies, containment both ways, cancellation, a trigger turn,
  backend and tool errors, the turn deadline over a hung async tool, a
  blocking sync tool and a blocking hook, the key-free harness environment
  under a custom host, the process-tree kill, and every Python SDK option
  the bridge models (`tests/antigravity_scripted/`: options, limits,
  skills, subagents, hooks, lifecycle, MCP, policies, triggers,
  cancellation, and the parity-table guard). This replaced
  the planned mockable transport trait: it tests the bridge *and* the
  harness behavior it depends on, which a mock of the socket could not. CI runs it in the
  required `Test Suite (antigravity)` job with the key blanked, forks
  included.
- **Full-integration** (`#[ignore = "Requires API key"]`): real chat, tool
  round-trip, policy confirmation flow, structured output, resume — what a
  real model's choices exercise.
- CI: the `test-antigravity` job installs `google-antigravity==<pinned>`,
  runs the scripted suite on every PR, and the live harness suite where the
  key is available.

## Example projects

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
  same note is appended to every subagent's instructions. Since D-023 the
  note goes only with custom instructions, the agent's or a subagent's:
  the harness's own prompt, which appended instructions keep, lists the
  workspaces itself.
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
  (`Transient` / `Severe`); turn-ending failures still go through the
  `AntigravityError::Turn` path.

### Harness upgrades

| Move | What broke | Direction | Handling |
|------|-----------|-----------|----------|
| 0.1.5 → 0.1.10 | `STATE_IDLE` → `STATE_FULLY_IDLE`; `usageMetadata` → `usageUpdate` | inbound | Aliased (D-003); one build reads both |
| 0.1.10 → 0.1.18 | `userInput` string arm removed, `complexUserInput` renamed to `userInput` (a `UserInput` message) | outbound | Cannot be aliased — the harness accepts one shape — so the build drives 0.1.18 only |
| 0.1.18 → 0.1.23 | `STATE_WAITING_FOR_SUBAGENTS` added (non-terminal); `runWorkflow` toggle added (0.1.18 rejects it); `create_file` gained `Append` | inbound and outbound | State modeled; `runWorkflow` written disabled, so the build drives 0.1.23 only |

Lessons from 0.1.18, each now encoded somewhere:

- **An outbound break is invisible on the socket.** The harness logged
  `failed to unmarshal InputEvent` to stderr and never started the turn;
  the stall diagnosis now quotes that line.
- **A field can keep its name and change type.** The drift guard checked
  names only and caught this break by luck (`complexUserInput` vanished in
  the same release); it now checks the types of hand-encoded fields too.
- **The gate moved.** 0.1.18 gates every observed call through the
  pre-tool hook, not tool confirmations, and names calls there by step
  field (`invoke_subagent`) and bare MCP tool name. Policies are now mapped
  onto their targets on that path, and hook-denied calls (which arrive as
  error steps) are marked `ToolDecision::Denied`.
- **Hook arguments are the model's.** On the hook path `args` are the
  harness's tool-schema names (`AbsolutePath`, `CommandLine`), not the
  action record's; two example gates keyed on the latter were silently
  dead. Documented, with the examples fixed to fail closed.
- **Defaults are not stable.** 0.1.10 exposed `manage_task`/`schedule` to
  every agent with no toggle; 0.1.18 made them toggles defaulting off and
  made the system prompt depend on `agent_behavior` (autonomous by
  default, which suppresses `ask_question`). Every toggle is now written
  explicitly, `schedule` stays off, and behavior is configurable.

Probing technique worth reusing: point `GeminiApiEndpoint.base_url` at a
local capture server and read the harness's model request. It gives the
exact tool declarations (and parameter schemas) and system prompt a config
produces, deterministically, with no API key. The builder exposes it as
`with_gemini_base_url`, and the scripted suite is built on it.

Process lifecycle findings (0.1.23, 2026-11-04, same technique):

- The harness gives every `run_command` command a process group of its
  own (pgid == the command's pid). Killing the harness's pid, or its
  group, orphans the command (reparented to init, still running). The
  bridge now spawns the harness in its own group and, on the forced paths,
  signals that group and each child's group (`process_tree.rs`).
- Stdin EOF is the only graceful exit: the harness then stops its command
  groups itself. SIGTERM takes the default action (the harness dies of
  the signal, nothing cleaned up), so the shutdown escalation's SIGTERM is for the harness's
  children, not the harness.
- Stdio MCP servers inherit the harness's group and exit on their own
  stdin EOF when it dies.
- With a custom `baseUrl` and no `apiKey` (or `apiKey: ""`, which proto3
  JSON treats as unset), the harness sends its own environment's
  `GEMINI_API_KEY` to that host. So `with_gemini_base_url` requires a
  non-blank `with_api_key`, and while a base URL is set the bridge starts
  the harness without `GEMINI_API_KEY` and `GOOGLE_API_KEY` in its
  environment. Commands the harness runs inherit its environment.

Policy, subagent and sandbox findings (0.1.23, 2026-11-05, against a scripted
model; the sandbox one on CI's runners):

- The harness's policy evaluator orders rules named tool over
  `mcp_<server>_*` over `"*"`, then deny over ask over allow, and with any
  rule present fails closed on a call no rule matches (D-022).
- A subagent's own tool calls reach neither the pre-tool callback nor the
  harness's policy evaluator, and workspace containment does not confine
  them: a read-only subagent read a file outside the workspace with
  deny-all rules, a denying hook and containment all set.
- `run_command` with the OS sandbox requested fails where the harness
  reports no sandbox available, as on GitHub's Ubuntu 24.04 runners,
  rather than running unsandboxed.

Cancellation (0.1.23, 2026-11-06, scripted model,
`test_scripted_cancel_halts_a_turn_waiting_on_the_model`): a halt sent
while the turn waits on the model ends the turn without `STATE_CANCELLED`,
so `chat` resolves normally, as on 0.1.10 and 0.1.18, and the next turn
runs. The live `test_antigravity_cancel_handle_halts_an_in_flight_turn`
accepts a `Turn` error as well, so it is not the evidence for this.

Subagent tools and model errors (0.1.23, 2026-11-07, scripted model):

- The harness offers `define_subagent` only beside `invoke_subagent`;
  `with_define_subagent_tool(true)` alone adds no tool. `spawn()`'s
  subagent refusal (D-042) still counts it, in case a release separates
  them.
- A fatal model-backend error (an HTTP 400 from the model endpoint) ends
  the session: the harness reports `Agent execution terminated due to
  error` and closes the WebSocket, so the agent's next call fails with a
  WebSocket error (`test_scripted_model_bad_request_fails_the_turn_and_ends_the_session`
  holds it, accepting `ConnectionClosed` too). Respawn the agent after one.

Questions (0.1.23, 2026-11-07, scripted model,
`test_scripted_a_question_is_gated_like_any_tool_call`): an `ask_question`
call reaches the harness's pre-tool callback, where this client evaluates
its policies and pre-tool hook, before the question is posed, as on
0.1.18. Under `deny_all()` alone the model gets `denied by pre-tool hook:
Denied by policy for tool 'ask_question'` and `on_questions` never runs;
with `allow("ask_question")` beside it, the question reaches the hook.

Turn failures (0.1.23, 2026-11-09, scripted model; D-062):

- A 503 with no retries left and an MCP server that fails to load both end
  the harness's run (it logs `executor run failed`) and close the
  WebSocket, on the same "Agent execution terminated" system error step.
  The step's `httpCode` is 503 for the first and 2, gRPC's `UNKNOWN`, for
  the second, so only a status in 400..600 is read as the model backend's
  (`TurnFailure::ModelBackend`); the rest is `TurnFailure::Trajectory`
  (`test_scripted_retry_config_decides_whether_a_503_ends_the_turn`,
  `test_scripted_mcp_timeout_bounds_listing_tools_and_each_call`).
- After a delegation, the main trajectory's error can come on a
  `STATE_WAITING_FOR_SUBAGENTS` update, and the turn then goes idle with
  none; the agent keeps the first error and fails the turn with it
  (`test_scripted_a_503_after_delegating_fails_the_turn`).
- The harness can take the next message after such a failure, before it
  closes the WebSocket: on a CI runner the next `chat` returned `Ok` with
  no text and no model call, an idle update for that message. Locally (30
  runs) it closed first every time. The agent now refuses the next call
  itself (D-062).

Trigger turns and retried model calls (0.1.23, 2026-11-13, scripted model):

- A turn a trigger starts never reaches the pre-turn callback: with
  `on_pre_turn` refusing every turn, `next_trigger_turn` still returned the
  trigger turn's answer, and the stop and post-turn callbacks fired for it
  (a probe for the 2026-11-09 grade counted the same). A `chat` after it
  was refused. `spawn()` warns when the two are set together
  (`test_scripted_a_trigger_turn_skips_the_pre_turn_hook`).
- A 503 the harness retries arrives as a system error step on the main
  trajectory with `httpCode` 503, as one with no retries left does; only
  the message differs ("Encountered retryable error from model provider:"
  before "Agent execution terminated due to error."). The retry follows as
  the next step of the same trajectory. So a 5xx error step does not mean
  the session is ending, and its `AgentEvent::Error` stays `Transient`
  (#556): severity is decided when the step arrives, before the harness
  has retried or given up, and telling the two apart would mean reading
  the harness's text. The status a backend failure gives the turn is
  dropped once a later model step or error step of the main trajectory
  follows it, and a subagent's is never taken: after a retried 503, a
  model call to an undeclared tool with no output retries left (a system
  error step with `httpCode` 0) ended the turn as
  `TurnFailure::Trajectory`, where the turn had reported
  `ModelBackend { status: 503 }`
  (`test_scripted_a_retried_503_is_a_transient_event_and_not_the_turns_failure`).
  Any later step would not do: after a delegation, a 503 with no retries
  left on the main trajectory was followed in 8 of 40 runs by a
  `STATE_DONE` system step at the next index, the harness's message to the
  model that the subagent had gone idle, before the idle. Dropping the
  status on it failed `test_scripted_a_503_after_delegating_fails_the_turn`
  as `Trajectory` in 2 of 16 runs, each one with that step; dropping it
  only on a model or error step passed 24 of 24, 6 of them with that step.

Subagent names (0.1.23, 2026-11-07, live
`test_antigravity_subagent_is_actually_invoked`): the `invokeSubagent`
action still carries no name, as on 0.1.5 through 0.1.18, so
`ToolAction::subagent_name()` is `None`.

## Python SDK parity (google-antigravity 0.1.23)

Every option the 0.1.23 Python SDK exposes for the harness, and what this
crate does with it. The first column's keys are exactly what
`.github/scripts/dump_sdk_options.py` reads from the wheel's sources
(`Class.field`, `Enum.MEMBER`, `Class.method`, `module.function`, and a
bare `Class` for a class with nothing of its own);
`test_python_sdk_options_are_all_in_the_parity_table`
(`tests/antigravity_scripted/parity.rs`, in the required key-free CI step)
fails when the installed wheel has a key this table lacks, or this table
has one the wheel lacks. The script also fails when anything the SDK
exposes goes unplaced: a module under `google/antigravity/` it neither
reads nor skips with a reason (transport plumbing, the runners; its
`MODULE_SKIP`), a public class in a module it reads that it neither reads
nor skips (outputs, errors, the hook base classes; its `CLASS_SKIP`), or a
name the SDK exports (a module's `__all__`, a package's re-exports, an
`@beta` export) that it does not place. So a new config model, content
type, helper module or exported function cannot go unlisted. A harness
bump updates the table with the wheel; the release watch's issue shows
the new wheel's keys against the pin's.

"Not modeled" rows give the reason, and
`every_parity_row_maps_its_keys_and_gives_a_reason_when_not_modeled`
(`parity.rs`, key-free) fails on a row that maps nothing, or whose
genai-rs cell says "Not modeled" or names no backticked API (whatever its
wording) with an empty Notes cell. Each modeled setting is shown changing
the harness's behavior by a test in `tests/antigravity_scripted/`
(D-010): the model request it sends, the tools it declares, a tool's
result, the turn's outcome and its stop reason, the time between its
model retries, or what it writes to disk. The budget caps and scope, the
retry backoff and the output retries are in `limits.rs`, an inline
skill's metadata and dependent skills in `skills.rs`, and a subagent's
behavior in `subagents.rs`. Four kinds of row have no such test of their
own:

- A constructor or preset that only builds a value the harness never
  sees as such (`Media::from_file`, `RetryConfig::benchmark`,
  `Capabilities::none` and `all`) is unit-tested on the value, whose
  setting is scripted.
- A response accessor (`text`, `thoughts`, `usage`, `conversation_id()`,
  …) reads what the tests assert on rather than changing it.
- A setting harness 0.1.23 ignores has nothing to show, and a scripted
  test asserts that it changes nothing, so the day the harness honors it
  is noticed. `Media::with_description` is not passed to the model (the
  media test). `InlineSkill::add_allowed_tool` and `add_dependent_tool`
  change neither the skill's listing nor its lookup's answer, nor the
  tools declared after the lookup (a deferred MCP tool named as
  dependent stays undeclared), nor how a later call is gated (a call to
  an allowed tool is still denied under `deny_all()`, and a call to a
  tool off the list still runs under `allow_all()`), verified
  2026-11-09 with `test_scripted_skill_allowed_and_dependent_tools_change_nothing_on_this_harness`.
- `RetryConfig::with_jitter` is random by design. The backoff test sets
  it to 0, which makes the harness's waits exact, but a nonzero range
  moves each wait by a random amount that no bounded test tells apart
  from scheduling noise.

### Agent and session

| Python SDK option | genai-rs | Notes |
|---|---|---|
| `Agent.chat` | `AntigravityAgent::chat`, `send_streaming` | Each takes a string or a `Prompt` of text, `Media` and `SlashCommand`s, as Python takes a string or content primitives (see Prompts). |
| `Agent.conversation` | Not modeled as an object | Its methods map onto the agent and `ChatResponse` (see Conversation and response). |
| `Agent.conversation_id`, `AgentConfig.conversation_id` | `conversation_id()`, `with_conversation_id` | Python also checks the id's shape (32+ characters); the harness checks existence. |
| `Agent.is_started` | Not modeled | A value of `AntigravityAgent` exists only once `spawn()` succeeded. |
| `Agent.run_workflow`, `BuiltinTools.RUN_WORKFLOW`, `workflows.agent`, `workflows.call_tool`, `workflows.define`, `workflows.extract_workflow_source`, `workflows.log`, `workflows.parallel`, `workflows.phase`, `workflows.pipeline`, `workflows.validate_workflow_source` | Not modeled (product decision, D-043) | Beta. A workflow is a Python script written against the SDK's `workflows` module (the `workflows.*` keys). `Agent.run_workflow` turns the `run_workflow` builtin on and asks the model, in a chat turn, to run the script with it. Nothing in a workflow can be written in Rust, and running one needs the Python SDK installed beside the harness, the dependency this crate exists to avoid. The toggle is written disabled. |
| `Agent.sandbox_status` | `AntigravityAgent::sandbox_status` | `spawn()` warns when a requested sandbox is unavailable, as Python does. |
| `AgentConfig.save_dir` | `with_save_dir` | Python makes a temporary one when unset; the harness then keeps nothing. |
| `AgentConfig.app_data_dir` | `with_app_data_dir` | |
| `AgentConfig.session_continuation_mode`, `SessionContinuationMode.CREATE_ONLY`, `SessionContinuationMode.CREATE_OR_RESUME`, `SessionContinuationMode.RESUME` | `with_session_continuation(SessionContinuation::…)` | `Resume` of a missing conversation fails `spawn()`. |
| `BaseLocalAgentConfig.env` | `add_harness_env` | Set in the handshake and on the process, as Python does; refused for API key variables while a custom Gemini host is set. |
| `BaseLocalAgentConfig.workspaces`, `AgentConfig.workspaces` | `add_workspace`, `with_workspace` | Python defaults to the working directory; genai-rs to none. |
| `AgentConfig.response_schema`, `CapabilitiesConfig.finish_tool_schema_json` | `with_response_schema` | |
| `AgentConfig.debug_config`, `DebugConfig.enable_server_side_tracing`, `DebugConfig.logging_level` | Not modeled | Python logging settings; the local connection never sends the tracing flag. genai-rs logs through `tracing` and wire inspectors (`add_wire_inspector`, `LOUD_WIRE`). |
| `AgentConfig.lightweight` | Not modeled (preset) | Minimal tools, `AgentBehavior::Minimal`, no subagents, compaction at 65,536 tokens: each is a setter here. |
| `AgentConfig.eval` | Not modeled (preset) | `policy::allow_all()`, `RetryConfig::benchmark()`, `ThinkingLevel::High`, daemons on, no image tool or subagents: each is a setter here. |

### Models

| Python SDK option | genai-rs | Notes |
|---|---|---|
| `LocalAgentConfig.model`, `ModelTarget.name` | `with_model`, `with_image_model` | One text model and one image model, the two roles the harness has. |
| `LocalAgentConfig.models`, `ModelTarget.types`, `ModelTarget.endpoint`, `ModelType.TEXT`, `ModelType.IMAGE` | `with_model`, `with_image_model` | The image model goes out whenever `generate_image` is on (default `DEFAULT_IMAGE_MODEL`): without one the harness failed every image call. Arbitrary target lists are not modeled; they serve no third role. |
| `LocalAgentConfig.api_key`, `GeminiAPIEndpoint.api_key` | `with_api_key` | |
| `ModelEndpoint.base_url` | `with_gemini_base_url` | Needs a non-blank `with_api_key`. |
| `ModelEndpoint.http_headers` | `add_model_header` | |
| `GeminiAPIEndpoint.options`, `GeminiModelOptions.thinking_level`, `ThinkingLevel.MINIMAL`, `ThinkingLevel.LOW`, `ThinkingLevel.MEDIUM`, `ThinkingLevel.HIGH` | `with_thinking_level(crate::ThinkingLevel)` | Sent as `generationConfig.thinkingConfig.thinkingLevel`. |
| `ThinkingLevel.EXTRA_HIGH` | `ThinkingLevel::Unknown { level_type: "extra_high", .. }` | The crate's `ThinkingLevel` follows the Interactions API's four levels; the harness forwards this value unchanged. |
| `GeminiModelOptions.service_tier`, `ServiceTier.STANDARD`, `ServiceTier.PRIORITY`, `ServiceTier.FLEX` | `with_service_tier(crate::ServiceTier)` | Sent as the request's `serviceTier`. |
| `GeminiModelOptions.max_output_tokens` | `with_max_output_tokens` | Harness default 65,535. |
| `LocalAgentConfig.vertex`, `LocalAgentConfig.project`, `LocalAgentConfig.location`, `VertexEndpoint.api_key`, `VertexEndpoint.project`, `VertexEndpoint.location`, `VertexEndpoint.options` | Not modeled (product decision) | Vertex AI endpoints, out of scope since v1: the supported path is a Gemini API key. The wire types exist (`protocol::VertexEndpoint`). |
| `LocalOpenAIAgentConfig.model`, `LocalOpenAIAgentConfig.base_url` | Not modeled (product decision) | An OpenAI-compatible local server (Ollama, LM Studio) as the model backend, through the harness's Gemma endpoint. This crate targets the Gemini API. |
| `LiteRTAgentConfig.model_path`, `LiteRTAgentConfig.backend`, `LiteRTAgentConfig.enable_speculative_decoding`, `LiteRTAgentConfig.cache_dir`, `LiteRTAgentConfig.audio_backend`, `LiteRTAgentConfig.vision_backend`, `LiteRTAgentConfig.port`, `LiteRTAgentConfig.download_if_missing`, `LiteRTBackend.CPU`, `LiteRTBackend.GPU`, `LiteRTBackend.NPU` | Not modeled (product decision) | An on-device LiteRT model server that the Python SDK runs in its own process; a different backend from the Gemini API. |
| `InteractionsAgentConfig` | Not modeled (blocked) | The Interactions-mode transport: harness 0.1.23 rejects every `interaction.create` that carries an agent config, the Python SDK's included (#494, D-018; see Interactions-mode transport below). |

### Instructions

| Python SDK option | genai-rs | Notes |
|---|---|---|
| `AgentConfig.system_instructions`, `SubagentConfig.system_instructions` | `with_system_instructions(impl Into<SystemInstructions>)` | A string appends, as in Python. |
| `TemplatedSystemInstructions.sections`, `SystemInstructionSection.content`, `SystemInstructionSection.title` | `SystemInstructions::appended`, `add_section`, `with_title` | Default title `user_system_instructions`, as in Python. |
| `TemplatedSystemInstructions.identity` | `SystemInstructions::with_identity` | With custom instructions it becomes a leading `identity` part. |
| `CustomSystemInstructions.text`, `CustomSystemInstructions.title` | `SystemInstructions::custom`, `with_title` | Replaces the harness prompt; genai-rs then announces the workspaces itself. |

### Prompts

| Python SDK option | genai-rs | Notes |
|---|---|---|
| `Image`, `Document`, `Audio`, `Video`, `_BaseMedia.data`, `_BaseMedia.mime_type` | `Media::new(bytes, mime_type)`, `Prompt::add_media` | One type; the MIME type says which. The harness hands it to the model as inline data after the text. A MIME type the Python SDK does not list is sent with a `warn!`, not refused. |
| `_BaseMedia.description` | `Media::with_description` | Harness 0.1.23 does not pass it to the model (verified 2026-11-05). |
| `_BaseMedia.from_file` | `Media::from_file` | The MIME type comes from the extension (`detect_mime_type`). |
| `types.from_file`, `types.from_bytes` | `Media::from_file`, `Media::new(bytes, mime_type)` | Python picks the media class from the MIME type; genai-rs has the one `Media` type. A description goes on with `with_description`. |
| `SlashCommand.name`, `BuiltinSlashCommandName.PLAN` | `SlashCommand::Plan`, `Prompt::add_slash_command` | The harness puts `/plan` before the prompt's text. |

### Conversation and response

| Python SDK option | genai-rs | Notes |
|---|---|---|
| `Conversation.create` | `AgentBuilder::spawn` | |
| `Conversation.chat`, `Conversation.send` | `chat`, `send_streaming` | |
| `Conversation.receive_steps`, `Conversation.receive_chunks`, `ChatResponse.chunks`, `ChatResponse.tool_calls`, `ChatResponse.resolve` | `send_streaming`'s `AgentEvent`s | Steps arrive as `ToolAction` and `Error` events, chunks as `TextDelta` and `ThoughtDelta`; `chat` drives the turn to its end. |
| `ChatResponse.text`, `ChatResponse.thoughts`, `ChatResponse.structured_output`, `ChatResponse.stop_reason`, `ChatResponse.usage_metadata`, `Conversation.last_response`, `Conversation.get_last_structured_output`, `Conversation.last_turn_usage` | `ChatResponse::text`, `thoughts`, `structured_output`, `stop_reason`, `usage` | Python's response streams; genai-rs returns it once the turn ends. |
| `ChatResponse.cancel`, `Conversation.cancel` | `cancel`, `cancel_handle()` | |
| `Conversation.conversation_id`, `Conversation.sandbox_status` | `conversation_id()`, `sandbox_status()` | |
| `Conversation.disconnect` | `shutdown` | |
| `Conversation.wait_for_wakeup` | `next_trigger_turn(wait)` | Waits for a turn the agent starts on its own, then drives it and returns its response. |
| `Conversation.history`, `Conversation.clear_history`, `Conversation.turn_count`, `Conversation.compaction_indices` | Not modeled | The SDK's own bookkeeping over the steps it has received. genai-rs keeps none, so there is nothing to clear: keep the `send_streaming` events you need, and take compaction step indices from `on_compaction`. A resumed conversation's history is `initial_history()`. |
| `Conversation.total_usage`, `Conversation.trajectory_usages` | Not modeled | Sums of the per-turn usage: add up `ChatResponse::usage()`. Usage is not split per subagent trajectory. |
| `Conversation.is_idle`, `Conversation.wait_for_idle` | Not modeled | A turn borrows the agent mutably, so nothing else can run on it mid-turn; `chat` returns once the agent is idle. |
| `Conversation.connection` | Not modeled | The SDK's transport object; genai-rs's session is internal. |

### Capabilities and builtins

| Python SDK option | genai-rs | Notes |
|---|---|---|
| `AgentConfig.capabilities`, `BaseLocalAgentConfig.capabilities` | `with_capabilities(Capabilities)` | Python's local default enables every default tool; genai-rs defaults to read-only. |
| `CapabilitiesConfig.enabled_tools`, `CapabilitiesConfig.disabled_tools`, `SubagentCapabilities.enabled_tools`, `SubagentCapabilities.disabled_tools` | `Capabilities::enable`, `disable`, `none`, `all` | |
| `CapabilitiesConfig.enable_subagents` | `Capabilities::enable(BuiltinTool::InvokeSubagent)` | |
| `CapabilitiesConfig.agent_behavior`, `SubagentCapabilities.agent_behavior`, `AgentBehavior.AUTONOMOUS`, `AgentBehavior.INTERACTIVE`, `AgentBehavior.MINIMAL` | `with_agent_behavior`, `Subagent::with_agent_behavior` | |
| `BuiltinTools.LIST_DIR`, `BuiltinTools.SEARCH_DIR`, `BuiltinTools.FIND_FILE`, `BuiltinTools.VIEW_FILE`, `BuiltinTools.CREATE_FILE`, `BuiltinTools.EDIT_FILE`, `BuiltinTools.RUN_COMMAND`, `BuiltinTools.ASK_QUESTION`, `BuiltinTools.INVOKE_SUBAGENT`, `BuiltinTools.GENERATE_IMAGE`, `BuiltinTools.SEARCH_WEB`, `BuiltinTools.READ_URL_CONTENT`, `BuiltinTools.FINISH` | `BuiltinTool::*` | Same wire names. |
| `BuiltinTools.START_SUBAGENT` | `policy::*("start_subagent")` | A policy alias for `invoke_subagent`, as Python keeps the enum alias. |
| `types.normalize_builtin_tool_name` | Not modeled | Maps the deprecated `start_subagent` to `invoke_subagent`; genai-rs's policies take either name. |
| `BuiltinTools.SCHEDULE` | Not modeled | A self-scheduled turn starts whenever the harness decides, and its events would land in a `chat` turn that starts meanwhile: the harness marks no turn boundary the client could tell apart. Trigger turns avoid this because the bridge serializes their delivery with `chat`; `next_trigger_turn` consumes those. Written disabled. |
| `BuiltinTools.read_only`, `BuiltinTools.none`, `BuiltinTools.all_tools` | `Capabilities::read_only`, `none`, `all` | genai-rs's read-only set keeps the three finders and leaves out `read_url_content` (egress) and `schedule`. |
| `BuiltinTools.default`, `BuiltinTools.deprecated`, `BuiltinTools.file_tools`, `BuiltinTools.minimal`, `BuiltinTools.nondestructive` | Not modeled (presets) | Tool lists; build them with `Capabilities::none().enable(..)`. |
| `CapabilitiesConfig.run_command_config`, `SubagentCapabilities.run_command_config`, `RunCommandConfig.timeout_seconds`, `RunCommandConfig.enable_daemons`, `RunCommandConfig.enable_sandbox` | `Capabilities::with_run_command(RunCommandOptions)` | Timeout kills the command; daemons add `IsDaemon` to the schema. |
| `CapabilitiesConfig.tool_output_truncation_config`, `ToolOutputTruncationConfig.max_tokens` | `with_tool_output_truncation` | |
| `CapabilitiesConfig.mcp_max_output_bytes` | `with_mcp_max_output_bytes` | |
| `CapabilitiesConfig.compaction_threshold`, `AgentConfig.compaction_config`, `CompactionConfig.token_threshold` | `with_compaction_threshold` | Both spellings written, as Python does. |
| `AgentConfig.tools` | `add_tool`, `add_tool_service` | |
| `ToolContext.conversation_id`, `StateStore.get_state`, `StateStore.set_state`, `StateStore.update_state`, `StateStore.lock` | Not modeled | Python hands a `ToolContext` to a tool with a parameter of that type: the conversation id and a per-session state store (`StateStore`, also behind `HookContext`). A genai-rs tool's state lives on its `ToolService`, shared by the tools it returns, and `conversation_id()` gives the id once `spawn()` returns. |

### Subagents

| Python SDK option | genai-rs | Notes |
|---|---|---|
| `AgentConfig.subagents`, `SubagentConfig.name`, `SubagentConfig.description` | `add_subagent(Subagent::new(..).with_description(..))` | |
| `SubagentConfig.capabilities` | `Subagent::with_capabilities` | `invoke_subagent` needs a nesting depth of 2 or more. |
| `SubagentConfig.tools` | `Subagent::add_tool` (by name) | Callables are registered on the parent. |
| `SubagentConfig.model` | `Subagent::with_model` | Name only; the parent's endpoint. |
| `CapabilitiesConfig.max_subagent_depth` | `with_max_subagent_depth` | |
| `CapabilitiesConfig.allowed_subagents`, `SubagentCapabilities.allowed_subagents` | `with_allowed_subagents`, `Subagent::with_allowed_subagents` | Names validated at `spawn()`, as in Python. |
| `SubagentConfig.skills_config`, `SubagentNoneSkillsConfig`, `SubagentSkillsConfig.inherit_config`, `SubagentSkillsConfig.none_config`, `SubagentSkillsConfig.override_config`, `SubagentInheritSkillsConfig.skill_names`, `SubagentInheritSkillsConfig.extra_skills_paths`, `SubagentOverrideSkillsConfig.skills_paths` | `Subagent::with_skills(SubagentSkills::inherit / none / from_paths)` | |
| `SubagentOverrideSkillsConfig.inline_skills` | Not modeled | The Python SDK refuses them on the local harness ("not supported by LocalConnectionStrategy"). |

### Skills, MCP, budgets, retries

| Python SDK option | genai-rs | Notes |
|---|---|---|
| `AgentConfig.skills_paths` | `add_skills_path` | |
| `AgentConfig.inline_skills`, `InlineSkill.name`, `InlineSkill.description`, `InlineSkill.content`, `InlineSkill.allowed_tools`, `InlineSkill.dependent_tools`, `InlineSkill.dependent_skills`, `InlineSkill.metadata` | `add_inline_skill(InlineSkill::new(..))` | Not combinable with skills paths, as in Python. `visibility: hidden` metadata keeps a skill out of the prompt's list, and a lookup's answer names its dependent skills. `allowed_tools` and `dependent_tools` change nothing on 0.1.23 (see above). |
| `AgentConfig.mcp_servers`, `McpStdioServer.command`, `McpStdioServer.args`, `McpStdioServer.type`, `McpStreamableHttpServer.url`, `McpStreamableHttpServer.type` | `add_mcp_server(McpServer::stdio / http)` | |
| `McpStdioServer.env` | `McpServer::add_env` | |
| `McpStreamableHttpServer.headers` | `McpServer::add_header` | |
| `BaseMcpServerConfig.name` | `McpServer::with_name` | Defaults from the command or host. |
| `BaseMcpServerConfig.enabled_tools`, `BaseMcpServerConfig.disabled_tools` | `with_enabled_tools`, `with_disabled_tools` | |
| `BaseMcpServerConfig.timeout_seconds` | `with_timeout_seconds` | On 0.1.23, bounds both loading the server (connecting and listing its tools; a slower server fails the turn that loads it) and each call (a slower one returns an error to the model). |
| `BaseMcpServerConfig.force_all_tools_eager` | `with_force_all_tools_eager` | Tools declared by name instead of behind `call_mcp_tool`. |
| `McpStreamableHttpServer.timeout`, `McpStreamableHttpServer.sse_read_timeout`, `McpStreamableHttpServer.terminate_on_close` | Not modeled | The Python SDK does not send them to the harness either (its `_to_mcp_server_proto` drops them), so they change nothing on the local harness. |
| `AgentConfig.budget_config`, `BudgetConfig.max_model_calls`, `BudgetConfig.max_tool_calls`, `BudgetConfig.max_input_tokens`, `BudgetConfig.max_output_tokens`, `BudgetConfig.max_total_tokens`, `BudgetConfig.scope`, `BudgetScope.LIFETIME`, `BudgetScope.FORWARD_LOOKING` | `with_budget(Budget)` | A reached cap surfaces as `ChatResponse::stop_reason`, each cap with its own. The scope shows on a resume: `Lifetime` counts the calls made before it, `ForwardLooking` does not. |
| `AgentConfig.retry_config`, `RetryConfig.api_retry`, `RetryConfig.model_output_retry`, `ModelAPIRetryConfig.max_retries`, `ModelAPIRetryConfig.initial_sleep_duration_ms`, `ModelAPIRetryConfig.exponential_multiplier`, `ModelAPIRetryConfig.jitter_range`, `ModelOutputRetryConfig.max_retries` | `with_retry(RetryConfig)` | With no jitter the waits are exact: the initial backoff, then that times the multiplier at each retry (0.1.23's defaults: 4 s and 2, verified 2026-11-09). A call to an undeclared tool counts as malformed output. |
| `RetryConfig.benchmark` | `RetryConfig::benchmark` | |

### Hooks

| Python SDK option | genai-rs | Notes |
|---|---|---|
| `AgentConfig.hooks` | the `on_*` setters | One hook per kind (Python takes a list); combine in the closure. Python's `HookContext` state store is not modeled: closures carry their own state. |
| `hooks.on_session_start`, `hooks.on_session_end` | `on_session_start`, `on_session_end` | Start runs before `spawn()` returns; end during `shutdown()`. |
| `hooks.pre_turn` | `on_pre_turn` | `TurnDecision::Deny` fails the turn and keeps the agent usable. |
| `hooks.post_turn` | `on_post_turn` | |
| `hooks.pre_tool_call_decide`, `HookResult.allow`, `HookResult.message`, `HookResult.modified_args` | `on_pre_tool` → `PreToolDecision::Allow / Deny / Modify` | |
| `hooks.post_tool_call` | `on_post_tool` | |
| `hooks.on_tool_error` | `on_tool_error` | Returning text replaces the error the model sees. |
| `hooks.on_compaction` | `on_compaction` | |
| `hooks.stop`, `StopHookResult.decision`, `StopHookResult.reason`, `StopDecision.ALLOW_STOP`, `StopDecision.CONTINUE` | `on_stop` → `StopDecision::Stop / Continue` | |
| `hooks.on_interaction`, `QuestionHookResult.responses`, `QuestionHookResult.cancelled`, `QuestionResponse.selected_option_ids`, `QuestionResponse.freeform_response`, `QuestionResponse.skipped` | `on_questions` → `QuestionReply`, `QuestionAnswer` | |

### Policies

| Python SDK option | genai-rs | Notes |
|---|---|---|
| `AgentConfig.policies`, `BaseLocalAgentConfig.policies` | `add_policy` | Python's local default is `confirm_run_command()`; genai-rs has none and refuses write tools without a policy or hook. Evaluated client-side in the pre-tool callback (D-022). |
| `policy.allow`, `policy.deny`, `policy.allow_all`, `policy.deny_all`, `Policy.tool`, `Policy.decision` | `policy::allow`, `deny`, `allow_all`, `deny_all`, `confirm` | MCP rules target `mcp_<server>_<tool>`, `mcp_<server>_*` or `mcp_*` (Python: `server/tool`, `server/*`). The server-wide forms match the call's server exactly, as Python's do. |
| `Decision.APPROVE`, `Decision.DENY`, `Decision.ASK_USER` | `PolicyDecision::Allow`, `Deny`, `Confirm` | |
| `policy.ask_user`, `Policy.ask_user` | `policy::ask_user`, `Policy::with_handler` | `confirm` defers to the pre-tool hook instead. A panicking handler denies, as Python's fails closed. |
| `Policy.when` | `Policy::when` | A panicking condition denies, as Python fails closed on an exception. |
| `Policy.name`, `Policy.reason` | `Policy::with_name`, `with_reason` | |
| `policy.auto`, `AutoPolicy.ask_user`, `AutoPolicy.name`, `AutoPolicy.tool`, `AutoPolicy.decision`, `AutoPolicy.when`, `AutoPolicy.reason` | `policy::auto()`, `with_handler`, `with_name` | Harness-side; flagged calls come back as `policyDecisionRequest`s. `tool`, `decision`, `when` and `reason` are fixed by the constructor in both SDKs. |
| `AutoPolicy.model` | Not modeled | The 0.1.23 harness ignores it: the assessment ran on the agent's model even with a model config of that name (verified 2026-11-05). |
| `policy.workspace_only` | `with_workspace_containment` | On by default with a workspace; harness-enforced. |
| `policy.confirm_run_command`, `policy.safe_defaults` | Not modeled (presets) | `[deny("run_command"), allow_all()]`; read-only tools allowed and `ask_user("*", ..)` for the rest. |
| `policy.enforce` | Not modeled | Python's switch to evaluate policies as a pre-tool hook; genai-rs always does. |
| `policy.flatten_policies` | Not modeled | A helper for nested rule lists; `add_policy` takes one rule at a time. |

### Triggers

| Python SDK option | genai-rs | Notes |
|---|---|---|
| `AgentConfig.triggers`, `triggers.every` | `add_trigger(TriggerConfig::new(message, interval))` | A trigger's turn is driven by `next_trigger_turn`, on request; Python consumes it in the background. A background consumer needs turn processing moved off `&mut self` into a task of its own, tracked in #535. |
| `triggers.trigger`, `TriggerContext.send` | `add_custom_trigger(\|ctx\| async move { ctx.send(..).await })` | Sends wait while a turn runs. |
| `triggers.on_file_change` | `triggers::on_file_change(path, poll_interval, callback)` | Polls size and modification time (no OS watcher dependency); skips the directories `watchfiles` skips. Changes made during a turn reach the callback as one batch once the agent is idle. |
| `FileChange.kind`, `FileChange.path`, `FileChangeKind.ADDED`, `FileChangeKind.MODIFIED`, `FileChangeKind.DELETED` | `triggers::FileChange`, `FileChangeKind` | What the callback is handed. |

### Utilities

| Python SDK option | genai-rs | Notes |
|---|---|---|
| `otel.get_otel_hooks` | Not modeled | OpenTelemetry spans for sessions, turns, steps and tool calls, made by a set of hooks (the SDK's `otel` extra). genai-rs logs through `tracing` (`docs/LOGGING_STRATEGY.md`) and wire inspectors (`add_wire_inspector`); an application that wants spans records them from its own `on_*` hooks. |
| `interactive.run_interactive_loop`, `interactive.async_input`, `interactive.ask_user_handler` | Not modeled | A terminal chat loop with a spinner, a yes/no prompt for `ask_user` policies, and answers to `ask_question`. In Rust: call `chat` per line read, answer questions with `on_questions`, and confirm calls with `policy::ask_user(tool, handler)`. |

## Harness config beyond the Python SDK

Proto fields the Python SDK never sets. Not part of the guard; listed so a
reader knows they were looked at.

| Field | genai-rs | Notes |
|---|---|---|
| `SubagentsConfig.disable_define_subagent_tool` | `with_define_subagent_tool` (off by default) | #493 item 3: hides `define_subagent`. |
| `PolicyConfig.rules`, `PolicyRule.*` | Not modeled | The harness evaluator. Rules run client-side (D-022); with rules, the harness fails closed on a call no rule matches (verified on 0.1.23). |
| `RulesConfig.filenames`, `CompactionConfig.checkpoint_interval_tokens` / `max_context_tokens` / `summary_prompt_override` / `use_model_reported_token_count` / `include_last_user_message`, `SkillsConfig.presentation_mode` / `materialized_skills_dir` / `preactivate_skills` / `skill_versions`, `ToolOutputTruncation.error` / `TruncateStrategy.save_full_output`, `Tool.defer_loading` / `Tool.tool_output_truncation`, `HarnessSideTools.tool_search_config` / `permissions`, `RunCommandToolConfig.execution_mode`, `McpServerConfig.auth_provider_type`, `HarnessConfig.initial_trajectory`, `ModelConfig.gemma_endpoint` / `custom_endpoint`, `CustomSystemInstructions.Part.template` / `builtin_section` | Not modeled | No Python SDK option sets them, so there is no reference behavior to match; each would need its own probe first. |

## Interactions-mode transport (#494): decision

**Stay on the bespoke `localharness` proto** (D-018). Do not build the
Interactions-mode transport or a builder switch for it now.

### Probe (google-antigravity 0.1.23, darwin arm64, 2026-11-04)

The harness takes `InputConfig.use_interactions_api = true` (field 6) on the
same stdio handshake and WebSocket, then rejects every `interaction.create`
that carries an agent config, and closes the socket:

```text
Failed to parse initial message: failed to unmarshal ExternalClientEvent:
genai json unmarshal error for genai.ExternalClientEvent: Field
genai.AgentInteraction.dynamic_config has json_only_discriminator 'type'
but 'type' is not a reserved field in message genai.DynamicAgentConfig.
```

| First message | Harness answer |
|---------------|----------------|
| The Python SDK's own event (`interactions_config_converter.build_create_interaction_event`), `agent_config: {"type": "antigravity", ...}` | the error above |
| `agent_config: {"antigravity_config": {...}}` | the error above |
| `antigravity_config` at the top level; `agentConfig` | `Unknown parameter` |
| `interaction.create` with no agent config | `only antigravity_config is supported, got not set` |
| `input` first | `expected create_interaction in initial payload` |

The Python SDK fails the same way end to end: `Agent(InteractionsAgentConfig(...))`
raises `RuntimeError: Failed to initialize interaction ...` with that
message, while `Agent(LocalAgentConfig(...))` completes a turn against the
same scripted model. `InteractionsAgentConfig` is not exported from
`google.antigravity`; the default `LocalAgentConfig` uses the bespoke proto.

No turn can run in this mode on 0.1.23, so the shapes below come from the
SDK's client for it (`interactions_config_converter.py`,
`interactions_event_processor.py`, `interactions_step_assembler.py`), not
from the wire.

### Shapes against genai-rs types

| Event (direction) | Shape, per the SDK client | genai-rs type | Fit |
|-------------------|---------------------------|---------------|-----|
| `interaction.create` (out) | `agent`, typed `tools` (`function`, `bash`, `filesystem{supported_operations}`, `google_search`, `url_context`, `mcp_server`, `manage_task`, `schedule`), `developer_instructions` or `appended_developer_instructions`, `agent_config{type: "antigravity", models, workspaces, policy, policy_config, subagents_config, enabled_hooks, compaction_config, budget_config, retry_config, agent_behavior, ...}` | `InteractionRequest`, `AgentConfig` (hosted `{"type": "antigravity"}`), `Tool` | Envelope and `function`/`google_search`/`url_context`/`mcp_server` tools fit; `bash`, `filesystem`, `manage_task`, `schedule` and every local `agent_config` field do not exist |
| `input` (out) | `{content: [Content]}`, or `input_content` with slash commands | `Content` | Content fits; the event is new |
| `interaction.created` (in) | `{interaction_id}` | `StreamChunk::Created` wants an interaction object | No |
| `step.start` / `step.delta` / `step.stop` (in) | `index`, `step` / `delta`, plus `sub_interaction_id`, `parent_interaction_id`, `depth` | `StreamChunk::StepStart` / `StepDelta` / `StepStop` | Envelope fits; subagent identity only in the raw event |
| Steps `user_input`, `model_output`, `thought`, `function_call`/`_result`, `code_execution_*`, `google_search_*`, `url_context_*`, `mcp_server_tool_*` | | `Step` variants of those names | Fit |
| Steps `view_file_*`, `list_directory_*`, `find_file_*`, `search_directory_*`, `edit_file_*`, `generate_image_*`, `skill_lookup_*`, `elicitation_call`/`_result`, `compaction` | | none | `Step::Unknown` (an error in strict mode, `--cfg genai_strict_unknown`) |
| Deltas `text`, `thought_summary`; `raw_thought` | | `StepDelta::Text`, `ThoughtSummary`; none | Fit; `raw_thought` is `Unknown` |
| `state_update` (in) | `state` (`running`, `waiting_for_tasks`, `fully_idle`, `cancelled`), `reason`, `error` | `StreamChunk::StatusUpdate` (`InteractionStatus`) | No: trajectory states, not interaction statuses |
| `usage_update` (in) | usage payload | `UsageMetadata` | Payload likely; the event is new |
| `interaction.completed` (in) | acknowledges `interaction.complete` (session end) | `StreamChunk::Completed` (the finished interaction) | No: different meaning |
| `function_invocation` (in) / `function_result` (out) | `{call_id, name, arguments}` / `{call_id, name, is_error, result}` | `Step::FunctionCall`, `Content::FunctionResult` | Payloads close; the hosted API has no client-event channel |
| `call_hook_request` (in) / `call_hook_response` (out) | `{request_id, type, <type>_args}` / `{request_id, pre_tool_result{decision, reason, modified_args}, ...}` | none | New |
| `elicitation_result` (out) | `{elicitation_id, multiple_choice{selected_choice_labels, user_input}}` or `confirmation{is_confirmed}` | none | New |
| `interaction.cancel`, `interaction.complete` (out) | | none | New |

### Gaps

- **Triggers:** none. The mode has no trigger event; the SDK's
  `InteractionsAgentConfig` rejects triggers and its
  `send_trigger_notification` raises `NotImplementedError`. This crate's
  interval triggers would have nothing to send.
- **Hooks:** the round trip exists, with the same hooks this crate answers
  on the bespoke proto (session, turn, tool-error, compaction and stop
  hooks, and `modified_args`). Not a blocker; it would be new code either
  way.
- **Policy round trips:** `policy_config` is static rules only. The SDK
  refuses `when` predicates, `ask_user` and `auto` policies in this mode
  ("not yet supported by the GAOS Interactions API protocol in
  localharness"), so there is no equivalent of the bespoke proto's
  `policyDecisionRequest`.
- **Questions:** elicitation steps keyed by `elicitation_id` and answered
  by choice label, not index: a different mapping from `on_questions`.
- **Step model:** about half the step types the harness would emit are not
  in `Step`, so "one event model for hosted and local" needs new variants
  first, and the lifecycle events do not line up with `StreamChunk`'s.

### Why, and when to revisit

It does not run on 0.1.23 (for the SDK either); Python does not default to
it; it drops triggers and dynamic policies; and reusing `Step`/`StepDelta`
covers the envelope and the hosted-API step types, not the local ones.
Revisit when any of these happens: Python makes it the default (the
release watch quotes where each new wheel sets `use_interactions_api`), a
scripted probe completes a turn in this mode, or the bespoke proto shows
signs of deprecation. A Rust re-probe needs only `handshake::InputConfig`
to carry field 6 (`use_interactions_api`, bool) and the scripted model.

### Still open

- **`harness fetch`** (download + extract the matching wheel binary).
- The not-modeled rows of the [parity table](#python-sdk-parity-google-antigravity-0123)
  that are product decisions: Vertex AI, the OpenAI-compatible and LiteRT
  backends, workflows, and `schedule`.
