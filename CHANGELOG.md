# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `DEFAULT_TRANSCRIPTION_MODEL` (`gemini-3.5-transcribe`, new in google-genai
  3.0.0's model list), the speech-to-text model on which
  `TranscriptionConfig` takes effect: a verbatim mode with word timestamps
  annotates each word of the transcript with an `Annotation::WordInfo`
  (its span, offsets and speaker), and a smart mode's `source_text` steers
  the wording; general models accept the config without effect (verified
  live 2026-11-07). The model only transcribes: it ignores a text prompt
  beside the audio and rejects function tools. `examples/audio_input.rs`
  now transcribes speech it synthesizes with `DEFAULT_TTS_MODEL`, showing
  both modes.
- `document_from_file()` sends Word templates (`.dotx`,
  `application/vnd.openxmlformats-officedocument.wordprocessingml.template`)
  as document content, and `detect_mime_type()` maps the extension. The type
  is new in google-genai 3.0.0's document MIME list, which the crate's
  document types now match exactly; a real template was read back live
  (2026-11-07), as was a real legacy `.doc` file, which until then had only
  passed the API's MIME check.
- `InteractionResponse::mcp_calls()` returns the MCP tool calls the server
  ran, as `McpCallInfo { id, server, tool, arguments, result, is_error }`.
  It reads both shapes the API has used: since 2026-11-05 an
  `mcp_server_tool_call` with `server_name` and a `<server>:<tool>` name
  (the tool comes back without the prefix), and from 2026-10-01 to
  2026-11-04 an answered `function_call` named `<server>:<tool>`, where
  `<server>` is the name the request gave `McpServerConfig` (both verified
  live on `gemini-3.8-flash`). A client function may also contain
  a colon (the API accepted `weather:get` on 2026-11-04), so a call counts
  only when its result is in the same response. `is_error` is `Some(true)`
  when either the result step's `is_error` or the result object's own
  `is_error` key is `true`: on failed calls the API leaves the step's flag
  `false` (verified live 2026-11-04). The result object holds `result`,
  `call_tool_result_json` (the raw MCP `CallToolResult` as a JSON string),
  and `is_error: true` on failure or `structured_content` on success
  (#459).
- `extra()` and `extra_mut()` on `Step`, `StepDelta`, `Content` and
  `Annotation`, and `arguments_extra()` / `arguments_extra_mut()` on `Step`
  and `StepDelta`, expose the wire fields the crate does not model, for
  reading or editing before replay (#491).
- `extra()` and `extra_mut()` on `Tool`, `ResponseFormat`,
  `TranscriptionMode` and `ToolChoice` expose the wire fields the crate does
  not model, as on `Step`. New constructors build values with no extra
  fields: `Tool::code_execution()`, `Tool::url_context()`,
  `TranscriptionMode::smart(..)`, `TranscriptionMode::verbatim(..)`, and
  `ToolChoice::from(AllowedTools)`.
- `GoogleSearchResultItem`, `GoogleMapsResultItem`, `UrlContextResultItem`,
  `ReviewSnippet` and `StepError` have a public `extra` map for unmodeled
  fields, like `Place`. As on the unions, an
  entry named after a modeled field is never serialized.
- Antigravity: the Python SDK's harness options (#493), each shown changing
  what harness 0.1.23 does against a scripted model (2026-11-05).
  `docs/ANTIGRAVITY_BRIDGE_DESIGN.md` lists every option the 0.1.23 SDK
  exposes, modeled or with the reason it is not, and a CI test fails when a
  new wheel adds one the table lacks.
  - `SystemInstructions`: `custom` replaces the harness's prompt,
    `harness_default().with_identity(..).add_section(..)` edits it, and a
    string appends to it.
  - Model options: `with_thinking_level`, `with_service_tier`,
    `with_max_output_tokens`, `with_image_model`, `add_model_header`.
  - Subagent controls: `with_max_subagent_depth`, `with_allowed_subagents`,
    `with_define_subagent_tool`, and `Subagent::with_model`,
    `with_allowed_subagents` and `with_skills(SubagentSkills)`.
  - Harness config: `with_budget(Budget)`, `with_retry(RetryConfig)`,
    `with_compaction_threshold`, `with_tool_output_truncation`,
    `with_mcp_max_output_bytes`, `add_inline_skill(InlineSkill)`,
    `with_session_continuation(SessionContinuation)`, `add_harness_env`,
    `McpServer::with_force_all_tools_eager`,
    `Capabilities::with_run_command(RunCommandOptions)` (timeout, daemons,
    OS sandbox) and `AntigravityAgent::sandbox_status`.
  - Lifecycle hooks: `on_session_start`, `on_session_end`, `on_pre_turn`,
    `on_post_turn`, `on_tool_error` (its text replaces the error the model
    sees), `on_compaction` and `on_stop` (`StopDecision::Continue` keeps
    the turn going). A pre-tool hook can return `PreToolDecision::modify`
    to run the call on other arguments; on a tool confirmation, which
    cannot carry arguments, a modify denies the call.
  - Policies: `Policy::when` conditions, `with_reason`, `with_name`,
    `policy::ask_user` handlers, `mcp_<server>_*` and `mcp_*` targets, and
    `policy::auto()`, the harness's model-judged assessment of risky calls,
    whose flagged calls its handler approves or refuses (D-022). A
    condition or handler that panics denies the call. Server-wide targets
    match the call's MCP server exactly (`ToolInvocation::server`), so
    `mcp_git_*` does not cover a server named `git_admin`; `spawn()` warns
    when one server's name plus `_` starts another's, since a full
    `mcp_<server>_<tool>` name can then be read two ways.
  - Triggers: `add_custom_trigger` with a `TriggerContext`,
    `triggers::on_file_change`, and `AntigravityAgent::next_trigger_turn`,
    which drives a turn a trigger started and returns its response.
  - Prompts with media and slash commands: `chat` and `send_streaming`
    take a `Prompt` of text, `Media` (images, documents, audio, video, from
    bytes or `Media::from_file`) and `SlashCommand::Plan`, as the Python
    SDK's `chat` takes content primitives.
  - `ToolInvocation::new` and `ToolInvocation::mcp`, for unit-testing
    policy conditions and hooks.
  - Examples `lifecycle_hooks` (a stop hook as a definition of done),
    `file_watcher` (`on_file_change` with `next_trigger_turn`) and
    `command_guard` (conditional policies), each run against harness
    0.1.23 (2026-11-05).
- `AgentBuilder::with_gemini_base_url(url)` points the Antigravity harness's
  model calls at another Gemini API host: a gateway, a recording proxy, or a
  scripted local server in tests. It needs a non-empty `with_api_key`.
  Without a configured key, or with an empty one, the harness sends its own
  environment's `GEMINI_API_KEY` to the custom host (verified on harness
  0.1.23, 2026-11-04), so `spawn()` fails with `AntigravityError::Config`
  instead. While a base URL is set, the harness also starts without
  `GEMINI_API_KEY` and `GOOGLE_API_KEY` in its environment, so it has no key
  to fall back on; commands the agent runs (`run_command`) don't see them
  either.
- `AntigravityAgent::harness_version()` returns the `google-antigravity`
  version of the running harness, read from the wheel install the binary
  sits in (`None` outside one). `spawn()` logs a `warn!` when it differs
  from `SUPPORTED_HARNESS_VERSION`. It warns rather than fails (D-019),
  because a mismatch's failures are already loud and running a newer wheel
  on purpose is how a bump is tested. An `InitFailed` message and a turn's
  `Timeout` diagnosis name a known mismatch.
- `document_from_file()` sends `.csv` (`text/csv`) and the Office/OpenDocument
  formats (`.docx`, `.xlsx`, `.pptx`, `.odt`, `.ods`, `.odp`, and the legacy
  `.doc`, `.xls`, `.ppt`) as document content; it rejected them before. With
  PDF, these are the inline document types the API accepts (verified live
  2026-11-05 by reading back a real file of each type; the legacy `.doc`,
  `.xls` and `.ppt` only passed the API's MIME check with plain-text bytes,
  so parsing a real legacy file is unverified). `detect_mime_type()`, and so
  `FileUpload::from_path()`, now recognizes the nine Office/OpenDocument
  extensions (#516).
- `InteractionRequest::extra` and `InteractionBuilder::add_extra(key, value)`
  send a request field the crate doesn't model yet, flat beside the modeled
  ones, as `CreateTriggerRequest::extra` already allowed. A repeated key
  replaces its value, as with `add_label`. The server validates the field
  like any other: an unknown name is `400 Unknown parameter '<name>'`. A key
  that names a modeled field overrides it, since the API keeps the last of a
  repeated key (both verified live 2026-11-04). The exception is `stream`,
  which belongs to the transport: `execute()` and `execute_stream()` drop it
  from `extra`, with a `warn!`, so it can't turn a non-streaming call into
  an SSE answer. A stored
  `Trigger::interaction` now keeps the interaction fields the crate doesn't
  model, such as `object: "interaction"`, which the API accepts back on
  create (verified live 2026-11-04).
- `DEFAULT_CONNECT_TIMEOUT` (30 s), the connect timeout every client now
  gets; see Changed. `with_connect_timeout(Duration::MAX)` turns it off,
  with no timer at all, for tests on a paused tokio clock. A `Client`'s
  `Debug` output shows its connect timeout, which reqwest's does not.
- Three crate features, `http2`, `system-proxy` and `charset`, each turning
  on reqwest's feature of the same name. All three are on by default; see
  Changed.
- `InteractionStatus::Queued` (`"queued"`), from the google-genai 2.34.0
  bindings, where the interaction, the stream's interaction and its status
  updates all list it (#501). It used to read as `Unknown`. Not observed
  live: on 2026-11-05 background agent and model interactions reported
  `in_progress` from their create response on.
- `InteractionResponse` models the request fields a stored interaction
  serves, which used to land in `extra`: `generation_config`,
  `response_format`, `response_modalities` and `safety_settings` (a stored
  read carries all four, and the create response echoes
  `safety_settings`), and on an agent interaction `agent_config` and
  `environment` (the create response and the stored read echo
  `agent_config`, the stored read `environment`; all verified live
  2026-11-05). `generation_config.speech_config` reads the
  `{"speakers": [...]}` form the read serves into the same list as the
  request's form. With `get_with_input` every list in them repeats each
  entry (live 2026-11-04 and 2026-11-05); the crate keeps what is served.
- `GenerationConfig::extra` keeps the generation-config fields the crate
  does not model, from a stored read or a deserialized config, and sends
  them flat beside the modeled ones.
- `InteractionBuilder::continue_decode(&response)` continues a decode that
  stopped early without resending the input: it sets the response's model,
  its id as the previous interaction and its `continuation_token`, replacing
  any id or token set earlier on the builder, and clearing them when the
  response has none (a token continues only its own interaction). The API
  continues a stored interaction from those alone, and with no
  `max_output_tokens` the continuation ran to the end (verified live
  2026-11-05 on `CONTINUATION_MODEL`). `build()` names the problem when the
  response has no token, or no id (a `store: false` interaction, whose
  continuation must resend its input with `with_continuation_token`) (#489).
- `ClientBuilder::with_api_version(version)` replaces the `v1beta` path
  segment of every URL; `"v1"` selects the stable endpoint (#489). Measured
  live 2026-11-05: `/v1` serves interactions with the same protocol as
  `/v1beta` (create, get, delete, cancel, streaming, and decoding
  continuation, which the v1 spec leaves out), the Files API (uploads
  included, under `/upload/v1`) and the File Search Store API, and answers
  404 for agents, environments, webhooks, credentials, voices and triggers.
  An interaction created on one version reads on the other. `v1beta` stays the
  default, since only it has every resource.
- `CodeMenderConfig`, with `CodeMenderFindRequest`, `CodeMenderFixRequest`,
  `CodeMenderFile` and `CodeMenderMode`, builds the CodeMender agent config
  of the google-genai 2.34.0 bindings (`agent_config.type: "code-mender"`:
  a find or fix request over source files, a model, a session id and
  `session_config.max_rounds`) (#501). It is Vertex-only: the Gemini API
  answers `400 The value 'code-mender' is not supported for
  'agent_config.type' on the Gemini API, it is allowed on the Gemini
  Enterprise Agent Platform` (live 2026-11-05), so it is modeled and
  documented per D-005 but not exercised live.
- `FileSearchStores::import_file(store_name, import)` imports a Files API
  file into a file search store, waits for the import operation, and
  returns the new `FileSearchDocument`. It takes a file name, a
  `&FileMetadata`, or a `DocumentImport` that adds custom metadata, a
  chunking config and the wait's `PollOptions` (default 60 s, every
  500 ms). The document is named after the file's ID and is a copy, so
  deleting the file leaves it. The document exists, pending, under the
  operation's ID as soon as the import is accepted, and a second import
  makes a second document, so a timeout or a failed poll returns an error
  that names the operation and that document, and says not to import again
  (all verified live 2026-11-05).
- `DocumentUpload` wraps a `FileUpload` with the custom metadata and
  chunking config of a document `FileSearchStores::upload` adds.
  `CustomMetadata` (`string`, `numeric`, `string_list`, with `StringList`
  for the last) and `ChunkingConfig::white_space(max_tokens_per_chunk,
  max_overlap_tokens)` (with `WhiteSpaceConfig`, which keeps unmodeled
  fields in `extra` like `ChunkingConfig`) build them, and
  `FileSearchDocument::custom_metadata` reads them back, in the order sent
  (verified live 2026-11-05). An upload or import whose white-space overlap
  is not smaller than its chunk fails with `GenaiError::InvalidInput` before
  any request: the API answers it with a 500 (live 2026-11-05).
- `CreateFileSearchStoreRequest::embedding_model` and
  `with_embedding_model` pick a store's embedding model. Unset, the API
  uses `models/gemini-embedding-001`; an unknown model is a 400 (verified
  live 2026-11-05).
- `FileSearchStore::active_documents_count`, `pending_documents_count`,
  `failed_documents_count` and `size_bytes`, read from the API's string
  int64s. The first two and the size were served live on 2026-11-05;
  `failed_documents_count` was not seen.
- `FileMetadata::update_time` and `FileMetadata::source` (the new
  `FileSource` enum: `Uploaded`, `Generated`, `Registered`), served on
  every file (verified live 2026-11-05). `FileError::details` and
  `FileError::extra` keep a failed file's `google.rpc.Status` whole.
- `FileMetadata::download_uri`, the bindings' `downloadUri`, which they
  set only on a file the API generated (`FileSource::Generated`). It was
  the last field the coverage oracle listed as not modeled. It is modeled
  from the bindings and not yet seen live: an uploaded file carries none
  (live 2026-11-07), so it is `None` there.
- `Agent::agent_config` and `Agent::with_agent_config(AntigravityConfig)`
  set the base agent's configuration. It is stored and echoed,
  `max_total_tokens` as a string; a config of another type is stored as a
  bare `{"type": "antigravity"}`, its settings lost (verified live
  2026-11-05), so the setter takes only an `AntigravityConfig`.
- `VoiceSpec::description`, `language_code`, `region_code`, `accent`,
  `persona`, `context`, `gender` and `pitch`, set with the matching
  `CreateVoiceRequest::with_*` builders. A created voice stores and echoes
  them, and the list filters match them (verified live 2026-11-05). The
  bindings' `sample_audio` is output only: a value sent is ignored, so it
  is not modeled on the request.
- `RemoteEnvironment::environment_id` and `with_environment_id` run an
  agent interaction in an existing environment, with its files: the live
  test's agent reads back a file seeded into it. The interaction reports
  the ID as its `environment_id`; an ID that does not exist is a 404. On an
  agent's `base_environment` the API accepts and drops it (all verified
  live 2026-11-05).
- `EnvironmentFiles::download(environment_id, path)` reads a file out of an
  environment, as an `EnvironmentFileDownload` (its bytes and served
  content type); a directory comes back as a tar archive
  (`is_tar_archive()`). It is `GET` on the listing's path with `alt=media`
  (verified live 2026-11-05).
- `ConversationBuilder` implements `Debug`, as every other public type
  does: it prints the parent `InteractionBuilder` (without the client, so
  without the API key) and the steps so far.

### Changed

- **Breaking: every known `StreamChunk` variant has an `extra` field, and
  `StreamChunk::Completed` is a struct variant,
  `Completed { interaction, extra }`** (#538). `extra` is a
  `serde_json::Map<String, serde_json::Value>` holding the SSE event's keys
  the variant does not read, as they came, such as a key the API adds
  beside the payload, which a `StreamChunk` used to drop;
  `StreamChunk::extra()` reads it, and the chunk's serialized
  `{chunk_type, data}` form writes and reads it as `extra`. It is empty for
  every event the API sends today. A key the crate reads on another event
  type is no longer parsed on one that does not read it, so a value the
  crate could not read there no longer fails the event. The auto-function
  stream's `Delta` carries the `step.delta` event's `extra` too (below).
  Migration:
  `StreamChunk::Completed(response)` becomes
  `StreamChunk::Completed { interaction: response, .. }` (or
  `Completed { interaction, .. }`); a pattern that lists every field of a
  known variant adds `..` (`StreamChunk::Error { message, code, .. }`); a
  struct literal adds `extra: Default::default()`.
- **Breaking: `AutoFunctionStreamChunk::Delta` is a struct variant,
  `Delta { delta, extra }`, and the auto-function stream passes on an SSE
  event this crate does not recognize as `AutoFunctionStreamChunk::Unknown`**
  (#504). `extra` holds the `step.delta` event's keys the crate does not
  read, as `StreamChunk::StepDelta`'s `extra` does; the chunk's
  `{chunk_type, data}` form writes a non-empty one beside `data` and reads
  it back. An unknown event was dropped with a warning; it now reaches the
  caller with its type, JSON and `event_id`, and the round goes on to its
  `interaction.completed`. An `Unknown` chunk named like a known one is
  written with `"unknown": true`, so it reads back unknown. Migration:
  `AutoFunctionStreamChunk::Delta(delta)` becomes `Delta { delta, .. }` in
  a pattern and `Delta { delta, extra: Default::default() }` in an
  expression; a match's wildcard arm, which the `#[non_exhaustive]` enum
  already needs, now also sees `Unknown`.
- **Breaking: `CreateFileSearchStoreRequest::with_extra(key, value)` is
  `add_extra(key, value)`**, the name `InteractionBuilder` gives the same
  operation: an `add_*` setter adds an entry where a `with_*` one replaces
  (`docs/BUILDER_API.md`). It still adds one unmodeled field, keeping the
  ones added before, and a repeated key replaces its value. Migration:
  rename the call; its arguments are unchanged.
- **Breaking: `FunctionDeclarationBuilder::add_parameter` takes the name as
  `impl Into<String>`, and `InteractionBuilder::add_functions` takes
  `impl IntoIterator<Item = FunctionDeclaration>`**, in place of `&str`
  and `Vec<FunctionDeclaration>`. The name takes what the builder's `new`
  and `with_description` take, so a `String` needs no `&`, and an array or
  a mapped iterator of declarations needs no `vec!` or `collect()`.
  Migration: a `&str`, `&String`, `String` or `Vec` argument compiles
  unchanged. A `&&str` name (what iterating over `&["a", "b"]` yields), a
  `&Box<str>` or a `&Cow<str>` has no `Into<String>`: dereference it
  (`*name`) or pass `name.to_string()`. A `collect()` passed straight to
  `add_functions` can no longer infer its collection (E0283): pass the
  iterator without it, or write `collect::<Vec<_>>()`.
- **Breaking: `TranscriptionMode::Smart` is a struct variant,
  `Smart { source_text: Option<String> }`**, for google-genai 3.0.0's
  `SmartTranscriptionMode.source_text`: the text the audio is expected to
  say, which the transcript follows (accepted and stored as sent, verified
  live 2026-11-07). A stored interaction's smart mode now keeps it, where
  the unit variant dropped it. Migration: `TranscriptionMode::Smart` becomes
  `TranscriptionMode::smart(None)` in an expression and
  `TranscriptionMode::Smart { .. }` in a pattern.
- **Breaking: `Step::CodeExecutionResult`'s `result` is a
  `CodeExecutionOutput`, not a `String`.** google-genai 3.0.0 types it as
  the output as a string or as a list of content blocks. The new enum reads
  both (`Text`, `Contents`) and writes each back in the form it came in, so
  a list-form result no longer turns the step into `Step::Unknown`.
  `CodeExecutionResultInfo::result` and
  `InteractionResponse::successful_code_output()` now give a
  `&CodeExecutionOutput`. The Gemini API still sends the string, and it
  accepts the list form in replayed history without showing its content to
  the model, so replay the string form (verified live 2026-11-07). Migration:
  `result.text()` reads either form as text (a `Cow<str>`), `as_text()`
  returns the string form, and `Display` prints the text, so
  `println!("{}", r.result)` is unchanged; build one with `"...".into()` or
  `vec![Content::text("...")].into()`. `StepDelta::CodeExecutionResult`'s
  `result` stays a `String`, as in the bindings.
- **Breaking: every known variant of `Step`, `StepDelta`, `Content` and
  `Annotation` has a new `extra` field.** It is a
  `serde_json::Map<String, serde_json::Value>`. The call variants
  `CodeExecutionCall`, `UrlContextCall`, `GoogleSearchCall`,
  `GoogleMapsCall` and `RetrievalCall` (on both `Step` and `StepDelta`) also
  have `arguments_extra`, and `VideoProcessing::StaticSegment` gains `extra`.
  The enums are `#[non_exhaustive]` but their variants are not, so patterns
  that list every field and struct literals stop compiling. Migration: add
  `..` to such patterns (`StepDelta::Text { text, .. }`,
  `Step::Thought { signature, summary, .. }`). In struct literals, add
  `extra: Default::default()` (and `arguments_extra: Default::default()`),
  or use the constructors (`Step::user_text`, `Step::function_call`,
  `Content::text`, `Content::image_data`,
  `VideoProcessing::segment()...build()`), which set them empty.
- **Breaking: every known variant of `Tool`, `ResponseFormat` and
  `TranscriptionMode` has a new `extra` field, `Tool::CodeExecution` and
  `Tool::UrlContext` are struct variants, and `ToolChoice::AllowedTools` is
  `AllowedTools { allowed_tools, extra }`.** A stored interaction serves
  these back (`tools`, `response_format`, and `generation_config`'s
  `transcription_config.mode` and `tool_choice`), and a field the crate does
  not model now survives the round trip in `extra` instead of being
  dropped; for `tool_choice` that is a key beside `allowed_tools`. A known
  transcription mode with a mistyped field is kept whole as
  `TranscriptionMode::Unknown`, where it used to be read leniently, dropping
  the bad value. Migration: in a struct literal add
  `extra: Default::default()`, or use the constructors
  (`TranscriptionMode::smart(..)`, `TranscriptionMode::verbatim(..)`,
  `ResponseFormat::json_schema(..)`, the tool configs such as
  `GoogleSearchConfig`); add `..` to a pattern that lists every field.
  `Tool::CodeExecution` and `Tool::UrlContext` become
  `Tool::code_execution()` and `Tool::url_context()` as values and
  `Tool::CodeExecution { .. }` / `Tool::UrlContext { .. }` in patterns.
  `ToolChoice::AllowedTools(allowed)` becomes `ToolChoice::from(allowed)`,
  and the pattern `ToolChoice::AllowedTools { allowed_tools, .. }`.
- **Breaking: `Annotation::speech_metadata` is no longer a `const fn`.** Its
  `extra` map cannot be built in a const context. Migration: call it at
  runtime.
- **Breaking: the request configs a stored interaction serves back have a
  new `extra` field**: `SpeechConfig`, `ImageConfig`, `VideoConfig`,
  `TranscriptionConfig`, `AllowedTools`, `SafetySetting` and
  `WebhookConfig`. It keeps the fields the crate does not model, from a
  response's `generation_config`, `safety_settings` and `webhook_config` or
  any other deserialized config, and sends them flat beside the modeled
  ones, as `GenerationConfig::extra` already did; a round trip used to drop
  them. `SafetySetting::new` is no longer a `const fn`. Migration: in a
  struct literal, add `..Default::default()` (`extra:
  serde_json::Map::new()` for `SafetySetting`, which has no `Default`), or
  use the constructors (`SpeechConfig::for_speaker`, `AllowedTools::new`,
  `SafetySetting::new`, `VideoConfig::new`, `WebhookConfig::new`).
- **Breaking: the Antigravity types read from the harness that dropped
  unmodeled fields have a new `extra` field**, as their siblings do:
  `ModalityTokenCount`, `TrajectoryUsageEntry`, `ListDirectoryEntry`,
  `DiffBlock`, `DiffLine`, `PreTurnArgs`, `PostTurnArgs`,
  `OnToolErrorArgs`, `OnCompactionArgs`, `StopArgs`, and the input types
  the harness echoes back (`UserInput`, `UserInputPart`, `Media`,
  `SlashCommand`, `ToolResponse`). `OutputEvent::extra` keeps an envelope
  key a newer harness adds beside a recognized payload, which used to be
  dropped. Migration: add `..Default::default()` to a struct literal.
- **Breaking: an Antigravity harness enum (`TrajectoryState`, `StepState`,
  `LifecycleHook`, ...) read from a non-string value names it
  `<non-string: 7>`, not `7`.** That is what `unknown_<context>_type()` and
  `as_wire_str()` return and what `drift_report()` counts it under, as the
  API enums do. The value is written back unchanged, `7`; it used to be
  written as the string `"7"`. Migration: compare against
  `<non-string: …>` where code compared the raw JSON text, or read the value
  itself from `unknown_data()`.
- **Breaking: Antigravity's `protocol::OutputEvent` has a new field,
  `usage_update`.** It holds the rest of a harness `usageUpdate` beside its
  total: the usage of each trajectory (`UsageUpdate::agents`, as
  `TrajectoryUsageEntry`s) and any field the crate does not model.
  `usage_metadata` still holds the total. The breakdown used to be dropped,
  and an event read from a `usageUpdate` serialized back as a flat
  `usageMetadata`; it now round-trips as it came. Migration: add
  `..Default::default()` to an `OutputEvent` struct literal.
- **Breaking: a known step, delta, content block or annotation whose
  modeled field is malformed now deserializes as `Unknown`.** `Unknown`
  keeps the whole object. Examples: a non-object `arguments`, a non-string
  element in `urls` or `queries`. These used to be read leniently, dropping
  the bad value. A static video `processing` object with a mistyped field
  becomes `VideoProcessing::Unknown`. In strict mode, a malformed
  `Step` or `Content` is an error that names the field and why
  (``field `arguments`: expected an object``) instead of calling the type
  unknown; that includes an unrecognized `language` on
  `code_execution_call`, which strict mode used to read as Python. A
  streamed `StepDelta` holding such a value (that `language`, or an unknown
  content type in a `thought_summary`) fails the same way; strict mode used
  to read the delta's `language` as absent. Other malformed deltas are
  still kept as `StepDelta::Unknown`. Well-formed API responses are
  unaffected. Migration: none, unless you
  relied on the lenient reading.
- **Breaking: a string passed to Antigravity's `with_system_instructions`
  is added to the harness's own prompt instead of replacing it**, as in the
  Python SDK, so the harness's tool guidance, safety rules, workspace list,
  skills list and built-in subagents stay (D-023). The same holds for
  `Subagent::with_system_instructions`, whose sections are now titled
  `user_system_instructions` instead of `System`. The workspace note this
  crate adds now goes only with custom instructions; the harness's prompt
  lists the workspaces itself (verified on 0.1.23). Both setters take
  `impl Into<SystemInstructions>`. Migration: to replace the prompt as
  before, pass `SystemInstructions::custom(text)`. A `&str`, `String`,
  `&String`, `Cow<str>` or `Box<str>` compiles unchanged, while a value of
  a generic `T: Into<String>` needs `.into()` to a `String` first.
- **Breaking: Antigravity's `chat` and `send_streaming` take
  `impl Into<Prompt>` instead of `impl Into<String>`.** Migration: a
  `&str`, `String`, `&String`, `Cow<str>` or `Box<str>` argument compiles
  unchanged. A value of a generic `T: Into<String>` does not: call
  `.into()` to make it a `String` first.
- **Breaking: `BuiltinTool::StartSubagent` is `BuiltinTool::InvokeSubagent`,
  and the subagent tool is named `invoke_subagent` everywhere**: in
  `ToolAction::tool_name()`, in the pre-tool hook's `ToolInvocation`, and as
  a policy target. Migration: rename the variant and any string match;
  `policy::deny("start_subagent")` and the other constructors still target
  it.
- **Breaking: enabling the subagent builtin no longer offers the model
  `define_subagent`**, with which it writes subagents of its own at runtime
  (D-023). Migration: call `with_define_subagent_tool(true)` to keep it.
- **Breaking: Antigravity's `spawn()` refuses subagents beside a gate they
  bypass** (#520, D-042). Harness 0.1.23 applies neither policies,
  the pre-tool hook nor workspace containment to a subagent's own tool
  calls, so a read-only subagent can read any file the process can
  (verified 2026-11-05, with deny-all rules, a denying hook and containment
  all set); the built-in `research` subagent, which `invoke_subagent`
  always offers, is no different. The docs said the pre-tool callback gated
  subagent calls. An agent that enables `BuiltinTool::InvokeSubagent` or
  `with_define_subagent_tool(true)` now fails with
  `AntigravityError::Config` naming the gates when any is set: a pre-tool
  hook, a deny, confirm or `ask_user` policy, `policy::auto()`, workspace
  containment (on by default with a workspace), or the `run_command`
  sandbox with `run_command` enabled. Policies that only allow, such as
  `allow_all()`, are not gates.
  Migration: give each subagent only the builtins it may use unchecked and
  add `.with_ungated_subagents(true)`, or drop the gates or the subagent
  builtin.
- **Breaking: Antigravity policy precedence follows the harness's
  evaluator.** A named tool still beats `mcp_<server>_*`, which beats `"*"`;
  within a tier, deny now beats confirm/ask, which beats allow, where the
  first registered rule used to win. `[allow("t"), deny("t")]` now denies
  `t`. `Policy` is no longer `PartialEq`/`Eq`, as it can hold closures.
  Migration: remove the rule you do not want, or narrow it with `when`, and
  drop any `==` between two policies.
- **Breaking: a call the pre-tool hook or a policy denied surfaces as
  `ToolAction::Denied { name, args }`**, named and with the arguments it was
  judged on. It was a `ToolAction::Error` named `"error"`, as the harness's
  error step carries no action. Migration: match on `Denied` where you
  matched that `Error` (the event's decision is unchanged).
- **Breaking: `AntigravityAgent::shutdown()` returns
  `AntigravityError::HarnessExited`** with the exit code and stderr tail when
  the harness exits with a failure status after stdin closes, as the Python
  SDK raises. Escalated exits (SIGTERM, SIGKILL) still return `Ok`.
  Migration: handle the `Err` where `shutdown()` was treated as
  infallible, or discard it (`let _ =`) where a failed exit after the
  session does not matter.
- **Breaking: Antigravity protocol types changed with the new options.**
  `HarnessSideTools.run_command` and `.subagents` are `RunCommandToolConfig`
  and `SubagentsConfig`; `InputEvent::CallHookResponse` and
  `OutputPayload::CallHookRequest` hold a `Box`; `HookVerdict`,
  `GeminiModelOptions`, `McpServerConfig`, `CustomAgent`,
  `SystemInstructionPart`, `PolicyConfig` and `HarnessConfig` gained fields.
  Migration: `ToolToggle::new(on).into()` builds the old shape of the two
  tools, `Box::new` wraps the hook payloads, and `..Default::default()`
  fills the new fields.
- **Breaking: Antigravity hooks run on a dedicated thread, not on the
  thread driving the turn.** A hook that blocks (`on_pre_tool`,
  `on_post_tool`, `on_questions`) therefore no longer stalls the runtime,
  and the turn deadline still fires. A hook abandoned at the deadline runs
  on until it returns, but holds up neither runtime shutdown nor process
  exit. A hook no longer sees the turn's thread-local state, a tokio
  `task_local!` scoped around the turn, or the turn's current tracing span.
  `tokio::runtime::Handle::current()` still works inside one. A panicking
  hook still panics the turn. Migration: take such state in through the
  hook's captures, and enter a span inside the hook if its events should
  nest under one.
- **Breaking: the Antigravity turn deadline covers this crate's own work in
  the turn.** A custom tool or hook still running when `with_turn_timeout`'s
  budget runs out now ends the turn with `AntigravityError::Timeout`, whose
  `operation` names it (`agent turn (custom tool 'x' was still running at
  the deadline)`). Before, only waiting for harness events was bounded. Each
  custom tool call runs on a tokio task of its own that the deadline aborts,
  so an async tool stops at its next `.await`. A synchronous tool body that
  blocks its thread is abandoned on time on a multi-thread runtime with two
  or more workers. On a current-thread runtime, or a multi-thread one with a
  single worker (`#[tokio::main]` on a one-CPU host or a container limited
  to one CPU), it holds the thread that drives the timer until it returns;
  make such a tool `async`, or call `spawn_blocking` inside it. A tool that
  read a tokio `task_local!` of the turn's task no longer sees it; pass the
  value as an argument or through the tool service's state. Migration:
  raise the budget, or call `without_turn_timeout()`, for tools that
  legitimately run long.
- **Breaking: unparseable Antigravity pre-tool hook arguments fail
  closed.** When the harness sends tool arguments that are not valid JSON
  and a pre-tool hook is set, the call is denied before the hook runs, with
  the reason in the denial. The hook used to be handed `{}`, which a path
  gate allows. Without a hook, name-based policies decide as before. A
  custom tool call whose arguments decode in neither of the harness's
  encodings is answered with an error instead of running the tool on
  `Null`. Migration: none; well-formed calls are unaffected.
- **Breaking: a blank Antigravity API key counts as no key.**
  `with_api_key("")` (or whitespace) together with `with_model` or
  `with_gemini_base_url` now fails at `spawn()` with
  `AntigravityError::Config`. Without either, the blank key is no longer
  sent to the harness. Before, an empty key went on the wire as an unset
  field and the harness fell back to its environment's `GEMINI_API_KEY`.
  Code like `.with_api_key(env::var("X").unwrap_or_default())` hits this.
  Migration: pass a real key, or leave out `with_api_key`.
- **Breaking: `AntigravityError` has no `#[from]` conversions, and its
  causes are explicit.** `Json` is removed. A harness message this client
  cannot parse, which was `Json`, is now `Protocol`, as `Protocol`'s doc
  always said, and `Protocol` is a struct variant,
  `Protocol { message, source }`, whose `source` (also `source()`) is the
  parse error when one caused it. `Io` is a struct variant,
  `Io { message, source }`: `Media::from_file` names the path it could not
  read in `message`. A `?` on a `serde_json::Error` or `std::io::Error` no
  longer converts to `AntigravityError`. Migration: match
  `AntigravityError::Protocol { message, .. }` for `Protocol(message)` and
  `AntigravityError::Io { source, .. }` for `Io(source)`; drop `Json` arms
  (a parse failure arrives as `Protocol`); and map your own `io` and
  `serde_json` errors into your error type rather than through
  `AntigravityError`.
- **Breaking: a cut-off stream ends with the new
  `GenaiError::StreamInterrupted`, which is retryable.** It covers every way
  the connection can end a stream body before `interaction.completed` or an
  error event. A close between events was `MalformedResponse` in 0.13.0. A
  close in the middle of an event was a non-retryable `Parse` or `Utf8`. A
  transport failure mid-body (a reset, the client's read or total timeout
  between chunks, a decode error) was `Http`; that `reqwest::Error` is now
  its `source`. One event used to be classified two ways depending on how
  the peer closed; `is_retryable()` explains the choice. A failure before
  the response headers is still `Http`, and a stream stopped by the
  request-level `InteractionBuilder::with_timeout` is still `Timeout`.
  Migration: match `GenaiError::StreamInterrupted { .. }` where you matched
  `MalformedResponse` for a cut-off stream. Match
  `StreamInterrupted { source: Some(e), .. } if e.is_timeout()` where you
  matched `Http(e) if e.is_timeout()` for a stream stalled by
  `ClientBuilder::with_read_timeout` or `ClientBuilder::with_timeout`; its
  `message` and `Display` say what ended the body and leave the transport
  error's text to `source()` (#554), so walk the chain for it. A
  retry loop built on `is_retryable()` now also retries a clean early close.
  Discard the partial output first, because a foreground retry is a new
  generation.
- **Breaking: an auto-function stream round that ends on an event this crate
  doesn't recognize fails with the retryable `StreamInterrupted`, not
  `MalformedResponse`.** The plain stream ends such a body cleanly, since
  the event may be a new terminal one (Evergreen), but
  `create_stream_with_auto_functions()` needs the round's `Completed` to go
  on. It reported a permanent `MalformedResponse("Stream ended without
  Complete event")`, so a retry loop treated the same end of a body two ways
  depending on which stream read it. Migration: match
  `StreamInterrupted { source: None, .. }` where you matched that
  `MalformedResponse`; a retry loop built on `is_retryable()` now retries
  it. See "An unrecognized last event" in `docs/RELIABILITY.md`.
- **Breaking: `GenaiError::is_retryable()` is true for a 408 and false for
  the 5xx a retry cannot fix: 501, 505, 506, 508, 510 and 511.** A 408
  means the server stopped waiting before the request arrived whole, so it
  acted on nothing and the same request can succeed; google-genai retries
  it too. Each of the six names a condition the same request meets on every
  attempt (501 is Google's `UNIMPLEMENTED`), so a retry loop spent its
  budget on them. One 501 can pass: the API also sends it, saying "MCP
  server returned an error", when a remote MCP server answered with an
  error; a server it cannot reach gets a 500, which stays retryable. Every
  other 5xx stays retryable, including 507 and the unregistered codes some
  proxies send; a status of 600 or more is no longer retryable, as
  `docs/RELIABILITY.md` already said. The crate's own waits follow:
  `wait_until_active` and a file search store import's wait now poll
  through a 408 and end at once on the six.
  Migration: a retry loop built on `is_retryable()` needs no change. To
  keep retrying the six, or a status of 600 or more, test `matches!(e,
  GenaiError::Api { status_code: 501 | 505 | 506 | 508 | 510 | 511 | 600..,
  .. })` before `is_retryable()` (to keep only the MCP server's 501, also
  match its message, "MCP server returned an error"); to stop retrying a
  408, test `GenaiError::Api { status_code: 408, .. }` first.
- **Breaking: `Client::new` and `ClientBuilder::build` set a 30 s connect
  timeout.** `DEFAULT_CONNECT_TIMEOUT` covers DNS, TCP and TLS. Before,
  there was none, so a black-holed route waited for the OS's timeout (about
  75 s on macOS, 127 s on Linux). There is still no default read or total
  timeout: a non-streaming response's headers arrive only after generation
  finishes, so any such default would cut off long legitimate requests
  (reasoning in `docs/RELIABILITY.md`). Migration: call
  `with_connect_timeout(..)` for a different value. Tests on a paused tokio
  clock (`#[tokio::test(start_paused = true)]`) that send a real request to
  a local mock server now usually fail with this timeout "after 30 s",
  because tokio auto-advances to it while the connect is in flight. Build
  that test's client with `with_connect_timeout(Duration::MAX)`, which arms
  no timer (see "Tests on a paused clock" in `docs/RELIABILITY.md`).
- **Breaking: `InteractionRequest` has a new public field, `extra`.**
  Migration: a struct literal needs `extra: Default::default()`, or
  `..Default::default()`; `client.interaction()...build()` is unaffected.
- **Breaking: a deserialized `InteractionRequest` keeps the keys it doesn't
  model, in `extra`, and sends them.** 0.13.0 dropped them silently. This
  covers a request loaded from JSON or a config file, and the nested
  `interaction` of a deserialized `CreateTriggerRequest`. A stale, misplaced
  or misspelled key, such as a top-level `temperature` that belongs in
  `generation_config`, now reaches the server, which rejects a name it
  doesn't know with `400 Unknown parameter '<name>'`. Migration: remove the
  key from the JSON, or call `request.extra.clear()` before sending to send
  what 0.13.0 sent.
- **Breaking: a `wait_until_active` timeout shorter than one status
  request's round trip now always fails.** Each status request used to get
  at least a second, so a zero timeout read the state once. Now the first
  request is cut off at the deadline like any other, and the error says the
  wait's deadline cut it off rather than that it failed. Migration: to read
  the state once, call `files().get(name)` or
  `file_search_stores().documents().get(name)` instead.
- **Breaking: a successful streaming response whose `Content-Type` is not
  `text/event-stream` fails at once with `MalformedResponse`** and a preview
  of the body. In 0.13.0 a captive portal's page or a plain JSON interaction
  was parsed as SSE and failed only at its end, as a cut-off stream. The API
  answers `text/event-stream` (verified live 2026-11-04). The check ignores
  letter case and parameters (`text/event-stream; charset=utf-8` passes),
  and a response with no `Content-Type` is still read as a stream. A proxy
  or gateway (`ClientBuilder::with_base_url`) that rewrites the type of a
  working stream, for example to `text/plain` or `application/octet-stream`,
  now fails where 0.13.0 streamed. The check covers every streaming call:
  `create_stream()`, `create_stream_with_auto_functions()`, and
  `interactions().stream()` / `resume_stream()`. Migration: have the proxy
  pass the API's `Content-Type` through, or send none. Behind a proxy you
  can't change, use the non-streaming call instead: `create()`,
  `create_with_auto_functions()`, or `interactions().get(id)` for a stored
  interaction.
- **Breaking: `FileMetadata::size_bytes` is an `Option<i64>`, and
  `FileMetadata::size_bytes_as_u64()` is gone.** The field reads the API's
  string (`"17"`) or a plain number, and serializes back as a string, like
  `FileSearchDocument::size_bytes`. Migration: read `file.size_bytes`
  directly; where a `u64` is needed, `file.size_bytes.and_then(|n|
  u64::try_from(n).ok())`.
- **Breaking: `RemoteEnvironment` and `VoiceSpec` have new public
  fields** (`environment_id`; `description`, `language_code`, `region_code`,
  `accent`, `persona`, `context`, `gender`, `pitch`), so struct literals
  that list every field stop compiling. Migration: add
  `..Default::default()`, or build them with `RemoteEnvironment::new()` and
  `CreateVoiceRequest`'s constructors and `with_*` setters.
- **Breaking: `FileSearchDocument` and `FileSearchDocumentListResponse` no
  longer implement `Eq`**, since `CustomMetadata` holds an `f64`. Migration:
  `==` still works (`PartialEq`); drop `Eq` bounds that required it.
- **Breaking: `FileSearchStores::upload` takes `impl Into<DocumentUpload>`
  and uses the resumable upload protocol.** A `FileUpload` converts, so
  existing calls compile unchanged. The upload is now a start request
  carrying the document's settings as JSON (`displayName`, `mimeType`,
  `customMetadata`, `chunkingConfig`), then the file, then the read-back,
  where it used to be one `raw` request with the display name in the query
  string. Documents are named as before (verified live 2026-11-05).
  Migration: none for callers; a test double that answered the `raw`
  request must answer the start request with an `x-goog-upload-url`
  header.
- **Breaking: `Tool::Function`'s `description` is an `Option<String>` and
  its `parameters` an `Option<FunctionParameters>`.** The bindings make
  both optional, the API accepts a function tool with only a `name`, and a
  stored interaction read back serves it that way (verified live
  2026-11-05). Migration: wrap the values in `Some(..)` where you build a
  `Tool::Function` by hand, and match `description: Some(d)` /
  `parameters: Some(p)` where you read one. `FunctionDeclaration::into_tool`
  sets both, so the builder path sends what it sent before.
- **Breaking: response fields the API never serves are removed.** Each was
  modeled from an older spec, is in neither the google-genai 2.34.0
  bindings nor the spec, and was checked live on 2026-11-05:
  - `GoogleSearchResultItem` keeps only `search_suggestions`: `title`,
    `url`, `rendered_content` and `has_rendered_content()` are gone, and
    `GoogleSearchResultItem::new` takes the suggestions payload instead of a
    title and URL. Search result items carry only `search_suggestions`.
  - `Place` keeps `name`, `place_id`, `url` and `review_snippets`:
    `formatted_address`, `lat`, `lng`, `types`, `rating`,
    `user_ratings_total`, `website` and `phone_number` are gone (30 places
    across three Maps results carried only the first three). `Place` is now
    `Eq`.
  - `FileSearchResultItem` and `InteractionResponse::file_search_results()`
    are gone, and `Step::FileSearchResult` has no `result`: the step
    carries only `call_id` and `signature`, stored and streamed (#429).
    `StepDelta::FileSearchResult::result` is a `Vec<serde_json::Value>`,
    the opaque objects the spec gives the delta; the stream accumulator
    keeps any it receives in the step's `extra` under `result`.

  Migration: a removed key that ever arrives lands in the type's `extra`,
  so read it there (`item.extra.get("title")`). For file search, read the
  `file_citation` annotations on the answer (`all_annotations()`);
  `has_file_search_results()` still says whether a search ran.
- **Breaking: `GenerationConfig` has a new public field, `extra`.**
  Migration: a struct literal needs `extra: Default::default()`, or
  `..Default::default()`.
- **Breaking: `generation_config`, `response_format`,
  `response_modalities`, `safety_settings`, `agent_config` and
  `environment` no longer land in `InteractionResponse::extra`.**
  Migration: read the new fields (`response.safety_settings` instead of
  `response.extra["safety_settings"]`). The environment echo's `env`, a
  list of single-key maps on the wire, serializes back as the map a request
  sends.
- **Breaking: `StreamChunk::StepDelta` has a new field, `total_usage`.**
  It carries the `step.delta` event's `metadata.total_usage`, which the
  google-genai 2.34.0 bindings and the spec define and the crate dropped.
  Not observed live: text, TTS and agent streams sent no `metadata` on a
  `step.delta` (2026-11-05). When it arrives it also serves as the usage
  of a `Completed` event that carries none, like a `step.stop`'s. The
  serialized chunk writes it as `data.total_usage`. Migration: add `..` to
  a pattern that lists every field (`StreamChunk::StepDelta { index, delta,
  .. }`), and `total_usage: None` to a hand-built chunk.
- **Breaking: `InteractionRequest::input` is an `Option<InteractionInput>`,
  and `InteractionInput` no longer implements `Default`.** The bindings make
  `input` optional, and a decoding continuation of a stored interaction
  sends none (#489). `None` is left out of the body. A deserialized
  `InteractionRequest` without `input` now reads as `None` instead of
  failing; a `CreateTriggerRequest` config file still fails on a missing (or
  misspelled `inputs`) input, since a trigger never fires a continuation,
  and a malformed `input` value is still a parse error. A stored trigger's
  interaction without an input, or with one that does not parse, reads as
  `None` where it read as empty text. `build()` still requires input, except
  for a continuation. Migration: write
  `input: Some(InteractionInput::Text(..))` in a struct literal, match
  `Some(InteractionInput::Steps(..))` where you read it, and replace
  `InteractionInput::default()` with the input you mean (or `None`).
- **Breaking: genai-rs no longer forces reqwest's default features on.**
  It depends on reqwest with `default-features = false`, and reqwest's
  `http2`, `system-proxy` and `charset` come through genai-rs's own default
  features of the same names, so a consumer can turn them off. TLS is
  rustls with the OS trust store, as before. With genai-rs's default
  features nothing changes. A crate that sets `default-features = false` on
  genai-rs (to drop `wire-color`) now also drops HTTP/2, the system proxy
  settings (`HTTPS_PROXY` and the OS configuration) and charset decoding.
  Migration: add `features = ["http2", "system-proxy", "charset"]` to keep
  them.
- **Breaking: `CallableFunction::call` returns a `FunctionFuture` instead
  of being an `#[async_trait]` method, and genai-rs no longer depends on
  `async-trait`** (#490). `FunctionFuture<'a>` is
  `Pin<Box<dyn Future<Output = Result<Value, FunctionError>> + Send + 'a>>`,
  the type `#[async_trait]` generated, written out, so tools are still
  `dyn CallableFunction` (D-026). `async-trait` brought in a second `syn`
  that every consumer compiled. `#[tool]` functions are unaffected.
  Migration: in a hand-written impl, drop `#[async_trait]` and write
  `fn call(&self, args: Value) -> FunctionFuture<'_> { Box::pin(async move { ... }) }`
  around the old body; remove `async-trait` from your dependencies if
  nothing else uses it.
- **Breaking: `genai_rs::function_calling::submit` and
  `CallableFunctionFactory` are no longer public API** (#490). They were
  `#[tool]`'s `inventory` registration, and live in the doc-hidden
  `genai_rs::__private` now, with no semver guarantee. A `genai-rs-macros`
  must match its `genai-rs` release: a 0.13 `genai-rs-macros` does not
  compile against this release, nor a new one against 0.13. Migration:
  register functions with `#[tool]`, or return them from a `ToolService`;
  bump `genai-rs` and `genai-rs-macros` together.
- **Breaking: `GenaiError::Json` and `GenaiError::Parse` are removed**
  (#490, D-027). `Json` had no producer. A streamed SSE event that does not
  parse, or one that grows past 64 MiB, is `MalformedResponse` now, as a
  non-streamed body that does not parse already was. Migration: match
  `MalformedResponse` where you matched `Parse`, and drop `Json` arms.
- **Breaking: a `wait_until_active` timeout is
  `GenaiError::PollTimeout { timeout, message }`, and a file, document or
  import that fails processing is `GenaiError::ProcessingFailed(String)`;
  both were `Internal`** (#490, D-027). Neither is retryable, as before. A
  file search store import's wait that times out is `PollTimeout` too.
  `Internal` is left with the client's own failures: a request body that
  would not serialize, and a read-back of a document an upload or import
  already made that fails unexpectedly. Migration: match the new variants
  where you told these cases apart by `Internal`'s message.
- **Breaking: `GenaiError::ClientBuild` holds the `reqwest::Error` instead
  of its message**, and returns it from `source()` (#490). Its `Display` is
  `Failed to build HTTP client` alone, leaving the reqwest error's text to
  the chain, so a reporter that walks `source()` prints it once (#554).
  Migration: `ClientBuild(e)` binds a `reqwest::Error`; `e.to_string()`
  gives the old string.
- **Breaking: four types are renamed to the pattern the others follow,
  and `VideoProcessingBuilder`'s setters take the `with_` prefix** (#490,
  D-028). A list page is `<Resource>ListResponse` and a create body is
  `Create<Resource>Request`. Migration:

  | Was | Now |
  |-----|-----|
  | `ListFilesResponse` | `FileListResponse` |
  | `EnvironmentFileList` | `EnvironmentFileListResponse` |
  | `DocumentListResponse` | `FileSearchDocumentListResponse` |
  | `TriggerCreateParams` | `CreateTriggerRequest` |
  | `VideoProcessing::segment().start_offset(s)` | `.with_start_offset(s)` |
  | `VideoProcessing::segment().end_offset(s)` | `.with_end_offset(s)` |
  | `VideoProcessing::segment().fps(f)` | `.with_fps(f)` |
- **Breaking: every collection accessor on `InteractionResponse` returns a
  `Vec`** (#490, D-028). `output_contents()`, `all_annotations()`,
  `images()`, `audios()`, `thought_signatures()` and `thought_summaries()`
  returned an `impl Iterator`, while `function_calls()`, `tool_calls()`,
  `mcp_calls()` and the rest returned a `Vec`. Migration: drop `.collect()`
  after them, write `.len()` for `.count()`, `.first()` for `.next()`, and
  `.into_iter()` before any other iterator adapter (`.into_iter().map(..)`).
  A `for` loop over them is unchanged.
- **Breaking: the large, rare payloads of the stream types are boxed**
  (#490). `StreamChunk::Created { interaction, .. }`,
  `StreamChunk::Completed`, and
  `AutoFunctionStreamChunk::ExecutingFunctions { response, .. }`,
  `Complete` and `MaxLoopsReached` hold a `Box<InteractionResponse>`;
  `StreamChunk`'s `usage`, `total_usage` and `step_usage` fields are
  `Option<Box<UsageMetadata>>`; and `StepDelta::ThoughtSummary`'s `content`
  is an `Option<Box<Content>>`. An enum is as large as its largest variant,
  so every `StreamEvent`, a one-word text delta included, was 1.9 KB; it is
  216 bytes now. Migration: method calls and field reads go through the box
  unchanged (`response.as_text()`, `usage.total_tokens`); where you need the
  value itself, dereference the box (`*response`, or
  `StreamChunk::Completed { interaction, .. } => Some(*interaction)`);
  build a chunk with `Box::new(..)`.
- **Breaking: strict mode is the `--cfg genai_strict_unknown` flag; the
  `strict-unknown` Cargo feature is gone** (#490, D-030). Cargo unifies
  features across a build, so any crate that turned the feature on made
  every other user of genai-rs in that build fail on unknown API values: a
  feature must only add, and this one changed behavior. A cfg is set only
  by whoever runs the build. Behavior under it is unchanged, except that
  strict-mode errors name the cfg. Migration: replace
  `--features strict-unknown` with
  `RUSTFLAGS="--cfg genai_strict_unknown"` (and `RUSTDOCFLAGS` for
  doctests), or put `rustflags = ["--cfg", "genai_strict_unknown"]` under
  `[build]` in `.cargo/config.toml`; replace
  `#[cfg(feature = "strict-unknown")]` in your tests with
  `#[cfg(genai_strict_unknown)]`.
- **Breaking: `GenaiError::Utf8` and its `From<std::str::Utf8Error>` impl
  are removed** (#504). The SSE parser was its only producer. A streamed
  line that is not valid UTF-8 is `MalformedResponse` now, not retryable
  as `Utf8` was: the server sent those bytes, and a retry gets them again.
  A line cut off at the end of the body inside a multi-byte character is
  still the retryable `StreamInterrupted`. Migration: match
  `MalformedResponse` where you matched `Utf8`; where `?` converted a
  `Utf8Error` into a `GenaiError`, map it into your own error.
- **Breaking: `FunctionDeclaration::new` and `FunctionParameters::new` are
  no longer public** (#504). They were `#[doc(hidden)]` plumbing for
  `#[tool]`, which now builds its declaration through the doc-hidden
  `genai_rs::__private`, with no semver guarantee. Migration: build a
  declaration with
  `FunctionDeclaration::builder(name).with_description(..).add_parameter(..).with_required(..).build()`,
  or deserialize one with `serde_json`; bump `genai-rs` and
  `genai-rs-macros` together.
- **Breaking: `Client::new` and `Client::builder` take `impl Into<String>`**,
  so `Client::new("key")` and `Client::builder("key")` compile (#504). A
  `String` argument is unchanged; a call that converted with `.into()` no
  longer infers the target type, and `Client::builder` is no longer a
  `const fn`. Migration: pass the `&str` or `String` itself, without
  `.into()`.
- **Breaking: a failed Antigravity turn is
  `AntigravityError::Turn { kind, message }`, and `kind` says why.** `kind`
  is the new `TurnFailure` (`#[non_exhaustive]`): `ModelBackend { status }`
  (the model backend failed a call: a 400, 401 or 403, or a 503 whose
  retries ran out), `Trajectory` (the harness ended the turn with an error
  of its own, such as an MCP server that failed to load), `Denied` (the
  pre-turn hook refused the turn; `message` is its reason), `Cancelled`
  (the harness cancelled the turn) or `Hook` (a hook's thread did not run).
  `TurnFailure::ends_session()` is true for `ModelBackend` and
  `Trajectory`, after which harness 0.1.23 ends its run, and the agent
  fails every later call with `ConnectionClosed` without sending. `message`
  is the harness's text alone, and `Display` names the kind. Migration:
  match `AntigravityError::Turn { kind, message }` where you matched
  `Turn(message)`, and decide on `kind` (or `kind.ends_session()` for
  whether to spawn a new agent) where you searched the message for "HTTP
  400" or a reason.
- **Breaking: `AntigravityError::InitFailed` has a `source` field, and no
  Antigravity error repeats its source's text in `Display`.** A WebSocket
  connect that fails during `spawn()`, and a failed read of the harness's
  initialization reply, are `InitFailed` errors whose `source()` is the
  `WebSocket` error (it held only the text). `WebSocket`, `Protocol`, `Io`
  and `InitFailed` print what was being done; `source()` gives the cause,
  so a reporter that walks the chain prints each cause once. Migration:
  match `InitFailed { message, stderr, .. }`, and walk `source()` (or use
  a reporter that does) where you printed only the top error and want the
  cause.
- **Breaking: `AgentBuilder::with_tool_service` is `add_tool_service`.**
  It appends, so each call registers another service; the `with_` prefix
  says a second call replaces. Migration: rename the call.
- **Breaking: a harness path the caller names is used as given.** When the
  path from `AgentBuilder::with_harness_path` or `ANTIGRAVITY_HARNESS_PATH`
  is not a file, `spawn()` fails with `AntigravityError::HarnessNotFound`
  naming it, and its message says to fix that path. It used to search
  site-packages and `PATH` next, so a mistyped path silently ran another
  harness. An empty `ANTIGRAVITY_HARNESS_PATH` counts as unset. Migration:
  fix the path, or remove it to let discovery search.
- **Breaking: `AntigravityError::WebSocket` is a struct variant,
  `WebSocket { message, source }`** (#504). `source` is the transport's
  error, a `Box<dyn std::error::Error + Send + Sync>`, and `source()`
  returns it; the variant used to hold only its text. `message` says what
  failed: a read or a send. (A connect that fails, with the port and
  attempt count, reaches `spawn()`'s caller as `InitFailed`, with this
  error as its source.) The `Display` text is `websocket error: {message}`;
  the transport's text is the source's. Migration: match
  `AntigravityError::WebSocket { message, source }` where you matched
  `WebSocket(text)`, and `downcast_ref` the source for the transport's own
  error type.
- `PollOptions` polls no faster than `PollOptions::MIN_POLL_INTERVAL`
  (10 ms): a shorter `with_poll_interval`, `Duration::ZERO` included, is
  raised to it. A zero interval re-polled the moment each status request
  answered, back to back until the deadline. 10 ms is far below a status
  request's round trip, so an interval chosen on purpose is unaffected
  (D-047).
- Every client sets TCP keepalive explicitly: a probe after 15 s idle, then
  every 15 s, failing the connection after 3 unanswered (Windows can't set
  the count and sends its own 10). These are reqwest 0.13's defaults, which
  the crate relied on without setting, so a reqwest upgrade could have
  changed the only thing that notices a dead peer on a connection with no
  read timeout. A `Client`'s `Debug` output shows them; see
  `docs/RELIABILITY.md`.
- Antigravity's pre-tool hook runs once per custom tool call; it ran twice,
  once when the harness asked and again on dispatch.
- Harness discovery no longer starts `python3` when the explicit path or
  `ANTIGRAVITY_HARNESS_PATH` names the binary.
- A Files API or file search store upload's start request now sends
  `Api-Revision`, as every other API request does; the request that sends
  the file goes to the session URL and still carries neither it nor the
  key.
- The Antigravity harness runs in its own process group on Unix. A
  terminal's Ctrl-C now reaches only your process; when it exits, the
  harness sees stdin close and shuts down gracefully. To stop the harness on
  Ctrl-C yourself, call `shutdown()` from your signal handler.
- `Interactions::get_with_input` docs no longer say the input is not
  echoed. With `include_input=true` the API returns the input as a list
  holding one `user_input` step, and on that read it repeats every entry of
  the stored request's lists (`stop_sequences`, `response_modalities`,
  `safety_settings`, speech speakers; verified live 2026-11-04).
- `docs/ENUM_WIRE_FORMATS.md` no longer says reads echo no
  `generation_config`. A stored interaction read back carries it, with
  `speech_config` in the `{"speakers": [...]}` form whichever form was sent,
  and `response_format`, `response_modalities` and `safety_settings` too
  (verified live 2026-11-04); see Added for the fields that model them.
- `wait_until_active` (files and file-search documents):
  - It honors the `Retry-After` of a 429 it polls through, and gives up at
    once when that falls past the deadline.
  - It never runs past its deadline: a status request is cut off at the
    deadline, and a poll that would start past it is not made.
  - It runs on tokio time instead of `std::time`. Against a real server on a
    paused test clock, its deadline is a timer that auto-advance fires while
    a status request is in flight, so hold the clock for each request (see
    "Tests on a paused clock" in `docs/RELIABILITY.md`).
  - The first status request is always made, so an invalid name is reported
    even with a zero timeout.
- A stream yields its first terminal event (`Completed` or `Error`) as soon
  as it arrives, and the next poll reads the rest of the body to its end,
  discarding every frame and swallowing every error, before the stream
  ends. A late error, such as a reset while the server closes, no longer
  follows a finished interaction or fails a finished auto-function round.
  The read matters: after `interaction.completed` the API sends
  `event: done` / `data: [DONE]` and closes about a second later, and the
  stored interaction is readable only then, so a `get` or a chained request
  sent before the stream has ended can fail with
  `500 Internal error encountered` (verified live 2026-11-05). To get or
  chain on a streamed interaction right away, read the stream to its end;
  `create_stream_with_auto_functions()` reads each round to its end before
  sending the next. The read gives up after 5 s with a `warn!`, so a
  connection left open after the terminal event can't hang the last poll,
  and `with_timeout()`'s per-chunk limit doesn't apply to it. A
  client-level timeout (`ClientBuilder::with_timeout`) that ends the read
  early is logged at `warn` too; either way the stored interaction may not
  be readable yet.
- One SSE event may hold up to 64 MiB. Past that, the stream fails with
  `GenaiError::MalformedResponse`; before, a body that never sent a newline
  was buffered without bound. The largest real events, measured live
  2026-11-04 on `DEFAULT_IMAGE_MODEL` at `image_size: "4K"`, are a 16.0 MB
  `image` delta and a 15.7 MB `thought_signature` delta; TTS audio deltas are
  under 8 KB.
- The published crate holds the library, its examples and the guides its
  doctests include, and no longer the integration tests, their fixtures, CI
  files, scripts or the library's unit-test sources: 184 files and about
  2.6 MiB unpacked, down from 383 files and 5.5 MiB.
- A `file_search` tool with no store names no longer writes
  `"file_search_store_names": []`; the key is left out, as for the other
  optional lists.
- `create_stream()`, `create_stream_with_auto_functions()` and
  `Client::execute_stream()` return `BoxStream<'static, _>`: the stream owns
  a clone of the client, as `interactions().stream()` and the list streams
  do, so it can be stored or `tokio::spawn`ed (#498). `create_stream()` and
  `create_stream_with_auto_functions()` now build the request when called;
  a request the builder rejects is still the stream's only item, and
  nothing is sent until the stream is polled.
- `with_content()` composes with `with_history()`: the content is this
  turn's input after the history, `[...history, user_input(content)]`, with
  any `with_text()` text prepended to it as without history (#454, D-031).
  `build()` used to refuse the pair, a rule left from when content went on
  the wire as a bare array; the API takes the composed form like any
  history (verified live 2026-11-06).
- `detect_mime_type` is `#[must_use]`, so a call that discards the result
  warns.
- The file search store and document listings page with `pageSize` and
  `pageToken`, the camelCase pair the Files API and google-genai's bindings
  use (#541); they sent `page_size` and `page_token`, which the API also
  accepts, so paging behaves as before.

### Fixed

- `Debug` no longer prints the secrets a value holds. `ExaAiSearchConfig`
  and `ParallelAiSearchConfig` printed their `api_key` (and one nested in
  `custom_config`), `McpServerConfig` and `Tool::McpServer` their header
  values, `AllowlistEntry` its `transform` header values, `EnvVar` its
  plain value and `RemoteEnvironment` an `env` it could not read, and so
  did the `InteractionBuilder`, `InteractionRequest`, `InteractionResponse`,
  trigger, agent or environment holding one. `wire::WireEvent` printed
  every request and response body whole. Each prints `[REDACTED]` in the
  secret's place, as `LOUD_WIRE` does, and keeps the header and variable
  names, so `dbg!`, `tracing::debug!(?request)` or an inspector logging
  `{event:?}` no longer undoes the wire's redaction. A `WireEvent`'s fields
  and serialized form still hold the bodies raw.
- On Windows, Antigravity harness discovery finds the wheel's
  `localharness.exe`, in `python3`'s site-packages and on `PATH`, as the
  Python SDK does; it looked only for `localharness`, so a pip-installed
  harness was not found without `ANTIGRAVITY_HARNESS_PATH`.
- On Windows, Antigravity harness discovery asks `python` for its
  site-packages after `python3`. python.org's installer provides no
  `python3` (often only the Microsoft Store's stub is there), so a wheel
  installed with its `pip` was not found without `ANTIGRAVITY_HARNESS_PATH`.
  `python` is started only when `python3`'s site-packages lack the harness.
- Antigravity's `spawn()` no longer waits forever on a harness that
  accepts the WebSocket connection but never answers the upgrade: each of
  the 5 connect attempts gives up after 5 s and counts as failed, so
  `spawn()` fails with `InitFailed` after about 26.5 s.
- Antigravity's file watch (`triggers::on_file_change`): a poll that cannot
  read a directory or entry under the watched path (a permission or I/O
  error) is skipped, with one warning naming what could not be read,
  instead of reporting every file there deleted and then added back when
  it reads again; a directory the watch skips anyway (`.git`,
  `node_modules`, …) still cannot fail a poll.
  A watched path that is a symbolic link is followed, as `watchfiles`
  follows its root (it read as an empty tree); links below it are still
  not followed. A `poll_interval` under 10 ms, zero included, polls every
  10 ms, as `PollOptions` does (D-047), rather than walking the tree back
  to back.
- `McpServer::with_timeout_seconds` said only that it bounds each call. On
  harness 0.1.23 it bounds both each call and loading the server
  (connecting and listing its tools, on the first turn), where a server
  slower than it fails that turn; the rustdoc and `docs/ANTIGRAVITY.md` say
  so.
- The `backon` retry snippet in `docs/RELIABILITY.md` and
  `examples/retry_with_backoff.rs` let a server's `Retry-After` bypass
  `with_max_delay`: `adjust` replaces backon's delay after the cap, so a
  429 asking for an hour slept for an hour. Both cap it at the backoff's
  maximum now; copy the change into a loop built from either.
- The retry loop in `InteractionRequest`'s rustdoc had no bound and no
  delay, so a 429 or 503 was retried as fast as the network allowed, and
  `Client::execute`'s had no jitter and ignored `retry_after()`. The
  recipe, now on `InteractionRequest` alone, bounds its attempts and waits
  with jitter or for the server's `Retry-After`, capped, as
  `docs/RELIABILITY.md` does. The background poll in
  `docs/AGENTS_AND_BACKGROUND.md` ended on the first transient error and
  checked its deadline only between requests; it now polls through a
  retryable error, honoring `Retry-After`, and bounds each request by the
  time left. Copy the change into a loop built from either.
- Docs that said more than holds: `with_labels` said labels are echoed on
  the response, but only a stored interaction read back with
  `interactions().get` carries them; the retry predicate in
  `docs/ERROR_HANDLING.md` left out the 400s that say "please retry the
  request"; and `with_seed` was said to make sampling repeatable, which
  the API does not guarantee. `AntigravityError::Turn` now says that a
  fatal model error (an HTTP 400 from the model endpoint) also ends the
  harness session on 0.1.23, so the next call fails and the agent must be
  spawned again.
- `#[tool]` documents everything it generates: the `<Name>Callable`
  struct, its `new`, and the `<name>_declaration` and
  `<name>_callable_factory` functions. They are public, so a crate that
  warns on or denies `missing_docs` reported each of them for every tool.
- Unknown fields on known `Step`, `StepDelta`, `Content` and `Annotation`
  variants now survive a deserialize/re-serialize round trip, and so do
  unknown fields on the result items, review snippets, step errors and
  static video segments nested in them. Replayed stateless history keeps a
  field the API adds before the crate models it. Previously a
  `function_call` lost an unknown top-level field, and a `text` block lost
  an unknown field. A `url_context_call` lost `arguments` keys other than
  `urls`; `google_search_call`, `google_maps_call` and `retrieval_call` lost
  keys other than `queries`; `code_execution_call` lost keys other than
  `language` and `code` (#491).
- `Place` no longer serializes an `extra` entry named after one of its
  modeled fields. Only a hand-built map can hold one, and it overrode the
  field under `serde_json::to_value` and wrote the key twice in a request
  body.
- Streaming: two images in one `model_output` step now stay two
  `Content::Image` blocks. The accumulator appended each image delta to the
  previous image block in the step. That would merge two images into one
  corrupt base64 string whenever `interaction.completed` omits `steps`,
  which it did on every stream observed 2026-11-04. Each streamed image
  arrives whole in one delta that names its `mime_type` (verified live
  2026-11-04, `gemini-3.1-flash-image`), so such a delta now starts a new
  block. In three live streams that day each image came in its own step,
  which was already handled; the same-step case is defensive (#491).
- Streaming: the assembled `Completed` response keeps the unmodeled fields
  that deltas carry, on the content block or step each delta builds,
  including every chunk of a merged thought summary.
- `StepDelta::CodeExecutionCall` no longer serializes an absent `language`
  or `code` as `null`.
- Antigravity's `generate_image` builtin failed every call ("model is
  empty"): no image model was configured. With the builtin enabled the
  bridge now sends one, `DEFAULT_IMAGE_MODEL` unless `with_image_model`
  says otherwise (verified on 0.1.23, 2026-11-05).
- An Antigravity step in an error state no longer streams its text as a
  `TextDelta`. Harness 0.1.23 puts a denial's message in the delta of a
  model-sourced error step, which read as the model's answer; it surfaces as
  `AgentEvent::Error` only.
- Dropping an Antigravity agent no longer leaves processes the harness
  started running; neither does a failed `spawn()` or a shutdown that had to
  escalate. The kill used to signal only the harness's pid, and harness
  0.1.23 gives every `run_command` command a process group of its own, so
  the command was orphaned and kept running (verified on macOS,
  2026-11-04). The forced paths now signal the harness's process group and
  the group of each of its children. A command that detaches itself
  (`setsid`) is still out of reach. This is the Unix behavior: on Windows,
  with no process groups, the forced paths still end the harness process
  only, so call `shutdown()` there.
- Antigravity harness discovery no longer runs on the calling task's
  thread. Finding the binary started `python3` to list its site-packages
  there, with no bound, so a hung `python3` held `spawn()`, and with it a
  current-thread runtime, forever. Discovery now runs on tokio's blocking
  pool, `python3` gets 10 seconds before the search moves on to `PATH`, and
  the read of the wheel's version beside the binary has the same bound.
- Antigravity keeps a bounded tail of the harness's stderr: each line up to
  8 KiB, marked when cut (the rest of the line is dropped), and 64 KiB in
  all for error messages. Output with no newlines used to grow one buffer
  without limit.
- `triggers::on_file_change` skips a poll whose snapshot failed (the
  snapshot task panicked, the runtime was shutting down, or the watched
  path exists but could not be read, such as a transient I/O error on a
  network mount) and logs a warning. It used to take the failure for an
  empty tree, reporting every watched file deleted, then added back on the
  next poll. If the path cannot be read when the watch is created, the
  first snapshot that reads it becomes the baseline instead of reporting
  every file in it added. A watched path that no longer exists still reads
  as empty.
- `document_from_file()` sends `.txt` and `.md` files as text content
  (`Content::Text` with the file's UTF-8 contents) instead of `text/plain` /
  `text/markdown` document content, which the API rejects with a 400 since
  2026-11-04. A file that isn't valid UTF-8 returns
  `GenaiError::InvalidInput`. If you match on the returned `Content`, expect
  `Content::Text` for these extensions. `document_from_file_with_mime()` is
  unchanged and still sends any MIME type as a document (#514).
- `Webhooks::create` sent a `Webhook`'s output-only fields (`id`, `state`,
  `signing_secrets`, `new_signing_secret`, `create_time`, `update_time`)
  whenever they were set, as on a webhook read back from the API. The
  create body is now what google-genai's `WebhookInput` defines: `uri`,
  `subscribed_events` and `name`, plus anything in `extra`.
- An unknown `StreamChunk` whose type was spelled like a known `chunk_type`
  (`created`, `step_delta`, ...) read back as that known chunk after
  serializing. It now carries `"unknown": true` and stays unknown; chunks
  serialized by 0.13.0 read back as before (`docs/ENUM_WIRE_FORMATS.md`).
  Found by the new fuzz targets.
- The `CreateTriggerRequest::extra` docs said a colliding key wins because
  the request path uses `serde_json::to_value`. In fact the body carries
  both keys, with the `extra` one last, and it wins because the API keeps
  the last of a repeated key (verified live 2026-11-04).
- A string enum that read a non-string wire value (a number, an object,
  `null`) into `Unknown` now serializes that value back unchanged. It used
  to write the `"<non-string: ...>"` descriptor its `Unknown` carries, so
  the one case did not round-trip. This covers every enum built on the
  crate's string-enum pattern, such as `InteractionStatus`, `ServiceTier`
  and `HarmCategory`, and a code execution call step's or delta's
  `language`, whose nested `arguments` were written from the language's
  text. A hand-built `Unknown` still serializes its type string.
- A tool that leaves out a field the bindings make optional reads as that
  tool instead of `Tool::Unknown`: a `function` with no `description` or
  `parameters` (a stored interaction serves one sent with only a `name`
  that way, live 2026-11-05), a `computer_use` with no `environment`, an
  `mcp_server` with no `name` or `url`, and a `file_search` with no store
  names. Each serializes back without the missing key.
- `FunctionParameters` keeps every JSON Schema keyword it does not model
  (`additionalProperties`, `$defs`, ...) and writes it back; it used to
  drop them. It also reads a schema with no `type` or `properties`
  (`{"type": "object"}` for a function without arguments), which made the
  whole tool `Unknown`. `FunctionParameters::extra()` returns the kept
  keywords.
- A per-modality usage entry (`ModalityTokens`) without a `tokens` count
  reads as 0 (protobuf JSON leaves out a zero), and one without a
  `modality` as an empty one. Either used to fail the whole response.
- Streaming: the assembled `Completed` response keeps media deltas apart
  by what they are. Streamed TTS sends one recording as hundreds of raw PCM
  deltas in one step, each naming the same format (live 2026-11-05,
  `DEFAULT_TTS_MODEL`: `audio/l16`, 24000 Hz, mono), so an audio delta
  continues the last audio block unless it names a `uri` or a different
  `mime_type`, `sample_rate` or `channels`; before, every audio delta joined
  the last audio block whatever its format. A video or document delta now
  follows the image rule: one that names a `mime_type` or `uri` starts a
  block, so two whole files in one step no longer merge into one corrupt
  string (no model streams either to observe, 2026-11-05). Chunks join as
  text only when both are whole base64 quanta (a multiple of 4 characters,
  with no padding before the join); otherwise, a padded chunk or an
  unpadded one of another length, they are joined as bytes, so the result
  stays valid base64. Every live chunk has been whole quanta.
- `VideoMetadata` keeps the keys it does not model in a new `extra` map,
  as the other response types do; it used to drop them. The bindings type
  it `dict[str, Any]`, and the API serves `videoDuration` (live
  2026-11-05).
- An environment's network read back as `{"allowlist": [{"mode":
  "disabled"}]}`, the echo of `{"allowlist": "disabled"}`, parsed as an
  allowlist entry with an empty domain. Both forms now read as the new
  `NetworkConfig::AllowlistDisabled`, which sends `{"allowlist":
  "disabled"}` (the bindings' `Disabled` arm of the allowlist). It blocks
  egress exactly as `NetworkConfig::Disabled` does (live 2026-11-05: an
  agent's `curl` in either sandbox got the egress proxy's 403, and 200 in
  one with no network config), and `NetworkConfig::is_disabled()` is true
  for both (#489).
- `ModalityTokens`, `GroundingToolCount`, `RotateSigningSecretResponse`,
  `VoiceAudio` and `RankService` keep the fields they do not model in a new
  `extra` map and write them back, as the other response types do; a
  deserialize/serialize round trip dropped them.
- `FileMetadata` failed to deserialize when `sizeBytes` was a JSON number
  rather than the protobuf string, so `Files::get` returned an error and a
  list page dropped the file. It now reads the number.
- `FileError` dropped a failed file's `details` and any other unmodeled
  key; it keeps them now.
- `with_content(vec![])` with no `with_text()` fails in `build()` with
  `GenaiError::InvalidInput` naming `with_content()`, as a missing input
  does, instead of costing a round trip for the API's
  `400 Request has empty input.` (#452).
- Antigravity: a turn being halted and drained, because it hit its turn
  timeout or no one consumes it, no longer runs the per-turn lifecycle hooks
  (`on_pre_turn`, `on_post_turn`, `on_tool_error`, `on_compaction`,
  `on_stop`). The drain answers the harness as if they were not set. Before,
  a timed-out turn reported `stop` and an empty `post_turn` during the
  drain (scripted against harness 0.1.23, 2026-11-06), and a stop hook's
  `Continue` could keep the halted turn running.
- Antigravity: abandoning a turn no longer desyncs the agent. Dropping a
  `chat` future (an outer `tokio::time::timeout`, a losing `select!`
  branch) or a `send_streaming` stream while a custom tool or the pre-tool
  hook was running left the harness inside that turn, waiting for an answer
  that never came. The next `chat` hung until its turn timeout (300s by
  default), and the one after it returned the abandoned turn's reply
  (scripted against harness 0.1.23, 2026-11-07). The agent now records
  whether each turn reached its end (its `Finished`, or an error the
  harness ended it with), and the next `chat`/`send_streaming` halts and
  drains a turn that did not before sending its message: half a second
  when nothing is left running, five seconds at most (#504).
- `docs/CONVERSATIONS.md` no longer suggests a follow-up turn can simply
  drop a built-in tool. After a turn that used Google Search, a follow-up
  sent without it sometimes ends `failed` with a `malformed_tool_call`
  error, because the model calls the tool anyway (verified live
  2026-11-07); send the built-in tools on every turn. The url_context loop
  under a response schema is recorded with what mostly avoids it: at
  `with_thinking_level(ThinkingLevel::Low)` it looped in 3 of 61 single
  requests, against 69 of 129 at the default level
  (`docs/INTERACTIONS_API_GAP.md`, #504).
- The `Debug` output of the Antigravity types that hold a secret no longer
  prints it: `protocol::GeminiApiEndpoint`'s API key, and the header and
  environment variable values of `protocol::GeminiApiEndpoint`,
  `VertexEndpoint`, `McpHttpTransport`, `McpStdioTransport` and
  `McpServer` print as `"[REDACTED]"`, their names kept. So does
  `AgentBuilder`'s, which printed the values given to `add_model_header`
  and `add_harness_env`. `dbg!` or `tracing::debug!(?value)` undid the
  wire inspector's redaction.
- An Antigravity turn whose model call failed after the agent delegated to
  a subagent (a 503 with no retries left) fails with
  `AntigravityError::Turn`, as one without a subagent does. Harness 0.1.23
  reports that error before the turn's final state, so `chat` returned
  `Ok` with the error only in `errors()`, and the next call failed on the
  closed connection.
- Antigravity: no message to the harness can wait forever. `chat()` and
  `send_streaming()` start the turn deadline before sending the message, so
  a harness that has stopped reading fails the turn with
  `AntigravityError::Timeout` at `with_turn_timeout` (the default 300 s
  included) instead of holding the call past it, however large the prompt
  (inline media included). The halt sent before a turn counts against the
  five-second halt-and-drain budget, the initialization and the session-end
  request against their 30 s budgets (a stuck or failed initialization
  message fails `spawn()` with `InitFailed` and the stderr tail, where a
  failed one was a `WebSocket` error), the close in `shutdown()` waits for a
  send in progress only within its half-second bound, and any other send
  (`cancel()`, a trigger's delivery, a reply to the harness mid-turn, the
  message of a turn with no timeout) gives up after 30 s with `Timeout`
  (#504, D-066).
- Antigravity: a harness that dies mid-turn, or while `next_trigger_turn`
  waits, fails the call with `AntigravityError::ConnectionClosed` and the
  tail of the harness's stderr, as that variant says. A connection that
  ended without a close frame, as a crashed or killed harness's does, read
  as a `WebSocket` error with no stderr.
- `InlineSkill::add_dependent_tool` said the tool is loaded when the skill
  is looked up. Harness 0.1.23 loads nothing, and does nothing with
  `add_allowed_tool` either: a call to a tool outside the list still runs,
  and one inside it is still gated by the policies. Both rustdocs say so,
  and a scripted test notices the day the harness starts to honor them.

## [0.13.0] - 2026-11-03

Tracks the Interactions API as of `google-genai` 2.34.0 (swept 2026-11-01,
every item probed live; #463).

### Added

- `ClientBuilder::with_read_timeout`: the longest wait for the response
  headers or the next chunk, reset by each one. Unlike `with_timeout`, it bounds a stalled
  stream (a dropped NAT mapping, a half-open proxy) without capping a healthy
  long one, and it reaches `interactions().stream()` / `resume_stream()`,
  which no request-level timeout does.
- Decoding continuation. A response cut off by `max_output_tokens` carries
  `InteractionResponse::continuation_token`; resend the same input with
  `InteractionBuilder::with_continuation_token(token)` and a larger
  `max_output_tokens` (it counts across continuations) to continue it. Only
  `CONTINUATION_MODEL` (`gemini-4-argon`) supports it; other models answer
  `400 Decoding continuation is not enabled for this model` (verified live
  2026-11-01). `InteractionStatus::RequiresContinuation` is modeled from the
  spec but never observed: continuable responses report `Incomplete`.
- `InteractionResponse::errors` (`InteractionError`, with a string `code`
  such as `"CANCELLED"`), `UsageMetadata::initial_input_tokens`,
  `PromptedVoice::base_voice` and `CreateVoiceRequest::with_base_voice`,
  `CodeExecutionLanguage::Bash` (spec only) and `ServiceTier::Deferred`
  (Vertex-only).

### Changed

- **Breaking: `genai-rs` no longer turns on tokio's full feature set.** It
  asks for `fs` and `time` (plus `io-util`, `net`, `process`, `rt` and `sync`
  with the `antigravity` feature). Migration: a crate that wrote
  `#[tokio::main]` while relying on genai-rs to enable `rt-multi-thread` and
  `macros` enables them in its own tokio dependency.
- `GenaiError::is_retryable()` is false for an HTTP error from a request
  that can't be built (a scheme-less `with_base_url`, a rejected header) or a
  redirect loop. They fail identically every time, and a retry loop spent its
  whole budget on them.
- **Breaking: the Antigravity bridge drives harness 0.1.23**
  (`google-antigravity==0.1.23`, `SUPPORTED_HARNESS_VERSION`). It writes
  0.1.23's new `runWorkflow` toggle (disabled), which 0.1.18 rejects at init,
  and models the new non-terminal `TrajectoryState::WaitingForSubagents`,
  which otherwise read as version drift. The full harness suite passes on
  0.1.23. Migration: install `google-antigravity==0.1.23`; an older
  harness fails at init.
- **Breaking: Antigravity agents are confined to their workspaces by default.** The
  harness contains its file tools only when the client sends a policy
  config, which this crate never did: an agent could read any file the
  process could, and the content reached the model (verified on harness
  0.1.18 and 0.1.23). With a workspace configured, `spawn()` now sends one;
  a read outside the workspaces fails with `Access to path "…" is denied`.
  The harness still allows the system temp directory and
  `~/.gemini/antigravity/brain`. An explicit
  `with_workspace_containment(true)` with no workspace fails `spawn()`
  instead of running unconfined. Migration: an agent that must reach files
  outside its workspaces calls `AgentBuilder::with_workspace_containment(false)`,
  which restores the old behavior.
- **Breaking: `StreamChunk::StatusUpdate` gains `usage` and
  `continuation_token`, and `interaction_id` becomes `Option<String>`.** A
  `store: false` stream's status updates carry no interaction ID (verified
  live 2026-11-01), and the crate dropped them with a `warn!`; they now reach
  the caller. Background streams send `usage` mid-run. Migration: a
  pattern that lists `StatusUpdate`'s fields adds `..` or the new ones, and
  code that read `interaction_id` as a `String` handles `None`.
- **Breaking: removed `GenerationConfig::presence_penalty` and
  `frequency_penalty`, with `with_presence_penalty()` and
  `with_frequency_penalty()`.** The API rejects both (`400 Unknown parameter
  'presence_penalty' at 'generation_config'`, 2026-11-01), and google-genai
  2.34.0 does not list them, so any request that set one failed.
  Migration: remove the calls; no request that made one succeeded.
- A stream whose body ends before `interaction.completed` or an error event
  now ends with `Err(GenaiError::MalformedResponse)`, so a cut-off connection
  can't pass for a finished stream. A stream whose last event is
  unrecognized still ends cleanly, since that may be a new terminal event.
- `temperature`, `top_p` and `TranscriptionConfig::adaptation_phrases` are
  documented as deprecated upstream (google-genai 2.34.0); the API still
  accepts them.
- `Trigger` re-serializes `max_consecutive_failures`,
  `consecutive_failure_count` and `execution_timeout_seconds` as plain JSON
  numbers, the trigger API's own wire form (verified live 2026-11-01). It
  used the protobuf-JSON string form (`"5"`). Deserializing still accepts
  both.

### Fixed

- `LOUD_WIRE` and the `wire` tracing target printed header secrets in full:
  an MCP server's `headers` (`Authorization: Bearer ...`) and an environment
  allowlist entry's `transform`. Every value under those keys is now
  redacted; the header names stay visible.
- `wait_until_active` (files and file-search documents) bounds each status
  request by the time left, so a stalled connection can no longer hold the
  wait past its deadline (the default client has no HTTP timeout), and it
  polls through a retryable error such as a 503 instead of ending a long
  wait on it. Both waits now share one implementation.
- `EnvironmentFileList`, `FileMetadata` and `FileUploadResponse` keep fields
  the crate doesn't model (`extra`), like every other resource type.
- The streaming step accumulator no longer lets an empty `signature`
  fragment overwrite the real one an earlier event carried, which made
  replaying the step fail with `400 ... missing signature`.
- A harness output key without a decoding arm is preserved as
  `OutputPayload::Unknown` instead of panicking.
- `docs/ANTIGRAVITY.md`: on 0.1.23, `create_file`'s new `Append` argument
  lets `{Overwrite: false, Append: true}` modify an existing file, so a
  "new files only" gate must check `Append` too; a trigger before the first
  turn no longer crashes the harness; and the harness's own prompt does list
  the workspaces, which only `with_system_instructions` hides.
- A mid-stream error ended a stream silently. The API sends it as a bare
  JSON line with no `data:` prefix, then closes the connection (a recitation
  block, live 2026-11-01), and the SSE parser dropped the line. It now
  arrives as `StreamChunk::Error`, with `code: Some("recitation")`.
- The streaming docs said a stored foreground stream could be resumed. It
  can't: a foreground stream carries no `event_id`, and when its connection
  drops the server cancels the interaction. Only background interactions
  resume (`docs/STREAMING_API.md`).
- The triggers resource is verified live end to end (2026-11-01), now that
  custom agents can be created: create, get, list, update, run, list
  executions and delete. The docs no longer call its shapes unverified, and
  record what the probe found. `Triggers::run` returns an empty
  `TriggerExecution`, because the API answers `{}`; find the firing with
  `list_executions`. Only `display_name` and `status` are updatable, and the
  PATCH applies exactly the fields sent. A stored trigger's `interaction`
  echoes `store: true`, which create rejects, so clear it before reuse.

## [0.12.0] - 2026-10-31

Tracks the Interactions API as of `google-genai` 2.25.0 (swept 2026-09-24),
`gemini-3.8-flash`, the `antigravity-preview-09-2026` agent, and Antigravity
harness 0.1.18.

There is no 0.11.0: `v0.11.0` was tagged but failed validation before
publishing, so this release follows 0.10.0 directly.

### Added

- `.pages()` / `.items()` on every list builder stream all pages. They
  require `futures_util::{StreamExt, TryStreamExt}`, keep filters and page
  size on every request, stop on an empty token, and fail with
  `MalformedResponse` if a token repeats. The streams own a clone of the
  client, so they can be spawned.
- `FileUpload::with_display_name` sets a path upload's display name, which
  was always the file name.
- `docs/RESOURCES.md`: every old method mapped to its handle call, and each
  resource mapped to Google's Python SDK.
- **Voices API**: `client.voices().{list, get, create, delete}` and
  `CreateVoiceRequest`; `list()` filters with `with_region_code`,
  `with_accent`, `with_persona` and `with_context`. A custom voice id works
  as `SpeechConfig::voice` on 3.8 TTS models.
- **Credentials API**: `client.credentials().{create, get, list, update,
  delete}`, plus `RemoteEnvironment::add_env_var` / `EnvVar` and
  `AllowlistEntry::with_credential` references, verified end to end: the
  sandbox never sees a credential's secret, and the egress proxy injects it
  into requests to trusted domains. Bearer `header_name`/`prefix` are
  accepted but not applied by the API yet. `LOUD_WIRE`, the
  `genai_rs::wire` tracing output and `Debug` on `CredentialConfig`,
  `CreateCredentialRequest` and `CredentialUpdate` redact credential secrets
  (`token`, `client_secret`, `refresh_token`, `environment_variable` values)
  and environment `env` maps. Custom `WireInspector`s still receive raw
  bodies. An `environment_variable` credential takes its trusted domains as
  a required argument, `CreateCredentialRequest::environment_variable(value,
  locations, trusted_domains)`: since late October 2026 the API rejects one
  without a domain (400 `invalid_request`), although the `google-genai`
  2.34.0 bindings still mark the field optional.
- **Environment files and forking**: `client.environments().files().list(env, path)`
  (`.with_recursive(true)` for subdirectories) and `.upload(env, path,
  EnvironmentFileUpload::new(data, mime))`, and
  `CreateEnvironmentRequest::from_environment` (bare environment id only).
  `EnvironmentFile::size_bytes` is reported for files from environment
  sources; files written with `upload()` carry none (observed 2026-10-06).
- **File Search Store management**: `client.file_search_stores()` creates
  (`CreateFileSearchStoreRequest`), gets, lists, deletes and force-deletes
  stores, and uploads a `FileUpload` (a path, streamed from disk, or bytes);
  `.documents()` gets, lists, deletes and force-deletes documents, and
  `wait_until_active` waits for indexing (search returns nothing for a
  pending document). Types `FileSearchStore`, `FileSearchDocument`,
  `DocumentState`.
- **Video `processing`**: `Content::with_processing()` and
  `VideoProcessing::segment()` for clip windows, `fps` sampling and agentic
  mode. A segment window is the main token-cost lever (16,198 vs 57,778 video
  tokens for a 5s window on one test video, 2026-08-18).
- **New step types**: `Step::ToolCall` (the generic step MCP calls arrived
  as through September 2026 — previously `Unknown`, so they went uncounted;
  since 2026-10-01 MCP calls arrive as answered `function_call`s, see
  Fixed), `Step::ProcessingCall` / `ProcessingResult` (agentic video),
  `Step::RetrievalCall` / `RetrievalResult` (Vertex-only), with matching
  `StepDelta` variants, `InteractionResponse::tool_calls()` / `has_tool_calls()`,
  and new `StepSummary` counts.
- **TTS on `gemini-3.8-flash-tts`**: `Annotation::SpeechMetadata` and
  `Annotation::WordInfo`, `Content::speaker_text`, `SpeechConfig::for_speaker`.
  3.8 TTS requires a speaker annotation per text turn for multi-speaker audio.
- **Response fields**: `InteractionResponse::{labels, system_instruction,
  extra}` and `UsageMetadata::extra`. `labels` and `system_instruction` are
  filled only by `interactions().get`: create responses stopped echoing
  request fields (observed 2026-10-01). With `extra`, fields the crate
  doesn't model yet (e.g. `model_invocation_token_counts`) survive a round
  trip. Resource shapes
  (`Trigger`, `TriggerExecution`, `Environment`, `Agent`, `Webhook`) gained
  `extra` too.
- **Small parity items**: `Content::with_video_name`,
  `ResponseFormat::Video.resolution` (`VideoResolution`),
  `TranscriptionConfig::with_mode` (`TranscriptionMode`),
  `RagRanking::with_rank_service` (`RankService`).
- `ClientBuilder::with_base_url()` — point every endpoint, including upload
  URLs, at another host (a proxy or a local test stub).
- `GenaiError::Stream { message, code }` for in-stream server errors in the
  auto-function streaming loop.
- `DEFAULT_DEEP_RESEARCH_AGENT` and `DEFAULT_ANTIGRAVITY_AGENT` constants.
- `ModalityTokens::new()`.
- Antigravity: `AgentBehavior` with `AgentBuilder`/`Subagent::with_agent_behavior`,
  `BuiltinTool::ReadUrlContent` (write-capable: network egress),
  `ChatResponse::stop_reason()`, `protocol::{StopReason, Modality, AgentBehavior}`.
- List envelopes (`AgentListResponse`, `CredentialListResponse`,
  `EnvironmentListResponse`, `FileSearchStoreListResponse`,
  `DocumentListResponse`, `ListFilesResponse`, `TriggerListResponse`,
  `TriggerExecutionListResponse`, `VoiceListResponse`, `WebhookListResponse`)
  and `SigningSecret` gained a flattened `extra` map, so fields the crate
  doesn't model yet survive a round trip. All were already
  `#[non_exhaustive]`, so nothing breaks. `SigningSecret`'s `Debug` prints
  only the keys of `extra`, never the values.

### Changed

- **Breaking: `Agent::new(id, base_agent)` takes the base agent;
  `Agent::with_base_agent` is removed.** Creating custom agents now works on
  a standard API key (it was refused with a generic 400 through 2026-10-08),
  and the API rejects an agent without a base (`400 'base_agent' must be
  specified.`). The only base it accepts so far is
  `DEFAULT_ANTIGRAVITY_AGENT`; Deep Research and model ids get
  `400 Unsupported 'base_agent'.` (verified 2026-10-30). Migrate
  `Agent::new(id).with_base_agent(b)` to `Agent::new(id, b)`, and
  `Agent::new(id)` to `Agent::new(id, genai_rs::DEFAULT_ANTIGRAVITY_AGENT)`.
- **Breaking: resource methods moved from `Client` to per-resource
  handles.** `client.agents()`, `.webhooks()`, `.triggers()`,
  `.environments()`, `.files()` and `.interactions()` each return a `Copy`
  handle that borrows the client, as the new `.credentials()`, `.voices()`
  and `.file_search_stores()` do. Rename `client.<verb>_<resource>(…)` to
  `client.<resources>().<verb>(…)`: `get_file(name)` → `files().get(name)`.
  Exceptions: `run_trigger` → `triggers().run`, `list_trigger_executions(id, …)`
  → `triggers().list_executions(id)`, `ping_webhook` → `webhooks().ping`,
  `rotate_webhook_signing_secret` → `webhooks().rotate_signing_secret`,
  `get_interaction_with_input` → `interactions().get_with_input`,
  `get_interaction_stream(id, None | Some(e))` → `interactions().stream(id)` /
  `resume_stream(id, e)` (the stream now owns a clone of the client).
  `client.interaction()`,
  `execute` and `execute_stream` are unchanged. The full table is in
  `docs/RESOURCES.md`. The handle types, the `List*` builders, `FileUpload`
  and `PollOptions` are exported at the crate root.
- **Breaking: every `list_*` method returns a builder.** The positional
  `page_size`, `page_token` and `parent` arguments are gone. Chain
  `.with_page_size(n)`, `.with_page_token(t)` or `.with_parent(p)` on
  `client.<resources>().list()`, then `.send()` for one page (the same
  `*ListResponse`).
- **Breaking: uploads take a `FileUpload`.** `upload_file(p)` →
  `files().upload(FileUpload::from_path(p))`; `upload_file_with_mime(p, m)`
  → `files().upload(FileUpload::from_path(p).with_mime_type(m))`;
  `upload_file_bytes(d, m, Some(n))` →
  `files().upload(FileUpload::from_bytes(d, m).with_display_name(n))`. The
  error for an extension with
  no known MIME type now points at `FileUpload::with_mime_type()`.
- **Breaking: waits take `PollOptions`.** `wait_for_file_ready(&f, poll, timeout)`
  → `files().wait_until_active(&f.name, PollOptions::new().with_poll_interval(poll).with_timeout(timeout))`
  (defaults 120 s / 2 s). Either option can be left out.
- **Breaking: `update_mask` moved onto the update value.**
  `update_webhook(id, &u, Some("state"))` →
  `webhooks().update(id, &u.with_update_mask("state"))`. `None` becomes no
  `with_update_mask` call. The mask is sent as the query parameter, never in
  the body.
- `DEFAULT_ANTIGRAVITY_AGENT` is now `antigravity-preview-09-2026`, which
  replaced `antigravity-preview-05-2026`; the old agent shuts down on
  2026-10-05. Remote-sandbox runs that read only the model output need no
  other change. Code that parses the hosted agent's `function_call` steps by
  tool name must use the new built-in tool names (`write_to_file`,
  `replace_file_content`, `view_file`, `list_dir`, `find_by_name`,
  `grep_search`), which take PascalCase parameters. `AntigravityConfig::with_model`
  now works: the agent accepts the 3.8/3.7/3.6/3.5 Flash models and
  `gemini-3.5-flash-lite`.
- **Breaking:** `genai_rs::environment` is merged into
  `genai_rs::environments`, and the Files API types moved to a
  public `genai_rs::files` module (`FileUploadResponse` is now exported). Root
  re-exports are unchanged.
- **Breaking:** path uploads (`files().upload(FileUpload::from_path(p))`, was
  `upload_file` / `upload_file_with_mime`) stream from disk with bounded
  memory. `upload_file_chunked*`, `ResumableUpload` and
  `DEFAULT_CHUNK_SIZE` are removed: the handle was only returned after a
  successful upload, so it could never resume anything.
- **Breaking:** `InteractionStreamEvent`, `StreamMetadata` and `StreamError` are
  no longer public; no public API took or returned them.
- **Breaking:** `strict-unknown` now rejects unknown values of every string
  enum, not only `Content` and `Step` types. Every string enum implements `Eq`,
  `Hash` and `Display`.
- **Breaking:** `From<serde_json::Value> for FunctionResultPayload` wraps a
  value that is neither an object nor a string (an array, number, bool or
  null) as `{"result": value}`; a string becomes `FunctionResultPayload::Text`
  (the API rejects a top-level array as a function result).

- **Breaking:** `DEFAULT_MODEL` is `gemini-3.8-flash` (was `gemini-3.7-flash`).
  It rejects `ThinkingLevel::Minimal`.
- **Breaking:** `DEFAULT_TTS_MODEL` is `gemini-3.8-flash-tts` (was
  `gemini-2.5-pro-preview-tts`). It returns `audio/wav` rather than raw L16, and
  multi-speaker requests need `Content::speaker_text` turns instead of a
  `Name: line` transcript.
- **Breaking:** Antigravity targets harness **0.1.18**
  (`SUPPORTED_HARNESS_VERSION`) and no longer drives 0.1.10: the harness
  rejects the old plain-string user message (visible only on its stderr), so
  every turn timed out. `protocol::InputEvent::UserInput` carries a `UserInput`
  (use `InputEvent::user_text`); `ComplexUserInput` is removed.
  `ask_question` is policy-gated on 0.1.18 and needs
  `AgentBehavior::Interactive`. `AgentEvent::TextDelta` now carries only model
  text addressed to the user.
- **Breaking:** builder names follow the `with_*` (replace) / `add_*` (append)
  rule: `InteractionBuilder::set_tools` → `with_tools`;
  `FunctionDeclarationBuilder::{description, parameter, required}` →
  `with_description` / `add_parameter` / `with_required`;
  `ComputerUseConfig::{excluding, disabling_safety_policies}` →
  `with_excluded_predefined_functions` / `with_disabled_safety_policies`.
  `with_google_search` / `with_google_maps` / `with_code_execution` /
  `with_url_context` now replace an existing tool of the same kind instead of
  appending a duplicate.
- **Breaking:** `Api.message` is the parsed error envelope,
  `"STATUS_OR_CODE: message"`, no longer the raw body cut at 200 characters.
  A 2xx body that fails to parse is `MalformedResponse` (was `Json`); an upload
  response without a session URL is `MalformedResponse` (was `InvalidInput`).
- **Breaking:** response structs are `#[non_exhaustive]`, enforced by
  `tests/non_exhaustive_responses.rs`. Construct them with `T::default()` and
  field assignment, or deserialize a fixture. Request-side types are unchanged.
- **Breaking:** `InteractionInput::Content` is sent as a single `user_input`
  step (the form that accepts video `processing`). `with_content()` callers
  need no change.
- **Breaking:** `Role` is a plain enum without serde or an `Unknown` variant
  (it never reaches the wire). `FunctionCallingMode` and
  `CodeExecutionLanguage` no longer accept pre-revision uppercase spellings.
- **Breaking:** `StepDelta::FunctionResult.call_id` is `Option<String>`;
  `AutoFunctionStreamChunk::ExecutingFunctions.pending_calls` is
  `Vec<OwnedFunctionCallInfo>`. New fields on `Content::Video`,
  `ResponseFormat::Video`, `RemoteEnvironment`, `AllowlistEntry`,
  `CreateEnvironmentRequest`, `TranscriptionConfig` and `RagRanking` break
  exhaustive struct literals.
- Request `labels` are accepted by the Gemini API (verified 2026-09-24; no
  longer Vertex-only).
- `#[tool]` needs only `genai-rs` and `genai-rs-macros` — no `async-trait`,
  `serde_json` or trait import at the expansion site. The two crates must be
  bumped together (the expansion uses `genai_rs::__private`).
- `genai-rs-macros` no longer depends on `utoipa`; `regex` is no longer a
  runtime dependency.
- Tool declarations are sent sorted by name; Files endpoints now send the
  `Api-Revision` header too (the server currently ignores its value).
- Examples: every example runs live, exits non-zero when what it
  demonstrates didn't happen, and cleans up; a subset runs in CI. New
  coverage for stream resume, `interactions().delete`, `VideoProcessing::segment`
  and `with_image_config`.

### Removed

- **Breaking:** `MINIMAL_THINKING_MODEL`. Every text-based constant now uses
  the 3.8 Flash models, which have no `minimal` thinking level. To keep using
  `ThinkingLevel::Minimal`, pass a model that accepts it (`gemini-3.6-flash`
  did on 2026-10-01) to `with_model()`.
- **Breaking:** `INLINE_VIDEO_MODEL`. Its premise was a fixture bug: the test
  clip was 0.2 s long and yields no sampled frame. Any clip of 1 s or longer
  works inline on `DEFAULT_MODEL` (D-012).
- **Breaking:** `with_cached_content()` / `InteractionRequest.cached_content`
  (the Interactions API rejects the field in every spelling and placement;
  implicit caching still shows in `usage.total_cached_tokens`).
- **Breaking:** the `interactions_api` module; `PendingFunctionCall`;
  `Content::{image,video}_{data,uri}_with_resolution` (use
  `.with_resolution()`); `InteractionResponse::{created, updated,
  code_execution_call, google_search_call, url_context_call_id}`;
  the `excludedPredefinedFunctions` alias.
- Examples that demonstrated nothing real: `rag_system`, `web_scraper_agent`,
  `code_assistant`, `testing_assistant`, `multi_turn_agent_manual`,
  `response_passthrough`, `retrieval_grounding`, `text_input`. Folded into
  others: `parallel_and_compositional_functions`, `thought_echo`,
  `cancel_interaction`, `data_analysis`.

### Fixed

- `safety_settings` work on the Gemini API, which accepts them as of
  2026-10-30 (Vertex-only through 2026-10-08). `SafetySetting` now sends its
  category under the wire key `type`, as the API defines it; it sent
  `category`, which the API never accepted (`400 Unknown parameter
  'category' at 'safety_settings[0]'`), and the old Vertex-only rejection hid
  the mistake. The Rust field is still `category`, but deserializing a
  `SafetySetting` now requires `type`. `HarmCategory::CivicIntegrity`, the
  four `Image*` categories and `SafetyThreshold::BlockNone` are still
  rejected by the Gemini API as Vertex-only.
- The auto-function loops now run a declared function when the API calls it
  as `default_api:<name>`, which it started doing on the second user turn of
  a non-streamed chain (2026-10-01). Previously the loop answered "function
  not found". The function result still carries the call's own name.
- The auto-function loops no longer try to run MCP calls the server already
  answered: since 2026-10-01 those come back as a `function_call` and its
  `function_result` in the same response. New
  `InteractionResponse::pending_function_calls()` returns only the calls
  still waiting for a result.
- The SSE parser rescanned its whole buffer on every network chunk, so a
  multi-megabyte event (image output) cost quadratic CPU: about 200 ms of
  CPU per image stream, now about 50 ms. Text streams parse 15–40% faster.
- An upload MIME type that cannot be a header value returns `InvalidInput`
  (not retryable) instead of a retryable `GenaiError::Http`, for every upload
  path.
- Files list responses drop only an undeserializable entry (with a warning)
  and treat a `null` list as empty, like every other resource list, instead
  of failing the whole page.

- **Antigravity policies were bypassed on the pre-tool hook path** for MCP
  tools (`mcp_<server>_<tool>`) and `start_subagent`, because the harness names
  them differently there; on 0.1.18 that hook is the only gate. Hook-denied
  calls now surface as `ToolDecision::Denied`, `on_post_tool` fires once per
  custom call, and every builtin toggle is sent explicitly (0.1.10 silently
  exposed `manage_task` / `schedule`).
- **`get_interaction_stream` never streamed** — it omitted `stream=true`, so
  the server returned plain JSON and the stream ended empty. Its replacement,
  `interactions().stream()`, works for background interactions (the only
  kind the API streams on GET).
- **Streamed `processing_call` / `processing_result` signatures were dropped**,
  so stateless replay of a streamed agentic-video turn failed with 400.
- Tracing spans recorded the whole request (prompts, base64 media) at INFO;
  bodies are now logged only at debug, and not serialized at all when debug
  is off.
- `with_tool_service()` functions were not declared to the model when any other
  tool was set.
- The streaming auto-function loop reported "Stream ended without Complete
  event" instead of the server's in-stream error.
- `execute_stream()` did not set `stream: true` itself.
- `wait_for_file_ready()` (now `files().wait_until_active()`) reported a
  failed file as a retryable 500.
- The SSE parser dropped a final event without a trailing newline and did not
  join multi-line `data:` fields.
- `#[tool]` mapped `i8`/`u8`/`i16`/`u16`, `&str`, `char` and path-qualified
  types to `object`, and dropped descriptions on `Vec` parameters.
- `speech_config` in the `{"speakers": [...]}` form deserialized to one empty
  config, silently discarding every speaker.
- `document_from_file` rejected `.txt` / `.md`; they are sent as `text/plain`
  / `text/markdown`.
- Unknown SSE event types are logged at `warn` (Evergreen), not `debug`.

### Security

- Antigravity policy bypass on the pre-tool hook path (see Fixed): `deny`
  rules for MCP tools and `start_subagent` were not applied.


## [0.10.0] - 2026-08-16

### Added

- **Model constants — one place to change the model.** `DEFAULT_MODEL`,
  `INLINE_VIDEO_MODEL`, `MINIMAL_THINKING_MODEL`, `DEFAULT_IMAGE_MODEL` and
  `DEFAULT_TTS_MODEL` are now public, and every test, example, doctest and
  doc snippet references them instead of a string literal.

  Bumping the model previously meant editing ~590 occurrences of the text
  model plus 27 image and 35 TTS, with no single source of truth. A sweep
  that size reliably misses a few, and a missed one is invisible: the test
  keeps passing against a model nobody meant to still be using, until that
  model is retired and the failure arrives with no obvious cause. It is now
  a one-line change.

  A new `tests/model_literals.rs` guard fails on any hardcoded
  `"gemini-<digit>"` outside those constants, so it cannot silently
  regress. In-crate unit tests use either the constants or an obviously
  synthetic `"test-model"` — they exercise serialization round-trips and
  never cared which model, so neither form is a bump site.

### Changed (breaking)

- **`AgentBuilder`'s per-turn budget defaults to `DEFAULT_TURN_TIMEOUT`
  (300s) instead of being unlimited.** An unbounded turn does not fail when
  the harness stops signalling completion — it *hangs*, which is strictly
  less diagnosable than an error and looks identical to latency. That is
  the exact shape of the 0.1.10 break this crate just shipped a fix for.
  `without_turn_timeout()` restores the old behavior explicitly.

### Changed

- **Antigravity examples moved to `examples/antigravity/`.** All seven —
  the `agent.rs` starter (was `examples/antigravity_agent.rs`) and the six
  projects (was `examples/real_world/*`) — now sit in one directory,
  mirroring `src/antigravity/`. They share a setup story the rest of
  `examples/` does not: every one needs the `antigravity` feature *and* the
  `localharness` binary. Example **names are unchanged**, so
  `cargo run --example repo_auditor --features antigravity` still works;
  only paths moved. The group has its own
  [README](examples/antigravity/README.md), and `examples/real_world/`
  points at it.

- **Default model is now `gemini-3.7-flash`** (from `gemini-3.6-flash`).
  Thinking cost on a trivial prompt is unchanged (68 vs 67 tokens), so the
  `max_output_tokens` headroom in the sampling tests still holds. The full
  live suite was run against it — 208 integration tests — which surfaced
  two capability gaps, neither of which unit tests could have shown:

  - **Inline (base64) video is rejected** with the same `400
    invalid_request` as 3.6, while video by URI works. `INLINE_VIDEO_MODEL`
    remains necessary; the bump does not close that gap.
  - **`ThinkingLevel::Minimal` is rejected**: *"'minimal' is not a
    supported thinking level for this model. Allowed values are: high, low,
    medium."* `gemini-3.6-flash` and `gemini-3.5-flash` still accept it.
    The variant stays valid — model support is what varies — and the new
    `MINIMAL_THINKING_MODEL` constant pins the test so it keeps exercising
    the `minimal` wire path.

- **`LOUD_WIRE` summary labels are scoped to received harness frames.**
  Outgoing `InputEvent` arms have no actions, so qualifying them produced
  `questionResponse/response` and collided with `response`, the HTTP
  category selector. Envelope stripping is likewise harness-only:
  `usageMetadata` is bookkeeping on that wire but a real field on a Gemini
  HTTP response, where stripping it could render a body as
  `(no payload keys)`.

## [0.9.0] - 2026-08-10

### Changed (breaking)

- `InteractionRequest` gains public `safety_settings` and `labels` fields
  and `GenerationConfig` gains `transcription_config` — source-breaking
  for downstream struct literals and exhaustive patterns on these
  constructible structs, hence the minor bump (cargo treats 0.8.x as
  compatible, so this could not ship as 0.8.1). The structs deliberately
  remain constructible (no `#[non_exhaustive]`): per the project's
  versioning philosophy, future field additions will take minor bumps
  rather than trading away struct-literal ergonomics.
  (`antigravity::protocol::MultipleChoice` — behind the `antigravity`
  feature — likewise gains a public flattened `extra` field.)

### Added

- **`LOUD_WIRE` filtering.** The variable now takes a comma-separated
  filter in addition to `1`: category selectors (`request`, `response`,
  `sse`, `upload`, `harness`, `ws`), any WebSocket payload key
  (`stepUpdate`, `toolCall`, …, matched case-insensitively), and a
  `summary` modifier that collapses each event to one line (order-free:
  `1,summary` and `summary,1` agree). The historical "on" spellings (`1`,
  `true`, `yes`, `on`, `all`, empty) still mean everything-pretty-printed.
  Note that the gate was previously "is the variable set at all", so *any*
  value produced the firehose; other values are now parsed as selectors,
  and a value matching no category and no payload key — `0`, `false`,
  `off` — prints nothing where it previously printed everything. Unfiltered
  output was unusable for antigravity sessions, where a few turns produce
  thousands of lines. Exposed programmatically as `wire::WireFilter` and
  `LoudWirePrinter::with_filter`; `LoudWirePrinter` is consequently no
  longer `Copy` (it still derives `Clone`). The antigravity agent
  builder honors the filter too — it previously re-implemented the env
  gate and ignored the value, which meant filtering did not work on the
  one surface that emits WebSocket messages at all.

- **`LOUD_WIRE` selectors reach step actions.** A selector now matches one
  level into a WebSocket payload, so `LOUD_WIRE=mcpTool` (or `runCommand`,
  `viewFile`, …) selects the steps carrying that action — previously those
  names matched nothing at all, because every builtin action lives under
  the single `stepUpdate` key. Summary lines are qualified to match
  (`stepUpdate/mcpTool`), so what you asked for is what the output names.

- **The protocol-drift guard now covers field renames, not just enum
  values.** The 0.1.5 → 0.1.10 upgrade shipped one of each — `STATE_IDLE`
  → `STATE_FULLY_IDLE` (a value) and `usageMetadata` → `usageUpdate` (a
  field) — so a guard checking only enums would have caught half the break
  it was written for. It now also checks the `OutputEvent` and `InputEvent`
  wire fields the crate reads by hand, where a rename yields a silent
  `None` rather than a parse error.

- **Three more antigravity worked examples**, all runnable against a real
  harness and smoke-run in CI: `mcp_toolbelt` (external tools over MCP,
  with a dependency-free stdio server fixture and the `mcp_<server>_<tool>`
  policy-target spelling), `proactive_agent` (`add_trigger`, and observing
  deliveries through a wire inspector — the only way to see one today), and
  `cancellable_turn` (`cancel_handle` from another task, keeping partial
  output). Together with the existing four, every antigravity builder
  surface now has an example.

  Writing them surfaced a harness limitation now documented in
  `docs/ANTIGRAVITY.md`: **a trigger that fires into a conversation with no
  history crashes harness 0.1.10 outright** (`earliest step index is out of
  bounds: 0 vs 0`), taking the session with it. One completed turn first
  avoids it.

- **Antigravity e2e coverage for the last untested surfaces.**
  `with_response_schema`, `CancelHandle`, the `on_questions` hook,
  `add_mcp_server` and subagent *invocation* (as opposed to subagent
  config, which was already covered) are now exercised against a real
  harness — the MCP test drives a stdio server fixture end to end and
  asserts on a token the model cannot have guessed.

  The cancellation test corrected a documented claim: harness
  0.1.10 answers a halt with `STATE_FULLY_IDLE`, the same terminal state as
  a natural completion, so a cancelled turn **resolves normally with
  partial output** rather than failing with `AntigravityError::Turn` as
  `CancelHandle` and `docs/ANTIGRAVITY.md` previously stated. The `Turn`
  error remains the outcome for harness-initiated cancellation.

- **Protocol-drift diagnostics for the antigravity bridge.**
  `protocol::drift_report()` returns every unrecognized wire value seen
  (`"EnumName=WIRE_VALUE" -> count`) with `clear_drift_report()` to reset
  it, `all_wire_values()` on each wire enum exposes the spellings the
  crate recognizes, and `shutdown()` logs the aggregate once. Unknown-value
  preservation previously produced no signal beyond a `warn!` nobody
  reads — which is what let a renamed value silently stop matching. CI
  additionally diffs the installed wheel's protobuf descriptor against
  these enums and fails naming anything the harness can send that the
  crate does not model. See the Debugging section of `docs/ANTIGRAVITY.md`.

- **Two antigravity worked examples**, both runnable against a real
  harness: `examples/real_world/session_resume` (trajectory persistence —
  the `with_save_dir` + `conversation_id()` + `with_conversation_id` round
  trip, and the fact that resuming an unknown id comes back *empty* rather
  than erroring) and `examples/real_world/workspace_explorer` (workspaces
  with real files, the typed `AgentEvent::ToolAction` stream, and an
  `on_pre_tool` hook that refuses by content where a name-based policy
  cannot). Both are covered by new harness integration tests.

- Wire enums accept alias spellings, so a value renamed between harness
  revisions resolves to one variant while `as_wire_str` keeps emitting
  the canonical (current-harness) form.
- A stalled turn now diagnoses itself. When a turn times out having seen
  unrecognized *main-trajectory* states, the timeout names them and
  points at `SUPPORTED_HARNESS_VERSION` instead of reporting an
  undifferentiated stall — the failure that took a wire trace to
  diagnose now reads as a version mismatch on the error itself.


- **Triggers resource** (`/v1beta/triggers`): server-side scheduled
  interactions with full CRUD plus `run_trigger` and
  `list_trigger_executions` — cron agents that fire with no client process
  running. Trigger creation requires a custom agent (gated on standard API
  keys, verified live); the list path and payload schema are live-verified.
- **Environments resource** (`/v1beta/environments`): explicit
  `create/get/list/delete_environment` so many interactions can share one
  container (full lifecycle verified live on a standard key). Handles the
  protobuf-JSON string int64 wire form for `file_count`/`size_bytes`.
- **`TranscriptionConfig`** in `generation_config`
  (`with_transcription_config`): language hints, diarization, custom
  vocabulary, adaptation phrases and timestamp granularities for audio
  input (accepted live).
- **`SafetySetting`/`HarmCategory`/`SafetyThreshold`/`SafetyMethod`** and
  the `safety_settings` request field (+ `with_safety_settings`/
  `add_safety_setting`), and **`labels`** request metadata
  (+ `with_labels`/`add_label`). Both parameters are currently rejected by
  the Gemini API ("available on the Gemini Enterprise Agent Platform",
  verified live 2026-08-08) — modeled for spec parity and forward
  compatibility, like the existing `Retrieval` tool.
- **`AntigravityConfig`** typed agent-config helper for server-side
  `agent("antigravity-preview-05-2026")` interactions (`model`,
  `max_total_tokens`; the config's `antigravity` string is only the
  `agent_config` type discriminant, not an agent ID).
- **Antigravity bridge: `on_questions` hook** — answer the agent's
  `ask_question` batches programmatically (select choices, freeform text,
  skip, or cancel) instead of the previous always-"unanswered" fallback.
  An unmodeled future question type arrives with
  `AgentQuestion::is_unknown_type()` set and its raw payload in `extra`
  (build that fixture with `AgentQuestion::unknown()`).
- `InteractionRequest` and `InteractionInput` implement `Default`, easing
  struct-literal construction (e.g. nested trigger interactions).
- `InteractionRequest`, `GenerationConfig`, `SpeechConfig`, `Tool`,
  `FunctionParameters`, `Trigger` and the trigger request/response types
  implement `PartialEq`, so whole-value assertions work across the new
  resource types uniformly. The pre-existing `Agent` and
  `AgentListResponse` gain it too, completing the set.

### Changed

- **Default model guidance moves to `gemini-3.6-flash`** (from
  `gemini-3-flash-preview`) across docs, examples, and tests, and image
  generation moves to `gemini-3.1-flash-image` (from
  `gemini-3-pro-image-preview`). Both were probed live before migrating.

  One capability difference surfaced and is worth knowing: `gemini-3.6-flash`
  returns `400 invalid_request` for **inline (base64) video bytes** while
  accepting video **by URI**. Image, audio and PDF inline data are
  unaffected. The four inline-video tests are pinned to a model that
  accepts that form (`tests/common::VIDEO_INLINE_MODEL`) so they keep
  testing the bytes path rather than the model's appetite for it.


- CI's antigravity job now receives `GEMINI_API_KEY` (with the same
  same-repo guard the integration matrix uses). Its model-backed tests
  are the only ones that drive a real turn end-to-end, and without a key
  they self-skipped — which is why the turn-completion break above
  reached a release unnoticed.
- `docs/ANTIGRAVITY.md` no longer claims newer harnesses "degrade
  gracefully". Unknown-value preservation stops a crash, but when a
  *renamed* value is one the bridge matches on, preservation is exactly
  what makes the breakage silent.



- All five resource list envelopes (`AgentListResponse`,
  `WebhookListResponse`, and the new trigger/execution/environment ones)
  now degrade a null or malformed list key to an empty page and drop
  undeserializable elements individually with a `tracing::warn!`, instead
  of failing the whole response. For the two pre-existing types this
  changes observable behavior: `list_agents()`/`list_webhooks()` calls
  that previously returned `Err` on a malformed page now return the
  surviving entries.
- `Webhook::create_time`/`update_time` and `SigningSecret::expire_time`
  now deserialize leniently like the trigger and environment timestamps:
  a response whose timestamp encoding diverges from RFC 3339 yields the
  field as `None` (with a `tracing::warn!`) instead of failing the call —
  so an absent timestamp on a returned webhook can mean either "not sent"
  or "sent but unparseable"; the warn log distinguishes them.

### Deprecated

- `response_modalities` is marked deprecated by the official SDK in
  favor of the typed `response_format` union; the builder and field docs
  now steer new code to `with_response_format`. (The SDK deprecates
  `response_mime_type` alongside it, but that field was already removed
  from this crate — the API rejects it in every form.)

### Fixed

- **`on_post_tool` reported successful harness tools as failures.** The
  harness sends `"error": ""` — protobuf's default for an unset string —
  on calls that succeeded, and the bridge surfaced that as `Some("")`, so
  `ToolOutcome::error.is_some()` (the check the field's own docs invite)
  was true for every successful builtin. Blank now normalizes to `None` on
  both dispatch paths. Found by running the new `workspace_explorer`
  example against a live harness; invisible to every existing test.

- **`ThinkingSummaries` sent a value the API now rejects.**
  `to_agent_config_value()` emitted the SCREAMING_CASE
  `THINKING_SUMMARIES_AUTO` / `_NONE` spelling; verified live 2026-08-10
  the API responds `The value 'THINKING_SUMMARIES_AUTO' is not supported
  for 'agent_config.thinking_summaries'. Supported values: 'auto',
  'none'.` — so deep-research requests carrying thinking summaries failed
  outright. Both contexts now emit the lowercase form; deserialization
  still accepts either spelling.

- **Antigravity harness 0.1.10 support (turn completion was broken).** The
  supported harness moves 0.1.5 → 0.1.10. Two wire spellings the bridge
  depends on were renamed in that range, and both failed silently rather
  than loudly:
  - `STATE_IDLE` → `STATE_FULLY_IDLE`. Only that value ends a turn, so
    against a 0.1.6+ harness **every turn ran to its timeout** — no parse
    error, no failed assertion, just a bare
    `Timeout { operation: "agent turn" }` after the full budget.
  - `usageMetadata` → `usageUpdate` (now `{agents[], total}`), which
    silently zeroed token accounting and was additionally misreported as
    an unknown payload variant.

  Both old spellings are accepted as aliases, so a single build drives
  either harness revision; verified by running the full harness suite
  against 0.1.5 and 0.1.10 (10/10 each). Also adds the non-terminal
  `STATE_WAITING_FOR_TASKS` state introduced in 0.1.10.


- Resource IDs are now percent-encoded when interpolated into URL paths
  (interactions get/delete/cancel/stream and the agents, webhooks, triggers
  and environments item URLs). Well-formed IDs are byte-identical on the
  wire; a value containing path metacharacters (`/`, `?`, `#`) is now
  sent as one encoded segment instead of silently rewriting the request
  path, and an empty or dot-segment resource ID (any WHATWG spelling —
  `.`/`..` bare or percent-encoded, which the URL parser would otherwise
  pop at parse time) is rejected locally as `InvalidInput` rather than
  issuing a request against the collection or a different URL. The Files
  API's `get_file`/`delete_file` now validate the full resource name's
  shape positively — the `files/` prefix plus exactly one non-empty
  segment, with the ID percent-encoded like every other resource — so an
  empty name, a `.`/`..` dot segment (bare or percent-encoded), a `?`/`#`
  query or fragment split, or stray extra segments all fail locally as
  `InvalidInput` instead of silently addressing a different URL. The Files API
  `list_files` page token is likewise percent-encoded (agents and
  webhooks already encoded theirs, and the new trigger/environment
  endpoints ride the shared encoder), so a token carrying a reserved
  character is no longer truncated on the wire (a standard-base64 `+`
  previously decoded to a space).
- docs.rs now builds with the `antigravity` feature enabled (the module was
  invisible in the 0.8.0 docs, which were built with default features only),
  with "Available on crate feature ... only" banners on feature-gated items.
  The CI doc gate now builds both `--all-features` and the docs.rs feature
  set under `-D warnings`.
- The `audio_input` and `video_input` examples now embed real media
  fixtures, replacing the zero-length WAV and header-only MP4 that the API
  rejects with 400 `invalid_request` — both examples previously always took
  their error branch.
- Corrected `#[tool]` snippets across the guides: an invalid
  `#[tool(description = ...)]` attribute form the macro parser rejects,
  `.declaration()` called on the function item instead of the generated
  callable struct, and missing `CallableFunction` / `tool` imports.
- README installation snippet now lists `async-trait` and `serde_json`,
  required by `#[tool]`-generated code, and notes the `CallableFunction`
  import — following the previous snippet produced a compile error on
  first macro use.

## [0.8.0] - 2026-07-14

This release migrates the crate to Interactions API wire revision
**2026-05-20** (the steps model), adds the remaining 2026-05-20 API surface
(webhooks, environments + agents, the retrieval tool, typed response
formats, video generation config, multi-speaker TTS, Deep Research
options), introduces a structured wire-inspection layer
(`genai_rs::wire`), and ships a new off-by-default `antigravity` feature —
a native Rust client for Google's Antigravity `localharness` agent
runtime. It is a deliberately breaking release; see the migration guide
below.

> **Verification**: wire shapes were derived from Google's generated
> `google-genai` 2.10 API bindings and covered with fixture + proptest
> roundtrip tests. Both the core revision and the phase-2 surface were
> verified live 2026-07 with a real `GEMINI_API_KEY` (`LOUD_WIRE=1`
> against `generativelanguage.googleapis.com`): revision accepted, steps
> model and snake_case field naming confirmed, `function_call`
> `signature` discovered and modeled, lowercase response modalities and
> the `response_mime_type` rejection confirmed; webhooks full
> CRUD/ping/rotate, environments, typed response formats, multi-speaker
> TTS, deep-research knobs, and the video config schema all exercised
> live. Live findings (details in `docs/INTERACTIONS_API_GAP.md` /
> `docs/ENUM_WIRE_FORMATS.md`): `VideoTask` gained a live-discovered
> `Extend` variant; webhook `:ping` accepts the empty `{}` body and PATCH
> `update_mask` is optional and observed to be ignored (the body scopes
> the update); several knobs are Vertex-only and rejected by the Gemini
> API (`Tool::Retrieval`, `enable_bigquery_tool`, video `gcs_uri`);
> audio/image response formats are inline-only today (audio
> `mime_type`/`delivery` and image `delivery` rejected; image `mime_type`
> limited to `image/jpeg`); Veo models are not served by the Interactions
> API; and agent creation is gated on standard API keys (agent `tools`
> accept only `code_execution`/`google_search`/`url_context`).

### Migration guide (0.7 → 0.8)

**Steps replace `Turn` and `outputs`.** `InteractionResponse.outputs:
Vec<Content>` is now `steps: Vec<Step>`, and `Turn` /
`InteractionInput::Turns` are gone — conversation history is a step list.
Convenience accessors (`as_text()`, `function_calls()`, `images()`, ...)
keep working unchanged.

```rust
// Before
.with_history(vec![Turn::user("hi"), Turn::model("Hello!")])

// After
let mut history = vec![Step::user_text("hi")];
history.extend(response.output_steps()); // replay prior model output
```

**Function-call signatures MUST be replayed.** `function_call` (and
thought) steps carry an opaque `signature` the API rejects stateless
replay without. Never reconstruct model turns by hand — extend history
with `response.output_steps()`, which preserves signatures.

**Response modalities are lowercase.** The API rejects `"IMAGE"`/`"AUDIO"`;
`with_image_output()`/`with_audio_output()` now send `"image"`/`"audio"`
and `with_response_modalities()` normalizes to lowercase. No code change
needed unless you passed uppercase strings you also match on elsewhere.

**`response_mime_type` is gone** (the API rejects the field in every
form). Use `with_response_format()` — passing a JSON schema implies JSON
output:

```rust
// Before
.with_response_mime_type("application/json").with_response_format(schema)
// After
.with_response_format(schema)
```

**`with_response_format()` now takes `impl Into<ResponseFormat>`.** Raw
`serde_json::Value` schemas keep compiling (they convert to
`ResponseFormat::Text { mime_type: "application/json", schema }`); pass
typed `ResponseFormat` variants for audio/image/video output, or
`with_response_formats(Vec<ResponseFormat>)` for the list form.

**`speech_config` is now a list** (`Option<Vec<SpeechConfig>>`,
multi-speaker TTS). `with_speech_config(single)` still compiles and sends
a one-entry list; use `with_speech_configs()` / `add_speech_config()` for
multi-speaker dialogue.

**`tool_choice` is a typed union.** The top-level
`generation_config.allowed_tools: Vec<String>` was removed;
`with_allowed_tools(vec![...])` now emits the spec's
`{"allowed_tools": {"mode", "tools"}}` object, and
`with_tool_choice(ToolChoice::...)` sets the lowercase mode strings
(`auto|any|none|validated`) directly.

**reqwest 0.13 changed TLS trust anchors.** Certificates are now verified
against the **OS trust store** (`rustls-platform-verifier`) instead of
bundled Mozilla roots. Scratch/distroless containers without a CA bundle
will fail TLS — install one (e.g. `ca-certificates`). The TLS stack is
still rustls.

### Added

#### Interactions API revision 2026-05-20

- Every Interactions API request now sends `Api-Revision: 2026-05-20`
  (create/get/delete/cancel, streaming included; the Files API is
  unrevisioned, matching google-genai). Header constants live in
  `src/http/common.rs`.
- New types: `Step`, `StepDelta`, `StepError`, `FunctionResultPayload`,
  `StepSummary`, the `Annotation` citation union + `ReviewSnippet`,
  `ToolChoice` + `AllowedTools`, `ServiceTier`, `GroundingToolCount`,
  `StreamMetadata`.
- New response accessors: `output_contents()`, `output_steps()`,
  `thought_summaries()`, `unknown_steps()`; `Step`/`StepDelta` accessors
  (`as_text()`, `signature()`, `step_type()`, `as_arguments_delta()`,
  ...); `StreamChunk::delta_text()` convenience.
- `InteractionResponse` gained `environment_id`, `output_text`, a typed
  optional `input`, and a `Default` impl. It also now models `object`
  (`"interaction"`), `service_tier` (the effective billing tier), and the
  `webhook_config` echo — all returned by the live API and previously
  dropped silently.
- `InteractionStatus`: first-class `BudgetExceeded` (`"budget_exceeded"`)
  and new `Incomplete` variant for interactions that end before
  completion.
- `service_tier` request field + `with_service_tier()`
  (`flex | standard | priority` + Unknown).
- `cached_content` request field + `with_cached_content()` (explicit
  caching).
- `presence_penalty` / `frequency_penalty` generation config fields +
  `with_presence_penalty()` / `with_frequency_penalty()`.
- `include_input` query param support:
  `Client::get_interaction_with_input()`.
- `UsageMetadata.grounding_tool_count: Vec<GroundingToolCount>` +
  `grounding_count_for_tool()`.
- `AudioInfo::sample_rate()` / `channels()` (and `Content::Audio` gained
  `sample_rate` and `channels`); `GoogleSearchResultItem.search_suggestions`;
  `Place.url` / `review_snippets`.

#### Webhooks

- Full `/v1beta/webhooks` resource client: `Client::create_webhook()`,
  `get_webhook()`, `list_webhooks()`, `update_webhook()` (with
  `update_mask`), `delete_webhook()`, `ping_webhook()`, and
  `rotate_webhook_signing_secret()` (with `RevocationBehavior`).
- New types in `src/webhooks.rs`: `Webhook`, `WebhookUpdate`,
  `SigningSecret`, `WebhookListResponse`, `RotateSigningSecretResponse`,
  and Evergreen enums `WebhookEvent` (`batch.succeeded/expired/failed`,
  `interaction.requires_action/completed/failed`, `video.generated`) and
  `WebhookState` (`enabled`, `disabled`,
  `disabled_due_to_failed_deliveries`).
- Per-request webhook routing: `webhook_config {uris, user_metadata}` on
  `InteractionRequest` + `InteractionBuilder::with_webhook_config()`.
  The API echoes it back on the create response (verified live 2026-07);
  `InteractionResponse.webhook_config` models the echo.
- Webhook/agent endpoints send the same `Api-Revision: 2026-05-20` header
  as interactions (matching google-genai, which applies the revision
  header globally).

#### Environments and Agents resource

- `environment` field on `InteractionRequest` +
  `InteractionBuilder::with_environment()`, accepting a string environment
  ID or a typed `RemoteEnvironment` (`EnvironmentSpec` union). New types in
  `src/environment.rs`: `EnvironmentSource` (sources
  `gcs|inline|repository|skill_registry` via `SourceType`),
  `NetworkConfig` (`"disabled"` | `{allowlist}` union), `AllowlistEntry`
  (domain wildcard + header-injection `transform`).
- `/v1beta/agents` resource client: `Client::create_agent()`,
  `get_agent()`, `list_agents()` (`page_size`/`page_token`/`parent`),
  `delete_agent()`; `Agent` type (`id`, `base_agent`,
  `system_instruction`, `description`, `tools`, `base_environment`) in
  `src/agents.rs`.

#### Retrieval tool

- `Tool::Retrieval` with `RetrievalType`
  (`vertex_ai_search|rag_store|exa_ai_search|parallel_ai_search` +
  Unknown) and per-backend configs: `VertexAiSearchConfig`
  (`engine`/`datastores`), `ExaAiSearchConfig`/`ParallelAiSearchConfig`
  (`api_key`/`custom_config`), `RagStoreConfig` (`rag_resources`,
  deprecated `similarity_top_k`/`vector_distance_threshold`,
  `rag_retrieval_config` with `top_k`/`hybrid_search`/`filter`/`ranking`).
- `RetrievalConfig` builder for `add_tool()` that keeps `retrieval_types`
  in sync with the configured backends. Note: rejected as Vertex-only by
  the Gemini API (verified live 2026-07).

#### Video generation and Deep Research config

- `generation_config.video_config {task}` (`VideoConfig` + `VideoTask`
  enum: `text_to_video|image_to_video|reference_to_video|edit|extend` +
  Unknown; `extend` discovered via the API's live validation error
  2026-07), `with_video_config()`, and the `with_video_output()` modality
  shortcut (`response_modalities: ["video"]`).
- `DeepResearchConfig::with_visualization(Visualization)` (`off|auto` +
  Unknown), `with_collaborative_planning(bool)`, and
  `with_bigquery_tool(bool)` (Vertex-only); managed agent IDs (incl.
  `deep-research-preview-04-2026`, `deep-research-max-preview-04-2026`,
  `antigravity-preview-05-2026`) documented in
  `docs/AGENTS_AND_BACKGROUND.md`.

#### Built-in tool configuration

- Google Maps built-in tool support: `Tool::GoogleMaps` (with
  `latitude`/`longitude`), `google_maps_call`/`google_maps_result` steps,
  `GoogleMapsResultInfo` view type, response accessors
  (`has_google_maps_results()`, `google_maps_results()`), `Place` struct
  (with Evergreen forward-compatible `extra` field), and the
  `with_google_maps()` shorthand.
- `SearchType` enum for Google Search `search_types` configuration, incl.
  `SearchType::EnterpriseWebSearch`.
- `Tool::McpServer` gained optional `allowed_tools` (`[{mode, tools}]`
  per the spec) and `headers` fields.
- `Tool::ComputerUse` gained `enable_prompt_injection_detection` and
  `disabled_safety_policies`; documented `mobile`/`desktop` environments;
  `ComputerUseConfig::with_environment()` /
  `with_prompt_injection_detection()` / `disabling_safety_policies()`.
- Unified `add_tool(impl Into<Tool>)` on `InteractionBuilder` with tool
  configuration structs: `GoogleMapsConfig`, `GoogleSearchConfig`,
  `McpServerConfig`, `ComputerUseConfig`, `FileSearchConfig`.

#### Wire inspection (`genai_rs::wire`)

- Public `genai_rs::wire` module for structured wire-level inspection:
  - `WireEvent` (requests, response status/bodies, error bodies, SSE
    frames, file uploads) with per-client request-id correlation.
  - `WireInspector` trait and `ClientBuilder::add_wire_inspector()`
    (multiple inspectors supported).
  - `LoudWirePrinter` built-in inspector — the colored stderr printer
    behind `LOUD_WIRE=1` (unchanged UX: the env var, now read at `Client`
    construction, installs it automatically).
  - `TracingForwarder` built-in inspector — forwards wire events to
    `tracing` at `DEBUG` under the new `genai_rs::wire` target
    (`RUST_LOG=genai_rs::wire=debug`).
- SSE `event:` lines are now surfaced to wire inspectors as
  `WireEvent::SseFrame { event_type, .. }`.

#### Antigravity (new `antigravity` feature, off by default)

- Native `genai_rs::antigravity` client for Google's Antigravity
  `localharness` agent runtime (see `docs/ANTIGRAVITY.md`):
  - `AntigravityAgent::builder()` — spawn the harness binary (discovery
    via `ANTIGRAVITY_HARNESS_PATH`, python3 site-packages, or `PATH`),
    stdio handshake, localhost WebSocket, conversation init; pinned to
    `google-antigravity` 0.1.5 (`SUPPORTED_HARNESS_VERSION`).
  - `agent.chat()` and `agent.send_streaming()` (`AgentEvent`:
    text/thinking deltas, structured `ToolAction`s, custom tool
    dispatches, `Finished`, Evergreen `Unknown`), `CancelHandle`, turn
    timeouts, graceful `shutdown()` with SIGTERM/SIGKILL escalation.
  - Custom tools reuse the existing `#[tool]`/`FunctionRegistry`/
    `ToolService` machinery; harness built-ins gated via `Capabilities`
    (read-only by default).
  - Tool policies (`policy::allow/deny/confirm/allow_all/deny_all`)
    evaluated Rust-side before every dispatch, plus
    `on_pre_tool`/`on_post_tool` hooks; spawn-time safety gate refuses
    write-capable tools or MCP servers without a policy or hook (parity
    with the Python SDK).
  - MCP server config (`McpServer::stdio`/`McpServer::http`), structured
    output via `with_response_schema`, session persistence/resume via
    `with_save_dir` + `with_conversation_id`.
  - Proto-JSON protocol types under `antigravity::protocol` with
    Evergreen unknown-variant preservation throughout.
  - New `WireEvent` variants `HarnessSpawn`, `WsSend`, `WsReceive`,
    `HarnessStderr` (LOUD_WIRE and wire inspectors cover harness
    sessions).
  - Client-side triggers: `AgentBuilder::add_trigger(TriggerConfig)`
    spawns a per-trigger timer task that injects an `automated_trigger`
    message every interval, delivered only while the agent is idle
    (firings that come due mid-turn are deferred until the turn ends;
    missed intervals collapse into a single delivery). Tasks stop cleanly
    on `shutdown()`/drop; zero intervals are rejected at `spawn()`.
  - Subagent registration: `AgentBuilder::add_subagent(Subagent)` sends
    static `custom_subagents` in the conversation init (name, description,
    appended-style system instructions, per-subagent `Capabilities`,
    custom tools referenced by name). `spawn()` validates that referenced
    custom tools are registered on the parent agent and that subagent
    names are unique; nested subagents are force-disabled (harness
    limitation, reference-SDK parity).

- **Antigravity: workspace announcement.** `add_workspace(..)` roots are now
  announced to the model automatically — `spawn()` appends a concise,
  delimited note listing the configured workspace root(s) to the effective
  system instructions (the string passed to `with_system_instructions` is
  never mutated), and appends the same note to every subagent's instructions.
  Agents no longer guess workspace paths. Opt out with
  `AgentBuilder::with_workspace_announcement(false)`. The wire protocol has no
  native announcement field, so this is prompt-level grounding.
- **Antigravity: `ToolAction::subagent_name()`** and a typed
  `ActionInvokeSubagent::name` field (Evergreen; `None` on harness 0.1.5,
  which emits an empty `invokeSubagent` action).
- **Antigravity: `ToolDecision`** (`Allowed` / `Denied { reason }`) and
  **`ErrorSeverity`** (`Transient` / `Severe`) public enums.

#### New examples

- `examples/webhooks_and_background.rs` — webhook resource lifecycle +
  per-request webhook routing (runs without an API key, printing request
  shapes).
- `examples/retrieval_grounding.rs` — retrieval tool over Vertex AI
  Search, RAG store, and Exa.ai backends.
- `examples/antigravity_agent.rs` — Antigravity harness walkthrough
  (requires `--features antigravity`).
- `examples/real_world/repo_auditor/` — end-to-end Antigravity bridge
  application: repo workspace, read-only built-ins, custom `#[tool]`
  severity classifier, subagents, structured report output.

### Changed

#### Breaking — response model (`outputs` → `steps`)

- `InteractionResponse.outputs: Vec<Content>` replaced by
  `steps: Vec<Step>`. `Step` is a new `#[non_exhaustive]` tagged union
  (`user_input`, `model_output`, `thought`, `function_call`,
  `function_result`, `code_execution_call/result`,
  `url_context_call/result`, `google_search_call/result`,
  `mcp_server_tool_call/result`, `file_search_call/result`,
  `google_maps_call/result`, plus the standard `Unknown { step_type,
  data }` variant). Tool call/result and thought steps carry opaque
  `signature` fields that must be replayed unchanged.
- Convenience accessors (`as_text()`, `all_text()`, `function_calls()`,
  `images()`, `audios()`, `code_execution_*()`, `google_search_*()`,
  `url_context_*()`, `file_search_results()`, `google_maps_results()`,
  `thought_signatures()`, annotations helpers) are reimplemented over
  steps and keep working.
- `Content` slimmed to the spec content union: `Text`, `Image`, `Audio`,
  `Video`, `Document`, `Unknown`. All tool/thought `Content` variants were
  removed (they are `Step`s now).
- `Annotation` is now a typed citation union: `UrlCitation`,
  `FileCitation`, `PlaceCitation` (with `ReviewSnippet`s), plus `Unknown`.
  Byte-offset `extract_span()` is preserved.
- `InteractionResponse` serializes uniformly in snake_case (the previous
  `camelCase` rename was wrong for this API).
- `content_summary()`/`ContentSummary` replaced by
  `step_summary()`/`StepSummary`.
- `FunctionCallInfo.id` is now `&str` (required by the spec);
  `FunctionResultInfo.result` is a typed `FunctionResultPayload`
  (string | JSON | content blocks union).

#### Breaking — SSE lifecycle

- Wire events migrated from `interaction.start` /
  `content.start|delta|stop` / `interaction.complete` to
  `interaction.created`, `interaction.status_update`, `step.start`,
  `step.delta`, `step.stop`, `interaction.completed`, and `error`.
- `StreamChunk` variants renamed/reshaped: `Created { interaction }`,
  `StepStart { index, step }`, `StepDelta { index, delta: StepDelta }`,
  `StepStop { index, usage, step_usage }` (per-step usage), and
  `Completed(InteractionResponse)`. Stream termination keys on
  `interaction.completed` / `error`.
- New `StepDelta` payload union: `text`, `image`, `audio` (with
  `rate`/`sample_rate`/`channels`), `video`, `document`,
  `thought_summary`, `thought_signature`, `text_annotation_delta`,
  `arguments_delta` (function-call arguments now stream incrementally),
  built-in tool call/result deltas, `function_result`, and `Unknown`.
- The HTTP layer accumulates `step.start`/`step.delta`/`step.stop` into
  the final `Completed` response (including parsing streamed
  `arguments_delta` fragments into `FunctionCall.arguments`), so
  `response.function_calls()` and `as_text()` work after streaming.
  Lifecycle `metadata.total_usage` is folded into the completed response
  when the payload omits usage.
- `AutoFunctionStreamChunk::Delta` now carries `StepDelta` (exposing
  `arguments_delta` in the auto-function stream); the auto-function
  streaming loop and `last_event_id` resume work over the new events.

#### Breaking — input model & requests

- `Turn` and `InteractionInput::Turns` removed (deprecated in the spec).
  Conversation history is represented as steps:
  `InteractionInput::Steps(Vec<Step>)`, `with_history(Vec<Step>)`,
  `Step::user_text()` / `Step::model_text()` /
  `InteractionResponse::output_steps()`. `ConversationBuilder` keeps its
  fluent `.user()`/`.model()`/`.turn()` API but produces steps.
- `system_instruction` is a plain `Option<String>` per the spec.
- `generation_config.tool_choice` is a typed union `ToolChoice`: a
  lowercase mode string (`auto|any|none|validated`) or
  `{"allowed_tools": {"mode", "tools"}}`. The crate's previous top-level
  `generation_config.allowed_tools: Vec<String>` was removed;
  `with_allowed_tools()` now produces the object form. New
  `with_tool_choice()` escape hatch.
- `interactions_api` helper constructors renamed `*_content` → `*_step`
  and return `Step`.

#### Breaking — typed `response_format`

- `InteractionRequest.response_format` is now a typed
  `Option<ResponseFormatSpec>` (single object or list) instead of raw
  `serde_json::Value`. `ResponseFormat` is a tagged union:
  `Text{mime_type, schema}`, `Audio{mime_type, delivery, sample_rate,
  bit_rate}`, `Image{mime_type, delivery, aspect_ratio, image_size}`,
  `Video{delivery, gcs_uri, aspect_ratio, duration}`, plus Unknown;
  `ResponseDelivery` is `inline|uri` + Unknown.
- `with_response_format()` now takes `impl Into<ResponseFormat>` — raw
  `serde_json::Value` schemas keep compiling and convert to
  `ResponseFormat::Text{mime_type: "application/json", schema}` (the
  typed equivalent of the old wire shape). New
  `with_response_formats(Vec<ResponseFormat>)` for the list form.
- Raw schema dicts received on the wire (no recognized `type` tag)
  roundtrip losslessly through `ResponseFormat::Unknown`.
- `ImageConfig` with `ImageAspectRatio` and `ImageSize` enums for image
  generation configuration.

#### Breaking — multi-speaker TTS (`speech_config` list)

- `GenerationConfig.speech_config` is now `Option<Vec<SpeechConfig>>` to
  match the spec's list wire format (multi-speaker TTS). The legacy
  single-object wire form is still accepted on deserialize.
- Builder: `with_speech_config(single)` keeps working (sends a one-entry
  list); new `with_speech_configs(Vec<SpeechConfig>)` and
  `add_speech_config()` for multi-speaker dialogue.

#### Breaking — dependencies

- `reqwest` upgraded 0.12 → 0.13 (`GenaiError::Http(reqwest::Error)` and
  `ResumableUpload` methods expose reqwest types publicly). MSRV is
  unchanged (Rust 1.88).
  - TLS **trust roots** changed with the upgrade: reqwest 0.13 removed
    the bundled-Mozilla-roots feature (`rustls-tls-webpki-roots`); its
    `rustls` feature now verifies certificates against the **OS trust
    store** via `rustls-platform-verifier`. The TLS stack is still rustls
    (not native TLS). If your deployment relies on bundled roots (e.g.
    minimal containers without a CA bundle), install a CA bundle or open
    an issue — restoring bundled roots would require a preconfigured
    rustls client.
- Minor dependency bumps: tokio 1.48 → 1.52, proptest 1.9 → 1.11,
  utoipa 5.4 → 5.5.

#### Antigravity

- **Antigravity (breaking): `AgentEvent::ToolAction`** is now a struct variant
  `ToolAction { action, decision, trajectory_id }`. `decision` distinguishes
  executed actions from policy/hook-denied ones (previously indistinguishable
  in the stream), and `trajectory_id` tells parent and subagent actions apart.
- **Antigravity (breaking): `AgentEvent::Error(String)`** is now
  `Error { message, severity }` so consumers can ignore transient
  harness-internal noise (retried internally; the turn continues) and react
  only to `Severe` errors. Turn-ending failures still surface as
  `AntigravityError::Turn`.
- **Antigravity (behavior): `ToolOutcome.result`** passed to post-tool hooks
  is now the inner tool result, not the raw `{"result": ...}` wire envelope
  (a scalar arrives as its string form; an object is passed through
  serialized).

#### Other changes

- `Tool::GoogleSearch` is now a struct variant with optional
  `search_types` field (was unit variant).
- New default-on feature `wire-color` gates the `colored`/`colored_json`
  dependencies; build with `default-features = false` for plain-text wire
  output.
- Wire debug request ids are now per-`Client` (previously a
  process-global counter).
- Proptest roundtrip comparisons use `serde_json::Value` for HashMap key
  order independence.

### Fixed

#### Spec/implementation disagreements (verified live)

- **BREAKING**: `Step::FunctionCall` and `Step::FunctionResult` gained a
  `signature: Option<String>` field (verified live 2026-07: the API
  returns `signature` on `function_call` steps and rejects stateless
  replay of history that omits it; the generated SDK bindings do not list
  it on `function_call`). Existing constructors set it to `None`; it is
  preserved on deserialize/serialize roundtrip.
- Response modalities are now sent lowercase: `with_image_output()` /
  `with_audio_output()` send `"image"` / `"audio"` (previously `"IMAGE"` /
  `"AUDIO"`, which the API rejects — supported values are `text`,
  `image`, `audio`, `video`, `document`; verified live 2026-07).
  `with_response_modalities()` normalizes provided values to lowercase.
- `FunctionCallingMode` now serializes lowercase (`"auto"`, ...); legacy
  UPPERCASE still accepted on deserialize.
- `CodeExecutionLanguage` now serializes `"python"` (was `"PYTHON"`);
  legacy accepted on deserialize.
- `Tool::ComputerUse` `excluded_predefined_functions` now serializes
  snake_case (was `excludedPredefinedFunctions`); legacy alias accepted
  on deserialize.
- MCP server `allowed_tools` is now `[{mode, tools}]`
  (`Vec<AllowedTools>`) per the spec (was `[String]`).
- `InteractionResponse` no longer serializes camelCase field names.
- Live `google_search_result` items can carry only `search_suggestions`;
  the non-optional `title`/`url` fields previously synthesized empty
  strings on stateless replay and are now skip-serialized when empty, so
  captured live responses roundtrip byte-identically.
- `NetworkConfig::Allowlist` now preserves sibling keys next to
  `allowlist` on roundtrip (Evergreen) via a struct variant with an
  `extra` map — construct with the new `NetworkConfig::allowlist(entries)`
  helper. Previously an unmodeled sibling field (e.g. a future
  `default_policy`) was silently dropped on deserialize.

#### Streaming & wire

- Streaming image, video, and document deltas now accumulate into a
  single content block (matching audio behavior) instead of producing one
  block per chunk.
- Streaming responses whose terminal event omits usage now fall back to
  the cumulative usage from the last `step.stop` event, so
  `response.usage()` stays populated.
- Error response bodies are now visible in wire output
  (`WireEvent::ErrorBody`); previously `LOUD_WIRE=1` showed only the
  status line for failed requests.
- Wire output no longer panics when truncating multi-byte UTF-8 content
  (`data`/`signature` fields and non-JSON bodies truncate on character
  boundaries).
- Request bodies are no longer serialized for wire debugging when it is
  disabled (previously every request paid the serialization cost even
  without `LOUD_WIRE`).

#### Evergreen roundtrip

- `RemoteEnvironment`, `EnvironmentSource`, and `AllowlistEntry` now
  preserve unknown wire fields via an Evergreen `extra` field
  (roundtrip-safe; breaking for exhaustive struct literals — use
  `..Default::default()`).

#### Antigravity

- The stdio handshake now rejects reply frames whose declared length
  exceeds 4 MiB before allocating (a non-harness binary can no longer
  trigger an unbounded allocation; it fails with `HandshakeFailed`
  instead).
- A turn that exceeds `with_turn_timeout` is now halted on the harness
  and its remaining events drained before `AntigravityError::Timeout` is
  returned; previously the abandoned turn kept running and its buffered
  events (including its terminal state) desynced the next
  `chat`/`send_streaming`, which could return the previous turn's output.
- Turns started by `add_trigger` deliveries are now halted and their
  events discarded by the next `chat`/`send_streaming` before it sends
  its input; previously the unconsumed trigger turn's buffered events
  were misattributed to the user's turn, shifting every later response by
  one turn. Trigger-turn output is not surfaced (documented in
  `docs/ANTIGRAVITY.md`).
- A cancelled *subagent* trajectory no longer fails the parent's whole
  turn (mirroring the existing subagent-idle handling); subagent failures
  surface through their step errors. Main-trajectory cancellation still
  fails the turn with `AntigravityError::Turn`.
- The harness stderr drain no longer stops permanently on a non-UTF-8
  line (it now reads bytes and replaces invalid sequences lossily); a
  stopped drain could let the stderr pipe fill and deadlock the child —
  the drain's whole purpose at the wrong-binary trust boundary.
- Closed a race between trigger delivery and turn begin: a trigger that
  had passed its idle check could deliver its message *after*
  `chat`/`send_streaming` marked the agent busy and consumed the
  trigger-fired flag, injecting an `automated_trigger` into the user's
  turn window with nobody left to drain the resulting harness turn. The
  fire decision and turn begin are now mutually exclusive (shared lock +
  idle re-check under it).
- Unrecognized harness tool confirmations now **fail closed** — a
  confirmation whose action fields this client does not recognize (e.g. a
  builtin newer than the pinned harness) is approved only when a policy
  rule (`allow_all()` or an exact rule naming the unknown wire field) or
  the `on_pre_tool` hook allows it, with a `warn!` either way. Previously
  any unmappable confirmation was auto-approved, bypassing deny policies.
  Genuine pre-request notifications (steps with no action payload) remain
  auto-approved — the concrete call still gets its own policy check.

### Removed

- **BREAKING**: `response_mime_type` removed outright — the
  `InteractionRequest` field and
  `InteractionBuilder::with_response_mime_type()` (previously
  `#[deprecated]`). Live verification (2026-07) showed the API rejects
  every request carrying the field: alone it returns 400 "responseFormat
  must be set when responseMimeType is set", the same 400 is returned
  even when `response_format` IS set (raw-schema or typed form), and
  camelCase `responseMimeType` gets "Unknown parameter" — the field is
  dead server-side, so no working code can be using it. Use
  `with_response_format()` alone; passing a JSON schema implies JSON
  output.
- **BREAKING**: `Turn` and `InteractionInput::Turns` (deprecated in the
  spec) — history is steps now; see the migration guide.
- **BREAKING**: `ComputerUseCall`/`ComputerUseResult` content dropped
  entirely (computer use surfaces as `function_call` steps).
- **BREAKING**: `generation_config.top_k` removed (dropped from the
  spec).
- **BREAKING**: `UsageMetadata.total_reasoning_tokens` removed (not in
  the spec; use `total_thought_tokens`) and
  `InteractionResponse::reasoning_tokens()` removed in favor of
  `thought_tokens()`.
- Speculative `grounding_metadata`/`url_context_metadata` response fields
  (and `GroundingMetadata`, `GroundingChunk`, `WebSource`,
  `UrlContextMetadata`, `UrlMetadataEntry`, `UrlRetrievalStatus` types) —
  not part of the Interactions API; grounding data lives in steps and
  typed annotations.
- `with_computer_use()` and `with_computer_use_excluding()` — use
  `add_tool(ComputerUseConfig::new())`.
- `add_mcp_server()` — use `add_tool(McpServerConfig::new(name, url))`.
- `with_file_search()` and `with_file_search_config()` — use
  `add_tool(FileSearchConfig::new(stores))`.

### Security

- Wire-inspection redaction now covers webhook signing secrets
  (`new_signing_secret` on create, `secret` on rotate — one-time values),
  and `TracingForwarder` applies the same redaction/truncation as
  `LoudWirePrinter` to request/response/error bodies and SSE frames
  (previously it forwarded them raw to `tracing`, bypassing redaction
  entirely — including the Exa/Parallel retrieval `api_key` fields).
- The Antigravity WS-send inspection copy now redacts every value inside
  `env`, `headers`, and `httpHeaders` maps (MCP stdio env secrets,
  `Authorization` bearer tokens, model-endpoint headers), not just
  `apiKey`. The harness/API still receives the originals; only inspector
  copies are redacted.
- `LOUD_WIRE` output now fully redacts `api_key` fields (e.g.
  Exa/Parallel retrieval configs) instead of printing them.
- Wire-inspection redaction now recurses into `data`/`signature` keys
  whose values are objects or arrays (e.g. Evergreen `Unknown` payloads
  preserved under a `data` key), so secrets nested inside them are
  redacted; previously such subtrees were skipped entirely by both
  `LoudWirePrinter` and `TracingForwarder`.
- `Debug` for `Webhook` and `RotateSigningSecretResponse` now redacts
  `new_signing_secret` / `secret` (matching the client's `api_key`
  redaction precedent).

## [0.7.2] - 2026-01-17

### Changed

- **BREAKING**: `AutoFunctionStreamChunk::ExecutingFunctions` changed from tuple variant to struct variant with `pending_calls` field:
  ```rust
  // Before
  AutoFunctionStreamChunk::ExecutingFunctions(response) => {
      // response.function_calls() was often empty in streaming mode
  }

  // After
  AutoFunctionStreamChunk::ExecutingFunctions { response, pending_calls } => {
      // pending_calls always contains the validated function calls
      for call in pending_calls {
          println!("Executing: {}({})", call.name, call.args);
      }
  }
  ```

### Added

- `PendingFunctionCall` type: Represents a function call about to be executed, with `name`, `call_id`, and `args` fields. Available in `ExecutingFunctions` events before function execution begins.

### Fixed

- `ExecutingFunctions` chunk now provides function call information via `pending_calls` field. Previously, `response.function_calls()` was often empty in streaming mode because function calls arrived via Delta chunks rather than the Complete response.

## [0.7.1] - 2026-01-17

### Changed

- **BREAKING**: Removed typestate pattern from `InteractionBuilder`. The builder no longer uses `FirstTurn`, `Chained`, or `StoreDisabled` marker types. Invalid combinations (e.g., `store=false` with `with_previous_interaction()`) are now caught at runtime in `build()` with descriptive error messages instead of compile-time type errors. This enables conditional chaining patterns that were previously impossible:
  ```rust
  // Now possible - conditional chaining
  let mut builder = client.interaction()
      .with_model("gemini-3.6-flash")
      .with_text("Hello");

  if let Some(prev_id) = previous_interaction_id {
      builder = builder.with_previous_interaction(prev_id);
  }

  let response = builder.create().await?;
  ```
- **BREAKING**: `FunctionExecutionResult::new()` now requires `args` parameter (position 3, before `result`)
- `FunctionExecutionResult` now includes `args` field for complete execution context - enables logging function calls with their arguments after execution completes

### Fixed

- Auto-function execution (`create_with_auto_functions()` and `create_stream_with_auto_functions()`) now reports accurate accumulated token usage across all API calls. Previously, the final response could show 0 input tokens because the API only reports input tokens on the first call.

### Migration

If you relied on compile-time enforcement of builder constraints, you'll now get runtime errors from `build()` instead:
- `with_store_disabled()` + `with_previous_interaction()` → `GenaiError::InvalidInput("Chained interactions require storage...")`
- `with_store_disabled()` + `with_background(true)` → `GenaiError::InvalidInput("Background execution requires storage...")`
- `with_store_disabled()` + `create_with_auto_functions()` → `GenaiError::InvalidInput("create_with_auto_functions() requires storage...")`

## [0.7.0] - 2026-01-15

### Added

- New `docs/BUILDER_API.md` documenting the InteractionBuilder API, method naming conventions, and validation errors
- `build()` now validates that `with_agent_config()` requires `with_agent()` - returns error instead of silently ignoring
- **New Content API**: Static constructors on `Content` for all content types:
  - `Content::text()`, `Content::image_data()`, `Content::image_uri()`, `Content::audio_data()`, `Content::audio_uri()`, `Content::video_data()`, `Content::video_uri()`, `Content::document_data()`, `Content::document_uri()`
  - `Content::from_file(&FileMetadata)` - create content from Files API upload
  - `Content::from_uri_and_mime(uri, mime)` - generic URI content
  - Resolution variants: `Content::image_data_with_resolution()`, etc.
- **Content builder methods**:
  - `Content::with_resolution(Resolution)` - chain resolution setting
  - `Content::with_result(value)` - convert `FunctionCall` to `FunctionResult`
  - `Content::with_result_error(value)` - convert `FunctionCall` to error `FunctionResult`

### Changed

- **BREAKING**: Renamed `InteractionContent` → `Content` for ergonomics. Update imports: `use genai_rs::Content;`
- **BREAKING**: Renamed `Content::text()` getter → `Content::as_text()` to follow Rust getter conventions
- **BREAKING**: Renamed `InteractionResponse::text()` getter → `InteractionResponse::as_text()` for consistency
- **BREAKING**: Renamed `TurnContent::parts()` → `TurnContent::as_parts()` for consistency with `as_text()`
- **BREAKING**: Renamed `with_turns()` to `with_history()`. The new name better reflects that this sets conversation history, and now composes correctly with `with_text()`: calling both produces `[...history, Turn::user(current_message)]` regardless of call order.
- **BREAKING**: `with_text()` now sets `current_message` instead of replacing `input`. This fixes issue #359 where `with_turns().with_text()` silently overwrote the history.
- **BREAKING**: `with_system_instruction()` is now available on ALL builder states (FirstTurn, Chained, StoreDisabled), not just FirstTurn. The API does NOT inherit system instructions via `previousInteractionId`, so users should set it explicitly on each turn if needed. For `create_with_auto_functions()`, the SDK automatically includes system_instruction on all internal turns.
- Method naming consistency overhaul:
  - `with_function()` → `add_function()` (accumulates)
  - `with_functions()` → `add_functions()` (accumulates)
- `build()` now returns an error if content input is combined with history (incompatible modes), with a helpful error message explaining the workaround

### Removed

- **BREAKING**: Removed all `add_*` multimodal methods from `InteractionBuilder`:
  - `add_image_data()`, `add_image_uri()`, `add_image_file()`, `add_image_bytes()`
  - `add_audio_data()`, `add_audio_uri()`, `add_audio_file()`, `add_audio_bytes()`
  - `add_video_data()`, `add_video_uri()`, `add_video_file()`, `add_video_bytes()`
  - `add_document_data()`, `add_document_uri()`, `add_document_file()`, `add_document_bytes()`
  - `add_file()`, `add_file_uri()` (Files API methods)
  - All `*_with_resolution()` variants

  **Migration**: Use `with_content(vec![Content::*(...)])` instead. See migration guide below.

- **BREAKING**: Removed all `*_content()` free functions from `interactions_api`:
  - `text_content()`, `image_data_content()`, `image_uri_content()`, `audio_data_content()`, `audio_uri_content()`
  - `video_data_content()`, `video_uri_content()`, `document_data_content()`, `document_uri_content()`
  - `function_call_content()`, `function_result_content()`, `file_data_content()`, `file_uri_content()`

  **Migration**: Use `Content::*()` static constructors instead (e.g., `text_content("hi")` → `Content::text("hi")`).

  **Note**: Model output constructors for testing remain in `interactions_api`: `code_execution_*`, `google_search_*`, `url_context_*`, `file_search_*`.

### Fixed

- `AgentConfig` (DeepResearchConfig) now serializes `thinking_summaries` with snake_case per API spec, not camelCase `thinkingSummaries`
- **BREAKING**: `document_from_file()` now correctly rejects non-PDF files. The Gemini API only supports `application/pdf` for document content type. For text-based files (CSV, TXT, JSON, etc.), read the file and send as `Content::text()` instead.
- `FileSearchResult` now serializes `call_id` with snake_case per API spec, not camelCase `callId`
- `CodeExecutionCall` now serializes with nested `arguments` object containing `language` and `code` per API spec
- `GoogleSearchResultItem.rendered_content` now uses snake_case per API spec

### Changed

- **BREAKING**: Removed `CodeExecutionOutcome` enum - actual wire format uses `is_error: bool` and `result: String` fields directly, not `outcome`/`output` as documented
- `CodeExecutionResultInfo` now has `is_error: bool` and `result: &str` fields instead of `outcome: CodeExecutionOutcome` and `output: &str`
- `Content::CodeExecutionResult` variant now uses `is_error: bool, result: String` instead of `outcome: CodeExecutionOutcome, output: String`
- `InteractionResponse::successful_code_output()` now checks `!is_error` instead of `outcome.is_success()`
- **BREAKING**: `FunctionCallInfo` and `OwnedFunctionCallInfo` no longer have `thought_signature` field - API never sends this on function calls
- Renamed `Content::new_function_call_with_signature()` to `Content::function_call_with_id()` and removed `thought_signature` parameter
- Renamed `function_call_content_with_signature()` to `function_call_content_with_id()` and removed `thought_signature` parameter

### Removed

- **BREAKING**: `CodeExecutionOutcome` enum - the actual API wire format doesn't use this enum
- **BREAKING**: `thought_signature` field from `Content::FunctionCall` variant - API does not send this field on function calls (thought signatures appear only on `Thought` content blocks)

### Migration Guide

**Type rename - `InteractionContent` → `Content`:**
```rust
// Before (0.6.0)
use genai_rs::InteractionContent;
let content = InteractionContent::new_text("Hello");

// After (0.7.0)
use genai_rs::Content;
let content = Content::text("Hello");  // Static constructor
```

**Multimodal content - `add_*()` methods removed:**
```rust
// Before (0.6.0)
let response = client.interaction()
    .with_model("gemini-3.6-flash")
    .with_text("Describe this image")
    .add_image_file("photo.jpg").await?
    .create()
    .await?;

// After (0.7.0) - Option A: Content constructors
let response = client.interaction()
    .with_model("gemini-3.6-flash")
    .with_content(vec![
        Content::text("Describe this image"),
        Content::image_data(base64_data, "image/png"),
    ])
    .create()
    .await?;

// After (0.7.0) - Option B: File helpers
use genai_rs::image_from_file;
let image = image_from_file("photo.jpg").await?;
let response = client.interaction()
    .with_model("gemini-3.6-flash")
    .with_content(vec![
        Content::text("Describe this image"),
        image,
    ])
    .create()
    .await?;
```

**Files API - `add_file()` removed:**
```rust
// Before (0.6.0)
let file = client.upload_file("video.mp4").await?;
let response = client.interaction()
    .with_model("gemini-3.6-flash")
    .add_file(&file)
    .with_text("Describe this video")
    .create()
    .await?;

// After (0.7.0)
let file = client.upload_file("video.mp4").await?;
let response = client.interaction()
    .with_model("gemini-3.6-flash")
    .with_content(vec![
        Content::text("Describe this video"),
        Content::from_file(&file),
    ])
    .create()
    .await?;
```

**Resolution control:**
```rust
// Before (0.6.0)
.add_image_data_with_resolution(base64, "image/png", Resolution::High)

// After (0.7.0) - Constructor
Content::image_data_with_resolution(base64, "image/png", Resolution::High)

// After (0.7.0) - Builder chain
Content::image_data(base64, "image/png").with_resolution(Resolution::High)
```

**`with_turns()` renamed to `with_history()` and composes with `with_text()`:**
```rust
// Before (0.6.0)
// with_turns().with_text() silently overwrote history - bug!
let response = client.interaction()
    .with_model("gemini-3.6-flash")
    .with_turns(history)
    .create()
    .await?;

// After (0.7.0)
// Renamed to with_history(), and now composes correctly with with_text()
let response = client.interaction()
    .with_model("gemini-3.6-flash")
    .with_history(history)
    .with_text("Current message")  // Appended as final user turn
    .create()
    .await?;
// Produces: [...history, Turn::user("Current message")]
// Order doesn't matter - with_text().with_history() produces same result
```

**`CodeExecutionOutcome` removal:**
```rust
// Before
if result.outcome.is_success() {
    println!("Output: {}", result.output);
}

// After
if !result.is_error {
    println!("Output: {}", result.result);
}
```

**`thought_signature` removal from FunctionCall:**
```rust
// Before
let call = InteractionContent::new_function_call_with_signature(
    Some("call_123"),
    "get_weather",
    json!({"location": "SF"}),
    Some("signature".to_string())  // No longer needed - API doesn't send this
);
if let InteractionContent::FunctionCall { thought_signature, .. } = content {
    // thought_signature was always None
}

// After
let call = InteractionContent::new_function_call_with_id(
    Some("call_123"),
    "get_weather",
    json!({"location": "SF"})
);
// Note: Thought signatures appear on Thought content blocks, not function calls.
// Use response.thought_signatures() to iterate over them.
```

## [0.6.0] - 2025-01-11

### Added

- `InteractionBuilder::build()`: Build requests without executing, enabling retry patterns and request serialization
- `Client::execute()` and `Client::execute_stream()`: Execute pre-built `InteractionRequest` objects
- `GenaiError::is_retryable()`: Helper to identify transient errors (429, 5xx, timeouts) for retry logic
- `GenaiError::Api::retry_after`: Extracts `Retry-After` header from 429 rate limit responses (seconds or HTTP date format)
- `GenaiError::retry_after()`: Accessor method for the retry delay (consistent with `is_retryable()` pattern)
- `Deserialize` derive on `InteractionRequest`: Enables loading requests from JSON/config files
- `#[tracing::instrument]` on `execute()` and `execute_stream()`: Automatic span creation with model/agent context
- `docs/RETRY_PATTERNS.md`: Documents retry philosophy and recommended patterns using `backon` crate
- `examples/retry_with_backoff.rs`: Demonstrates retry patterns using the `backon` crate

### Changed

- **BREAKING**: Renamed `CreateInteractionRequest` to `InteractionRequest` for consistency
- **BREAKING**: Migrated from `log` crate to `tracing` crate for structured logging and spans
- Updated `docs/LOGGING_STRATEGY.md` to document tracing integration and instrumentation patterns

### Removed

- **BREAKING**: `Client::create_interaction()` - use `Client::execute()` instead
- **BREAKING**: `Client::create_interaction_stream()` - use `Client::execute_stream()` instead

### Migration Guide

**`create_interaction()` → `execute()`:**
```rust
// Before
let response = client.create_interaction(request).await?;
let stream = client.create_interaction_stream(request);

// After
let response = client.execute(request).await?;
let stream = client.execute_stream(request);
```

**`CreateInteractionRequest` → `InteractionRequest`:**
```rust
// Before
use genai_rs::CreateInteractionRequest;

// After
use genai_rs::InteractionRequest;
```

**`log` → `tracing`:**
If you were filtering logs with `RUST_LOG=genai_rs=debug`, this continues to work.
For tracing subscribers, use `tracing_subscriber` instead of `env_logger`:

```rust
// Before (with env_logger)
env_logger::init();

// After (with tracing-subscriber)
tracing_subscriber::fmt::init();
```

## [0.5.3] - 2026-01-10

### Fixed

- Release workflow: Use `--tests` to exclude doctests from `--include-ignored` run (v0.5.2 release workflow failed because `--include-ignored` compiles `ignore` doctest snippets)

## [0.5.2] - 2026-01-10

### Fixed

- Doctest compilation failures in `INTERACTIONS_API_FEEDBACK.md` (missing `ignore` annotation)
- Updated `thought_content()` docstring to clarify it's for testing only (API rejects thought blocks in user input)

### Changed

- `INTERACTIONS_API_FEEDBACK.md`: Downgraded thought signature issue from P0 to P2, clarified that signatures ARE present on `Thought` outputs (not `FunctionCall`), and documented that API rejects thought blocks in user input

## [0.5.1] - 2026-01-10

### Fixed

- Streaming tests no longer assert `event_id` presence (optional per API spec)
- `test_get_interaction_stream` handles API not replaying completed interactions
- Updated `STREAMING_API.md` with notes about optional `event_id` field

## [0.5.0] - 2026-01-10

### Added

- `docs/INTERACTIONS_API_FEEDBACK.md`: Comprehensive feedback report for Google Gemini API team documenting 9 issues discovered while building genai-rs
- Thought signature test coverage across 7 configurations (stateful, stateless, parallel, sequential, ThinkingLevel::High, FunctionCallingMode::Any, streaming)
- `test_speech_config_nested_format_fails_flat_succeeds`: Test proving only flat SpeechConfig format works (nested format returns 400)

### Changed

- `docs/ENUM_WIRE_FORMATS.md`: Updated SpeechConfig section - nested format fails with 400 error
- `docs/MULTI_TURN_FUNCTION_CALLING.md`: Added thought signature matrix with verified test links

### BREAKING CHANGES

#### Enum Unknown Variant Upgrade (#329)

Three enums upgraded from `#[serde(other)]` fallback to full Evergreen Unknown variant pattern. This enables logging and debugging of unrecognized API values.

**Affected types:**
- `UrlRetrievalStatus`: Variant renames for consistency
- `CodeExecutionOutcome`: Full Unknown pattern with data preservation
- `CodeExecutionLanguage`: Full Unknown pattern, `Unspecified` variant removed

**UrlRetrievalStatus variant renames:**
| Before | After |
|--------|-------|
| `UrlRetrievalStatusUnspecified` | `Unspecified` |
| `UrlRetrievalStatusSuccess` | `Success` |
| `UrlRetrievalStatusUnsafe` | `Unsafe` |
| `UrlRetrievalStatusError` | `Error` |

**CodeExecutionLanguage changes:**
- `Unspecified` variant removed (API only returns known languages)
- `Unknown { language_type, data }` variant added for forward compatibility

**Copy trait removed** from all three types (Unknown variants contain `serde_json::Value`).

**Migration guide:**

```rust
// UrlRetrievalStatus: Update variant names
// Before:
match status {
    UrlRetrievalStatus::UrlRetrievalStatusSuccess => { ... }
    UrlRetrievalStatus::UrlRetrievalStatusError => { ... }
    _ => { ... }
}

// After:
match status {
    UrlRetrievalStatus::Success => { ... }
    UrlRetrievalStatus::Error => { ... }
    UrlRetrievalStatus::Unknown { status_type, .. } => {
        log::warn!("Unknown status: {}", status_type);
    }
    _ => { ... }
}

// CodeExecutionLanguage: Handle Unknown instead of Unspecified
// Before:
match language {
    CodeExecutionLanguage::Python => { ... }
    CodeExecutionLanguage::Unspecified => { ... }
}

// After:
match language {
    CodeExecutionLanguage::Python => { ... }
    CodeExecutionLanguage::Unknown { language_type, .. } => {
        log::warn!("Unknown language: {}", language_type);
    }
    _ => { ... }
}

// Copy trait removal: Use .clone() where needed
// Before:
let outcome = *some_outcome_ref;

// After:
let outcome = some_outcome_ref.clone();
```

#### InteractionContent Field Type Audit (#318)

Wire format alignment fixes for `InteractionContent` variants. These changes fix critical mismatches where real API data was silently falling back to `Unknown` variants.

- **`InteractionContent::Thought`**: Field `text` renamed to `signature`
  - Thoughts contain cryptographic signatures for verification, not human-readable reasoning
  - Use `response.thought_signatures()` to iterate over signatures
  - Use `response.has_thoughts()` to check for thought presence

- **`InteractionContent::UrlContextCall`**: Field `url` split into `id` + `urls`
  - `id: String` - Call identifier for matching results
  - `urls: Vec<String>` - List of URLs requested
  - Use `response.url_context_call_id()` and `response.url_context_call_urls()`

- **`InteractionContent::UrlContextResult`**: Fields `url`/`content` replaced with `call_id` + `result`
  - `call_id: String` - Matches the corresponding call
  - `result: Vec<UrlContextResultItem>` - Results for each URL
  - New `UrlContextResultItem` type with `url`, `status` fields and `is_success()`/`is_error()`/`is_unsafe()` helpers

- **`InteractionResponse::thoughts()`**: Method removed
  - Was returning signatures but named incorrectly
  - Use `thought_signatures()` instead

- **`InteractionResponse::url_context_call()`**: Method renamed to `url_context_call_id()`
  - New `url_context_call_urls()` method returns the list of URLs

**Migration guide:**

```rust
// Before: Thought had text field
InteractionContent::Thought { text: Some(t) } => println!("{}", t);

// After: Thought has signature field (cryptographic, not readable)
InteractionContent::Thought { signature: Some(s) } => {
    // s is a cryptographic signature, not human-readable text
    println!("Has thought signature: {}", s.len() > 0);
}

// Before: UrlContextCall had single url
InteractionContent::UrlContextCall { url } => println!("{}", url);

// After: UrlContextCall has id + urls
InteractionContent::UrlContextCall { id, urls } => {
    println!("Call {}: {:?}", id, urls);
}

// Before: UrlContextResult had url/content
InteractionContent::UrlContextResult { url, content } => { ... }

// After: UrlContextResult has call_id + result array
InteractionContent::UrlContextResult { call_id, result } => {
    for item in result {
        if item.is_success() {
            println!("Fetched: {}", item.url);
        }
    }
}

// Before: Using thoughts() method
for thought in response.thoughts() { ... }

// After: Use thought_signatures()
for sig in response.thought_signatures() { ... }
```

### Added

- **`UrlContextResultItem` type** (#318): New struct for URL context result items
  - `url: String` - The URL that was fetched
  - `status: String` - Result status ("success", "error", "unsafe")
  - Helper methods: `is_success()`, `is_error()`, `is_unsafe()`

- **`UsageMetadata::total_thought_tokens` field** (#318): Token count for thinking/reasoning
  - Use `response.thought_tokens()` helper method

### Fixed

- **`CodeExecutionResult` outcome derivation** (#318): When `is_error` is `None`, outcome now correctly defaults to `Ok` instead of `Unspecified`

## [0.4.0] - 2026-01-08

### BREAKING CHANGES

#### Crate Renamed to `genai-rs`
- **`rust-genai` is now `genai-rs`** - Update your Cargo.toml dependencies
- **`rust-genai-macros` is now `genai-rs-macros`** - Update macro imports
- Change `use rust_genai::*` to `use genai_rs::*` in your code

#### MSRV Bumped to Rust 1.88
- **Minimum Supported Rust Version is now 1.88** (was 1.85)
- Required for Edition 2024 `let` chains feature
- Update your Rust toolchain: `rustup update`

#### Crate Consolidation (#302)
- **`genai-client` crate merged into `genai-rs`**
- Internal HTTP and type modules are now `pub(crate)` instead of separate crate
- Users only depend on `genai-rs` - no change to public API

### Added

#### Text-to-Speech Audio Output (#303)
- **New `with_audio_output()` method** - Generate speech from text
- **New `with_voice(name)` method** - Select voice (Kore, Puck, Aoede, etc.)
- **New `with_speech_config(SpeechConfig)` method** - Full voice/language/speaker control
- **New `SpeechConfig` type** with constructors:
  - `SpeechConfig::with_voice("Kore")`
  - `SpeechConfig::with_voice_and_language("Puck", "en-GB")`
- **New response helpers**: `first_audio()`, `audios()`, `has_audio()`
- **New `AudioInfo` type** with `bytes()`, `mime_type()`, `extension()` methods
- Use model `gemini-2.5-pro-preview-tts` for TTS

```rust
let response = client
    .interaction()
    .with_model("gemini-2.5-pro-preview-tts")
    .with_text("Hello, world!")
    .with_audio_output()
    .with_voice("Kore")
    .create()
    .await?;

if let Some(audio) = response.first_audio() {
    std::fs::write("speech.wav", audio.bytes()?)?;
}
```

#### New Built-in Tools
- **File Search tool (#299)** - Semantic document retrieval from vector stores
  - New `with_file_search(store_ids)` method
- **Computer Use tool (#298)** - Browser automation via Gemini
  - New `with_computer_use()` method (requires allowlisted API key)
- **MCP Server convenience (#295)** - Connect to Model Context Protocol servers
  - New `add_mcp_server(uri)` method

#### Explicit Multi-Turn Conversations (#296)
- **New `with_turns(Vec<Turn>)` method** - Provide full conversation history
- **New `Turn` type** with `user()` and `model()` constructors
- Alternative to `previous_interaction_id` for stateless deployments

#### Typed Agent Configuration (#293)
- **New `AgentConfig` type** for Deep Research and Dynamic agents
- **`DeepResearchConfig`** with `with_thinking_summaries()` builder
- **`DynamicConfig`** for dynamic agent interactions
- Use `with_agent_config(config.into())` on builder

#### Resolution Control for Media (#297)
- **New `with_resolution(MediaResolution)` method** on `ImageInput` and `VideoInput`
- Control processing resolution: `Low`, `Medium`, `High`, `Native`

### Infrastructure

#### CI/CD Workflows
- **Automated crates.io publishing** on version tags
- **Release Drafter** for automatic release notes from PR labels
- **MSRV Check** (Rust 1.88), **Cross-platform testing** (Linux, macOS, Windows)
- **Code coverage** with Codecov integration
- **Security audit** with cargo-audit

#### Comprehensive Documentation
- 12 new documentation guides in `docs/`
- New `TROUBLESHOOTING.md` for common issues

### Fixed

- **ThinkingSummaries wire format** (#272): Fixed serialization to use `THINKING_SUMMARIES_AUTO` and `THINKING_SUMMARIES_NONE` (API's actual wire format) instead of `auto`/`none` (what the documentation claims). This enables `agent_config` with `thinking_summaries` to work correctly with the Deep Research agent.
- **Clippy lints for Rust 1.92** - Use `is_multiple_of()` and collapsed `if let` chains

### BREAKING CHANGES

#### Timestamp Fields Use chrono::DateTime<Utc> (#273)
- **`FileMetadata.create_time`**: Changed from `Option<String>` to `Option<DateTime<Utc>>`
- **`FileMetadata.expiration_time`**: Changed from `Option<String>` to `Option<DateTime<Utc>>`
- **`InteractionResponse`**: Added `created: Option<DateTime<Utc>>` and `updated: Option<DateTime<Utc>>` fields
- **New dependency**: `chrono` crate with serde support
- Internal `loud_wire.rs` timestamp generation simplified to use chrono

**Migration guide:**
```rust
// Before (FileMetadata timestamps were strings):
if let Some(created) = file.create_time {
    println!("Created: {}", created);  // String
}

// After (timestamps are DateTime<Utc>):
use chrono::{DateTime, Utc};
if let Some(created) = file.create_time {
    println!("Created: {}", created.to_rfc3339());  // DateTime<Utc>
    // Or use chrono's formatting:
    println!("Created: {}", created.format("%Y-%m-%d %H:%M:%S"));
}

// InteractionResponse now has created/updated fields:
if let Some(created) = response.created {
    println!("Interaction created at: {}", created);
}
```

#### Streaming Returns StreamEvent Wrapper (#262)
- **`create_stream()`** now returns `Stream<Item = Result<StreamEvent, GenaiError>>` instead of `Stream<Item = Result<StreamChunk, GenaiError>>`
- **`create_stream_with_auto_functions()`** now returns `Stream<Item = Result<AutoFunctionStreamEvent, GenaiError>>` instead of `Stream<Item = Result<AutoFunctionStreamChunk, GenaiError>>`
- **New `StreamEvent` struct**: Wraps `StreamChunk` with `event_id` field for stream resume support
- **New `AutoFunctionStreamEvent` struct**: Wraps `AutoFunctionStreamChunk` with `event_id` field
- **New `get_interaction_stream()` method**: Resume streams from a specific `event_id` position

**Migration guide:**
```rust
// Before:
while let Some(chunk) = stream.next().await {
    match chunk? {
        StreamChunk::Delta(content) => { /* ... */ }
        StreamChunk::Complete(response) => { /* ... */ }
        _ => {}
    }
}

// After:
while let Some(result) = stream.next().await {
    let event = result?;
    // Optionally track event_id for resume support
    if let Some(id) = &event.event_id {
        last_event_id = Some(id.clone());
    }
    match event.chunk {  // Access .chunk on the event
        StreamChunk::Delta(content) => { /* ... */ }
        StreamChunk::Complete(response) => { /* ... */ }
        _ => {}
    }
}

// To resume an interrupted stream:
let resumed = client.get_interaction_stream(&interaction_id, Some(&last_event_id));
```

#### Additional Enums Now #[non_exhaustive] (#196)
- **`GenaiError`**: Match statements must include a wildcard arm
- **`FunctionError`**: Match statements must include a wildcard arm
- **`InteractionInput`**: Match statements must include a wildcard arm
- **New `GenaiError::MalformedResponse` variant**: For cases where the API returns 200 OK but with unexpected/malformed content
- This follows [Evergreen principles](https://github.com/google-deepmind/evergreen-spec) for forward-compatible API design

**Migration guide:**
```rust
// Before (exhaustive match):
match error {
    GenaiError::Http(e) => ...,
    GenaiError::Api { .. } => ...,
    // etc.
}

// After (must include wildcard):
match error {
    GenaiError::Http(e) => ...,
    GenaiError::Api { .. } => ...,
    GenaiError::MalformedResponse(msg) => ...,
    _ => ...,  // Required for forward compatibility
}
```

#### URI Content Helpers Require mime_type (#131)
- **Changed signatures**: `image_uri_content()`, `audio_uri_content()`, `video_uri_content()`, `document_uri_content()` now require `mime_type` as a mandatory parameter instead of `Option<String>`
- **Rationale**: Gemini API requires mime_type for URI-based content; making this compile-time enforced prevents runtime API errors

**Migration guide:**
```rust
// Before:
image_uri_content("https://example.com/image.png", Some("image/png".to_string()))

// After:
image_uri_content("https://example.com/image.png", "image/png")
```

#### Tool Enum is Now #[non_exhaustive] (#131)
- **`Tool` enum now includes `#[non_exhaustive]`**: Match statements must include a wildcard arm
- **New `Tool::Unknown` variant**: Captures unrecognized tool types from the API without failing deserialization
- This follows [Evergreen principles](https://github.com/google-deepmind/evergreen-spec) for forward-compatible API design

#### Error Type Consolidation (#131)
- **`InternalError` renamed to `GenaiError`** in `genai-client` crate
- New `Internal` and `InvalidInput` variants for better error categorization
- Users of the public `genai-rs` crate are unaffected (uses the same `GenaiError`)

#### create_with_auto_functions() Returns AutoFunctionResult (#148)
- **Changed return type**: `create_with_auto_functions()` now returns `AutoFunctionResult` instead of `InteractionResponse`
- **New `AutoFunctionResult` type**: Contains both the final response and execution history
- Provides visibility into which functions were called, enabling debugging, logging, and evaluation

**Migration guide:**
```rust
// Before:
let response = builder.create_with_auto_functions().await?;
println!("{}", response.text().unwrap());

// After:
let result = builder.create_with_auto_functions().await?;
println!("{}", result.response.text().unwrap());

// New: Access execution history with timing
for exec in &result.executions {
    println!("Called {} ({:?}) -> {}", exec.name, exec.duration, exec.result);
}
```

### Added

- **Request timeout and token usage helpers** (#228):
  - New `with_timeout(Duration)` on `InteractionBuilder` for per-request timeouts
  - For `create()`: Overall request timeout
  - For `create_stream()`: Per-chunk timeout to detect stalled connections
  - New `GenaiError::Timeout(Duration)` variant returned when requests exceed timeout
  - Token usage helper methods on `InteractionResponse`:
    - `input_tokens()`, `output_tokens()`, `total_tokens()`
    - `reasoning_tokens()`, `cached_tokens()`, `tool_use_tokens()`
  - Warning logged when timeout used with auto-function methods (not yet supported)

- **ToolService trait for dependency injection** (#197):
  - New `ToolService` trait enables tools to access shared state (DB connections, API clients, config)
  - Use `with_tool_service(Arc<dyn ToolService>)` on `InteractionBuilder` to provide tools
  - Service-provided functions take precedence over global `#[tool]` registry functions
  - When a service function shadows a global function, a warning is logged
  - Works with both `create_with_auto_functions()` and `create_stream_with_auto_functions()`

- **Partial results when max_function_call_loops exceeded** (#172):
  - `create_with_auto_functions()` now returns partial results instead of error when limit is hit
  - New `reached_max_loops: bool` field on `AutoFunctionResult` indicates if limit was reached
  - The `response` field contains the last API response (likely with pending function calls)
  - The `executions` vector preserves all function calls that were executed before hitting the limit
  - Enables debugging stuck function loops and accessing partial work
  - New `AutoFunctionStreamChunk::MaxLoopsReached` variant for streaming (parallel change)
  - `AutoFunctionResultAccumulator` now handles `MaxLoopsReached` and sets `reached_max_loops: true`
  - Legacy JSON without `reached_max_loops` field deserializes with default `false`

- **Function execution timing** (#148):
  - `FunctionExecutionResult.duration` tracks how long each function took to execute
  - Duration is serialized as milliseconds for JSON compatibility
  - Useful for performance monitoring, debugging, and optimization

- **Streaming accumulator helper** (#148):
  - New `AutoFunctionResultAccumulator` type to collect `AutoFunctionResult` from streaming
  - Allows combining streaming UI updates with execution history collection
  - Example:
    ```rust
    let mut accumulator = AutoFunctionResultAccumulator::new();
    while let Some(chunk) = stream.next().await {
        if let Some(result) = accumulator.push(chunk?) {
            // Stream complete, result contains full execution history
            println!("Executed {} functions", result.executions.len());
        }
    }
    ```

- **Full `Serialize`/`Deserialize` support for save/resume semantics** (#148, #151):
  - `InteractionResponse` now implements `Serialize` for logging, caching, and persistence
  - `AutoFunctionResult` implements `Serialize` and `Deserialize` for full execution history
  - `FunctionExecutionResult` now implements `Deserialize` for roundtrip serialization
  - `StreamChunk` and `AutoFunctionStreamChunk` implement both traits for streaming event replay
  - New `AutoFunctionStreamChunk::Unknown` variant for forward-compatible deserialization
  - Enables offline replay, testing/mocking, and state persistence for long-running agents

- **New convenience helpers on `InteractionResponse`** (#131):
  - `google_search_call()` - returns first Google Search call (singular)
  - `code_execution_call()` - returns first Code Execution call (singular)
  - `url_context_call()` - returns first URL Context call (singular)

#### Unified Streaming Content Types (#39, #27)
- **`StreamDelta` enum removed**: Streaming deltas now use `InteractionContent` directly
  - `StreamChunk::Delta(InteractionContent)` contains incremental content during streaming
  - `StreamChunk::Complete(InteractionResponse)` contains the final complete response
- **New `InteractionContent::ThoughtSignature` variant**: Captures streaming thought signatures
- **New helper methods on `InteractionContent`**: `text()`, `is_text()`, `is_thought()`, `is_thought_signature()`, `is_function_call()`
- **New type exported**: `StreamChunk` (note: `StreamDelta` is no longer exported)

**Migration guide:**
```rust
// Before:
match chunk {
    StreamChunk::Delta(delta) => match delta {
        StreamDelta::Text { text } => println!("{}", text),
        StreamDelta::Thought { text } => println!("[thinking: {}]", text),
        _ => {}
    }
    StreamChunk::Complete(response) => { /* ... */ }
}

// After:
match chunk {
    StreamChunk::Delta(content) => match content {
        InteractionContent::Text { text } => println!("{}", text.as_deref().unwrap_or("")),
        InteractionContent::Thought { text } => println!("[thinking: {}]", text.as_deref().unwrap_or("")),
        InteractionContent::FunctionCall { name, args, .. } => {
            println!("Function call: {}({:?})", name, args);
        }
        _ => {}
    }
    StreamChunk::Complete(response) => { /* ... */ }
}

// Helper methods still work the same:
if let Some(text) = delta.text() { /* ... */ }
```

### Added
- **Google Search grounding support** (#25): Enable real-time web search integration with Gemini models
  - New `with_google_search()` builder method on `InteractionBuilder`
  - New types: `GroundingMetadata`, `GroundingChunk`, `WebSource`
  - New helper methods: `has_google_search_metadata()`, `google_search_metadata()`, `has_google_search_calls()`, `google_search_calls()` on `InteractionResponse`
  - Full streaming support via `StreamChunk::Complete`

- **Code execution support** (#26): Enable Python code execution via Gemini's built-in sandbox
  - New `with_code_execution()` builder method on `InteractionBuilder`
  - New `CodeExecutionOutcome` enum with `Ok`, `Failed`, `DeadlineExceeded`, `Unspecified` variants
  - Updated `InteractionContent::CodeExecutionCall` with typed fields: `id`, `language`, `code`
  - Updated `InteractionContent::CodeExecutionResult` with typed fields: `call_id`, `outcome`, `output`
  - New helper methods on `InteractionResponse`: `code_execution_calls()`, `code_execution_results()`, `successful_code_output()`
  - New helper functions: `code_execution_call_content()`, `code_execution_result_content()`, `code_execution_success()`, `code_execution_error()`
  - Backward-compatible deserialization for old API response format
  - **Breaking (serialization)**: `CodeExecutionCall` now serializes `language` and `code` as top-level fields instead of nested in `arguments`. Deserialization remains backward-compatible with both formats.

- **URL context support** (#63): Enable URL content fetching and analysis
  - New `with_url_context()` builder method on `InteractionBuilder`
  - New types: `UrlContextMetadata`, `UrlMetadataEntry`, `UrlRetrievalStatus`
  - New helper methods: `has_url_context_metadata()`, `url_context_metadata()`, `has_url_context_calls()`, `url_context_calls()` on `InteractionResponse`
  - Supports up to 20 URLs per request, max 34MB per URL

- **Structured output JSON schema support** (#80): Enforce JSON schema constraints on model responses
  - Use `.with_response_format(schema)` to specify a JSON schema for structured output
  - Works standalone for structured data extraction
  - Combines with built-in tools (Google Search, URL Context)
  - New comprehensive example: `examples/structured_output.rs`

- **Function call/result structs**: New `FunctionCallInfo` and `FunctionResultInfo` structs with named fields for cleaner access
  - `function_calls()` now returns `Vec<FunctionCallInfo>` with fields: `id`, `name`, `args`, `thought_signature`
  - `function_results()` now returns `Vec<FunctionResultInfo>` with fields: `name`, `call_id`, `result`
  - New `has_function_results()` method on `InteractionResponse` for parity with `has_function_calls()`

- **Logging strategy documentation** (#203): New `docs/LOGGING_STRATEGY.md` with comprehensive guidelines
  - Log level definitions (error/warn/debug) with concrete examples
  - Sensitive data handling (API keys redacted, user content at debug only)
  - Evergreen pattern logging (Unknown variants log at warn level)
  - Debug logging for auto-function loop lifecycle (iteration tracking, execution timing)
  - Enable with `RUST_LOG=genai_rs=debug`

### Changed
- **`InteractionContent` is now `#[non_exhaustive]`** (#44): Match statements must include a wildcard arm (`_ => {}`). This allows adding new variants in minor version updates without breaking downstream code.
- **Deep Research example now requires background mode** (#179): Updated `deep_research.rs` example to reflect API requirement that `background=true` is mandatory for agent interactions. Removed synchronous mode demonstration since it is no longer supported by the API.
- **Function execution failures now log at `warn!` instead of `error!`** (#203): Since function failures are recoverable (the error is sent to the model which can retry or adapt), they are now correctly logged as warnings rather than errors. This aligns with the new logging strategy documented in `docs/LOGGING_STRATEGY.md`.

### Fixed
- **Streaming with function calls now works** (#27): Function call deltas are now properly parsed instead of causing errors
- **Streaming now properly yields content chunks** (#17): The streaming API was returning 0 chunks because the code expected all SSE events to have an `interaction` field, but the API sends different event types (`content.delta` and `interaction.complete`)

#### Simplified Client API
- **`Client::new()` signature simplified**: No longer takes `api_version` parameter
  - Before: `Client::new(api_key, None)`
  - After: `Client::new(api_key)`
  - The `api_version` was stored but never used; the library defaults to V1Beta internally
- **`ApiVersion` no longer re-exported** from genai-rs (still available in genai-client for internal use)

#### Removed deprecated function calling helpers
- **`function_response_content()` helper removed**: Use `function_result_content()` instead
  - Before: `function_response_content("get_weather", json!({"temp": 72}))`
  - After: `function_result_content("get_weather", "call_123", json!({"temp": 72}))`
  - The `call_id` parameter is required for proper API response matching
- **`InteractionContent::FunctionResponse` variant removed**: Use `FunctionResult` variant instead

#### UsageMetadata field names updated (#24)
- **Field names now match Interactions API**: The old GenerateContent API field names have been replaced
  - `prompt_tokens` → `total_input_tokens`
  - `candidates_tokens` → `total_output_tokens`
  - `total_tokens` remains unchanged
- **New fields added**: `total_cached_tokens`, `total_reasoning_tokens`, `total_tool_use_tokens`
- **Token usage now works**: Previously always returned `None` due to field name mismatch

## [0.2.0] - 2025-12-23

### BREAKING CHANGES

This release removes the legacy GenerateContent API in favor of the unified Interactions API. This is a major breaking change that requires code migration.

#### Removed
- **GenerateContent API**: All `GenerateContentBuilder` methods and related functionality removed
  - `Client::with_model()` method removed
  - `GenerateContentBuilder` type removed
  - `generate_from_request()` and `stream_from_request()` methods removed
  - `GenerateContentResponse` type removed (use `InteractionResponse` instead)

- **Helper modules**:
  - `content_api` module removed (use `interactions_api` instead)
  - `internal/response_processing` module removed

- **Examples**: Removed all GenerateContent examples
  - `simple_request.rs`
  - `stream_request.rs`
  - `code_execution.rs`
  - `function_call.rs`
  - `gemini3_thought_signatures.rs`

- **Internal crates**:
  - `genai-client/src/core.rs` removed
  - `genai-client/src/models/request.rs` removed
  - `genai-client/src/models/response.rs` removed

### Added

- **Enhanced InteractionResponse**:
  - New `.text()` convenience method to extract text from interaction responses
  - New `.function_calls()` convenience method to extract function calls with thought signatures

- **Automatic function calling for Interactions API**:
  - New `InteractionBuilder::create_with_auto_functions()` method
  - Auto-discovers and executes functions from the global registry
  - Supports multi-turn function calling with automatic loop handling

- **New helper functions**:
  - `function_result_content()` for sending function execution results (correct API format)
  - Enhanced `function_call_content_with_signature()` to include optional call ID

### Fixed

- **Function calling implementation** now correctly follows Google's Interactions API specification:
  - Added `id` field to `FunctionCall` to capture the call identifier from the API
  - Added new `FunctionResult` content type with `call_id` field (replaces `FunctionResponse`)
  - `create_with_auto_functions()` now sends only function results (not the original calls)
  - The API server maintains function call context via `previous_interaction_id`
  - Deprecated `FunctionResponse` variant (use `FunctionResult` instead)
  - Improved error message when max function call loops (5) is exceeded

### Changed

- **Primary API**: The Interactions API is now the only supported API
- **Migration Path**:
  - Replace `client.with_model(...).with_prompt(...).generate()`
  - With `client.interaction().with_model(...).with_text(...).create()`
  - Replace `generate_with_auto_functions()` with `create_with_auto_functions()`
  - Use `interactions_api` helper functions instead of `content_api`

### Migration Guide

#### Before (v0.1.x - GenerateContent API):
```rust
let response = client
    .with_model("gemini-3.6-flash")
    .with_prompt("Hello, world!")
    .generate()
    .await?;

println!("{}", response.text.unwrap());
```

#### After (v0.2.0 - Interactions API):
```rust
let response = client
    .interaction()
    .with_model("gemini-3.6-flash")
    .with_text("Hello, world!")
    .create()
    .await?;

println!("{}", response.text().unwrap_or("No text"));
```

#### Streaming:
```rust
// Before
let stream = client
    .with_model("gemini-3.6-flash")
    .with_prompt("Hello")
    .generate_stream()?;

// After
let stream = client
    .interaction()
    .with_model("gemini-3.6-flash")
    .with_text("Hello")
    .create_stream();
```

#### Automatic Function Calling:
```rust
// Before
let response = client
    .with_model("gemini-3.6-flash")
    .with_prompt("What's the weather?")
    .generate_with_auto_functions()
    .await?;

// After
let response = client
    .interaction()
    .with_model("gemini-3.6-flash")
    .with_text("What's the weather?")
    .create_with_auto_functions()
    .await?;
```

## [0.1.0] - 2024-12-XX

### Added
- Initial release
- Support for GenerateContent API
- Support for Interactions API
- Function calling with automatic discovery via macros
- Streaming support for both APIs
- Comprehensive test suite
- Example programs for both APIs
