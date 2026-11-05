# Streaming API Guide

Streaming delivers a response as it is generated. Three methods return
streams:

| Method | Yields | Use for |
|--------|--------|---------|
| `InteractionBuilder::create_stream()` | `StreamEvent` | A single streamed interaction |
| `InteractionBuilder::create_stream_with_auto_functions()` | `AutoFunctionStreamEvent` | Streaming plus automatic function execution |
| `client.interactions().stream(id)` / `.resume_stream(id, last_event_id)` | `StreamEvent` | Streaming a background interaction, or resuming a stream |

Each event is a wrapper holding `chunk` (what happened) and
`event_id: Option<String>` (the SSE event id, used to resume).

## Basic streaming

```rust,no_run
use futures_util::StreamExt;
use genai_rs::{Client, StreamChunk};

# async fn example() -> Result<(), genai_rs::GenaiError> {
let client = Client::new("your-api-key".to_string());

let mut stream = client.interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("Count to 10")
    .create_stream();

while let Some(result) = stream.next().await {
    let event = result?;
    if let Some(text) = event.chunk.delta_text() {
        print!("{text}");
    }
    if let StreamChunk::Completed(response) = &event.chunk {
        println!("\n\nDone! Tokens: {:?}", response.usage);
    }
}
# Ok(())
# }
```

`chunk.delta_text()` returns the fragment when the chunk is a `StepDelta`
carrying `StepDelta::Text`.

## Events

Under API revision 2026-05-20 the server emits this lifecycle:

| SSE event type | `StreamChunk` variant | Notes |
|----------------|------------------------|-------|
| `interaction.created` | `Created { interaction }` | First event; carries the interaction id |
| `interaction.status_update` | `StatusUpdate { interaction_id, status, usage, .. }` | Background/agent progress. `interaction_id` is `None` on a `store: false` stream; background streams carry `usage` mid-run |
| `step.start` | `StepStart { index, step }` | A step begins; it may be partial (a `FunctionCall` with empty arguments) |
| `step.delta` | `StepDelta { index, delta }` | Incremental payload for that step |
| `step.stop` | `StepStop { index, usage, step_usage }` | `usage` is cumulative; `step_usage` is this step's alone |
| `interaction.completed` | `Completed(response)` | Terminal: the full response |
| `error` | `Error { message, code }` | Terminal |
| anything else | `Unknown { chunk_type, data }` | Preserved (Evergreen) |

`chunk.is_terminal()` (also on the event) is true for `Completed` and
`Error`. `StreamEvent` has `is_delta()`, `is_complete()`, `interaction_id()`
and `status()` for the common checks.

### How a stream ends

- **At its terminal event.** The stream yields `Completed` or `Error` and
  ends there: nothing after it is read, and the connection is dropped. (The
  API follows `interaction.completed` with only `event: done` /
  `data: [DONE]`, verified live 2026-11-04.) So no late error, such as a
  reset while the server closes, can follow a finished interaction.
