# Enum Wire Formats & Unknown Variants

The verified wire formats for this crate's enums and unions, and the
Evergreen `Unknown` variant each one carries. The official docs are sometimes
wrong, so this catalog records what the live API actually sent or accepted,
and when.

**Revision.** Everything here is Interactions API revision **2026-05-20**.
Every request sends `Api-Revision: 2026-05-20`. The server currently ignores
the header and serves 2026-05-20 whatever value it carries.

**Status markers.** "Verified live *date*" means the value was observed on
`generativelanguage.googleapis.com`, usually with `LOUD_WIRE=1`. "Pending
live verification" means the value comes from the spec and unit-tested
serialization only. "Pre-revision" means verified before 2026-05-20, when
the shape was a `Content` block rather than a step.

## Types with Unknown variants

| # | Type | Context field | Notes |
|---|------|---------------|-------|
| 1 | `Content` | `content_type` | Media only: text/image/audio/video/document |
| 2 | `Step` | `step_type` | Interaction steps |
| 3 | `StepDelta` | `delta_type` | `step.delta` SSE payloads |
| 4 | `Annotation` | `annotation_type` | Citations (url/file/place), `speech_metadata`, `word_info` |
| 5 | `Resolution` | `resolution_type` | Image/video quality |
| 6 | `StreamChunk` | `chunk_type` | Low-level SSE chunks |
| 7 | `AutoFunctionStreamChunk` | `chunk_type` | Auto-function streaming |
| 8 | `FileState` | `state_type` | Files API states |
| 9 | `Tool` | `tool_type` | Tool types |
| 10 | `FunctionCallingMode` | `mode_type` | auto/any/none/validated |
| 11 | `ToolChoice` | `choice_type` | Mode string or `allowed_tools` object |
| 12 | `ThinkingLevel` | `level_type` | minimal/low/medium/high |
| 13 | `ThinkingSummaries` | `summaries_type` | auto/none |
| 14 | `ServiceTier` | `tier_type` | flex/standard/priority |
| 15 | `InteractionStatus` | `status_type` | Response status |
| 16 | `CodeExecutionLanguage` | `language_type` | python |
| 17 | `ImageAspectRatio` | `ratio_type` | 14 aspect ratios |
| 18 | `ImageSize` | `size_type` | 512/1K/2K/4K |
| 19 | `SearchType` | `search_type` | web_search/image_search/enterprise_web_search |
| 20 | `RetrievalType` | `retrieval_type` | vertex_ai_search/rag_store/exa_ai_search/parallel_ai_search |
| 21 | `WebhookEvent` | `event_type` | batch.*/interaction.*/video.generated |
| 22 | `WebhookState` | `state_type` | enabled/disabled/disabled_due_to_failed_deliveries |
| 23 | `RevocationBehavior` | `behavior_type` | Signing-secret rotation |
| 24 | `SourceType` | `source_type` | gcs/inline/repository/skill_registry |
| 25 | `NetworkConfig` | `network_type` | `"disabled"` or `{allowlist}` |
| 26 | `EnvironmentSpec` | `environment_type` | Environment id string or `{type: "remote"}` |
| 27 | `ResponseDelivery` | `delivery_type` | inline/uri |
| 28 | `ResponseFormat` | `format_type` | text/audio/image/video union |
| 29 | `VideoTask` | `task_type` | text_to_video/image_to_video/reference_to_video/edit/extend |
| 30 | `Visualization` | `visualization_type` | off/auto |
| 31 | `HarmCategory` | `category_type` | Ten harm categories (Vertex-only parameter) |
| 32 | `SafetyThreshold` | `threshold_type` | Block thresholds (Vertex-only parameter) |
| 33 | `SafetyMethod` | `method_type` | severity/probability (Vertex-only parameter) |
| 34 | `EnvironmentStatus` | `status_type` | active/expired |
| 35 | `TriggerStatus` | `status_type` | active/paused/error |
| 36 | `TriggerExecutionStatus` | `status_type` | Execution outcomes |
| 37 | `VideoProcessing` | `processing_type` | Mode string or `{type: "static", ...}` |
| 38 | `DocumentState` | `state_type` | File Search document indexing |
| 39 | `VoiceType` | `voice_type` | prebuilt/prompted/replicated |
| 40 | `VoicePitch` | `pitch_type` | low/medium/high |
| 41 | `CredentialType` | `credential_type` | bearer_token/environment_variable/oauth2 |
| 42 | `CredentialStatus` | `status_type` | active/revoked |
| 43 | `InjectionLocation` | `location_type` | header/query/body |
| 44 | `EnvironmentFileType` | `file_type` | FILE/DIRECTORY (uppercase on the wire) |
| 45 | `VideoResolution` | `resolution_type` | 360p/720p/1080p/4k |
| 46 | `TranscriptionMode` | `mode_type` | smart/verbatim, string or tagged object |

