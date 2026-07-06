# Logging Strategy

This document defines the logging strategy for `genai-rs`, ensuring consistent, secure, and useful log output across the codebase.

## Tracing Framework

This library uses the [`tracing`](https://docs.rs/tracing) crate for structured logging and diagnostics. Tracing provides:

- **Structured logging**: Key-value fields in addition to messages
- **Spans**: Track execution context across async boundaries
- **Ecosystem compatibility**: Works with `tracing-subscriber`, OpenTelemetry, and more
- **Zero-cost when disabled**: No overhead when no subscriber is configured

## Log Levels

### `error!` - Unrecoverable failures
Use for situations where the operation cannot continue and needs user intervention.

**Use when:**
- Internal logic errors that indicate bugs
- Fatal configuration issues
- Malformed API responses that prevent operation (e.g., missing required fields)

**Examples from codebase:**
```rust,ignore
// Missing required call_id field (src/request_builder/auto_functions.rs)
error!(
    "Function call '{}' is missing required call_id field.",
    function_name
);
```

**Not for:**
- API errors (those are returned as `GenaiError`, not logged)
- Recoverable function execution failures (use `warn!` instead)

### `warn!` - Recoverable issues requiring attention
Use for situations that are unusual, potentially problematic, or indicate degraded operation, but where the code can continue.

**Use when:**
- Unknown API types encountered (Evergreen pattern)
- Configuration issues that have reasonable fallbacks
- Large file uploads exceeding recommended limits
- Validation warnings (e.g., empty function names)
- Malformed API responses that can be recovered from
- Function execution failures in auto-function mode (error sent to model for recovery)

**Examples from codebase:**
```rust,ignore
// Function execution failure - recoverable (src/request_builder/auto_functions.rs)
warn!(
    "Function execution failed (recoverable): function='{}', error='{}'. \
     The error will be sent to the model, which may retry or adapt.",
    call.name, e
);

// Large file warning (src/multimodal.rs)
tracing::warn!(
    "File '{}' is {:.1}MB which exceeds the recommended 20MB limit for inline data...",
    path.display(), size_mb
);

// Unknown API type (src/content.rs)
tracing::warn!(
    "Encountered unknown Content type '{}'. \
     Parse error: {}. \
     This may indicate a new API feature or a malformed response. \
     The content will be preserved in the Unknown variant.",
    content_type, parse_error
);

// Validation warning (src/tools.rs)
tracing::warn!(
    "FunctionDeclaration '{}' requires parameter '{}' which is not defined in properties...",
    self.name, req
);
```

### `debug!` - Development and troubleshooting details
Use for information useful during development or when diagnosing issues.

**Use when:**
- API request/response lifecycle events
- Request body contents (JSON format preferred)
- SSE streaming events
- Internal state transitions

**Examples from codebase:**
```rust,ignore
// Request body logging (src/client.rs)
fn log_request_body<T: std::fmt::Debug + serde::Serialize>(body: &T) {
    match serde_json::to_string_pretty(body) {
        Ok(json) => tracing::debug!("Request Body (JSON):\n{json}"),
        Err(_) => tracing::debug!("Request Body: {body:#?}"),
    }
}

// Response body logging (src/client.rs)
fn log_response_body<T: std::fmt::Debug + serde::Serialize>(body: &T) {
    match serde_json::to_string_pretty(body) {
        Ok(json) => tracing::debug!("Response Body (JSON):\n{json}"),
        Err(_) => tracing::debug!("Response Body: {body:#?}"),
    }
}

// Interaction lifecycle (src/client.rs)
tracing::debug!("Creating interaction");
tracing::debug!("Interaction created: ID={}", response.id);

// SSE events (src/http/interactions.rs)
debug!(
    "SSE event received: event_type={:?}, has_delta={}, has_interaction={}...",
    event.event_type, event.delta.is_some(), event.interaction.is_some()
);

// Auto-function loop lifecycle (src/request_builder/auto_functions.rs)
debug!("Auto-function loop iteration {}/{}", loop_count + 1, max_loops);
debug!("Executing {} function call(s)", function_calls.len());
debug!("Function '{}' executed in {:?}", call.name, duration);
```

### `trace!` - Verbose low-level details
Use for very detailed information that would be overwhelming at debug level.

**Use when:**
- Byte-level streaming data
- Individual token processing
- Parser state transitions

**Note:** Currently not used in the codebase. Consider adding for SSE parser internals if deeper debugging is needed.

## Instrumented Methods

Key methods use `#[tracing::instrument]` for automatic span creation:

```rust,ignore
// Client::execute - Creates a span with model/agent context
#[tracing::instrument(skip(self), fields(model = ?request.model, agent = ?request.agent))]
pub async fn execute(&self, request: InteractionRequest) -> Result<InteractionResponse, GenaiError>

// Client::execute_stream - Creates a span with model/agent context
#[tracing::instrument(skip(self), fields(model = ?request.model, agent = ?request.agent))]
pub fn execute_stream(&self, request: InteractionRequest) -> BoxStream<'_, Result<StreamEvent, GenaiError>>
```

These spans:
- Automatically record entry/exit
- Include the model or agent being used
- Propagate through async boundaries
- Enable distributed tracing when used with OpenTelemetry

## Logging Categories

### 1. API Request/Response Logging

**Current state:** ✅ Implemented at `debug` level

| Event | Level | Location |
|-------|-------|----------|
| Request body | `debug` | `src/client.rs::log_request_body()` |
| Response body | `debug` | `src/client.rs::log_response_body()` |
| Interaction created | `debug` | `src/client.rs::execute()` |
| Interaction retrieved | `debug` | `src/client.rs::get_interaction()` |
| Interaction deleted | `debug` | `src/client.rs::delete_interaction()` |
| Stream chunk received | `debug` | `src/client.rs::execute_stream()` |
| SSE event details | `debug` | `src/http/interactions.rs` |
| Auto-function loop iteration | `debug` | `src/request_builder/auto_functions.rs` |
| Function execution timing | `debug` | `src/request_builder/auto_functions.rs` |

### 2. Unknown/Evergreen Type Handling

**Current state:** ✅ Implemented at `warn` level

All Evergreen-pattern `Unknown` variants log when encountered:

| Type | Location |
|------|----------|
| `Content::Unknown` | `src/content.rs` |
| `InteractionStatus::Unknown` | `src/response.rs` |
| `Tool::Unknown` | `src/tools.rs` |
| `StreamChunk::Unknown` | `src/wire_streaming.rs` |
| `AutoFunctionStreamChunk::Unknown` | `src/streaming.rs` |

### 3. Validation Warnings

**Current state:** ✅ Implemented at `warn` level

| Condition | Location |
|-----------|----------|
| Large file (>20MB) | `src/multimodal.rs::load_and_encode_file()` |
| Empty function name | `src/tools.rs::build()` |
| Missing required parameters | `src/tools.rs::build()` |
| Empty call_id/thought_signature | `src/interactions_api.rs` |
| max_function_call_loops=0 | `src/request_builder/mod.rs` |
| Function execution failure | `src/request_builder/auto_functions.rs` |
| Function not found in registry | `src/request_builder/auto_functions.rs` |

### 4. Silent Operations

**Current state:** ⚠️ Partially implemented

The following operations silently handle edge cases and may benefit from logging:

| Operation | Current Behavior | Recommendation |
|-----------|------------------|----------------|
| SSE lifecycle events | Silently skipped | Keep silent (expected behavior) |
| Unknown SSE events with interaction field | `warn` logged | ✅ Already logged |
| CodeExecutionCall language fallback | `warn` logged | ✅ Already logged |
| Unknown InteractionInput variant | `warn` logged | ✅ Already logged |

## Sensitive Data Handling

### API Key Protection

**Current state:** ✅ Protected

- `Client` and `ClientBuilder` implement custom `Debug` that shows `[REDACTED]` for API keys
- API keys are passed via URL query parameter (standard for Google APIs)
- Request bodies do not contain API keys

### Request Body Content

**Current state:** ⚠️ Logs at debug level

The `log_request_body()` function logs full request contents including:
- User prompts (text content)
- Base64-encoded media (images, audio, video, documents)
- Function call parameters

**Recommendation:** This is appropriate at `debug` level since:
1. Debug logs are not enabled by default
2. Users who enable debug logging expect detailed output
3. The library is a client-side tool (logs stay local)

**Consider:** Adding a separate `trace` level for base64 content to keep debug logs more readable.

## Structured Logging with Tracing

The library uses tracing's structured logging capabilities:

```rust,ignore
// Structured fields with spans
#[tracing::instrument(fields(model = ?request.model))]
pub async fn execute(&self, request: InteractionRequest) -> Result<InteractionResponse, GenaiError>

// Structured event logging
tracing::debug!(
    interaction_id = %response.id,
    model = ?response.model,
    "Interaction created"
);
```

Benefits:
- Machine-parseable log output
- Correlation across async boundaries via spans
- Compatible with distributed tracing (OpenTelemetry, Jaeger, etc.)
- Filterable by field values in some subscribers

## Guidelines for Adding New Logs

### When to Log

1. **Always log** Unknown variants in Evergreen enums
2. **Always log** validation issues that don't fail but may cause problems
3. **Consider logging** fallback behaviors (e.g., defaulting values)
4. **Never log** sensitive user data at `info` level or above

### Message Format

Use consistent message formatting:

```rust,ignore
// Good: Action-oriented with key=value context
tracing::debug!("Creating interaction");
tracing::debug!("Interaction created: ID={}", response.id);
tracing::warn!("File '{}' is {:.1}MB which exceeds the recommended 20MB limit...", path, size);

// Good: Explains why something is unusual
tracing::warn!(
    "Encountered unknown Tool type '{}'. \
     This may indicate a new API feature or a malformed response.",
    tool_type
);

// Good: Structured fields for machine parsing
tracing::debug!(
    interaction_id = %response.id,
    status = ?response.status,
    "Interaction retrieved"
);

// Bad: Too terse, no context
tracing::warn!("Unknown type");

// Bad: Exposes implementation details without context
tracing::debug!("{:?}", internal_state);
```

### Adding Instrumentation

For key async functions, add `#[tracing::instrument]`:

```rust,ignore
// Good: Skip self to avoid logging entire struct, add meaningful fields
#[tracing::instrument(skip(self), fields(file_name = %file.name))]
pub async fn delete_file(&self, file: &File) -> Result<(), GenaiError>

// Good: Skip large arguments, record key identifiers
#[tracing::instrument(skip(self, request), fields(model = ?request.model))]
pub async fn execute(&self, request: InteractionRequest) -> Result<InteractionResponse, GenaiError>
```

## Integration with User Code

Users configure tracing using `tracing-subscriber` or compatible crates:

```rust,ignore
// Simple setup with tracing-subscriber
tracing_subscriber::fmt::init();

// With environment filter
tracing_subscriber::fmt()
    .with_env_filter("genai_rs=debug")
    .init();

// With JSON output for production
tracing_subscriber::fmt()
    .json()
    .init();

// With OpenTelemetry for distributed tracing
// See: https://docs.rs/tracing-opentelemetry
```

Log filtering by level:
```bash
RUST_LOG=genai_rs=debug cargo run --example simple_interaction
RUST_LOG=genai_rs=debug cargo run --example streaming
```

## Wire-Level Debugging with LOUD_WIRE

For zero-config debugging of raw API traffic, use the `LOUD_WIRE` environment variable:

```bash
LOUD_WIRE=1 cargo run --example simple_interaction
```

`LOUD_WIRE` is checked once, when the `Client` is constructed. Setting it
installs a `LoudWirePrinter` wire inspector on the client automatically —
it is sugar for the wire inspection API described below.

### Wire Inspection API

`LOUD_WIRE` output is built on the public `genai_rs::wire` module. Every wire
interaction — outgoing request (with JSON body), response status, response
body, **error response body**, SSE frame, and file upload — is surfaced as a
structured `wire::WireEvent`. Implement `wire::WireInspector` to observe them,
and register inspectors on the client builder (multiple allowed; each
receives every event):

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

Key properties:

- **Correlation**: every event carries a per-client request `id`; all events
  for one HTTP request share it (this is the `N` in `[REQ#N]`/`[RES#N]`).
- **Zero cost when unused**: with no inspectors installed, the library skips
  event construction entirely (request bodies are never serialized for
  inspection).
- **Synchronous**: inspectors run on the request path — keep them fast and
  non-blocking.

Built-in inspectors:

| Inspector | Behavior |
|-----------|----------|
| `wire::LoudWirePrinter` | Colored, pretty-printed stderr output (what `LOUD_WIRE=1` installs) |
| `wire::TracingForwarder` | Forwards events to `tracing` at `DEBUG` under target `genai_rs::wire` |

### Forwarding Wire Events to tracing

To route raw wire traffic through your existing `tracing` pipeline instead of
stderr, register a `TracingForwarder` and enable the `genai_rs::wire` target
(also exported as `wire::TRACING_TARGET`):

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
# Wire events only
RUST_LOG=genai_rs::wire=debug cargo run --example simple_interaction

# Library debug logs + wire events
RUST_LOG=genai_rs=debug,genai_rs::wire=debug cargo run --example simple_interaction
```

Events are emitted with structured fields (`kind`, `id`, `method`, `url`,
`status`, and the JSON `body` serialized as a string), so JSON subscribers
and log aggregators can filter and parse them.

### The wire-color Feature

Colored `LoudWirePrinter` output uses the `colored` and `colored_json`
crates behind the default-on `wire-color` feature. Build with
`default-features = false` to drop those dependencies; output falls back to
plain pretty-printed text.

### LOUD_WIRE vs RUST_LOG

| Feature | RUST_LOG | LOUD_WIRE |
|---------|----------|-----------|
| **Purpose** | Structured logging for all modules | Raw API traffic inspection |
| **Output** | Plain text to configured subscriber | Pretty-printed JSON to stderr |
| **Filtering** | Per-module level control | All-or-nothing |
| **Colors** | Depends on subscriber | Always (alternating for visual grouping, blue SSE) |
| **Base64 data** | Full content at debug level | Truncated to 100 chars |
| **Timestamps** | Depends on subscriber | Always included with request IDs |
| **SSE streaming** | Individual events at debug | Always included with correlation |

### When to Use Each

**Use RUST_LOG when:**
- Diagnosing internal library behavior
- Filtering specific modules or log levels
- Integrating with your application's logging pipeline
- Production debugging with controlled verbosity

**Use LOUD_WIRE when:**
- "What exactly is being sent to the API?"
- Debugging request/response mismatches
- Sharing API traces for bug reports
- Quick development iteration

### LOUD_WIRE Output Format

```text
[LOUD_WIRE] 2026-01-02T10:30:45Z [REQ#1] >>> POST https://...
[LOUD_WIRE] 2026-01-02T10:30:45Z [REQ#1] Body: {...}
[LOUD_WIRE] 2026-01-02T10:30:46Z [RES#1] <<< 200 OK
[LOUD_WIRE] 2026-01-02T10:30:46Z [RES#1] SSE: {...}
[LOUD_WIRE] 2026-01-02T10:30:47Z [REQ#2] >>> POST https://...
[LOUD_WIRE] 2026-01-02T10:30:47Z [REQ#2] Body: {...}
[LOUD_WIRE] 2026-01-02T10:30:48Z [RES#2] <<< 200 OK
```

**Key features:**
- **Request/response IDs** (`[REQ#N]`, `[RES#N]`) correlate requests with their responses
- **Alternating colors** make it easy to visually group request/response pairs:
  - Odd requests: Yellow `[REQ#1]`, Cyan `[RES#1]`
  - Even requests: Green `[REQ#2]`, Magenta `[RES#2]`
- **SSE chunks** are shown in blue for streaming responses
- **Error response bodies** are printed in full (`Error (429): {...}`), so
  failed requests are as inspectable as successful ones
- **Base64 data** is truncated: `"data": "AAAA..."`
- **File uploads** show progress: `>>> UPLOAD "video.mp4" (video/mp4, 150.25 MB)`

Request ids are per-`Client` (each client counts from `#1`).
