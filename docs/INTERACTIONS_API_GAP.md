# Interactions API Gap Analysis

> ## ⚠️ This is a point-in-time snapshot, not a completeness guarantee
>
> | | |
> |---|---|
> | **Last swept against** | `google-genai` **2.18.1** |
> | **Sweep date** | **2026-08-16** |
> | **Baseline in CI** | `.github/last-swept-sdk-version` |
>
> Everything below being checked off means *"nothing was missing as of the
> sweep date"* — it does **not** mean the crate is currently complete. The
> SDK ships new Interactions surface between sweeps, and this file cannot
> know about it.
>
> **Before relying on this file, check whether a newer `google-genai` has
> shipped.** The scheduled `api-surface-sweep` workflow does this daily and
> opens an issue when the SDK moves; if that issue is open, this file is
> behind by at least that much. The workflow closes that issue itself once
> `.github/last-swept-sdk-version` catches up, so an open issue always
> means genuinely-unswept surface rather than a close someone forgot.

## How to verify API surface (read this before trusting any source)

These three disagree, and they disagree in a consistent direction. Use them
in this order:

| Rank | Source | Why |
|------|--------|-----|
| 1 | **Generated bindings** — `google-genai`'s `_gaos/types/interactions/*.py` | Machine-generated from the spec; ships *ahead* of prose. Diffing two releases is the only reliable way to spot new surface. |
| 2 | **Live probes** against `generativelanguage.googleapis.com` | Ground truth for what the Gemini endpoint actually accepts, which is often narrower than the spec. |
| 3 | **Prose docs** — `ai.google.dev` *and this file* | Both lag the other two. Neither is evidence of absence. |

This ordering was learned the hard way. The 2.17.0 → 2.18.1 sweep found
`Content::Video.processing` (a 127x token-cost lever) and a widened
`speech_config` union — **neither documented on `ai.google.dev`**, and both
invisible to a reader who trusted this file's checked-off list. See #421.

Rank 2 matters as much as rank 1: the bindings describe a union for
`speech_config` that the Gemini API rejects outright, and `Tool::Retrieval`
is in the bindings but Vertex-only. New surface found at rank 1 must be
live-probed before it is modeled as usable.

Status: implementation tracker — items are removed/checked off as they land.
Source: Google's `google-genai` generated API bindings, originally 2.10.0
cross-checked against SDK 1.65/1.74/2.0 for protocol history; re-swept
2026-08 against 2.17.0 (items 18-20, landed) and 2026-08-16 against 2.18.1
(items 21-22, **found but not yet modeled**), with every new parameter
live-probed first. All behaviors should be re-verified live with
`LOUD_WIRE=1` before release.

## Headline: wire revision migration — ✅ DONE (2026-07)

The crate now sends `Api-Revision: 2026-05-20` on every Interactions API
request and implements the revision's protocol:

- ✅ `steps: [Step...]` response model (`Step`, `StepDelta`,
  `FunctionResultPayload` in `src/steps.rs`); convenience accessors
  reimplemented over steps.
- ✅ New SSE lifecycle: `interaction.created`, `interaction.status_update`,
  `step.start`, `step.delta`, `step.stop`, `interaction.completed`, `error`
  (`src/wire_streaming.rs`, dispatch + step accumulation in
  `src/http/interactions.rs`).
- ✅ Thought steps `{signature, summary}` + `thought_summary` /
  `thought_signature` stream deltas.
- ✅ Input union `str | [Step] | [Content] | Content`; `Turn` removed
  (deprecated in spec); history represented as steps.
- ✅ `arguments_delta` streaming function-call arguments (exposed through
  `StreamChunk::StepDelta` and `AutoFunctionStreamChunk::Delta`; assembled
  into `FunctionCall.arguments` on completion).
- ✅ Per-step usage (`usage`/`step_usage` on `step.stop`) and
  `metadata.total_usage` on lifecycle events.

✅ Live wire verification performed 2026-07 with a real `GEMINI_API_KEY`
against `generativelanguage.googleapis.com` (Api-Revision 2026-05-20).
Results:

