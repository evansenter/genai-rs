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

⚠️ Live wire verification with `LOUD_WIRE=1` + a real `GEMINI_API_KEY` is
pending (no key available in the migration environment). Diff observed wire
shapes against the fixture tests before release.

## Missing surface (by user value)

Completed in the revision-migration phase:

1. ~~`Api-Revision: 2026-05-20` migration (steps model + new SSE lifecycle).~~ ✅
2. ~~`tool_choice` restructure: lowercase enums or
   `{allowed_tools: {mode, tools}}`; remove crate's top-level `allowed_tools`
   inside generation_config.~~ ✅ (`ToolChoice` / `AllowedTools`)
3. ~~`cached_content` request field (explicit caching).~~ ✅
   (`with_cached_content()`)
4. ~~`service_tier`: `flex | standard | priority`.~~ ✅ (`ServiceTier`,
   `with_service_tier()`)
5. Webhooks: `webhook_config {uris, user_metadata}` on requests + full
   `/v1beta/webhooks` resource (CRUD, `:ping`, `:rotateSigningSecret`,
   events `batch.succeeded/expired/failed`, `interaction.requires_action/
   completed/failed`, `video.generated`). — **next phase**
6. ~~`include_input` query param on GET interaction.~~ ✅
   (`Client::get_interaction_with_input()`)
7. `retrieval` tool: `vertex_ai_search | rag_store | exa_ai_search |
   parallel_ai_search` + per-backend configs. — **next phase**
8. Video generation: `response_modalities: ["video"]`,
   `generation_config.video_config {task}`, video response_format
   (`gcs_uri`, `duration`, `delivery: uri`). — **next phase** (note: `video`
   content blocks and `video` stream deltas ARE already modeled)
9. Typed `response_format` union (text/audio/image/video) + list form +
   `delivery: inline|uri`. — **next phase** (`response_format` remains raw
   JSON for now)
10. Environments (`environment` request field, sources
    `gcs|inline|repository|skill_registry`, network allowlist) + Agents
    resource (`/v1beta/agents` CRUD). — **next phase** (note: the
    `environment_id` RESPONSE field is already modeled)
11. Multi-speaker TTS: `speech_config` as a list of `{voice, language,
    speaker}`. — **next phase** (crate still sends a single object)
12. ~~`presence_penalty` / `frequency_penalty` [-2, 2].~~ ✅
13. ~~Tool config completeness: GoogleMaps `latitude`/`longitude`; ComputerUse
    `enable_prompt_injection_detection`, `disabled_safety_policies`,
    `mobile|desktop` environments; GoogleSearch `enterprise_web_search`;
    MCP `allowed_tools` as `[{mode, tools}]`.~~ ✅
14. ~~`budget_exceeded` status (first-class); usage `grounding_tool_count`.~~ ✅
15. Deep-research config: `visualization`, `collaborative_planning`,
    `enable_bigquery_tool`; document agent IDs incl.
    `antigravity-preview-05-2026`. — **next phase**
16. ~~Typed citation annotations: `url_citation`, `file_citation`,
    `place_citation` (with review snippets); byte indices.~~ ✅
17. ~~Audio content `channels`/`sample_rate`.~~ ✅

## Spec-vs-implementation disagreements — ✅ ALL FIXED

- ~~`excludedPredefinedFunctions` serialized camelCase~~ → snake_case
  (legacy alias accepted on deserialize).
- ~~`FunctionCallingMode` serialized UPPERCASE~~ → lowercase.
- ~~`CodeExecutionLanguage` `"PYTHON"`~~ → `"python"`.
- ~~`top_k` in GenerationConfig~~ → removed.
- ~~`response_mime_type`~~ → `#[deprecated]`, still functional.
- ~~`Turn`-array input~~ → removed; history is steps.
- ~~`system_instruction` typed as InteractionInput~~ → plain string.
- ~~`total_reasoning_tokens` in usage~~ → removed.
- ~~`InteractionResponse` `rename_all = "camelCase"`~~ → snake_case.

## Verification protocol

Every change lands with wire-fixture tests derived from the generated SDK
bindings (see `src/steps.rs`, `src/wire_streaming.rs`,
`src/http/interactions.rs`, `tests/wire_format_verification_tests.rs`).
Before release: run the integration suite with a real `GEMINI_API_KEY` and
`LOUD_WIRE=1`, diff observed wire shapes against the fixtures, and update
`docs/ENUM_WIRE_FORMATS.md`.
