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
| `Http(_)` from a request that can't be built (bad base URL, rejected header) or a redirect loop | No |
| `Api { status_code: 429, .. }` | Yes |
| `Api { status_code: 500..=599, .. }` | Yes |
| `Timeout(_)` (request-level timeout) | Yes |
| `StreamInterrupted { .. }` (a stream cut off before its terminal event) | Yes; see [Cut-off streams](#cut-off-streams) |
| Any other `Api` status (400, 401, 403, 404, ...) | No |
| `Parse`, `Json`, `Utf8`, `Internal`, `InvalidInput`, `MalformedResponse`, `Stream`, `ClientBuild` | No |

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
generation. A dropped foreground stream is cancelled server-side (verified
live 2026-11-01), so only a **background** interaction can be resumed: pick
up the *same* generation from the last `event_id` with
`client.interactions().resume_stream(interaction_id, last_event_id)`; see
[Stream Resume](STREAMING_API.md#stream-resume).

### Cut-off streams

A stream whose body ends before `interaction.completed` or an `error` event
ends with `GenaiError::StreamInterrupted`, however the connection cut it
off:

| How the body ended | `source` |
|--------------------|----------|
| The server closed the connection between events | `None` |
| The server closed it in the middle of an event (the unterminated tail doesn't parse) | `None` |
| The transport failed mid-body: a reset, a read or total timeout between chunks, a decode error | `Some(reqwest::Error)` |

All three are retryable. The failure belongs to the connection, not the
request, so the same request succeeds when re-sent, just as a non-streaming
`create()` whose body read fails is a retryable `Http` error. Treating one
of them as permanent would make a retry loop handle the same event two ways.
Retrying a foreground stream is a new generation: discard the partial output
you showed first. For a background interaction, resume instead.

A stream stopped by the request's own per-chunk limit
(`InteractionBuilder::with_timeout()` on `create_stream()`) ends with
`GenaiError::Timeout` instead: the limit you set ran out, and the connection
was not at fault. It is retryable too.

A failure before the response headers arrive (connect, TLS, a read timeout
while the model thinks) is `Http`, since no stream had started. A successful
response that is not `text/event-stream` (a captive portal's page) is
`MalformedResponse`, not a cut-off. Parameters (`; charset=utf-8`) and
letter case don't matter, and a response with no `Content-Type` is read as
a stream, so a proxy that strips the header still works; one that rewrites
it to another type does not.

A stream yields its terminal event as soon as it arrives, and the next
poll reads the rest of the body to its end, discarding every frame and
swallowing every error, so a late error, such as a reset while the server
closes, can't follow a finished interaction. That matters most for the
auto-function loop: a late error used to fail a round whose `Completed` had
already arrived.

### Read a stream to its end

The stored interaction is readable once the stream has ended. After
`interaction.completed` the API sends `event: done` / `data: [DONE]` and
closes the connection about a second later, and finishes storing the
interaction during that tail (verified live 2026-11-05): a `GET` sent right
after closing the connection at `interaction.completed` answered
`500 Internal error encountered`, the same `GET` ten seconds later `200`,
and one sent after reading to the end `200` at once. So to `get` a streamed
interaction or chain on it with `previous_interaction_id` right away, read
the stream until `next()` returns `None`; dropping it at `Completed` races
the server. `create_stream_with_auto_functions()` reads each round to its
end before sending the next.

The read is bounded at 5 s, since the default client has no read timeout: a
connection left half-open after the terminal event is closed then, with a
`warn!`, and the stream ends cleanly. The request-level per-chunk timeout
(`with_timeout()`) doesn't apply to it; a client-level read or total
timeout does, and is swallowed like any other error there.

## Timeouts

There are independent timeouts, and they behave differently.

| | Set with | Default | Mechanism | Error when exceeded |
|---|---|---|---|---|
| **Client-level** | `ClientBuilder::with_timeout()` | None | reqwest total-request timeout, from connect until the body is fully read (**including a streamed body**) | `GenaiError::Http` (`is_timeout()` is true); mid-stream, `StreamInterrupted` with that error as `source` |
| **Connect** | `ClientBuilder::with_connect_timeout()` | **30 s** (`DEFAULT_CONNECT_TIMEOUT`) | reqwest connect timeout: DNS, TCP and TLS | `GenaiError::Http` (`is_connect()` and `is_timeout()` are true) |
| **Read** | `ClientBuilder::with_read_timeout()` | None | reqwest read timeout: the longest wait for the headers or the next chunk, reset by each one | `GenaiError::Http` (`is_timeout()` is true) before the headers; mid-stream, `StreamInterrupted` with that error as `source` |
| **Request-level** | `InteractionBuilder::with_timeout()` | None | `tokio::time::timeout` around the call | `GenaiError::Timeout(duration)` |

`Client::new()` and `Client::builder(..).build()` get the same defaults.
`with_connect_timeout(Duration::MAX)` turns the connect timeout off.

### Tests on a paused clock

Tokio advances a paused clock (`#[tokio::test(start_paused = true)]`) to the
next timer whenever the runtime is idle, even while a socket operation is in
flight. Any timeout the client arms is such a timer, and since 0.14 every
client arms one while it connects (`DEFAULT_CONNECT_TIMEOUT`). So a
paused-clock test that sends a real request to a local mock server usually
fails with a connect timeout "after 30 s" that took no real time. Two ways
out:

- Turn the connect timeout off in the test client with
  `with_connect_timeout(Duration::MAX)`, which arms no timer, and set no
  read, total or request-level timeout. This is not enough for
  `wait_until_active`, which bounds each status request with its deadline.
- Or stop the auto-advance while I/O is in flight: a
  `tokio::task::spawn_blocking` task that is still running holds the clock.
  `tests/reliability_tests.rs` (`hold_clock`) releases it once the mock
  server has the request, which keeps the client's real defaults. For code
  that sends several requests, such as `wait_until_active`, a wire
  inspector can hold the clock per request: `ClockHold` in
  `tests/common/http_stub.rs` starts the task on `WireEvent::Request` and
  ends it once the response body is read, so the sleeps between polls
  still advance.

```rust,no_run
use genai_rs::Client;
use std::time::Duration;

# fn mock_client(mock_server_url: &str) -> Result<Client, genai_rs::GenaiError> {
let client = Client::builder("test-key".to_string())
    .with_base_url(mock_server_url)
    .with_connect_timeout(Duration::MAX) // no timer for a paused clock to fire
    .build()?;
# Ok(client)
# }
```

### Why only a connect timeout by default

Decided 2026-11-04 (#500). Connecting takes well under a second to Google's
front ends, so 30 s only ever fires on a broken path. It rides out four lost
SYNs on Linux (retransmits at 1, 3, 7 and 15 s) and a slow mobile or
satellite link, and fails a black-holed route in 30 s instead of the OS
default (about 75 s on macOS, about 127 s on Linux).

A default read or total timeout can't be chosen safely. A non-streaming
request's response headers arrive only after generation finishes, so the
read timeout's wait for the headers is the whole generation, and there is no
upper bound on that: a long think, a 4K image (the measured stream took 24 s,
2026-11-04), a deep context, a background agent between status updates. Any
default short enough to catch a hung connection cuts off some legitimate
request, and a generous one (say 10 minutes) catches almost nothing useful.
So the bound is yours to set once you know your workload: a read timeout for
streaming clients, a request-level timeout per call, or background execution
for work that may run for minutes.
`tests/reliability_tests.rs` pins the decision: on a paused clock, the default
client waits out an hour for the headers and an hour between stream events.

Consequences:

- **A request-level timeout cannot extend a client-level one.** Both
  apply, and the shorter wins. If you set per-request timeouts, or stream
  long responses, leave `ClientBuilder::with_timeout()` unset. With neither
  set, a request waits indefinitely once connected.
- **The read timeout is the client-level bound that suits streams.** It
  covers every request on the client, including `interactions().stream()`
  and `resume_stream()`, which no request-level timeout reaches, and ends a
  stalled connection without capping a healthy long stream. It also bounds
  time to first byte, so set it above the longest a non-streaming call
  thinks before answering.
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

## Waiting for a file or document

`files().wait_until_active()` and
`file_search_stores().documents().wait_until_active()` poll until the
resource is active, bounded by `PollOptions` (timeouts of 120 s and 60 s by
default, polling every 2 s and 500 ms):

- **The deadline is never overshot.** Each status request is cut off at the
  deadline, and the wait gives up as soon as the next poll would start at or
  past it, rather than sleeping into it. The first status request always
  runs, so an invalid name is reported even with a zero timeout.
- **The timeout must be longer than one round trip.** The first status
  request is cut off at the deadline too, so a shorter timeout fails even
  for a resource that is already active. The error then says the request
  was cut off by the wait's deadline (no request failed).
- **Transient errors are polled through.** A retryable error (a 5xx, a 429,
  a dropped connection) is logged and the wait goes on; any other error
  ends it.
- **`Retry-After` is honored.** When that error carries one (a 429), the
  next poll waits at least that long. If that is past the deadline, the
  wait gives up at once, saying so.
- The deadline error is `Internal`, so `is_retryable()` is false: the
  resource may still be processing, and whether to wait again is yours.

The wait runs on tokio time. Against a server on a paused clock, though,
its deadline is a timer that tokio's auto-advance fires while a status
request is in flight, unless each request holds the clock; see
[Tests on a paused clock](#tests-on-a-paused-clock).

## Cancellation

`client.interactions().cancel(id)` stops a **background** interaction that is
still `InProgress` and returns it with status `Cancelled`. It errors if the
interaction is not background, or has already finished.

To cancel from user action, race your cancel signal against the poll loop
with `tokio::select!`, and call `interactions().cancel` on the signal branch.
Dropping a future only ends your wait. `interactions().cancel` is what stops a
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
