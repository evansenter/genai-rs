# Interactions API Gap Analysis (2026-07)

Status: implementation tracker — items are removed/checked off as they land.
Source: Google's `google-genai` Python SDK 2.10.0 generated API bindings
(machine-generated from the Interactions API spec), cross-checked against SDK
1.65/1.74/2.0 for protocol history. `ai.google.dev` doc endpoints were not
reachable from the analysis environment; all behaviors should be re-verified
live with `LOUD_WIRE=1` before release.

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

⚠️ Still pending live verification: the phase-2 surface expansion (webhooks,
environments, agents, retrieval, video config, typed response formats,
multi-speaker TTS — fixture coverage only) and the per-step usage shapes on
`step.stop`. Diff observed wire shapes against the fixture tests before
release.

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
