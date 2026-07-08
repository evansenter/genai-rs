# Enum Wire Formats & Unknown Variants

This document captures:
1. **Wire formats** for enums in the Gemini Interactions API (the official docs are sometimes wrong)
2. **Unknown variant types** that implement Evergreen soft-typing for forward compatibility

**API revision**: This catalog reflects **Interactions API revision 2026-05-20**. Every
request sends the `Api-Revision: 2026-05-20` header (see `src/http/common.rs`). Entries
marked **Pending live verification (2026-05-20 revision)** have wire formats derived from
the API spec and unit-tested serialization, but have not yet been confirmed against the
live API with `LOUD_WIRE=1`.

## Types with Unknown Variants

All types below implement graceful handling of unrecognized values via an `Unknown` variant. This ensures the library doesn't break when Google adds new enum values.

| # | Type | Location | Context Field | Notes |
|---|------|----------|---------------|-------|
| 1 | `Content` | src/content.rs | `content_type` | Media-only: text/image/audio/video/document |
| 2 | `Step` | src/steps.rs | `step_type` | Interaction steps (17 known types) |
| 3 | `StepDelta` | src/steps.rs | `delta_type` | `step.delta` SSE payloads |
| 4 | `Annotation` | src/content.rs | `annotation_type` | Citation union (url/file/place) |
| 5 | `Resolution` | src/content.rs | `resolution_type` | Image/video quality |
| 6 | `StreamChunk` | src/wire_streaming.rs | `chunk_type` | Low-level SSE chunks |
| 7 | `AutoFunctionStreamChunk` | src/streaming.rs | `chunk_type` | High-level streaming |
| 8 | `FileState` | src/http/files.rs | `state_type` | File upload states |
| 9 | `Tool` | src/tools.rs | `tool_type` | Tool types |
| 10 | `FunctionCallingMode` | src/tools.rs | `mode_type` | auto/any/none/validated (lowercase) |
| 11 | `ToolChoice` | src/tools.rs | `choice_type` | Mode string OR `allowed_tools` object |
| 12 | `Role` | src/request.rs | `role_type` | `ConversationBuilder` only — not an API wire enum |
| 13 | `ThinkingLevel` | src/request.rs | `level_type` | minimal/low/medium/high |
| 14 | `ThinkingSummaries` | src/request.rs | `summaries_type` | Context-dependent format |
| 15 | `ServiceTier` | src/request.rs | `tier_type` | flex/standard/priority |
| 16 | `InteractionStatus` | src/response.rs | `status_type` | Response status (+`budget_exceeded`) |
| 17 | `CodeExecutionLanguage` | src/content.rs | `language_type` | Programming language (lowercase) |
| 18 | `ImageAspectRatio` | src/request.rs | `ratio_type` | Image aspect ratios (14 values) |
| 19 | `ImageSize` | src/request.rs | `size_type` | Image resolution (512/1K/2K/4K) |
| 20 | `SearchType` | src/tools.rs | `search_type` | web_search/image_search/enterprise_web_search |
| 21 | `RetrievalType` | src/tools.rs | `retrieval_type` | vertex_ai_search/rag_store/exa_ai_search/parallel_ai_search |
| 22 | `WebhookEvent` | src/webhooks.rs | `event_type` | batch.*/interaction.*/video.generated |
| 23 | `WebhookState` | src/webhooks.rs | `state_type` | enabled/disabled/disabled_due_to_failed_deliveries |
| 24 | `RevocationBehavior` | src/webhooks.rs | `behavior_type` | Signing-secret rotation behavior |
| 25 | `SourceType` | src/environment.rs | `source_type` | gcs/inline/repository/skill_registry |
| 26 | `NetworkConfig` | src/environment.rs | `network_type` | `"disabled"` string OR `{allowlist}` object |
| 27 | `EnvironmentSpec` | src/environment.rs | `environment_type` | Env-ID string OR `{type:"remote"}` object |
| 28 | `ResponseDelivery` | src/response_format.rs | `delivery_type` | inline/uri |
| 29 | `ResponseFormat` | src/response_format.rs | `format_type` | text/audio/image/video union |
| 30 | `VideoTask` | src/request.rs | `task_type` | text_to_video/image_to_video/reference_to_video/edit |
| 31 | `Visualization` | src/request.rs | `visualization_type` | off/auto (Deep Research agent_config) |

**Removed in revision 2026-05-20** (no longer exist in this library or on the wire):
`UrlRetrievalStatus`, `GroundingMetadata`, `UrlContextMetadata`, `Turn`, and all tool-related
`Content` variants (`Thought`, `FunctionCall`, `FunctionResult`, `CodeExecutionCall/Result`,
`GoogleSearchCall/Result`, `UrlContextCall/Result`, `FileSearchResult`, `GoogleMapsCall/Result`,
`ComputerUseCall/Result`). Tool activity now flows through the `Step` enum; computer-use
actions flow through plain `function_call` steps.

### Unknown Variant Pattern

All Unknown variants follow this naming convention:

```rust,ignore
Unknown {
    <context>_type: String,      // The unrecognized type from API
    data: serde_json::Value,     // Full JSON preserved for roundtrip
}
```

Helper methods on each type:
- `is_unknown()` - Check if this is an Unknown variant
- `unknown_<context>_type()` - Get the unrecognized type string
- `unknown_data()` - Get the preserved JSON data

### `strict-unknown` Feature Flag

- **Default (disabled)**: Unknown values deserialize into `Unknown` variant, logs warning
- **Strict mode (enabled)**: Unknown `Content` **and** `Step` types cause a deserialization error (fail-fast)
- Enable: `cargo test --features strict-unknown`
- CI runs dedicated `test-strict-unknown` job

## Quick Reference