- Revision `2026-05-20` accepted; the steps model and snake_case field
  naming confirmed on the wire.
- `function_call` steps carry a `signature` field the generated SDK bindings
  omit — the API returns it and **rejects stateless replay without it**.
  `Step::FunctionCall` / `Step::FunctionResult` now model it.
- Response modalities are enforced lowercase (`text`, `image`, `audio`,
  `video`, `document`); uppercase values (e.g. `"AUDIO"`) are rejected.
  `with_response_modalities()` now normalizes to lowercase.
- The deprecated `response_mime_type` is rejected outright
  (400 "responseFormat must be set when responseMimeType is set" — returned
  even when `response_format` IS set, raw-schema or typed; and camelCase
  `responseMimeType` gets "Unknown parameter"). The field has therefore been
  removed from this crate; use `response_format` alone.
- The typed `response_format` union (`{type: "text", mime_type, schema}`)
  and the raw JSON-schema form were both accepted live for text output.

✅ Phase-2 surface verified live 2026-07 (real `GEMINI_API_KEY`,
`generativelanguage.googleapis.com`, Api-Revision 2026-05-20). Per-item
results — full wire notes in `docs/ENUM_WIRE_FORMATS.md`:

- **Webhooks**: full CRUD + `:ping` + `:rotateSigningSecret` round-trip
  green; get/list echo create's fields exactly; `new_signing_secret` only
  on create; rotate returns a fresh distinct secret and old secrets get a
  24h `expire_time`. `:ping` accepts our empty `{}` body (and a bodiless
  POST). `update_mask` on PATCH is optional and observed to be **ignored**
  — the body's fields alone determine what changes. `create_time`/
  `update_time` were never returned. `webhook_config` on requests needs
  `background=true` and is echoed back verbatim in the create response.
