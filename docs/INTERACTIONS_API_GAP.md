# Interactions API Gap Tracker

> ## ⚠️ This is a point-in-time snapshot, not a completeness guarantee
>
> | | |
> |---|---|
> | **Last swept against** | `google-genai` **2.34.0** |
> | **Sweep date** | **2026-11-01** |
> | **Baseline in CI** | `.github/last-swept-sdk-version` |
>
> "Done" below means *nothing was missing as of the sweep date*. The SDK
> ships new surface between sweeps. The daily `api-surface-sweep` workflow
> opens an issue when the bindings move past the baseline and closes it once
> a sweep catches up, so an open issue means this file is behind.

## How to verify API surface

| Rank | Source | Why |
|------|--------|-----|
| 1 | **Generated bindings**: `google-genai`'s `_gaos/types/**` and the endpoint `path=` lines in `_gaos/*.py` | Ships ahead of prose. Diff two releases to find new surface. |
| 1b | **OpenAPI specs**: `https://ai.google.dev/static/api/interactions.openapi.json` (v1beta) and `interactions-v1.openapi.json` (v1) | Machine-readable, and agrees with the bindings where both speak; found 2026-11-01. |
| 2 | **Live probes** against `generativelanguage.googleapis.com` | Ground truth, and it disagrees with rank 1 in both directions. |
| 3 | **Prose docs**: `ai.google.dev`, and this file | Both lag. Neither is evidence of absence. |

Rank 2 disagrees with rank 1 in both directions, which is why nothing from
the bindings is modeled until it is probed. From the 2.25.0 sweep:

- **Bindings list surface the API rejects:** `cached_content` and
  `transcription_config.language_hints` (`400 Unknown parameter`), the
  `service_tier: "deferred"` value, and the string arm of `environment.env`.
- **The API accepts surface the bindings lack:** the tool types
  `filesystem`, `tool_search`, `manage_task`, `schedule`, `bash`; the
  annotation types `in_context_file_citation` and `reference_metadata`; and
  extra usage and environment fields.

## Protocol facts (live, 2026-09-24)

- **`Api-Revision` is ignored.** Any value, garbage, or no header returns
  the 2026-05-20 steps protocol, streaming included. The bindings still pin
  `2026-05-20`; the crate keeps sending it.
- **Inline video works on every current model.** The earlier "3.7-flash
  rejects inline video" came from the 0.2s test fixture. A clip that yields
  no sampled frame at the default ~1 fps gets a generic `400 Request contains
  an invalid argument`. A clip of 1s or longer, or a higher `processing.fps`,
  passes. `INLINE_VIDEO_MODEL` is gone (D-012).
- **`speech_config`:** the object form `{"speakers": [...]}` is now accepted
  on every TTS model (it was rejected on 2026-08-16). A bare `{voice,
  language}` object is still rejected. The crate sends the list, which works
  everywhere.
- **`labels` are accepted** and stored (Vertex-only on 2026-08-08).
  `safety_settings` became accepted by 2026-10-30, with the category under
  the item key `type` (a few values remain Vertex-only; see
  `docs/ENUM_WIRE_FORMATS.md`). By 2026-10-01 create responses
  stopped echoing request fields (`labels`, `system_instruction`,
  `generation_config`); a GET of the stored interaction returns them.
- **`gemini-3.8-flash-tts`** returns `audio/wav`, and multi-speaker needs a
  `speech_metadata` annotation per text turn (see `docs/OUTPUT_MODALITIES.md`).
- **The Voices resource** uses the standard Google error envelope
  (`code: 400, status: "INVALID_ARGUMENT"`).

## Server-side changes (live, 2026-10-01)

Not in the changelog or the bindings; each reproduces with raw HTTP.

- **Namespaced function names.** On the second user turn of a non-streamed
  chain, a declared function is called as `default_api:<name>`. The
  auto-function loops resolve it. MCP tools run server-side appear as an
  answered `function_call` / `function_result` pair named `<server>:<tool>`,
  not `tool_call` steps; `pending_function_calls()` leaves them out.