| Enum / Type | Wire Format | Example | Notes |
|------|-------------|---------|-------|
| `Step` | tagged by `"type"`, snake_case | `"user_input"`, `"model_output"`, `"function_call"`, ... | Pending live verification (2026-05-20 revision) |
| `StepDelta` | tagged by `"type"` | `"text"`, `"arguments_delta"`, `"text_annotation_delta"` | Two tags differ from variant names — see details. Pending live verification (2026-05-20 revision) |
| `Annotation` | tagged by `"type"` | `"url_citation"`, `"file_citation"`, `"place_citation"` | Pending live verification (2026-05-20 revision) |
| `FunctionResultPayload` | untagged union | `"ok"` / `{...}` / `[{"type": "text", ...}]` | String, JSON, or content-block list — see details |
| `ToolChoice` | string OR object | `"any"` / `{"allowed_tools": {...}}` | Pending live verification (2026-05-20 revision) |
| `FunctionCallingMode` | lowercase | `"auto"`, `"any"`, `"none"`, `"validated"` | **Changed** from SCREAMING_CASE; legacy uppercase accepted on deserialize. Pending live verification (2026-05-20 revision) |
| `CodeExecutionLanguage` | lowercase | `"python"` | **Changed** from `"PYTHON"`; legacy uppercase accepted on deserialize. Pending live verification (2026-05-20 revision) |
| `ServiceTier` | lowercase | `"flex"`, `"standard"`, `"priority"` | New in 2026-05-20. Pending live verification (2026-05-20 revision) |
| `InteractionStatus` | snake_case | `"in_progress"`, `"requires_action"`, `"budget_exceeded"` | `budget_exceeded` new; pending live verification (2026-05-20 revision). `Default` is `InProgress` |
| `SearchType` | snake_case string | `"web_search"`, `"image_search"`, `"enterprise_web_search"` | `enterprise_web_search` new; pending live verification (2026-05-20 revision) |
| `GroundingToolCount` | `{"type": ..., "count": n}` | `{"type": "google_search", "count": 2}` | In `usage.grounding_tool_count`. Pending live verification (2026-05-20 revision) |
| `ThinkingSummaries` | `THINKING_SUMMARIES_*` | `"THINKING_SUMMARIES_AUTO"` | Docs claim `auto`/`none` - **wrong** |
| `ThinkingLevel` | lowercase | `"low"`, `"medium"`, `"high"` | Docs are correct |
| `Resolution` | snake_case | `"low"`, `"medium"`, `"high"`, `"ultra_high"` | Image/video content |
| `Tool::FileSearch` | snake_case object | `{"type": "file_search", ...}` | Rust: `store_names`, Wire: `file_search_store_names` |
| `Tool::GoogleSearch` | snake_case + optional array | `{"type": "google_search", "search_types": ["web_search"]}` | |
| `Tool::GoogleMaps` | snake_case + optional fields | `{"type": "google_maps", "enable_widget": true, "latitude": ..., "longitude": ...}` | `latitude`/`longitude` pending live verification (2026-05-20 revision) |
| `Tool::ComputerUse` | snake_case | `{"type": "computer_use", "environment": "browser", ...}` | **Changed**: fields now snake_case. Pending live verification (2026-05-20 revision) |
| `SpeechConfig` | **list** of flat objects | `[{"voice": "Kore", "language": "en-US", "speaker": "Alice"}]` | **Changed** in 2026-05-20: `speech_config` is a list (multi-speaker TTS); legacy single object accepted on deserialize. ✅ Verified live 2026-07 (two-speaker list accepted; single combined `audio/l16` stream returned; the API does not echo `speech_config` on reads — `include_input` observed as a no-op) |
| `Tool::Retrieval` | snake_case object | `{"type": "retrieval", "retrieval_types": [...], "vertex_ai_search_config": {...}}` | New. ⚠️ Live 2026-07: the Gemini API rejects `type: "retrieval"` (Vertex-only — "allowed on the Gemini Enterprise Agent Platform"); Gemini tool types are `google_maps`, `mcp_server`, `function`, `google_search`, `file_search`, `computer_use`, `code_execution`, `url_context` |
| `RetrievalType` | snake_case string | `"vertex_ai_search"`, `"rag_store"`, `"exa_ai_search"`, `"parallel_ai_search"` | Not verifiable live on the Gemini API (the retrieval tool itself is rejected as Vertex-only, 2026-07) |
| `WebhookEvent` | dotted lowercase | `"batch.succeeded"`, `"interaction.completed"`, `"video.generated"` | ✅ Verified live 2026-07: the API's own validation error lists exactly our 7 values |
| `WebhookState` | snake_case | `"enabled"`, `"disabled"`, `"disabled_due_to_failed_deliveries"` | Output only. ✅ Verified live 2026-07 (`enabled`/`disabled` observed; failed-deliveries state documented but not triggered) |
| `RevocationBehavior` | snake_case | `"revoke_previous_secrets_after_h24"`, `"revoke_previous_secrets_immediately"` | Request only (`:rotateSigningSecret`). ✅ Verified live 2026-07: the API's validation error lists exactly these two values; camelCase key rejected |
| `SourceType` | snake_case | `"gcs"`, `"inline"`, `"repository"`, `"skill_registry"` | Environment source `type`. ✅ `inline` verified live 2026-07 (accepted, `environment_id` returned); other source types not exercised |
| `NetworkConfig` | string OR object | `"disabled"` / `{"allowlist": [{"domain": "*.googleapis.com"}]}` | Omit field to allow all traffic. ✅ Verified live 2026-07: both `"disabled"` and `{allowlist: [{domain, transform}]}` accepted |
| `EnvironmentSpec` | string OR object | `"env-123"` / `{"type": "remote", "sources": [...], "network": ...}` | Request `environment` + agent `base_environment`. ✅ Verified live 2026-07 on requests: both the string-ID form and the typed remote object accepted (agent `base_environment` not verifiable — agent creation gated) |
| `ResponseFormat` | tagged by `"type"` OR raw schema dict | `{"type": "text", "mime_type": "application/json", "schema": {...}}` | Single object or list; raw schema dicts preserved via `Unknown`. ✅ Verified live 2026-07: single + list forms accepted (list errors index as `response_format[i]`); text schema enforced; image works inline-only with `image/jpeg` only; audio works with `sample_rate` but rejects `mime_type`/`delivery`; video `gcs_uri` is Vertex-only |
| `ResponseDelivery` | lowercase | `"inline"`, `"uri"` | Audio/image/video formats. ✅ Verified live 2026-07: the API's validation error lists exactly `inline`/`uri` — but `delivery` itself is currently rejected for audio and image on the Gemini API (inline-only) |
| `VideoTask` | snake_case | `"text_to_video"`, `"image_to_video"`, `"reference_to_video"`, `"edit"`, `"extend"` | `generation_config.video_config.task`. ✅ Verified live 2026-07 via the API's validation error — which also revealed `"extend"` (added to the enum) |
| `Visualization` | lowercase | `"off"`, `"auto"` | Deep Research `agent_config.visualization`. ✅ Verified live 2026-07: the API's validation error lists exactly `off`/`auto`; accepted with `collaborative_planning` (`enable_bigquery_tool` is Vertex-only) |
| Audio MIME type (TTS response) | plain | `"audio/l16"` | Raw PCM audio. Live 2026-07 (revision 2026-05-20): lowercase `audio/l16` with a separate `sample_rate: 24000` field on the content block (no `;codec=...;rate=...` params observed) |
| `GoogleSearchResultItem` | snake_case | `{"title": "...", "url": "...", "rendered_content": "..."}` | Optional `search_suggestions` added in 2026-05-20. Verified live 2026-07: items may carry **only** `search_suggestions` (an HTML rendering payload) with no `title`/`url`; empty `title`/`url` are skipped on serialize for wire fidelity |
| `UrlContextResultItem` | snake_case | `{"url": "...", "status": "success"}` | Verified 2026-01-13 - no paywall field |
| `ImageAspectRatio` | ratio string | `"1:1"`, `"16:9"`, `"9:16"` | 14 aspect ratios |
| `ImageSize` | size string | `"512"`, `"1K"`, `"2K"`, `"4K"` | Image resolution |

## Details

### Step (response `steps` / stateless history)

Revision 2026-05-20 replaces the launch-era `outputs: [Content]` array with
`steps: [Step]`. Steps are a discriminated union tagged by `"type"` with
snake_case values. All tool call/result steps (including `function_call` /
`function_result`) carry an optional opaque `signature` used for validation
on replay.

**Status**: Core shapes verified live 2026-07 against
`generativelanguage.googleapis.com` (Api-Revision 2026-05-20): the steps
model, snake_case field naming, and the `function_call` `signature` field
were all confirmed on the wire. Serialization covered by unit and proptest
roundtrip tests.

