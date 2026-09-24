# Interactions API Gap Tracker

> ## ⚠️ This is a point-in-time snapshot, not a completeness guarantee
>
> | | |
> |---|---|
> | **Last swept against** | `google-genai` **2.25.0** |
> | **Sweep date** | **2026-09-24** |
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
- **`labels` are accepted** and echoed on the response (Vertex-only on
  2026-08-08). `safety_settings` is still Vertex-only.
- **`gemini-3.8-flash-tts`** returns `audio/wav`, and multi-speaker needs a
  `speech_metadata` annotation per text turn (see `docs/OUTPUT_MODALITIES.md`).
- **The Voices resource** uses the standard Google error envelope
  (`code: 400, status: "INVALID_ARGUMENT"`).

## Sweep 2.18.1 → 2.25.0 (2026-09-24)

### Modeled

| Surface | Where | Live result |
|---|---|---|
| `processing_call` / `processing_result` steps and deltas | `Step::Processing*`, `StepDelta::Processing*` | Emitted for video with `processing: "agentic"`. The ~36KB signatures are required on replay. The streaming accumulator dropped them (`step.start` has `""`; the value arrives in `step.delta`), so stateless replay of a streamed turn got `400 Processing call step is missing signature`. Fixed; covered by `tests/processing_steps_tests.rs`. Unknown deltas now also merge a `signature` into a same-typed Unknown step. |
| `retrieval_call` / `retrieval_result` steps and deltas | `Step::Retrieval*` | Spec parity; the retrieval tool is Vertex-only |
| `speech_metadata` and `word_info` annotations | `Annotation::SpeechMetadata` / `WordInfo`, `Content::speaker_text` | Required for multi-speaker on 3.8 TTS; rejected by older TTS models |
| Voices resource `/v1beta/voices` | `src/voices.rs` | List with filters and paging, prompted create, get, synthesize with the custom ID, delete |
| Credentials resource `/v1beta/credentials` | `src/credentials.rs` | Create, get, list, patch, delete. OAuth2 create checks that `token_url` is reachable. The ID is optional on create. |
| `environment.env` and `AllowlistEntry.credential` | `RemoteEnvironment::env`, `EnvVar` | Validated (unknown ID → 404) and echoed; the echo spells `env` as a list of single-key maps. **No runtime effect observed**: the sandbox saw no variable and no header was injected. |
| Environment files (list and resumable upload) | `src/environments/files.rs` | Works; entry `type` is uppercase `FILE`/`DIRECTORY` on the wire |
| `from_environment` (fork) | `CreateEnvironmentRequest::from_environment` | Works with a bare ID; `environments/{id}` returns 404 |
| `Video.name` | `Content::with_video_name` | Accepted |
| Video `response_format.resolution` | `VideoResolution` | Server-validated; no Interactions model outputs video (Veo 404s) |
| `transcription_config.mode` | `TranscriptionMode` | String and object forms accepted and validated; no output effect seen |
| `Ranking.rank_service` | `RankService` | Vertex-only (retrieval tool) |
| `FunctionResultDelta.call_id` dropped | now `Option` | Keeps the delta typed if the field stops arriving |
| Interaction echoes: `labels`, `system_instruction` | `InteractionResponse` | Observed live |
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
  audio: `sample_rate` only); environments and agents (agent create is gated
  on a standard key; agent tools are code execution, search and URL context
  only); multi-speaker TTS; presence/frequency penalties; tool-config
  completeness; `budget_exceeded`; deep-research knobs (`enable_bigquery_tool`
  is Vertex-only); typed citations; audio channels and sample rate.
- **2.17.0 (2026-08):** triggers resource (list verified, create
  agent-gated), environments resource (full lifecycle),
  `transcription_config`, `safety_settings` (Vertex-only), `labels` (now
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
next to the type (`src/steps.rs`, `src/voices.rs`, ...). It also gets a
strict live test that cleans up after itself and is registered in the
`rust.yml` integration matrix (`tests/ci_coverage.rs` enforces this).
Before a release, run the integration suite with `LOUD_WIRE=1` and update
`docs/ENUM_WIRE_FORMATS.md`.
