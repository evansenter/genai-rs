# Troubleshooting

Symptoms and fixes, each with a link to the guide that covers it.

## First: look at the wire

```bash
LOUD_WIRE=1 cargo run --example simple_interaction            # requests, responses, SSE frames
RUST_LOG=genai_rs=debug cargo run --example simple_interaction # library logs
```

`LOUD_WIRE` is read once, when the `Client` is built, so set it before your
program starts. Error responses are printed in full. For output format,
`RUST_LOG=genai_rs::wire=debug` and programmatic capture, see
[Logging Strategy](docs/LOGGING_STRATEGY.md).

Every request sends `Api-Revision: 2026-05-20`. The server currently ignores
the header and serves the 2026-05-20 protocol whatever value it carries, so
changing it changes nothing.

## Errors

| Symptom | Fix |
|---------|-----|
| `Api { status_code: 401 \| 403, .. }` | `GEMINI_API_KEY` is unset, invalid, or lacks access to the feature |
| `Api { status_code: 404, .. }` naming a model or agent | Use `genai_rs::DEFAULT_MODEL` (or another constant in the crate root) instead of a typed id |
| `Api { status_code: 429, .. }` | Rate limited. Retry with backoff and honor `error.retry_after()`; see [Reliability](docs/RELIABILITY.md) |
| `Api { status_code: 400, .. }` | The `message` usually names the field. Some fields are Vertex-only and rejected by the Gemini API (see [docs/INTERACTIONS_API_GAP.md](docs/INTERACTIONS_API_GAP.md)) |
| A 400 that passes on re-run | See [Known transient errors](docs/ERROR_HANDLING.md#known-transient-errors) |
| `GenaiError::Timeout(_)` | The request-level `with_timeout()` elapsed |
| `GenaiError::Http(e)` with `e.is_timeout()` | The **client-level** `ClientBuilder::with_timeout()` elapsed. It caps every request, including streams, and a request-level timeout cannot extend it; see [Timeouts](docs/RELIABILITY.md#timeouts) |
| `GenaiError::InvalidInput(_)` before any request is sent | The builder rejected the combination (for example, no input, both model and agent, or `with_store_disabled()` with chaining or background) |
| `'minimal' is not a supported thinking level for this model` | `DEFAULT_MODEL` rejects `ThinkingLevel::Minimal`; use `genai_rs::MINIMAL_THINKING_MODEL` |
| TLS errors in minimal containers | The crate verifies against the OS trust store; install a CA bundle |

## Function calling

**The model answers in text instead of calling your function.**
- Check that the request has a `tools` array (`LOUD_WIRE=1`).
- `create_with_auto_functions()` auto-discovers `#[tool]` and `ToolService`
  functions only when no tools are set on the request. Any explicit tool
  (`add_function()`, `with_google_search()`, ...) switches discovery off, so
  add every function you want declared.
- Tools are not inherited across turns: resend them on every new user turn
  that should be able to call them.
- To force a call, use `.with_function_calling_mode(FunctionCallingMode::Any)`.

**Wrong or missing arguments.** Improve the parameter descriptions. For
constrained values, declare a JSON-schema `enum` with
`FunctionDeclaration::builder(..).parameter(..)`. Missing or mistyped
arguments reach the model as `{"error": "Argument mismatch: ..."}`; see
[Function calling errors](docs/ERROR_HANDLING.md#function-calling-errors).

**The model sees your JSON as a string.** A `#[tool]` returning a `String`
is sent as `{"result": "<the string>"}`. Return a `serde_json::Value` or a
`Serialize` struct instead; see
[What a `#[tool]` function's return value becomes](docs/ERROR_HANDLING.md#what-a-tool-functions-return-value-becomes).

**The loop stops early.** The auto-function loop allows 5 rounds by default.
When it hits the limit it returns a partial `AutoFunctionResult` with
`reached_max_loops: true`. Raise the limit with
`.with_max_function_call_loops(n)`.

**`create_with_auto_functions()` errors immediately with store disabled.**
The loop chains turns by `previous_interaction_id`, so it needs storage.
Stateless conversations need the manual loop; see
[Function Calling](docs/FUNCTION_CALLING.md#manual-function-handling).

## Streaming

**The stream ends early.**
- `Err(GenaiError)` items are transport, parse or timeout errors.
- A `StreamChunk::Error { message, code }` event is a server-side error; it
  is terminal.
- A client-level timeout (`ClientBuilder::with_timeout`) cuts off long
  streams; prefer the request-level timeout, which applies between chunks.
- For stored interactions, resume from the last `event_id`; see
  [Stream Resume](docs/STREAMING_API.md#stream-resume).

## Multimodal and files

| Symptom | Fix |
|---------|-----|
| The model says it can't see the image | Use full MIME types (`"image/png"`, not `"png"`) and standard base64 |
| Inline video fails with a generic `400 Request contains an invalid argument` | The clip is shorter than one sampled frame (sub-second at the default ~1 fps). Use a longer clip or raise `fps`; see [Multimodal](docs/MULTIMODAL.md#video-processing) |
| An uploaded file is not usable yet | `client.wait_for_file_ready(&file, poll_interval, timeout).await?` returns the ready `FileMetadata`; `get_file()` and `is_active()` / `is_processing()` / `is_failed()` show the state |
| Image generation returns text | Use `genai_rs::DEFAULT_IMAGE_MODEL` and `.with_image_output()` |
| File Search finds nothing right after an upload | Wait with `client.wait_for_document_active(&doc.name, None, None)`; a pending document just returns no matches |
| `file_search_results()` is empty | Expected: the API does not emit that step; results are folded into the text (#429) |
| `'google_search' and 'file_search' cannot be combined` | File Search can't be combined with Google Search or URL Context in one request |

## Other questions

**"Unknown ... type" warnings in the logs.** The API returned a value the
crate doesn't model yet. It is preserved in an `Unknown` variant and your code
keeps working (the [Evergreen](https://github.com/google-deepmind/evergreen-spec)
pattern). Match with a wildcard arm and consider upgrading.

**Can I use this synchronously?** No. The crate is async-only (Tokio).

**Why do tests fail intermittently?** LLM output varies. Assert on
structure, use semantic validation for text, and see
[docs/TESTING.md](docs/TESTING.md).

## Reporting an issue

Include the `LOUD_WIRE=1` output (secrets are redacted), `rustc --version`,
the `genai-rs` version, and the `request_id` from any `GenaiError::Api`.
Open issues at <https://github.com/evansenter/genai-rs/issues>.