- **Streamed multi-turn function calling failed (server bug, fixed by
  2026-11-01).** When the user turn and the turn before it were both
  streamed, the function result was rejected with `400 Please ensure that
  function response turn comes immediately after a function call turn`.
  `test_multiturn_streaming_auto_functions` streams both turns again.
- **An MCP interaction ends in `requires_action` (server bug, still present
  2026-11-01)** after the server has run every MCP call and produced the
  final `model_output`. `test_mcp_server_tool_round_trip` accepts that
  status only when no call is pending.
- **`cancel` fails on `antigravity-preview-09-2026`** with `400 There was a
  problem processing your request` (the retired 05-2026 agent accepted it).
- **url_context loops under a response schema (server bug, 2026-11-01).**
  With `response_format` set to a JSON schema, `url_context` refetches until
  the API gives up with `400 too_many_tool_calls ... Please retry the
  request` after ~117 s, in 8 of 12 requests. Without the schema, 6 of 6
  answered in ~5 s. `test_structured_output_with_url_context` gives each
  attempt 30 s and 4 tries.
- **Uploaded environment files carry no `size_bytes`** (2026-10-06), in the
  upload response or in listings; files from environment sources still
  report it.
- **Agentic video processing often failed (fixed by 2026-11-01).** On
  2026-10-01 2 of 6 identical raw-HTTP requests hung past 150 s; on
  2026-10-06 3 of 5 returned `500 Internal error encountered`. On 2026-11-01
  all 16 requests answered, 10 of them concurrent, so
  `processing_steps_tests` no longer retries.

## Sweep 2.25.0 → 2.34.0 (2026-11-01)

Every item probed live on 2026-11-01.

### Modeled

| Surface | Where | Live result |
|---|---|---|
| Decoding continuation: `continuation_token` on the response and the request | `InteractionResponse::continuation_token`, `InteractionBuilder::with_continuation_token`, `CONTINUATION_MODEL` | Only `gemini-4-argon` supports it; every other model answers `400 Decoding continuation is not enabled for this model`. A response cut off by `max_output_tokens` is `incomplete` with a token (plain, streamed and background). Resending the original input with the token continues mid-sentence; any other input is a 400. `max_output_tokens` is cumulative across continuations. |
| `requires_continuation` status | `InteractionStatus::RequiresContinuation` | Spec only; never produced. Continuable responses report `incomplete`. |
| `InteractionStatusUpdate.usage`, `.continuation_token` | `StreamChunk::StatusUpdate` | Background streams send `usage` mid-run. A `store: false` stream's updates carry no `interaction_id`; the crate dropped them until now. |
| `Usage.initial_input_tokens` | `UsageMetadata::initial_input_tokens` | On every response |
| `Interaction.errors` | `InteractionResponse::errors`, `InteractionError` | `[{"code": "CANCELLED", ...}]` on an interaction cancelled by a dropped foreground stream |
| `PromptedVoice.base_voice` | `PromptedVoice::base_voice`, `CreateVoiceRequest::with_base_voice` | Accepted, validated, echoed |
| `CodeExecutionLanguage` `bash` | `CodeExecutionLanguage::Bash` | Spec only: `gemini-3.8-flash` runs only Python |
| `service_tier: "deferred"` | `ServiceTier::Deferred` | Now explicitly Vertex-only ("allowed on the Gemini Enterprise Agent Platform"), so modeled per D-005 |
| Function results with audio, video and document content | `FunctionResultPayload::Contents` (already) | The model reads all three |
| `Environment.env` as plain strings | `RemoteEnvironment::env` (already) | Accepted; stored as `{"value": ...}` |
| Mid-stream error as a bare JSON line | `http::sse_parser`, `StreamChunk::Error` | A recitation block arrives as `{"error": {...}}` with no `data:` prefix, then EOF. Not in the spec. |

Also removed: `presence_penalty` and `frequency_penalty`, which the API now
rejects (`400 Unknown parameter 'presence_penalty' at 'generation_config'`)
and the bindings don't list. Deprecated upstream but still accepted:
`temperature`, `top_p`, `adaptation_phrases`.

### Deliberately not modeled