| Wire `type` | Rust Variant | Payload shape |
|-------------|--------------|---------------|
| `user_input` | `Step::UserInput` | `{"content": [Content, ...]}` |
| `model_output` | `Step::ModelOutput` | `{"content": [Content, ...], "error"?: {code, message, details}}` |
| `thought` | `Step::Thought` | `{"signature"?: "...", "summary"?: [Content, ...]}` |
| `function_call` | `Step::FunctionCall` | `{"id": "...", "name": "...", "arguments": {...}, "signature"?: "..."}` — **top-level**, no nesting. `signature` VERIFIED LIVE 2026-07: the API returns it and rejects stateless replay without it (the generated SDK bindings omit it) |
| `function_result` | `Step::FunctionResult` | `{"call_id": "...", "name"?: "...", "result": <payload>, "is_error"?: bool, "signature"?: "..."}` — optional signature hash for backend validation |
| `code_execution_call` | `Step::CodeExecutionCall` | `{"id": "...", "arguments": {"language": "python", "code": "..."}, "signature"?: "..."}` — **nested** arguments |
| `code_execution_result` | `Step::CodeExecutionResult` | `{"call_id": "...", "result": "...", "is_error": bool, "signature"?: "..."}` |
| `url_context_call` | `Step::UrlContextCall` | `{"id": "...", "arguments": {"urls": [...]}, "signature"?: "..."}` |
| `url_context_result` | `Step::UrlContextResult` | `{"call_id": "...", "result": [UrlContextResultItem], "is_error"?: bool, "signature"?: "..."}` |
| `google_search_call` | `Step::GoogleSearchCall` | `{"id": "...", "arguments": {"queries": [...]}, "search_type"?: "...", "signature"?: "..."}` |
| `google_search_result` | `Step::GoogleSearchResult` | `{"call_id": "...", "result": [GoogleSearchResultItem], "is_error"?: bool, "signature"?: "..."}` |
| `mcp_server_tool_call` | `Step::McpServerToolCall` | `{"id": "...", "name": "...", "server_name": "...", "arguments": {...}}` |
| `mcp_server_tool_result` | `Step::McpServerToolResult` | `{"call_id": "...", "name"?: "...", "server_name"?: "...", "result": <payload>}` |
| `file_search_call` | `Step::FileSearchCall` | `{"id": "...", "signature"?: "..."}` |
| `file_search_result` | `Step::FileSearchResult` | `{"call_id": "...", "result": [FileSearchResultItem], "signature"?: "..."}` |
| `google_maps_call` | `Step::GoogleMapsCall` | `{"id": "...", "arguments": {"queries": [...]}, "signature"?: "..."}` |
| `google_maps_result` | `Step::GoogleMapsResult` | `{"call_id": "...", "result": [GoogleMapsResultItem], "signature"?: "..."}` |
| (anything else) | `Step::Unknown { step_type, data }` | Full JSON preserved for roundtrip |

Note the asymmetry: `function_call` and `mcp_server_tool_call` keep their
arguments at the **top level**, while the built-in tool calls
(`code_execution_call`, `url_context_call`, `google_search_call`,
`google_maps_call`) nest theirs inside an `arguments` object. The library
flattens the nested forms into ergonomic fields (`urls: Vec<String>`,
`queries: Vec<String>`, `language` + `code`).

```rust
use genai_rs::Step;
use serde_json::json;

// function_call: id/name/arguments at top level
let step = Step::function_call("call_1", "get_weather", json!({"city": "Paris"}));
let wire = serde_json::to_value(&step).unwrap();
assert_eq!(wire["type"], "function_call");
assert_eq!(wire["id"], "call_1");
assert_eq!(wire["arguments"]["city"], "Paris");
```

Example `code_execution_call` wire format (nested `arguments`):

```json
{
  "type": "code_execution_call",
  "id": "exec_123",
  "arguments": { "language": "python", "code": "print('Hello')" },
  "signature": "ErIE..."
}
```

`Step::Unknown` serializes back with the original `type` and all sibling
fields intact (lossless roundtrip).

### Thought (step)

Thoughts are now a **step** (`type: "thought"`), not a content block.

```json
{
  "type": "thought",
  "signature": "Eq0JCqoJAXLI2nyuo7yupoglxIQxc5h0...",
  "summary": [{"type": "text", "text": "Considering the options..."}]
}
```

| Rust Field | Wire Name | Notes |
|------------|-----------|-------|
| `signature` | `signature` | Cryptographic signature for verification, NOT readable text |
| `summary` | `summary` | Optional content blocks summarizing the reasoning |

Use `response.has_thoughts()`, `response.thought_signatures()`, and
`response.thought_summaries()`.

**Status**: Pending live verification (2026-05-20 revision). The signature-not-text
behavior was verified 2026-01-09 on the pre-revision API; the `summary` field is new.

### StepDelta (SSE `step.delta` payload)

Deltas incrementally build the step announced by the matching `step.start`
event. Tagged by `"type"` (snake_case of the variant name), with **two
exceptions** where the wire tag differs from the variant name:

| Wire `type` | Rust Variant | Notes |
|-------------|--------------|-------|
| `text` | `StepDelta::Text` | Text fragment |
| `image` / `audio` / `video` / `document` | media variants | Same field shapes as `Content` (audio also accepts legacy `rate`) |
| `thought_summary` | `StepDelta::ThoughtSummary` | `{"content": Content}` |
| `thought_signature` | `StepDelta::ThoughtSignature` | `{"signature": "..."}` |
| `text_annotation_delta` | `StepDelta::TextAnnotation` | **Tag differs from variant name.** `{"annotations": [Annotation]}` |
| `arguments_delta` | `StepDelta::ArgumentsDelta` | `{"arguments": "<raw JSON fragment>"}` — function-call arguments stream as string fragments; concatenate and parse at `step.stop` |
| `function_result` | `StepDelta::FunctionResult` | Same shape as the step |
| `code_execution_call` / `code_execution_result` | code execution variants | Call delta carries flattened `language`/`code` |
| `url_context_call` / `url_context_result` | URL context variants | |
| `google_search_call` / `google_search_result` | Google Search variants | |
| `mcp_server_tool_call` / `mcp_server_tool_result` | MCP variants | |
| `file_search_call` / `file_search_result` | file search variants | |
| `google_maps_call` / `google_maps_result` | Google Maps variants | |
| (anything else) | `StepDelta::Unknown { delta_type, data }` | Preserved |

Helpers: `as_text()`, `as_arguments_delta()`, `is_unknown()`, `unknown_delta_type()`, `unknown_data()`.

The HTTP layer accumulates `step.start`/`step.delta`/`step.stop` into the final
`Completed` response's `steps` — including assembling `arguments_delta`
fragments into `Step::FunctionCall.arguments` — so `response.function_calls()`
works after streaming.

**Status**: Pending live verification (2026-05-20 revision).

Example SSE payloads (new event names — `interaction.start`, `content.start/delta/stop`,
and `interaction.complete` are **gone** from the protocol):

```json
{"event_type": "interaction.created", "interaction": {"id": "...", "status": "in_progress"}}
{"event_type": "step.start", "index": 0, "step": {"type": "model_output", "content": []}}
{"event_type": "step.delta", "index": 0, "delta": {"type": "text", "text": "Hello"}, "event_id": "..."}
{"event_type": "step.stop", "index": 0, "usage": {"total_tokens": 42}, "step_usage": {"total_tokens": 12}}
{"event_type": "interaction.completed", "interaction": {"id": "...", "status": "completed", "steps": [...]}}
{"event_type": "error", "error": {"message": "...", "code": "..."}}
```

The corresponding `StreamChunk` variants are `Created`, `StatusUpdate`,
`StepStart`, `StepDelta`, `StepStop`, `Completed`, `Error`, and
`Unknown { chunk_type, data }`.

### Annotation (citation union)

Old revision: a single struct `{start_index, end_index, source}`. Revision
2026-05-20: a discriminated union tagged by `"type"`.

```json
{
  "type": "url_citation",
  "url": "https://example.com",
  "title": "Example Domain",
  "start_index": 0,
  "end_index": 42
}
```