- **Agents**: creation is **gated** on a standard API key — every
  schema-valid payload got a generic 400 "Request contains an invalid
  argument." (field names still validated: snake_case `id`, `base_agent`,
  `system_instruction`, `description`, `tools`, `base_environment`).
  Agent `tools` accept only `code_execution` / `google_search` /
  `url_context` (per the API's validation error). `GET/LIST /agents` work
  (`{"agents": [...]}`); managed agent IDs are not retrievable (404).
  CRUD round-trip beyond create therefore unverifiable on this account.
- **Environments**: inline source, `network: "disabled"`, allowlist with
  header `transform`, and the string environment-ID form all accepted
  (agent `antigravity-preview-05-2026`, background); `environment_id`
  returned on typed requests.
- **Typed response_format**: single + list forms accepted. Text-with-schema
  output validates against the schema. Image: inline `image/jpeg` only
  (`delivery` rejected). Audio: `sample_rate` works, `mime_type`/`delivery`
  rejected (inline `audio/l16` returned). Video: `gcs_uri` is Vertex-only.
- **Multi-speaker TTS**: list-form `speech_config` accepted; one combined
  `audio/l16` stream returned. `include_input=true` on GET is a no-op (no
  input or config echo), so the speech_config echo shape is unobservable.
- **Video config**: Veo models 404 on the Interactions API (models list
  shows them as `predictLongRunning`-only); `video_config.task` enum
  validated server-side and revealed a fifth value `extend` (added to
  `VideoTask`).
- **Retrieval tool**: rejected as **Vertex-only** ("allowed on the Gemini
  Enterprise Agent Platform"); Gemini tool types are `google_maps`,
  `mcp_server`, `function`, `google_search`, `file_search`,
  `computer_use`, `code_execution`, `url_context`.
- **Deep-research knobs**: `visualization` (`off|auto`, server-validated) +
  `collaborative_planning` accepted; `enable_bigquery_tool` is Vertex-only.
- **`safety_settings` / `labels`**: both rejected as **Vertex-only**
  (2026-08-08: "not available on the Gemini API but ... available on the
  Gemini Enterprise Agent Platform") — modeled for spec parity like the
  Retrieval tool and `enable_bigquery_tool` above.
- **Environments resource**: full CRUD lifecycle verified live
  (`/v1beta/environments`); wire uses `created`/`updated`/`last_accessed`
  timestamps and string-serialized int64 `file_count`/`size_bytes`.
- **Triggers resource**: list verified live (`{}` when empty); create is
  custom-agent-gated ("Agent '' is invalid or not found") and rejects
  `store` inside the nested interaction, so the create/update/execution
  shapes are modeled from the SDK spec with per-field Evergreen defaults.
- **`transcription_config`**: accepted by the API (200) inside
  `generation_config`.

Evergreen extras spotted during verification (returned by the API but not
previously modeled on `InteractionResponse`) — all now modeled:
`object: "interaction"`, `service_tier`, and the `webhook_config` echo.

⚠️ Still pending live verification: per-step usage shapes on `step.stop`.

## Missing surface (by user value)

Completed in the revision-migration phase and the phase-2 surface expansion
(2026-07):

1. ~~`Api-Revision: 2026-05-20` migration (steps model + new SSE lifecycle).~~ ✅
2. ~~`tool_choice` restructure: lowercase enums or
   `{allowed_tools: {mode, tools}}`; remove crate's top-level `allowed_tools`
   inside generation_config.~~ ✅ (`ToolChoice` / `AllowedTools`)
3. ~~`cached_content` request field (explicit caching).~~ ✅
   (`with_cached_content()`)
4. ~~`service_tier`: `flex | standard | priority`.~~ ✅ (`ServiceTier`,
   `with_service_tier()`)
5. ~~Webhooks: `webhook_config {uris, user_metadata}` on requests + full
   `/v1beta/webhooks` resource (CRUD, `:ping`, `:rotateSigningSecret`,
   events `batch.succeeded/expired/failed`, `interaction.requires_action/
   completed/failed`, `video.generated`).~~ ✅ (`src/webhooks.rs`,
   `Client::*_webhook*()`, `with_webhook_config()`)
6. ~~`include_input` query param on GET interaction.~~ ✅
   (`Client::get_interaction_with_input()`)
7. ~~`retrieval` tool: `vertex_ai_search | rag_store | exa_ai_search |
   parallel_ai_search` + per-backend configs.~~ ✅ (`Tool::Retrieval`,
   `RetrievalConfig`)
8. ~~Video generation: `response_modalities: ["video"]`,
   `generation_config.video_config {task}`, video response_format
   (`gcs_uri`, `duration`, `delivery: uri`).~~ ✅ (`VideoConfig`/`VideoTask`,
   `with_video_output()`, `ResponseFormat::Video`)
9. ~~Typed `response_format` union (text/audio/image/video) + list form +
   `delivery: inline|uri`.~~ ✅ (`ResponseFormat`/`ResponseFormatSpec`/
   `ResponseDelivery`; raw JSON schemas still accepted by
   `with_response_format()`)
10. ~~Environments (`environment` request field, sources
    `gcs|inline|repository|skill_registry`, network allowlist) + Agents
    resource (`/v1beta/agents` CRUD).~~ ✅ (`src/environment.rs`,
    `src/agents.rs`, `with_environment()`, `Client::*_agent*()`)
11. ~~Multi-speaker TTS: `speech_config` as a list of `{voice, language,
    speaker}`.~~ ✅ (list wire form; `with_speech_configs()` /
    `add_speech_config()`; legacy single object accepted on deserialize)
12. ~~`presence_penalty` / `frequency_penalty` [-2, 2].~~ ✅
13. ~~Tool config completeness: GoogleMaps `latitude`/`longitude`; ComputerUse
    `enable_prompt_injection_detection`, `disabled_safety_policies`,
    `mobile|desktop` environments; GoogleSearch `enterprise_web_search`;
    MCP `allowed_tools` as `[{mode, tools}]`.~~ ✅
14. ~~`budget_exceeded` status (first-class); usage `grounding_tool_count`.~~ ✅
15. ~~Deep-research config: `visualization`, `collaborative_planning`,
    `enable_bigquery_tool`; document agent IDs incl.
    `antigravity-preview-05-2026`.~~ ✅ (`DeepResearchConfig` options;
    agent IDs in `docs/AGENTS_AND_BACKGROUND.md`)
16. ~~Typed citation annotations: `url_citation`, `file_citation`,
    `place_citation` (with review snippets); byte indices.~~ ✅
17. ~~Audio content `channels`/`sample_rate`.~~ ✅

Completed in the 2026-08 sweep against SDK 2.17.0 (the 0.9.0 release):

18. ~~Triggers resource (`/v1beta/triggers` CRUD + `:run`-equivalent
    `POST .../executions` + executions listing).~~ ✅ (`src/triggers.rs`,
    `Client::*_trigger*()`; list live-verified, create agent-gated — see
    verification notes above)
19. ~~Environments as a standalone resource (`/v1beta/environments` CRUD,
    complementing the inline `environment` request field).~~ ✅
    (`src/environments.rs`, `Client::*_environment()`; full lifecycle
    live-verified)
20. ~~`generation_config.transcription_config`, `safety_settings`,
    request `labels` (both Vertex-only), and the `AntigravityConfig`
    typed agent-config helper.~~ ✅ (`TranscriptionConfig`,
    `SafetySetting` in `src/safety.rs`, `with_labels()`/`add_label()`,
    `AntigravityConfig`)

## Found in the 2026-08-16 sweep against SDK 2.18.1 — NOT yet modeled

⚠️ **These are open gaps, not completed items.** Both were found by diffing
the bindings and confirmed by live probe; neither is in the crate yet.
Deliberately listed separately from the completed items above, because a
struck-through line reads as shipped and that is precisely the confusion
this file's header now warns about.

21. `Content::Video.processing` — segment clipping (`start_offset` /
    `end_offset`), `fps`, and the `static | agentic` mode enum.
    **Tracked in #419.**

    New in 2.18.x and **absent from `ai.google.dev`** — found only by
    diffing the bindings. Live-probed: the segment window is a ~127x
    token-cost lever (455 vs 57,775 video input tokens on one source
    video), while the mode and `fps` alone change nothing. Also
    position-sensitive — accepted only inside a `user_input` step (#427).

22. `generation_config.speech_config` widened to
    `SpeakerConfig | List[SpeechConfig]`. **Tracked in #420.**

    Live-probed: the Gemini API **rejects both object forms** on send
    (`400 ... Expected an array, got object`), so the crate should keep
    emitting the list; the gap is on deserialize, where the object form
    currently matches an all-optional `SpeechConfig` and silently discards
    every speaker.

## Spec-vs-implementation disagreements — ✅ ALL FIXED

- ~~`excludedPredefinedFunctions` serialized camelCase~~ → snake_case
  (legacy alias accepted on deserialize).
- ~~`FunctionCallingMode` serialized UPPERCASE~~ → lowercase.
- ~~`CodeExecutionLanguage` `"PYTHON"`~~ → `"python"`.
- ~~`top_k` in GenerationConfig~~ → removed.
- ~~`response_mime_type`~~ → removed (the API rejects it in all forms).
- ~~`Turn`-array input~~ → removed; history is steps.
- ~~`system_instruction` typed as InteractionInput~~ → plain string.
- ~~`total_reasoning_tokens` in usage~~ → removed.
- ~~`InteractionResponse` `rename_all = "camelCase"`~~ → snake_case.

## Verification protocol

Every change lands with wire-fixture tests derived from the generated SDK
bindings (see `src/steps.rs`, `src/wire_streaming.rs`,
`src/http/interactions.rs`, `tests/wire_format_verification_tests.rs`;
phase-2 fixtures in `src/webhooks.rs`, `src/environment.rs`,
`src/agents.rs`, `src/response_format.rs`, `src/tools.rs`, and
`tests/webhooks_and_agents_tests.rs`).
Before release: run the integration suite with a real `GEMINI_API_KEY` and
`LOUD_WIRE=1`, diff observed wire shapes against the fixtures, and update
`docs/ENUM_WIRE_FORMATS.md`.
