# Interactions API Gap Analysis (2026-07)

Status: implementation tracker — items are removed/checked off as they land.
Source: Google's `google-genai` Python SDK 2.10.0 generated API bindings
(machine-generated from the Interactions API spec), cross-checked against SDK
1.65/1.74/2.0 for protocol history. `ai.google.dev` doc endpoints were not
reachable from the analysis environment; all old-revision behaviors should be
re-verified live with `LOUD_WIRE=1` before and after migration.

## Headline: wire revision migration

The Interactions API is date-revisioned via an `Api-Revision` HTTP header.
Current spec revision: **`2026-05-20`** (sent by google-genai >= 2.0.0).
genai-rs sends no revision header and therefore speaks the launch-era
protocol. Differences:

| | Old revision (current crate) | Revision 2026-05-20 |
|---|---|---|
| Response body | `outputs: [Content...]` | `steps: [Step...]`; content nested in typed steps |
| SSE events | `interaction.start`, `content.start/delta/stop`, `interaction.complete` | `interaction.created`, `interaction.status_update`, `step.start`, `step.delta`, `step.stop`, `interaction.completed`, `error` |
| Tool calls | `Content` variants | Step variants: `user_input`, `model_output`, `thought`, `function_call`, `function_result`, `code_execution_call/result`, `url_context_call/result`, `google_search_call/result`, `mcp_server_tool_call/result`, `file_search_call/result`, `google_maps_call/result` |
| Thoughts | `Content::Thought { signature }` | `thought` step `{signature, summary: [content]}`; stream deltas `thought_summary`, `thought_signature` |
| Input | `str \| [Content] \| [Turn]` | `str \| [Step] \| [Content] \| Content` — `Turn` deprecated |
| Streaming fn args | n/a | `arguments_delta` (function-call args stream incrementally) |
| Usage | interaction-level | per-step `usage`/`step_usage` on `step.stop` + `metadata.total_usage` on lifecycle events |

## Missing surface (by user value)

1. `Api-Revision: 2026-05-20` migration (steps model + new SSE lifecycle).
2. `tool_choice` restructure: lowercase enums (`auto|any|none|validated`) or
   object `{allowed_tools: {mode, tools: [names]}}`; remove crate's top-level
   `allowed_tools` inside generation_config.
3. `cached_content` request field (explicit caching).
4. `service_tier`: `flex | standard | priority`.
5. Webhooks: `webhook_config {uris, user_metadata}` on requests + full
   `/v1beta/webhooks` resource (CRUD, `:ping`, `:rotateSigningSecret`,
   events `batch.succeeded/expired/failed`, `interaction.requires_action/
   completed/failed`, `video.generated`).
6. `include_input` query param on GET interaction.
7. `retrieval` tool: `vertex_ai_search | rag_store | exa_ai_search |
   parallel_ai_search` + per-backend configs.
8. Video generation: `response_modalities: ["video"]`,
   `generation_config.video_config {task}`, video response_format
   (`gcs_uri`, `duration`, `delivery: uri`), `video` content/delta blocks.
9. Typed `response_format` union (text/audio/image/video) + list form +
   `delivery: inline|uri`.
10. Environments (`environment` request field: sources
    `gcs|inline|repository|skill_registry`, network allowlist;
    `environment_id` response field) + Agents resource
    (`/v1beta/agents` CRUD: `base_agent`, `system_instruction`, tools,
    `base_environment`).
11. Multi-speaker TTS: `speech_config` as a list of `{voice, language,
    speaker}`.
12. `presence_penalty` / `frequency_penalty` [-2, 2].
13. Tool config completeness: GoogleMaps `latitude`/`longitude`; ComputerUse
    `enable_prompt_injection_detection`, `disabled_safety_policies`,
    `mobile|desktop` environments; GoogleSearch `enterprise_web_search`;
    MCP `allowed_tools` as `[{mode, tools}]`.
14. `budget_exceeded` status (first-class); usage `grounding_tool_count`.
15. Deep-research config: `visualization`, `collaborative_planning`,
    `enable_bigquery_tool`; document agent IDs incl.
    `antigravity-preview-05-2026`.
16. Typed citation annotations: `url_citation`, `file_citation`,
    `place_citation` (with review snippets); byte indices.
17. Audio content `channels`/`sample_rate`.

## Spec-vs-implementation disagreements (fix regardless of revision)

- `excludedPredefinedFunctions` serialized camelCase — spec:
  `excluded_predefined_functions` (src/tools.rs).
- `FunctionCallingMode` serialized UPPERCASE — spec: lowercase.
- `CodeExecutionLanguage` `"PYTHON"` — spec: `"python"`.
- `top_k` in GenerationConfig — dropped from spec.
- `response_mime_type` — deprecated; migrate to `response_format`.
- `Turn`-array input — deprecated.
- `system_instruction` — spec type is plain string.
- `total_reasoning_tokens` in usage — not in current spec.
- `InteractionResponse` uses `rename_all = "camelCase"` — spec wire is
  uniformly snake_case; verify live.

## Verification protocol

Every change lands with wire-fixture tests derived from the generated SDK
bindings. Before release: run the integration suite with a real
`GEMINI_API_KEY` and `LOUD_WIRE=1`, diff observed wire shapes against the
fixtures, and update `docs/ENUM_WIRE_FORMATS.md`.