- **Cut off.** A body that ends before a terminal event yields
  `Err(GenaiError::StreamInterrupted)` last, whether the server closed the
  connection (between events or partway through one) or the transport
  failed mid-body; a transport failure is kept as its `source`. It is
  retryable; see [Cut-off streams](RELIABILITY.md#cut-off-streams).
- **On an unknown event.** A body that ends right after an event this crate
  doesn't recognize ends cleanly, with a `warn!`: it may be a new terminal
  event (Evergreen).

A successful response that is not `text/event-stream` (a captive portal's
page) is `GenaiError::MalformedResponse`, not a cut-off: no stream started.
Parameters such as `; charset=utf-8` and letter case don't matter, and a
response with no `Content-Type` is read as a stream. A proxy in front of the
API (`ClientBuilder::with_base_url`) must pass the type through or send
none.

### Event size limit

One SSE event may grow to 64 MiB before it ends; past that the stream fails
with `GenaiError::Parse`, so a server that never sends a newline can't grow
the buffer without bound. The largest events measured live (2026-11-04,
`DEFAULT_IMAGE_MODEL` at `image_size: "4K"`) are a 16.0 MB `image` delta of
base64 JPEG and a 15.7 MB `thought_signature` delta in the same stream; TTS
audio deltas are under 8 KB. The limit leaves four times the largest.

**The `Completed` response is fully assembled.** The HTTP layer accumulates
`step.start` / `step.delta` / `step.stop`. If the completed payload omits the
steps, it fills them in, including parsing streamed `arguments_delta`
fragments into `Step::FunctionCall.arguments`. If the payload omits usage, it
falls back to the event's `metadata.total_usage`, then to the last cumulative
`step.stop` usage. So `response.as_text()`, `response.function_calls()` and
`response.usage` work on `Completed` exactly as they do after `create()`.
Accumulate deltas yourself only for live display.

Each streamed image arrives whole in one `image` delta that names its
`mime_type`, and each image came in its own `model_output` step, after a
`thought` step and before any text in that step (verified 2026-11-04,
`gemini-3.1-flash-image`, three streams, one with four images). An image
delta that names a `mime_type` or `uri` starts a new `Content::Image` block,
so two images in one step would also stay two blocks; that shape has not
been observed. Audio, video and document data deltas concatenate into the
last block of their kind. Fields a delta carries that the crate does not
model land in the `extra` map of the block or step it builds, so the
assembled steps replay as history exactly as a non-streamed response's
would.

### Step deltas

`StepDelta` (exported at the crate root, `#[non_exhaustive]`) is tagged on the
wire by `type`. Every known variant also has an `extra` map of unmodeled
fields (`delta.extra()`), and the call deltas an `arguments_extra`; match with
`..`:

| Variant | Wire `type` | Payload |
|---------|-------------|---------|
| `Text { text }` | `text` | Text fragment |
| `ThoughtSummary { content }` | `thought_summary` | `Option<Content>` summary block |
| `ThoughtSignature { signature }` | `thought_signature` | Opaque signature fragment |
| `TextAnnotation { annotations }` | `text_annotation_delta` | Citations for previously streamed text |
| `ArgumentsDelta { arguments }` | `arguments_delta` | Raw JSON fragment of function-call arguments |
| `Image` / `Audio` / `Video` / `Document` | `image` / `audio` / ... | Media data |
| `FunctionResult { .. }` | `function_result` | Function result payload |
| `CodeExecutionCall` / `CodeExecutionResult` | `code_execution_*` | Server-side code execution |
| `UrlContextCall` / `UrlContextResult` | `url_context_*` | URL context |
| `GoogleSearchCall` / `GoogleSearchResult` | `google_search_*` | Google Search |
| `FileSearchCall` / `FileSearchResult` | `file_search_*` | File Search |
| `GoogleMapsCall` / `GoogleMapsResult` | `google_maps_*` | Google Maps |
| `ProcessingCall` / `ProcessingResult` | `processing_*` | Media processing for agentic video. Carries the signature that `step.start` announces as `""`; the assembled `Completed` response keeps it |
| `RetrievalCall` / `RetrievalResult` | `retrieval_*` | Retrieval tool (Vertex-only) |
| `McpServerToolCall` / `McpServerToolResult` | `mcp_server_tool_*` | Spec-defined, never observed. MCP calls arrive as `function_call` / `function_result` steps named `<server>:<tool>` (since 2026-10-01; earlier as a generic `tool_call` step) |
| `Unknown { delta_type, data }` | anything else | Preserved for forward compatibility |

Helpers: `delta.as_text()`, `delta.as_arguments_delta()`, and the Evergreen
trio `is_unknown()` / `unknown_delta_type()` / `unknown_data()`.

## Streaming function calls

When the model calls a function:

1. `step.start` announces a `Step::FunctionCall` with its `id` and `name` and
   empty arguments.
2. `step.delta` events carry `StepDelta::ArgumentsDelta { arguments }`, raw
   JSON **string fragments**.
3. `step.stop` closes the step.

```rust,no_run
use futures_util::StreamExt;
use genai_rs::{Client, Step, StreamChunk};

# async fn example(functions: Vec<genai_rs::FunctionDeclaration>) -> Result<(), genai_rs::GenaiError> {
let client = Client::new("your-api-key".to_string());

let mut stream = client.interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("What's the weather in Tokyo?")
    .add_functions(functions)
    .create_stream();

let mut streamed_args = String::new();

while let Some(result) = stream.next().await {
    let event = result?;
    match &event.chunk {
        StreamChunk::StepStart { step, .. } => {
            if let Step::FunctionCall { name, .. } = step {
                println!("[Function call starting: {name}]");
            }
        }
        StreamChunk::StepDelta { delta, .. } => {
            // Arguments arrive incrementally as raw JSON fragments
            if let Some(fragment) = delta.as_arguments_delta() {
                streamed_args.push_str(fragment);
            }
        }
        StreamChunk::Completed(response) => {
            // The streamed steps were assembled into the final response,
            // including the parsed function-call arguments:
            for call in response.function_calls() {
                println!("Call: {}({}) [id={}]", call.name, call.args, call.id);
            }
        }
        _ => {}
    }
}
# Ok(())
# }
```

To have the functions executed for you, use
`create_stream_with_auto_functions()`.

## Auto-function streaming

Streams step deltas in real time while functions run between rounds. It uses
the same discovery, round limit and storage requirement as
`create_with_auto_functions()` (see
[Function Calling](FUNCTION_CALLING.md#automatic-execution-create_with_auto_functions)).

```rust,no_run
use futures_util::StreamExt;
use genai_rs::{AutoFunctionStreamChunk, Client};

# async fn example() -> Result<(), genai_rs::GenaiError> {
let client = Client::new("your-api-key".to_string());

// Functions are auto-discovered from the #[tool] registry / tool service.
let mut stream = client.interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("What's the weather in Tokyo?")
    .create_stream_with_auto_functions();

while let Some(result) = stream.next().await {
    let event = result?;

    match &event.chunk {
        AutoFunctionStreamChunk::Delta(delta) => {
            if let Some(text) = delta.as_text() {
                print!("{text}");
            }
        }
        AutoFunctionStreamChunk::ExecutingFunctions { pending_calls, .. } => {
            for call in pending_calls {
                println!("\n[Executing: {}({})]", call.name, call.args);
            }
        }
        AutoFunctionStreamChunk::FunctionResults(results) => {
            for r in results {
                println!("  {} took {:?}", r.name, r.duration);
            }
        }
        AutoFunctionStreamChunk::Complete(_response) => {
            println!("\n[Done]");
        }
        _ => {}
    }
}
# Ok(())
# }
```

| `AutoFunctionStreamChunk` | Meaning |
|---------------------------|---------|
| `Delta(StepDelta)` | Incremental payload from the model, the same type as basic streaming |
| `ExecutingFunctions { response, pending_calls }` | Calls detected; about to execute |
| `FunctionResults(Vec<FunctionExecutionResult>)` | Execution finished for this round |
| `Complete(response)` | Terminal: no more function calls |
| `MaxLoopsReached(response)` | Terminal: the round limit was hit |
| `Unknown { chunk_type, data }` | Forward compatibility |

`ExecutingFunctions` and `FunctionResults` are generated client-side, so
their `event_id` is always `None`. Only track ids from API events.

To end up with the same `AutoFunctionResult` as the non-streaming call, feed
every chunk to an accumulator:

```rust,no_run
use futures_util::StreamExt;
use genai_rs::{AutoFunctionResultAccumulator, AutoFunctionStreamChunk, Client};

# async fn example() -> Result<(), genai_rs::GenaiError> {
# let client = Client::new("your-api-key".to_string());
# let mut stream = client.interaction()
#     .with_model(genai_rs::DEFAULT_MODEL)
#     .with_text("What's the weather in Tokyo?")
#     .create_stream_with_auto_functions();
let mut accumulator = AutoFunctionResultAccumulator::new();

while let Some(event) = stream.next().await {
    let event = event?;

    // Process deltas for UI
    if let AutoFunctionStreamChunk::Delta(delta) = &event.chunk {
        if let Some(text) = delta.as_text() {
            print!("{text}");
        }
    }

    // Feed all chunks to accumulator
    if let Some(result) = accumulator.push(event.chunk) {
        // Stream complete - result has same shape as create_with_auto_functions()
        println!("Executed {} functions", result.executions.len());
        println!("Final text: {:?}", result.response.as_text());
    }
}
# Ok(())
# }
```

## Stream resume

Only **background** interactions can be resumed (verified live 2026-11-01).
A foreground stream from `create_stream()` carries no `event_id`s, and when
its connection drops the server cancels the interaction: it keeps only the
input step, reports `status: cancelled` with an `errors` entry, and
`interactions().stream(id)` answers `400 Streaming retrieval of
interactions is not supported`. Retrying means a new request.

For a background interaction, follow or resume the *same* generation with
`client.interactions().resume_stream(interaction_id, last_event_id)`. It
requests `GET /v1beta/interactions/{id}?alt=sse&last_event_id=…` (the id is
URL-encoded) and continues after that event.

Requirements:

1. The interaction runs in the background (`with_background(true)`) and is
   stored (on by default).
2. You have the interaction id, from the create response.
3. You have the last `event_id` you processed. Only some events carry one
   (text deltas did, live), so resume from the last you saw. Otherwise
   follow from the start with `client.interactions().stream(id)`, or fetch
   the finished interaction with `client.interactions().get(id)`.

```rust,no_run
use futures_util::StreamExt;
use genai_rs::{Client, StreamChunk};

# async fn example() -> Result<(), genai_rs::GenaiError> {
# let client = Client::new("your-api-key".to_string());
// Initial stream
let mut last_event_id: Option<String> = None;
let mut interaction_id: Option<String> = None;

let created = client.interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("Count to 100")
    .with_background(true)  // Required for resume
    .create()
    .await?;
interaction_id = created.id.clone();

let mut stream = client.interactions().stream(interaction_id.as_deref().unwrap_or_default());

while let Some(result) = stream.next().await {
    let event = result?;

    // Track position for resume
    if event.event_id.is_some() {
        last_event_id = event.event_id.clone();
    }

    // Process event...
    if let StreamChunk::StepDelta { delta, .. } = &event.chunk {
        print!("{}", delta.as_text().unwrap_or_default());
    }
}

// If interrupted, resume from last position:
if let (Some(id), Some(last_evt)) = (&interaction_id, &last_event_id) {
    let mut resumed = client.interactions().resume_stream(id, last_evt);
    while let Some(result) = resumed.next().await {
        let _event = result?;
        // Continue processing from where we left off
    }
}
# Ok(())
# }
```

`client.interactions().stream(id)` streams a background interaction from the
start, which is how to watch a long agent run live.

## Timeouts

- `InteractionBuilder::with_timeout()` on a stream applies to **each gap
  between chunks**, not the whole stream. An elapsed gap yields
  `Err(GenaiError::Timeout)`.
- `ClientBuilder::with_timeout()` is a total-request timeout that also covers
  reading the streamed body, so it cuts long streams off. Leave it unset for
  streaming clients.
- `ClientBuilder::with_read_timeout()` bounds the wait for the headers and
  for each next chunk, so it ends a stalled connection without capping a
  healthy long stream. It is the only client-level bound that reaches
  `interactions().stream()` and `resume_stream()`. A stall mid-stream ends
  in `GenaiError::StreamInterrupted` with the timed-out `reqwest::Error` as
  its `source`.
- None of these is set by default; only the 30 s connect timeout is. See
  [Why only a connect timeout by default](RELIABILITY.md#why-only-a-connect-timeout-by-default).

See [Reliability](RELIABILITY.md#timeouts).

## Forward compatibility

`StreamChunk`, `AutoFunctionStreamChunk` and `StepDelta` are
`#[non_exhaustive]`. Always include a wildcard arm. Unknown events carry the
original JSON (`unknown_chunk_type()`, `unknown_data()`); an unknown SSE
event type is logged at `warn`.

## Examples

| Example | Features |
|---------|----------|
| `streaming` | Basic streaming, event types, resume |
| `streaming_auto_functions` | Auto-function streaming |
| `thinking` | Streaming thought summaries |
| `deep_research` | Long-running background interaction |

`LOUD_WIRE=1 cargo run --example streaming` prints every SSE frame; see
[Logging Strategy](LOGGING_STRATEGY.md#loud_wire-output-format) for the
format.
