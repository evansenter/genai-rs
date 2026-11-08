# Error Handling

This page covers error types, what each one means, and how function-call
errors reach the model. For retries, timeouts and cancellation, see
[Reliability](RELIABILITY.md).

## `GenaiError`

Every `Client` and `InteractionBuilder` operation returns
`Result<_, GenaiError>`. The enum is `#[non_exhaustive]`, so always include a
wildcard arm.

| Variant | Meaning | `is_retryable()` |
|---------|---------|------------------|
| `Api { status_code, message, request_id, retry_after }` | The API returned a non-2xx status | 429 and 5xx only |
| `Http(reqwest::Error)` | Network, connect (including the default 30 s connect timeout), TLS, or a client-level timeout before the response headers | Yes, except a request that can't be built (bad base URL, rejected header) or a redirect loop |
| `Timeout(Duration)` | The request-level timeout (`InteractionBuilder::with_timeout`) elapsed | Yes |
| `StreamInterrupted { message, source }` | A stream's body ended before its terminal event: the server closed early, or the transport failed mid-body (`source` holds that `reqwest::Error`). Also an auto-function round that ends on an event this crate doesn't recognize, with `source` `None` ([An unrecognized last event](RELIABILITY.md#an-unrecognized-last-event)) | Yes; see [Cut-off streams](RELIABILITY.md#cut-off-streams) |
| `InvalidInput(String)` | The builder rejected the request before sending (for example: no input, neither or both of model and agent, or `with_store_disabled()` combined with chaining or background) | No |
| `MalformedResponse(String)` | A 2xx response did not parse or did not have the expected shape, including a streaming response that is not `text/event-stream`, an SSE event that does not parse or is not valid UTF-8, and one that grows past 64 MiB without ending | No |
| `PollTimeout { timeout, message }` | A `wait_until_active` (or a file search store import's wait) reached its `PollOptions` timeout; the resource may still be processing | No: wait again rather than re-send |
| `ProcessingFailed(String)` | A file or file search document failed processing, or an import's operation reported an error | No: only a new upload recovers |
| `Stream { message, code }` | The server sent an in-stream `error` event (auto-function streaming loop only; plain streams yield `StreamChunk::Error`) | No |
| `Internal(String)` | The client failed in a way that is neither the API's answer nor your input: a request body that would not serialize, or an unexpected failure reading back a document an upload or import already made (the message names it) | No |
| `ClientBuild(reqwest::Error)` | The HTTP client could not be built (TLS backend init); the reqwest error is the `source()` | No |

```rust
use genai_rs::GenaiError;

fn describe(err: &GenaiError) -> String {
    match err {
        GenaiError::Api { status_code, message, request_id, .. } => {
            format!("API {status_code}: {message} (request id {request_id:?})")
        }
        GenaiError::Timeout(after) => format!("timed out after {after:?}"),
        GenaiError::Http(e) if e.is_timeout() => "client-level timeout".to_string(),
        GenaiError::InvalidInput(msg) => format!("bad request built locally: {msg}"),
        other => other.to_string(),
    }
}
# let _ = describe;
```

`Api.message` is the message from Google's error envelope, prefixed with its
status or code (`"INVALID_ARGUMENT: ..."`, `"invalid_request: ..."`). A body
that is not an error envelope is kept as a truncated preview. The full body is
visible with `LOUD_WIRE=1` (see [Logging](LOGGING_STRATEGY.md)).

## API status codes

| Status | Usual cause | What to do |
|--------|-------------|------------|
| 400 | Invalid request: a bad field, an unsupported combination, a field the Gemini API rejects as Vertex-only | Read `message`; it usually names the field |
| 401 / 403 | Missing, invalid or unauthorized API key, or a feature not enabled for the key | Check `GEMINI_API_KEY` and access |
| 404 | Unknown model, agent, file or interaction id | Use the constants in `genai_rs` (`DEFAULT_MODEL`, ...) rather than typed ids |
| 429 | Rate limited | Back off; honor `retry_after()` when set |
| 5xx | Server error | Retry with backoff |

`request_id` comes from the `x-goog-request-id` response header. Log it; it is
what Google support needs to find a specific failed request.

## Known transient errors

`is_retryable()` covers transport errors, timeouts, 429 and 5xx. The
integration suite also retries four error classes that `is_retryable()`
deliberately leaves out, because they are matched on message text rather than
status (`tests/common/retry.rs`, `is_transient_error`):

| Error | Notes |
|-------|-------|
| `Api` whose message contains both `spanner` and `utf-8` | Google backend issue seen on stateful conversations (#60) |
| `Api` 400 containing `invalid json syntax` | The model occasionally emits invalid JSON (structured output, or a malformed function call) |
| `Api` 400 containing `there was a problem processing your request` | Server-side bursts that pass on re-run |
| `Api` 400 containing `please retry the request`, unless it names `too_many_tool_calls` | The API says so itself after a broken tool call (`malformed_tool_call`). A tool-call loop (`too_many_tool_calls`) is not retried whatever its message says: the same request loops again (see the [gap tracker](INTERACTIONS_API_GAP.md)), and each attempt runs to the 60-call limit |

If you want the same behavior, compose the two predicates:

```rust
use genai_rs::GenaiError;

fn should_retry(err: &GenaiError) -> bool {
    if err.is_retryable() {
        return true;
    }
    match err {
        GenaiError::Api { status_code, message, .. } => {
            let m = message.to_lowercase();
            if m.contains("too_many_tool_calls") {
                return false;
            }
            (m.contains("spanner") && m.contains("utf-8"))
                || (*status_code == 400 && m.contains("invalid json syntax"))
                || (*status_code == 400
                    && m.contains("there was a problem processing your request"))
                || (*status_code == 400 && m.contains("please retry the request"))
        }
        _ => false,
    }
}
# let _ = should_retry;
```

Keep the retry budget small: these matches are on message text, so an API
wording change turns a real rejection into a few wasted retries.

## Function calling errors

In `create_with_auto_functions()` a failing function does **not** fail the
loop. The error is sent back to the model as the function result, so it can
retry or explain. Each case is logged at `warn`.

| Situation | Result the model receives |
|-----------|---------------------------|
| The model calls a function that is not registered | `{"error": "Function '<name>' is not available or not found."}` |
| Arguments missing or of the wrong type (`#[tool]` argument extraction) | `{"error": "Argument mismatch: ..."}` |
| A `CallableFunction::call` returns `Err(FunctionError::ExecutionError(e))` | `{"error": "Function execution error: <e>"}` |

### What a `#[tool]` function's return value becomes

The macro serializes the return value with `serde_json::to_value`. If the
result is a JSON object it is sent as-is; anything else is wrapped as
`{"result": <value>}`. So:

- Return a `Serialize` struct or a `serde_json::Value` for structured data.
  A `String` that *contains* JSON arrives as a string,
  `{"result": "{\"temp\": 22}"}`, which the model can read but which is not
  structured.
- A `Result<T, E>` is serialized by serde as `{"Ok": ...}` or `{"Err": ...}`.
  It is *not* turned into `{"error": ...}`.
- For a real `{"error": ...}` result, return a JSON object with an `error`
  key, or implement `CallableFunction` yourself and return
  `Err(FunctionError::ExecutionError(..))`.

```rust
use genai_rs_macros::tool;
use serde_json::{json, Value};

/// Look up a user by id
#[tool(id(description = "The user id, a positive integer"))]
fn get_user(id: i64) -> Value {
    if id <= 0 {
        return json!({"error": "id must be positive"});
    }
    json!({"id": id, "name": "Alice"})
}
# let _ = get_user_declaration();
```

Don't panic inside a tool. A panic unwinds through the auto-function loop
instead of reaching the model as a result.

## Streaming errors

A stream from `create_stream()` ends in one of three ways:

- A terminal `StreamChunk::Completed { interaction, .. }`.
- A terminal `StreamChunk::Error { message, code, .. }` event, which the server
  sends inside the SSE stream, such as a recitation block (`code:
  "recitation"`). No events follow it.
- An `Err(GenaiError)` item, for transport, parse, or timeout errors. A
  body that ends before either terminal event ends with
  `GenaiError::StreamInterrupted`, whether the server closed the connection
  or the transport failed mid-body (a reset or a read timeout, kept as
  `source`). It is retryable; only background interactions can be resumed
  rather than re-sent. See [Cut-off streams](RELIABILITY.md#cut-off-streams)
  and [Stream Resume](STREAMING_API.md#stream-resume).

After a terminal event the stream reads the rest of the body and discards
it, errors included, so no error can follow `Completed` or `Error`. Read
the stream to its end before you `get` the interaction or chain on it; see
[Read to the end](STREAMING_API.md#read-to-the-end).

## Related

- [Reliability](RELIABILITY.md): retry primitives, `backon`, timeout semantics, cancellation
- [Troubleshooting](../TROUBLESHOOTING.md): symptoms and fixes
- [Logging Strategy](LOGGING_STRATEGY.md): `LOUD_WIRE`, `RUST_LOG`, wire inspectors
