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
| `Http(reqwest::Error)` | Network, connect, TLS, or client-level timeout | Yes |
| `Timeout(Duration)` | The request-level timeout (`InteractionBuilder::with_timeout`) elapsed | Yes |
| `Json(serde_json::Error)` | A response body failed to deserialize | No |
| `Parse(String)` | An SSE stream could not be parsed | No |
| `Utf8(Utf8Error)` | A response was not valid UTF-8 | No |
| `InvalidInput(String)` | The builder rejected the request before sending (for example: no input, neither or both of model and agent, or `with_store_disabled()` combined with chaining or background) | No |
| `MalformedResponse(String)` | A 2xx response did not have the expected shape | No |
| `Internal(String)` | A client-side invariant failed | No |
| `ClientBuild(String)` | The HTTP client could not be built (TLS backend init) | No |

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

`Api.message` holds the error response body, truncated. The full body is
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
integration suite also retries three error classes that `is_retryable()`
deliberately leaves out, because they are matched on message text rather than
status (`tests/common/mod.rs`, `is_transient_error`):

| Error | Notes |
|-------|-------|
| `Api` whose message contains both `spanner` and `utf-8` | Google backend issue seen on stateful conversations (#60) |
| `Api` 400 containing `invalid json syntax` | The model occasionally emits invalid JSON (structured output, or a malformed function call) |
| `Api` 400 containing `there was a problem processing your request` | Server-side bursts that pass on re-run |

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
            (m.contains("spanner") && m.contains("utf-8"))
                || (*status_code == 400 && m.contains("invalid json syntax"))
                || (*status_code == 400
                    && m.contains("there was a problem processing your request"))
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

A stream from `create_stream()` can end in two ways:

- `Err(GenaiError)` items, for transport, parse, or timeout errors. Resume
  from the last `event_id` if the interaction was stored; see
  [Stream Resume](STREAMING_API.md#stream-resume).
- A terminal `StreamChunk::Error { message, code }` event, which the server
  sends inside the SSE stream. No events follow it.

## Related

- [Reliability](RELIABILITY.md): retry primitives, `backon`, timeout semantics, cancellation
- [Troubleshooting](../TROUBLESHOOTING.md): symptoms and fixes
- [Logging Strategy](LOGGING_STRATEGY.md): `LOUD_WIRE`, `RUST_LOG`, wire inspectors
