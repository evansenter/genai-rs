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
| 1 | **Generated bindings**: `google-genai`'s `_gaos/types/**` and the endpoint `path=` lines in `_gaos/*.py`, snapshot for the baseline in `tests/fixtures/api_surface/bindings.json` (written only by `.github/scripts/refresh_api_snapshot.sh`) | Ships ahead of prose. Diff two releases to find new surface. |
| 1b | **OpenAPI specs** (v1beta and v1), snapshot in `tests/fixtures/api_surface/` | Machine-readable, and agrees with the bindings where both speak; found 2026-11-01. The published URLs (`ai.google.dev/static/api/interactions.openapi.json`, `interactions-v1.openapi.json`) redirect to aistudio.google.com paths that 404 since about 2026-11-03, and no new public URL was found (2026-11-04), so the 2026-11-01 copies are the latest. |
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

## Coverage oracle (2026-11-04)

Completeness against ranks 1 and 1b is a CI check, not a sweep finding:
`src/coverage_oracle/` and `tests/request_wire_oracle.rs` check every
request key against the binding classes and every response field for an
empty `extra`, offline, against the committed snapshot; every binding class
must be reached by those checks or listed with a reason; and
`tests/ci_coverage.rs` maps every endpoint to the handle verbs that call it
and requires a live test per handle verb. How it works and how to refresh
the snapshot is in `docs/TESTING.md` ("Coverage oracle"). Its exception
lists are this file's working list: the `GAPS`, `NOT_SENT` and `UNMAPPED`
entries are what is not modeled yet.

