# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **The mold linker is now opt-in** (#428). `.cargo/config.toml` was checked
  in and set `-fuse-ld=mold` unconditionally, so a clone on a machine without
  mold failed every build before compiling anything:

  ```text
  error: linking with `cc` failed: exit status: 1
    = note: collect2: fatal error: cannot find 'ld'
  ```

  `ld`, `lld` and `gold` are all present in that state — the missing binary
  is mold, which the message never names. CI installs mold explicitly, so
  this only ever hit new contributors and fresh containers.

  The config now ships as `.cargo/config.toml.example`; run
  `./scripts/setup-dev.sh` to enable it. The script asks the compiler the
  question cargo will ask it — link a trivial program with
  `cc -fuse-ld=mold` — rather than checking whether mold is on `PATH`.
  `-fuse-ld=mold` is a compiler-driver option that GCC only accepts from
  12.1 (clang 12+), so on Ubuntu 22.04, whose default gcc is 11, `apt
  install mold` satisfies a `PATH` check while leaving every build broken in
  exactly the way above. The probe covers both conditions at once and needs
  no version table; when it fails the script explains and exits 0, since
  building without mold is fine, just slower. CI enables the config
  alongside its existing mold install, so build times there are unchanged.

- **Scheduled workflows escalate their own failures** (#431). The flakiness
  report failed 22 days running, then stopped firing for ~10 weeks, and none
  of the three transitions produced a signal anyone acted on. `CI Flakiness
  Report` and `Security Audit` now open a rolling `ci-health` issue on a
  failing scheduled run, comment on each subsequent failure, and close it on
  recovery. The issue body carries streak length — "failing since 2026-05-01,
  22 consecutive scheduled runs" — because "it failed again" is what the
  email channel already provided 22 times, and what nobody acted on.

  Bounded honestly: this runs inside the job it reports on, so a run that is
  cancelled, hits its execution limit, or dies before `actions/checkout`
  still fails silently. Catching those needs a `workflow_run` watcher, which
  is deferred with the rest of the liveness work.

  `Security Audit` additionally gains `issues: write`, which the escalation
  needs — and which also switches on `audit-check`'s own per-advisory issue
  reporting, inert until now for want of the permission.

### Changed (breaking)

- **BREAKING**: **`InteractionInput::Content` is now sent as a single
  `user_input` step**
  rather than as a bare content array. Both are valid arms of the API's input
  union, but only the step form accepts video `processing` — the identical
  content in a bare array is rejected with
  `Unknown parameter 'processing' at 'input[1]'`, which names the field
  rather than the input shape and so points at the wrong thing entirely.

  That field is not modeled by this crate yet (#419), so today the change is
  alignment with the canonical form rather than a fix for a pairing a caller
  can express; it means #419 can land without a second wire-shape decision.

  Verified live (2026-08-16, `gemini-3.7-flash`, revision 2026-05-20) that
  the step form is accepted everywhere the bare form is: text, inline image,
  inline audio, inline document, video by URI, and a stored follow-up turn
  via `previous_interaction_id` all complete under both shapes. The steps
  array is also the canonical form under this revision — the one `Turn` was
  removed in favour of.

  Callers using `with_content()` need no change. The wrap is scoped to
  `InteractionRequest::input` rather than to `InteractionInput`'s own
  `Serialize`, so `InteractionResponse::input` — which echoes back what the
  server sent — still re-serializes in the shape it arrived in. A request's
  `Content` input does now deserialize back as
  `InteractionInput::Steps(vec![Step::user_input(..)])`, since the two are
  indistinguishable once serialized. Marked breaking for that reason rather
  than for a signature change — there is none — since anyone who persists a
  built `InteractionRequest` and matches on `input` after reloading it now
  takes a different branch. (#427)

- **Breaking: response structs are now `#[non_exhaustive]`.** The convention
  was already documented in `docs/ENUM_WIRE_FORMATS.md`, but 31 deserializable
  response types had drifted from it — including the five (`Trigger`,
  `TriggerExecution`, `Environment`, `Agent`, `Webhook`) that adding an `extra`
  field had just turned into a breaking change. Under the convention that
  would have been a non-event.

  **What breaks:** struct-literal construction of these types from another
  crate, and `..Default::default()` functional-update syntax. Exhaustive
  `match` on them needs a `..` arm.

  **What still works:** `T::default()` followed by field assignment, on every
  one of these types that derives `Default` — which is most of them, including
  `InteractionResponse`, `UsageMetadata`, `Agent`, `Trigger`, `Environment`,
  `Webhook` and the `*ListResponse` wrappers. That is the migration for most
  call sites:

  ```rust
  // before
  let response = InteractionResponse { id: Some("x".into()), ..Default::default() };
  // after
  let mut response = InteractionResponse::default();
  response.id = Some("x".into());
  ```

  For the few with neither `Default` nor a constructor, deserialize a JSON
  fixture. `ModalityTokens` gains a `new()` in this release for that reason.

  Request-side types are deliberately untouched: `GenerationConfig`,
  `FunctionDeclaration`, the tool configs and the create/update bodies are
  yours to build, and closing them would cost construction syntax while
  gaining the crate nothing.

  `tests/non_exhaustive_responses.rs` now fails the build on a new response
  struct without the attribute, so the backlog cannot re-accumulate.

- **Breaking:** `Content::Video` has a new `processing` field. Code that
  constructs or exhaustively destructures the variant with struct-literal
  syntax needs `processing: None` (or `..`) added. The `Content::video_*()`
  constructors are unaffected.

- **Evergreen `extra` passthrough on response-side resource shapes.**
  `Trigger`, `TriggerExecution`, `Environment`, `Agent`, and `Webhook` now
  carry a flattened `extra` map, so a deserialize-then-serialize cycle no
  longer silently drops fields the crate hasn't modeled.

  Scoped to those five resource shapes themselves. Types nested inside them
  still drop unmodeled keys — `SigningSecret`, `EnvironmentSource`,
  `NetworkConfig` — as do the list envelopes (`AgentListResponse` and its
  siblings), so a new field alongside `next_page_token` is still lost.
  Extending the passthrough down those trees is follow-up work.

  `Content` and `RemoteEnvironment` already did this; the request bodies
  gained it in 0.9.0. These five were the remaining hole, and `Trigger` /
  `TriggerExecution` are the sharpest cases — trigger creation is agent-gated,
  so their response shapes have never been live-verified and a field the API
  returns today would be both invisible and unrecoverable.

  As on the request side, a key colliding with a modeled field wins on
  serialize via `serde_json::to_value`.

- **Breaking:** the five structs above have a new `extra` field. All derive
  `Default`; see the `#[non_exhaustive]` entry above for how to construct
  them from outside the crate — `..Default::default()` is functional-update
  syntax, which that attribute now blocks, so the route is `T::default()`
  followed by field assignment.

- **`StepSummary` gains a `tool_call_count` field, and is now
  `#[non_exhaustive]`.** Exhaustive struct literals and destructuring without
  a `..` rest pattern will fail to compile — both because of the new field and
  because the struct is now closed.

  Closed in the same change that takes the break, deliberately. The field
  addition is source-breaking *only* because the struct was open, and the API
  is expected to grow step types — `mcp_server_tool_call` may start arriving,
  and the recurring SDK-bindings sweep (#421) is the intended detector for
  new ones — so every future counter would
  repeat this break for a purely mechanical reason. Doing it now costs
  consumers nothing extra: they are already recompiling for the new field.

  `Default` is derived, so the migration is `StepSummary::default()` then
  assign — pinned by `tests/ui/pass_step_summary_migration.rs`, a trybuild
  fixture compiled as its own crate. Its counterpart
  `tests/ui/fail_step_summary_struct_literal.rs` pins the attribute itself:
  the migration path compiles the same with or without it, so only a
  `compile_fail` fixture goes red if it is removed.

### Fixed

- **`#[tool]` no longer requires consumer-side dependencies or imports**
  (#402). A crate depending only on `genai-rs` and `genai-rs-macros`, with no
  trait import, now compiles.

  Previously the generated code named `::async_trait` and `::serde_json`,
  which resolve in the *consumer's* dependency graph, so both had to be added
  as direct dependencies; and it called `.declaration()` in method position,
  which needed `use genai_rs::CallableFunction;` at every expansion site.

  ```toml
  # No longer needed alongside genai-rs-macros:
  # async-trait = "0.1"
  # serde_json = "1.0"
  ```

  Scoped to `#[tool]`: a **manual** `CallableFunction` impl still needs both
  `async-trait` and `serde_json` as direct dependencies — the trait is
  declared with `#[async_trait]`, and its `call` signature names
  `serde_json::Value`.

  **Version coupling:** the generated code now references
  `genai_rs::__private`, so `genai-rs-macros` requires a `genai-rs` from this
  release or later. `genai-rs-macros` is a proc-macro crate and declares no
  dependency on `genai-rs`, so Cargo cannot enforce this — pinning the two to
  independent versions with the macro ahead of the library fails at every
  `#[tool]` site with `could not find __private in genai_rs`. Bump them
  together, as the release checklist already does.

  `tests/ui/pass_no_consumer_imports.rs` pins the no-imports behavior.

- **`speech_config` in the `{"speakers": [...]}` form no longer silently
  discards every speaker.** `google-genai` 2.18.x widened the field to
  `SpeakerConfig | List[SpeechConfig]`. Because `SpeechConfig`'s fields are
  all optional and serde ignores unknown keys, the object form matched the
  deserializer's single-object arm and produced one all-`None` config — the
  speakers vanished with no error.

  All three wire forms now normalize to the list: the spec list, the
  `{"speakers": [...]}` object, and the legacy single object.

  Note the Gemini API **rejects both object forms on send** (`400 ... Expected
  an array, got object`, verified live 2026-08-16), so the crate continues to
  emit the list. The leniency is deserialize-only, and it matters because a
  `GenerationConfig` also arrives nested inside a stored `Trigger`
  interaction that another SDK may have created.

  No public type changed — `speech_config` is still `Option<Vec<SpeechConfig>>`.

### Added

- **Video `processing` — segment clipping, frame-rate sampling, and agentic
  mode.** `Content::Video` gained a `processing` field, modeled as the new
  `VideoProcessing` enum with a `with_processing()` setter and a
  `VideoProcessing::segment()` builder.

  A segment window is the difference between the model ingesting a five-second
  clip and the entire video. Re-measured live 2026-08-18 against
  `gemini-3.7-flash` on one source video: **16,198 video input tokens with a
  5s-10s window, 57,778 without.** Among the `static` forms the window is the
  lever — omitting the field, `"static"`, `{"type": "static"}` and
  `{"type": "static", "fps": 1}` all produced the same 57,778, and `fps`
  alone moved nothing.

  Two caveats, both service-side and both worth knowing before relying on
  the figures. When first measured on 2026-08-16 the same window produced
  455 tokens against 57,775 — a ~127x saving rather than today's ~3.6x — so
  the clipped accounting has been revised while the unclipped side held.
  And `"agentic"` no longer reports video tokens at all: it bills as `image`,
  2,112 and 4,158 on two consecutive runs, which makes it the cheapest mode
  rather than one equivalent to `static` — so mode selection *is* a lever
  now, contrary to what the 2026-08-16 reading showed. What held across both
  is that a window reduces ingestion among the `static` forms.

  ```rust
  let video = Content::video_uri("files/abc123", "video/mp4")
      .with_processing(VideoProcessing::segment().start_offset("5s").end_offset("10s").build());
  ```

  One sharp edge, documented on the type: the API accepts `processing` only
  when the video sits inside a `user_input` step. Sending the same content via
  `InteractionInput::Content` returns `400 Unknown parameter 'processing'`, so
  use `InteractionInput::Steps`.

- **`Step::ToolCall`** — the generic `tool_call` step the API actually emits
  for server-side tool invocations, including MCP (#433).

  Before this it landed in `Step::Unknown`, so a successful MCP interaction
  reported no tool calls at all. Verified live (2026-08-16,
  `gemini-3.7-flash`, a real MCP server): the step carries only
  `{id, signature}`. Which server or tool ran is not recoverable from the
  response; `usage.total_tool_use_tokens` is what shows the call happened.

- **`InteractionResponse::tool_calls()` / `has_tool_calls()`** and the
  `ToolCallInfo` borrowed view they return — `id` plus `signature`, which is
  everything the API discloses about a server-side tool call.

- **`StepSummary::tool_call_count`** — where MCP calls are counted.
  `mcp_server_tool_call_count` reads **0** on a successful MCP interaction,
  which is worse than absent: a caller checking it concludes MCP did not run.
  Both fields now cross-reference each other.

  `Step::McpServerToolCall` / `McpServerToolResult` are retained and now
  documented as spec-present but never observed — the same status as
  `Tool::Retrieval`, and unlike `cached_content` (D-005) nothing rejects
  them.

- **`ModalityTokens::new()`** — the type had no `Default` and no constructor,
  so closing it above would otherwise have left `serde` as the only way to
  produce one.

### Removed

- **Breaking: `with_cached_content()` and `InteractionRequest.cached_content`.**
  The Interactions API rejects the field outright:

  ```
  400 Unknown parameter 'cached_content'
  ```

  `cachedContent` and `cached_content_name` are rejected too, so it isn't a
  spelling problem — and neither is it a placement one: `generation_config`
  holds several fields that are not top-level (`transcription_config`,
  `speech_config`), but both spellings are rejected there as well
  (`Unknown parameter 'cached_content' at 'generation_config'`). The
  `/v1beta/cachedContents` resource works fine — a cache creates and reports
  its token count — but nothing in the Interactions API consumes one, so the
  builder method could only ever produce a 400.

  It shipped because the field was modeled from the spec and never live-probed;
  its test asserted the field *serialized* correctly, which it did.

  This follows the `response_mime_type` precedent — rejected outright, so
  removed — rather than the `safety_settings` / `Tool::Retrieval` one, where
  the API explicitly names the feature as Vertex-only and the surface is kept
  for spec parity.

  Implicit caching is unaffected and still reported via
  `usage.total_cached_tokens`.

## [0.10.0] - 2026-08-16

### Added

- **Model constants — one place to change the model.** `DEFAULT_MODEL`,
  `INLINE_VIDEO_MODEL`, `MINIMAL_THINKING_MODEL`, `DEFAULT_IMAGE_MODEL` and
  `DEFAULT_TTS_MODEL` are now public, and every test, example, doctest and
  doc snippet references them instead of a string literal.

  Bumping the model previously meant editing ~590 occurrences of the text
  model plus 27 image and 35 TTS, with no single source of truth. A sweep
  that size reliably misses a few, and a missed one is invisible: the test
  keeps passing against a model nobody meant to still be using, until that
  model is retired and the failure arrives with no obvious cause. It is now
  a one-line change.

  A new `tests/model_literals.rs` guard fails on any hardcoded
  `"gemini-<digit>"` outside those constants, so it cannot silently
  regress. In-crate unit tests use either the constants or an obviously
  synthetic `"test-model"` — they exercise serialization round-trips and
  never cared which model, so neither form is a bump site.

### Changed (breaking)

- **`AgentBuilder`'s per-turn budget defaults to `DEFAULT_TURN_TIMEOUT`
  (300s) instead of being unlimited.** An unbounded turn does not fail when
  the harness stops signalling completion — it *hangs*, which is strictly
  less diagnosable than an error and looks identical to latency. That is
  the exact shape of the 0.1.10 break this crate just shipped a fix for.
  `without_turn_timeout()` restores the old behavior explicitly.

### Changed

- **Antigravity examples moved to `examples/antigravity/`.** All seven —
  the `agent.rs` starter (was `examples/antigravity_agent.rs`) and the six
  projects (was `examples/real_world/*`) — now sit in one directory,
  mirroring `src/antigravity/`. They share a setup story the rest of
  `examples/` does not: every one needs the `antigravity` feature *and* the
  `localharness` binary. Example **names are unchanged**, so
  `cargo run --example repo_auditor --features antigravity` still works;
  only paths moved. The group has its own
  [README](examples/antigravity/README.md), and `examples/real_world/`
  points at it.

- **Default model is now `gemini-3.7-flash`** (from `gemini-3.6-flash`).
  Thinking cost on a trivial prompt is unchanged (68 vs 67 tokens), so the
  `max_output_tokens` headroom in the sampling tests still holds. The full
  live suite was run against it — 208 integration tests — which surfaced
  two capability gaps, neither of which unit tests could have shown:

  - **Inline (base64) video is rejected** with the same `400
    invalid_request` as 3.6, while video by URI works. `INLINE_VIDEO_MODEL`
    remains necessary; the bump does not close that gap.
  - **`ThinkingLevel::Minimal` is rejected**: *"'minimal' is not a
    supported thinking level for this model. Allowed values are: high, low,
    medium."* `gemini-3.6-flash` and `gemini-3.5-flash` still accept it.
    The variant stays valid — model support is what varies — and the new
    `MINIMAL_THINKING_MODEL` constant pins the test so it keeps exercising
    the `minimal` wire path.

- **`LOUD_WIRE` summary labels are scoped to received harness frames.**
  Outgoing `InputEvent` arms have no actions, so qualifying them produced
  `questionResponse/response` and collided with `response`, the HTTP
  category selector. Envelope stripping is likewise harness-only:
  `usageMetadata` is bookkeeping on that wire but a real field on a Gemini
  HTTP response, where stripping it could render a body as
  `(no payload keys)`.

## [0.9.0] - 2026-08-10

### Changed (breaking)

- `InteractionRequest` gains public `safety_settings` and `labels` fields
  and `GenerationConfig` gains `transcription_config` — source-breaking
  for downstream struct literals and exhaustive patterns on these
  constructible structs, hence the minor bump (cargo treats 0.8.x as
  compatible, so this could not ship as 0.8.1). The structs deliberately
  remain constructible (no `#[non_exhaustive]`): per the project's
  versioning philosophy, future field additions will take minor bumps
  rather than trading away struct-literal ergonomics.
  (`antigravity::protocol::MultipleChoice` — behind the `antigravity`
  feature — likewise gains a public flattened `extra` field.)

### Added

- **`LOUD_WIRE` filtering.** The variable now takes a comma-separated
  filter in addition to `1`: category selectors (`request`, `response`,
  `sse`, `upload`, `harness`, `ws`), any WebSocket payload key
  (`stepUpdate`, `toolCall`, …, matched case-insensitively), and a
  `summary` modifier that collapses each event to one line (order-free:
  `1,summary` and `summary,1` agree). The historical "on" spellings (`1`,
  `true`, `yes`, `on`, `all`, empty) still mean everything-pretty-printed.
  Note that the gate was previously "is the variable set at all", so *any*
  value produced the firehose; other values are now parsed as selectors,
  and a value matching no category and no payload key — `0`, `false`,
  `off` — prints nothing where it previously printed everything. Unfiltered
  output was unusable for antigravity sessions, where a few turns produce
  thousands of lines. Exposed programmatically as `wire::WireFilter` and
  `LoudWirePrinter::with_filter`; `LoudWirePrinter` is consequently no
  longer `Copy` (it still derives `Clone`). The antigravity agent
  builder honors the filter too — it previously re-implemented the env
  gate and ignored the value, which meant filtering did not work on the
  one surface that emits WebSocket messages at all.

- **`LOUD_WIRE` selectors reach step actions.** A selector now matches one
  level into a WebSocket payload, so `LOUD_WIRE=mcpTool` (or `runCommand`,
  `viewFile`, …) selects the steps carrying that action — previously those
  names matched nothing at all, because every builtin action lives under
  the single `stepUpdate` key. Summary lines are qualified to match
  (`stepUpdate/mcpTool`), so what you asked for is what the output names.

- **The protocol-drift guard now covers field renames, not just enum
  values.** The 0.1.5 → 0.1.10 upgrade shipped one of each — `STATE_IDLE`
  → `STATE_FULLY_IDLE` (a value) and `usageMetadata` → `usageUpdate` (a
  field) — so a guard checking only enums would have caught half the break
  it was written for. It now also checks the `OutputEvent` and `InputEvent`
  wire fields the crate reads by hand, where a rename yields a silent
  `None` rather than a parse error.

- **Three more antigravity worked examples**, all runnable against a real
  harness and smoke-run in CI: `mcp_toolbelt` (external tools over MCP,
  with a dependency-free stdio server fixture and the `mcp_<server>_<tool>`
  policy-target spelling), `proactive_agent` (`add_trigger`, and observing
  deliveries through a wire inspector — the only way to see one today), and
  `cancellable_turn` (`cancel_handle` from another task, keeping partial
  output). Together with the existing four, every antigravity builder
  surface now has an example.

  Writing them surfaced a harness limitation now documented in
  `docs/ANTIGRAVITY.md`: **a trigger that fires into a conversation with no
  history crashes harness 0.1.10 outright** (`earliest step index is out of
  bounds: 0 vs 0`), taking the session with it. One completed turn first
  avoids it.

- **Antigravity e2e coverage for the last untested surfaces.**
  `with_response_schema`, `CancelHandle`, the `on_questions` hook,
  `add_mcp_server` and subagent *invocation* (as opposed to subagent
  config, which was already covered) are now exercised against a real
  harness — the MCP test drives a stdio server fixture end to end and
  asserts on a token the model cannot have guessed.

  The cancellation test corrected a documented claim: harness
  0.1.10 answers a halt with `STATE_FULLY_IDLE`, the same terminal state as
  a natural completion, so a cancelled turn **resolves normally with
  partial output** rather than failing with `AntigravityError::Turn` as
  `CancelHandle` and `docs/ANTIGRAVITY.md` previously stated. The `Turn`
  error remains the outcome for harness-initiated cancellation.

- **Protocol-drift diagnostics for the antigravity bridge.**
  `protocol::drift_report()` returns every unrecognized wire value seen
  (`"EnumName=WIRE_VALUE" -> count`) with `clear_drift_report()` to reset
  it, `all_wire_values()` on each wire enum exposes the spellings the
  crate recognizes, and `shutdown()` logs the aggregate once. Unknown-value
  preservation previously produced no signal beyond a `warn!` nobody
  reads — which is what let a renamed value silently stop matching. CI
  additionally diffs the installed wheel's protobuf descriptor against
  these enums and fails naming anything the harness can send that the
  crate does not model. See the Debugging section of `docs/ANTIGRAVITY.md`.

- **Two antigravity worked examples**, both runnable against a real
  harness: `examples/real_world/session_resume` (trajectory persistence —
  the `with_save_dir` + `conversation_id()` + `with_conversation_id` round
  trip, and the fact that resuming an unknown id comes back *empty* rather
  than erroring) and `examples/real_world/workspace_explorer` (workspaces
  with real files, the typed `AgentEvent::ToolAction` stream, and an
  `on_pre_tool` hook that refuses by content where a name-based policy
  cannot). Both are covered by new harness integration tests.

- Wire enums accept alias spellings, so a value renamed between harness
  revisions resolves to one variant while `as_wire_str` keeps emitting
  the canonical (current-harness) form.
- A stalled turn now diagnoses itself. When a turn times out having seen
  unrecognized *main-trajectory* states, the timeout names them and
  points at `SUPPORTED_HARNESS_VERSION` instead of reporting an
  undifferentiated stall — the failure that took a wire trace to
  diagnose now reads as a version mismatch on the error itself.


- **Triggers resource** (`/v1beta/triggers`): server-side scheduled
  interactions with full CRUD plus `run_trigger` and
  `list_trigger_executions` — cron agents that fire with no client process
  running. Trigger creation requires a custom agent (gated on standard API
  keys, verified live); the list path and payload schema are live-verified.
- **Environments resource** (`/v1beta/environments`): explicit
  `create/get/list/delete_environment` so many interactions can share one
  container (full lifecycle verified live on a standard key). Handles the
  protobuf-JSON string int64 wire form for `file_count`/`size_bytes`.
- **`TranscriptionConfig`** in `generation_config`
  (`with_transcription_config`): language hints, diarization, custom
  vocabulary, adaptation phrases and timestamp granularities for audio
  input (accepted live).
- **`SafetySetting`/`HarmCategory`/`SafetyThreshold`/`SafetyMethod`** and
  the `safety_settings` request field (+ `with_safety_settings`/
  `add_safety_setting`), and **`labels`** request metadata
  (+ `with_labels`/`add_label`). Both parameters are currently rejected by
  the Gemini API ("available on the Gemini Enterprise Agent Platform",
  verified live 2026-08-08) — modeled for spec parity and forward
  compatibility, like the existing `Retrieval` tool.
- **`AntigravityConfig`** typed agent-config helper for server-side
  `agent("antigravity-preview-05-2026")` interactions (`model`,
  `max_total_tokens`; the config's `antigravity` string is only the
  `agent_config` type discriminant, not an agent ID).
- **Antigravity bridge: `on_questions` hook** — answer the agent's
  `ask_question` batches programmatically (select choices, freeform text,
  skip, or cancel) instead of the previous always-"unanswered" fallback.
  An unmodeled future question type arrives with
  `AgentQuestion::is_unknown_type()` set and its raw payload in `extra`
  (build that fixture with `AgentQuestion::unknown()`).
- `InteractionRequest` and `InteractionInput` implement `Default`, easing
  struct-literal construction (e.g. nested trigger interactions).
- `InteractionRequest`, `GenerationConfig`, `SpeechConfig`, `Tool`,
  `FunctionParameters`, `Trigger` and the trigger request/response types
  implement `PartialEq`, so whole-value assertions work across the new
  resource types uniformly. The pre-existing `Agent` and
  `AgentListResponse` gain it too, completing the set.

### Changed

- **Default model guidance moves to `gemini-3.6-flash`** (from
  `gemini-3-flash-preview`) across docs, examples, and tests, and image
  generation moves to `gemini-3.1-flash-image` (from
  `gemini-3-pro-image-preview`). Both were probed live before migrating.

  One capability difference surfaced and is worth knowing: `gemini-3.6-flash`
  returns `400 invalid_request` for **inline (base64) video bytes** while
  accepting video **by URI**. Image, audio and PDF inline data are
  unaffected. The four inline-video tests are pinned to a model that
  accepts that form (`tests/common::VIDEO_INLINE_MODEL`) so they keep
  testing the bytes path rather than the model's appetite for it.


- CI's antigravity job now receives `GEMINI_API_KEY` (with the same
  same-repo guard the integration matrix uses). Its model-backed tests
  are the only ones that drive a real turn end-to-end, and without a key
  they self-skipped — which is why the turn-completion break above
  reached a release unnoticed.
- `docs/ANTIGRAVITY.md` no longer claims newer harnesses "degrade
  gracefully". Unknown-value preservation stops a crash, but when a
  *renamed* value is one the bridge matches on, preservation is exactly
  what makes the breakage silent.



- All five resource list envelopes (`AgentListResponse`,
  `WebhookListResponse`, and the new trigger/execution/environment ones)
  now degrade a null or malformed list key to an empty page and drop
  undeserializable elements individually with a `tracing::warn!`, instead
  of failing the whole response. For the two pre-existing types this
  changes observable behavior: `list_agents()`/`list_webhooks()` calls
  that previously returned `Err` on a malformed page now return the
  surviving entries.
- `Webhook::create_time`/`update_time` and `SigningSecret::expire_time`
  now deserialize leniently like the trigger and environment timestamps:
  a response whose timestamp encoding diverges from RFC 3339 yields the
  field as `None` (with a `tracing::warn!`) instead of failing the call —
  so an absent timestamp on a returned webhook can mean either "not sent"
  or "sent but unparseable"; the warn log distinguishes them.

### Deprecated

- `response_modalities` is marked deprecated by the official SDK in
  favor of the typed `response_format` union; the builder and field docs
  now steer new code to `with_response_format`. (The SDK deprecates
  `response_mime_type` alongside it, but that field was already removed
  from this crate — the API rejects it in every form.)

### Fixed

- **`on_post_tool` reported successful harness tools as failures.** The
  harness sends `"error": ""` — protobuf's default for an unset string —
  on calls that succeeded, and the bridge surfaced that as `Some("")`, so
  `ToolOutcome::error.is_some()` (the check the field's own docs invite)
  was true for every successful builtin. Blank now normalizes to `None` on
  both dispatch paths. Found by running the new `workspace_explorer`
  example against a live harness; invisible to every existing test.

- **`ThinkingSummaries` sent a value the API now rejects.**
  `to_agent_config_value()` emitted the SCREAMING_CASE
  `THINKING_SUMMARIES_AUTO` / `_NONE` spelling; verified live 2026-08-10
  the API responds `The value 'THINKING_SUMMARIES_AUTO' is not supported
  for 'agent_config.thinking_summaries'. Supported values: 'auto',
  'none'.` — so deep-research requests carrying thinking summaries failed
  outright. Both contexts now emit the lowercase form; deserialization
  still accepts either spelling.

- **Antigravity harness 0.1.10 support (turn completion was broken).** The
  supported harness moves 0.1.5 → 0.1.10. Two wire spellings the bridge
  depends on were renamed in that range, and both failed silently rather
  than loudly:
  - `STATE_IDLE` → `STATE_FULLY_IDLE`. Only that value ends a turn, so
    against a 0.1.6+ harness **every turn ran to its timeout** — no parse
    error, no failed assertion, just a bare
    `Timeout { operation: "agent turn" }` after the full budget.
  - `usageMetadata` → `usageUpdate` (now `{agents[], total}`), which
    silently zeroed token accounting and was additionally misreported as
    an unknown payload variant.

  Both old spellings are accepted as aliases, so a single build drives
  either harness revision; verified by running the full harness suite
  against 0.1.5 and 0.1.10 (10/10 each). Also adds the non-terminal
  `STATE_WAITING_FOR_TASKS` state introduced in 0.1.10.


- Resource IDs are now percent-encoded when interpolated into URL paths
  (interactions get/delete/cancel/stream and the agents, webhooks, triggers
  and environments item URLs). Well-formed IDs are byte-identical on the
  wire; a value containing path metacharacters (`/`, `?`, `#`) is now
  sent as one encoded segment instead of silently rewriting the request
  path, and an empty or dot-segment resource ID (any WHATWG spelling —
  `.`/`..` bare or percent-encoded, which the URL parser would otherwise
  pop at parse time) is rejected locally as `InvalidInput` rather than
  issuing a request against the collection or a different URL. The Files
  API's `get_file`/`delete_file` now validate the full resource name's
  shape positively — the `files/` prefix plus exactly one non-empty
  segment, with the ID percent-encoded like every other resource — so an
  empty name, a `.`/`..` dot segment (bare or percent-encoded), a `?`/`#`
  query or fragment split, or stray extra segments all fail locally as
  `InvalidInput` instead of silently addressing a different URL. The Files API
  `list_files` page token is likewise percent-encoded (agents and
  webhooks already encoded theirs, and the new trigger/environment
  endpoints ride the shared encoder), so a token carrying a reserved
  character is no longer truncated on the wire (a standard-base64 `+`
  previously decoded to a space).
- docs.rs now builds with the `antigravity` feature enabled (the module was
  invisible in the 0.8.0 docs, which were built with default features only),
  with "Available on crate feature ... only" banners on feature-gated items.
  The CI doc gate now builds both `--all-features` and the docs.rs feature
  set under `-D warnings`.
- The `audio_input` and `video_input` examples now embed real media
  fixtures, replacing the zero-length WAV and header-only MP4 that the API
  rejects with 400 `invalid_request` — both examples previously always took
  their error branch.
- Corrected `#[tool]` snippets across the guides: an invalid
  `#[tool(description = ...)]` attribute form the macro parser rejects,
  `.declaration()` called on the function item instead of the generated
  callable struct, and missing `CallableFunction` / `tool` imports.
- README installation snippet now lists `async-trait` and `serde_json`,
  required by `#[tool]`-generated code, and notes the `CallableFunction`
  import — following the previous snippet produced a compile error on
  first macro use.

## [0.8.0] - 2026-07-14

This release migrates the crate to Interactions API wire revision
**2026-05-20** (the steps model), adds the remaining 2026-05-20 API surface
(webhooks, environments + agents, the retrieval tool, typed response
formats, video generation config, multi-speaker TTS, Deep Research
options), introduces a structured wire-inspection layer
(`genai_rs::wire`), and ships a new off-by-default `antigravity` feature —
a native Rust client for Google's Antigravity `localharness` agent
runtime. It is a deliberately breaking release; see the migration guide
below.

> **Verification**: wire shapes were derived from Google's generated
> `google-genai` 2.10 API bindings and covered with fixture + proptest
> roundtrip tests. Both the core revision and the phase-2 surface were
> verified live 2026-07 with a real `GEMINI_API_KEY` (`LOUD_WIRE=1`
> against `generativelanguage.googleapis.com`): revision accepted, steps
> model and snake_case field naming confirmed, `function_call`
> `signature` discovered and modeled, lowercase response modalities and
> the `response_mime_type` rejection confirmed; webhooks full
> CRUD/ping/rotate, environments, typed response formats, multi-speaker
> TTS, deep-research knobs, and the video config schema all exercised
> live. Live findings (details in `docs/INTERACTIONS_API_GAP.md` /
> `docs/ENUM_WIRE_FORMATS.md`): `VideoTask` gained a live-discovered
> `Extend` variant; webhook `:ping` accepts the empty `{}` body and PATCH
> `update_mask` is optional and observed to be ignored (the body scopes
> the update); several knobs are Vertex-only and rejected by the Gemini
> API (`Tool::Retrieval`, `enable_bigquery_tool`, video `gcs_uri`);
> audio/image response formats are inline-only today (audio
> `mime_type`/`delivery` and image `delivery` rejected; image `mime_type`
> limited to `image/jpeg`); Veo models are not served by the Interactions
> API; and agent creation is gated on standard API keys (agent `tools`
> accept only `code_execution`/`google_search`/`url_context`).

### Migration guide (0.7 → 0.8)

**Steps replace `Turn` and `outputs`.** `InteractionResponse.outputs:
Vec<Content>` is now `steps: Vec<Step>`, and `Turn` /
`InteractionInput::Turns` are gone — conversation history is a step list.
Convenience accessors (`as_text()`, `function_calls()`, `images()`, ...)
keep working unchanged.

```rust
// Before
.with_history(vec![Turn::user("hi"), Turn::model("Hello!")])

// After
let mut history = vec![Step::user_text("hi")];
history.extend(response.output_steps()); // replay prior model output
```

**Function-call signatures MUST be replayed.** `function_call` (and
thought) steps carry an opaque `signature` the API rejects stateless
replay without. Never reconstruct model turns by hand — extend history
with `response.output_steps()`, which preserves signatures.

**Response modalities are lowercase.** The API rejects `"IMAGE"`/`"AUDIO"`;
`with_image_output()`/`with_audio_output()` now send `"image"`/`"audio"`
and `with_response_modalities()` normalizes to lowercase. No code change
needed unless you passed uppercase strings you also match on elsewhere.

**`response_mime_type` is gone** (the API rejects the field in every
form). Use `with_response_format()` — passing a JSON schema implies JSON
output:

```rust
// Before
.with_response_mime_type("application/json").with_response_format(schema)
// After
.with_response_format(schema)
```

**`with_response_format()` now takes `impl Into<ResponseFormat>`.** Raw
`serde_json::Value` schemas keep compiling (they convert to
`ResponseFormat::Text { mime_type: "application/json", schema }`); pass
typed `ResponseFormat` variants for audio/image/video output, or
`with_response_formats(Vec<ResponseFormat>)` for the list form.

**`speech_config` is now a list** (`Option<Vec<SpeechConfig>>`,
multi-speaker TTS). `with_speech_config(single)` still compiles and sends
a one-entry list; use `with_speech_configs()` / `add_speech_config()` for
multi-speaker dialogue.

**`tool_choice` is a typed union.** The top-level
`generation_config.allowed_tools: Vec<String>` was removed;
`with_allowed_tools(vec![...])` now emits the spec's
`{"allowed_tools": {"mode", "tools"}}` object, and
`with_tool_choice(ToolChoice::...)` sets the lowercase mode strings
(`auto|any|none|validated`) directly.

**reqwest 0.13 changed TLS trust anchors.** Certificates are now verified
against the **OS trust store** (`rustls-platform-verifier`) instead of
bundled Mozilla roots. Scratch/distroless containers without a CA bundle
will fail TLS — install one (e.g. `ca-certificates`). The TLS stack is
still rustls.

### Added

#### Interactions API revision 2026-05-20

- Every Interactions API request now sends `Api-Revision: 2026-05-20`
  (create/get/delete/cancel, streaming included; the Files API is
  unrevisioned, matching google-genai). Header constants live in
  `src/http/common.rs`.
- New types: `Step`, `StepDelta`, `StepError`, `FunctionResultPayload`,
  `StepSummary`, the `Annotation` citation union + `ReviewSnippet`,
  `ToolChoice` + `AllowedTools`, `ServiceTier`, `GroundingToolCount`,
  `StreamMetadata`.
- New response accessors: `output_contents()`, `output_steps()`,
  `thought_summaries()`, `unknown_steps()`; `Step`/`StepDelta` accessors
  (`as_text()`, `signature()`, `step_type()`, `as_arguments_delta()`,
  ...); `StreamChunk::delta_text()` convenience.
- `InteractionResponse` gained `environment_id`, `output_text`, a typed
  optional `input`, and a `Default` impl. It also now models `object`
  (`"interaction"`), `service_tier` (the effective billing tier), and the
  `webhook_config` echo — all returned by the live API and previously
  dropped silently.
- `InteractionStatus`: first-class `BudgetExceeded` (`"budget_exceeded"`)
  and new `Incomplete` variant for interactions that end before
  completion.
- `service_tier` request field + `with_service_tier()`
  (`flex | standard | priority` + Unknown).
- `cached_content` request field + `with_cached_content()` (explicit
  caching).
- `presence_penalty` / `frequency_penalty` generation config fields +
  `with_presence_penalty()` / `with_frequency_penalty()`.
- `include_input` query param support:
  `Client::get_interaction_with_input()`.
- `UsageMetadata.grounding_tool_count: Vec<GroundingToolCount>` +
  `grounding_count_for_tool()`.
- `AudioInfo::sample_rate()` / `channels()` (and `Content::Audio` gained
  `sample_rate` and `channels`); `GoogleSearchResultItem.search_suggestions`;
  `Place.url` / `review_snippets`.

#### Webhooks

- Full `/v1beta/webhooks` resource client: `Client::create_webhook()`,
  `get_webhook()`, `list_webhooks()`, `update_webhook()` (with
  `update_mask`), `delete_webhook()`, `ping_webhook()`, and
  `rotate_webhook_signing_secret()` (with `RevocationBehavior`).
- New types in `src/webhooks.rs`: `Webhook`, `WebhookUpdate`,
  `SigningSecret`, `WebhookListResponse`, `RotateSigningSecretResponse`,
  and Evergreen enums `WebhookEvent` (`batch.succeeded/expired/failed`,
  `interaction.requires_action/completed/failed`, `video.generated`) and
  `WebhookState` (`enabled`, `disabled`,
  `disabled_due_to_failed_deliveries`).
- Per-request webhook routing: `webhook_config {uris, user_metadata}` on
  `InteractionRequest` + `InteractionBuilder::with_webhook_config()`.
  The API echoes it back on the create response (verified live 2026-07);
  `InteractionResponse.webhook_config` models the echo.
- Webhook/agent endpoints send the same `Api-Revision: 2026-05-20` header
  as interactions (matching google-genai, which applies the revision
  header globally).

#### Environments and Agents resource

- `environment` field on `InteractionRequest` +
  `InteractionBuilder::with_environment()`, accepting a string environment
  ID or a typed `RemoteEnvironment` (`EnvironmentSpec` union). New types in
  `src/environment.rs`: `EnvironmentSource` (sources
  `gcs|inline|repository|skill_registry` via `SourceType`),
  `NetworkConfig` (`"disabled"` | `{allowlist}` union), `AllowlistEntry`
  (domain wildcard + header-injection `transform`).
- `/v1beta/agents` resource client: `Client::create_agent()`,
  `get_agent()`, `list_agents()` (`page_size`/`page_token`/`parent`),
  `delete_agent()`; `Agent` type (`id`, `base_agent`,
  `system_instruction`, `description`, `tools`, `base_environment`) in
  `src/agents.rs`.

#### Retrieval tool

- `Tool::Retrieval` with `RetrievalType`
  (`vertex_ai_search|rag_store|exa_ai_search|parallel_ai_search` +
  Unknown) and per-backend configs: `VertexAiSearchConfig`
  (`engine`/`datastores`), `ExaAiSearchConfig`/`ParallelAiSearchConfig`
  (`api_key`/`custom_config`), `RagStoreConfig` (`rag_resources`,
  deprecated `similarity_top_k`/`vector_distance_threshold`,
  `rag_retrieval_config` with `top_k`/`hybrid_search`/`filter`/`ranking`).
- `RetrievalConfig` builder for `add_tool()` that keeps `retrieval_types`
  in sync with the configured backends. Note: rejected as Vertex-only by
  the Gemini API (verified live 2026-07).

#### Video generation and Deep Research config

- `generation_config.video_config {task}` (`VideoConfig` + `VideoTask`
  enum: `text_to_video|image_to_video|reference_to_video|edit|extend` +
  Unknown; `extend` discovered via the API's live validation error
  2026-07), `with_video_config()`, and the `with_video_output()` modality
  shortcut (`response_modalities: ["video"]`).
- `DeepResearchConfig::with_visualization(Visualization)` (`off|auto` +
  Unknown), `with_collaborative_planning(bool)`, and
  `with_bigquery_tool(bool)` (Vertex-only); managed agent IDs (incl.
  `deep-research-preview-04-2026`, `deep-research-max-preview-04-2026`,
  `antigravity-preview-05-2026`) documented in
  `docs/AGENTS_AND_BACKGROUND.md`.

#### Built-in tool configuration

- Google Maps built-in tool support: `Tool::GoogleMaps` (with
  `latitude`/`longitude`), `google_maps_call`/`google_maps_result` steps,
  `GoogleMapsResultInfo` view type, response accessors
  (`has_google_maps_results()`, `google_maps_results()`), `Place` struct
  (with Evergreen forward-compatible `extra` field), and the
  `with_google_maps()` shorthand.
- `SearchType` enum for Google Search `search_types` configuration, incl.
  `SearchType::EnterpriseWebSearch`.
- `Tool::McpServer` gained optional `allowed_tools` (`[{mode, tools}]`
  per the spec) and `headers` fields.
- `Tool::ComputerUse` gained `enable_prompt_injection_detection` and
  `disabled_safety_policies`; documented `mobile`/`desktop` environments;
  `ComputerUseConfig::with_environment()` /
  `with_prompt_injection_detection()` / `disabling_safety_policies()`.
- Unified `add_tool(impl Into<Tool>)` on `InteractionBuilder` with tool
  configuration structs: `GoogleMapsConfig`, `GoogleSearchConfig`,
  `McpServerConfig`, `ComputerUseConfig`, `FileSearchConfig`.

#### Wire inspection (`genai_rs::wire`)

- Public `genai_rs::wire` module for structured wire-level inspection:
  - `WireEvent` (requests, response status/bodies, error bodies, SSE
    frames, file uploads) with per-client request-id correlation.
  - `WireInspector` trait and `ClientBuilder::add_wire_inspector()`
    (multiple inspectors supported).
  - `LoudWirePrinter` built-in inspector — the colored stderr printer
    behind `LOUD_WIRE=1` (unchanged UX: the env var, now read at `Client`
    construction, installs it automatically).
  - `TracingForwarder` built-in inspector — forwards wire events to
    `tracing` at `DEBUG` under the new `genai_rs::wire` target
    (`RUST_LOG=genai_rs::wire=debug`).
- SSE `event:` lines are now surfaced to wire inspectors as
  `WireEvent::SseFrame { event_type, .. }`.

#### Antigravity (new `antigravity` feature, off by default)

- Native `genai_rs::antigravity` client for Google's Antigravity
  `localharness` agent runtime (see `docs/ANTIGRAVITY.md`):
  - `AntigravityAgent::builder()` — spawn the harness binary (discovery
    via `ANTIGRAVITY_HARNESS_PATH`, python3 site-packages, or `PATH`),
    stdio handshake, localhost WebSocket, conversation init; pinned to
    `google-antigravity` 0.1.5 (`SUPPORTED_HARNESS_VERSION`).
  - `agent.chat()` and `agent.send_streaming()` (`AgentEvent`:
    text/thinking deltas, structured `ToolAction`s, custom tool
    dispatches, `Finished`, Evergreen `Unknown`), `CancelHandle`, turn
    timeouts, graceful `shutdown()` with SIGTERM/SIGKILL escalation.
  - Custom tools reuse the existing `#[tool]`/`FunctionRegistry`/
    `ToolService` machinery; harness built-ins gated via `Capabilities`
    (read-only by default).
  - Tool policies (`policy::allow/deny/confirm/allow_all/deny_all`)
    evaluated Rust-side before every dispatch, plus
    `on_pre_tool`/`on_post_tool` hooks; spawn-time safety gate refuses
    write-capable tools or MCP servers without a policy or hook (parity
    with the Python SDK).
  - MCP server config (`McpServer::stdio`/`McpServer::http`), structured
    output via `with_response_schema`, session persistence/resume via
    `with_save_dir` + `with_conversation_id`.
  - Proto-JSON protocol types under `antigravity::protocol` with
    Evergreen unknown-variant preservation throughout.
  - New `WireEvent` variants `HarnessSpawn`, `WsSend`, `WsReceive`,
    `HarnessStderr` (LOUD_WIRE and wire inspectors cover harness
    sessions).
  - Client-side triggers: `AgentBuilder::add_trigger(TriggerConfig)`
    spawns a per-trigger timer task that injects an `automated_trigger`
    message every interval, delivered only while the agent is idle
    (firings that come due mid-turn are deferred until the turn ends;
    missed intervals collapse into a single delivery). Tasks stop cleanly
    on `shutdown()`/drop; zero intervals are rejected at `spawn()`.
  - Subagent registration: `AgentBuilder::add_subagent(Subagent)` sends
    static `custom_subagents` in the conversation init (name, description,
    appended-style system instructions, per-subagent `Capabilities`,
    custom tools referenced by name). `spawn()` validates that referenced
    custom tools are registered on the parent agent and that subagent
    names are unique; nested subagents are force-disabled (harness
    limitation, reference-SDK parity).

- **Antigravity: workspace announcement.** `add_workspace(..)` roots are now
  announced to the model automatically — `spawn()` appends a concise,
  delimited note listing the configured workspace root(s) to the effective
  system instructions (the string passed to `with_system_instructions` is
  never mutated), and appends the same note to every subagent's instructions.
  Agents no longer guess workspace paths. Opt out with
  `AgentBuilder::with_workspace_announcement(false)`. The wire protocol has no
  native announcement field, so this is prompt-level grounding.
- **Antigravity: `ToolAction::subagent_name()`** and a typed
  `ActionInvokeSubagent::name` field (Evergreen; `None` on harness 0.1.5,
  which emits an empty `invokeSubagent` action).
- **Antigravity: `ToolDecision`** (`Allowed` / `Denied { reason }`) and
  **`ErrorSeverity`** (`Transient` / `Severe`) public enums.

#### New examples

- `examples/webhooks_and_background.rs` — webhook resource lifecycle +
  per-request webhook routing (runs without an API key, printing request
  shapes).
- `examples/retrieval_grounding.rs` — retrieval tool over Vertex AI
  Search, RAG store, and Exa.ai backends.
- `examples/antigravity_agent.rs` — Antigravity harness walkthrough
  (requires `--features antigravity`).
- `examples/real_world/repo_auditor/` — end-to-end Antigravity bridge
  application: repo workspace, read-only built-ins, custom `#[tool]`
  severity classifier, subagents, structured report output.

### Changed

#### Breaking — response model (`outputs` → `steps`)

- `InteractionResponse.outputs: Vec<Content>` replaced by
  `steps: Vec<Step>`. `Step` is a new `#[non_exhaustive]` tagged union
  (`user_input`, `model_output`, `thought`, `function_call`,
  `function_result`, `code_execution_call/result`,
  `url_context_call/result`, `google_search_call/result`,
  `mcp_server_tool_call/result`, `file_search_call/result`,
  `google_maps_call/result`, plus the standard `Unknown { step_type,
  data }` variant). Tool call/result and thought steps carry opaque
  `signature` fields that must be replayed unchanged.
- Convenience accessors (`as_text()`, `all_text()`, `function_calls()`,
  `images()`, `audios()`, `code_execution_*()`, `google_search_*()`,
  `url_context_*()`, `file_search_results()`, `google_maps_results()`,
  `thought_signatures()`, annotations helpers) are reimplemented over
  steps and keep working.
- `Content` slimmed to the spec content union: `Text`, `Image`, `Audio`,
  `Video`, `Document`, `Unknown`. All tool/thought `Content` variants were
  removed (they are `Step`s now).
- `Annotation` is now a typed citation union: `UrlCitation`,
  `FileCitation`, `PlaceCitation` (with `ReviewSnippet`s), plus `Unknown`.
  Byte-offset `extract_span()` is preserved.
- `InteractionResponse` serializes uniformly in snake_case (the previous
  `camelCase` rename was wrong for this API).
- `content_summary()`/`ContentSummary` replaced by
  `step_summary()`/`StepSummary`.
- `FunctionCallInfo.id` is now `&str` (required by the spec);
  `FunctionResultInfo.result` is a typed `FunctionResultPayload`
  (string | JSON | content blocks union).

#### Breaking — SSE lifecycle

- Wire events migrated from `interaction.start` /
  `content.start|delta|stop` / `interaction.complete` to
  `interaction.created`, `interaction.status_update`, `step.start`,
  `step.delta`, `step.stop`, `interaction.completed`, and `error`.
- `StreamChunk` variants renamed/reshaped: `Created { interaction }`,
  `StepStart { index, step }`, `StepDelta { index, delta: StepDelta }`,
  `StepStop { index, usage, step_usage }` (per-step usage), and
  `Completed(InteractionResponse)`. Stream termination keys on
  `interaction.completed` / `error`.
- New `StepDelta` payload union: `text`, `image`, `audio` (with
  `rate`/`sample_rate`/`channels`), `video`, `document`,
  `thought_summary`, `thought_signature`, `text_annotation_delta`,
  `arguments_delta` (function-call arguments now stream incrementally),
  built-in tool call/result deltas, `function_result`, and `Unknown`.
- The HTTP layer accumulates `step.start`/`step.delta`/`step.stop` into
  the final `Completed` response (including parsing streamed
  `arguments_delta` fragments into `FunctionCall.arguments`), so
  `response.function_calls()` and `as_text()` work after streaming.
  Lifecycle `metadata.total_usage` is folded into the completed response
  when the payload omits usage.
- `AutoFunctionStreamChunk::Delta` now carries `StepDelta` (exposing
  `arguments_delta` in the auto-function stream); the auto-function
  streaming loop and `last_event_id` resume work over the new events.

#### Breaking — input model & requests

- `Turn` and `InteractionInput::Turns` removed (deprecated in the spec).
  Conversation history is represented as steps:
  `InteractionInput::Steps(Vec<Step>)`, `with_history(Vec<Step>)`,
  `Step::user_text()` / `Step::model_text()` /
  `InteractionResponse::output_steps()`. `ConversationBuilder` keeps its
  fluent `.user()`/`.model()`/`.turn()` API but produces steps.
- `system_instruction` is a plain `Option<String>` per the spec.
- `generation_config.tool_choice` is a typed union `ToolChoice`: a
  lowercase mode string (`auto|any|none|validated`) or
  `{"allowed_tools": {"mode", "tools"}}`. The crate's previous top-level
  `generation_config.allowed_tools: Vec<String>` was removed;
  `with_allowed_tools()` now produces the object form. New
  `with_tool_choice()` escape hatch.
- `interactions_api` helper constructors renamed `*_content` → `*_step`
  and return `Step`.

#### Breaking — typed `response_format`

- `InteractionRequest.response_format` is now a typed
  `Option<ResponseFormatSpec>` (single object or list) instead of raw
  `serde_json::Value`. `ResponseFormat` is a tagged union:
  `Text{mime_type, schema}`, `Audio{mime_type, delivery, sample_rate,
  bit_rate}`, `Image{mime_type, delivery, aspect_ratio, image_size}`,
  `Video{delivery, gcs_uri, aspect_ratio, duration}`, plus Unknown;
  `ResponseDelivery` is `inline|uri` + Unknown.
- `with_response_format()` now takes `impl Into<ResponseFormat>` — raw
  `serde_json::Value` schemas keep compiling and convert to
  `ResponseFormat::Text{mime_type: "application/json", schema}` (the
  typed equivalent of the old wire shape). New
  `with_response_formats(Vec<ResponseFormat>)` for the list form.
- Raw schema dicts received on the wire (no recognized `type` tag)
  roundtrip losslessly through `ResponseFormat::Unknown`.
- `ImageConfig` with `ImageAspectRatio` and `ImageSize` enums for image
  generation configuration.

#### Breaking — multi-speaker TTS (`speech_config` list)

- `GenerationConfig.speech_config` is now `Option<Vec<SpeechConfig>>` to
  match the spec's list wire format (multi-speaker TTS). The legacy
  single-object wire form is still accepted on deserialize.
- Builder: `with_speech_config(single)` keeps working (sends a one-entry
  list); new `with_speech_configs(Vec<SpeechConfig>)` and
  `add_speech_config()` for multi-speaker dialogue.

#### Breaking — dependencies

- `reqwest` upgraded 0.12 → 0.13 (`GenaiError::Http(reqwest::Error)` and
  `ResumableUpload` methods expose reqwest types publicly). MSRV is
  unchanged (Rust 1.88).
  - TLS **trust roots** changed with the upgrade: reqwest 0.13 removed
    the bundled-Mozilla-roots feature (`rustls-tls-webpki-roots`); its
    `rustls` feature now verifies certificates against the **OS trust
    store** via `rustls-platform-verifier`. The TLS stack is still rustls
    (not native TLS). If your deployment relies on bundled roots (e.g.
    minimal containers without a CA bundle), install a CA bundle or open
    an issue — restoring bundled roots would require a preconfigured
    rustls client.
- Minor dependency bumps: tokio 1.48 → 1.52, proptest 1.9 → 1.11,
  utoipa 5.4 → 5.5.

#### Antigravity

- **Antigravity (breaking): `AgentEvent::ToolAction`** is now a struct variant
  `ToolAction { action, decision, trajectory_id }`. `decision` distinguishes
  executed actions from policy/hook-denied ones (previously indistinguishable
  in the stream), and `trajectory_id` tells parent and subagent actions apart.
- **Antigravity (breaking): `AgentEvent::Error(String)`** is now
  `Error { message, severity }` so consumers can ignore transient
  harness-internal noise (retried internally; the turn continues) and react
  only to `Severe` errors. Turn-ending failures still surface as
  `AntigravityError::Turn`.
- **Antigravity (behavior): `ToolOutcome.result`** passed to post-tool hooks
  is now the inner tool result, not the raw `{"result": ...}` wire envelope
  (a scalar arrives as its string form; an object is passed through
  serialized).

#### Other changes

- `Tool::GoogleSearch` is now a struct variant with optional
  `search_types` field (was unit variant).
- New default-on feature `wire-color` gates the `colored`/`colored_json`
  dependencies; build with `default-features = false` for plain-text wire
  output.
- Wire debug request ids are now per-`Client` (previously a
  process-global counter).
- Proptest roundtrip comparisons use `serde_json::Value` for HashMap key
  order independence.

### Fixed

#### Spec/implementation disagreements (verified live)

- **BREAKING**: `Step::FunctionCall` and `Step::FunctionResult` gained a
  `signature: Option<String>` field (verified live 2026-07: the API
  returns `signature` on `function_call` steps and rejects stateless
  replay of history that omits it; the generated SDK bindings do not list
  it on `function_call`). Existing constructors set it to `None`; it is
  preserved on deserialize/serialize roundtrip.
- Response modalities are now sent lowercase: `with_image_output()` /
  `with_audio_output()` send `"image"` / `"audio"` (previously `"IMAGE"` /
  `"AUDIO"`, which the API rejects — supported values are `text`,
  `image`, `audio`, `video`, `document`; verified live 2026-07).
  `with_response_modalities()` normalizes provided values to lowercase.
- `FunctionCallingMode` now serializes lowercase (`"auto"`, ...); legacy
  UPPERCASE still accepted on deserialize.
- `CodeExecutionLanguage` now serializes `"python"` (was `"PYTHON"`);
  legacy accepted on deserialize.
- `Tool::ComputerUse` `excluded_predefined_functions` now serializes
  snake_case (was `excludedPredefinedFunctions`); legacy alias accepted
  on deserialize.
- MCP server `allowed_tools` is now `[{mode, tools}]`
  (`Vec<AllowedTools>`) per the spec (was `[String]`).
- `InteractionResponse` no longer serializes camelCase field names.
- Live `google_search_result` items can carry only `search_suggestions`;
  the non-optional `title`/`url` fields previously synthesized empty
  strings on stateless replay and are now skip-serialized when empty, so
  captured live responses roundtrip byte-identically.
- `NetworkConfig::Allowlist` now preserves sibling keys next to
  `allowlist` on roundtrip (Evergreen) via a struct variant with an
  `extra` map — construct with the new `NetworkConfig::allowlist(entries)`
  helper. Previously an unmodeled sibling field (e.g. a future
  `default_policy`) was silently dropped on deserialize.

#### Streaming & wire

- Streaming image, video, and document deltas now accumulate into a
  single content block (matching audio behavior) instead of producing one
  block per chunk.
- Streaming responses whose terminal event omits usage now fall back to
  the cumulative usage from the last `step.stop` event, so
  `response.usage()` stays populated.
- Error response bodies are now visible in wire output
  (`WireEvent::ErrorBody`); previously `LOUD_WIRE=1` showed only the
  status line for failed requests.
- Wire output no longer panics when truncating multi-byte UTF-8 content
  (`data`/`signature` fields and non-JSON bodies truncate on character
  boundaries).
- Request bodies are no longer serialized for wire debugging when it is
  disabled (previously every request paid the serialization cost even
  without `LOUD_WIRE`).

#### Evergreen roundtrip

- `RemoteEnvironment`, `EnvironmentSource`, and `AllowlistEntry` now
  preserve unknown wire fields via an Evergreen `extra` field
  (roundtrip-safe; breaking for exhaustive struct literals — use
  `..Default::default()`).

#### Antigravity

- The stdio handshake now rejects reply frames whose declared length
  exceeds 4 MiB before allocating (a non-harness binary can no longer
  trigger an unbounded allocation; it fails with `HandshakeFailed`
  instead).
- A turn that exceeds `with_turn_timeout` is now halted on the harness
  and its remaining events drained before `AntigravityError::Timeout` is
  returned; previously the abandoned turn kept running and its buffered
  events (including its terminal state) desynced the next
  `chat`/`send_streaming`, which could return the previous turn's output.
- Turns started by `add_trigger` deliveries are now halted and their
  events discarded by the next `chat`/`send_streaming` before it sends
  its input; previously the unconsumed trigger turn's buffered events
  were misattributed to the user's turn, shifting every later response by
  one turn. Trigger-turn output is not surfaced (documented in
  `docs/ANTIGRAVITY.md`).
- A cancelled *subagent* trajectory no longer fails the parent's whole
  turn (mirroring the existing subagent-idle handling); subagent failures
  surface through their step errors. Main-trajectory cancellation still
  fails the turn with `AntigravityError::Turn`.
- The harness stderr drain no longer stops permanently on a non-UTF-8
  line (it now reads bytes and replaces invalid sequences lossily); a
  stopped drain could let the stderr pipe fill and deadlock the child —
  the drain's whole purpose at the wrong-binary trust boundary.
- Closed a race between trigger delivery and turn begin: a trigger that
  had passed its idle check could deliver its message *after*
  `chat`/`send_streaming` marked the agent busy and consumed the
  trigger-fired flag, injecting an `automated_trigger` into the user's
  turn window with nobody left to drain the resulting harness turn. The
  fire decision and turn begin are now mutually exclusive (shared lock +
  idle re-check under it).
- Unrecognized harness tool confirmations now **fail closed** — a
  confirmation whose action fields this client does not recognize (e.g. a
  builtin newer than the pinned harness) is approved only when a policy
  rule (`allow_all()` or an exact rule naming the unknown wire field) or
  the `on_pre_tool` hook allows it, with a `warn!` either way. Previously
  any unmappable confirmation was auto-approved, bypassing deny policies.
  Genuine pre-request notifications (steps with no action payload) remain
  auto-approved — the concrete call still gets its own policy check.

### Removed

- **BREAKING**: `response_mime_type` removed outright — the
  `InteractionRequest` field and
  `InteractionBuilder::with_response_mime_type()` (previously
  `#[deprecated]`). Live verification (2026-07) showed the API rejects
  every request carrying the field: alone it returns 400 "responseFormat
  must be set when responseMimeType is set", the same 400 is returned
  even when `response_format` IS set (raw-schema or typed form), and
  camelCase `responseMimeType` gets "Unknown parameter" — the field is
  dead server-side, so no working code can be using it. Use
  `with_response_format()` alone; passing a JSON schema implies JSON
  output.
- **BREAKING**: `Turn` and `InteractionInput::Turns` (deprecated in the
  spec) — history is steps now; see the migration guide.
- **BREAKING**: `ComputerUseCall`/`ComputerUseResult` content dropped
  entirely (computer use surfaces as `function_call` steps).
- **BREAKING**: `generation_config.top_k` removed (dropped from the
  spec).
- **BREAKING**: `UsageMetadata.total_reasoning_tokens` removed (not in
  the spec; use `total_thought_tokens`) and
  `InteractionResponse::reasoning_tokens()` removed in favor of
  `thought_tokens()`.
- Speculative `grounding_metadata`/`url_context_metadata` response fields
  (and `GroundingMetadata`, `GroundingChunk`, `WebSource`,
  `UrlContextMetadata`, `UrlMetadataEntry`, `UrlRetrievalStatus` types) —
  not part of the Interactions API; grounding data lives in steps and
  typed annotations.
- `with_computer_use()` and `with_computer_use_excluding()` — use
  `add_tool(ComputerUseConfig::new())`.
- `add_mcp_server()` — use `add_tool(McpServerConfig::new(name, url))`.
- `with_file_search()` and `with_file_search_config()` — use
  `add_tool(FileSearchConfig::new(stores))`.

### Security

- Wire-inspection redaction now covers webhook signing secrets
  (`new_signing_secret` on create, `secret` on rotate — one-time values),
  and `TracingForwarder` applies the same redaction/truncation as
  `LoudWirePrinter` to request/response/error bodies and SSE frames
  (previously it forwarded them raw to `tracing`, bypassing redaction
  entirely — including the Exa/Parallel retrieval `api_key` fields).
- The Antigravity WS-send inspection copy now redacts every value inside
  `env`, `headers`, and `httpHeaders` maps (MCP stdio env secrets,
  `Authorization` bearer tokens, model-endpoint headers), not just
  `apiKey`. The harness/API still receives the originals; only inspector
  copies are redacted.
- `LOUD_WIRE` output now fully redacts `api_key` fields (e.g.
  Exa/Parallel retrieval configs) instead of printing them.
- Wire-inspection redaction now recurses into `data`/`signature` keys
  whose values are objects or arrays (e.g. Evergreen `Unknown` payloads
  preserved under a `data` key), so secrets nested inside them are
  redacted; previously such subtrees were skipped entirely by both
  `LoudWirePrinter` and `TracingForwarder`.
- `Debug` for `Webhook` and `RotateSigningSecretResponse` now redacts
  `new_signing_secret` / `secret` (matching the client's `api_key`
  redaction precedent).

## [0.7.2] - 2026-01-17

### Changed

- **BREAKING**: `AutoFunctionStreamChunk::ExecutingFunctions` changed from tuple variant to struct variant with `pending_calls` field:
  ```rust
  // Before
  AutoFunctionStreamChunk::ExecutingFunctions(response) => {
      // response.function_calls() was often empty in streaming mode
  }

  // After
  AutoFunctionStreamChunk::ExecutingFunctions { response, pending_calls } => {
      // pending_calls always contains the validated function calls
      for call in pending_calls {
          println!("Executing: {}({})", call.name, call.args);
      }
  }
  ```

### Added

- `PendingFunctionCall` type: Represents a function call about to be executed, with `name`, `call_id`, and `args` fields. Available in `ExecutingFunctions` events before function execution begins.

### Fixed

- `ExecutingFunctions` chunk now provides function call information via `pending_calls` field. Previously, `response.function_calls()` was often empty in streaming mode because function calls arrived via Delta chunks rather than the Complete response.

## [0.7.1] - 2026-01-17

### Changed

- **BREAKING**: Removed typestate pattern from `InteractionBuilder`. The builder no longer uses `FirstTurn`, `Chained`, or `StoreDisabled` marker types. Invalid combinations (e.g., `store=false` with `with_previous_interaction()`) are now caught at runtime in `build()` with descriptive error messages instead of compile-time type errors. This enables conditional chaining patterns that were previously impossible:
  ```rust
  // Now possible - conditional chaining
  let mut builder = client.interaction()
      .with_model("gemini-3.6-flash")
      .with_text("Hello");

  if let Some(prev_id) = previous_interaction_id {
      builder = builder.with_previous_interaction(prev_id);
  }

  let response = builder.create().await?;
  ```
- **BREAKING**: `FunctionExecutionResult::new()` now requires `args` parameter (position 3, before `result`)
- `FunctionExecutionResult` now includes `args` field for complete execution context - enables logging function calls with their arguments after execution completes

### Fixed

- Auto-function execution (`create_with_auto_functions()` and `create_stream_with_auto_functions()`) now reports accurate accumulated token usage across all API calls. Previously, the final response could show 0 input tokens because the API only reports input tokens on the first call.

### Migration

If you relied on compile-time enforcement of builder constraints, you'll now get runtime errors from `build()` instead:
- `with_store_disabled()` + `with_previous_interaction()` → `GenaiError::InvalidInput("Chained interactions require storage...")`
- `with_store_disabled()` + `with_background(true)` → `GenaiError::InvalidInput("Background execution requires storage...")`
- `with_store_disabled()` + `create_with_auto_functions()` → `GenaiError::InvalidInput("create_with_auto_functions() requires storage...")`

## [0.7.0] - 2026-01-15

### Added

- New `docs/BUILDER_API.md` documenting the InteractionBuilder API, method naming conventions, and validation errors
- `build()` now validates that `with_agent_config()` requires `with_agent()` - returns error instead of silently ignoring
- **New Content API**: Static constructors on `Content` for all content types:
  - `Content::text()`, `Content::image_data()`, `Content::image_uri()`, `Content::audio_data()`, `Content::audio_uri()`, `Content::video_data()`, `Content::video_uri()`, `Content::document_data()`, `Content::document_uri()`
  - `Content::from_file(&FileMetadata)` - create content from Files API upload
  - `Content::from_uri_and_mime(uri, mime)` - generic URI content
  - Resolution variants: `Content::image_data_with_resolution()`, etc.
- **Content builder methods**:
  - `Content::with_resolution(Resolution)` - chain resolution setting
  - `Content::with_result(value)` - convert `FunctionCall` to `FunctionResult`
  - `Content::with_result_error(value)` - convert `FunctionCall` to error `FunctionResult`

### Changed

- **BREAKING**: Renamed `InteractionContent` → `Content` for ergonomics. Update imports: `use genai_rs::Content;`
- **BREAKING**: Renamed `Content::text()` getter → `Content::as_text()` to follow Rust getter conventions
- **BREAKING**: Renamed `InteractionResponse::text()` getter → `InteractionResponse::as_text()` for consistency
- **BREAKING**: Renamed `TurnContent::parts()` → `TurnContent::as_parts()` for consistency with `as_text()`
- **BREAKING**: Renamed `with_turns()` to `with_history()`. The new name better reflects that this sets conversation history, and now composes correctly with `with_text()`: calling both produces `[...history, Turn::user(current_message)]` regardless of call order.
- **BREAKING**: `with_text()` now sets `current_message` instead of replacing `input`. This fixes issue #359 where `with_turns().with_text()` silently overwrote the history.
- **BREAKING**: `with_system_instruction()` is now available on ALL builder states (FirstTurn, Chained, StoreDisabled), not just FirstTurn. The API does NOT inherit system instructions via `previousInteractionId`, so users should set it explicitly on each turn if needed. For `create_with_auto_functions()`, the SDK automatically includes system_instruction on all internal turns.
- Method naming consistency overhaul:
  - `with_function()` → `add_function()` (accumulates)
  - `with_functions()` → `add_functions()` (accumulates)
- `build()` now returns an error if content input is combined with history (incompatible modes), with a helpful error message explaining the workaround

### Removed

- **BREAKING**: Removed all `add_*` multimodal methods from `InteractionBuilder`:
  - `add_image_data()`, `add_image_uri()`, `add_image_file()`, `add_image_bytes()`
  - `add_audio_data()`, `add_audio_uri()`, `add_audio_file()`, `add_audio_bytes()`
  - `add_video_data()`, `add_video_uri()`, `add_video_file()`, `add_video_bytes()`
  - `add_document_data()`, `add_document_uri()`, `add_document_file()`, `add_document_bytes()`
  - `add_file()`, `add_file_uri()` (Files API methods)
  - All `*_with_resolution()` variants

  **Migration**: Use `with_content(vec![Content::*(...)])` instead. See migration guide below.

- **BREAKING**: Removed all `*_content()` free functions from `interactions_api`:
  - `text_content()`, `image_data_content()`, `image_uri_content()`, `audio_data_content()`, `audio_uri_content()`
  - `video_data_content()`, `video_uri_content()`, `document_data_content()`, `document_uri_content()`
  - `function_call_content()`, `function_result_content()`, `file_data_content()`, `file_uri_content()`

  **Migration**: Use `Content::*()` static constructors instead (e.g., `text_content("hi")` → `Content::text("hi")`).

  **Note**: Model output constructors for testing remain in `interactions_api`: `code_execution_*`, `google_search_*`, `url_context_*`, `file_search_*`.

### Fixed

- `AgentConfig` (DeepResearchConfig) now serializes `thinking_summaries` with snake_case per API spec, not camelCase `thinkingSummaries`
- **BREAKING**: `document_from_file()` now correctly rejects non-PDF files. The Gemini API only supports `application/pdf` for document content type. For text-based files (CSV, TXT, JSON, etc.), read the file and send as `Content::text()` instead.
- `FileSearchResult` now serializes `call_id` with snake_case per API spec, not camelCase `callId`
- `CodeExecutionCall` now serializes with nested `arguments` object containing `language` and `code` per API spec
- `GoogleSearchResultItem.rendered_content` now uses snake_case per API spec

### Changed

- **BREAKING**: Removed `CodeExecutionOutcome` enum - actual wire format uses `is_error: bool` and `result: String` fields directly, not `outcome`/`output` as documented
- `CodeExecutionResultInfo` now has `is_error: bool` and `result: &str` fields instead of `outcome: CodeExecutionOutcome` and `output: &str`
- `Content::CodeExecutionResult` variant now uses `is_error: bool, result: String` instead of `outcome: CodeExecutionOutcome, output: String`
- `InteractionResponse::successful_code_output()` now checks `!is_error` instead of `outcome.is_success()`
- **BREAKING**: `FunctionCallInfo` and `OwnedFunctionCallInfo` no longer have `thought_signature` field - API never sends this on function calls
- Renamed `Content::new_function_call_with_signature()` to `Content::function_call_with_id()` and removed `thought_signature` parameter
- Renamed `function_call_content_with_signature()` to `function_call_content_with_id()` and removed `thought_signature` parameter

### Removed

- **BREAKING**: `CodeExecutionOutcome` enum - the actual API wire format doesn't use this enum
- **BREAKING**: `thought_signature` field from `Content::FunctionCall` variant - API does not send this field on function calls (thought signatures appear only on `Thought` content blocks)

### Migration Guide

**Type rename - `InteractionContent` → `Content`:**
```rust
// Before (0.6.0)
use genai_rs::InteractionContent;
let content = InteractionContent::new_text("Hello");

// After (0.7.0)
use genai_rs::Content;
let content = Content::text("Hello");  // Static constructor
```

**Multimodal content - `add_*()` methods removed:**
```rust
// Before (0.6.0)
let response = client.interaction()
    .with_model("gemini-3.6-flash")
    .with_text("Describe this image")
    .add_image_file("photo.jpg").await?
    .create()
    .await?;

// After (0.7.0) - Option A: Content constructors
let response = client.interaction()
    .with_model("gemini-3.6-flash")
    .with_content(vec![
        Content::text("Describe this image"),
        Content::image_data(base64_data, "image/png"),
    ])
    .create()
    .await?;

// After (0.7.0) - Option B: File helpers
use genai_rs::image_from_file;
let image = image_from_file("photo.jpg").await?;
let response = client.interaction()
    .with_model("gemini-3.6-flash")
    .with_content(vec![
        Content::text("Describe this image"),
        image,
    ])
    .create()
    .await?;
```

**Files API - `add_file()` removed:**
```rust
// Before (0.6.0)
let file = client.upload_file("video.mp4").await?;
let response = client.interaction()
    .with_model("gemini-3.6-flash")
    .add_file(&file)
    .with_text("Describe this video")
    .create()
    .await?;

// After (0.7.0)
let file = client.upload_file("video.mp4").await?;
let response = client.interaction()
    .with_model("gemini-3.6-flash")
    .with_content(vec![
        Content::text("Describe this video"),
        Content::from_file(&file),
    ])
    .create()
    .await?;
```

**Resolution control:**
```rust
// Before (0.6.0)
.add_image_data_with_resolution(base64, "image/png", Resolution::High)

// After (0.7.0) - Constructor
Content::image_data_with_resolution(base64, "image/png", Resolution::High)

// After (0.7.0) - Builder chain
Content::image_data(base64, "image/png").with_resolution(Resolution::High)
```

**`with_turns()` renamed to `with_history()` and composes with `with_text()`:**
```rust
// Before (0.6.0)
// with_turns().with_text() silently overwrote history - bug!
let response = client.interaction()
    .with_model("gemini-3.6-flash")
    .with_turns(history)
    .create()
    .await?;

// After (0.7.0)
// Renamed to with_history(), and now composes correctly with with_text()
let response = client.interaction()
    .with_model("gemini-3.6-flash")
    .with_history(history)
    .with_text("Current message")  // Appended as final user turn
    .create()
    .await?;
// Produces: [...history, Turn::user("Current message")]
// Order doesn't matter - with_text().with_history() produces same result
```

**`CodeExecutionOutcome` removal:**
```rust
// Before
if result.outcome.is_success() {
    println!("Output: {}", result.output);
}

// After
if !result.is_error {
    println!("Output: {}", result.result);
}
```

**`thought_signature` removal from FunctionCall:**
```rust
// Before
let call = InteractionContent::new_function_call_with_signature(
    Some("call_123"),
    "get_weather",
    json!({"location": "SF"}),
    Some("signature".to_string())  // No longer needed - API doesn't send this
);
if let InteractionContent::FunctionCall { thought_signature, .. } = content {
    // thought_signature was always None
}

// After
let call = InteractionContent::new_function_call_with_id(
    Some("call_123"),
    "get_weather",
    json!({"location": "SF"})
);
// Note: Thought signatures appear on Thought content blocks, not function calls.
// Use response.thought_signatures() to iterate over them.
```

## [0.6.0] - 2025-01-11

### Added

- `InteractionBuilder::build()`: Build requests without executing, enabling retry patterns and request serialization
- `Client::execute()` and `Client::execute_stream()`: Execute pre-built `InteractionRequest` objects
- `GenaiError::is_retryable()`: Helper to identify transient errors (429, 5xx, timeouts) for retry logic
- `GenaiError::Api::retry_after`: Extracts `Retry-After` header from 429 rate limit responses (seconds or HTTP date format)
- `GenaiError::retry_after()`: Accessor method for the retry delay (consistent with `is_retryable()` pattern)
- `Deserialize` derive on `InteractionRequest`: Enables loading requests from JSON/config files
- `#[tracing::instrument]` on `execute()` and `execute_stream()`: Automatic span creation with model/agent context
- `docs/RETRY_PATTERNS.md`: Documents retry philosophy and recommended patterns using `backon` crate
- `examples/retry_with_backoff.rs`: Demonstrates retry patterns using the `backon` crate

### Changed

- **BREAKING**: Renamed `CreateInteractionRequest` to `InteractionRequest` for consistency
- **BREAKING**: Migrated from `log` crate to `tracing` crate for structured logging and spans
- Updated `docs/LOGGING_STRATEGY.md` to document tracing integration and instrumentation patterns

### Removed

- **BREAKING**: `Client::create_interaction()` - use `Client::execute()` instead
- **BREAKING**: `Client::create_interaction_stream()` - use `Client::execute_stream()` instead

### Migration Guide

**`create_interaction()` → `execute()`:**
```rust
// Before
let response = client.create_interaction(request).await?;
let stream = client.create_interaction_stream(request);

// After
let response = client.execute(request).await?;
let stream = client.execute_stream(request);
```

**`CreateInteractionRequest` → `InteractionRequest`:**
```rust
// Before
use genai_rs::CreateInteractionRequest;

// After
use genai_rs::InteractionRequest;
```

**`log` → `tracing`:**
If you were filtering logs with `RUST_LOG=genai_rs=debug`, this continues to work.
For tracing subscribers, use `tracing_subscriber` instead of `env_logger`:

```rust
// Before (with env_logger)
env_logger::init();

// After (with tracing-subscriber)
tracing_subscriber::fmt::init();
```

## [0.5.3] - 2026-01-10

### Fixed

- Release workflow: Use `--tests` to exclude doctests from `--include-ignored` run (v0.5.2 release workflow failed because `--include-ignored` compiles `ignore` doctest snippets)

## [0.5.2] - 2026-01-10

### Fixed

- Doctest compilation failures in `INTERACTIONS_API_FEEDBACK.md` (missing `ignore` annotation)
- Updated `thought_content()` docstring to clarify it's for testing only (API rejects thought blocks in user input)

### Changed

- `INTERACTIONS_API_FEEDBACK.md`: Downgraded thought signature issue from P0 to P2, clarified that signatures ARE present on `Thought` outputs (not `FunctionCall`), and documented that API rejects thought blocks in user input

## [0.5.1] - 2026-01-10

### Fixed

- Streaming tests no longer assert `event_id` presence (optional per API spec)
- `test_get_interaction_stream` handles API not replaying completed interactions
- Updated `STREAMING_API.md` with notes about optional `event_id` field

## [0.5.0] - 2026-01-10

### Added

- `docs/INTERACTIONS_API_FEEDBACK.md`: Comprehensive feedback report for Google Gemini API team documenting 9 issues discovered while building genai-rs
- Thought signature test coverage across 7 configurations (stateful, stateless, parallel, sequential, ThinkingLevel::High, FunctionCallingMode::Any, streaming)
- `test_speech_config_nested_format_fails_flat_succeeds`: Test proving only flat SpeechConfig format works (nested format returns 400)

### Changed

- `docs/ENUM_WIRE_FORMATS.md`: Updated SpeechConfig section - nested format fails with 400 error
- `docs/MULTI_TURN_FUNCTION_CALLING.md`: Added thought signature matrix with verified test links

### BREAKING CHANGES

#### Enum Unknown Variant Upgrade (#329)

Three enums upgraded from `#[serde(other)]` fallback to full Evergreen Unknown variant pattern. This enables logging and debugging of unrecognized API values.

**Affected types:**
- `UrlRetrievalStatus`: Variant renames for consistency
- `CodeExecutionOutcome`: Full Unknown pattern with data preservation
- `CodeExecutionLanguage`: Full Unknown pattern, `Unspecified` variant removed

**UrlRetrievalStatus variant renames:**
| Before | After |
|--------|-------|
| `UrlRetrievalStatusUnspecified` | `Unspecified` |
| `UrlRetrievalStatusSuccess` | `Success` |
| `UrlRetrievalStatusUnsafe` | `Unsafe` |
| `UrlRetrievalStatusError` | `Error` |

**CodeExecutionLanguage changes:**
- `Unspecified` variant removed (API only returns known languages)
- `Unknown { language_type, data }` variant added for forward compatibility

**Copy trait removed** from all three types (Unknown variants contain `serde_json::Value`).

**Migration guide:**

```rust
// UrlRetrievalStatus: Update variant names
// Before:
match status {
    UrlRetrievalStatus::UrlRetrievalStatusSuccess => { ... }
    UrlRetrievalStatus::UrlRetrievalStatusError => { ... }
    _ => { ... }
}

// After:
match status {
    UrlRetrievalStatus::Success => { ... }
    UrlRetrievalStatus::Error => { ... }
    UrlRetrievalStatus::Unknown { status_type, .. } => {
        log::warn!("Unknown status: {}", status_type);
    }
    _ => { ... }
}

// CodeExecutionLanguage: Handle Unknown instead of Unspecified
// Before:
match language {
    CodeExecutionLanguage::Python => { ... }
    CodeExecutionLanguage::Unspecified => { ... }
}

// After:
match language {
    CodeExecutionLanguage::Python => { ... }
    CodeExecutionLanguage::Unknown { language_type, .. } => {
        log::warn!("Unknown language: {}", language_type);
    }
    _ => { ... }
}

// Copy trait removal: Use .clone() where needed
// Before:
let outcome = *some_outcome_ref;

// After:
let outcome = some_outcome_ref.clone();
```

#### InteractionContent Field Type Audit (#318)

Wire format alignment fixes for `InteractionContent` variants. These changes fix critical mismatches where real API data was silently falling back to `Unknown` variants.

- **`InteractionContent::Thought`**: Field `text` renamed to `signature`
  - Thoughts contain cryptographic signatures for verification, not human-readable reasoning
  - Use `response.thought_signatures()` to iterate over signatures
  - Use `response.has_thoughts()` to check for thought presence

- **`InteractionContent::UrlContextCall`**: Field `url` split into `id` + `urls`
  - `id: String` - Call identifier for matching results
  - `urls: Vec<String>` - List of URLs requested
  - Use `response.url_context_call_id()` and `response.url_context_call_urls()`

- **`InteractionContent::UrlContextResult`**: Fields `url`/`content` replaced with `call_id` + `result`
  - `call_id: String` - Matches the corresponding call
  - `result: Vec<UrlContextResultItem>` - Results for each URL
  - New `UrlContextResultItem` type with `url`, `status` fields and `is_success()`/`is_error()`/`is_unsafe()` helpers

- **`InteractionResponse::thoughts()`**: Method removed
  - Was returning signatures but named incorrectly
  - Use `thought_signatures()` instead

- **`InteractionResponse::url_context_call()`**: Method renamed to `url_context_call_id()`
  - New `url_context_call_urls()` method returns the list of URLs

**Migration guide:**

```rust
// Before: Thought had text field
InteractionContent::Thought { text: Some(t) } => println!("{}", t);

// After: Thought has signature field (cryptographic, not readable)
InteractionContent::Thought { signature: Some(s) } => {
    // s is a cryptographic signature, not human-readable text
    println!("Has thought signature: {}", s.len() > 0);
}

// Before: UrlContextCall had single url
InteractionContent::UrlContextCall { url } => println!("{}", url);

// After: UrlContextCall has id + urls
InteractionContent::UrlContextCall { id, urls } => {
    println!("Call {}: {:?}", id, urls);
}

// Before: UrlContextResult had url/content
InteractionContent::UrlContextResult { url, content } => { ... }

// After: UrlContextResult has call_id + result array
InteractionContent::UrlContextResult { call_id, result } => {
    for item in result {
        if item.is_success() {
            println!("Fetched: {}", item.url);
        }
    }
}

// Before: Using thoughts() method
for thought in response.thoughts() { ... }

// After: Use thought_signatures()
for sig in response.thought_signatures() { ... }
```

### Added

- **`UrlContextResultItem` type** (#318): New struct for URL context result items
  - `url: String` - The URL that was fetched
  - `status: String` - Result status ("success", "error", "unsafe")
  - Helper methods: `is_success()`, `is_error()`, `is_unsafe()`

- **`UsageMetadata::total_thought_tokens` field** (#318): Token count for thinking/reasoning
  - Use `response.thought_tokens()` helper method

### Fixed

- **`CodeExecutionResult` outcome derivation** (#318): When `is_error` is `None`, outcome now correctly defaults to `Ok` instead of `Unspecified`

## [0.4.0] - 2026-01-08

### BREAKING CHANGES

#### Crate Renamed to `genai-rs`
- **`rust-genai` is now `genai-rs`** - Update your Cargo.toml dependencies
- **`rust-genai-macros` is now `genai-rs-macros`** - Update macro imports
- Change `use rust_genai::*` to `use genai_rs::*` in your code

#### MSRV Bumped to Rust 1.88
- **Minimum Supported Rust Version is now 1.88** (was 1.85)
- Required for Edition 2024 `let` chains feature
- Update your Rust toolchain: `rustup update`

#### Crate Consolidation (#302)
- **`genai-client` crate merged into `genai-rs`**
- Internal HTTP and type modules are now `pub(crate)` instead of separate crate
- Users only depend on `genai-rs` - no change to public API

### Added

#### Text-to-Speech Audio Output (#303)
- **New `with_audio_output()` method** - Generate speech from text
- **New `with_voice(name)` method** - Select voice (Kore, Puck, Aoede, etc.)
- **New `with_speech_config(SpeechConfig)` method** - Full voice/language/speaker control
- **New `SpeechConfig` type** with constructors:
  - `SpeechConfig::with_voice("Kore")`
  - `SpeechConfig::with_voice_and_language("Puck", "en-GB")`
- **New response helpers**: `first_audio()`, `audios()`, `has_audio()`
- **New `AudioInfo` type** with `bytes()`, `mime_type()`, `extension()` methods
- Use model `gemini-2.5-pro-preview-tts` for TTS

```rust
let response = client
    .interaction()
    .with_model("gemini-2.5-pro-preview-tts")
    .with_text("Hello, world!")
    .with_audio_output()
    .with_voice("Kore")
    .create()
    .await?;

if let Some(audio) = response.first_audio() {
    std::fs::write("speech.wav", audio.bytes()?)?;
}
```

#### New Built-in Tools
- **File Search tool (#299)** - Semantic document retrieval from vector stores
  - New `with_file_search(store_ids)` method
- **Computer Use tool (#298)** - Browser automation via Gemini
  - New `with_computer_use()` method (requires allowlisted API key)
- **MCP Server convenience (#295)** - Connect to Model Context Protocol servers
  - New `add_mcp_server(uri)` method

#### Explicit Multi-Turn Conversations (#296)
- **New `with_turns(Vec<Turn>)` method** - Provide full conversation history
- **New `Turn` type** with `user()` and `model()` constructors
- Alternative to `previous_interaction_id` for stateless deployments

#### Typed Agent Configuration (#293)
- **New `AgentConfig` type** for Deep Research and Dynamic agents
- **`DeepResearchConfig`** with `with_thinking_summaries()` builder
- **`DynamicConfig`** for dynamic agent interactions
- Use `with_agent_config(config.into())` on builder

#### Resolution Control for Media (#297)
- **New `with_resolution(MediaResolution)` method** on `ImageInput` and `VideoInput`
- Control processing resolution: `Low`, `Medium`, `High`, `Native`

### Infrastructure

#### CI/CD Workflows
- **Automated crates.io publishing** on version tags
- **Release Drafter** for automatic release notes from PR labels
- **MSRV Check** (Rust 1.88), **Cross-platform testing** (Linux, macOS, Windows)
- **Code coverage** with Codecov integration
- **Security audit** with cargo-audit

#### Comprehensive Documentation
- 12 new documentation guides in `docs/`
- New `TROUBLESHOOTING.md` for common issues

### Fixed

- **ThinkingSummaries wire format** (#272): Fixed serialization to use `THINKING_SUMMARIES_AUTO` and `THINKING_SUMMARIES_NONE` (API's actual wire format) instead of `auto`/`none` (what the documentation claims). This enables `agent_config` with `thinking_summaries` to work correctly with the Deep Research agent.
- **Clippy lints for Rust 1.92** - Use `is_multiple_of()` and collapsed `if let` chains

### BREAKING CHANGES

#### Timestamp Fields Use chrono::DateTime<Utc> (#273)
- **`FileMetadata.create_time`**: Changed from `Option<String>` to `Option<DateTime<Utc>>`
- **`FileMetadata.expiration_time`**: Changed from `Option<String>` to `Option<DateTime<Utc>>`
- **`InteractionResponse`**: Added `created: Option<DateTime<Utc>>` and `updated: Option<DateTime<Utc>>` fields
- **New dependency**: `chrono` crate with serde support
- Internal `loud_wire.rs` timestamp generation simplified to use chrono

**Migration guide:**
```rust
// Before (FileMetadata timestamps were strings):
if let Some(created) = file.create_time {
    println!("Created: {}", created);  // String
}

// After (timestamps are DateTime<Utc>):
use chrono::{DateTime, Utc};
if let Some(created) = file.create_time {
    println!("Created: {}", created.to_rfc3339());  // DateTime<Utc>
    // Or use chrono's formatting:
    println!("Created: {}", created.format("%Y-%m-%d %H:%M:%S"));
}

// InteractionResponse now has created/updated fields:
if let Some(created) = response.created {
    println!("Interaction created at: {}", created);
}
```

#### Streaming Returns StreamEvent Wrapper (#262)
- **`create_stream()`** now returns `Stream<Item = Result<StreamEvent, GenaiError>>` instead of `Stream<Item = Result<StreamChunk, GenaiError>>`
- **`create_stream_with_auto_functions()`** now returns `Stream<Item = Result<AutoFunctionStreamEvent, GenaiError>>` instead of `Stream<Item = Result<AutoFunctionStreamChunk, GenaiError>>`
- **New `StreamEvent` struct**: Wraps `StreamChunk` with `event_id` field for stream resume support
- **New `AutoFunctionStreamEvent` struct**: Wraps `AutoFunctionStreamChunk` with `event_id` field
- **New `get_interaction_stream()` method**: Resume streams from a specific `event_id` position

**Migration guide:**
```rust
// Before:
while let Some(chunk) = stream.next().await {
    match chunk? {
        StreamChunk::Delta(content) => { /* ... */ }
        StreamChunk::Complete(response) => { /* ... */ }
        _ => {}
    }
}

// After:
while let Some(result) = stream.next().await {
    let event = result?;
    // Optionally track event_id for resume support
    if let Some(id) = &event.event_id {
        last_event_id = Some(id.clone());
    }
    match event.chunk {  // Access .chunk on the event
        StreamChunk::Delta(content) => { /* ... */ }
        StreamChunk::Complete(response) => { /* ... */ }
        _ => {}
    }
}

// To resume an interrupted stream:
let resumed = client.get_interaction_stream(&interaction_id, Some(&last_event_id));
```

#### Additional Enums Now #[non_exhaustive] (#196)
- **`GenaiError`**: Match statements must include a wildcard arm
- **`FunctionError`**: Match statements must include a wildcard arm
- **`InteractionInput`**: Match statements must include a wildcard arm
- **New `GenaiError::MalformedResponse` variant**: For cases where the API returns 200 OK but with unexpected/malformed content
- This follows [Evergreen principles](https://github.com/google-deepmind/evergreen-spec) for forward-compatible API design

**Migration guide:**
```rust
// Before (exhaustive match):
match error {
    GenaiError::Http(e) => ...,
    GenaiError::Api { .. } => ...,
    // etc.
}

// After (must include wildcard):
match error {
    GenaiError::Http(e) => ...,
    GenaiError::Api { .. } => ...,
    GenaiError::MalformedResponse(msg) => ...,
    _ => ...,  // Required for forward compatibility
}
```

#### URI Content Helpers Require mime_type (#131)
- **Changed signatures**: `image_uri_content()`, `audio_uri_content()`, `video_uri_content()`, `document_uri_content()` now require `mime_type` as a mandatory parameter instead of `Option<String>`
- **Rationale**: Gemini API requires mime_type for URI-based content; making this compile-time enforced prevents runtime API errors

**Migration guide:**
```rust
// Before:
image_uri_content("https://example.com/image.png", Some("image/png".to_string()))

// After:
image_uri_content("https://example.com/image.png", "image/png")
```

#### Tool Enum is Now #[non_exhaustive] (#131)
- **`Tool` enum now includes `#[non_exhaustive]`**: Match statements must include a wildcard arm
- **New `Tool::Unknown` variant**: Captures unrecognized tool types from the API without failing deserialization
- This follows [Evergreen principles](https://github.com/google-deepmind/evergreen-spec) for forward-compatible API design

#### Error Type Consolidation (#131)
- **`InternalError` renamed to `GenaiError`** in `genai-client` crate
- New `Internal` and `InvalidInput` variants for better error categorization
- Users of the public `genai-rs` crate are unaffected (uses the same `GenaiError`)

#### create_with_auto_functions() Returns AutoFunctionResult (#148)
- **Changed return type**: `create_with_auto_functions()` now returns `AutoFunctionResult` instead of `InteractionResponse`
- **New `AutoFunctionResult` type**: Contains both the final response and execution history
- Provides visibility into which functions were called, enabling debugging, logging, and evaluation

**Migration guide:**
```rust
// Before:
let response = builder.create_with_auto_functions().await?;
println!("{}", response.text().unwrap());

// After:
let result = builder.create_with_auto_functions().await?;
println!("{}", result.response.text().unwrap());

// New: Access execution history with timing
for exec in &result.executions {
    println!("Called {} ({:?}) -> {}", exec.name, exec.duration, exec.result);
}
```

### Added

- **Request timeout and token usage helpers** (#228):
  - New `with_timeout(Duration)` on `InteractionBuilder` for per-request timeouts
  - For `create()`: Overall request timeout
  - For `create_stream()`: Per-chunk timeout to detect stalled connections
  - New `GenaiError::Timeout(Duration)` variant returned when requests exceed timeout
  - Token usage helper methods on `InteractionResponse`:
    - `input_tokens()`, `output_tokens()`, `total_tokens()`
    - `reasoning_tokens()`, `cached_tokens()`, `tool_use_tokens()`
  - Warning logged when timeout used with auto-function methods (not yet supported)

- **ToolService trait for dependency injection** (#197):
  - New `ToolService` trait enables tools to access shared state (DB connections, API clients, config)
  - Use `with_tool_service(Arc<dyn ToolService>)` on `InteractionBuilder` to provide tools
  - Service-provided functions take precedence over global `#[tool]` registry functions
  - When a service function shadows a global function, a warning is logged
  - Works with both `create_with_auto_functions()` and `create_stream_with_auto_functions()`

- **Partial results when max_function_call_loops exceeded** (#172):
  - `create_with_auto_functions()` now returns partial results instead of error when limit is hit
  - New `reached_max_loops: bool` field on `AutoFunctionResult` indicates if limit was reached
  - The `response` field contains the last API response (likely with pending function calls)
  - The `executions` vector preserves all function calls that were executed before hitting the limit
  - Enables debugging stuck function loops and accessing partial work
  - New `AutoFunctionStreamChunk::MaxLoopsReached` variant for streaming (parallel change)
  - `AutoFunctionResultAccumulator` now handles `MaxLoopsReached` and sets `reached_max_loops: true`
  - Legacy JSON without `reached_max_loops` field deserializes with default `false`

- **Function execution timing** (#148):
  - `FunctionExecutionResult.duration` tracks how long each function took to execute
  - Duration is serialized as milliseconds for JSON compatibility
  - Useful for performance monitoring, debugging, and optimization

- **Streaming accumulator helper** (#148):
  - New `AutoFunctionResultAccumulator` type to collect `AutoFunctionResult` from streaming
  - Allows combining streaming UI updates with execution history collection
  - Example:
    ```rust
    let mut accumulator = AutoFunctionResultAccumulator::new();
    while let Some(chunk) = stream.next().await {
        if let Some(result) = accumulator.push(chunk?) {
            // Stream complete, result contains full execution history
            println!("Executed {} functions", result.executions.len());
        }
    }
    ```

- **Full `Serialize`/`Deserialize` support for save/resume semantics** (#148, #151):
  - `InteractionResponse` now implements `Serialize` for logging, caching, and persistence
  - `AutoFunctionResult` implements `Serialize` and `Deserialize` for full execution history
  - `FunctionExecutionResult` now implements `Deserialize` for roundtrip serialization
  - `StreamChunk` and `AutoFunctionStreamChunk` implement both traits for streaming event replay
  - New `AutoFunctionStreamChunk::Unknown` variant for forward-compatible deserialization
  - Enables offline replay, testing/mocking, and state persistence for long-running agents

- **New convenience helpers on `InteractionResponse`** (#131):
  - `google_search_call()` - returns first Google Search call (singular)
  - `code_execution_call()` - returns first Code Execution call (singular)
  - `url_context_call()` - returns first URL Context call (singular)

#### Unified Streaming Content Types (#39, #27)
- **`StreamDelta` enum removed**: Streaming deltas now use `InteractionContent` directly
  - `StreamChunk::Delta(InteractionContent)` contains incremental content during streaming
  - `StreamChunk::Complete(InteractionResponse)` contains the final complete response
- **New `InteractionContent::ThoughtSignature` variant**: Captures streaming thought signatures
- **New helper methods on `InteractionContent`**: `text()`, `is_text()`, `is_thought()`, `is_thought_signature()`, `is_function_call()`
- **New type exported**: `StreamChunk` (note: `StreamDelta` is no longer exported)

**Migration guide:**
```rust
// Before:
match chunk {
    StreamChunk::Delta(delta) => match delta {
        StreamDelta::Text { text } => println!("{}", text),
        StreamDelta::Thought { text } => println!("[thinking: {}]", text),
        _ => {}
    }
    StreamChunk::Complete(response) => { /* ... */ }
}

// After:
match chunk {
    StreamChunk::Delta(content) => match content {
        InteractionContent::Text { text } => println!("{}", text.as_deref().unwrap_or("")),
        InteractionContent::Thought { text } => println!("[thinking: {}]", text.as_deref().unwrap_or("")),
        InteractionContent::FunctionCall { name, args, .. } => {
            println!("Function call: {}({:?})", name, args);
        }
        _ => {}
    }
    StreamChunk::Complete(response) => { /* ... */ }
}

// Helper methods still work the same:
if let Some(text) = delta.text() { /* ... */ }
```

### Added
- **Google Search grounding support** (#25): Enable real-time web search integration with Gemini models
  - New `with_google_search()` builder method on `InteractionBuilder`
  - New types: `GroundingMetadata`, `GroundingChunk`, `WebSource`
  - New helper methods: `has_google_search_metadata()`, `google_search_metadata()`, `has_google_search_calls()`, `google_search_calls()` on `InteractionResponse`
  - Full streaming support via `StreamChunk::Complete`

- **Code execution support** (#26): Enable Python code execution via Gemini's built-in sandbox
  - New `with_code_execution()` builder method on `InteractionBuilder`
  - New `CodeExecutionOutcome` enum with `Ok`, `Failed`, `DeadlineExceeded`, `Unspecified` variants
  - Updated `InteractionContent::CodeExecutionCall` with typed fields: `id`, `language`, `code`
  - Updated `InteractionContent::CodeExecutionResult` with typed fields: `call_id`, `outcome`, `output`
  - New helper methods on `InteractionResponse`: `code_execution_calls()`, `code_execution_results()`, `successful_code_output()`
  - New helper functions: `code_execution_call_content()`, `code_execution_result_content()`, `code_execution_success()`, `code_execution_error()`
  - Backward-compatible deserialization for old API response format
  - **Breaking (serialization)**: `CodeExecutionCall` now serializes `language` and `code` as top-level fields instead of nested in `arguments`. Deserialization remains backward-compatible with both formats.

- **URL context support** (#63): Enable URL content fetching and analysis
  - New `with_url_context()` builder method on `InteractionBuilder`
  - New types: `UrlContextMetadata`, `UrlMetadataEntry`, `UrlRetrievalStatus`
  - New helper methods: `has_url_context_metadata()`, `url_context_metadata()`, `has_url_context_calls()`, `url_context_calls()` on `InteractionResponse`
  - Supports up to 20 URLs per request, max 34MB per URL

- **Structured output JSON schema support** (#80): Enforce JSON schema constraints on model responses
  - Use `.with_response_format(schema)` to specify a JSON schema for structured output
  - Works standalone for structured data extraction
  - Combines with built-in tools (Google Search, URL Context)
  - New comprehensive example: `examples/structured_output.rs`

- **Function call/result structs**: New `FunctionCallInfo` and `FunctionResultInfo` structs with named fields for cleaner access
  - `function_calls()` now returns `Vec<FunctionCallInfo>` with fields: `id`, `name`, `args`, `thought_signature`
  - `function_results()` now returns `Vec<FunctionResultInfo>` with fields: `name`, `call_id`, `result`
  - New `has_function_results()` method on `InteractionResponse` for parity with `has_function_calls()`

- **Logging strategy documentation** (#203): New `docs/LOGGING_STRATEGY.md` with comprehensive guidelines
  - Log level definitions (error/warn/debug) with concrete examples
  - Sensitive data handling (API keys redacted, user content at debug only)
  - Evergreen pattern logging (Unknown variants log at warn level)
  - Debug logging for auto-function loop lifecycle (iteration tracking, execution timing)
  - Enable with `RUST_LOG=genai_rs=debug`

### Changed
- **`InteractionContent` is now `#[non_exhaustive]`** (#44): Match statements must include a wildcard arm (`_ => {}`). This allows adding new variants in minor version updates without breaking downstream code.
- **Deep Research example now requires background mode** (#179): Updated `deep_research.rs` example to reflect API requirement that `background=true` is mandatory for agent interactions. Removed synchronous mode demonstration since it is no longer supported by the API.
- **Function execution failures now log at `warn!` instead of `error!`** (#203): Since function failures are recoverable (the error is sent to the model which can retry or adapt), they are now correctly logged as warnings rather than errors. This aligns with the new logging strategy documented in `docs/LOGGING_STRATEGY.md`.

### Fixed
- **Streaming with function calls now works** (#27): Function call deltas are now properly parsed instead of causing errors
- **Streaming now properly yields content chunks** (#17): The streaming API was returning 0 chunks because the code expected all SSE events to have an `interaction` field, but the API sends different event types (`content.delta` and `interaction.complete`)

#### Simplified Client API
- **`Client::new()` signature simplified**: No longer takes `api_version` parameter
  - Before: `Client::new(api_key, None)`
  - After: `Client::new(api_key)`
  - The `api_version` was stored but never used; the library defaults to V1Beta internally
- **`ApiVersion` no longer re-exported** from genai-rs (still available in genai-client for internal use)

#### Removed deprecated function calling helpers
- **`function_response_content()` helper removed**: Use `function_result_content()` instead
  - Before: `function_response_content("get_weather", json!({"temp": 72}))`
  - After: `function_result_content("get_weather", "call_123", json!({"temp": 72}))`
  - The `call_id` parameter is required for proper API response matching
- **`InteractionContent::FunctionResponse` variant removed**: Use `FunctionResult` variant instead

#### UsageMetadata field names updated (#24)
- **Field names now match Interactions API**: The old GenerateContent API field names have been replaced
  - `prompt_tokens` → `total_input_tokens`
  - `candidates_tokens` → `total_output_tokens`
  - `total_tokens` remains unchanged
- **New fields added**: `total_cached_tokens`, `total_reasoning_tokens`, `total_tool_use_tokens`
- **Token usage now works**: Previously always returned `None` due to field name mismatch

## [0.2.0] - 2025-12-23

### BREAKING CHANGES

This release removes the legacy GenerateContent API in favor of the unified Interactions API. This is a major breaking change that requires code migration.

#### Removed
- **GenerateContent API**: All `GenerateContentBuilder` methods and related functionality removed
  - `Client::with_model()` method removed
  - `GenerateContentBuilder` type removed
  - `generate_from_request()` and `stream_from_request()` methods removed
  - `GenerateContentResponse` type removed (use `InteractionResponse` instead)

- **Helper modules**:
  - `content_api` module removed (use `interactions_api` instead)
  - `internal/response_processing` module removed

- **Examples**: Removed all GenerateContent examples
  - `simple_request.rs`
  - `stream_request.rs`
  - `code_execution.rs`
  - `function_call.rs`
  - `gemini3_thought_signatures.rs`

- **Internal crates**:
  - `genai-client/src/core.rs` removed
  - `genai-client/src/models/request.rs` removed
  - `genai-client/src/models/response.rs` removed

### Added

- **Enhanced InteractionResponse**:
  - New `.text()` convenience method to extract text from interaction responses
  - New `.function_calls()` convenience method to extract function calls with thought signatures

- **Automatic function calling for Interactions API**:
  - New `InteractionBuilder::create_with_auto_functions()` method
  - Auto-discovers and executes functions from the global registry
  - Supports multi-turn function calling with automatic loop handling

- **New helper functions**:
  - `function_result_content()` for sending function execution results (correct API format)
  - Enhanced `function_call_content_with_signature()` to include optional call ID

### Fixed

- **Function calling implementation** now correctly follows Google's Interactions API specification:
  - Added `id` field to `FunctionCall` to capture the call identifier from the API
  - Added new `FunctionResult` content type with `call_id` field (replaces `FunctionResponse`)
  - `create_with_auto_functions()` now sends only function results (not the original calls)
  - The API server maintains function call context via `previous_interaction_id`
  - Deprecated `FunctionResponse` variant (use `FunctionResult` instead)
  - Improved error message when max function call loops (5) is exceeded

### Changed

- **Primary API**: The Interactions API is now the only supported API
- **Migration Path**:
  - Replace `client.with_model(...).with_prompt(...).generate()`
  - With `client.interaction().with_model(...).with_text(...).create()`
  - Replace `generate_with_auto_functions()` with `create_with_auto_functions()`
  - Use `interactions_api` helper functions instead of `content_api`

### Migration Guide

#### Before (v0.1.x - GenerateContent API):
```rust
let response = client
    .with_model("gemini-3.6-flash")
    .with_prompt("Hello, world!")
    .generate()
    .await?;

println!("{}", response.text.unwrap());
```

#### After (v0.2.0 - Interactions API):
```rust
let response = client
    .interaction()
    .with_model("gemini-3.6-flash")
    .with_text("Hello, world!")
    .create()
    .await?;

println!("{}", response.text().unwrap_or("No text"));
```

#### Streaming:
```rust
// Before
let stream = client
    .with_model("gemini-3.6-flash")
    .with_prompt("Hello")
    .generate_stream()?;

// After
let stream = client
    .interaction()
    .with_model("gemini-3.6-flash")
    .with_text("Hello")
    .create_stream();
```

#### Automatic Function Calling:
```rust
// Before
let response = client
    .with_model("gemini-3.6-flash")
    .with_prompt("What's the weather?")
    .generate_with_auto_functions()
    .await?;

// After
let response = client
    .interaction()
    .with_model("gemini-3.6-flash")
    .with_text("What's the weather?")
    .create_with_auto_functions()
    .await?;
```

## [0.1.0] - 2024-12-XX

### Added
- Initial release
- Support for GenerateContent API
- Support for Interactions API
- Function calling with automatic discovery via macros
- Streaming support for both APIs
- Comprehensive test suite
- Example programs for both APIs