The Antigravity harness enums are listed in their
[own section](#antigravity-harness-protocol-feature-antigravity).

**Removed in revision 2026-05-20**: `UrlRetrievalStatus`, `GroundingMetadata`,
`UrlContextMetadata`, `Turn`, and all tool-related `Content` variants
(`Thought`, `FunctionCall`, `FunctionResult`, `CodeExecutionCall/Result`,
`GoogleSearchCall/Result`, `UrlContextCall/Result`, `FileSearchResult`,
`GoogleMapsCall/Result`, `ComputerUseCall/Result`). Tool activity is now a
`Step`, and computer-use actions arrive as plain `function_call` steps.

### The Unknown variant pattern

```rust,ignore
Unknown {
    <context>_type: String,   // the unrecognized wire value
    data: serde_json::Value,  // full JSON, re-serialized unchanged
}
```

Every type above also has `is_unknown()`, `unknown_<context>_type()` and
`unknown_data()`. With the `strict-unknown` feature, unknown `Content` and
`Step` types and unknown values of any string enum (all generated by
`wire_enum!`) fail to deserialize instead; other tagged unions (`Tool`,
`StepDelta`, `StreamChunk`, ...) are unaffected. The CI `test-strict-unknown`
job runs this configuration.

## Quick reference

| Type | Wire format | Example | Status and notes |
|------|-------------|---------|------------------|
| `InteractionInput` | string, `[Step]`, `[Content]` or `Content` | `"hi"` / `[{"type": "user_input", "content": [...]}]` | Requests send `Content` input as one `user_input` step; see [InteractionInput](#interactioninput-request-input). Verified live 2026-08-16 |
| `Step` | tagged by `"type"`, snake_case | `"user_input"`, `"model_output"`, `"function_call"` | Core shapes verified live 2026-07; see [Step](#step) |
| `StepDelta` | tagged by `"type"` | `"text"`, `"arguments_delta"`, `"text_annotation_delta"` | Two tags differ from the variant names. Pending live verification |
| `Annotation` | tagged by `"type"` | `"url_citation"`, `"speech_metadata"` | `url_citation` observed live 2026-07; `speech_metadata` verified 2026-09-24 |
| `FunctionResultPayload` | untagged | `"ok"` / `{...}` / `[{"type": "text", ...}]` | String, JSON, or content-block list |
| `ToolChoice` | string or object | `"any"` / `{"allowed_tools": {...}}` | Pending live verification |
| `FunctionCallingMode` | lowercase | `"auto"`, `"any"`, `"none"`, `"validated"` | Uppercase spellings (pre-revision) now deserialize to `Unknown`. Pending live verification |
| `CodeExecutionLanguage` | lowercase | `"python"` | `"PYTHON"` (pre-revision) now deserializes to `Unknown`. Pending live verification |
| `ServiceTier` | lowercase | `"flex"`, `"standard"`, `"priority"` | Response echo `"standard"` verified live 2026-07 |
| `HarmCategory` | snake_case | `"hate_speech"`, `"jailbreak"` | `safety_settings` is rejected by the Gemini API as Vertex-only (live 2026-08-08) |
| `SafetyThreshold` | snake_case | `"block_only_high"`, `"off"` | Same Vertex-only constraint |
| `SafetyMethod` | lowercase | `"severity"`, `"probability"` | Same Vertex-only constraint |
| `EnvironmentStatus` | lowercase | `"active"`, `"expired"` | `"active"` verified live 2026-08-08 |
| `TriggerStatus` | lowercase | `"active"`, `"paused"`, `"error"` | From the SDK spec; creation is agent-gated, so pending live verification |
| `TriggerExecutionStatus` | snake_case | `"in_progress"`, `"timed_out"` | From the SDK spec; pending live verification |
| `InteractionStatus` | snake_case | `"in_progress"`, `"requires_action"`, `"budget_exceeded"` | `budget_exceeded` pending live verification. `Default` is `InProgress` |
| `SearchType` | snake_case | `"web_search"`, `"image_search"`, `"enterprise_web_search"` | `enterprise_web_search` pending live verification |
| `GroundingToolCount` | `{"type": ..., "count": n}` | `{"type": "google_search", "count": 1}` | In `usage.grounding_tool_count`; observed live 2026-07 |
| `ThinkingSummaries` | lowercase | `"auto"`, `"none"` | Sent lowercase in both contexts; `THINKING_SUMMARIES_*` also accepted on deserialize. See [ThinkingSummaries](#thinkingsummaries) |
| `ThinkingLevel` | lowercase | `"low"`, `"medium"`, `"high"` | `DEFAULT_MODEL` rejects `"minimal"` (live 2026-09-24) |
| `Resolution` | snake_case | `"low"`, `"medium"`, `"high"`, `"ultra_high"` | Verified 2026-01-05 (pre-revision) |
| `VideoProcessing` | string or object | `"static"` / `"agentic"` / `{"type": "static", "start_offset": "5s", "fps": 1}` | The window is the cost lever among the `static` forms. Only valid inside a `user_input` step. Measured live 2026-08-18 |
| `Tool::FileSearch` | object | `{"type": "file_search", "file_search_store_names": [...]}` | Rust `store_names` → wire `file_search_store_names`. Verified live 2026-08-16 |
| `Tool::GoogleSearch` | object + optional array | `{"type": "google_search", "search_types": ["web_search"]}` | |
| `Tool::GoogleMaps` | object + optional fields | `{"type": "google_maps", "enable_widget": true, "latitude": ..., "longitude": ...}` | `latitude`/`longitude` pending live verification |
| `Tool::ComputerUse` | snake_case object | `{"type": "computer_use", "environment": "browser", ...}` | Pending live verification |
| `Tool::Retrieval` | object | `{"type": "retrieval", "retrieval_types": [...], ...}` | **Rejected by the Gemini API** as Vertex-only (live 2026-07) |
| `RetrievalType` | snake_case | `"vertex_ai_search"`, `"rag_store"` | Not verifiable on the Gemini API |
| `SpeechConfig` | **list** of flat objects | `[{"voice": "Kore", "language": "en-US", "speaker": "Alice"}]` | The crate sends the list. See [speech_config wire forms](#speech_config-wire-forms) (live 2026-09-24) |
| `WebhookEvent` | dotted lowercase | `"batch.succeeded"`, `"interaction.completed"`, `"video.generated"` | Verified live 2026-07: the API's validation error lists exactly our 7 values |
| `WebhookState` | snake_case | `"enabled"`, `"disabled"`, `"disabled_due_to_failed_deliveries"` | Output only. `enabled`/`disabled` observed live 2026-07 |
| `RevocationBehavior` | snake_case | `"revoke_previous_secrets_after_h24"`, `"revoke_previous_secrets_immediately"` | Request only. Verified live 2026-07 (the validation error lists exactly these) |
| `SourceType` | snake_case | `"gcs"`, `"inline"`, `"repository"`, `"skill_registry"` | `inline` verified live 2026-07 |
| `NetworkConfig` | string or object | `"disabled"` / `{"allowlist": [{"domain": "*.googleapis.com"}]}` | Omit to allow all traffic. Both forms verified live 2026-07 |
| `EnvironmentSpec` | string or object | `"env-123"` / `{"type": "remote", "sources": [...]}` | Both forms verified live 2026-07 on requests |
| `ResponseFormat` | tagged by `"type"`, or a raw schema dict | `{"type": "text", "mime_type": "application/json", "schema": {...}}` | Single object or list. Verified live 2026-07; see [ResponseFormat](#responseformat) |
| `ResponseDelivery` | lowercase | `"inline"`, `"uri"` | The validation error lists exactly these (2026-07), but `delivery` is rejected for audio and image |
| `VideoTask` | snake_case | `"text_to_video"`, ..., `"extend"` | Verified live 2026-07 via the validation error |
| `Visualization` | lowercase | `"off"`, `"auto"` | Verified live 2026-07 via the validation error |
| Audio MIME type (TTS) | plain | `"audio/wav"` / `"audio/L16;codec=pcm;rate=24000"` | Model-dependent; see [Audio response](#audio-response-tts-output). Live 2026-09-24 |
| `VoiceType` | lowercase | `"prebuilt"`, `"prompted"`, `"replicated"` | `prebuilt`/`prompted` verified live 2026-09-24 |
| `VoicePitch` | lowercase | `"low"`, `"medium"`, `"high"` | Verified live 2026-09-24 |
| `CredentialType` | snake_case | `"bearer_token"`, `"environment_variable"`, `"oauth2"` | Create bodies are tagged by it. The first two verified live 2026-09-24 (`oauth2` create validates that `token_url` is reachable) |
| `CredentialStatus` | lowercase | `"active"`, `"revoked"` | Output only; `active` observed 2026-09-24 |
| `InjectionLocation` | lowercase | `"header"`, `"query"`, `"body"` | Sent as a list; the API also accepts a single string (2026-09-24) |
| `VideoResolution` | lowercase | `"360p"`, `"720p"`, `"1080p"`, `"4k"` | Server-validated (2026-09-24); no Interactions model outputs video to exercise it |
| `TranscriptionMode` | string or object | `"smart"` / `{"type": "verbatim", "diarization_mode": "speaker"}` | See [TranscriptionConfig](#transcriptionconfig) |
| `EnvironmentFileType` | **uppercase** | `"FILE"`, `"DIRECTORY"` | The bindings say lowercase; the API sends uppercase (live 2026-09-24). Both accepted; serializes uppercase |
| `GoogleSearchResultItem` | snake_case | `{"title": "...", "url": "...", "rendered_content": "..."}` | Live 2026-07: items may carry **only** `search_suggestions` |
| `UrlContextResultItem` | snake_case | `{"url": "...", "status": "success"}` | `status` is `success`/`error`/`paywall`/`unsafe`; no separate paywall field (2026-01-13) |
| `FileState` | uppercase | `"PROCESSING"`, `"ACTIVE"`, `"FAILED"` | Files API |
| `DocumentState` | prefixed uppercase | `"STATE_PENDING"`, `"STATE_ACTIVE"`, `"STATE_FAILED"` | File Search documents; verified live 2026-08-16 |
| `ImageAspectRatio` | ratio string | `"1:1"`, `"16:9"`, `"9:16"` | 14 values |
| `ImageSize` | size string | `"512"`, `"1K"`, `"2K"`, `"4K"` | |

## Details

### InteractionInput (request `input`)

The spec's input union is `str | [Step] | [Content] | Content`, and all four
deserialize. Requests never send the bare `[Content]` form:
`InteractionInput::Content` goes out as a single `user_input` step (#427).
`Text` stays a bare string, and `Steps` is already the step form.

The two array forms are not equivalent. Probed live 2026-08-16
(`gemini-3.7-flash`), with identical content sent each way:

| Input | bare `[Content]` | `[{"type": "user_input", "content": [...]}]` |
|---|---|---|
| text; text + inline image, audio or PDF; text + video by URI | completed | completed |
| text + video + `"processing": "static"` | **400** `Unknown parameter 'processing' at 'input[1]'` | completed |
| follow-up turn via `previous_interaction_id` | completed | completed |
| *empty* content | 400 `Missing input.` | 400 `Request has empty input.` |

The wrap applies only to `InteractionRequest::input`.
`InteractionResponse::input` re-serializes in the shape the server sent. A
request's `Content` input therefore deserializes back as
`Steps(vec![Step::user_input(..)])`, since the two are identical on the wire.

### Step

Steps replace the launch-era `outputs: [Content]`. They are tagged by
`"type"` with snake_case values. Core shapes (the steps model, snake_case
fields, and the `function_call` `signature`) were verified live 2026-07.

| Wire `type` | Variant | Payload |
|-------------|---------|---------|
| `user_input` | `UserInput` | `{"content": [Content]}` |
| `model_output` | `ModelOutput` | `{"content": [Content], "error"?: {code, message, details}}` |
| `thought` | `Thought` | `{"signature"?: "...", "summary"?: [Content]}`. `signature` is opaque, not text (verified pre-revision). `summary` appears only when summaries are requested (live 2026-09-24) |
| `function_call` | `FunctionCall` | `{"id", "name", "arguments": {...}, "signature"?}`, **top-level** arguments. The API returns `signature` and **rejects stateless replay without it** (live 2026-07); the generated bindings omit it |
| `function_result` | `FunctionResult` | `{"call_id", "name"?, "result": <payload>, "is_error"?: bool, "signature"?}` |
| `code_execution_call` | `CodeExecutionCall` | `{"id", "arguments": {"language": "python", "code": "..."}, "signature"?}`, **nested** arguments |
| `code_execution_result` | `CodeExecutionResult` | `{"call_id", "result": "...", "is_error": bool, "signature"?}` |
| `url_context_call` | `UrlContextCall` | `{"id", "arguments": {"urls": [...]}, "signature"?}` |
| `url_context_result` | `UrlContextResult` | `{"call_id", "result": [UrlContextResultItem], "is_error"?, "signature"?}` |
| `google_search_call` | `GoogleSearchCall` | `{"id", "arguments": {"queries": [...]}, "search_type"?, "signature"?}` |
| `google_search_result` | `GoogleSearchResult` | `{"call_id", "result": [GoogleSearchResultItem], "is_error"?, "signature"?}` |
| `tool_call` | `ToolCall` | `{"id", "signature"?}`. **What MCP calls actually arrive as** (live 2026-08-16) |
| `mcp_server_tool_call` | `McpServerToolCall` | `{"id", "name", "server_name", "arguments"}`. Spec-only, never observed |
| `mcp_server_tool_result` | `McpServerToolResult` | `{"call_id", "name"?, "server_name"?, "result"}`. Spec-only, never observed |
| `file_search_call` | `FileSearchCall` | `{"id", "signature"?}` |
| `file_search_result` | `FileSearchResult` | `{"call_id", "result": [FileSearchResultItem], "signature"?}`. `result` (items with `title`, `text`, `file_search_store`) is spec-only and never populated (see [File Search](#file-search)) |
| `google_maps_call` | `GoogleMapsCall` | `{"id", "arguments": {"queries": [...]}, "signature"?}` |
| `google_maps_result` | `GoogleMapsResult` | `{"call_id", "result": [GoogleMapsResultItem], "signature"?}` |
| `processing_call` | `ProcessingCall` | `{"id", "signature"?}`. Emitted for video with `processing: "agentic"`, one or more per turn (live 2026-09-24, `gemini-3.8-flash`). The ~36KB signature is **required** on stateless replay (`400 Processing call step is missing signature`) |
| `processing_result` | `ProcessingResult` | `{"call_id", "signature"?}`. Same replay requirement |
| `retrieval_call` | `RetrievalCall` | `{"id", "arguments": {"queries": [...]}, "retrieval_type"?, "signature"?}`. Vertex-only |
| `retrieval_result` | `RetrievalResult` | `{"call_id", "is_error"?, "signature"?}`. Vertex-only |
| anything else | `Unknown { step_type, data }` | Round-trips losslessly |

`function_call` (and, per spec, `mcp_server_tool_call`) keep arguments at the
top level. The built-in tool calls nest theirs in `arguments`, and the crate
flattens them into `urls`, `queries`, `language` and `code`.

**MCP arrives as `tool_call`.** Verified live 2026-08-16 (`gemini-3.7-flash`,
a real MCP server, successful call). The response carried a `tool_call`
step with only `id`, `signature` and `type`, plus `total_tool_use_tokens:
1962`. There was no `mcp_server_tool_call` step, and no server name, tool
name or arguments anywhere. So `step_summary().mcp_server_tool_call_count`
reads 0 on a successful call. Count with `step_summary().tool_call_count` or
`tool_calls()`. The MCP variants are kept because nothing rejects them (unlike
`cached_content`, D-005) (#459).

### StepDelta

Deltas build up the step that the matching `step.start` announced.

| Wire `type` | Variant | Notes |
|-------------|---------|-------|
| `text` | `Text` | Text fragment |
| `image` / `audio` / `video` / `document` | media variants | `Content` field shapes (audio also accepts legacy `rate`) |
| `thought_summary` | `ThoughtSummary` | `{"content": Content}` |
| `thought_signature` | `ThoughtSignature` | `{"signature": "..."}` |
| `text_annotation_delta` | `TextAnnotation` | **Tag differs from the variant name.** `{"annotations": [Annotation]}` |
| `arguments_delta` | `ArgumentsDelta` | `{"arguments": "<raw JSON fragment>"}`, concatenated and parsed at `step.stop` |
| `function_result` | `FunctionResult` | `call_id` is optional (dropped from the 2.25 bindings); the accumulator keeps the one from `step.start` |
| `code_execution_*`, `url_context_*`, `google_search_*`, `file_search_*`, `google_maps_*`, `mcp_server_tool_*` | matching variants | Call deltas carry the flattened fields |
| `processing_call` / `processing_result` | processing variants | `{"signature": "..."}`. `step.start` announces `signature: ""`; the value arrives only here (live 2026-09-24) |
| `retrieval_call` / `retrieval_result` | retrieval variants | Vertex-only |
| `tool_call` | *(none)* | Never observed as a delta: `tool_call` arrives whole on `step.start` |
| anything else | `Unknown { delta_type, data }` | Preserved. A `signature` on it is merged onto a same-typed `Step::Unknown` at that index, so replay keeps it |

SSE event types in this revision: `interaction.created`,
`interaction.status_update`, `step.start`, `step.delta`, `step.stop` (with
`usage` and `step_usage`), `interaction.completed`, `error`. The pre-revision
`interaction.start`, `content.start` / `delta` / `stop` and
`interaction.complete` are gone. Unknown event types become
`StreamChunk::Unknown { chunk_type, data }`.

### Annotation

A union tagged by `"type"` (previously a single struct):

| Wire `type` | Variant | Fields |
|-------------|---------|--------|
| `url_citation` | `UrlCitation` | `url`, `title` |
| `file_citation` | `FileCitation` | `document_uri`, `file_name`, `source`, `custom_metadata`, `page_number`, `media_id` |
| `place_citation` | `PlaceCitation` | `place_id`, `name`, `url`, `review_snippets: [ReviewSnippet { title, url, review_id }]` |
| `speech_metadata` | `SpeechMetadata` | `speaker`, `style`; indices optional. A TTS **input** annotation: required per turn for multi-speaker on `gemini-3.8-flash-tts`, rejected by older TTS models. Spanned annotations must tile the text without gaps (live 2026-09-24) |
| `word_info` | `WordInfo` | `text`, `speaker`, `start_offset` / `end_offset` (duration strings), optional indices. Transcription output; not yet observed |
| anything else | `Unknown { annotation_type, data }` | The server's enum also lists `in_context_file_citation` and `reference_metadata`, which are absent from the 2.25 bindings (2026-09-24) |

Citations carry `start_index` / `end_index` as **UTF-8 byte offsets** into
the annotated text. On `speech_metadata` and `word_info` they are optional,
so `start_index()` / `end_index()` return `None` when they are absent.
`extract_span(&text)` slices the text by those offsets.

### FunctionResultPayload

The `result` of `function_result` and `mcp_server_tool_result` serializes
**untagged**, exactly as the inner value:

| Variant | Serializes as | Deserialized when |
|---------|---------------|-------------------|
| `Text(String)` | JSON string | The value is a string |
| `Contents(Vec<Content>)` | Array of content blocks | A non-empty array whose every element is an object with a string `"type"` |
| `Json(Value)` | The raw value | Anything else; this doubles as the Evergreen catch-all |

`From` exists for `serde_json::Value`, `&str`, `String` and `Vec<Content>`.
From a `Value`, strings become `Text` and objects `Json`; any other value
(array, number, bool, null) is wrapped as `{"result": value}`, because the
API rejects a top-level array. Deserializing never wraps.

### ToolChoice and FunctionCallingMode

`generation_config.tool_choice` is a plain mode string or an
`allowed_tools` object:

```json
{"tool_choice": "any"}
{"tool_choice": {"allowed_tools": {"mode": "any", "tools": ["get_weather"]}}}
```

| Variant | Wire shape |
|---------|-----------|
| `ToolChoice::Mode(FunctionCallingMode)` | `"auto"` / `"any"` / `"none"` / `"validated"` |
| `ToolChoice::AllowedTools(AllowedTools)` | `{"allowed_tools": {"mode"?, "tools": [...]}}` |
| `ToolChoice::Unknown { choice_type, data }` | Unrecognized shapes |

An unknown mode **string** becomes `ToolChoice::Mode(FunctionCallingMode::Unknown)`;
an unknown **shape** becomes `ToolChoice::Unknown`. `AllowedTools` is also the
element type of an MCP server's `allowed_tools`. The pre-revision uppercase
modes (`"AUTO"`) deserialize to `Unknown`. Pending live verification.

### ServiceTier

`service_tier` on the request: `"flex"` / `"standard"` / `"priority"`. Every
response echoes the effective tier (`"standard"`) along with
`object: "interaction"`; both were verified live 2026-07 and are modeled on
`InteractionResponse`. Sending `flex` / `priority` is pending live
verification.

### Safety settings

| Enum | Wire values |
|------|-------------|
| `HarmCategory` | `hate_speech`, `dangerous_content`, `harassment`, `sexually_explicit`, `civic_integrity`, `image_hate`, `image_dangerous_content`, `image_harassment`, `image_sexually_explicit`, `jailbreak` |
| `SafetyThreshold` | `block_low_and_above`, `block_medium_and_above`, `block_only_high`, `block_none`, `off` |
| `SafetyMethod` | `severity`, `probability` |

The values come from the SDK spec. The `safety_settings` parameter itself is
rejected by the Gemini API (live 2026-08-08: 400 "not available on the
Gemini API but it is available on the Gemini Enterprise Agent Platform").

### EnvironmentStatus

`"active"` (verified live 2026-08-08) and `"expired"` (spec). In the same
probe, int64 counts arrived as protobuf-JSON strings and timestamps as ISO 8601
with an offset.

### TriggerStatus and TriggerExecutionStatus

| Enum | Wire values |
|------|-------------|
| `TriggerStatus` | `active`, `paused`, `error` |
| `TriggerExecutionStatus` | `in_progress`, `completed`, `failed`, `skipped`, `timed_out` |

The values come from the SDK spec. Live verification is blocked because
creation needs a custom agent, which is gated. Verified live 2026-08-08: a
model-only trigger interaction is rejected ("Agent '' is invalid or not
found"), and an empty `GET /v1beta/triggers` returns `{}`. The unverified
shapes are hedged:

- `create_time` / `update_time` (the spec) also accept the Environments
  spelling `created` / `updated`.
- The executions list reads `trigger_executions` (the spec) or `executions`
  (the path segment).
- All trigger-family timestamps go through a lenient RFC 3339
  deserializer. An unexpected shape degrades to `None` with a `warn!`, rather
  than failing the list.

### ThinkingSummaries

Sent as `"auto"` / `"none"` in both `generation_config.thinking_summaries`
and `agent_config.thinking_summaries`. `THINKING_SUMMARIES_AUTO` /
`_NONE` are still accepted on deserialize.

The agent-config spelling reversed. On 2026-01-04 the API rejected `"auto"`
(`unknown enum value: 'auto'`), so the crate sent `THINKING_SUMMARIES_*`
there. On 2026-08-10, verified live, the reverse:
`The value 'THINKING_SUMMARIES_AUTO' is not supported for 'agent_config.thinking_summaries'. Supported values: 'auto', 'none'.`

### ThinkingLevel

`"minimal"`, `"low"`, `"medium"`, `"high"` in
`generation_config.thinking_level`. Live 2026-09-24: `gemini-3.8-flash`
rejects `"minimal"` (`Allowed values are: high, low, medium`), and omitting the
field does not disable thinking (see [Thinking Mode](THINKING_MODE.md)).

### InteractionStatus

| Variant | Wire value | Notes |
|---------|------------|-------|
| `Completed` | `completed` | |
| `InProgress` | `in_progress` | `Default` |
| `RequiresAction` | `requires_action` | |
| `Failed` | `failed` | |
| `Cancelled` | `cancelled` | |
| `Incomplete` | `incomplete` | From the SDK |
| `BudgetExceeded` | `budget_exceeded` | Pending live verification |

### Resolution

`resolution` on image and video content: `"low"`, `"medium"`, `"high"`,
`"ultra_high"`. Verified 2026-01-05 (pre-revision) with `LOUD_WIRE=1`.

### VideoProcessing

`processing` on video content:

| Variant | Wire value |
|---------|------------|
| `Static` | `"static"` |
| `Agentic` | `"agentic"` |
| `StaticSegment { start_offset, end_offset, fps }` | `{"type": "static", "start_offset": "10.5s", "end_offset": "30s", "fps": 1.0}` |
| `Unknown { processing_type, data }` | The original string or object |

`Static` and `StaticSegment` stay distinct, so each form round-trips as it
arrived.

Video input tokens, re-measured 2026-08-18 on one source video
(`gemini-3.7-flash`):

| `processing` | Video input tokens |
|--------------|--------------------|
| *(omitted)*, `"static"`, `{"type": "static"}`, `{"type": "static", "fps": 1}` | 57,778 |
| `{"type": "static", "start_offset": "5s", "end_offset": "10s", "fps": 1}` | **16,198** |
| `"agentic"` | No video modality; billed as `image` (2,112 and 4,158 on two runs) |

On 2026-08-16 the window's saving was ~127x (455 vs 57,775); the clipped
accounting has since been revised. What has held across both measurements:
a window reduces ingestion among the `static` forms, and `fps` alone does
not.

- **Position.** `processing` is accepted only inside a `user_input` step. A
  bare content array gets `400 Unknown parameter 'processing' at 'input[1]'`.
  The crate always sends `Content` input as a `user_input` step, so
  `with_content()` and `with_history()` both work.
- **Validation.** Unknown values are rejected by field path:
  `400 Invalid enum value 'bogus_nonsense' at 'input[0].content[1].processing'`.
- `"agentic"` produces `processing_call` / `processing_result` steps whose
  signatures are required on replay (see [Step](#step)).

Verified 2026-08-16 and re-measured 2026-08-18, including by
`test_video_processing_segment_reduces_token_cost`.

### File Search

**The store and document resources are camelCase**, unlike the
Interactions API.

```json
{"name": "fileSearchStores/my-docs-4kws71n2ybpr", "displayName": "my-docs",
 "createTime": "2026-08-16T15:13:13.783782Z", "updateTime": "2026-08-16T15:13:13.783782Z",
 "embeddingModel": "models/gemini-embedding-001"}
```

- List envelopes are `{"fileSearchStores": [...]}` and `{"documents": [...]}`.
  An empty store list is a bare `{}`. `page_size` and `pageSize` are both
  accepted.
- Document `sizeBytes` is a JSON **string** (`"27"`).
- `DocumentState` is `STATE_PENDING` / `STATE_ACTIVE` / `STATE_FAILED`,
  prefixed, unlike the Files API's bare `PROCESSING` / `ACTIVE` / `FAILED`.

The request tool:

```json
{"type": "file_search", "file_search_store_names": ["fileSearchStores/my-store-123"],
 "top_k": 10, "metadata_filter": "category = 'technical'"}
```

Rust `store_names` maps to `file_search_store_names`: full store resource
names, not file ids.

Behavior, all verified live 2026-08-16:

- **Indexing is asynchronous.** A fresh upload is `STATE_PENDING` and does
  not match until `STATE_ACTIVE` (about 1-2 s for a small text file). Use
  `wait_for_document_active()`.
- **Deleting a non-empty document or store needs `force=true`**. Otherwise:
  `400 Cannot delete non-empty Document` / `... FileSearchStore`
  (`FAILED_PRECONDITION`).
- **Uploads accept `raw` and `multipart`.** The crate uses `raw`, with
  `display_name` as a query parameter.
- **The upload response is an operation wrapper**:
  `{"name": ".../upload/operations/...", "response": {"documentName": ...}}`.
  The crate resolves it with a follow-up GET.
- **`file_search_result` steps carry no chunks.** Only `call_id`,
  `signature` and `type` arrive, so `has_file_search_results()` is true
  while `file_search_results()` is empty. The retrieved content shows only
  in the answer (#429).
- **`file_search` can't be combined with `google_search` or
  `url_context`**: 400 `'<other>' and 'file_search' cannot be combined in the
  same request. Please choose one to continue.` `code_execution` is accepted.

Covered by `tests/file_search_stores_tests.rs` and `examples/file_search.rs`.

### Google Search steps

Verified live 2026-07 with `LOUD_WIRE=1`:

- `google_search_call` carried `id`, nested `arguments.queries`,
  `search_type: "web_search"` and `signature`.
- The `google_search_result` items carried **only** `search_suggestions`
  (an HTML rendering payload), with no `title` / `url` / `rendered_content`.
  Those then deserialize empty and are skipped on re-serialize.
- Citations arrived as `url_citation` annotations on the `model_output`
  text, and `usage.grounding_tool_count` was `[{"type": "google_search",
  "count": 1}]`.

### Google Maps steps

- `google_maps_call`: `id`, `arguments.queries`, `signature`.
- `google_maps_result`: `call_id`, `result: [GoogleMapsResultItem]`,
  `signature`. Each item has `places: [Place]` (`name`,
  `formatted_address`, `place_id`, `lat`, `lng`, plus `url` and
  `review_snippets` in this revision) and `widget_context_token`.

Verified pre-revision as `Content`. The step form and the new `Place` fields
are pending live verification.

### Computer Use

```json
{"type": "computer_use", "environment": "browser",
 "excluded_predefined_functions": ["submit_form", "download"],
 "enable_prompt_injection_detection": true,
 "disabled_safety_policies": ["financial_transactions"]}
```

- `environment`: `"browser"`, `"mobile"` or `"desktop"`.
- Known `disabled_safety_policies`: `financial_transactions`,
  `sensitive_data_modification`, `communication_tool`, `account_creation`,
  `data_modification`, `user_consent_management`,
  `legal_terms_and_agreements`.
- Fields are snake_case (legacy `excludedPredefinedFunctions` is accepted on
  deserialize).
- There is no `computer_use_call` / `computer_use_result` any more. Actions
  arrive as plain `function_call` steps (`navigate`, `click_at`, ...).

Derived from the API docs; pending live verification (the tool is
allowlisted).

### SpeechConfig (generation_config)

`generation_config.speech_config` is a **list** of flat speaker configs:
one entry for a single voice, one per `speaker` for multi-speaker TTS.

```json
{
  "model": "gemini-3.8-flash-tts",
  "input": [{"type": "user_input", "content": [
    {"type": "text", "text": "Hi Bob!", "annotations": [{"type": "speech_metadata", "speaker": "Alice"}]},
    {"type": "text", "text": "Hey Alice!", "annotations": [{"type": "speech_metadata", "speaker": "Bob"}]}
  ]}],
  "response_modalities": ["audio"],
  "generation_config": {"speech_config": [
    {"voice": "Kore", "language": "en-US", "speaker": "Alice"},
    {"voice": "Puck", "language": "en-US", "speaker": "Bob"}
  ]}
}
```

| Field | Notes |
|-------|-------|
| `voice` | Voice name (`"Kore"`, `"Puck"`, ...); a system voice if omitted |
| `language` | Required when `voice` is set |
| `speaker` | Multi-speaker only: the name each turn's `speech_metadata` annotation (or transcript label) refers to |

- On `gemini-3.8-flash-tts` each text turn names its speaker with a
  `speech_metadata` annotation. The 2.5-pro and 3.1-flash TTS models take the
  `Alice: ...` transcript form instead, and reject the annotation (live
  2026-09-24).
- The nested `voiceConfig.prebuiltVoiceConfig.voiceName` form from Google's
  docs returns 400 (`no such field: 'voiceConfig'`, 2026-01-10).
- The API does not echo `speech_config` (or any `generation_config`) on
  reads, and `include_input=true` was a no-op for it (live 2026-07).

#### speech_config wire forms

| Form sent | 2026-08-16 (`gemini-2.5-pro-preview-tts`) | 2026-09-24 (all TTS models) |
|------|------|------|
| `[{voice, language, speaker}, ...]` | accepted | accepted |
| `{"speakers": [...]}` | `400 ... Expected an array, got object.` | accepted (mapped to `structured_speech_config`) |
| bare `{voice, language}` | `400 ... Expected an array, got object.` | `400 Unknown parameter 'voice' at 'generation_config.structured_speech_config'` |

The crate always **sends the list**. On deserialize it accepts all three
forms and normalizes them to a list, since a `GenerationConfig` nested in a
`Trigger` may have been written by another SDK. For maintainers: the
untagged `{"speakers": [...]}` arm must be tried before the single-object
arm. Every `SpeechConfig` field is optional, so the single arm would match
it and silently drop the speakers.

### Audio response (TTS output)

TTS audio arrives as an `audio` content block in a `model_output` step:

| MIME type | Models | Format | `extension()` |
|-----------|--------|--------|---------------|
| `audio/wav` | `gemini-3.8-flash-tts`, `gemini-3.8-flash-lite-tts` | RIFF/WAV, 24 kHz mono s16; no `sample_rate` / `channels` fields | `wav` |
| `audio/L16;codec=pcm;rate=24000` | `gemini-2.5-pro-preview-tts` | Raw 16-bit PCM | `pcm` |
| `audio/l16; rate=24000; channels=1` | `gemini-3.1-flash-tts-preview` | Raw 16-bit PCM, with `sample_rate` / `channels` | `pcm` |

All three verified live 2026-09-24.

### URL context steps

- `url_context_call`: `{"id", "arguments": {"urls": [...]}}`, flattened to
  `urls: Vec<String>`.
- `url_context_result`: `{"call_id", "result": [{"url", "status"}]}`.
  `UrlContextResultItem` has `is_success()`, `is_error()`, `is_unsafe()`
  and `is_paywall()`.

Shapes verified 2026-01-09 pre-revision; the step form is pending live
verification.

### Code execution

`code_execution_result` is `{"call_id", "is_error": bool, "result": "...",
"signature"?}`. The official docs describe an `outcome` enum
(`OUTCOME_OK`), but the wire uses `is_error` + `result` (found with
`LOUD_WIRE=1`, 2026-01-12, pre-revision). `call_id` is required since
2026-05-20.

`CodeExecutionLanguage` is lowercase `"python"` in
`code_execution_call.arguments.language`. `"PYTHON"` (pre-revision) now
deserializes to `Unknown`, and `Display` prints `"python"`. Pending live
verification.

### SearchType

`Tool::GoogleSearch.search_types` and `google_search_call.search_type`:
`web_search` (verified pre-revision), `image_search` (model-restricted),
`enterprise_web_search` (pending live verification).

### Tool::Retrieval

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

- `rag_store_config.similarity_top_k` / `vector_distance_threshold` are
  deprecated in favor of `rag_retrieval_config`.
- The RAG filter is `filter` on the wire (the Python bindings alias it as
  `filter_`).
- `ranking.ranking_config` is always `"rank_service"`.
- Exa / Parallel `api_key` values are sent on the wire.

**Rejected by the Gemini API** (live 2026-07): "The value 'retrieval' is not
supported for 'tools[0].type' on the Gemini API, it is allowed on the Gemini
Enterprise Agent Platform." The same error lists the supported tool types:
`google_maps`, `mcp_server`, `function`, `google_search`, `file_search`,
`computer_use`, `code_execution`, `url_context`.

### Webhooks

```json
{
  "id": "wh123bare0pq", "name": "my-hook", "uri": "https://example.com/hook",
  "subscribed_events": ["batch.succeeded", "interaction.completed", "video.generated"],
  "state": "enabled",
  "signing_secrets": [{"truncated_secret": "whsec_...abcd", "expire_time": "2026-08-01T00:00:00Z"}],
  "new_signing_secret": "whsec_full"
}
```

- `WebhookEvent`: `batch.succeeded`, `batch.expired`, `batch.failed`,
  `interaction.requires_action`, `interaction.completed`,
  `interaction.failed`, `video.generated`.
- `:rotateSigningSecret` takes `{"revocation_behavior": ...}` and returns
  `{"secret": "..."}`. `:ping` takes and returns `{}`.
- Per-request routing: `webhook_config: {"uris": [...], "user_metadata": {...}}`.

Verified live 2026-07 (full CRUD, `:ping`, `:rotateSigningSecret`):

- Get and list echo exactly what create sent. `new_signing_secret` appears
  only on create. Ids are bare strings (no `webhooks/` prefix).
- `create_time` / `update_time` were not returned by any endpoint; the crate
  keeps them optional.
- `:ping` accepts `{}` or no body, and returns `{}` even for unreachable URIs.
- PATCH applies the fields present in the body. `update_mask` is not required
  and was observed to be ignored. Unknown query parameters are ignored too,
  but camelCase **body** keys get "Unknown parameter ... Did you mean ...".
- Each rotation returns a fresh secret. By default the previous secrets get a
  24 h `expire_time`.
- Invalid `subscribed_events` are rejected with a list of exactly our seven
  values.
- `webhook_config` on an interaction requires `background=true`, and is
  echoed verbatim in the create response (modeled as
  `InteractionResponse::webhook_config`).

### Environment

`environment` (request) and `base_environment` (agent) take an environment id
string or a remote environment object:

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
    {"domain": "api.example.com", "transform": [{"Authorization": "Bearer ..."}]},
    {"domain": "api.github.com", "credential": "github-token"}
  ]},
  "env": {"PLAIN_VAR": {"value": "hello"}, "SECRET_VAR": {"credential": "my-env-credential"}}
}
```

- `network`: the string `"disabled"`, an `{"allowlist": [...]}` object, or
  omitted (all traffic allowed).
- `env` and `AllowlistEntry::credential` reference `/v1beta/credentials`.
  Live 2026-09-24: both are validated (unknown sibling keys rejected; an
  unknown credential id is a 404), echoed, and applied at runtime; see
  `src/credentials.rs` for what the sandbox and the egress proxy see.
  The echo spells `env` as a list of single-key maps; both forms
  deserialize. The bindings' string form of `env` is rejected
  (`Invalid input at 'environment'`) and lands in `extra` if read.
- The response echoes the assigned `environment_id`, which works as the
  string form on later turns.

Verified live 2026-07 (Antigravity agent, `background: true`): the inline
source, `"disabled"`, an allowlist with `transform`, and the id string form.
`gcs`, `repository` and `skill_registry` sources and `base_environment` were
not exercised.

### ResponseFormat

`response_format` takes one object or a list (one per output modality):

```json
{"type": "text", "mime_type": "application/json", "schema": {"type": "object"}}
{"type": "audio", "mime_type": "audio/mp3", "delivery": "inline", "sample_rate": 24000, "bit_rate": 128000}
{"type": "image", "mime_type": "image/jpeg", "delivery": "uri", "aspect_ratio": "16:9", "image_size": "2K"}
{"type": "video", "delivery": "uri", "gcs_uri": "gs://bucket/out", "aspect_ratio": "9:16", "duration": "8s"}
```

- Known MIME types: text `application/json`, `text/plain`; audio `audio/mp3`,
  `audio/ogg_opus`, `audio/l16`, `audio/wav`, `audio/alaw`, `audio/mulaw`;
  image `image/jpeg`.
- A raw JSON-schema dict (the pre-revision form) is accepted too. It has no
  known `"type"`, so it round-trips through `ResponseFormat::Unknown`. When
  building a request, a raw `serde_json::Value` given to
  `with_response_format()` becomes the typed `text` / `application/json`
  form.

Verified live 2026-07, single and list forms (list errors index as
`response_format[i]`). Constraints observed on the Gemini API:

| Modality | Behavior |
|----------|----------|
| text | The `application/json` schema form works end to end; output is validated against the schema |
| image | Inline only. `mime_type` accepts only `image/jpeg`. Any `delivery` → 400 "Image delivery mode is not supported." |
| audio | Works with `sample_rate`. Any `mime_type` → 400 "Audio mime_type is not supported in response_format."; any `delivery` → 400 "Audio delivery mode is not supported." |
| video | `gcs_uri` is Vertex-only. No Interactions-served model supports video output |

### VideoConfig and VideoTask

`generation_config.video_config.task`: `text_to_video`, `image_to_video`,
`reference_to_video`, `edit`, `extend`. Omit it to let the model infer the
task. Verified live 2026-07 via the validation error, which is how `extend`
was found. Video generation itself is not reachable through the Interactions
API: Veo models return 404 (they list only `predictLongRunning`), Gemini
models reject `response_modalities: ["video"]`, and `video_config` is
accepted but ignored on non-video models.

### AntigravityConfig

`{"agent_config": {"type": "antigravity", "max_total_tokens": 200000}}`.
Verified live 2026-08-09 on `antigravity-preview-05-2026`, which requires an
`environment`. The validation error lists the `agent_config.type` values
`dynamic`, `deep-research`, `code-mender` and `antigravity`. `model` is
validated per agent: an unavailable value returns 404 (observed with
`gemini-3.6-flash`; the agent's model catalog can't be listed on a standard
key).

### Deep Research agent_config

`{"type": "deep-research", "visualization": "auto", "collaborative_planning": true}`.
`Visualization` is `"off"` / `"auto"`, verified live 2026-07 via the
validation error. `visualization` and `collaborative_planning` were accepted
on a background run. `enable_bigquery_tool` is rejected on the Gemini API as
Vertex-only.

### GroundingToolCount

`usage.grounding_tool_count` is a list of `{"type": ..., "count": n}`. The
Rust field `tool_type` stays a plain string (known values: `google_search`,
`google_maps`, `retrieval`). `usage.grounding_count_for_tool("google_search")`
reads one entry.

### TranscriptionConfig

`generation_config.transcription_config` keeps its constrained fields as
open strings:

| Field | Documented values | Notes |
|-------|-------------------|-------|
| `diarization_mode` | `"speaker"` | Deprecated in 2.25 in favor of `mode` |
| `timestamp_granularities` | `"word"` | An empty list means no timestamps. Deprecated in 2.25 in favor of `mode` |
| `language_codes` | BCP-47 codes | Empty or omitted means auto-detect |

`mode` (`TranscriptionMode`) is `"smart"` / `"verbatim"` or
`{"type": "smart"}` / `{"type": "verbatim", "diarization_mode"?,
"timestamp_granularities"?}`. The crate sends the object form and reads both.
The bindings' `language_hints` is not modeled: `400 Unknown parameter
'language_hints'`. Verified live 2026-09-24 with audio input (the enum is
server-validated: `Invalid enum value 'zzz'`); no output difference, and no
`word_info` annotation, was observed on general models.

## Probing a new enum

Send candidates with curl and read the error. Validation errors usually list
the accepted values.

```bash
curl -s https://generativelanguage.googleapis.com/v1beta/interactions \
  -H "X-Goog-Api-Key: $GEMINI_API_KEY" -H "Content-Type: application/json" \
  -d '{"model": "gemini-3.8-flash", "input": "test", "generation_config": {"thinking_level": "zzz"}}'
```

Try lowercase (`"auto"`), SCREAMING_CASE (`"AUTO"`) and fully qualified
(`"THINKING_SUMMARIES_AUTO"`) spellings. This revision is mostly
lowercase/snake_case, but check with `LOUD_WIRE=1` before assuming.

## Structs and `#[non_exhaustive]`

Response and resource structs are `#[non_exhaustive]`, so the crate can add
fields without a breaking change (D-002). `tests/non_exhaustive_responses.rs`
fails the build when a deserializable public struct lacks the attribute and
isn't in its `REQUEST_SIDE` exemption list. The rule:

- **Closed**: anything the API returns (`InteractionResponse`,
  `UsageMetadata`, the `*ListResponse` wrappers, result items). That includes
  read-write resources (`Agent`, `Environment`, `Trigger`, `Webhook`), where
  constructors like `Agent::new(id)` cover the sending side. Create bodies
  that ship builders (`CreateFileSearchStoreRequest`) are closed too.
- **Open**: request types the user assembles and the API never returns
  (`GenerationConfig`, `FunctionDeclaration`, the tool configs).

Outside the crate the attribute blocks struct literals and
`..Default::default()`, but not `T::default()` plus field assignment:

```text
let mut response = InteractionResponse::default();
response.status = InteractionStatus::Completed;
```

Types with neither `Default` nor a constructor need a JSON fixture:
`FileMetadata`, `FileError`, `VideoMetadata`, `ListFilesResponse`,
`FileUploadResponse` and `AutoFunctionResult`.
(`ModalityTokens::new()`, `StreamEvent::new()` and
`FunctionCallInfo::to_owned()` cover the others without `Default`.)

## Antigravity harness protocol (feature `antigravity`)

`genai_rs::antigravity::protocol` speaks the localharness proto-JSON
protocol (see [ANTIGRAVITY.md](ANTIGRAVITY.md)). It was verified against the
descriptor set and a live harness from the `google-antigravity` 0.1.18 wheel
(`LOUD_WIRE=1` sessions, plus a descriptor diff against 0.1.10), and earlier
against 0.1.10 and 0.1.5.

- Field names are **camelCase**; enums are **SCREAMING_SNAKE_CASE** strings.
- 64-bit integers (`seqNum`, token counts) arrive as JSON strings; both
  strings and numbers are accepted.
- 0.1.18 sends many unset strings as `""` (`thinking`, `serverName`,
  `unavailableReason`). Where presence carries meaning (an error that fails
  the turn, a parent trajectory id), blank reads as absent.
- A user message is `{"userInput": {"parts": [{"text": ...}]}}` on 0.1.18.
  The pre-0.1.18 `{"userInput": "..."}` string form is rejected.

| Type | Context field | Wire values |
|------|---------------|-------------|
| `StepState` | `state_type` | `STATE_ACTIVE`, `STATE_DONE`, `STATE_WAITING_FOR_USER`, `STATE_ERROR` |
| `StepSource` | `source_type` | `SOURCE_SYSTEM`, `SOURCE_USER`, `SOURCE_MODEL` |
| `StepTarget` | `target_type` | `TARGET_USER`, `TARGET_MODEL`, `TARGET_ENVIRONMENT` |
| `TrajectoryState` | `state_type` | `STATE_RUNNING`, `STATE_FULLY_IDLE` (terminal; alias `STATE_IDLE`), `STATE_WAITING_FOR_TASKS`, `STATE_CANCELLED` |
| `StopReason` | `reason_type` | `STOP_REASON_MAX_MODEL_CALLS_EXCEEDED`, `..._MAX_TOOL_CALLS_...`, `..._MAX_{INPUT,OUTPUT,TOTAL}_TOKENS_EXCEEDED`, `STOP_REASON_QUOTA_EXHAUSTED` (0.1.18) |
| `ModelType` | `model_type` | `MODEL_TYPE_TEXT`, `MODEL_TYPE_IMAGE` |
| `Modality` | `modality_type` | `TEXT`, `IMAGE`, `VIDEO`, `AUDIO`, `DOCUMENT`: bare, unlike `MODALITY_UNSPECIFIED` (0.1.18) |
| `AgentBehavior` | `behavior_type` | `AGENT_BEHAVIOR_AUTONOMOUS`, `AGENT_BEHAVIOR_INTERACTIVE`, `AGENT_BEHAVIOR_MINIMAL` (0.1.18; client → harness) |
| `LifecycleHook` | `hook_type` | `LIFECYCLE_HOOK_PRE_TOOL`, `LIFECYCLE_HOOK_POST_TOOL`, ..., `LIFECYCLE_HOOK_ON_COMPACTION`, `LIFECYCLE_HOOK_STOP` (0.1.18) |
| `HookDecision` | `decision_type` | `ALLOW`, `DENY` |
| `LineAction` | `action_type` | `LINE_ACTION_INSERT`, `LINE_ACTION_DELETE`, `LINE_ACTION_NONE` |

The envelope oneofs have Unknown variants too (`InputEvent::Unknown` and
`OutputPayload::Unknown`, with `event_type` + `data`). Harness-emitted
structs (`StepUpdate`, `ToolCall`, `UsageMetadata`, action submessages, ...)
keep unrecognized fields in a flattened `extra` map. `strict-unknown` does
not apply here: the harness protocol is internal and unstable, so soft
typing is always on.

**Alias spellings are a deliberate exception to round-trip fidelity (D-003).**
These enums accept a value renamed between harness revisions, and
`as_wire_str` re-emits the current spelling: `STATE_IDLE` in,
`STATE_FULLY_IDLE` out. They are inbound-only (the client never sends a
`TrajectoryState`), so the asymmetry never reaches the wire. What it buys is
one build that *reads* either revision. Preserving the old spelling as
`Unknown` instead is what broke 0.1.5 → 0.1.10: only `Idle` ends a turn, so
every turn ran to its timeout. (0.1.18 changed the outbound `userInput` shape,
which no alias can cover, so this build *drives* 0.1.18 only.)
