# Interactions API Gap Tracker

> ## ⚠️ This is a point-in-time snapshot, not a completeness guarantee
>
> | | |
> |---|---|
> | **Last swept against** | `google-genai` **3.0.0** |
> | **Sweep date** | **2026-11-07** |
> | **Baseline in CI** | `.github/last-swept-sdk-version` |
>
> "Done" below means *nothing was missing as of the sweep date*. The SDK
> ships new surface between sweeps. The daily `api-surface-sweep` workflow
> opens an issue when the bindings move past the baseline and closes it once
> a sweep catches up, so an open issue means this file is behind.

**Where dated live facts live (D-032).** A fact observed on the live API is
recorded with its date here or in `docs/ENUM_WIRE_FORMATS.md`, and nowhere
else. That file holds what a value or a shape looks like on the wire (enum
values, steps and deltas, resource bodies, paging); this one holds what the
API accepts, rejects and does, and what the crate models because of it. The
guides, the README and the rustdoc state the current behavior without dates
and link here. When a fact changes, update it here and in the one guide
sentence that states it. Antigravity harness findings concern a local
binary, not the API: they are keyed by harness version, and their dated
record is in `docs/ANTIGRAVITY_BRIDGE_DESIGN.md`.

## How to verify API surface

| Rank | Source | Why |
|------|--------|-----|
| 1 | **Generated bindings**: `google-genai`'s `_gaos/types/**` and the endpoint `path=` lines in `_gaos/*.py`, snapshot for the baseline in `tests/fixtures/api_surface/bindings.json` (written only by `.github/scripts/refresh_api_snapshot.sh`) | Ships ahead of prose. Diff two releases to find new surface. |
| 1b | **OpenAPI specs** (v1beta and v1), snapshot in `tests/fixtures/api_surface/` | Machine-readable; found 2026-11-01. It does not always agree with the bindings where both speak. Each lists query parameters the other lacks (five, under [Spec-only query parameters](#spec-only-query-parameters-live-2026-11-09)), and the spec wraps three request bodies the bindings send flat: webhook create (`{webhook: ...}`), webhook update and trigger update. The webhook create endpoint rejects the wrapper and takes the flat body (2026-11-09); the two updates' wrappers were not probed. The published URLs (`ai.google.dev/static/api/interactions.openapi.json`, `interactions-v1.openapi.json`) redirect to aistudio.google.com paths, which 404'd from about 2026-11-03 (no new public URL found on 2026-11-04). Since 2026-11-07 both serve OpenAPI 3.0.3 JSON again, from `aistudio.google.com/api-reference/interactions.openapi.json` and `interactions-v1.openapi.json`. The copies fetched that day differed from the 2026-11-01 copies only by surface google-genai 3.0.0 added, which the crate models (the code execution result's string-or-content union, the document MIME types, `source_text`), so the coverage oracle's spec passes needed no new mapping or reason. The committed snapshots were fetched on 2026-11-09, their schemas and endpoints unchanged from the 2026-11-07 copies, and each also keeps every operation's query parameters (with whether each is required) and request body. Each snapshot records its URL. The daily sweep (`watch_spec_urls.sh`) reduces what each URL serves and compares it with the snapshot, parameters and bodies included, and opens an `api-sweep` issue when one drifts or stops serving; on 2026-11-08 both matched. This is the one record of the URLs' state; `CLAUDE.md` and `docs/TESTING.md` point here. |
| 2 | **Live probes** against `generativelanguage.googleapis.com` | Ground truth, and it disagrees with rank 1 in both directions. |
| 3 | **Prose docs**: Google's, on `aistudio.google.com` (the old `ai.google.dev` URLs redirect there), and this file | Both lag. Neither is evidence of absence. |

Rank 2 disagrees with rank 1 in both directions, which is why nothing from
the bindings is modeled until it is probed. The 2.25.0 sweep found both:

- **The bindings listed surface the API rejected:** `cached_content` and
  `transcription_config.language_hints` (`400 Unknown parameter`), the
  `service_tier: "deferred"` value (named Vertex-only and modeled since the
  2.34.0 sweep), and the string arm of `environment.env`.
- **The API accepted surface the bindings lacked:** the tool types
  `filesystem`, `tool_search`, `manage_task`, `schedule`, `bash` (rejected
  with a 400 since the 2.34.0 sweep); the annotation types
  `in_context_file_citation` and `reference_metadata`; and extra usage and
  environment fields.

## Coverage oracle (2026-11-04)

Completeness against rank 1 is a CI check, not a sweep finding, and so is
most of it against rank 1b: `src/coverage_oracle/` and
`tests/request_wire_oracle.rs` check, offline against the committed
snapshot, every request key and every query key against the bindings,
every response field for an empty `extra` against the bindings and both
specs (v1beta and v1), and the specs' query parameters against the
bindings' and what the verbs send (since 2026-11-09); every binding class
and spec schema must be reached by those checks or listed with a reason;
and `tests/ci_coverage.rs` maps every endpoint to the handle verbs that
call it and requires a live test per handle verb. The spec's request
bodies are the exception: a request is checked against its binding class
only, since the spec lags the bindings on requests and wraps three of
them (see rank 1b above). How it works and how to refresh
the snapshot is in `docs/TESTING.md` ("Coverage oracle"). Its exception
lists are this file's working list: the `GAPS`, `NOT_SENT` and `UNMAPPED`
entries are what is not modeled yet. `GAPS` is empty since 2026-11-07,
when `File.downloadUri`, its last entry, was modeled.

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
- **In the snapshot, not modeled then:** all since modeled or reasoned
  out. Modeled: the `queued` status and the CodeMender agent config (#501),
  `Interaction.agent_config` and `environment`. Not modeled, with the
  reason: `continuation_ids`, `output_audio`/`_image`/`_video` (#501) and
  the spec's `max_total_tokens`. Each is under "Interaction residue"
  below. The resource fields this bullet listed (`Agent.agent_config`, the
  inline environment's `environment_id`, `embeddingModel`, the document
  upload and import configs, the `VoiceInput` fields, the store's document
  counts, `Document.customMetadata`, `File.downloadUri`,
  `FileStatus.details`) are under "Resource residue" below.
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

## Live behavior by area

The behavior the guides and the rustdoc state as current, with the date it
was last observed. A fact that a sweep turned up is in that sweep's section
below, and a wire shape is in `docs/ENUM_WIRE_FORMATS.md`; both are linked
from here rather than repeated.

### Models

- **`DEFAULT_MODEL` (`gemini-3.8-flash`)** rejects `thinking_level:
  "minimal"` with a 400. On 2026-09-24 the message was `'minimal' is not a
  supported thinking level for this model. Allowed values are: high, low,
  medium.`; after the server changes of late September it read `Thinking
  level THINKING_LEVEL_MINIMAL is not supported for this model.` (recorded
  2026-09-30, #469), and the request was a 400 again on 2026-10-01. Omitting
  the level does not turn thinking off. One short arithmetic puzzle
  (2026-09-24) spent these thought tokens:

  | Level sent | Thought tokens |
  |------------|----------------|
  | *(none)* | 103, then 222 on a second run |
  | `low` | 56 |
  | `medium` | 142 |
  | `high` | 260 |

  `thinking_summaries: "auto"` adds a `summary` to the thought steps;
  `"none"`, or no setting, adds none (2026-09-24).
- **Seeds:** three identical requests with the same `seed`
  (`DEFAULT_MODEL`, `thinking_level: "low"`) returned identical text
  (2026-09-24). The API documents no guarantee.
- **Output ceiling:** `GET /v1beta/models/gemini-3.8-flash` reports an
  `outputTokenLimit` of 65,536 (2026-09-24).
- **Images:** there is no 3.8 image model, and `gemini-3.8-flash` answers an
  image request with text only (2026-10-01), hence `DEFAULT_IMAGE_MODEL`.
- **Transcription:** `DEFAULT_TRANSCRIPTION_MODEL` (`gemini-3.5-transcribe`,
  new in the google-genai 3.0.0 `Model` literal; version
  `3.5-transcribe-08-2026`, 98,304 input and 32,768 output tokens) answers
  audio with its transcript and nothing else (2026-11-07, speech from
  macOS `say` and from `DEFAULT_TTS_MODEL`). It ignores a text prompt beside
  the audio (asked to translate, it transcribed), answers silence and
  text-only input with a `completed` interaction holding no steps and zero
  output tokens, rejects a function tool (`400 Function calling is not
  enabled for this model`), and streams the transcript as one `text` delta.
  `transcription_config` changes its output: a verbatim mode with
  `timestamp_granularities: ["word"]` and `diarization_mode: "speaker"`
  adds a `word_info` annotation per word (the deprecated top-level
  `diarization_mode` and `timestamp_granularities` do the same), and a
  smart mode's `source_text` made the transcript follow the script's
  spelling (`genai-rs` where the plain transcript had `JRS`). On
  `DEFAULT_MODEL` the same `source_text` was accepted with no effect, as
  every transcription setting had been on general models since 2026-09-24.
- **Decoding continuation** runs only on `CONTINUATION_MODEL` (2026-11-01);
  the rules are under [Sweep 2.25.0 → 2.34.0](#sweep-2250--2340-2026-11-01)
  and [Interaction residue](#interaction-residue-2026-11-05).
- **TTS models:** the output format and the multi-speaker rules per model are
  under Audio response and SpeechConfig in `docs/ENUM_WIRE_FORMATS.md`. A
  custom voice worked on the 3.8 and 2.5-pro TTS models;
  `gemini-3.1-flash-tts-preview` answered 500 (recorded without a date,
  from the 2.25.0 sweep's Voices work of 2026-09-24).

### Requests

- **Unmodeled fields** (`InteractionRequest::extra`,
  `CreateTriggerRequest::extra`): the server validates them like any field,
  so an unknown name is `400 Unknown parameter '<name>'`, and a key that
  names a modeled field overrides it, because the API keeps the last of a
  repeated key (2026-11-04). The `object: "interaction"` a stored trigger's
  interaction echoes, kept in `extra`, is accepted back on create
  (2026-11-04).
- **`response_mime_type`** is rejected even beside `response_format`: `400
  responseFormat must be set when responseMimeType is set` (2026-07), so it
  was removed.
- **Inline document content** is accepted for the twelve types of the
  bindings' `DocumentContentMimeType` (google-genai 3.0.0):
  `application/pdf`, `text/csv` and the Office/OpenDocument types (`.docx`,
  the Word template `.dotx`, `.xlsx`, `.pptx`, `.odt`, `.ods`, `.odp`, and
  the legacy `.doc`, `.xls`, `.ppt`). Each was read back from a real file on
  2026-11-05 except `.dotx` and the legacy three. A real `.dotx` (new in
  3.0.0's list) and a real legacy `.doc` (an OLE2 Word file) were read back
  on 2026-11-07. `.xls` and `.ppt` passed the MIME check only with
  plain-text bytes (2026-11-05), so parsing a real one is unverified
  (#530).
  Since 2026-11-04 `text/plain` and `text/markdown` are a 400 (#514), as
  is every other type tried (`text/html`, `application/json`,
  `application/xml`, `application/rtf`): `Unsupported mime_type '...' in
  document content` (2026-11-05). A `.txt` or `.md` file uploaded to the
  Files API still works as a document by URI (2026-11-05).
- **Content after history** is accepted: see InteractionInput in
  `docs/ENUM_WIRE_FORMATS.md` (D-031).

### Streaming

- **A dropped foreground stream cancels the interaction** (2026-11-01). It
  keeps only the input step and reports `cancelled` with an `errors` entry,
  and `interactions().stream(id)` answers `400 Streaming retrieval of
  interactions is not supported`. A foreground stream carries no event ids;
  only a background interaction's stream can be resumed.
- **The tail after `interaction.completed`** (2026-11-05, `DEFAULT_MODEL`):
  the API sends `event: done` / `data: [DONE]` and closes about a second
  later, and stores the interaction during that tail. A `GET` sent right
  after closing the connection at `interaction.completed` answered `500
  Internal error encountered`, the same `GET` ten seconds later `200`, and
  one sent after reading to the end `200` at once.
- **Content type:** a stream answers `text/event-stream` (2026-11-04).
- **Event sizes** (2026-11-04, `DEFAULT_IMAGE_MODEL` at `image_size:
  "4K"`): a 16.0 MB `image` delta of base64 JPEG and a 15.7 MB
  `thought_signature` delta in the same stream, which took 24 s; TTS audio
  deltas are under 8 KB.
- **A mid-stream error** arrives as a bare JSON line without `data:`, then
  EOF (2026-11-01, a recitation block); see the 2.34.0 sweep.

### Tools

- **MCP:** the call and result shapes, and how they changed, are under MCP
  identity in `docs/ENUM_WIRE_FORMATS.md`. An
  interaction can end in `requires_action` after every MCP call was
  answered (see [Server-side changes](#server-side-changes-live-2026-10-01)).
  A create whose `mcp_server` the API cannot use fails with a status that
  says why (2026-11-05): a server it cannot reach is `500 An internal error
  occurred while connecting to the MCP server.`, a URL that is no MCP
  server `501 MCP server returned an error. Please check the server
  configuration.`, and a malformed tool `400 Unknown parameter ...`. The
  501's message names the server's error, so a real server that answers
  with an error should get it too; that case was not observed. It is
  the one 501 that can pass, which is why `docs/RELIABILITY.md` and
  `docs/ERROR_HANDLING.md` single it out. `classify_mcp_create_error` in
  `tests/tools_and_config_tests.rs` sorts the three.
- **File search** can't be combined with `google_search` or `url_context`
  (2026-08-16: `400 '<other>' and 'file_search' cannot be combined in the
  same request. Please choose one to continue.`); `code_execution` is
  accepted. What its steps carry is under File Search in
  `docs/ENUM_WIRE_FORMATS.md`.
- **Structured output:** the `application/json` schema form is enforced on
  the output (2026-07); see ResponseFormat in
  `docs/ENUM_WIRE_FORMATS.md`.
- **Tool-use tokens:** declaring a tool without using it costs none. An
  interaction that declared an MCP server and answered from the model's own
  knowledge reported `total_tool_use_tokens` 0, as the same prompt with no
  tool did (2026-08-16).
- **Code execution:** `gemini-3.8-flash` runs only Python, and
  `gemini-4-argon` runs shell commands through Python's `subprocess`; the
  spec's `bash` language was never observed (2026-11-01). A result's
  `result` comes back as a string, never the list form the 3.0.0 bindings
  allow, and a plot or file the code makes arrives in the next
  `model_output` step (2026-11-07; the shapes are under Code execution in
  `docs/ENUM_WIRE_FORMATS.md`). A list-form result sent back in stateless
  history is accepted (`200`), but the model does not see its content:
  asked what a random-number program printed, `gemini-3.8-flash` and
  `gemini-3.5-flash` (served as `gemini-3.6-flash`) answered that no output
  was visible, while on `gemini-3.8-flash` the same value in the string form
  was read back exactly (2026-11-07).
- **`tool_choice`** (2026-11-07, `gemini-3.8-flash`, with a weather
  function declared): the four modes and the `allowed_tools` object are
  accepted and honored. `"auto"`, `"any"` and `"validated"` each produced a
  `function_call`, `"none"` a text answer with no call, and `allowed_tools`
  naming `get_time` a call to `get_time` only. The pre-revision uppercase
  modes are a `400 Invalid enum value 'AUTO' at
  'generation_config.tool_choice'`. The shapes are under ToolChoice and
  FunctionCallingMode in `docs/ENUM_WIRE_FORMATS.md`.
- **Google Search `search_types`** (2026-11-07): `enterprise_web_search`
  is Vertex-only (`400 The value 'enterprise_web_search' is not supported
  for 'tools[0].search_types[0]' on the Gemini API, it is allowed on the
  Gemini Enterprise Agent Platform. Supported values: 'web_search',
  'image_search'.`), kept in `SearchType` (D-005). `image_search` is
  accepted on `gemini-3.1-flash-image` and refused on `gemini-3.8-flash`
  (`400 Image search as tool is not enabled for
  models/gemini-3.8-flash`). What the call steps carry is under SearchType
  in `docs/ENUM_WIRE_FORMATS.md`.
- **A follow-up that drops a built-in tool can fail.** Tools are not
  carried over by `previous_interaction_id`, and after a turn that used
  `google_search`, a follow-up sent without it sometimes ends with the
  model calling a tool it no longer has: `status: "failed"` with
  `errors: [{"code": "malformed_tool_call", "message": "Model generated
  invalid JSON syntax and the output could not be parsed. Please retry the
  request. ..."}]` in a `200` (1 of 6 on 2026-11-07). In CI the follow-up
  failed in 13 of 99 runs from 2026-11-01 to 2026-11-06: as a `400
  malformed_tool_call` through 2026-11-04, as `status: "failed"` from
  2026-11-05, and twice as `requires_action`. Sent with the tool again, 6
  of 6 completed, answering from the history without a second search.
  On 2026-11-08 a follow-up without the tool that asked for a search
  again failed in 4 of 4: each a `200` with `status: "failed"`, no steps,
  and that one `malformed_tool_call` error, its message unchanged.
  Streamed, the same request ended in an `interaction.completed` whose
  interaction had `status: "failed"` and no `errors` (1 of 1). A function
  declared on turn 1 and left off the follow-up was called anyway
  (`requires_action`, 3 of 3), never refused. The integration suite's
  `retry_request!` retries the `200` form as it did the `400` (#545).
  The live jobs' logs of 112 CI runs from 2026-11-01 to 2026-11-07 show
  the `400` form in this follow-up and once in
  `test_structured_output_with_url_context`, last on 2026-11-04; after
  that the only test to fail on a `failed` interaction was this
  follow-up (4 runs), so no other test fails this way.
- **Google Maps `latitude` / `longitude`** are accepted and used: asked
  for "a coffee shop near me" with Times Square's coordinates, the model
  answered with one on West 47th Street (2026-11-07).

### Agents and environments

- **Custom agents** take a subset of the tool union: `code_execution`,
  `function`, `google_search`, `mcp_server` and `url_context` (the
  validation error, 2026-11-01; code execution, search and URL context only
  in 2026-07). Creating custom agents was gated on standard API keys
  through 2026-10-08 (every create was a generic `400 Request contains an
  invalid argument.`) and works since 2026-10-30.
- **Agent create** (2026-10-30): the one base agent accepted is
  `DEFAULT_ANTIGRAVITY_AGENT`, bare or as `agents/<id>` (stored bare); a
  Deep Research or model id is `400 Unsupported 'base_agent'.`, a missing
  one `400 'base_agent' must be specified.`, a missing id `400 Invalid agent
  id:`, and an existing id `409 Requested entity already exists`. Create
  echoes the agent, and create, get and list serve the same shape. Managed
  agent ids such as `DEFAULT_ANTIGRAVITY_AGENT` are not readable through
  `GET /v1beta/agents/{id}` (404).
- **Environments** do not expire in practice: on 2026-11-01 environments
  created in July and August were still `active`, three months after last
  use.
- **Environment file download** of a path with nothing at it, the root
  (`""`) included, is a `404 Directory not found or is empty` (2026-11-05).
- **`agent_config.max_total_tokens`** (2026-11-07): an Antigravity run
  with a budget of 1,000 stopped after one code execution step, at 4,976
  total tokens, and ended `incomplete`, not with the spec's
  `budget_exceeded` (deprecated in v1beta). That is one run with no
  unbudgeted run beside it, so it does not show that the budget stopped
  it.
- **`cancel` on `DEFAULT_ANTIGRAVITY_AGENT`** fails; see
  [Server-side changes](#server-side-changes-live-2026-10-01).
- **The retired agent:** `antigravity-preview-09-2026` replaced
  `antigravity-preview-05-2026`, which shut down on 2026-10-05; the new
  agent's built-in tools are renamed and take PascalCase parameters.

### Credentials

- **Lifecycle** (2026-09-24): create (bearer token, environment variable),
  get, list, patch with and without `update_mask`, and delete. The ID is
  optional on create (a UUID is assigned). An OAuth2 create checks that
  `token_url` is reachable.
- **Since about 2026-10-30** an `environment_variable` create needs at least
  one `trusted_domains` entry as well as an `injection_location`, although
  the google-genai 2.34.0 bindings still mark the domains optional.
- **Both references take effect at runtime** (2026-09-24, in a
  `DEFAULT_ANTIGRAVITY_AGENT` sandbox; `tests/credentials_tests.rs`): an
  `environment_variable` credential's variable holds a placeholder in the
  sandbox, and the egress proxy substitutes the secret into requests to its
  `trusted_domains`; a bearer credential on an allowlist entry is injected
  into requests to that domain.

### Triggers

- **The whole lifecycle** (2026-11-01, against a custom agent built on
  `DEFAULT_ANTIGRAVITY_AGENT`): create, get, list, update, run, list
  executions and delete. Before custom agents could be created (above), only
  `list` was verifiable.
- **The nested interaction** must target a custom agent: a model-only
  interaction gets `Agent '' is invalid or not found`, and `store` gets
  `Field 'store' is not allowed in 'interaction'` (2026-11-01). A malformed
  cron schedule gets only a generic `400 Request contains an invalid
  argument`.
- **Update** applies exactly the fields in the PATCH body, and only
  `display_name` and `status` are updatable: `schedule` is `400 Unknown
  parameter 'schedule'` (2026-11-01).
- **Run** fires asynchronously and answers `{}` (2026-11-01), so the firing
  is found through `list_executions`.
- **What it stores** (2026-11-01): a text input comes back as one
  `user_input` step, and the echo adds `store: true` and `object:
  "interaction"`. Without an `environment_id` on create, the first firing
  creates an environment and the trigger reports it from then on.
- **Shape** (2026-11-01): the ID is a bare UUID; int64s are plain JSON
  numbers; a paused trigger has no `next_run_time`, and `environment_id`,
  `last_run_time` and `previous_interaction_id` appear once it has fired;
  sending `status: "error"` is `400 Invalid status transition from active
  to error`. The executions list's envelope key is `trigger_executions`; an
  execution's `trigger_id` is bare, and a manual run's execution has no
  `scheduled_time` or `environment_id`. Paging and empty lists are under
  TriggerStatus in `docs/ENUM_WIRE_FORMATS.md`.

### Files

- **A file that doesn't exist** is a 403 whose message ends in "or it may not
  exist", not a 404 (2026-09-24).
- Uploads, paging and processing failures are under Files in
  `docs/ENUM_WIRE_FORMATS.md`, and the unmodeled verbs under
  [Resource residue](#resource-residue-live-2026-11-05).

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
  No uploaded or failed file carries `downloadUri`, nor did an upload on
  2026-11-07. The bindings set it only on a downloadable (generated) file,
  such as a video generation's, which no probe has made: it is modeled from
  the bindings since 2026-11-07 (`FileMetadata::download_uri`) and not yet
  seen live.
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
- **Deferred** (in the bindings, but the crate sends none of them:
  `tests/ci_coverage.rs` lists the three endpoints in
  `ENDPOINTS_WITHOUT_A_VERB`, `tests/request_wire_oracle.rs` the triggers
  `filter` in `NOT_SENT`, and `src/coverage_oracle/` the upload's `name` in
  `UNMAPPED`):
  `files.download` (an uploaded file is a `400 Only GENERATED files can be
  downloaded`), `files.register_files` (`401 API keys are not supported`),
  `files.upload(config={"name"})` (live 2026-11-07: the file is served
  under the chosen name, and a second upload under it fails at finalize
  with `409 ... already exists`; not modeled, D-039),
  `file_search_stores.download_media`
  (needs a `file_citation` `media_id`, never served) and
  `triggers.list(filter_=)` (every expression tried is a `400 Invalid filter
  expression`, syntax undocumented).

## Spec-only query parameters (live, 2026-11-09)

The spec gives three endpoints a query parameter the bindings lack, and the
bindings give two one the spec lacks. `tests/request_wire_oracle.rs` lists
each difference in `SPEC_QUERY` with its reason (`spec_query.rs`), along
with the parameters the spec marks required, each spec copy on its own. Probed on 2026-11-09, with the
test resources deleted afterwards. These endpoints take an unknown query
key without complaint (`bogus_param=1` on a webhook or agent create), so
being accepted shows nothing; what each did is below. None is modeled: an
option that changes nothing would claim a feature the API does not have
(D-010, D-NEXT-I1).

- **`webhook_id` on webhook create** (the spec: "If not specified, the
  server will generate a unique ID"): ignored. A create with
  `webhook_id=genairstest...`, with `webhookId=...`, or with a value
  holding spaces each answered 200 with a webhook whose ID the server
  generated.
- **The `{webhook: ...}` wrapper** the spec gives webhook create's body:
  `400 Unknown parameter 'webhook'.` The flat body the bindings send
  (`uri`, `subscribed_events`, `name`) is the one the endpoint takes.
- **`parent` on agent create** (the spec: required,
  `projects/{project}/locations/{location}`): ignored. A create with
  `projects/foo/locations/bar`, with `bogus`, with an empty value or with
  none made the same agent, served the same, and a listing with any of
  those `parent` values or none listed every agent. So `parent` on the
  listing, which the bindings define and `ListAgents::with_parent` sends,
  filters nothing either.
- **`filter` on a trigger's executions**: ignored. With one execution
  stored, every expression returned it: malformed ones (`(((`,
  `bogus ==`) and ones it does not match (`status = "failed"`,
  `trigger_id = "nope"`, `start_time > "2030-01-01T00:00:00Z"`). The
  triggers listing's own `filter` still parses its expression, and still
  rejects every one tried (`400 Invalid filter expression: status =
  "ACTIVE"`), as under [Resource residue](#resource-residue-live-2026-11-05).
- **The bindings' side:** `include_input` on an interaction read and
  `update_mask` on a webhook update are sent as the bindings define them;
  the spec lists neither.

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
  auto-function loops resolve it. From 2026-10-01 to 2026-11-04, MCP tools
  run server-side appeared as an answered `function_call` /
  `function_result` pair named `<server>:<tool>`, not `tool_call` steps,
  and `pending_function_calls()` left them out. Re-verified 2026-11-04,
  streamed and not: `<server>` was the declared `mcp_server` name. A
  declared client function named `weather:get` was accepted the same day,
  so the colon alone does not mark an MCP call. Since 2026-11-05 a
  non-streamed MCP call arrives as an `mcp_server_tool_call` step, and
  `mcp_calls()` reads both shapes back with server and tool split (#459;
  the shapes and their dates are under MCP identity in
  `docs/ENUM_WIRE_FORMATS.md`).
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
- **url_context loops under a response schema (server bug, 2026-11-01; still
  present 2026-11-07).** With `response_format` set to a JSON schema,
  `url_context` fetches the same page again and again, each fetch a
  `success`, until the API gives up with `400 too_many_tool_calls` after
  about two minutes: 8 of 12 requests on 2026-11-01. A loop ends after 60
  URL context calls (`The request reached the tool-use step limit for a
  single request before the model finished its answer. The model made 60 URL
  context calls. ...`; on 2026-11-01 it read `Model generated too many tool
  calls. Please retry the request. ...`). On 2026-11-07 the thinking level
  decided how often: 69 of 129 requests looped at the default level, 3 of 4
  at `thinking_level: "medium"` and 15 of 20 at `"high"`, against 9 of 219
  at `"low"` in back-to-back batches, and 3 of 61 single requests at `"low"`
  spread over the day. Loops came in bursts: in one batch that alternated
  the default and `low`, 38 and 4 of 60 looped. Above `low`, neither another
  page (three others), another prompt or schema, `tool_choice: "validated"`,
  a `seed` nor `temperature: 0` stopped it, and `tool_choice: "none"` does
  not stop url_context calls at all. `temperature: 0` at `low` looked better
  in back-to-back batches (1 of 255 looped) but not in single requests
  spread over the day (5 of 38). Without the schema, 4 of 4 answered after
  one fetch (6 of 6 on 2026-11-01), as did 4 of 4 with `application/json`
  output and no schema, and 24 of 24 two-turn chains at `low` that fetched
  first and asked for the schema on the next turn: 12 sent that turn
  without url_context, and 12 with it, where the model fetched the page
  again every time (12 is too few to tell from the single request's rate).
  `test_structured_output_with_url_context` sends `low`, once, and fails on
  a loop. Until 2026-11-07 it retried the loop up to 4 times at the default
  level instead: 41 of its 69 CI runs from 2026-11-01 needed a second
  attempt, and 6 looped on all 4.
- **Uploaded environment files carry no `size_bytes`** (2026-10-06), in the
  upload response or in listings; files from environment sources still
  report it.
- **Agentic video processing often failed (fixed by 2026-11-01).** On
  2026-10-01 2 of 6 identical raw-HTTP requests hung past 150 s; on
  2026-10-06 3 of 5 returned `500 Internal error encountered`. On 2026-11-01
  all 16 requests answered, 10 of them concurrent, so
  `processing_steps_tests` no longer retries.

## Sweep 2.34.0 → 3.0.0 (2026-11-07)

google-genai 3.0.0 was released 2026-11-06. Every item probed live on
2026-11-07; the coverage oracle passes against the 3.0.0 snapshot with the
two Files entries below listed in its `ALLOWED`.

### Modeled

| Surface | Where | Live result |
|---|---|---|
| `CodeExecutionResultStep.result` as `Union[List[Content], str]` | `CodeExecutionOutput` (`Text`, `Contents`) on `Step::CodeExecutionResult`; `CodeExecutionResultInfo::result`, `successful_code_output()` | The API sent the string on every run, plain and with a plot or files; a non-empty output read the same through the stored `GET` and arrived whole in one stream delta. Generated files come in the next `model_output` step. The list form is accepted back in history, but its content does not reach the model. The delta's `result` stays a string in the bindings and on the wire. See Tools above and Code execution in `docs/ENUM_WIRE_FORMATS.md` |
| `SmartTranscriptionMode.source_text` | `TranscriptionMode::Smart { source_text, .. }`, built with `TranscriptionMode::smart(..)` | Accepted and stored as sent; on `gemini-3.5-transcribe` it steers the transcript's wording, on `DEFAULT_MODEL` it has no effect. See Models above |
| `gemini-3.5-transcribe` in the `Model` literal | `DEFAULT_TRANSCRIPTION_MODEL` | The model on which `transcription_config` takes effect, and on which `word_info` annotations were first seen. See Models above |
| `DocumentContentMimeType` / `DocumentDeltaMimeType`: twelve types (pdf, csv, the legacy Word, Excel and PowerPoint types, the three OpenDocument types, and four OOXML types) | `document_from_file()` and `detect_mime_type()` (`.dotx` added); the coverage oracle holds the crate's document types to the bindings' list | All twelve accepted inline; a real `.dotx` (the new `wordprocessingml.template`) and a real legacy `.doc` read back; real `.xls` and `.ppt` files are unverified (#530). See Requests above |

### Deliberately not modeled

| Surface | Why |
|---|---|
| `legacy.ClaudeFile` and `ListFilesResponse.data` / `nextPage` | The Claude Files API's list shape, which the 3.0.0 bindings document as not supported in the Gemini API. The Gemini Files API list served `files` and `nextPageToken` only, with two uploaded files at `pageSize=1`, and rejected the `ids` parameter the new cursor describes (`400 Unknown name "ids"`). Listed in the coverage oracle's `ALLOWED`; either field lands in `FileListResponse::extra` if it comes. |

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
| `input` optional on create | Superseded: the API answered `400 Missing input.` in every form tried then. A stored decode's continuation now takes no input, and a request with neither input nor a continuation token is a misleading `404` that `build()` heads off ([Interaction residue](#interaction-residue-2026-11-05)) |
| `service_tier: "deferred"` | Superseded: the API answered `400 The value 'deferred' is not supported` then. The 2.25.0 → 2.34.0 sweep found it named Vertex-only and modeled it as `ServiceTier::Deferred` (D-005; [Sweep 2.25.0 → 2.34.0](#sweep-2250--2340-2026-11-01)) |
| String arm of `environment.env` | `Invalid input at 'environment'`. Preserved in `extra` if read. |
| Server-only tool types (`bash`, `filesystem`, `tool_search`, `manage_task`, `schedule`) | Superseded: absent from the bindings, they were accepted bare then, and `bash`/`filesystem` ended in `400 malformed_tool_call` while the rest did nothing on a raw model. The 2.25.0 → 2.34.0 sweep found them rejected with a 400 that lists the supported tool types ([Sweep 2.25.0 → 2.34.0](#sweep-2250--2340-2026-11-01)). |
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
