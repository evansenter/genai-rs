# Reliability: Retries, Timeouts, Cancellation

`genai-rs` provides retry **primitives**, not a retry executor. Retry policy
(attempt count, backoff shape, budgets, circuit breaking, metrics) is
application-specific, so the crate gives you the pieces and leaves the policy
to a retry library such as [`backon`](https://docs.rs/backon).

For the error types themselves, see [Error Handling](ERROR_HANDLING.md).

## Primitives

### Rebuildable requests

`InteractionBuilder::build()` returns an `InteractionRequest` (which is
`Clone`) without sending it, and `Client::execute()` sends one. A retry loop
clones the request per attempt:

```rust,no_run
# async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
let request = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("Hello")
    .build()?;

let response = client.execute(request.clone()).await?;
# let _ = response;
# Ok(())
# }
```

### `GenaiError::is_retryable()`

| Error | Retryable? |
|-------|------------|
| `Http(_)` (network, connect, TLS, client-level timeout) | Yes |
| `Api { status_code: 429, .. }` | Yes |
| `Api { status_code: 500..=599, .. }` | Yes |
| `Timeout(_)` (request-level timeout) | Yes |
| Any other `Api` status (400, 401, 403, 404, ...) | No |
| `Parse`, `Json`, `Utf8`, `Internal`, `InvalidInput`, `MalformedResponse`, `ClientBuild` | No |

A few 400s are transient in practice but are **not** covered by
`is_retryable()`; see
[Known transient errors](ERROR_HANDLING.md#known-transient-errors).

### `GenaiError::retry_after()`

Returns the `Retry-After` delay the server sent (integer seconds or an HTTP
date), parsed into a `Duration`. It is populated on `Api` errors, typically
429s, and is `None` everywhere else.

## Recommended: `backon`

```rust,no_run
use backon::{ExponentialBuilder, Retryable};
use genai_rs::GenaiError;
use std::time::Duration;

# async fn run(client: genai_rs::Client) -> Result<(), GenaiError> {
let request = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("Hello")
    .build()?;

let backoff = ExponentialBuilder::default()
    .with_min_delay(Duration::from_millis(100))
    .with_max_delay(Duration::from_secs(30))
    .with_max_times(3);

let response = (|| async { client.execute(request.clone()).await })
    .retry(backoff)
    .when(|e: &GenaiError| e.is_retryable())
    // Prefer the server's Retry-After over our own backoff step
    .adjust(|e: &GenaiError, dur: Option<Duration>| dur.map(|d| e.retry_after().unwrap_or(d)))
    .await?;
# let _ = response;
# Ok(())
# }
```

`examples/retry_with_backoff.rs` is the runnable version.

## What is *not* retried for you

### The auto-function loop

`create_with_auto_functions()` makes one API call per round. If any call
fails, the whole loop returns that error; nothing inside it retries. This is
deliberate: re-running the loop re-executes your functions, which may have
side effects (writes, payments, external calls).

Make your tools idempotent before wrapping `create_with_auto_functions()` in
a retry. Otherwise drive the loop manually (see
[Function Calling](FUNCTION_CALLING.md#manual-function-handling)) so you can
retry only the API call.

### Streams

A stream cannot be retried mid-flight: re-sending the request starts a new
generation. For stored interactions (store is on by default), resume the
*same* generation from the last `event_id` with
`client.get_interaction_stream(interaction_id, Some(last_event_id))`; see
[Stream Resume](STREAMING_API.md#stream-resume). `event_id` is optional per
the API spec, so resume only works when the server sent one.

## Timeouts

There are two independent timeouts, and they behave differently.

| | Set with | Mechanism | Error when exceeded |
|---|---|---|---|
| **Client-level** | `ClientBuilder::with_timeout()` | reqwest total-request timeout, from connect until the body is fully read (**including a streamed body**) | `GenaiError::Http` (`is_timeout()` is true) |
| **Connect** | `ClientBuilder::with_connect_timeout()` | reqwest connect timeout | `GenaiError::Http` |
| **Request-level** | `InteractionBuilder::with_timeout()` | `tokio::time::timeout` around the call | `GenaiError::Timeout(duration)` |

Consequences:

- **A request-level timeout cannot extend a client-level one.** Both
  apply, and the shorter wins. If you set per-request timeouts, or stream
  long responses, leave `ClientBuilder::with_timeout()` unset. With neither
  set, a request waits indefinitely.
- Where the request-level timeout applies depends on the method:

| Method | Request-level timeout applies to |
|--------|----------------------------------|
| `create()` | The whole request |
| `create_stream()` | Each gap between chunks |
| `create_with_auto_functions()` | Each API call (function execution time is not counted) |
| `create_stream_with_auto_functions()` | Each gap between chunks, per round |

For a total deadline over an auto-function run, wrap it in
`tokio::time::timeout()` yourself.

```rust,no_run
use genai_rs::{Client, GenaiError};
use std::time::Duration;

# async fn run() -> Result<(), GenaiError> {
// No client-level timeout, so request-level timeouts are the only limit
let client = Client::builder("api-key".to_string())
    .with_connect_timeout(Duration::from_secs(10))
    .build()?;

match client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("Summarize this long document...")
    .with_timeout(Duration::from_secs(300))
    .create()
    .await
{
    Err(GenaiError::Timeout(after)) => eprintln!("gave up after {after:?}"),
    other => { let _ = other?; }
}
# Ok(())
# }
```

For work that may run for minutes (agents, deep research), use background
execution and poll or use webhooks instead of a long timeout; see
[Agents and Background Execution](AGENTS_AND_BACKGROUND.md).

## Cancellation

`client.cancel_interaction(id)` stops a **background** interaction that is
still `InProgress` and returns it with status `Cancelled`. It errors if the
interaction is not background, or has already finished.

To cancel from user action, race your cancel signal against the poll loop
with `tokio::select!`, and call `cancel_interaction` on the signal branch.
Dropping a future only ends your wait. `cancel_interaction` is what stops a
background interaction on the server.

## Service tiers

`with_service_tier(ServiceTier::Flex | Standard | Priority)` sets the
request's latency/priority tier. On the wire the values are `"flex"`,
`"standard"` and `"priority"`. The crate documents Flex as "flexible latency,
lower cost" and Priority as "prioritized processing"; it makes no claim about
queueing or shedding behavior beyond that.

```rust,no_run
use genai_rs::ServiceTier;

# async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("Summarize this document...")
    .with_service_tier(ServiceTier::Flex)
    .create()
    .await?;
# let _ = response;
# Ok(())
# }
```

## Related

- [Error Handling](ERROR_HANDLING.md): error types, status codes, known transients
- [Streaming API](STREAMING_API.md#stream-resume): resuming an interrupted stream
- [Agents and Background Execution](AGENTS_AND_BACKGROUND.md): polling, webhooks, cancellation
