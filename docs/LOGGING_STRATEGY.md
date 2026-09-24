# Logging Strategy

`genai-rs` has two observability channels:

- **Library logs** through [`tracing`](https://docs.rs/tracing), controlled
  with `RUST_LOG`.
- **Wire inspection**: every request, response, SSE frame and upload as a
  structured `wire::WireEvent`. `LOUD_WIRE=1` prints them; you can also
  install your own inspector.

## Library logs

Install a subscriber in your application. The library itself emits nothing
until one is installed.

```rust,ignore
tracing_subscriber::fmt()
    .with_env_filter("genai_rs=debug")
    .init();
```

```bash
RUST_LOG=genai_rs=debug cargo run --example simple_interaction
```

| Level | Used for |
|-------|----------|
| `error` | Failures the library cannot recover from (for example a Files API upload whose server-side processing failed) |
| `warn` | Recoverable problems: unknown enum values preserved in `Unknown` variants (Evergreen), function failures sent back to the model, functions not found, validation warnings (missing required parameters, large inline files), shadowed tool names |
| `info` | Only in the `antigravity` feature (policy denials) |
| `debug` | API lifecycle, request and response bodies, SSE events, auto-function rounds and timings |

API errors are **returned** as `GenaiError`, not logged.

`Client::execute` and `Client::execute_stream` carry
`#[tracing::instrument]` spans with `model` and `agent` fields, so events
inside them are attributed to the request.

### Sensitive data

- The API key is sent in the `X-Goog-Api-Key` header, never in the URL, and
  `Client` / `ClientBuilder` print it as `[REDACTED]` in `Debug` output.
- At `debug`, request and response bodies are logged in full: prompts, base64
  media, function arguments and results. Treat debug logs as sensitive, and
  don't enable them in production unless you mean to.

## Wire inspection

### `LOUD_WIRE`

```bash
LOUD_WIRE=1 cargo run --example simple_interaction
```

`LOUD_WIRE` is read once, when the `Client` is constructed, and installs a
`wire::LoudWirePrinter`. The value is a comma-separated filter:

| Value | Prints |
|-------|--------|
| `1`, `true`, or empty | Everything, pretty-printed |
| `request`, `response`, `sse`, `upload` | Only those HTTP event kinds (combine with commas) |
| `ws`, `harness`, or a WebSocket payload key such as `toolCall` | Antigravity harness traffic (see [ANTIGRAVITY.md](ANTIGRAVITY.md)) |
| `summary` | Modifier: one line per event instead of full bodies |
| anything else (`0`, `false`, `off`) | **Nothing**; an unrecognized selector selects no events |

### LOUD_WIRE output format

```text
[LOUD_WIRE] 2026-09-24T10:30:45Z [REQ#1] >>> POST https://generativelanguage.googleapis.com/v1beta/interactions
[LOUD_WIRE] 2026-09-24T10:30:45Z [REQ#1] Body:
[LOUD_WIRE] 2026-09-24T10:30:45Z [REQ#1] {
[LOUD_WIRE] 2026-09-24T10:30:45Z [REQ#1]   "model": "gemini-3.8-flash",
[LOUD_WIRE] 2026-09-24T10:30:45Z [REQ#1]   "input": "Hello!"
[LOUD_WIRE] 2026-09-24T10:30:45Z [REQ#1] }
[LOUD_WIRE] 2026-09-24T10:30:46Z [RES#1] <<< 200 OK
[LOUD_WIRE] 2026-09-24T10:30:46Z [RES#1] Response:
[LOUD_WIRE] 2026-09-24T10:30:46Z [RES#1] { ...pretty-printed JSON... }
```

Streaming responses print one `SSE event: <type>` line plus an `SSE:` body per
frame.

- **`[REQ#N]` / `[RES#N]`** correlate a request with its response. Ids count
  from 1 per `Client`.
- **Colors** (with the default-on `wire-color` feature): odd requests are
  yellow `[REQ#N]` / cyan `[RES#N]`, even ones green / magenta; SSE frames are
  blue. Build with `default-features = false` for plain text and no
  `colored` / `colored_json` dependencies.
- **Error responses** print a `<<< <status> ERROR` line, then `Error (<status>)`
  with the body (JSON pretty-printed; a non-JSON body is cut at 1,000 bytes).
- **Base64 fields** (`data`, `signature`) are truncated to about 100 bytes.
  **Secret fields** are redacted: `api_key`, `secret`, `new_signing_secret`,
  `token`, `client_secret` and `refresh_token` anywhere, and `value` inside
  an `environment_variable` credential or an `env` map. The `wire` tracing
  target redacts the same fields.
- **Uploads** print as `>>> UPLOAD "video.mp4" (video/mp4, 150.25 MB)`.

### Wire Inspection API

`LOUD_WIRE` is sugar over the public `genai_rs::wire` module. Implement
`wire::WireInspector` to receive every `WireEvent` (request with JSON body,
response status, response body, error body, SSE frame, upload start and
complete). Register it on the client builder; multiple inspectors are
allowed, and each receives every event:

```rust,no_run
use genai_rs::Client;
use genai_rs::wire::{WireEvent, WireInspector};
use std::sync::Arc;

struct RequestCounter;

impl WireInspector for RequestCounter {
    fn on_event(&self, event: &WireEvent) {
        if let WireEvent::Request { id, method, url, .. } = event {
            eprintln!("request #{id}: {method} {url}");
        }
    }
}

# fn main() -> Result<(), genai_rs::GenaiError> {
let client = Client::builder("api-key".to_string())
    .add_wire_inspector(Arc::new(RequestCounter))
    .build()?;
# Ok(())
# }
```

- **Correlation**: all events for one HTTP request share a per-client `id`.
- **Zero cost when unused**: with no inspectors, events are never built and
  bodies are never serialized for inspection.
- **Synchronous**: inspectors run on the request path, so keep them fast.
- **Raw data**: custom inspectors receive bodies unredacted. Only the
  built-in inspectors truncate and redact.

### Forwarding wire events to `tracing`

`wire::TracingForwarder` sends events to `tracing` at `DEBUG` under the
`genai_rs::wire` target (`wire::TRACING_TARGET`), with structured fields
(`kind`, `id`, `method`, `url`, `status`, and the redacted JSON `body` as a
string):

```rust,no_run
use genai_rs::Client;
use genai_rs::wire::TracingForwarder;
use std::sync::Arc;

# fn main() -> Result<(), genai_rs::GenaiError> {
let client = Client::builder("api-key".to_string())
    .add_wire_inspector(Arc::new(TracingForwarder::new()))
    .build()?;
# Ok(())
# }
```

```bash
RUST_LOG=genai_rs::wire=debug cargo run --example simple_interaction                 # wire events only
RUST_LOG=genai_rs=debug,genai_rs::wire=debug cargo run --example simple_interaction  # both
```

## Guidelines for contributors

- **Always log** when data lands in an `Unknown` variant (`warn`), and when
  a validation issue doesn't fail but may cause problems (`warn`).
- **Never log** user content above `debug`.
- Write messages that say what happened and why it matters, with the
  identifying values: `"Encountered unknown Step type '{}'. This may indicate
  a new API feature..."`, not `"Unknown type"`.
- Prefer structured fields (`tracing::debug!(interaction_id = ?id, "...")`)
  for values someone will filter on.
- On new async entry points, add `#[tracing::instrument(skip(self, ...))]`,
  skipping large arguments and recording ids.
