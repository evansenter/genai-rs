# Streaming API Guide

This guide covers everything you need to know about streaming responses and stream resumption in `genai-rs`. It explains the types, patterns, trade-offs, and design decisions behind the streaming API.

## Table of Contents

- [Overview](#overview)
- [Streaming vs Non-Streaming](#streaming-vs-non-streaming)
- [Type Hierarchy](#type-hierarchy)
- [Event Types and Lifecycle](#event-types-and-lifecycle)
- [Step Deltas](#step-deltas)
- [Per-Step Usage](#per-step-usage)
- [Basic Streaming](#basic-streaming)
- [Streaming Function Calls](#streaming-function-calls)
- [Auto-Function Streaming](#auto-function-streaming)
- [Stream Resume](#stream-resume)
- [Accessor Consistency](#accessor-consistency)
- [Forward Compatibility](#forward-compatibility)
- [Design Patterns](#design-patterns)
- [Decision Matrix](#decision-matrix)
- [Examples Reference](#examples-reference)

## Overview

Streaming allows you to receive responses incrementally as they're generated, rather than waiting for the complete response. This is essential for:

1. **Real-time UX**: Display text as it arrives for perceived responsiveness
2. **Long responses**: Process large outputs without waiting for completion
3. **Resource efficiency**: Start processing early, reduce memory for buffering
4. **Resilience**: Resume interrupted streams using `event_id`

The key decision points are:

1. **Streaming mode**: Basic (`create_stream`) vs auto-function (`create_stream_with_auto_functions`)
2. **Resume capability**: Whether to track `event_id` for stream resumption

## Streaming vs Non-Streaming

### Non-Streaming (`create()`)

```rust,no_run
# use genai_rs::Client;
# async fn example() -> Result<(), genai_rs::GenaiError> {
# let client = Client::new("your-api-key".to_string());
let response = client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Write a poem about Rust")
    .create()
    .await?;

// All content available at once
println!("{}", response.as_text().unwrap_or_default());
# Ok(())
# }
```

**Characteristics:**
- Simple API - returns `InteractionResponse` directly
- Waits for complete response before returning
- Best for short responses or when streaming isn't needed

### Basic Streaming (`create_stream()`)

```rust,no_run
use futures_util::StreamExt;
use genai_rs::{Client, StreamChunk};

# async fn example() -> Result<(), genai_rs::GenaiError> {
# let client = Client::new("your-api-key".to_string());
let mut stream = client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Write a poem about Rust")
    .create_stream();

while let Some(result) = stream.next().await {
    let event = result?;
    if let StreamChunk::StepDelta { delta, .. } = &event.chunk {
        if let Some(text) = delta.as_text() {
            print!("{text}");
        }
    }
}
# Ok(())
# }
```

**Characteristics:**
- Returns `Stream<StreamEvent>` for incremental processing
- Each `StreamEvent` contains a `chunk` and optional `event_id`
- Events arrive as steps are generated (`step.start` / `step.delta` / `step.stop`)
- Final `Completed` event contains the full response with usage metadata

### When to Use Each

| Use Case | Non-Streaming | Streaming |
|----------|---------------|-----------|
| Short responses | ✅ | |
| Real-time display (chat UI) | | ✅ |
| Long-form content | | ✅ |
| Need resume capability | | ✅ |
| Simple code preferred | ✅ | |
| Background/batch processing | ✅ | |

## Type Hierarchy

The streaming API has a layered type design with consistent naming:

### Basic Streaming Types

```text
StreamEvent                    # Wrapper with position metadata
├── chunk: StreamChunk         # The actual content/event
└── event_id: Option<String>   # For stream resumption
```

### Auto-Function Streaming Types

```text
AutoFunctionStreamEvent        # Wrapper with position metadata
├── chunk: AutoFunctionStreamChunk  # Step deltas + function lifecycle
└── event_id: Option<String>   # For stream resumption (API events only)
```

### Design Rationale

The `*Event` wrapper types serve two purposes:

1. **Position tracking**: `event_id` enables stream resumption
2. **Clean separation**: Chunk contains content, Event contains metadata

This mirrors how SSE works: each Server-Sent Event has an optional `id` field separate from the data payload.

### Naming Conventions

| Suffix | Meaning | Example |
|--------|---------|---------|
| `*Chunk` | Content/data enum (what happened) | `StreamChunk`, `AutoFunctionStreamChunk` |
| `*Event` | Wrapper with metadata (chunk + event_id) | `StreamEvent`, `AutoFunctionStreamEvent` |
| `*_stream()` | Returns `Stream<*Event>` | `create_stream()`, `get_interaction_stream()` |

## Event Types and Lifecycle

### SSE Lifecycle Events

Under API revision 2026-05-20 the server emits this lifecycle:

| SSE event type | `StreamChunk` variant | Notes |
|----------------|------------------------|-------|
| `interaction.created` | `Created { interaction }` | First event; contains the interaction ID |
| `interaction.status_update` | `StatusUpdate { interaction_id, status }` | Background/agent progress |
| `step.start` | `StepStart { index, step }` | A step begins at an output index |
| `step.delta` | `StepDelta { index, delta }` | Incremental payload for that step |
| `step.stop` | `StepStop { index, usage, step_usage }` | Step finished; carries per-step usage |
| `interaction.completed` | `Completed(response)` | Terminal; full response |
| `error` | `Error { message, code }` | Terminal |

The stream terminates on `interaction.completed` or `error` - no more events follow either.

### StreamChunk Variants

```rust,ignore
#[non_exhaustive]
pub enum StreamChunk {
    /// Interaction created (first event, provides early access to ID).
    /// The payload is a partial interaction resource.
    Created { interaction: InteractionResponse },

    /// Status change during processing (for background/agent interactions)
    StatusUpdate { interaction_id: String, status: InteractionStatus },

    /// A step started at the given index. The step may be partially
    /// populated (e.g. a FunctionCall step with empty arguments) -
    /// deltas fill it in.
    StepStart { index: usize, step: Step },

    /// Incremental payload for the step at the given index
    /// (text fragment, arguments_delta, thought signature, ...)
    StepDelta { index: usize, delta: StepDelta },

    /// The step at the given index finished.
    /// `usage` is cumulative interaction usage; `step_usage` is
    /// attributable to this step alone.
    StepStop { index: usize, usage: Option<UsageMetadata>, step_usage: Option<UsageMetadata> },

    /// Final complete response (terminal)
    Completed(InteractionResponse),

    /// Error occurred (terminal)
    Error { message: String, code: Option<String> },

    /// Unknown type (forward compatibility)
    Unknown { chunk_type: String, data: serde_json::Value },
}
```

The convenience helper `chunk.delta_text() -> Option<&str>` returns the text fragment when the chunk is a `StepDelta` carrying `StepDelta::Text` - handy for the common "print streaming text" loop.

### Typical Event Sequence

```text
Created         →  Interaction accepted, ID available
StepStart       →  Step 0 starting (e.g. a model_output step)
StepDelta       →  "The "
StepDelta       →  "answer "
StepDelta       →  "is "
StepDelta       →  "42."
StepStop        →  Step 0 complete (usage / step_usage)
Completed       →  Full response with assembled steps and usage metadata
```

### Terminal Events

`Completed` and `Error` are terminal events - no more events will follow:

```rust,ignore
if event.is_terminal() {
    break;  // Stream has ended
}
```

### The Completed Response Is Fully Assembled

The HTTP layer accumulates `step.start` / `step.delta` / `step.stop` events as they arrive. If the server's `interaction.completed` payload omits the steps (because they were already delivered incrementally), the accumulated steps are filled into the final `Completed(response)` - including parsing streamed `arguments_delta` fragments into `Step::FunctionCall.arguments`.

This means `response.function_calls()`, `response.as_text()`, and the other step accessors work on the `Completed` response exactly as they do for non-streaming `create()`. You do not need to accumulate deltas yourself unless you want incremental display.

### AutoFunctionStreamChunk Variants

For auto-function streaming, additional variants track function lifecycle:

```rust,ignore
#[non_exhaustive]
pub enum AutoFunctionStreamChunk {
    /// Incremental step payload from the model (text fragments,
    /// arguments_delta fragments, thought summaries/signatures, ...)
    Delta(StepDelta),

    /// Function calls detected, about to execute.
    /// `pending_calls` contains the validated function calls that will be executed.
    ExecutingFunctions {
        response: InteractionResponse,
        pending_calls: Vec<PendingFunctionCall>,
    },

    /// Function execution completed with results
    FunctionResults(Vec<FunctionExecutionResult>),

    /// Final response (no more function calls) - terminal
    Complete(InteractionResponse),

    /// Max function iterations reached - terminal
    MaxLoopsReached(InteractionResponse),

    /// Unknown type (forward compatibility)
    Unknown { chunk_type: String, data: serde_json::Value },
}
```

## Step Deltas

`StepDelta` (exported at the crate root) is the incremental payload carried by `step.delta` events. It is `#[non_exhaustive]` and tagged on the wire by `type`:

| Variant | Wire `type` | Payload |
|---------|-------------|---------|
| `Text { text }` | `text` | Text fragment |
| `ThoughtSummary { content }` | `thought_summary` | `Option<Content>` summary block |
| `ThoughtSignature { signature }` | `thought_signature` | Opaque signature fragment |
| `TextAnnotation { annotations }` | `text_annotation_delta` | Citations for previously streamed text |
| `ArgumentsDelta { arguments }` | `arguments_delta` | Raw JSON fragment of function-call arguments |
| `Image` / `Audio` / `Video` / `Document` | `image` / `audio` / ... | Media data (base64 `data`, `uri`, `mime_type`, ...) |
| `FunctionResult { call_id, name, result, is_error }` | `function_result` | Function result payload |
| `CodeExecutionCall` / `CodeExecutionResult` | `code_execution_*` | Server-side code execution |
| `UrlContextCall` / `UrlContextResult` | `url_context_*` | URL context tool |
| `GoogleSearchCall` / `GoogleSearchResult` | `google_search_*` | Google Search tool |
| `McpServerToolCall` / `McpServerToolResult` | `mcp_server_tool_*` | MCP server tools |
| `FileSearchCall` / `FileSearchResult` | `file_search_*` | File search tool |
| `GoogleMapsCall` / `GoogleMapsResult` | `google_maps_*` | Google Maps tool |
| `Unknown { delta_type, data }` | anything else | Preserved for forward compatibility |

**Helpers:**

- `delta.as_text() -> Option<&str>` - the text fragment for `Text` deltas
- `delta.as_arguments_delta() -> Option<&str>` - the raw JSON fragment for `ArgumentsDelta`
- `delta.is_unknown()` / `delta.unknown_delta_type()` / `delta.unknown_data()` - Evergreen unknown handling

## Per-Step Usage

Token usage is reported at two granularities during streaming:

1. **`StepStop { usage, step_usage, .. }`**: each `step.stop` event may carry
   - `usage`: cumulative interaction usage so far
   - `step_usage`: usage attributable to that step alone
2. **Final total**: the `interaction.completed` event may carry the total in the interaction payload itself (`response.usage`), or in the event's metadata (`InteractionStreamEvent.metadata: StreamMetadata { total_usage }`). The HTTP layer copies `metadata.total_usage` into the final `Completed(response).usage` when the interaction payload omits it, so consumers can always read usage from the `Completed` response.

```rust,ignore
match &event.chunk {
    StreamChunk::StepStop { index, usage, step_usage } => {
        println!("step {index} done: step={step_usage:?} cumulative={usage:?}");
    }
    StreamChunk::Completed(response) => {
        println!("total usage: {:?}", response.usage);
    }
    _ => {}
}
```

## Basic Streaming

### Minimal Example

```rust,no_run
use futures_util::StreamExt;
use genai_rs::{Client, StreamChunk};

# async fn example() -> Result<(), genai_rs::GenaiError> {
let client = Client::new("your-api-key".to_string());

let mut stream = client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Count to 10")
    .create_stream();

while let Some(result) = stream.next().await {
    let event = result?;
    match &event.chunk {
        StreamChunk::StepDelta { delta, .. } => {
            if let Some(text) = delta.as_text() {
                print!("{text}");
            }
        }
        StreamChunk::Completed(response) => {
            println!("\n\nDone! Tokens: {:?}", response.usage);
        }
        _ => {}  // Handle unknown future variants
    }
}
# Ok(())
# }
```

For the common text-only loop, `event.chunk.delta_text()` collapses the first match arm:

```rust,ignore
if let Some(text) = event.chunk.delta_text() {
    print!("{text}");
}
```

### Complete Event Handling

```rust,ignore
while let Some(result) = stream.next().await {
    let event = result?;

    // Track event_id for potential resume
    if event.event_id.is_some() {
        last_event_id = event.event_id.clone();
    }

    match &event.chunk {
        StreamChunk::Created { interaction } => {
            interaction_id = interaction.id.clone();
            eprintln!("[Created] ID={:?}", interaction.id);
        }
        StreamChunk::StatusUpdate { status, .. } => {
            eprintln!("[Status] {:?}", status);
        }
        StreamChunk::StepStart { index, step } => {
            eprintln!("[StepStart] index={}, type={}", index, step.step_type());
        }
        StreamChunk::StepDelta { delta, .. } => {
            if let Some(text) = delta.as_text() {
                print!("{}", text);
            }
            if let StepDelta::ThoughtSummary { content: Some(c) } = delta {
                eprintln!("[Thought] {:?}", c.as_text());
            }
        }
        StreamChunk::StepStop { index, usage, step_usage } => {
            eprintln!("[StepStop] index={}, step_usage={:?}, cumulative={:?}",
                index, step_usage, usage);
        }
        StreamChunk::Completed(response) => {
            println!("\n[Completed] Tokens: {:?}", response.usage);
        }
        StreamChunk::Error { message, code } => {
            eprintln!("[Error] {} (code: {:?})", message, code);
            break;
        }
        _ => {
            eprintln!("[Unknown] New event type");
        }
    }
}
```

## Streaming Function Calls

Function calls now stream incrementally. When the model decides to call a function:

1. `step.start` announces a `Step::FunctionCall` with the call `id` and `name` (arguments still empty)
2. `step.delta` events carry `StepDelta::ArgumentsDelta { arguments }` - raw JSON **string fragments** of the function-call arguments
3. `step.stop` closes the step

Concatenating the fragments yields the complete arguments JSON. You can do this yourself for live display, but you usually don't have to: the HTTP layer assembles the streamed steps (including parsing accumulated `arguments_delta` fragments into `FunctionCall.arguments`) into the final `Completed` response, so **`response.function_calls()` works after streaming** just like in non-streaming mode.

```rust,no_run
use futures_util::StreamExt;
use genai_rs::{Client, Step, StreamChunk};

# async fn example(functions: Vec<genai_rs::FunctionDeclaration>) -> Result<(), genai_rs::GenaiError> {
let client = Client::new("your-api-key".to_string());

let mut stream = client.interaction()
    .with_model("gemini-3-flash-preview")
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

**Recommendation:** If you want the functions executed for you, use `create_stream_with_auto_functions()` instead, which detects calls, executes your registered functions, and loops until the model finishes. See [Auto-Function Streaming](#auto-function-streaming).

## Auto-Function Streaming

Combines streaming with automatic function execution. Step deltas are streamed in real-time while functions execute between streaming rounds.

### Basic Usage

```rust,no_run
use futures_util::StreamExt;
use genai_rs::{AutoFunctionStreamChunk, Client};

# async fn example() -> Result<(), genai_rs::GenaiError> {
let client = Client::new("your-api-key".to_string());

// Functions are auto-discovered from the #[tool] registry / tool service.
let mut stream = client.interaction()
    .with_model("gemini-3-flash-preview")
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

Note that `AutoFunctionStreamChunk::Delta` carries a `StepDelta` - the same incremental payload type as basic streaming - so text fragments, `arguments_delta` fragments, and thought summaries/signatures all flow through it.

### Event ID Behavior

> **Note**: Per the [Interactions API spec](https://ai.google.dev/api/interactions-api#Resource:InteractionSseEvent),
> `event_id` is **optional** on all SSE event types. The API may or may not include it.

- **API events** (`Delta`, `Complete`): May include `event_id` for resume (optional per spec)
- **Client events** (`ExecutingFunctions`, `FunctionResults`): `event_id` is `None`

Client-generated events don't come from the SSE stream, so they have no event ID.

```rust,ignore
// Track only API events for resume
if event.event_id.is_some() {
    last_event_id = event.event_id.clone();
}
```

### Using AutoFunctionResultAccumulator

Convert a stream into the same result type as non-streaming `create_with_auto_functions()`:

```rust,no_run
use futures_util::StreamExt;
use genai_rs::{AutoFunctionResultAccumulator, AutoFunctionStreamChunk, Client};

# async fn example() -> Result<(), genai_rs::GenaiError> {
# let client = Client::new("your-api-key".to_string());
# let mut stream = client.interaction()
#     .with_model("gemini-3-flash-preview")
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

## Stream Resume

The streaming API supports resuming interrupted streams using `event_id`.

> **Note**: `event_id` is optional per the API spec. Stream resume only works when the API
> provides event IDs. If the API doesn't return them, you'll need alternative recovery
> strategies (e.g., restart from beginning, use stored interaction replay).

### How It Works

1. Each streaming event may include an `event_id` (optional per API spec)
2. Track the last received `event_id` as events arrive (when present)
3. If connection drops, call `get_interaction_stream()` with the saved ID
4. Stream resumes from after that event

### Resume Pattern

```rust,no_run
use futures_util::StreamExt;
use genai_rs::{Client, StreamChunk};

# async fn example() -> Result<(), genai_rs::GenaiError> {
# let client = Client::new("your-api-key".to_string());
// Initial stream
let mut last_event_id: Option<String> = None;
let mut interaction_id: Option<String> = None;

let mut stream = client.interaction()
    .with_model("gemini-3-flash-preview")
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

### Requirements for Resume

1. **Store enabled**: Interaction must be stored (`with_store_enabled()`)
2. **Interaction ID**: Need the interaction ID (from the `Created` event or response)
3. **Event ID**: Need the last successfully processed event's ID

### get_interaction_stream()

```rust,ignore
/// Retrieves an existing interaction by its ID with streaming.
pub fn get_interaction_stream(
    &self,
    interaction_id: &str,
    last_event_id: Option<&str>,  // Resume point
) -> BoxStream<'_, Result<StreamEvent, GenaiError>>
```

**Use cases:**
- Resuming an interrupted stream
- Streaming a long-running background interaction (e.g., deep research)
- Re-streaming an interaction for replay/debugging

### URL Construction

The `last_event_id` is URL-encoded and passed as a query parameter:

```text
GET /v1beta/interactions/{id}?alt=sse&last_event_id={encoded_id}
```

Special characters in event IDs are properly escaped (e.g., `+` → `%2B`).

## Accessor Consistency

The wrapper types (`StreamEvent`, `AutoFunctionStreamEvent`) delegate accessors to their inner chunk types for convenience.

### StreamEvent Accessors

| Method | Delegates To | Description |
|--------|--------------|-------------|
| `is_delta()` | matches chunk | Check if `StepDelta` variant |
| `is_complete()` | matches chunk | Check if `Completed` variant |
| `is_unknown()` | `chunk.is_unknown()` | Check if Unknown variant |
| `is_terminal()` | `chunk.is_terminal()` | Check if Completed or Error |
| `interaction_id()` | `chunk.interaction_id()` | Get ID (Created/StatusUpdate/Completed) |
| `status()` | `chunk.status()` | Get status (Created/StatusUpdate/Completed) |
| `unknown_chunk_type()` | `chunk.unknown_chunk_type()` | Get unknown type name |
| `unknown_data()` | `chunk.unknown_data()` | Get preserved JSON data |

The text helper lives on the chunk itself: `event.chunk.delta_text()`.

### AutoFunctionStreamEvent Accessors

| Method | Delegates To | Description |
|--------|--------------|-------------|
| `is_delta()` | `chunk.is_delta()` | Check if Delta variant |
| `is_complete()` | `chunk.is_complete()` | Check if Complete variant |
| `is_unknown()` | `chunk.is_unknown()` | Check if Unknown variant |
| `unknown_chunk_type()` | `chunk.unknown_chunk_type()` | Get unknown type name |
| `unknown_data()` | `chunk.unknown_data()` | Get preserved JSON data |

### Design Principle

Accessors are delegated when:
1. They're commonly needed without destructuring
2. The chunk field is public, so direct access is always possible
3. The delegation is obvious and adds convenience

## Forward Compatibility

Following the [Evergreen](https://github.com/google-deepmind/evergreen-spec) philosophy, the streaming types handle unknown data gracefully.

### Non-Exhaustive Enums

All chunk enums (and `StepDelta`) use `#[non_exhaustive]`:

```rust,ignore
#[non_exhaustive]
pub enum StreamChunk {
    // Known variants...
    Unknown { chunk_type: String, data: serde_json::Value },
}
```

Always include a wildcard arm in match statements:

```rust,ignore
match &event.chunk {
    StreamChunk::StepDelta { .. } => { /* ... */ }
    StreamChunk::Completed(_) => { /* ... */ }
    _ => {
        // Handle unknown future variants
        if let Some(chunk_type) = event.unknown_chunk_type() {
            log::warn!("Unknown chunk type: {}", chunk_type);
        }
    }
}
```

### Unknown Variant Pattern

Unknown variants preserve data for debugging and roundtrip serialization:

```rust,ignore
Unknown {
    /// The unrecognized type from the API
    chunk_type: String,
    /// The full JSON data, preserved for debugging
    data: serde_json::Value,
}
```

Access with helper methods:
- `is_unknown()` - Check if unknown
- `unknown_chunk_type()` - Get the type name (for `StepDelta`: `unknown_delta_type()`)
- `unknown_data()` - Get the preserved JSON

### Logging Unknown Events

Unknown events log at `warn` level to surface API evolution:

```rust,ignore
log::warn!(
    "Encountered unknown StreamChunk type '{}'. \
     This may indicate a new API feature.",
    chunk_type
);
```

## Design Patterns

### Pattern 1: Streaming with Progress Tracking

```rust,ignore
struct StreamingSession {
    client: Client,
    last_event_id: Option<String>,
    interaction_id: Option<String>,
    total_chars: usize,
}

impl StreamingSession {
    async fn stream(&mut self, prompt: &str) -> Result<String, Error> {
        let mut full_text = String::new();

        let mut stream = self.client.interaction()
            .with_model("gemini-3-flash-preview")
            .with_text(prompt)
            .with_store_enabled()
            .create_stream();

        while let Some(result) = stream.next().await {
            let event = result?;

            // Track for resume
            if event.event_id.is_some() {
                self.last_event_id = event.event_id.clone();
            }

            match &event.chunk {
                StreamChunk::Created { interaction } => {
                    self.interaction_id = interaction.id.clone();
                }
                StreamChunk::StepDelta { delta, .. } => {
                    if let Some(text) = delta.as_text() {
                        full_text.push_str(text);
                        self.total_chars += text.len();
                    }
                }
                _ => {}
            }
        }

        Ok(full_text)
    }

    async fn resume(&self) -> Result<impl Stream<Item = Result<StreamEvent, Error>>, Error> {
        let id = self.interaction_id.as_ref().ok_or("No interaction to resume")?;
        Ok(self.client.get_interaction_stream(id, self.last_event_id.as_deref()))
    }
}
```

### Pattern 2: Buffered UI Updates

For high-frequency deltas, buffer before updating UI:

```rust,ignore
use std::time::{Duration, Instant};

let mut buffer = String::new();
let mut last_flush = Instant::now();
const FLUSH_INTERVAL: Duration = Duration::from_millis(50);

while let Some(event) = stream.next().await {
    let event = event?;

    if let Some(text) = event.chunk.delta_text() {
        buffer.push_str(text);
    }

    // Flush periodically or on terminal events
    if last_flush.elapsed() >= FLUSH_INTERVAL || event.is_terminal() {
        if !buffer.is_empty() {
            update_ui(&buffer);
            buffer.clear();
        }
        last_flush = Instant::now();
    }
}
```

### Pattern 3: Error Recovery with Resume

```rust,ignore
async fn stream_with_retry(
    client: &Client,
    prompt: &str,
    max_retries: usize,
) -> Result<String, Error> {
    let mut full_text = String::new();
    let mut last_event_id: Option<String> = None;
    let mut interaction_id: Option<String> = None;
    let mut retries = 0;

    loop {
        let stream: BoxStream<_> = if let Some(id) = &interaction_id {
            // Resume from last position
            Box::pin(client.get_interaction_stream(id, last_event_id.as_deref()))
        } else {
            // Initial stream
            Box::pin(client.interaction()
                .with_model("gemini-3-flash-preview")
                .with_text(prompt)
                .with_store_enabled()
                .create_stream())
        };

        let mut stream = stream;
        let mut completed = false;

        while let Some(result) = stream.next().await {
            match result {
                Ok(event) => {
                    if event.event_id.is_some() {
                        last_event_id = event.event_id.clone();
                    }

                    match &event.chunk {
                        StreamChunk::Created { interaction } => {
                            interaction_id = interaction.id.clone();
                        }
                        StreamChunk::StepDelta { delta, .. } => {
                            if let Some(text) = delta.as_text() {
                                full_text.push_str(text);
                            }
                        }
                        StreamChunk::Completed(_) => {
                            completed = true;
                        }
                        StreamChunk::Error { message, .. } => {
                            return Err(Error::Api(message.clone()));
                        }
                        _ => {}
                    }
                }
                Err(e) if retries < max_retries => {
                    retries += 1;
                    eprintln!("Stream error, retrying ({}/{}): {}", retries, max_retries, e);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    break;  // Break inner loop to retry
                }
                Err(e) => return Err(e.into()),
            }
        }

        if completed {
            return Ok(full_text);
        }
    }
}
```

## Decision Matrix

### Choosing Streaming Mode

```text
Need real-time display? ─────────────────────────> Streaming
Short response, simple code? ────────────────────> Non-streaming
Need resume capability? ─────────────────────────> Streaming
Function calling with UI feedback? ──────────────> Auto-function streaming
Batch processing? ───────────────────────────────> Non-streaming
```

### Choosing Resume Strategy

```text
Long-running interaction (deep research)? ───────> Always track event_id
Unreliable network? ─────────────────────────────> Track event_id + retry logic
Simple chat UI? ─────────────────────────────────> Optional, can restart on error
Background processing? ──────────────────────────> Poll with get_interaction()
```

### Event Handling Approach

```text
Just need text output? ──────────────────────────> Only handle StepDelta + Completed
Need progress tracking? ─────────────────────────> Handle Created, StepStart/StepStop
Building chat UI? ───────────────────────────────> StepDelta for text, Completed for metadata
Agent with functions? ───────────────────────────> Use auto-function streaming
```

## Examples Reference

Examples demonstrating streaming patterns:

| Example | Features |
|---------|----------|
| `streaming` | Basic streaming, all event types, resume pattern |
| `streaming_auto_functions` | Auto-function streaming with progress tracking |
| `thinking` | Streaming with thought summaries |
| `deep_research` | Long-running background streaming |

Run any example:

```bash
cargo run --example streaming

# With wire-level debugging (see all SSE events)
LOUD_WIRE=1 cargo run --example streaming
```

### Wire-Level Debugging

With `LOUD_WIRE=1`, you'll see the raw SSE events:

```text
[REQ#1] POST /v1beta/interactions?alt=sse
  model: gemini-3-flash-preview
  input: "Write a poem"

[RES#1] SSE stream:
  event_type: interaction.created
  event_id: evt_001

  event_type: step.start
  event_id: evt_002
  step: {"type": "model_output", "content": []}

  event_type: step.delta
  event_id: evt_003
  delta: {"type": "text", "text": "In "}

  event_type: step.delta
  event_id: evt_004
  delta: {"type": "text", "text": "circuits "}

  ...

  event_type: step.stop
  event_id: evt_041
  usage: {...}
  step_usage: {...}

  event_type: interaction.completed
  event_id: evt_042
```

This helps debug streaming behavior, event ordering, and resume points.