| Wire `type` | Rust Variant | Extra fields |
|-------------|--------------|--------------|
| `url_citation` | `Annotation::UrlCitation` | `url`, `title` |
| `file_citation` | `Annotation::FileCitation` | `document_uri`, `file_name`, `source`, `custom_metadata`, `page_number`, `media_id` |
| `place_citation` | `Annotation::PlaceCitation` | `place_id`, `name`, `url`, `review_snippets: [ReviewSnippet]` |
| (anything else) | `Annotation::Unknown { annotation_type, data }` | Preserved |

All citation variants carry `start_index`/`end_index` (UTF-8 byte offsets into
the annotated text). `ReviewSnippet { title, url, review_id }` (all optional)
is exported. Helpers: `start_index()`, `end_index()`, `source()`,
`extract_span(&text)`, plus the standard Unknown trio.

**Status**: Pending live verification (2026-05-20 revision).

### FunctionResultPayload (untagged union)

The `result` field of `function_result` and `mcp_server_tool_result` steps is a
union of three wire shapes. It serializes **untagged** — exactly as the inner
value:

| Rust Variant | Serializes as | Deserialized when |
|--------------|---------------|-------------------|
| `Text(String)` | JSON string | Wire value is a string |
| `Contents(Vec<Content>)` | JSON array of content blocks | Wire value is a non-empty array where **every** element is an object with a string `"type"` field |
| `Json(Value)` | The raw JSON value | Anything else (objects, numbers, booleans, mixed arrays) — doubles as the Evergreen catch-all |

```rust
use genai_rs::FunctionResultPayload;

let text = FunctionResultPayload::from("done");
assert_eq!(serde_json::to_string(&text).unwrap(), "\"done\"");

let json = FunctionResultPayload::from(serde_json::json!({"temp": 22}));
assert_eq!(serde_json::to_string(&json).unwrap(), "{\"temp\":22}");
```

`From` impls exist for `serde_json::Value` (strings become `Text`, everything
else `Json`), `&str`, `String`, and `Vec<Content>`. Helpers: `as_text()`,
`as_json()`, `as_contents()`, `to_value()`.

**Status**: Pending live verification (2026-05-20 revision).

### ToolChoice (generation_config)

`generation_config.tool_choice` is a union: a plain mode string or an object
restricting the model to a named tool set.

```json
{ "generation_config": { "tool_choice": "any" } }
```

```json
{
  "generation_config": {
    "tool_choice": {
      "allowed_tools": { "mode": "any", "tools": ["get_weather"] }
    }
  }
}
```

| Rust Variant | Wire shape |
|--------------|-----------|
| `ToolChoice::Mode(FunctionCallingMode)` | Plain string: `"auto"` / `"any"` / `"none"` / `"validated"` |
| `ToolChoice::AllowedTools(AllowedTools)` | `{"allowed_tools": {"mode"?: ..., "tools": [...]}}` |
| `ToolChoice::Unknown { choice_type, data }` | Unrecognized strings/shapes, preserved for roundtrip |

```rust
use genai_rs::{FunctionCallingMode, ToolChoice};

let choice = ToolChoice::Mode(FunctionCallingMode::Any);
assert_eq!(serde_json::to_string(&choice).unwrap(), "\"any\"");

let choice = ToolChoice::allowed_tools(
    Some(FunctionCallingMode::Any),
    vec!["get_weather".to_string()],
);
let wire = serde_json::to_value(&choice).unwrap();
assert_eq!(wire["allowed_tools"]["mode"], "any");
```

`AllowedTools { mode, tools }` is also the element type of the MCP server
tool's `allowed_tools` list. Unknown mode **strings** delegate to
`FunctionCallingMode::Unknown` (wrapped in `ToolChoice::Mode`); unknown
**shapes** land in `ToolChoice::Unknown`.

**Status**: Pending live verification (2026-05-20 revision).

### FunctionCallingMode (generation_config)

Used as the string form of `generation_config.tool_choice` and as
`AllowedTools.mode`.

| Rust Enum | Wire Value | Legacy (accepted on deserialize) |
|-----------|------------|----------------------------------|
| `FunctionCallingMode::Auto` | `"auto"` | `"AUTO"` |
| `FunctionCallingMode::Any` | `"any"` | `"ANY"` |
| `FunctionCallingMode::None` | `"none"` | `"NONE"` |
| `FunctionCallingMode::Validated` | `"validated"` | `"VALIDATED"` |

**Changed in 2026-05-20**: wire values are now **lowercase** (previously
SCREAMING_CASE). Serialization always emits lowercase; deserialization accepts
both. **Pending live verification (2026-05-20 revision).**

### ServiceTier (request)

New in revision 2026-05-20: `service_tier` on the interaction request
(builder: `with_service_tier(ServiceTier::Flex)`).

| Rust Enum | Wire Value |
|-----------|------------|
| `ServiceTier::Flex` | `"flex"` |
| `ServiceTier::Standard` | `"standard"` |
| `ServiceTier::Priority` | `"priority"` |
| `ServiceTier::Unknown { tier_type, data }` | preserved |

**Status**: Response side verified live 2026-07 (Api-Revision 2026-05-20): every
interaction response echoes the effective tier as `service_tier: "standard"`,
alongside an `object: "interaction"` resource discriminator — both now modeled
on `InteractionResponse` (`service_tier`, `object`). The request-side values
(`flex`/`priority`) are still pending live verification.

### ThinkingSummaries (agent_config)

Used in `agent_config.thinking_summaries` for Deep Research agent.

```json
{
  "agent_config": {
    "type": "deep-research",
    "thinking_summaries": "THINKING_SUMMARIES_AUTO"
  }
}
```

| Rust Enum | Wire Value | Doc Claims (wrong) |
|-----------|------------|-------------------|
| `ThinkingSummaries::Auto` | `"THINKING_SUMMARIES_AUTO"` | `"auto"` |
| `ThinkingSummaries::None` | `"THINKING_SUMMARIES_NONE"` | `"none"` |

**Discovered**: 2026-01-04 - API returned `"unknown enum value: 'auto'"` until we tested the fully-qualified format.

### ThinkingLevel (generation_config)

Used in `generation_config.thinking_level`.

```json
{
  "generation_config": {
    "thinking_level": "low"
  }
}
```

| Rust Enum | Wire Value |
|-----------|------------|
| `ThinkingLevel::Minimal` | `"minimal"` |
| `ThinkingLevel::Low` | `"low"` |
| `ThinkingLevel::Medium` | `"medium"` |
| `ThinkingLevel::High` | `"high"` |

### InteractionStatus (response)

Returned in API responses - we only deserialize, never serialize (to the API;
local serialization exists for fixtures/roundtrip). Implements `Default`
(`InProgress`) since revision 2026-05-20.

| Rust Enum | Wire Value | Notes |
|-----------|------------|-------|
| `InteractionStatus::Completed` | `"completed"` | |
| `InteractionStatus::InProgress` | `"in_progress"` | `Default` |
| `InteractionStatus::RequiresAction` | `"requires_action"` | |
| `InteractionStatus::Failed` | `"failed"` | |
| `InteractionStatus::Cancelled` | `"cancelled"` | |
| `InteractionStatus::Incomplete` | `"incomplete"` | SDK-sourced, not yet in official API docs |
| `InteractionStatus::BudgetExceeded` | `"budget_exceeded"` | New in 2026-05-20; pending live verification (2026-05-20 revision) |

### Resolution (content)

Used in image and video content for quality vs. token cost trade-off.

