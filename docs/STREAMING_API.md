# Streaming API Guide

Streaming delivers a response as it is generated. Three methods return
streams:

| Method | Yields | Use for |
|--------|--------|---------|
| `InteractionBuilder::create_stream()` | `StreamEvent` | A single streamed interaction |
| `InteractionBuilder::create_stream_with_auto_functions()` | `AutoFunctionStreamEvent` | Streaming plus automatic function execution |
| `Client::get_interaction_stream(id, last_event_id)` | `StreamEvent` | Resuming a stream, or streaming a background interaction |

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
| `interaction.status_update` | `StatusUpdate { interaction_id, status }` | Background/agent progress |
| `step.start` | `StepStart { index, step }` | A step begins; it may be partial (a `FunctionCall` with empty arguments) |
| `step.delta` | `StepDelta { index, delta }` | Incremental payload for that step |
| `step.stop` | `StepStop { index, usage, step_usage }` | `usage` is cumulative; `step_usage` is this step's alone |
| `interaction.completed` | `Completed(response)` | Terminal: the full response |
| `error` | `Error { message, code }` | Terminal |
| anything else | `Unknown { chunk_type, data }` | Preserved (Evergreen) |

`chunk.is_terminal()` (also on the event) is true for `Completed` and
`Error`. `StreamEvent` has `is_delta()`, `is_complete()`, `interaction_id()`
and `status()` for the common checks.

**The `Completed` response is fully assembled.** The HTTP layer accumulates
`step.start` / `step.delta` / `step.stop`. If the completed payload omits the
steps, it fills them in, including parsing streamed `arguments_delta`
fragments into `Step::FunctionCall.arguments`. If the payload omits usage, it
falls back to the event's `metadata.total_usage`, then to the last cumulative
`step.stop` usage. So `response.as_text()`, `response.function_calls()` and
`response.usage` work on `Completed` exactly as they do after `create()`.
Accumulate deltas yourself only for live display.

### Step deltas

`StepDelta` (exported at the crate root, `#[non_exhaustive]`) is tagged on the
wire by `type`:

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
| `McpServerToolCall` / `McpServerToolResult` | `mcp_server_tool_*` | Spec-defined, never observed: MCP calls arrive whole on `step.start` as a generic `tool_call` step |
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

If the connection drops, resume the *same* generation with
`client.get_interaction_stream(interaction_id, Some(last_event_id))`. It
requests `GET /v1beta/interactions/{id}?alt=sse&last_event_id=…` (the id is
URL-encoded) and continues after that event.

Requirements:

1. The interaction is stored. Storage is on by default; `with_store_disabled()`
   rules resume out.
2. You have the interaction id, from the `Created` event or the response.
3. You have the last `event_id` you processed. `event_id` is **optional** per
   the API spec, so resume only works when the server sent one. Otherwise
   restart, or fetch the finished interaction with `get_interaction()`.

```rust,no_run
use futures_util::StreamExt;
use genai_rs::{Client, StreamChunk};

# async fn example() -> Result<(), genai_rs::GenaiError> {
# let client = Client::new("your-api-key".to_string());
// Initial stream
let mut last_event_id: Option<String> = None;
let mut interaction_id: Option<String> = None;

let mut stream = client.interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("Count to 100")
    .with_store_enabled()  // Required for resume
    .create_stream();

while let Some(result) = stream.next().await {
    let event = result?;

    // Track position for resume
    if event.event_id.is_some() {
        last_event_id = event.event_id.clone();
    }

    // Capture interaction ID from the Created event
    if let StreamChunk::Created { interaction } = &event.chunk {
        interaction_id = interaction.id.clone();
    }

    // Process event...
}

// If interrupted, resume from last position:
if let (Some(id), Some(last_evt)) = (&interaction_id, &last_event_id) {
    let mut resumed = client.get_interaction_stream(id, Some(last_evt));
    while let Some(result) = resumed.next().await {
        let _event = result?;
        // Continue processing from where we left off
    }
}
# Ok(())
# }
```

`get_interaction_stream(id, None)` streams a background interaction from the
start, which is how to watch a long agent run live.

## Timeouts

- `InteractionBuilder::with_timeout()` on a stream applies to **each gap
  between chunks**, not the whole stream. An elapsed gap yields
  `Err(GenaiError::Timeout)`.
- `ClientBuilder::with_timeout()` is a total-request timeout that also covers
  reading the streamed body, so it cuts long streams off. Leave it unset for
  streaming clients.

See [Reliability](RELIABILITY.md#timeouts).

## Forward compatibility

`StreamChunk`, `AutoFunctionStreamChunk` and `StepDelta` are
`#[non_exhaustive]`. Always include a wildcard arm. Unknown events carry the
original JSON (`unknown_chunk_type()`, `unknown_data()`); an unknown SSE
event type is logged at `debug`.

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
