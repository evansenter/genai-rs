# Enum Wire Formats & Unknown Variants

The verified wire formats for this crate's enums and unions, and the
Evergreen `Unknown` variant each one carries. The official docs are sometimes
wrong, so this catalog records what the live API actually sent or accepted,
and when.

This file and `docs/INTERACTIONS_API_GAP.md` are the only places a dated
live-API fact is recorded (D-032): this one for what a value or shape looks
like on the wire, that one for what the API accepts, rejects and does. The
guides and the rustdoc state the current behavior and link here. Antigravity
harness facts concern a local binary rather than the API and are keyed by
harness version: its wire shapes are under
[Antigravity harness protocol](#antigravity-harness-protocol-feature-antigravity)
below, and the dated probe record is in `docs/ANTIGRAVITY_BRIDGE_DESIGN.md`.

**Revision.** Everything here is Interactions API revision **2026-05-20**.
Every request sends `Api-Revision: 2026-05-20`, except the second request
of a resumable upload, which goes to the session URL the first one returned
and carries neither the key nor the header. The server currently ignores
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
| 12 | `ThinkingLevel` | `level_type` | minimal/low/medium/high/xhigh |
| 13 | `ThinkingSummaries` | `summaries_type` | auto/none |
| 14 | `ServiceTier` | `tier_type` | flex/standard/priority/deferred |
| 15 | `InteractionStatus` | `status_type` | Response status |
| 16 | `CodeExecutionLanguage` | `language_type` | python/bash |
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
| 31 | `HarmCategory` | `category_type` | Ten harm categories (some Vertex-only) |
| 32 | `SafetyThreshold` | `threshold_type` | Block thresholds (`block_none` Vertex-only) |
| 33 | `SafetyMethod` | `method_type` | severity/probability |
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
| 47 | `CodeMenderMode` | `mode_type` | scan/verify (CodeMender is Vertex-only) |
| 48 | `FileSource` | `source_type` | Files API file origin: UPLOADED/GENERATED/REGISTERED |
| 49 | `Question` | `question_type` | Decision questions: noul/choice/score (request only; `Unknown` is sent as its data) |
| 50 | `Answer` | `answer_type` | Decision answers: noul/choice/score |
| 51 | `BlockReason` | `reason_type` | A decision's `prompt_feedback.block_reason` |

The Antigravity harness enums are listed in their
[own section](#antigravity-harness-protocol-feature-antigravity).

**Removed in revision 2026-05-20**: `UrlRetrievalStatus`, `GroundingMetadata`,
`UrlContextMetadata`, `Turn`, and all tool-related `Content` variants
(`Thought`, `FunctionCall`, `FunctionResult`, `CodeExecutionCall/Result`,
`GoogleSearchCall/Result`, `UrlContextCall/Result`, `FileSearchResult`,
`GoogleMapsCall/Result`, `ComputerUseCall/Result`). Tool activity is now a
`Step`, and computer-use actions arrive as plain `function_call` steps.

### The Unknown variant pattern

<!-- ignore: a pattern, not code: `<context>_type` stands for each enum's field name -->
```rust,ignore
Unknown {
    <context>_type: String,   // the unrecognized wire value
    data: serde_json::Value,  // full JSON, re-serialized unchanged
}
```

Every type above also has `is_unknown()`, `unknown_<context>_type()` and
`unknown_data()`. In strict mode (`--cfg genai_strict_unknown`; a cfg, not a
Cargo feature, because it is not additive), unknown `Content` and
`Step` types and unknown values of any string enum (all generated by
`wire_enum!`) fail to deserialize instead; other tagged unions (`Tool`,
`StepDelta`, `StreamChunk`, ...) keep an unknown type as `Unknown`. A
known `StepDelta` holding a value strict mode rejects (an unknown
`arguments.language`, an unknown content type in a summary) fails like the
same value in a `Step`, naming the field. The CI `test-strict-unknown` job
runs this configuration.

### Unknown fields on known variants

An unknown *field* on a known type round-trips too (#491). Every known
variant of `Step`, `StepDelta`, `Content` and `Annotation` has an `extra`
map holding the keys it does not model, read with `extra()` and edited
with `extra_mut()`. The call steps and deltas that flatten a nested
`arguments` object (`code_execution_call`, `url_context_call`,
`google_search_call`, `google_maps_call`, `retrieval_call`) keep its other
keys in `arguments_extra`, merged back into `arguments` on serialize. The
result items (`GoogleSearchResultItem`, `UrlContextResultItem`,
`GoogleMapsResultItem`), `Place`, `ReviewSnippet`,
`StepError` and `VideoProcessing::StaticSegment` keep theirs in a flattened
`extra` field. So a field the API adds survives stateless history replay
before the crate models it, which is how `signature` once broke replay.
The request-side unions a stored interaction serves back do the same: every
known variant of `Tool`, `ResponseFormat` and `TranscriptionMode` has an
`extra` map (with `extra()` / `extra_mut()`), and
`ToolChoice::AllowedTools` keeps the keys beside `allowed_tools` in its
`extra`, as `NetworkConfig::Allowlist` does beside `allowlist`.

- Serializing writes the modeled fields first, then `extra`, for every type
  listed above. An `extra` entry named after one of the type's modeled
  fields, set or not, is never written, so a hand-built map cannot shadow a
  field or write its key twice. On the tagged types (the unions and
  `StaticSegment`) `type` counts as modeled, so it cannot retag a value
  either; on the plain structs and `ToolChoice::AllowedTools` a `type`
  entry is ordinary data and is written. An `arguments_extra` entry named after a modeled argument
  (`urls`, `queries`, `language`, `code`) is never merged into `arguments`.
  Other types with a flattened `extra`, such as `InteractionResponse` and
  `UsageMetadata`, write it unfiltered.
- A known type whose modeled field is malformed becomes `Unknown`, which
  keeps the whole object (a `TranscriptionMode` used to read its fields
  leniently, dropping a mistyped one). That was already so for a missing `id` or a
  mistyped `signature`. A non-object `arguments`, a non-string in `urls` or
  `queries`, and a mistyped `fps` on a static video segment used to be read
  leniently, dropping the bad value. For `Step` and `Content` under
  strict mode this is an error that names the field and why (for
  example ``field `arguments`: expected an object``), not an unknown type.
  A malformed `StepDelta` is still kept as `Unknown` in strict mode, unless
  the cause is a value strict mode rejects, which fails as in a `Step`.
- Strict mode does not apply to fields: an unknown field is kept in
  `extra` in both configurations.
- `src/proptest_unknown_fields.rs` injects an unknown key into every object,
  at every depth, of random values of every known variant (an exhaustive
  ordinal match checks the strategies reach them all), the request-side
  unions included, inside a whole `InteractionResponse`, and
  `tests/round_trip_fidelity_tests.rs` does the same over one wire fixture
  per step type.

## Quick reference

| Type | Wire format | Example | Status and notes |
|------|-------------|---------|------------------|
| `InteractionInput` | string, `[Step]`, `[Content]` or `Content` | `"hi"` / `[{"type": "user_input", "content": [...]}]` | Requests send `Content` input as one `user_input` step; see [InteractionInput](#interactioninput-request-input). Verified live 2026-08-16 |
| `Step` | tagged by `"type"`, snake_case | `"user_input"`, `"model_output"`, `"function_call"` | Core shapes verified live 2026-07; see [Step](#step) |
| `StepDelta` | tagged by `"type"` | `"text"`, `"arguments_delta"`, `"text_annotation_delta"` | Two tags differ from the variant names. All three verified live 2026-11-07; see [StepDelta](#stepdelta) |
| `Annotation` | tagged by `"type"` | `"url_citation"`, `"speech_metadata"` | `url_citation` observed live 2026-07; `speech_metadata` verified 2026-09-24 |
| `FunctionResultPayload` | untagged | `"ok"` / `{...}` / `[{"type": "text", ...}]` | String, JSON, or content-block list |
| `ToolChoice` | string or object | `"any"` / `{"allowed_tools": {...}}` | Both forms verified live 2026-11-07; see [ToolChoice and FunctionCallingMode](#toolchoice-and-functioncallingmode) |
| `FunctionCallingMode` | lowercase | `"auto"`, `"any"`, `"none"`, `"validated"` | All four verified live 2026-11-07. The uppercase spellings (pre-revision) deserialize to `Unknown`, and the API refuses them on send |
| `CodeExecutionLanguage` | lowercase | `"python"`, `"bash"` | `python` verified live 2026-11-07; `"PYTHON"` (pre-revision) now deserializes to `Unknown`. `bash` is spec only (2.34.0) |
| `ServiceTier` | lowercase | `"flex"`, `"standard"`, `"priority"`, `"deferred"` | `flex`/`priority` accepted and echoed live 2026-11-01; `deferred` is Vertex-only |
| `HarmCategory` | snake_case | `"hate_speech"`, `"jailbreak"` | Sent under the item key `type`. Five values accepted live 2026-10-30; `civic_integrity` and `image_*` are Vertex-only |
| `SafetyThreshold` | snake_case | `"block_only_high"`, `"off"` | Four values accepted live 2026-10-30; `block_none` is Vertex-only |
| `SafetyMethod` | lowercase | `"severity"`, `"probability"` | Both accepted live 2026-10-30 |
| `EnvironmentStatus` | lowercase | `"active"`, `"expired"` | `"active"` verified live 2026-08-08 |
| `TriggerStatus` | lowercase | `"active"`, `"paused"`, `"error"` | `active`/`paused` verified live 2026-11-01; `error` is output-only (sending it is a 400) |
| `TriggerExecutionStatus` | snake_case | `"in_progress"`, `"timed_out"` | `completed` verified live 2026-11-01; the rest from the SDK spec |
| `InteractionStatus` | snake_case | `"in_progress"`, `"requires_action"`, `"budget_exceeded"`, `"requires_continuation"`, `"queued"` | `budget_exceeded` never observed, and deprecated in the v1beta spec; `requires_continuation` spec only; `queued` bindings only. See [InteractionStatus](#interactionstatus). `Default` is `InProgress` |
| `SearchType` | snake_case | `"web_search"`, `"image_search"`, `"enterprise_web_search"` | `web_search` and `image_search` served live 2026-11-07 (`image_search` on some models only); `enterprise_web_search` is Vertex-only. See [SearchType](#searchtype) |
| `GroundingToolCount` | `{"type": ..., "count": n}` | `{"type": "google_search", "count": 1}` | In `usage.grounding_tool_count`; observed live 2026-07 |
| `ThinkingSummaries` | lowercase | `"auto"`, `"none"` | Sent lowercase in both contexts; `THINKING_SUMMARIES_*` also accepted on deserialize. See [ThinkingSummaries](#thinkingsummaries) |
| `ThinkingLevel` | lowercase | `"minimal"`, `"low"`, `"medium"`, `"high"`, `"xhigh"` | Which levels `DEFAULT_MODEL` accepts, and which models take `xhigh`, is under Models in `docs/INTERACTIONS_API_GAP.md` |
| `Resolution` | snake_case | `"low"`, `"medium"`, `"high"`, `"ultra_high"` | Verified 2026-01-05 (pre-revision) |
| `VideoProcessing` | string or object | `"static"` / `"agentic"` / `{"type": "static", "start_offset": "5s", "fps": 1}` | The window is the cost lever among the `static` forms. Only valid inside a `user_input` step. Measured live 2026-08-18 |
| `Tool::Function` | object | `{"type": "function", "name": "get_time", "description": "...", "parameters": {...}}` | Only `name` is needed: a function tool sent with just a `name` is accepted and called, and a stored read serves it that way (live 2026-11-05). `description` and `parameters` are left out when `None`; `FunctionParameters` keeps schema keywords it doesn't model. A tool read without its optional fields is still `Tool::Function`, not `Unknown` |
| `Tool::FileSearch` | object | `{"type": "file_search", "file_search_store_names": [...]}` | Rust `store_names` → wire `file_search_store_names`, left out when the list is empty. Verified live 2026-08-16 |
| `Tool::GoogleSearch` | object + optional array | `{"type": "google_search", "search_types": ["web_search"]}` | |
| `Tool::GoogleMaps` | object + optional fields | `{"type": "google_maps", "enable_widget": true, "latitude": ..., "longitude": ...}` | `latitude`/`longitude` verified live 2026-11-07: a stored read serves them back as sent. What they do is under Tools in `docs/INTERACTIONS_API_GAP.md` |
| `Tool::ComputerUse` | snake_case object | `{"type": "computer_use", "environment": "browser", ...}` | Verified live 2026-11-07, every field stored as sent; see [Computer Use](#computer-use). An empty `environment` (as read from a tool without one) is left out; so are an `mcp_server` tool's empty `name` and `url` |
| `Tool::Retrieval` | object | `{"type": "retrieval", "retrieval_types": [...], ...}` | **Rejected by the Gemini API** as Vertex-only (live 2026-07) |
| `RetrievalType` | snake_case | `"vertex_ai_search"`, `"rag_store"` | Not verifiable on the Gemini API |
| `SpeechConfig` | **list** of flat objects | `[{"voice": "Kore", "language": "en-US", "speaker": "Alice"}]` | The crate sends the list. See [speech_config wire forms](#speech_config-wire-forms) (live 2026-09-24) |
| `WebhookEvent` | dotted lowercase | `"batch.succeeded"`, `"interaction.completed"`, `"video.generated"` | Verified live 2026-07: the API's validation error lists exactly our 7 values |
| `WebhookState` | snake_case | `"enabled"`, `"disabled"`, `"disabled_due_to_failed_deliveries"` | Output only. `enabled`/`disabled` observed live 2026-07 |
| `RevocationBehavior` | snake_case | `"revoke_previous_secrets_after_h24"`, `"revoke_previous_secrets_immediately"` | Request only. Verified live 2026-07 (the validation error lists exactly these) |
| `SourceType` | snake_case | `"gcs"`, `"inline"`, `"repository"`, `"skill_registry"` | `inline` verified live 2026-07 |
| `NetworkConfig` | string or object | `"disabled"` / `{"allowlist": [{"domain": "*.googleapis.com"}]}` | Omit to allow all traffic. Both forms verified live 2026-07. `{"allowlist": "disabled"}` (`AllowlistDisabled`, the bindings' `Disabled` arm) is accepted too, echoed by a GET as `{"allowlist": [{"mode": "disabled"}]}`, which reads as `AllowlistDisabled` as well, and blocks egress like `"disabled"` (an agent's `curl` got the egress proxy's 403 under both, 200 with no network config; live 2026-11-05). `is_disabled()` is true for both. With another key or entry beside it, the echo reads as `Allowlist`, its domain-less entry written back without a domain, as read |
| `EnvironmentSpec` | string or object | `"env-123"` / `{"type": "remote", "sources": [...]}` | Both forms verified live 2026-07 on requests. An object with no `type` (or a null one) reads as remote; any other `type`, a non-string one included (`<non-string: 5>`), is kept whole as `Unknown` |
| `ResponseFormat` | tagged by `"type"`, or a raw schema dict | `{"type": "text", "mime_type": "application/json", "schema": {...}}` | Single object or list. Verified live 2026-07; see [ResponseFormat](#responseformat) |
| `ResponseDelivery` | lowercase | `"inline"`, `"uri"` | The validation error lists exactly these (2026-07), but `delivery` is rejected for audio and image |
| `VideoTask` | snake_case | `"text_to_video"`, ..., `"extend"` | Verified live 2026-07 via the validation error |
| `Visualization` | lowercase | `"off"`, `"auto"` | Verified live 2026-07 via the validation error |
| `CodeMenderMode` | lowercase | `"scan"`, `"verify"` | `agent_config.find_request.mode`, from the google-genai 2.34.0 bindings. Unverified: the `code-mender` agent config is Vertex-only (`400 ... allowed on the Gemini Enterprise Agent Platform`, live 2026-11-05) |
| Audio MIME type (TTS) | plain | `"audio/wav"` / `"audio/L16;codec=pcm;rate=24000"` | Model-dependent; see [Audio response](#audio-response-tts-output). Live 2026-09-24 |
| `Question` | tagged by `"type"` | `{"type": "noul", "instructions": "...", "criteria": {"true_description": "..."}}` / `{"type": "choice", ..., "criteria": {"id": "description"}}` / `{"type": "score", ..., "criteria": ["level 0", "level 1"]}` | All three verified live 2026-11-14; see [Decisions](#decisions) |
| `Answer` | tagged by `"type"` | `{"type": "noul", "noul": 0.97, "confidence": 0.97}` / `{"type": "choice", "choice": "a", ...}` / `{"type": "score", "score": 0.2, ...}` | All three served live 2026-11-14; see [Decisions](#decisions) |
| `BlockReason` | snake_case | `"safety"`, `"other"`, `"blocklist"`, `"prohibited_content"`, `"image_safety"` | From the google-genai 3.3.0 bindings; never served (no probe sent input a filter would block) |
| `DecisionInput` | string or `[Content]` | `"text"` / `[{"type": "text", ...}, {"type": "image", ...}]` | Both verified live 2026-11-14; one bare content block is accepted too, and the crate sends one as a one-item list |
| `VoiceType` | lowercase | `"prebuilt"`, `"prompted"`, `"replicated"` | `prebuilt`/`prompted` verified live 2026-09-24 |
| `VoicePitch` | lowercase | `"low"`, `"medium"`, `"high"` | Verified live 2026-09-24 |
| `CredentialType` | snake_case | `"bearer_token"`, `"environment_variable"`, `"oauth2"` | Create bodies are tagged by it. The first two verified live 2026-09-24 (`oauth2` create validates that `token_url` is reachable) |
| `CredentialStatus` | lowercase | `"active"`, `"revoked"` | Output only; `active` observed 2026-09-24 |
| `InjectionLocation` | lowercase | `"header"`, `"query"`, `"body"` | Sent as a list; the API also accepts a single string (2026-09-24) |
| `VideoResolution` | lowercase | `"360p"`, `"720p"`, `"1080p"`, `"4k"` | Server-validated (2026-09-24); no Interactions model outputs video to exercise it |
| `TranscriptionMode` | string or object | `"smart"` / `{"type": "smart", "source_text": "..."}` / `{"type": "verbatim", "diarization_mode": "speaker"}` | See [TranscriptionConfig](#transcriptionconfig) |
| `EnvironmentFileType` | **uppercase** | `"FILE"`, `"DIRECTORY"` | The bindings say lowercase; the API sends uppercase (live 2026-09-24). Both accepted; serializes uppercase |
| `GoogleSearchResultItem` | snake_case | `{"search_suggestions": "<html>"}` | The only field in the 2.34.0 bindings and on the wire (live 2026-07 and 2026-11-05); `title`, `url` and `rendered_content` were removed, and one that arrives lands in `extra` |
| `UrlContextResultItem` | snake_case | `{"url": "...", "status": "success"}` | `status` is `success`/`error`/`paywall`/`unsafe`; no separate paywall field (2026-01-13) |
| `FileState` | uppercase | `"PROCESSING"`, `"ACTIVE"`, `"FAILED"` | Files API |
| `FileSource` | uppercase | `"UPLOADED"`, `"GENERATED"`, `"REGISTERED"` | Files API; `UPLOADED` on every uploaded file (live 2026-11-05), the other two from google-genai 2.34.0 |
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
`InteractionResponse::input` re-serializes in the shape the server sent,
with one normalization: a single object (the spec's bare `Content` arm, or a
single step, which the crate also reads) reads as a one-element list and is
written back as a one-element array. The value is kept; only the array
around it is added (D-071). A request's `Content` input deserializes
back as `Steps(vec![Step::user_input(..)])`, since the two are identical on
the wire.

Content after history, `[...history, {"type": "user_input", "content":
[...]}]`, is accepted, and the model answers from both (live 2026-11-06,
`test_image_content_after_history`: a text history, then an image turn).
That is what `with_history()` plus `with_content()` sends (D-031).

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
| `code_execution_result` | `CodeExecutionResult` | `{"call_id", "result": "..." or [Content], "is_error": bool, "signature"?}`. `result` is `CodeExecutionOutput`; the API sends the string (see [Code execution](#code-execution)) |
| `url_context_call` | `UrlContextCall` | `{"id", "arguments": {"urls": [...]}, "signature"?}` |
| `url_context_result` | `UrlContextResult` | `{"call_id", "result": [UrlContextResultItem], "is_error"?, "signature"?}` |
| `google_search_call` | `GoogleSearchCall` | `{"id", "arguments": {"queries": [...]}, "search_type"?, "signature"?}` |
| `google_search_result` | `GoogleSearchResult` | `{"call_id", "result": [GoogleSearchResultItem], "is_error"?, "signature"?}` |
| `tool_call` | `ToolCall` | `{"id", "signature"?}`. What MCP calls arrived as on 2026-08-16; from 2026-10-01 to 2026-11-04 they arrived as answered `function_call` pairs, and since 2026-11-05 as `mcp_server_tool_call` (see MCP identity below) |
| `mcp_server_tool_call` | `McpServerToolCall` | `{"id", "name", "server_name", "arguments"}`, as in the google-genai 2.34.0 bindings. Live since 2026-11-05: `name` is `<server>:<tool>`, and a `signature` (kept in `extra`) comes with it |
| `mcp_server_tool_result` | `McpServerToolResult` | `{"call_id", "name"?, "server_name"?, "result"}`, as in the 2.34.0 bindings. Live since 2026-11-05, with `name` and `server_name` set and the same `result` object as the earlier `function_result` shape |
| `file_search_call` | `FileSearchCall` | `{"id", "signature"?}` |
| `file_search_result` | `FileSearchResult` | `{"call_id", "signature"?}`, as in the bindings and the spec (live 2026-11-05). The delta's spec-only `result` items are opaque objects, kept as JSON (see [File Search](#file-search)) |
| `google_maps_call` | `GoogleMapsCall` | `{"id", "arguments": {"queries": [...]}, "signature"?}` |
| `google_maps_result` | `GoogleMapsResult` | `{"call_id", "result": [GoogleMapsResultItem], "signature"?}` |
| `processing_call` | `ProcessingCall` | `{"id", "signature"?}`. Emitted for video with `processing: "agentic"`, one or more per turn (live 2026-09-24, `gemini-3.8-flash`). The ~36KB signature is **required** on stateless replay (`400 Processing call step is missing signature`) |
| `processing_result` | `ProcessingResult` | `{"call_id", "signature"?}`. Same replay requirement |
| `retrieval_call` | `RetrievalCall` | `{"id", "arguments": {"queries": [...]}, "retrieval_type"?, "signature"?}`. Vertex-only |
| `retrieval_result` | `RetrievalResult` | `{"call_id", "is_error"?, "signature"?}`. Vertex-only |
| anything else | `Unknown { step_type, data }` | Round-trips losslessly |

`function_call` (and, per spec, `mcp_server_tool_call`) keep arguments raw.
The built-in tool calls nest theirs in `arguments`, and the crate flattens
them into `urls`, `queries`, `language` and `code`; any other key of
`arguments` is kept in `arguments_extra` (see
[Unknown fields on known variants](#unknown-fields-on-known-variants)).

**MCP identity (#459).** Since 2026-11-05 (`gemini-3.8-flash`, live) an MCP
call arrives as an `mcp_server_tool_call` step with `server_name` and a
`name` of `<server>:<tool>`, answered by an `mcp_server_tool_result` with
the same `result` object described below; `McpCallInfo::tool` strips the
`<server>:` prefix. From 2026-10-01 to 2026-11-04 MCP calls arrived as answered
`function_call` / `function_result` steps named `<server>:<tool>` (see
`docs/INTERACTIONS_API_GAP.md`). Re-verified live 2026-11-04 on
`gemini-3.8-flash`, streamed and not: with the tool declared as
`{"type": "mcp_server", "name": "wikiprobe", ...}`, the call was named
`wikiprobe:read_wiki_structure`, so `<server>` is the declared name. The
result is an object with the tool's text under `result` and the raw MCP
`CallToolResult` as a JSON string under `call_tool_result_json`. A successful
call added `structured_content` (the tool's structured output, an object). On
three failed calls the object added `is_error: true` (matching `"isError":
true` in the JSON string) while the step's own `is_error` stayed `false`, so
`McpCallInfo::is_error` is `true` when either flag is. Streamed,
the call arrives as `step.start` (`arguments: {}`) plus `arguments_delta`, and
the result as `step.start` (no `result`) plus a `function_result` delta; no
`mcp_server_tool_*` step or delta appeared. A client function may also
contain a colon (the API accepted a declared `weather:get` the same day), so
`InteractionResponse::mcp_calls()` returns a `<server>:<tool>` call only when
the response also carries its result, and reads the spec's
`mcp_server_tool_call` / `mcp_server_tool_result` pair too.

What follows is the 2026-08-16 shape.

**MCP arrived as `tool_call`.** Verified live 2026-08-16 (`gemini-3.7-flash`,
a real MCP server, successful call). The response carried a `tool_call`
step with only `id`, `signature` and `type`, plus `total_tool_use_tokens:
1962`. There was no `mcp_server_tool_call` step, and no server name, tool
name or arguments anywhere. So `step_summary().mcp_server_tool_call_count`
reads 0 on a successful call. Count with `step_summary().tool_call_count` or
`tool_calls()`. The MCP variants are kept because nothing rejects them (unlike
`cached_content`, D-005) (#459).

### StepDelta

Deltas build up the step that the matching `step.start` announced. The tags
below that differ from the variant names were verified live 2026-11-07 on
`gemini-3.8-flash`: a grounded answer streamed its text as `text` deltas
(`{"type": "text", "text": "..."}`) and its citations as one
`text_annotation_delta` (`{"annotations": [{"start_index", "end_index",
"url", ...}]}`), and a function call streamed a `step.start` with
`arguments: {}` and one `arguments_delta` holding the whole JSON
(`{"arguments": "{\"city\":\"Tokyo\"}"}`).

| Wire `type` | Variant | Notes |
|-------------|---------|-------|
| `text` | `Text` | Text fragment |
| `image` / `audio` / `video` / `document` | media variants | `Content` field shapes (audio also accepts legacy `rate`). Each streamed image arrived whole in one `image` delta with its `mime_type` and a complete base64 JPEG, in its own `model_output` step after a `thought` step and before any text in that step (live 2026-11-04, `gemini-3.1-flash-image`, three streams, up to four images; two images in one step never appeared). An image delta with a `mime_type` or `uri` starts a new block, so the unobserved same-step case stays two blocks, and only a bare `data` fragment continues the last one. A streamed TTS answer is many `audio` deltas in one step, each with the same `mime_type` (`audio/l16`), `sample_rate` (24000) and `channels` (1) and a padding-free base64 chunk (live 2026-11-05, 243 deltas); an audio delta continues the last audio block unless it names a `uri` or a different format. Video and document deltas, never captured streamed, follow the image rule |
| `thought_summary` | `ThoughtSummary` | `{"content": Content}` |
| `thought_signature` | `ThoughtSignature` | `{"signature": "..."}` |
| `text_annotation_delta` | `TextAnnotation` | **Tag differs from the variant name.** `{"annotations": [Annotation]}` |
| `arguments_delta` | `ArgumentsDelta` | `{"arguments": "<raw JSON fragment>"}`, concatenated and parsed at `step.stop` |
| `function_result` | `FunctionResult` | `call_id` is optional (dropped from the 2.25 bindings); the accumulator keeps the one from `step.start` |
| `code_execution_*`, `url_context_*`, `google_search_*`, `file_search_*`, `google_maps_*`, `mcp_server_tool_*` | matching variants | Call deltas carry the flattened fields, plus `arguments_extra`. `mcp_server_tool_*` never appeared in a streamed MCP interaction (2026-11-04) |
| `processing_call` / `processing_result` | processing variants | `{"signature": "..."}`. `step.start` announces `signature: ""`; the value arrives only here (live 2026-09-24) |
| `retrieval_call` / `retrieval_result` | retrieval variants | Vertex-only |
| `tool_call` | *(none)* | Never observed as a delta: `tool_call` arrives whole on `step.start` |
| anything else | `Unknown { delta_type, data }` | Preserved. A `signature` on it is merged onto a same-typed `Step::Unknown` at that index, so replay keeps it |

The stream accumulator puts a delta's `extra` (and `arguments_extra`) on
what it builds: the content block for text, annotation and media deltas, the
step for the rest. The assembled `Completed` response then round-trips them
as a non-streamed one would. `interaction.completed` carried no `steps` in
every stream observed on 2026-11-04, so the accumulator is what fills them.

SSE event types in this revision: `interaction.created`,
`interaction.status_update`, `step.start`, `step.delta`, `step.stop` (with
`usage` and `step_usage`), `interaction.completed`, `error`. The pre-revision
`interaction.start`, `content.start` / `delta` / `stop` and
`interaction.complete` are gone. Unknown event types become
`StreamChunk::Unknown { chunk_type, data }`.

### StreamChunk

`StreamChunk` is what a stream yields, parsed from the SSE events above. It
is not sent by the API: this crate serializes it, for logging and replay, as
a `{"chunk_type": ..., "data": ...}` map and reads that map back.
`StreamEvent` writes the same map plus `event_id` when it has one.

| `chunk_type` | Variant | `data` |
|--------------|---------|--------|
| `created` | `Created` | The `InteractionResponse` |
| `status_update` | `StatusUpdate` | `status`, plus `interaction_id`, `usage` and `continuation_token` when set |
| `step_start` | `StepStart` | `index`, `step` |
| `step_delta` | `StepDelta` | `index`, `delta`, plus `total_usage` when set (the wire event's `metadata.total_usage`; not seen live as of 2026-11-05) |
| `step_stop` | `StepStop` | `index`, plus `usage` and `step_usage` when set |
| `completed` | `Completed` | The `InteractionResponse` |
| `error` | `Error` | `message`, `code` |
| anything else | `Unknown { chunk_type, data }` | The event's data as received; left out when null |

An `Unknown` whose `chunk_type` is spelled like a known one (a future event
the parser names `created`, say) also writes `"unknown": true`, and a map
with that marker reads back as `Unknown`, so the chunk can't turn into the
known one on a round trip. The marker is written only on such a collision;
maps written by 0.12.0 and earlier have none and read back as before.

Every known variant also keeps the SSE event's keys it does not read, in
its `extra` map: a key no event type models (one the API adds beside the
payload), a modeled key on an event type that does not read it (an `index`
on a `status_update`, kept as it came and not parsed), and what is left of `metadata` beside its
`total_usage`, or of `error` beside its `message` and `code`, under that
key. The map writes a non-empty `extra` beside `data`, as `"extra": {...}`,
and reads it back; maps without it read back with an empty one.

### Annotation

A union tagged by `"type"` (previously a single struct). Each known variant
keeps unmodeled fields in `extra`:

| Wire `type` | Variant | Fields |
|-------------|---------|--------|
| `url_citation` | `UrlCitation` | `url`, `title` |
| `file_citation` | `FileCitation` | `document_uri`, `file_name`, `source`, `custom_metadata`, `page_number`, `media_id` |
| `place_citation` | `PlaceCitation` | `place_id`, `name`, `url`, `review_snippets: [ReviewSnippet { title, url, review_id }]` |
| `speech_metadata` | `SpeechMetadata` | `speaker`, `style`; indices optional. A TTS **input** annotation: required per turn for multi-speaker on `gemini-3.8-flash-tts`, rejected by older TTS models. Spanned annotations must tile the text without gaps (live 2026-09-24) |
| `word_info` | `WordInfo` | `text`, `speaker`, `start_offset` / `end_offset` (duration strings), optional indices. Transcription output: one per word on a transcript from `gemini-3.5-transcribe` with word timestamps requested, as `{"start_index": 0, "end_index": 3, "text": "The", "start_offset": "0.100s", "end_offset": "0.100s", "speaker": "spk:0", "type": "word_info"}` (live 2026-11-07); a whole second is written `"4s"` |
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
| `ToolChoice::AllowedTools { allowed_tools, extra }` | `{"allowed_tools": {"mode"?, "tools": [...]}}`, plus any `extra` keys |
| `ToolChoice::Unknown { choice_type, data }` | Unrecognized shapes |

An unknown mode **string** becomes `ToolChoice::Mode(FunctionCallingMode::Unknown)`;
an unknown **shape** becomes `ToolChoice::Unknown`. `AllowedTools` is also the
element type of an MCP server's `allowed_tools`. The pre-revision uppercase
modes (`"AUTO"`) deserialize to `Unknown`.

Verified live 2026-11-07 on `gemini-3.8-flash`, every mode and the
`allowed_tools` object sent in the forms above: a stored read serves
`tool_choice` back as sent (`"none"`, or the whole `allowed_tools` object),
and a create response carries no `generation_config`. What each mode does,
and the API's answer to an uppercase one, are under Tools in
`docs/INTERACTIONS_API_GAP.md`.

### ServiceTier

`service_tier` on the request: `"flex"` / `"standard"` / `"priority"` /
`"deferred"`. Every response echoes the effective tier along with
`object: "interaction"`; both are modeled on `InteractionResponse`. `flex`
and `priority` are accepted and echoed (live 2026-11-01). `deferred` is
Vertex-only: `400 The value 'deferred' is not supported for 'service_tier'
on the Gemini API, it is allowed on the Gemini Enterprise Agent Platform`.

### Safety settings

| Enum | Wire values |
|------|-------------|
| `HarmCategory` | `hate_speech`, `dangerous_content`, `harassment`, `sexually_explicit`, `civic_integrity`, `image_hate`, `image_dangerous_content`, `image_harassment`, `image_sexually_explicit`, `jailbreak` |
| `SafetyThreshold` | `block_low_and_above`, `block_medium_and_above`, `block_only_high`, `block_none`, `off` |
| `SafetyMethod` | `severity`, `probability` |

Each setting is `{"type", "threshold", "method"?}`. The Gemini API accepts
`safety_settings` as of 2026-10-30 (it was Vertex-only through 2026-10-08)
and validates every value. `civic_integrity`, the `image_*` categories and
`block_none` are rejected as Vertex-only (400 "...allowed on the Gemini
Enterprise Agent Platform"). The crate sent the category under `category`
until 0.12.0; the API never accepted that key, and the old whole-parameter
gate hid the mistake.

### EnvironmentStatus

`"active"` (verified live 2026-08-08) and `"expired"` (spec). In the same
probe, int64 counts arrived as protobuf-JSON strings and timestamps as ISO 8601
with an offset.

Verified live 2026-09-26 (list paging):

- `GET /v1beta/environments` returns the newest environment first, 50 per
  page by default (`page_size=1000` returned all 111 on the key in one page).
  A page can be short or empty while a `next_page_token` remains: at
  `page_size=1` one page mid-list was `{"environments": [], "next_page_token":
  ...}`, and at the default size the first page held 49. The last page omits
  the token. Tokens are URL-safe base64.
- `GET /v1beta/environments/{id}/files/{path}` honors `page_size`; its
  `next_page_token` is a decimal offset (`"1"`, `"2"`, ...) and is omitted on
  the last page. A token sent without the `recursive=true` of the first
  request answers `{}`, so `recursive` must ride on every page. A
  non-numeric token is `400` (`Invalid page_token.`), and an offset past the
  end is `{}`.
- Uploading to an existing path without `overwrite=true` fails on the
  finalizing request, after the bytes are sent: `409` with
  `{"error": {"message": "Requested entity already exists", "code": "aborted"}}`.

### TriggerStatus and TriggerExecutionStatus

| Enum | Wire values |
|------|-------------|
| `TriggerStatus` | `active`, `paused`, `error` |
| `TriggerExecutionStatus` | `in_progress`, `completed`, `failed`, `skipped`, `timed_out` |

The values come from the SDK spec; `active`, `paused` and `completed` are
verified live (2026-11-01, the full lifecycle against a custom agent).
Verified live 2026-08-08: a model-only trigger interaction is rejected
("Agent '' is invalid or not found"), and an empty `GET /v1beta/triggers`
returns `{}`. The live wire uses the spec spellings; these hedges remain
for robustness:

- `create_time` / `update_time` (the spec) also accept the Environments
  spelling `created` / `updated`.
- The executions list reads `trigger_executions` (the spec) or `executions`
  (the path segment).
- All trigger-family timestamps go through a lenient RFC 3339
  deserializer. An unexpected shape degrades to `None` with a `warn!`, rather
  than failing the list.

Verified live 2026-09-26 (list paging, no triggers on the key):

- `GET /v1beta/triggers?page_size=2` returns `{}`, the end of the list.
  An unrecognized `page_token` is a `400 INVALID_ARGUMENT`.
- `GET /v1beta/triggers/{id}/executions` returns `{}` for a trigger that does
  not exist, not a 404; `GET /v1beta/triggers/{id}` on the same ID is a
  `404 NOT_FOUND`. `POST .../executions` (run now) on it is a structured
  `404 NOT_FOUND`, while an unknown sub-collection is an empty-body 404, so
  the executions path exists server-side. Execution shapes are verified
  live as of 2026-11-01.

### ThinkingSummaries

Sent as `"auto"` / `"none"` in both `generation_config.thinking_summaries`
and `agent_config.thinking_summaries`. `THINKING_SUMMARIES_AUTO` /
`_NONE` are still accepted on deserialize.

The agent-config spelling reversed. On 2026-01-04 the API rejected `"auto"`
(`unknown enum value: 'auto'`), so the crate sent `THINKING_SUMMARIES_*`
there. On 2026-08-10, verified live, the reverse:
`The value 'THINKING_SUMMARIES_AUTO' is not supported for 'agent_config.thinking_summaries'. Supported values: 'auto', 'none'.`

### ThinkingLevel

`"minimal"`, `"low"`, `"medium"`, `"high"` and `"xhigh"` in
`generation_config.thinking_level`. Which levels `DEFAULT_MODEL` accepts,
the error it answers `"minimal"` with, which models take `"xhigh"` and what
omitting the field does are under Models in `docs/INTERACTIONS_API_GAP.md`.
`"xhigh"` is new in google-genai 3.3.0.

### InteractionStatus

| Variant | Wire value | Notes |
|---------|------------|-------|
| `Completed` | `completed` | |
| `InProgress` | `in_progress` | `Default` |
| `RequiresAction` | `requires_action` | |
| `Failed` | `failed` | |
| `Cancelled` | `cancelled` | |
| `Incomplete` | `incomplete` | Live 2026-11-01: `max_output_tokens` reached; carries a `continuation_token` on the continuation model |
| `BudgetExceeded` | `budget_exceeded` | Never observed live. The v1beta spec marks the value deprecated, and v1 drops it. How an Antigravity run past its `max_total_tokens` ended is under Agents and environments in `docs/INTERACTIONS_API_GAP.md` |
| `RequiresContinuation` | `requires_continuation` | google-genai 2.34.0; never observed live |
| `Queued` | `queued` | google-genai 2.34.0 bindings, not the spec (#501); never observed live: background interactions report `in_progress` from their create response on (2026-11-05) |

### Resolution

`resolution` on image and video content: `"low"`, `"medium"`, `"high"`,
`"ultra_high"`. Verified 2026-01-05 (pre-revision) with `LOUD_WIRE=1`.

### VideoProcessing

`processing` on video content:

| Variant | Wire value |
|---------|------------|
| `Static` | `"static"` |
| `Agentic` | `"agentic"` |
| `StaticSegment { start_offset, end_offset, fps, extra }` | `{"type": "static", "start_offset": "10.5s", "end_offset": "30s", "fps": 1.0}`; other keys go to `extra`, and a mistyped field makes it `Unknown` |
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

### Files

`FileState` is a file's bare uppercase `state` (`PROCESSING` / `ACTIVE` /
`FAILED`). The Files API is camelCase, paging included: `pageSize` and
`pageToken` go out, `nextPageToken` comes back. The resource is in
`src/files.rs`.

Verified live 2026-09-27 (list paging, three text files at `pageSize=1`):

- The list is newest first (by `createTime`), one file per page at
  `pageSize=1`. The last page omits `nextPageToken`, even when it is exactly
  full.
- `nextPageToken` is unpadded URL-safe base64 that encodes the last listed
  file's ID. It is not bound to the page size: the same token with another
  `pageSize`, or none, continues the list. An unrecognized token is a 400
  (`Requested page_token is invalid.`).
- `pageSize` must be 1 to 100; `5000` is a 400 (`page_size must be between
  1 and 100`). `0` is the default. The snake_case `page_size` is accepted
  too; the crate sends `pageSize`.
- Files carry `updateTime` and `source` (`UPLOADED`), both modeled since
  0.14.0 (live 2026-11-05). `sizeBytes` is a JSON string (`"17"`);
  `FileMetadata::size_bytes` is an `Option<i64>` that reads a string or a number.
- A file that fails processing (an undecodable video) carries
  `{"error": {"code": 3, "message": "The file failed to be processed."}}`
  with no `details` (live 2026-11-05). `FileError` models `details` and
  keeps any other key in `extra`.

### File Search

**The store and document resources are camelCase**, unlike the
Interactions API.

```json
{"name": "fileSearchStores/my-docs-4kws71n2ybpr", "displayName": "my-docs",
 "createTime": "2026-08-16T15:13:13.783782Z", "updateTime": "2026-08-16T15:13:13.783782Z",
 "embeddingModel": "models/gemini-embedding-001"}
```

- List envelopes are `{"fileSearchStores": [...]}` and `{"documents": [...]}`.
  An empty store list, or an empty store's document list, is a bare `{}`.
  `page_size` and `pageSize` are both accepted; the crate sends `pageSize`.
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
  `file_search_stores().documents().wait_until_active()`.
- **Deleting a non-empty document or store needs `force=true`**. Otherwise:
  `400 Cannot delete non-empty Document` / `... FileSearchStore`
  (`FAILED_PRECONDITION`).
- **Uploads accept `raw`, `multipart` and `resumable`.** The crate used
  `raw`, with `display_name` as a query parameter, until 0.14.0; it now uses
  `resumable`, as the Files API does, because only a JSON body carries
  `customMetadata` and `chunkingConfig` (live 2026-11-05). The start request's
  body is `{"displayName", "mimeType", "customMetadata", "chunkingConfig"}`;
  a body `mimeType` wins over the session's content-type header.
- **The upload response is an operation wrapper**:
  `{"name": ".../upload/operations/...", "response": {"documentName": ...}}`.
  The crate resolves it with a follow-up GET.
- **`file_search_result` steps carry no chunks.** Only `call_id`,
  `signature` and `type` arrive, on a stored interaction and in the stream's
  delta (live 2026-11-05). The retrieved content shows only in the answer,
  cited by `file_citation` annotations (#429). The spec gives the delta a
  `result` list of objects with no fields; `StepDelta::FileSearchResult`
  keeps any it receives as JSON, and the accumulator puts them in the
  step's `extra` under `result`.
- **`file_search` can't be combined with `google_search` or
  `url_context`**; the error and its date are under Tools in
  `docs/INTERACTIONS_API_GAP.md`.

Paging and uploads, verified live 2026-09-27 with three stores and three
documents:

- Both lists are **oldest first** (creation order). `page_size` must be 1 to
  20 for both (`21` is a 400, `page_size must be between 1 and 20`); `0` is
  the default. The last page omits `nextPageToken` even when exactly full.
- Tokens are unpadded URL-safe base64 of the last item's creation time and
  ID, and are not bound to the page size (the same token with another
  `page_size`, or none, continues the list). An unknown token is a 400
  `Requested page_token is invalid.`; a document token sent to another store
  lists nothing (`{}`).
- **Uploads take any body**, so in-memory bytes upload exactly as a file
  does. Without `display_name`, the document is named after the
  `X-Goog-Upload-File-Name` header (`"notes.txt"` becomes the display name
  and the ID prefix `notestxt-`); with neither, it has no display name and a
  random ID. With both, `display_name` wins. The crate sends the header for
  path uploads only, so an unnamed in-memory upload stays unnamed. The
  resumable protocol names documents the same way (live 2026-11-05).
- Deleting an empty store needs no `force`; a non-empty one is a 400
  `Cannot delete non-empty FileSearchStore`.

Stores, documents and imports, verified live 2026-11-05:

- A store serves `activeDocumentsCount`, `pendingDocumentsCount` (only while
  a document indexes) and `sizeBytes` as string int64s; an empty store has
  none of them. `failedDocumentsCount` was not seen: a document that cannot
  be indexed (random bytes as `application/pdf`) fails its upload with a 500
  instead.
- `embeddingModel` on create picks the model (`models/gemini-embedding-2`);
  unset, it is `models/gemini-embedding-001` (the server's default, which
  the live test checks only as a `models/` name), and an unknown model is a
  `400 INVALID_ARGUMENT`.
- `customMetadata` entries are `{"key", "stringValue" | "numericValue" |
  "stringListValue": {"values": [...]}}`, served back on the document in the
  order sent. A whole `numericValue` comes back without a fraction (`2026`).
- `chunkingConfig` is `{"whiteSpaceConfig": {"maxTokensPerChunk",
  "maxOverlapTokens"}}`. An unknown key is a `400 Invalid JSON payload`; an
  overlap as large as the chunk or larger fails the upload or import with a
  500, and creates no document. The crate rejects such an overlap before any
  request.
- `POST {store}:importFile` with `{"fileName": "files/<id>", "customMetadata",
  "chunkingConfig"}` answers with an unfinished operation
  (`{"name": "fileSearchStores/<s>/operations/<id>", "response": {"@type":
  ...}}`). `GET` on that name reports `"done": true` and the document's
  **bare** ID in `response.documentName` (and the store's bare ID as
  `parent`). The document is named after the file's ID and is a copy. Its
  ID is the operation's, and it exists, `STATE_PENDING`, as soon as the
  `POST` answers; importing the same file again makes a second document
  under a new ID.

Covered by `tests/file_search_stores_tests.rs` and `examples/file_search.rs`.

### Google Search steps

Verified live 2026-07 with `LOUD_WIRE=1`, and again 2026-11-05:

- `google_search_call` carried `id`, nested `arguments.queries`,
  `search_type: "web_search"` and `signature`.
- The `google_search_result` items carried **only** `search_suggestions`
  (an HTML rendering payload), the one field of the bindings'
  `GoogleSearchResult`. `GoogleSearchResultItem` models only that.
- Citations arrived as `url_citation` annotations on the `model_output`
  text, and `usage.grounding_tool_count` was `[{"type": "google_search",
  "count": 1}]`.

### Google Maps steps

- `google_maps_call`: `id`, `arguments.queries`, `signature`.
- `google_maps_result`: `call_id`, `result: [GoogleMapsResultItem]`,
  `signature`. Each item has `places: [Place]` and `widget_context_token`.
  A place carries `name`, `place_id` and `url` (live 2026-11-05, 30 places
  across three results), and the bindings add `review_snippets`. The
  older address, coordinate, rating and contact fields were never served
  and were removed from `Place`; one that arrives lands in `extra`.

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

Verified live 2026-11-07 on `gemini-3.8-flash`, on a standard API key, as
it has answered since 2026-08-16 (it was allowlisted before). The tool
above, every field set, was accepted, and a stored read served it back
exactly. Asked to read a live page, the model answered `requires_action`
with a `thought` step and one `function_call` step, `navigate` with
`arguments: {"url": "https://news.ycombinator.com", "intent": "Navigate to
Hacker News"}`; with `environment` `"mobile"` or `"desktop"` the first
action was `take_screenshot` (`arguments: {"intent": ...}`).

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
  2026-09-24). The 400s that day: transcript text without annotations on a
  3.8 TTS model, `Multi-speaker interactions must specify a speaker for each
  text turn`; annotations on the 2.5-pro or 3.1-flash TTS models, `Speech
  annotations are not supported for model ...`; a speaker missing from
  `speech_config`, `... must specify a speaker matching a speaker defined in
  speech_config`; spanned annotations with a gap, `... must cover the entire
  text string without gaps`.
- The nested `voiceConfig.prebuiltVoiceConfig.voiceName` form from Google's
  docs returns 400 (`no such field: 'voiceConfig'`, 2026-01-10).
- A stored interaction read back serves `generation_config`, with
  `speech_config` in the `{"speakers": [...]}` form whichever form was sent
  (live 2026-11-04 and 2026-11-05, `gemini-3.8-flash-tts`, sent as the list
  form). `InteractionResponse::generation_config` reads it into the same
  list, and serializes it back as the list. With `include_input=true` the
  input comes back as a `user_input` step, and every list in the stored
  config comes back twice over: each `speakers` entry, `stop_sequences`
  value, `response_modalities` value and `safety_settings` entry appears
  twice (live 2026-11-04 and 2026-11-05, on the TTS model and on
  `gemini-3.8-flash`). The crate keeps what is served and does not
  deduplicate. Neither was echoed in 2026-07.

#### speech_config wire forms

| Form sent | 2026-08-16 (`gemini-2.5-pro-preview-tts`) | 2026-09-24 (all TTS models) |
|------|------|------|
| `[{voice, language, speaker}, ...]` | accepted | accepted |
| `{"speakers": [...]}` | `400 ... Expected an array, got object.` | accepted (mapped to `structured_speech_config`) |
| bare `{voice, language}` | `400 ... Expected an array, got object.` | `400 Unknown parameter 'voice' at 'generation_config.structured_speech_config'` |

The crate **sends the list**. On deserialize it accepts all three
forms and normalizes them to a list, since a `GenerationConfig` nested in a
`Trigger` may have been written by another SDK. The keys a
`{"speakers": [...]}` form carries beside the list (none today) are kept in
`GenerationConfig::speech_config_extra`, and while it holds one the config
is written in that form, which the API accepts, so they go back out
(D-072). For maintainers: the untagged `{"speakers": [...]}` arm must
be tried before the single-object arm. Every `SpeechConfig` field is
optional, so the single arm would match it and silently drop the speakers.

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
- `url_context_result`: `{"call_id", "is_error", "result": [{"url",
  "status"}]}`. `UrlContextResultItem` has `is_success()`, `is_error()`,
  `is_unsafe()` and `is_paywall()`.

The step form verified live 2026-11-07 (`gemini-3.8-flash`, one fetch of
an IANA page): a `url_context_call` with `arguments.urls` holding the URL,
then a `url_context_result` with `is_error: false` and one `{"status":
"success", "url"}` item, then the answer, its text cited with
`url_citation` annotations on that URL. The shapes were first verified on
2026-01-09, pre-revision.

### Code execution

`code_execution_result` is `{"call_id", "is_error": bool, "result",
"signature"?}`. The official docs describe an `outcome` enum
(`OUTCOME_OK`), but the wire uses `is_error` + `result` (found with
`LOUD_WIRE=1`, 2026-01-12, pre-revision). `call_id` is required since
2026-05-20.

`result` is a union in the google-genai 3.0.0 bindings: the output as a
string, or a list of content blocks. `CodeExecutionOutput` reads both and
writes each back in the form it came in. The Gemini API sent the string on
every run probed on 2026-11-07 (`gemini-3.8-flash`; the plot also on
`gemini-3.5-flash`, which the response names `gemini-3.6-flash`):

| Run | Read through | `result` |
|---|---|---|
| A plain run (stdout) | create | the stdout |
| A matplotlib plot | create, stored `GET`, stream | `""` |
| A run that printed a CSV and saved a chart | create | the printed CSV |
| A run printing 2**200 and 40 Fibonacci numbers (272 chars) | stream, stored `GET` | the stdout |
| A run printing 1,500 lines (31,559 chars) | stream | the stdout |

What a run makes is not in `result`: the plot arrived as an `image` block
(`image/png`) in the next `model_output` step, and the CSV run's next step
held the saved chart's image and the CSV as a `text` block. Streamed,
`step.start` carries `call_id` and `signature: ""`, then a single
`code_execution_result` delta carries the whole `result` as a string, with
`is_error`; the 31,559-character output was not split. The delta's `result`
is a string in the bindings too. What the API does with the list form sent
back in history is under Tools in `docs/INTERACTIONS_API_GAP.md`.

`CodeExecutionLanguage` is lowercase `"python"` in
`code_execution_call.arguments.language` (`{"code": "...", "language":
"python"}`, live 2026-11-07). `"PYTHON"` (pre-revision) now deserializes to
`Unknown`, and `Display` prints `"python"`.

### SearchType

`Tool::GoogleSearch.search_types` and `google_search_call.search_type`.
Verified live 2026-11-07: a `google_search_call` carries `search_type:
"web_search"` when the request sent `["web_search"]` or no `search_types`,
and `"image_search"` (after a `web_search` call) when it sent
`["image_search"]` to a model that takes it. Which models take
`image_search`, and that `enterprise_web_search` is Vertex-only (kept,
D-005), are under Tools in `docs/INTERACTIONS_API_GAP.md`.

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

Verified live 2026-09-26 (list paging, three webhooks at `page_size=1`):

- `page_size` is honored, and `next_page_token` is present only while more
  webhooks remain; the last page omits it. An empty collection is `{}`.
- The token is raw bytes with control characters, not base64 (shape
  `"rC\n\x19B\x17<name>\n&B$<id>"`), so it must be percent-encoded byte for
  byte when resent. The `webhooks_and_agents_tests` paging test covers this.

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
- The remote object's own `environment_id` runs an interaction in that
  existing environment, with its files; the response's `environment_id` is
  that ID, and one that does not exist is a 404. On an agent's
  `base_environment` it is accepted, dropped and unchecked (live
  2026-11-05).

Verified live 2026-07 (Antigravity agent, `background: true`): the inline
source, `"disabled"`, an allowlist with `transform`, and the id string form.
`gcs`, `repository` and `skill_registry` sources and `base_environment` were
not exercised.

### Credentials

`CredentialType` tags create bodies (`{"type": "bearer_token", "token": ...}`)
and is required on updates; the credential resource itself is in
`src/credentials.rs`.

Verified live 2026-09-27 (IDs and list paging, three bearer credentials at
`page_size=1`):

- IDs are letters, digits and hyphens, up to 63 characters. Create rejects
  anything else with a 400 (`Credential ids should be alphanumeric with
  hyphens, up to 63 characters.`), and a `GET` on a percent-encoded
  `credentials/<id>` resource name gets the same 400.
- The list runs in creation (and ID) order, one credential per page at
  `page_size=1`. `next_page_token` is unpadded URL-safe base64 that encodes
  the last listed ID, and the last page omits it. An unrecognized token is a
  400 (`Precondition check failed.`). An empty collection is `{}`.

### Voices

`VoiceType` is the voice's `type` field and the `type` list filter;
`VoicePitch` is its `pitch` field and filter. The resource is in
`src/voices.rs`.

Verified live 2026-09-27 (list paging and IDs):

- The catalog lists in ID order, 50 voices per page by default. The last
  page has no `next_page_token`, even when it is exactly full. A query that
  matches nothing is `{}`.
- `next_page_token` is unpadded URL-safe base64 of a value that ends in the
  last listed voice's `<id>|<language_code>`. It is bound to the filters:
  sending it with a filter dropped or changed is a 400 (`Filter parameters do
  not match the filter parameters used in the request that returned this
  page_token.`), while `page_size` may change. An unrecognized token is a
  400 (`Invalid page token.`).
- A filter repeated with several values (`gender=female&gender=male`)
  matches any of them. The crate sends one value per filter.
- `GET /v1beta/voices/{id}` finds stored custom voices (`voice_...`) only.
  Prebuilt IDs (`achernar`, `kore`, `en-gb-storyteller-1`), an upper-cased
  custom ID and a percent-encoded `voices/<id>` resource name are 404s.

Verified live 2026-11-05: a create's `description`, `language_code`,
`region_code`, `accent`, `persona`, `context`, `gender` and `pitch` are
stored, echoed and matched by the list filters. `sample_audio` is output
only: a value sent is ignored, and the response carries a sample the API
generated.

### Decisions

`POST /v1beta/decisions` (`client.decisions().create()`, `src/decisions.rs`),
verified live 2026-11-14 on `DEFAULT_DECISION_MODEL`. The request is flat
snake_case:

```json
{"model": "gemini-decision-1",
 "input": "Review: the soup arrived cold.",
 "questions": {
   "positive": {"type": "noul", "instructions": "Is the review positive?",
                "criteria": {"true_description": "liked", "false_description": "disliked"}},
   "topic": {"type": "choice", "instructions": "About?",
             "criteria": {"food": "The food itself", "service": ""}},
   "stars": {"type": "score", "instructions": "Stars?",
             "criteria": ["zero stars", "one star", "two stars", "three stars"]}}}
```

The response keys each answer by its question's ID, with the question's
type as the answer's `type`:

```json
{"answers": {
   "positive": {"type": "noul", "noul": 0.0298, "confidence": 0.9702},
   "topic": {"type": "choice", "choice": "service", "confidence": 0.6232,
             "probabilities": {"food": 0.3636, "service": 0.6232, "price": 0.0133}},
   "stars": {"type": "score", "score": 0.2112, "confidence": 0.8055,
             "probabilities": {"0": 0.8055, "1": 0.1810, "2": 0.0104, "3": 0.0031},
             "legend": {"0": "zero stars", "1": "one star", "2": "two stars", "3": "three stars"}}},
 "usage": {"total_tokens": 249, "total_input_tokens": 249,
           "input_tokens_by_modality": [{"modality": "text", "tokens": 249}],
           "total_output_tokens": 0}}
```

- `noul` is `P(true)`, and its `confidence` is `max(noul, 1 - noul)`. A
  choice's `confidence` is its `choice`'s probability; a score's is its
  highest level probability, and `score` is the expected level, each level
  index times its probability, summed.
- A score's `probabilities` and `legend` are keyed by the level's index as
  a string. `Answer` reads an absent field as its zero (protobuf JSON omits
  zeros); a field of the wrong type, or an unknown `type`, keeps the answer
  whole as `Answer::Unknown`.
- `input` was taken as a string, one text block and a list of a text block
  and an inline image, and `instructions` as a string, one text block and a
  list of them. The crate sends `instructions` as a string.
- `prompt_feedback` (`block_reason`, `block_reason_message`) was not
  served; its shape is the bindings'.

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

`{"agent_config": {"type": "antigravity", "model": "gemini-3.8-flash", "max_total_tokens": 200000}}`.
Verified live 2026-10-01 on `antigravity-preview-09-2026`, which requires an
`environment`. The validation error lists the `agent_config.type` values
`dynamic`, `deep-research`, `code-mender` and `antigravity`. `model` is
validated per agent: `gemini-3.6-flash` and `gemini-3.5-flash-lite` were
accepted, an unknown value returns 404 `not_found`. (On
`antigravity-preview-05-2026`, `gemini-3.6-flash` returned 404.) The agent
defaults to `gemini-3.8-flash`, and Google's Antigravity Agent guide lists
the 3.8, 3.7, 3.6 and 3.5 Flash models and `gemini-3.5-flash-lite`; only the
two above were probed.

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
`{"type": "smart", "source_text"?}` / `{"type": "verbatim",
"diarization_mode"?, "timestamp_granularities"?}`. The crate sends the
object form and reads both. `source_text` is new in google-genai 3.0.0; a
stored interaction serves it back inside `mode` as sent (2026-11-07). The
bindings' `language_hints` is not modeled: `400 Unknown parameter
'language_hints'`. Verified live 2026-09-24 with audio input (the enum is
server-validated: `Invalid enum value 'zzz'`). What each setting does, and
on which model, is under Models in `docs/INTERACTIONS_API_GAP.md`.

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

## Structs, enums and `#[non_exhaustive]`

Response and resource structs are `#[non_exhaustive]`, so the crate can add
fields without a breaking change (D-002). `tests/non_exhaustive_responses.rs`
fails the build when a deserializable public struct lacks the attribute and
isn't in its `REQUEST_SIDE` exemption list. The rule:

- **Closed**: anything the API returns (`InteractionResponse`,
  `UsageMetadata`, the `*ListResponse` wrappers, result items). That includes
  read-write resources (`Agent`, `Environment`, `Trigger`, `Webhook`), where
  constructors like `Agent::new(id, base_agent)` cover the sending side. Create bodies
  that ship builders (`CreateFileSearchStoreRequest`) are closed too.
- **Open**: request types the user assembles and the API never returns
  (`GenerationConfig`, `FunctionDeclaration`, the tool configs).

Outside the crate the attribute blocks struct literals and
`..Default::default()`, but not `T::default()` plus field assignment:

```rust
# use genai_rs::{InteractionResponse, InteractionStatus};
let mut response = InteractionResponse::default();
response.status = InteractionStatus::Completed;
# assert_eq!(response.status, InteractionStatus::Completed);
```

Types with neither `Default` nor a constructor need a JSON fixture:
`FileMetadata`, `FileError`, `VideoMetadata`, `FileListResponse`,
`FileUploadResponse` and `AutoFunctionResult`.
(`ModalityTokens::new()`, `StreamEvent::new()` and
`FunctionCallInfo::to_owned()` cover the others without `Default`.)

The same file holds enums to the Evergreen rules (`enums_are_evergreen`,
D-034): every public enum is `#[non_exhaustive]`, every deserializable one
has an `Unknown` variant, and every known variant of such a union keeps an
`extra` map. A newtype variant always needs a reason, since the guard cannot
see whether keys sit beside its value on the wire, as they did beside the
old `ToolChoice::AllowedTools(AllowedTools)`. Exemptions are named with
their reasons in `EXHAUSTIVE_ENUMS`, `WITHOUT_UNKNOWN` and
`VARIANTS_WITHOUT_EXTRA`. The enums `wire_enum!` and the Antigravity
`wire_string_enum!` generate have no source to parse, so the guard checks
the macros' templates for `#[non_exhaustive]` and an `Unknown` variant.

## Antigravity harness protocol (feature `antigravity`)

`genai_rs::antigravity::protocol` speaks the localharness proto-JSON
protocol (see [ANTIGRAVITY.md](ANTIGRAVITY.md)). It was verified against the
descriptor set and a live harness from the `google-antigravity` 0.1.23 wheel
(the full harness suite, plus a descriptor diff against 0.1.18), and earlier
against 0.1.18, 0.1.10 and 0.1.5.

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
| `TrajectoryState` | `state_type` | `STATE_RUNNING`, `STATE_FULLY_IDLE` (terminal; alias `STATE_IDLE`), `STATE_WAITING_FOR_TASKS`, `STATE_WAITING_FOR_SUBAGENTS` (0.1.23), `STATE_CANCELLED` |
| `StopReason` | `reason_type` | `STOP_REASON_MAX_MODEL_CALLS_EXCEEDED`, `..._MAX_TOOL_CALLS_...`, `..._MAX_{INPUT,OUTPUT,TOTAL}_TOKENS_EXCEEDED`, `STOP_REASON_QUOTA_EXHAUSTED` (0.1.18) |
| `ModelType` | `model_type` | `MODEL_TYPE_TEXT`, `MODEL_TYPE_IMAGE` |
| `Modality` | `modality_type` | `TEXT`, `IMAGE`, `VIDEO`, `AUDIO`, `DOCUMENT`: bare, unlike `MODALITY_UNSPECIFIED` (0.1.18) |
| `AgentBehavior` | `behavior_type` | `AGENT_BEHAVIOR_AUTONOMOUS`, `AGENT_BEHAVIOR_INTERACTIVE`, `AGENT_BEHAVIOR_MINIMAL` (0.1.18; client → harness) |
| `LifecycleHook` | `hook_type` | `LIFECYCLE_HOOK_PRE_TOOL`, `LIFECYCLE_HOOK_POST_TOOL`, ..., `LIFECYCLE_HOOK_ON_COMPACTION`, `LIFECYCLE_HOOK_STOP` (0.1.18) |
| `HookDecision` | `decision_type` | `ALLOW`, `DENY` |
| `LineAction` | `action_type` | `LINE_ACTION_INSERT`, `LINE_ACTION_DELETE`, `LINE_ACTION_NONE` |

An unrecognized value lands in `Unknown` and is written back as it came,
under the same rule as `wire_enum!`'s: a non-string value (a number, an
object, `null`) is typed `<non-string: ...>` and serializes as the original
JSON, not as a string.

The envelope oneofs have Unknown variants too (`InputEvent::Unknown` and
`OutputPayload::Unknown`, with `event_type` + `data`). Harness-emitted
structs (`StepUpdate`, `ToolCall`, `UsageMetadata`, action submessages, ...)
keep unrecognized fields in a flattened `extra` map. Strict mode does
not apply here: the harness protocol is internal and unstable, so soft
typing is always on.

**Alias spellings are a deliberate exception to round-trip fidelity (D-003).**
These enums accept a value renamed between harness revisions, and
`as_wire_str` re-emits the current spelling: `STATE_IDLE` in,
`STATE_FULLY_IDLE` out. They are inbound-only (the client never sends a
`TrajectoryState`), so the asymmetry never reaches the wire. What it buys is
one build that *reads* either revision. Preserving the old spelling as
`Unknown` instead is what broke 0.1.5 → 0.1.10: only `Idle` ends a turn, so
every turn ran to its timeout. (Outbound shapes can't be aliased: 0.1.18
changed `userInput`, and 0.1.23 rejects nothing this build sends but 0.1.18
rejects its `runWorkflow` toggle, so this build *drives* 0.1.23 only.)