```json
{
  "input": [{
    "type": "image",
    "data": "base64...",
    "mime_type": "image/png",
    "resolution": "low"
  }]
}
```

| Rust Enum | Wire Value |
|-----------|------------|
| `Resolution::Low` | `"low"` |
| `Resolution::Medium` | `"medium"` |
| `Resolution::High` | `"high"` |
| `Resolution::UltraHigh` | `"ultra_high"` |

**Verified**: 2026-01-05 - Tested with `LOUD_WIRE=1 cargo run --example multimodal_image`.

### Tool::FileSearch (request)

Used to enable semantic document retrieval from file search stores.

```json
{
  "tools": [{
    "type": "file_search",
    "file_search_store_names": ["stores/my-store-123"],
    "top_k": 10,
    "metadata_filter": "category = 'technical'"
  }]
}
```

| Rust Field | Wire Name | Required | Notes |
|------------|-----------|----------|-------|
| `store_names` | `file_search_store_names` | Yes | Array of store identifiers |
| `top_k` | `top_k` | No | Number of results to return |
| `metadata_filter` | `metadata_filter` | No | Filter expression |

**Note**: The RFC proposed `file_ids` but the actual API uses `file_search_store_names` (stores, not individual files).

**Verified**: 2026-01-05 - Request format tested with `LOUD_WIRE=1 cargo run --example file_search`.

### FileSearchCall / FileSearchResult (steps)

Returned when the model retrieves documents from file search stores. Revision
2026-05-20 adds the paired `file_search_call` step.

```json
{ "type": "file_search_call", "id": "call_abc123" }
```

```json
{
  "type": "file_search_result",
  "call_id": "call_abc123",
  "result": [
    {
      "title": "Document.pdf",
      "text": "Relevant content from the document...",
      "file_search_store": "stores/my-store-123"
    }
  ]
}
```

| Rust Field | Wire Name | Notes |
|------------|-----------|-------|
| `call_id` | `call_id` | snake_case in JSON |
| `result` | `result` | Array of FileSearchResultItem |
| `result[].title` | `title` | Document title |
| `result[].text` | `text` | Retrieved text snippet |
| `result[].store` | `file_search_store` | snake_case in JSON |

**Status**: Item shape verified 2026-01 pre-revision (as `Content`); step form pending live verification (2026-05-20 revision).

### GoogleSearchCall / GoogleSearchResult (steps)

Wire types: `"google_search_call"` / `"google_search_result"`.

```json
{
  "type": "google_search_call",
  "id": "qs19a0jm",
  "arguments": { "queries": ["rust language news"] },
  "search_type": "web_search",
  "signature": "ErIE..."
}
```

`GoogleSearchResultItem` is snake_case:
`{"title": "...", "url": "...", "rendered_content": "...", "search_suggestions"?: ...}` —
`search_suggestions` is new in 2026-05-20.

**Status**: Step form verified live 2026-07 (Api-Revision 2026-05-20) via `LOUD_WIRE=1`:
the `google_search_call` step carried `id`, nested `arguments.queries`,
`search_type: "web_search"`, and `signature`. The paired `google_search_result` items
on the live wire carried **only** `search_suggestions` (an HTML `<style>...` rendering
payload as a string) — no `title`/`url`/`rendered_content`. `title`/`url` deserialize
to empty strings in that case and are skipped on re-serialize. Grounding citations
arrived as `url_citation` annotations on the `model_output` text
(`{"type": "url_citation", "url": ..., "title": ..., "start_index": ..., "end_index": ...}`),
and `usage.grounding_tool_count` reported `[{"type": "google_search", "count": 1}]`.

### GoogleMapsCall (step)

Wire type: `"google_maps_call"`

| Rust field | Wire field | Notes |
|-----------|-----------|-------|
| `id` | `id` | Unique call identifier |
| `queries` | `arguments.queries` | Nested inside `arguments` object |
| `signature` | `signature` | Optional, opaque backend validation |

Example wire format:
```json
{
  "type": "google_maps_call",
  "id": "qs19a0jm",
  "arguments": { "queries": ["coffee shops near Times Square"] },
  "signature": "ErIE..."
}
```

**Status**: Verified pre-revision via `LOUD_WIRE=1` (as `Content`); step form pending live verification (2026-05-20 revision).

### GoogleMapsResult (step)

Wire type: `"google_maps_result"`

| Rust field | Wire field | Notes |
|-----------|-----------|-------|
| `call_id` | `call_id` | Maps call ID |
| `result` | `result` | Array of `GoogleMapsResultItem` |
| `signature` | `signature` | Optional, opaque backend validation |

Each `GoogleMapsResultItem` contains:
- `places`: Optional array of `Place` objects (with `name`, `formatted_address`, `place_id`, `lat`, `lng`, plus `url` and `review_snippets` added in 2026-05-20)
- `widget_context_token`: Optional string for widget rendering

**Status**: Verified pre-revision via `LOUD_WIRE=1` (as `Content`); step form and new `Place` fields pending live verification (2026-05-20 revision).

### Computer Use (tool)