| Surface | Why |
|---|---|
| `VideoTask` `upscale` | `400 ... Supported values: text_to_video, image_to_video, reference_to_video, edit, extend` |
| `continuation_ids` | Never returned; as a request field, `400 Unknown parameter` |
| Server-only tool types (`bash`, `filesystem`, `tool_search`, `manage_task`, `schedule`) | Now rejected with a 400 that lists the supported tool types (they were accepted bare in 2.25.0's sweep) |
| New model ids (`gemini-4-argon`, `gemini-3.5-flash-lite`, `gemini-nano-banana-2.1`, `gemini-omni-*`, `lyria-3.5`, ...) | Model ids are open strings. All eight exist via `/v1beta/models`. |

### Found, NOT yet modeled

- **Continuation from a stored interaction without input.** With
  `previous_interaction_id` and the token, the API accepts a request with
  *no* `input`, but the crate always sends one. Needs an omittable input.
- **`{"allowlist": "disabled"}` network form.** Accepted; the echo reads back
  as `{"allowlist": [{"mode": "disabled"}]}`, which `NetworkConfig` parses
  as an allowlist entry with an empty domain. Its meaning next to
  `network: "disabled"` is undocumented.
- **The stable `/v1` endpoint.** Create, get, delete and cancel only; no
  continuation, environments or agent configs. The crate targets `v1beta`.

## Sweep 2.18.1 → 2.25.0 (2026-09-24)

### Modeled

| Surface | Where | Live result |
|---|---|---|
| `processing_call` / `processing_result` steps and deltas | `Step::Processing*`, `StepDelta::Processing*` | Emitted for video with `processing: "agentic"`. The ~36KB signatures are required on replay. The streaming accumulator dropped them (`step.start` has `""`; the value arrives in `step.delta`), so stateless replay of a streamed turn got `400 Processing call step is missing signature`. Fixed; covered by `tests/processing_steps_tests.rs`. Unknown deltas now also merge a `signature` into a same-typed Unknown step. |
| `retrieval_call` / `retrieval_result` steps and deltas | `Step::Retrieval*` | Spec parity; the retrieval tool is Vertex-only |
| `speech_metadata` and `word_info` annotations | `Annotation::SpeechMetadata` / `WordInfo`, `Content::speaker_text` | Required for multi-speaker on 3.8 TTS; rejected by older TTS models |
| Voices resource `/v1beta/voices` | `client.voices()` (`src/voices.rs`) | List with filters and paging, prompted create, get, synthesize with the custom ID, delete |
| Credentials resource `/v1beta/credentials` | `client.credentials()` (`src/credentials.rs`) | Create, get, list, patch, delete. OAuth2 create checks that `token_url` is reachable. The ID is optional on create. |
| `environment.env` and `AllowlistEntry.credential` | `RemoteEnvironment::env`, `EnvVar` | Validated (unknown ID → 404) and echoed; the echo spells `env` as a list of single-key maps. Works at runtime (`tests/credentials_tests.rs`): plain values are visible in the sandbox; credential-backed variables hold a placeholder and the egress proxy substitutes the secret; allowlist bearer credentials are injected. Bearer `header_name`/`prefix` are accepted but not applied. |
| Environment files (list and resumable upload) | `client.environments().files()` (`src/environments/files.rs`) | Works; entry `type` is uppercase `FILE`/`DIRECTORY` on the wire |
| `from_environment` (fork) | `CreateEnvironmentRequest::from_environment` | Works with a bare ID; `environments/{id}` returns 404 |
| `Video.name` | `Content::with_video_name` | Accepted |
| Video `response_format.resolution` | `VideoResolution` | Server-validated; no Interactions model outputs video (Veo 404s) |
| `transcription_config.mode` | `TranscriptionMode` | String and object forms accepted and validated; no output effect seen |
| `Ranking.rank_service` | `RankService` | Vertex-only (retrieval tool) |
| `FunctionResultDelta.call_id` dropped | now `Option` | Keeps the delta typed if the field stops arriving |
| Stored interaction fields: `labels`, `system_instruction` | `InteractionResponse` | Observed live (GET only since 2026-10-01) |
| Unmodeled response fields | `extra` on `InteractionResponse` and `UsageMetadata` | Usage carries `raw_prompt_token` and `model_invocation_token_counts`; interactions echo `environment`, `generation_config`, and more |
| Model literals `gemini-3.8-flash`, `gemini-3.8-flash-tts` | `DEFAULT_MODEL`, `DEFAULT_TTS_MODEL` | — |

### Deliberately not modeled

| Surface | Why |
|---|---|
| `cached_content` (re-added, deprecated) | Still `400 Unknown parameter 'cached_content'`. Stays removed (D-005). |
| `transcription_config.language_hints` | `400 Unknown parameter` |
| `input` optional on create | The API still answers `400 Missing input.` in every form tried |
| `service_tier: "deferred"` | `400 The value 'deferred' is not supported` |
| String arm of `environment.env` | `Invalid input at 'environment'`. Preserved in `extra` if read. |
| Server-only tool types (`bash`, `filesystem`, `tool_search`, `manage_task`, `schedule`) | Absent from the bindings. Accepted bare, but `bash`/`filesystem` end in `400 malformed_tool_call` and the rest do nothing on a raw model. Revisit when they reach the bindings. |
| Annotation types `in_context_file_citation`, `reference_metadata` | Server enum only. They land in `Annotation::Unknown`. |
| Environment `storage` field | Server only. Preserved in `Environment::extra`. |
| Path-parameter renames (`interactionsId`, `agentsId`, ...) and new MIME literals (`audio/webm`, `video/jpeg2000`) | Cosmetic. MIME types are open strings. |

## Earlier sweeps: landed

- **Revision 2026-05-20 migration (2026-07):** steps model, the
  `interaction.created` / `step.*` / `interaction.completed` SSE lifecycle,
  thought signatures, `arguments_delta`, per-step usage, lowercase enums,
  and the `tool_choice` union. `function_call` steps carry a `signature` the
  bindings omit, and replay requires it.
- **Phase 2 (2026-07):** service tier; webhooks (CRUD, `:ping`,
  `:rotateSigningSecret`; `update_mask` is ignored); `include_input`
  (a no-op); retrieval tool (Vertex-only); video config and `VideoTask`
  (with `extend`); typed `response_format` (image: `image/jpeg` inline only;
  audio: `sample_rate` only); environments and agents (agent create was gated
  on a standard key through 2026-10-08 and works since 2026-10-30; agent
  tools were code execution, search and URL context in 2026-07, and add
  `function` and `mcp_server` as of 2026-11-01); multi-speaker TTS; presence/frequency penalties; tool-config
  completeness; `budget_exceeded`; deep-research knobs (`enable_bigquery_tool`
  is Vertex-only); typed citations; audio channels and sample rate.
- **2.17.0 (2026-08):** triggers resource (list verified; the full
  lifecycle verified 2026-11-01, once custom agents could be created),
  environments resource (full lifecycle),
  `transcription_config`, `safety_settings` (Vertex-only until 2026-10-30), `labels` (now
  accepted, see above), `AntigravityConfig`.
- **2.18.1 (2026-08-16):** `Content::Video.processing` (#434; the segment
  window is the token-cost lever, and it is valid only inside a `user_input`
  step), and `speech_config` deserialize accepting `{"speakers": [...]}`
  (#437).
- **Reverted:** `cached_content` (#439). It was modeled from the spec without
  a probe and could only ever 400.

## Spec vs. implementation: fixed

`excludedPredefinedFunctions` → snake_case; `FunctionCallingMode` and
`CodeExecutionLanguage` → lowercase; `top_k`, `response_mime_type`,
`cached_content`, `Turn` input and `total_reasoning_tokens` removed;
`system_instruction` → plain string; `InteractionResponse` → snake_case.

## Verification protocol

New surface lands with wire-fixture unit tests taken from the bindings,
next to the type (`src/steps/`, `src/voices.rs`, ...). It also gets a
strict live test that cleans up after itself and is registered in the
`rust.yml` integration matrix (`tests/ci_coverage.rs` enforces this).
Before a release, run the integration suite with `LOUD_WIRE=1` and update
`docs/ENUM_WIRE_FORMATS.md`.
