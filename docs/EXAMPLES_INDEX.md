# Examples Index

Every example runs against the live API. Set `GEMINI_API_KEY` and:

```bash
cargo run --example <name>
LOUD_WIRE=1 cargo run --example <name>   # print each request and response
```

Examples exit non-zero when the thing they demonstrate didn't happen, so a
clean exit means it worked. A cheap subset is smoke-run in CI
(`example-smoke` in `.github/workflows/rust.yml`).

## Quick Reference

| I want to... | Example |
|--------------|---------|
| Make my first API call | `simple_interaction` |
| Stream responses (and resume a dropped stream) | `streaming` |
| Call my own functions | `auto_function_calling` |
| Hold a multi-turn conversation | `stateful_interaction` |
| Get JSON matching a schema | `structured_output` |
| Answer questions over my documents | `file_search` |
| Generate images | `image_generation` |
| Convert text to speech | `text_to_speech` |
| Retry transient failures | `retry_with_backoff` |

## Basics

| Example | Shows |
|---------|-------|
| `simple_interaction` | `create()`, reading text and usage |
| `streaming` | `create_stream()` and every `StreamChunk` variant; resuming with `get_interaction_stream(id, Some(last_event_id))` |
| `system_instructions` | `with_system_instruction()`; resending it each turn, since it is not inherited |
| `retry_with_backoff` | `build()` + `execute()`, `is_retryable()` / `retry_after()` with `backon` |

## Conversations

| Example | Shows |
|---------|-------|
| `stateful_interaction` | Server-side history via `with_previous_interaction()`; `get_interaction_with_input()`; `delete_interaction()` |
| `explicit_turns` | Client-side history: `conversation()`, `with_history()`, replaying `output_steps()` with signed thoughts under `with_store_disabled()` |

## Function Calling

| Example | Shows |
|---------|-------|
| `auto_function_calling` | `#[tool]` (zero-arg, typed, optional + enum params), auto-discovery vs `add_function()`, `FunctionCallingMode::Any` / `None` |
| `manual_function_calling` | Your own loop: parallel calls run with `join_all`, dependent calls across rounds, `Step::function_result` / `function_result_error` |
| `tool_service` | `ToolService` for tools that need shared state (`Arc<RwLock<_>>`), errors returned to the model |
| `streaming_auto_functions` | `create_stream_with_auto_functions()`: streamed text and argument deltas, execution events |

## Built-in Tools

| Example | Shows |
|---------|-------|
| `google_search` | Search grounding: queries issued, citation annotations, search-suggestion widgets |
| `url_context` | Fetching URLs named in the prompt, per-URL fetch status |
| `code_execution` | Server-side Python: executed code, output, answer |
| `google_maps` | Place data, `GoogleMapsConfig::with_widget()` |
| `file_search` | Create a store, upload, wait for indexing, retrieve, clean up |
| `computer_use` | `ComputerUseConfig`; the first requested browser action (executing actions needs your own browser harness) |

## Multimodal Input

| Example | Shows |
|---------|-------|
| `multimodal_image` | Inline images, several per request, follow-ups, `Resolution` vs image tokens |
| `audio_input` | Inline audio with `TranscriptionConfig` |
| `video_input` | Inline video; `VideoProcessing::segment()` to clip the window and frame rate |
| `pdf_input` | PDFs via `document_data`; text files via `document_from_file_with_mime` |
| `files_api` | `upload_file`, `wait_for_file_ready`, `Content::from_file`, list/get/delete, `upload_file_bytes` |

## Output

| Example | Shows |
|---------|-------|
| `structured_output` | `with_response_format()` into typed structs; with Google Search; while streaming |
| `thinking` | `ThinkingLevel` (incl. `Minimal` on `MINIMAL_THINKING_MODEL`), thought summaries, streamed summaries |
| `image_generation` | `DEFAULT_IMAGE_MODEL`, `with_image_output()`, `with_image_config()`, `images()` |
| `text_to_speech` | `DEFAULT_TTS_MODEL`, `with_audio_output()`, `SpeechConfig::for_speaker()` + `Content::speaker_text()` for dialogue |

## Background Work and Agents

| Example | Shows |
|---------|-------|
| `deep_research` | `DEFAULT_DEEP_RESEARCH_AGENT` in the background, polling with backoff, `cancel_interaction()` when the wait budget runs out |
| `webhooks_and_background` | Webhook CRUD / ping / secret rotation, per-request `webhook_config`, environments CRUD, `list_triggers()`. Without a key it prints the request shapes instead. |

## Applications

Located in [`examples/real_world/`](../examples/real_world/):

| Example | Shows |
|---------|-------|
| `multi_turn_agent_auto` | Support agent: `#[tool]` functions over a stub CRM, server-side history, system instruction and tools resent each turn |
| `multi_turn_agent_manual_stateless` | The same agent with `store` disabled: client-held history replayed via `output_steps()`, manual loop |

### Antigravity Harness

Located in [`examples/antigravity/`](../examples/antigravity/), grouped the
way `src/antigravity/` is. All seven need the `antigravity` feature **and**
the `localharness` binary (`pip install google-antigravity==0.1.18`), and
all seven are smoke-run in CI:

| Example | Description | Difficulty |
|---------|-------------|------------|
| [`agent.rs`](../examples/antigravity/agent.rs) (`--example antigravity_agent`) | The starter — spawn a harness, take a turn, Rust `#[tool]` functions, the `on_questions` hook | Advanced |
| [`repo_auditor/`](../examples/antigravity/repo_auditor/) | Agentic security audit on a fixture repo — subagents, policies + hooks, structured report | Advanced |
| [`session_resume/`](../examples/antigravity/session_resume/) | Agent that remembers across process restarts — trajectory persistence, `conversation_id` round trip, `initial_history` | Advanced |
| [`workspace_explorer/`](../examples/antigravity/workspace_explorer/) | Watching an agent work and gating it live — workspaces, typed `ToolAction` stream, content-based `on_pre_tool` deny | Advanced |
| [`mcp_toolbelt/`](../examples/antigravity/mcp_toolbelt/) | Giving an agent tools it didn't ship with — `add_mcp_server` (stdio), `mcp_<server>_<tool>` policy targets, MCP alongside `Capabilities::none()` | Advanced |
| [`proactive_agent/`](../examples/antigravity/proactive_agent/) | Work that starts without a user turn — `add_trigger`, observing deliveries via a wire inspector, the trigger/user-turn discard boundary | Advanced |
| [`cancellable_turn/`](../examples/antigravity/cancellable_turn/) | Stopping an agent mid-thought — `cancel_handle` from another task, partial output kept, contrast with `with_turn_timeout` | Advanced |

## Prerequisites

| Example | Needs |
|---------|-------|
| `deep_research` | Deep Research agent access; takes minutes (`DEEP_RESEARCH_MAX_WAIT_SECS` sets the budget) |
| `computer_use` | Computer Use access on your key |
| Antigravity examples | `localharness` binary (`pip install google-antigravity==0.1.18`) + `--features antigravity` |

Everything else runs on a standard API key.