**Status**: Pending live verification (2026-05-20 revision) — wire format derived
from the [Interactions API docs](https://ai.google.dev/static/api/interactions.md.txt).

Tool request format:
```json
{
  "tools": [{
    "type": "computer_use",
    "environment": "browser",
    "excluded_predefined_functions": ["submit_form", "download"],
    "enable_prompt_injection_detection": true,
    "disabled_safety_policies": ["financial_transactions"]
  }]
}
```

| Rust Field | Wire Name | Notes |
|------------|-----------|-------|
| `environment` | `environment` | Known values: `"browser"`, `"mobile"`, `"desktop"` |
| `excluded_predefined_functions` | `excluded_predefined_functions` | **Changed**: snake_case (legacy `excludedPredefinedFunctions` accepted on deserialize) |
| `enable_prompt_injection_detection` | `enable_prompt_injection_detection` | Optional bool |
| `disabled_safety_policies` | `disabled_safety_policies` | Known values: `financial_transactions`, `sensitive_data_modification`, `communication_tool`, `account_creation`, `data_modification`, `user_consent_management`, `legal_terms_and_agreements` |

**Important**: The old `computer_use_call` / `computer_use_result` content types
are **gone**. Computer-use actions flow through plain `function_call` /
`function_result` steps (predefined function names like `navigate`, `click_at`).

**TODO**: Verify with `LOUD_WIRE=1 cargo run --example computer_use` when API access is available.

### SpeechConfig (generation_config)

Used in `generation_config.speech_config` for text-to-speech audio output.
Per the 2026-05-20 spec the wire format is a **list** of speaker
configurations — one entry for single-voice TTS, multiple entries (each with
a distinct `speaker` matching the prompt) for multi-speaker TTS:

```json
{
  "model": "gemini-2.5-pro-preview-tts",
  "input": "Alice: Hi Bob!\nBob: Hey Alice!",
  "generation_config": {
    "response_modalities": ["audio"],
    "speech_config": [
      {"voice": "Kore", "language": "en-US", "speaker": "Alice"},
      {"voice": "Puck", "language": "en-US", "speaker": "Bob"}
    ]
  }
}
```

| Rust Field | Wire Name | Required | Notes |
|------------|-----------|----------|-------|
| `voice` | `voice` | No* | Voice name (e.g., "Kore", "Puck", "Charon") |
| `language` | `language` | Yes** | Language code (e.g., "en-US", "es-ES") |
| `speaker` | `speaker` | No | Must match a speaker name in the prompt for multi-speaker TTS |

*Voice defaults to a system voice if not specified.
**Language is required by the API when voice is specified.

**Important**: The Google docs suggest a nested structure (`voiceConfig.prebuiltVoiceConfig.voiceName`) but **that format returns 400 error**. Only the flat structure shown above works with the Interactions API.

**Status**: ✅ Verified live 2026-07. The two-speaker list form above was
accepted verbatim and returned a single combined `audio/l16` content block
(`sample_rate: 24000`) covering both voices. The API does not echo
`speech_config` (or any `generation_config`) back on reads — the
`include_input=true` GET parameter was observed to be a no-op — so the echo
shape (list vs. single object) is unobservable.

**Verified**: 2026-01-10 (flat single-object form) - Tested both formats in `test_speech_config_nested_format_fails_flat_succeeds`. Nested format fails with `no such field: 'voiceConfig'`. The **list** form is from the 2026-05-20 spec and is pending live verification; the legacy single-object form is still accepted on deserialize.

### Audio Response (TTS output)

TTS responses return audio content (inside a `model_output` step) with a
specific MIME type:

```json
{
  "steps": [{
    "type": "model_output",
    "content": [{
      "type": "audio",
      "data": "base64-encoded-pcm-data...",
      "mime_type": "audio/L16;codec=pcm;rate=24000"
    }]
  }]
}
```

| MIME Type | Format | Notes |
|-----------|--------|-------|
| `audio/L16;codec=pcm;rate=24000` | Raw PCM | 16-bit linear PCM at 24kHz |

The `AudioInfo::extension()` method maps this to `"pcm"` for file saving.
Audio content blocks also carry optional `sample_rate` and `channels` fields
since revision 2026-05-20.

**Status**: MIME type verified 2026-01-07 pre-revision; steps envelope pending live verification (2026-05-20 revision).

### UrlContextCall (step)

Emitted when the model requests URL content for context.

```json
{
  "type": "url_context_call",
  "id": "fpo8xd3s",
  "arguments": {
    "urls": ["https://example.com", "https://example.org"]
  }
}
```

| Rust Field | Wire Name | Notes |
|------------|-----------|-------|
| `id` | `id` | Call identifier for matching results |
| `urls` | `arguments.urls` | Array of URLs, nested inside `arguments` |

**Note**: The `urls` are nested inside an `arguments` object in the wire format. The library extracts them to a flat `urls: Vec<String>` field for convenience.

**Status**: Shape verified 2026-01-09 pre-revision (as `Content`); step form pending live verification (2026-05-20 revision).

### UrlContextResult (step)

Returned with the results of URL fetching.

```json
{
  "type": "url_context_result",
  "call_id": "fpo8xd3s",
  "result": [
    {
      "url": "https://example.com",
      "status": "success"
    },
    {
      "url": "https://example.org",
      "status": "error"
    }
  ]
}
```

| Rust Field | Wire Name | Notes |
|------------|-----------|-------|
| `call_id` | `call_id` | Matches the corresponding UrlContextCall |
| `result` | `result` | Array of UrlContextResultItem |
| `result[].url` | `url` | The URL that was fetched |
| `result[].status` | `status` | "success", "error", or "unsafe" |

**Note**: Each item in `result` is a `UrlContextResultItem` with helper methods `is_success()`, `is_error()`, and `is_unsafe()`. The old
`url_context_metadata` response block and its `UrlRetrievalStatus` enum were removed in revision 2026-05-20.

**Status**: Shape verified 2026-01-09 pre-revision (as `Content`); step form pending live verification (2026-05-20 revision).

### CodeExecutionResult (step)

Returned when code execution completes. Uses simple `is_error` boolean and `result` string fields.

```json
{
  "type": "code_execution_result",
  "call_id": "exec_123",
  "is_error": false,
  "result": "Hello, World!\n"
}
```

| Rust Field | Wire Name | Type | Notes |
|------------|-----------|------|-------|
| `call_id` | `call_id` | `String` | Matches the CodeExecutionCall id (required since 2026-05-20) |
| `is_error` | `is_error` | `bool` | `false` = success, `true` = error |
| `result` | `result` | `String` | Output text (stdout) or error message |
| `signature` | `signature` | `Option<String>` | Optional opaque signature (new in 2026-05-20) |

**Important**: The official API documentation mentions `outcome` enum with values like `OUTCOME_OK`, but the **actual wire format** uses `is_error: bool` and `result: String`. This was discovered via `LOUD_WIRE=1` testing.

**Status**: `is_error`/`result` shape verified 2026-01-12 pre-revision (as `Content`); step form and `signature` field pending live verification (2026-05-20 revision).

### CodeExecutionLanguage (step field)

Specifies the programming language inside `code_execution_call.arguments`.

```json
{
  "type": "code_execution_call",
  "id": "exec_123",
  "arguments": { "language": "python", "code": "print('Hello')" }
}
```

| Rust Enum | Wire Value | Legacy (accepted on deserialize) |
|-----------|------------|----------------------------------|
| `CodeExecutionLanguage::Python` | `"python"` | `"PYTHON"` |
| `CodeExecutionLanguage::Unknown { ... }` | `"*"` | Future languages preserved |

**Changed in 2026-05-20**: wire value is now **lowercase** `"python"`
(previously `"PYTHON"`). Serialization always emits lowercase; deserialization
accepts both. `Display` prints `"python"`.

Helper methods: `is_unknown()`, `unknown_language_type()`, `unknown_data()`

**Status**: Pending live verification (2026-05-20 revision).

### SearchType (Google Search tool)

Configures `Tool::GoogleSearch.search_types` and appears on
`google_search_call.search_type`.

| Rust Enum | Wire Value | Notes |
|-----------|------------|-------|
| `SearchType::WebSearch` | `"web_search"` | Verified 2026-01 pre-revision |
| `SearchType::ImageSearch` | `"image_search"` | Model-restricted |
| `SearchType::EnterpriseWebSearch` | `"enterprise_web_search"` | New in 2026-05-20; pending live verification (2026-05-20 revision) |
| `SearchType::Unknown { search_type, data }` | preserved | |

### Tool::Retrieval (request)

Grounds responses in external retrieval backends. `retrieval_types` selects
the backends; per-backend configs supply their parameters.

```json
{
  "type": "retrieval",
  "retrieval_types": ["vertex_ai_search", "rag_store"],
  "vertex_ai_search_config": {"engine": "projects/p/.../engines/e", "datastores": ["ds-1"]},
  "rag_store_config": {
    "rag_resources": [{"rag_corpus": "projects/p/.../ragCorpora/c", "rag_file_ids": ["f1"]}],
    "rag_retrieval_config": {
      "top_k": 8,
      "hybrid_search": {"alpha": 0.5},
      "filter": {"vector_distance_threshold": 0.7, "metadata_filter": "category = \"tech\""},
      "ranking": {"ranking_config": "rank_service", "model_name": "ranker-v2"}
    }
  },
  "exa_ai_search_config": {"api_key": "...", "custom_config": {}},
  "parallel_ai_search_config": {"api_key": "...", "custom_config": {}}
}
```

Notes:
- `RetrievalType`: `vertex_ai_search` | `rag_store` | `exa_ai_search` |
  `parallel_ai_search` + `Unknown { retrieval_type, data }` with the standard
  helpers.
- `rag_store_config.similarity_top_k` / `vector_distance_threshold` are
  deprecated by the API in favor of `rag_retrieval_config`.
- The RAG filter field is `filter` on the wire (the generated Python bindings
  alias it as `filter_`).
- `ranking.ranking_config` is always the literal `"rank_service"`.
- Exa/Parallel `api_key` values are sent on the wire — treat request logs as
  sensitive.

**Status**: ⚠️ Verified live 2026-07 — **rejected on the Gemini API**. The
request parses, but the API returns 400: "The value 'retrieval' is not
supported for 'tools[0].type' on the Gemini API, it is allowed on the Gemini
Enterprise Agent Platform." (i.e. the retrieval tool is Vertex-only). The
same error enumerates the Gemini API's supported tool types: `google_maps`,
`mcp_server`, `function`, `google_search`, `file_search`, `computer_use`,
`code_execution`, `url_context`.

### Webhooks (`/v1beta/webhooks` resource + `webhook_config`)

Webhook resource (snake_case, RFC3339 timestamps):

```json
{
  "id": "webhooks/wh-123",
  "name": "my-hook",
  "uri": "https://example.com/hook",
  "subscribed_events": ["batch.succeeded", "interaction.completed", "video.generated"],
  "state": "enabled",
  "signing_secrets": [{"truncated_secret": "whsec_...abcd", "expire_time": "2026-08-01T00:00:00Z"}],
  "new_signing_secret": "whsec_full",
  "create_time": "2026-07-01T12:00:00Z",
  "update_time": "2026-07-02T12:00:00Z"
}
```

- `WebhookEvent` wire values: `batch.succeeded`, `batch.expired`,
  `batch.failed`, `interaction.requires_action`, `interaction.completed`,
  `interaction.failed`, `video.generated` (+ `Unknown`).
- `WebhookState`: `enabled`, `disabled`,
  `disabled_due_to_failed_deliveries` (+ `Unknown`). Output only.
- `new_signing_secret` is only populated on create.
- `:rotateSigningSecret` takes `{"revocation_behavior":
  "revoke_previous_secrets_after_h24" | "revoke_previous_secrets_immediately"}`
  and returns `{"secret": "..."}`; `:ping` takes/returns empty objects.
- List/update query params are snake_case: `page_size`, `page_token`,
  `update_mask`.
- Per-request routing: `webhook_config: {"uris": [...], "user_metadata": {...}}`
  on the interaction request.
- Webhook and agent endpoints send the same `Api-Revision: 2026-05-20`
  header as interactions (the generated google-genai bindings apply the
  revision header globally).

**Status**: ✅ Verified live 2026-07 (full CRUD + `:ping` +
`:rotateSigningSecret` round-trip). Live findings:

- Get/list echo exactly what create sent (`uri`, `subscribed_events`,
  `name`); `new_signing_secret` appears only on create; IDs are bare opaque
  strings (no `webhooks/` prefix observed).
- `create_time` / `update_time` were **not** returned by any endpoint (the
  crate keeps them as optional fields).
- `:ping` accepts an empty JSON body (`{}` — what this crate sends) *and* a
  bodiless POST; returns `{}` even for unreachable URIs.
- `update_mask` on PATCH is **not required and observed to be ignored** —
  the PATCH applies exactly the fields present in the body (fields outside
  a supplied mask still updated). Unknown query params are silently
  ignored, so `update_mask` vs. `updateMask` casing cannot be
  distinguished; body field casing IS enforced (camelCase body keys get
  "Unknown parameter ... Did you mean ...").
- `:rotateSigningSecret` returns a fresh `{"secret": ...}` each call;
  the previous secrets gain a 24h `expire_time` by default. The
  `revocation_behavior` enum is validated server-side (exactly our two
  values).
- Invalid `subscribed_events` values are rejected with an error listing
  exactly our seven `WebhookEvent` values.
- `webhook_config` on an interaction request requires `background=true`
  and is echoed back verbatim (`uris` + `user_metadata`) in the create
  response. `InteractionResponse` models the echo (`webhook_config`),
  alongside the `object`/`service_tier` response fields discovered in the
  same verification pass.

### Environment (request `environment` / agent `base_environment`)

Union: a string environment ID, or a typed remote environment object.

```json
"env-123"
```

```json
{
  "type": "remote",
  "sources": [
    {"type": "gcs", "source": "gs://bucket/data", "target": "/data"},
    {"type": "inline", "target": "/etc/config", "content": "aGVsbG8=", "encoding": "base64"},
    {"type": "repository", "source": "github.com/org/repo", "target": "/workspace"},
    {"type": "skill_registry", "source": "skills/my-skill"}
  ],
  "network": {"allowlist": [
    {"domain": "*.googleapis.com"},
    {"domain": "api.example.com", "transform": [{"Authorization": "Bearer ..."}]}
  ]}
}
```

- `network` is a union: the string `"disabled"` (all network off), an
  `{"allowlist": [...]}` object, or omitted entirely (all traffic allowed).
- The response echoes the server-assigned environment as `environment_id`,
  which can be passed back as the string form on later turns.

**Status**: ✅ Verified live 2026-07 (with `agent:
"antigravity-preview-05-2026"`, `background: true`): inline source,
`network: "disabled"`, `network: {allowlist: [{domain, transform}]}`, and
the plain string environment-ID form were all accepted; accepted typed
requests returned `environment_id`. `gcs`/`repository`/`skill_registry`
sources and agent `base_environment` were not exercised (the latter needs
agent creation, which is gated — see Agents notes in
`docs/INTERACTIONS_API_GAP.md`).

### ResponseFormat (request `response_format`)

Typed union tagged by `"type"`; the request field accepts one object or a
list (one per output modality).

```json
{"type": "text", "mime_type": "application/json", "schema": {"type": "object"}}
{"type": "audio", "mime_type": "audio/mp3", "delivery": "inline", "sample_rate": 24000, "bit_rate": 128000}
{"type": "image", "mime_type": "image/jpeg", "delivery": "uri", "aspect_ratio": "16:9", "image_size": "2K"}
{"type": "video", "delivery": "uri", "gcs_uri": "gs://bucket/out", "aspect_ratio": "9:16", "duration": "8s"}
```

- `delivery` (`ResponseDelivery`): `"inline"` | `"uri"` (+ `Unknown`).
- Known text MIME types: `application/json`, `text/plain`. Known audio MIME
  types: `audio/mp3`, `audio/ogg_opus`, `audio/l16`, `audio/wav`,
  `audio/alaw`, `audio/mulaw`. Known image MIME type: `image/jpeg`.
- The API also accepts a raw JSON-schema dict here (the pre-revision form);
  such dicts have no recognized `"type"` tag and roundtrip through
  `ResponseFormat::Unknown` with the data preserved. When *building*
  requests, a raw `serde_json::Value` passed to `with_response_format()`
  converts to the typed `text`/`application/json` form.
- `video.gcs_uri` is required on Vertex when `delivery` is `"uri"`.

**Status**: ✅ Verified live 2026-07 (single object and list forms; list
validation errors index entries as `response_format[i]`). Server-side
constraints observed on the Gemini API:

- text: schema-bearing `application/json` form works end-to-end (output
  validated against the schema).
- image: works inline; `mime_type` only accepts `image/jpeg` (the API's
  validation error lists it as the sole supported value); any `delivery`
  value → 400 "Image delivery mode is not supported."
- audio: works with `sample_rate`; any `mime_type` → 400 "Audio mime_type
  is not supported in response_format." and any `delivery` → 400 "Audio
  delivery mode is not supported." (output arrives inline as `audio/l16`).
- video: `delivery`/`duration`/`aspect_ratio` are schema-recognized, but
  `gcs_uri` → 400 "not available on the Gemini API but it is available on
  the Gemini Enterprise Agent Platform" (Vertex-only), and no
  Interactions-served model currently supports the video modality.
- `ResponseDelivery` values confirmed via the API's validation error:
  exactly `inline` | `uri`.

### VideoConfig / VideoTask (generation_config)

```json
{"generation_config": {"video_config": {"task": "text_to_video"}}}
```

`VideoTask`: `text_to_video` | `image_to_video` | `reference_to_video` |
`edit` | `extend` (+ `Unknown { task_type, data }`). Omit to let the model
pick the mode from the prompt and input media. Pair with
`response_modalities: ["video"]` and (optionally) a `video` response format.

**Status**: ✅ Verified live 2026-07 via the API's own validation error for
`generation_config.video_config.task`, which lists exactly
`text_to_video`, `image_to_video`, `reference_to_video`, `edit`, `extend` —
the previously unmodeled `extend` was added to the enum. Note: video
generation itself is not currently reachable through the Interactions API —
Veo models (e.g. `veo-3.1-generate-preview`) return 404 "Model not found"
(they are listed by `/v1beta/models` with only the legacy
`predictLongRunning` method), and Gemini models reject
`response_modalities: ["video"]`. `video_config` is accepted (ignored) on
non-video models.

### Visualization (Deep Research agent_config)

```json
{"agent_config": {"type": "deep-research", "visualization": "auto", "collaborative_planning": true, "enable_bigquery_tool": true}}
```

`Visualization`: `"off"` | `"auto"` (+ `Unknown { visualization_type, data }`).
Note the contrast with `thinking_summaries`, which uses `THINKING_SUMMARIES_*`
in agent_config; `visualization` is lowercase per the spec.

**Status**: ✅ Verified live 2026-07: the API's validation error for
`agent_config.visualization` lists exactly `off` | `auto`;
`visualization` + `collaborative_planning` were accepted on a
`deep-research-preview-04-2026` background run (cancelled after creation).
⚠️ `enable_bigquery_tool` is rejected on the Gemini API — "not available on
the Gemini API but it is available on the Gemini Enterprise Agent Platform"
(Vertex-only).

### GroundingToolCount (usage metadata)

New in revision 2026-05-20: `usage.grounding_tool_count` reports per-tool
grounding invocation counts.

```json
{
  "usage": {
    "total_tokens": 123,
    "grounding_tool_count": [
      { "type": "google_search", "count": 2 },
      { "type": "google_maps", "count": 1 }
    ]
  }
}
```

| Rust Field | Wire Name | Notes |
|------------|-----------|-------|
| `tool_type` | `type` | Plain string for Evergreen forward compatibility. Known values: `google_search`, `google_maps`, `retrieval` |
| `count` | `count` | Number of invocations |

Helper: `usage.grounding_count_for_tool("google_search")`.

**Status**: Pending live verification (2026-05-20 revision).

## Testing New Enums

When adding new enums, always test the actual wire format with `curl`:

```bash
# Test what the API actually accepts
curl -s "https://generativelanguage.googleapis.com/v1beta/interactions?key=$GEMINI_API_KEY" \
  -H "Content-Type: application/json" \
  -H "Api-Revision: 2026-05-20" \
  -d '{"model": "gemini-3-flash-preview", "input": "test", ...}'
```

Common patterns to try:
1. lowercase: `"auto"`
2. SCREAMING_CASE: `"AUTO"`
3. Fully-qualified: `"ENUM_NAME_VALUE"` (e.g., `"THINKING_SUMMARIES_AUTO"`)

Revision 2026-05-20 standardized most enum wire values on **lowercase/snake_case**
(`FunctionCallingMode`, `CodeExecutionLanguage`, `ServiceTier`, `InteractionStatus`,
`SearchType`), but always verify with `LOUD_WIRE=1` before assuming.

## Evergreen Pattern

All enums implement the Evergreen pattern with an `Unknown` variant that preserves unrecognized values:

```rust,ignore
#[non_exhaustive]
pub enum ThinkingSummaries {
    Auto,
    None,
    Unknown {
        summaries_type: String,
        data: serde_json::Value,
    },
}
```

This ensures forward compatibility when Google adds new enum values.

### Structs and `#[non_exhaustive]`

Response structs (e.g., `AutoFunctionResult`) also use `#[non_exhaustive]` so we can add fields without breaking user code. This has a trade-off:

**Users cannot construct these types directly** (no struct literal syntax outside the crate). This is intentional:
- Response types represent API responses, not user-constructed data
- Mocking them in unit tests would give false confidence
- We can add fields (like `executions` was added to `AutoFunctionResult`) without breaking changes

**For testing**, users should:
1. Use integration tests with real API calls (recommended)
2. Mock at the HTTP layer, not the response type layer
3. Test their own logic separately from API response handling

Note: since revision 2026-05-20, `InteractionResponse` derives `Default` and
uses `#[serde(default)]`, so test fixtures can be built with
`InteractionResponse { status: InteractionStatus::Completed, steps: vec![...], ..Default::default() }`.

If a `test-support` feature for constructing mock instances becomes commonly requested, we'll consider adding it.

## Antigravity Harness Protocol (feature `antigravity`)

The `genai_rs::antigravity::protocol` module speaks the localharness
proto-JSON protocol (see `docs/ANTIGRAVITY.md`). Wire formats were verified
against the descriptor set and a live harness from the
`google-antigravity` 0.1.5 wheel (`LOUD_WIRE=1` on a real session):

- Field names are **camelCase**; enums are **SCREAMING_SNAKE_CASE** strings.
- 64-bit integers (`seqNum`, token counts) arrive as JSON **strings**; the
  crate accepts both strings and numbers.

Enums with Unknown variants (same pattern and helper methods as above):

| Type | Location | Context Field | Wire Values |
|------|----------|---------------|-------------|
| `StepState` | src/antigravity/protocol.rs | `state_type` | `STATE_ACTIVE`, `STATE_DONE`, `STATE_WAITING_FOR_USER`, `STATE_ERROR` |
| `StepSource` | src/antigravity/protocol.rs | `source_type` | `SOURCE_SYSTEM`, `SOURCE_USER`, `SOURCE_MODEL` |
| `StepTarget` | src/antigravity/protocol.rs | `target_type` | `TARGET_USER`, `TARGET_MODEL`, `TARGET_ENVIRONMENT` |
| `TrajectoryState` | src/antigravity/protocol.rs | `state_type` | `STATE_RUNNING`, `STATE_IDLE`, `STATE_CANCELLED` |
| `ModelType` | src/antigravity/protocol.rs | `model_type` | `MODEL_TYPE_TEXT`, `MODEL_TYPE_IMAGE` |
| `LifecycleHook` | src/antigravity/protocol.rs | `hook_type` | `LIFECYCLE_HOOK_PRE_TOOL`, `LIFECYCLE_HOOK_POST_TOOL`, ... |
| `HookDecision` | src/antigravity/protocol.rs | `decision_type` | `ALLOW`, `DENY` |
| `LineAction` | src/antigravity/protocol.rs | `action_type` | `LINE_ACTION_INSERT`, `LINE_ACTION_DELETE`, `LINE_ACTION_NONE` |

Envelope oneofs also carry Unknown variants (`event_type` + `data`):
`InputEvent::Unknown` and `OutputPayload::Unknown`. Harness-emitted structs
(`StepUpdate`, `ToolCall`, `UsageMetadata`, action submessages, ...)
preserve unrecognized fields in a flattened `extra` map for roundtrip.

Note: `strict-unknown` does not apply to the Antigravity protocol — the
harness protocol is explicitly internal/unstable, so soft-typing is always
on there.