What the first run found (google-genai 2.34.0, spec of 2026-11-01), live
facts from 2026-11-04. The interaction-side items are closed below, in
[Interaction residue](#interaction-residue-2026-11-05), and the
resource-side ones (files, file search stores, agents, voices,
environments) in [Resource residue](#resource-residue-live-2026-11-05).

- **No wrong request keys.** Every key the crate sends is on its binding
  class, `response_format` included, except response data a caller sends
  back in history: `signature` on `function_call` (required, the bindings
  omit it) and on `function_result`; `Place` fields beyond `place_id`,
  `name` and `url` (not served); `GoogleSearchResultItem.title`/`url`/
  `rendered_content` (not served); and `file_search_result.result` (the
  bindings dropped it, and it was never filled). The bindings mark a
  credential's `id` required; the API assigns one when it is omitted.
- **Served, landing in `extra`:** on a stored interaction read back,
  `generation_config` (with `speech_config` in the `{"speakers": [...]}`
  form, whichever form was sent), `response_format`, `response_modalities`
  and `safety_settings`. With `include_input=true` every one of those lists
  (`speakers`, `stop_sequences`, `response_modalities`, `safety_settings`)
  comes back with each entry twice, which modeling them has to allow for.
  `FileSearchStore.activeDocumentsCount` and `sizeBytes` (string int64s);
  `File.updateTime` and `File.source` (`UPLOADED`); all four modeled since
  (see "Resource residue" below).
- **In the snapshot, not modeled, not seen live yet:** the `queued` status
  (#501); `Interaction.agent_config`, `environment`, `continuation_ids`,
  `output_audio`/`_image`/`_video`, and the spec's `max_total_tokens`; the
  CodeMender agent config (#501). The resource fields this bullet listed
  (`Agent.agent_config`, the inline environment's `environment_id`,
  `embeddingModel`, the document upload and import configs, the `VoiceInput`
  fields, the store's document counts, `Document.customMetadata`,
  `File.downloadUri`, `FileStatus.details`) are modeled or reasoned out
  since; see "Resource residue" below.
- **Read but lost:** `StreamChunk::StepDelta` drops the event's
  `metadata.total_usage`. The spec types `file_search_result` delta items
  as opaque objects, and `FileSearchResultItem` keeps only `title`, `text`
  and `file_search_store` (#429). `Tool::Function` requires `name`,
  `description` and `parameters`, which the bindings and the spec make
  optional, so a function tool served without one reads as `Unknown`; the
  oracle's bodies set every field, so this one was found by reading.
- **Live coverage:** `FileSearchDocuments::get` and
  `Interactions::get_with_input` had no live test; both have one now.
  `include_input=true` now echoes the input as a `user_input` step.

## Interaction residue (2026-11-05)

Every interaction-side item the coverage oracle and #501 left open, and the
three #489 questions, probed live on 2026-11-05 and then modeled or
recorded. That left the oracle's `GAPS` with only resource-side items,
closed in [Resource residue](#resource-residue-live-2026-11-05), and a new
sparse-body pass reads every response class with only its required fields
(`docs/TESTING.md`, "Sparse response bodies").

### Modeled

| Surface | Where | Live result |
|---|---|---|
| Continuation without input (#489) | `InteractionRequest::input` is an `Option`; `InteractionBuilder::continue_decode(&response)` | `previous_interaction_id` and `continuation_token` alone continue a stored decode, and with no `max_output_tokens` it runs to the end. Without the id: `400 previous_interaction_id is required when continuation_token is set for a stored interaction`. A `store: false` continuation without input: `400 contents is not specified`. No input and no token: a misleading `404 Model ... not found`, which `build()` heads off |
| `{"allowlist": "disabled"}` (#489) | `NetworkConfig::AllowlistDisabled`, `NetworkConfig::is_disabled()` | Accepted; a GET echoes `{"allowlist": [{"mode": "disabled"}]}`, which read as an entry with an empty domain until now. An agent's `curl` got the egress proxy's 403 under it and under `network: "disabled"`, and 200 with no network config, so the two forms block egress alike. It is the bindings' `Disabled` arm of the allowlist |
| The `/v1` endpoint (#489) | `ClientBuilder::with_api_version("v1")` | Interactions in full, the same protocol as `/v1beta` (create, get, delete, cancel, streaming, and continuation, which the v1 spec leaves out), plus files and file search stores; `404` for agents, environments, webhooks, credentials, voices and triggers. An interaction created on one version reads on the other. `v1beta` stays the default |
| `queued` status (#501) | `InteractionStatus::Queued` | In the bindings (interaction, stream interaction, status update), not the spec; never observed: background interactions report `in_progress` from their create response on |
| CodeMender agent config (#501) | `CodeMenderConfig` and its find/fix request types | Vertex-only: `400 The value 'code-mender' is not supported for 'agent_config.type' on the Gemini API, it is allowed on the Gemini Enterprise Agent Platform`. Kept and documented (D-005) |
| Stored request config | `InteractionResponse::generation_config`, `response_format`, `response_modalities`, `safety_settings` | All four on a stored read; the create response echoes `safety_settings`. `speech_config` comes back as `{"speakers": [...]}` and reads into the same list. With `include_input=true` every list holds each entry twice (also 2026-11-04); kept as served |
| Agent echoes | `InteractionResponse::agent_config`, `environment` | `agent_config` on an agent's create response and stored read; `environment` (the id string, or the inline definition without an `environment_id`) on the stored read |
| `GenerationConfig` unknown keys | `GenerationConfig::extra` | Needed now that a response carries it |
| `step.delta` `metadata.total_usage` | `StreamChunk::StepDelta::total_usage` | In the bindings and the spec; no text, TTS or agent stream sent `metadata` on a `step.delta` |
| Streamed audio | The stream accumulator | A streamed TTS answer is 243 raw PCM deltas (`audio/l16`, 24000 Hz, mono) in one step, each naming the format and carrying padding-free base64; they assemble into one block, and a different format or a `uri` starts a new one. No model streams video or documents; they follow the image rule |
| Tools without optional fields | `Tool::Function` (`description`, `parameters` now `Option`), `ComputerUse`, `McpServer`, `FileSearch`; `FunctionParameters` keeps unmodeled schema keywords | A function tool sent with only a `name` is accepted and called, and a stored read serves it that way; it read as `Tool::Unknown`. Found by the sparse-body pass |
| `VideoMetadata` unknown keys | `VideoMetadata::extra` | A file's `videoMetadata` is `{"videoDuration": "1s"}`; the bindings type it `dict[str, Any]` |
| Non-string unknown enum values | `wire_enum!` | A number or object read into `Unknown` serializes back as itself |

### Removed or kept, after a live check

| Surface | Outcome | Live result |
|---|---|---|
| `GoogleSearchResultItem.title`/`url`/`rendered_content` | Removed | Items carry only `search_suggestions`, the one field of the bindings' `GoogleSearchResult` |
| `Place` address, coordinates, types, rating, ratings count, website, phone | Removed | 30 places across three Maps results carried only `name`, `place_id` and `url` (the bindings add `review_snippets`) |
| `Step::FileSearchResult.result`, `FileSearchResultItem`, `file_search_results()` | Removed; the delta's `result` is opaque JSON | The step and its delta carry only `call_id`/`signature`, stored and streamed. Neither the bindings nor the spec give the step a `result`; the spec gives the delta a list of field-less objects |
| `FunctionResult.signature` | Kept | A server-run MCP call's `function_result` carries one (a deepwiki call); not in the bindings |
| `InteractionResponse.output_text` | Kept | Never served: not on TTS, image or text create responses, stored reads, or stream completed events. Kept because it is in the bindings and `as_text()`/`all_text()`/`has_text()` fall back to it at no cost; removing it would break struct literals for nothing. Its siblings `output_audio`/`_image`/`_video` stay unmodeled (below) |

### Not modeled, with the reason

| Surface | Why |
|---|---|
| `Interaction.output_audio`/`_image`/`_video` (#501) | Never served: not on a create response or stored read of a TTS or image interaction, nor on a stream's completed event. `output_text` is not served either. The output is in `steps` |
| `Interaction.continuation_ids` | Never served, not even on the reads of a decode continued twice; rejected as a request field (2026-11-01) |
| `Interaction.max_total_tokens` (spec only) | `400 Unknown parameter 'max_total_tokens'` on model and agent interactions alike (D-005); the agent budget is `agent_config.max_total_tokens` |

## Resource residue (live, 2026-11-05)

Every field the bindings' classes define for the resources (Files, File
Search Stores and their documents, agents, voices, environments,
credentials, webhooks, triggers), and every endpoint they list, is modeled
or listed in the coverage oracle's tables with a dated reason. The Python
SDK has verbs beyond those, listed under "Deferred" below with dated
reasons in `docs/RESOURCES.md`. Probed live 2026-11-05:

- **Files:** `updateTime` and `source` (`FileSource`, `UPLOADED` on every
  upload) are modeled, and `sizeBytes` reads a string or a number. A file
  that fails processing serves `error: {code: 3, message}` with no
  `details`; `FileError` models `details` and keeps other keys in `extra`.
  `downloadUri` stays a `GAPS` entry: no uploaded or failed file carries it,
  and only a generated file (a video generation) would.
- **File Search Stores:** `activeDocumentsCount`, `pendingDocumentsCount`
  (served only while a document indexes) and `sizeBytes` are string int64s,
  all modeled; `failedDocumentsCount`, never seen because an unindexable
  document fails its upload with a 500, is modeled beside them.
  `embeddingModel` on create is read (another model is kept; an unknown one
  is a 400). Documents serve `customMetadata` in the order sent.
- **Uploads and imports:** the document upload moved from the `raw` to the
  `resumable` protocol so its JSON body can carry `customMetadata` and
  `chunkingConfig` (`DocumentUpload`). `file_search_stores.import_file` is
  `FileSearchStores::import_file` (`DocumentImport`): its operation
  finishes on the first `GET` and names the document by bare ID, which is
  the operation's; the document exists, pending, as soon as the `POST`
  answers, and a second import makes a second document, so a timed-out
  import's error names both. The bindings' `ImportFileConfig` lacks
  `fileName`, which the body needs. A white-space overlap as large as the
  chunk is a 500, so the crate rejects it first.
- **Agents:** `agent_config` is stored and echoed (`max_total_tokens` as a
  string); a config of another type is stored as `{"type": "antigravity"}`,
  and an unknown field is a 400.
- **Voices:** eight `VoiceInput` fields (`description`, `language_code`,
  `region_code`, `accent`, `persona`, `context`, `gender`, `pitch`) are
  stored, echoed and filterable. `sample_audio` is output only: a value sent
  is ignored and replaced by a generated sample (`NOT_SENT`).
- **Environments:** the remote environment's `environment_id` runs an agent
  interaction in that existing environment (the agent read a file the
  environment was created with, which the live test checks); an unknown ID
  is a 404. On an agent's `base_environment` it is accepted and dropped. A
  stored agent interaction serves `environment: {"environment_id", "type":
  "remote"}`. `environments.files.download` is
  `EnvironmentFiles::download`: `GET` on the listing's path with
  `alt=media` serves the file's bytes under its own content type, and a
  directory as `application/x-tar`. The bindings list that endpoint once,
  for both verbs, so the oracle maps both to it.
- **Deferred** (SDK verbs outside the bindings' classes and endpoints):
  `files.download` (an uploaded file is a `400 Only GENERATED files can be
  downloaded`), `files.register_files` (`401 API keys are not supported`),
  `files.upload(config={"name"})` (accepted, but `FileUpload` is shared with
  store uploads, which take no name), `file_search_stores.download_media`
  (needs a `file_citation` `media_id`, never served) and
  `triggers.list(filter_=)` (every expression tried is a `400 Invalid filter
  expression`, syntax undocumented).

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
  Re-verified 2026-11-04, streamed and not: `<server>` is the declared
  `mcp_server` name, and `mcp_calls()` reads the pairs back with server and
  tool split (#459). A declared client function named `weather:get` was
  accepted the same day, so the colon alone does not mark an MCP call.
- **Streamed multi-turn function calling failed (server bug, fixed by
  2026-11-01).** When the user turn and the turn before it were both
  streamed, the function result was rejected with `400 Please ensure that
  function response turn comes immediately after a function call turn`.
  `test_multiturn_streaming_auto_functions` streams both turns again.
- **An MCP interaction ends in `requires_action` (server bug, still present
  2026-11-01)** after the server has run every MCP call and produced the
  final `model_output`. `test_mcp_server_tool_round_trip` accepts that
  status only when no call is pending. Not reproduced on 2026-11-04: three
  MCP interactions (two non-streamed, one streamed) ended `completed`.
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
| `continuation_ids` | Never returned (again on 2026-11-05, on a decode continued twice); as a request field, `400 Unknown parameter` |
| Server-only tool types (`bash`, `filesystem`, `tool_search`, `manage_task`, `schedule`) | Now rejected with a 400 that lists the supported tool types (they were accepted bare in 2.25.0's sweep) |
| New model ids (`gemini-4-argon`, `gemini-3.5-flash-lite`, `gemini-nano-banana-2.1`, `gemini-omni-*`, `lyria-3.5`, ...) | Model ids are open strings. All eight exist via `/v1beta/models`. |

### Found, then modeled (2026-11-05)

The three items left open here (continuation without input, the
`{"allowlist": "disabled"}` form, the `/v1` endpoint) were probed and
modeled on 2026-11-05; see [Interaction residue](#interaction-residue-2026-11-05).

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
  (a no-op then; it echoes the input as of 2026-11-04); retrieval tool (Vertex-only); video config and `VideoTask`
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
`rust.yml` integration matrix (`tests/ci_coverage.rs` enforces this), and
the coverage oracle passes with any exception listed with its reason
(`docs/TESTING.md`, "Coverage oracle").
Before a release, run the integration suite with `LOUD_WIRE=1` and update
`docs/ENUM_WIRE_FORMATS.md`.
