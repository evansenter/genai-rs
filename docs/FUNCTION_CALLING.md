# Function Calling Guide

Function calling lets the model ask your code to run. `genai-rs` offers three
ways to provide functions:

| Approach | Registration | State | Execution | Best for |
|----------|-------------|-------|-----------|----------|
| `#[tool]` macro | Compile-time, global registry | Stateless | Auto or manual | Simple functions |
| `ToolService` | Runtime, per request | Stateful (DB pools, clients, config) | Auto or manual | Dependency injection |
| `FunctionDeclaration` + your own loop | Runtime | Anything | Manual | Custom execution: rate limits, caching, per-call timeouts |

Server-side tools (Google Search, code execution, URL context, ...) are a
different mechanism; see [Built-in Tools](BUILT_IN_TOOLS.md).

## The `#[tool]` macro

```rust,no_run
use genai_rs_macros::tool;

/// Gets the current weather for a city
#[tool(city(description = "The city to get weather for"))]
fn get_weather(city: String) -> String {
    // In production, call a weather API
    format!(r#"{{"city": "{}", "temp": "22°C"}}"#, city)
}

/// Gets current time in a timezone
#[tool(timezone(description = "Timezone like UTC, PST, EST"))]
fn get_time(timezone: String) -> String {
    format!(r#"{{"timezone": "{}", "time": "14:30"}}"#, timezone)
}

// Note what is *not* imported: `#[tool]` needs no `async-trait` or
// `serde_json` dependency and no `CallableFunction` in scope. This snippet
// is compiled as a doctest, so it is the in-repo proof of that, alongside
// `tests/ui/pass_no_consumer_imports.rs`.
//
// The generated free function avoids the trait import too. Calling
// `GetWeatherCallable.declaration()` in method position also works, but
// then `use genai_rs::CallableFunction;` is required — for that call, not
// for the macro.
let _declaration = get_weather_declaration();
```

For `fn get_weather`, the macro generates:

1. A `FunctionDeclaration`. The doc comment becomes the function description,
   and `name(description = "...")` attributes describe the parameters.
2. A callable type, `GetWeatherCallable`, registered in the global function
   registry (via `inventory`).
3. `get_weather_declaration()`, a free function returning the declaration.

**Parameters.** `String` maps to a JSON-schema `string`; integer types to
`integer`; `f32`/`f64` to `number`; `bool` to `boolean`; `Vec<T>` to `array`;
anything else to `object`. `Option<T>` parameters are optional; all others are
required. Constrain values with `enum_values`:

```rust,ignore
#[tool(
    city(description = "City name"),
    unit(description = "Temperature unit", enum_values = ["celsius", "fahrenheit"])
)]
fn get_weather(city: String, unit: Option<String>) -> serde_json::Value { /* ... */ }
```

`async fn` works too; the generated callable awaits it.

**Return values.** The return value is serialized with `serde_json`. A JSON
object is sent as-is; anything else is wrapped as `{"result": <value>}`. So a
`String` containing JSON reaches the model as a string, and a `Result` arrives
as `{"Ok": ...}` / `{"Err": ...}`. For structured data, return a
`serde_json::Value` or a `Serialize` type. For errors, return an object with an
`error` key. Details and the other error cases are in
[Error Handling](ERROR_HANDLING.md#function-calling-errors).

## Automatic execution: `create_with_auto_functions()`

```rust,ignore
let result = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("What's the weather in Tokyo?")
    .create_with_auto_functions()
    .await?;

for exec in &result.executions {
    println!("{}({}) -> {} in {:?}", exec.name, exec.args, exec.result, exec.duration);
}
println!("{}", result.response.as_text().unwrap_or_default());
```

How the loop behaves:

- **Discovery.** If the request has no tools set, every registered `#[tool]`
  function and every `ToolService` function is declared; a service function
  shadows a registry function with the same name. If *any* tool is set
  (`add_function()`, `with_google_search()`, ...), discovery is skipped and
  only what you set is declared. Registered functions are still executed
  when called, so `add_function(get_weather_declaration())` is how you
  restrict the model to a subset.
- **Rounds.** Each round sends the request, executes the function calls it
  gets back, and sends the results, chained by `previous_interaction_id`. The
  whole request, including tools, system instruction and generation config,
  is reused every round. That is why the loop requires storage: it returns
  `InvalidInput` with `with_store_disabled()`.
- **Limit.** The default is 5 rounds (`with_max_function_call_loops(n)` to
  change it). On hitting the limit the loop returns the partial
  `AutoFunctionResult` with `reached_max_loops: true` rather than an error.
- **Errors.** Function failures are sent to the model as results (see
  [Error Handling](ERROR_HANDLING.md#function-calling-errors)). An API error
  ends the loop with that error; nothing is retried (see
  [Reliability](RELIABILITY.md#the-auto-function-loop)).
- **Timeouts.** `with_timeout()` applies to each API call, not to the whole
  run or to function execution.

The streaming variant, `create_stream_with_auto_functions()`, is covered in
[Streaming API](STREAMING_API.md#auto-function-streaming).

## `ToolService` for stateful functions

Implement `CallableFunction` for each tool that needs state, and return them
from a `ToolService`. Implementing the trait by hand needs `async-trait` and
`serde_json` as direct dependencies.

```rust,no_run
use async_trait::async_trait;
use genai_rs::{CallableFunction, FunctionDeclaration, FunctionError, ToolService};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

struct NextTicket {
    counter: Arc<AtomicU64>,
}

#[async_trait]
impl CallableFunction for NextTicket {
    fn declaration(&self) -> FunctionDeclaration {
        FunctionDeclaration::builder("next_ticket")
            .description("Allocate the next support ticket number")
            .build()
    }

    async fn call(&self, _args: Value) -> Result<Value, FunctionError> {
        Ok(json!({"ticket": self.counter.fetch_add(1, Ordering::SeqCst)}))
    }
}

struct Tickets {
    counter: Arc<AtomicU64>,
}

impl ToolService for Tickets {
    fn tools(&self) -> Vec<Arc<dyn CallableFunction>> {
        vec![Arc::new(NextTicket { counter: self.counter.clone() })]
    }
}

# async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
let service = Arc::new(Tickets { counter: Arc::new(AtomicU64::new(1)) });

let result = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("Open a support ticket for me")
    .with_tool_service(service.clone()) // share one instance across requests
    .create_with_auto_functions()
    .await?;
println!("{}", result.response.as_text().unwrap_or_default());
# Ok(())
# }
```

For a failure the model should see, return
`Err(FunctionError::ExecutionError(Box::new(e)))`. The loop sends it as
`{"error": "Function execution error: ..."}`.

## Manual function handling

Use `create()` and run the loop yourself when you need control over
execution, or when storage is disabled.

Function calls arrive as `Step::FunctionCall { id, name, arguments, .. }` steps,
and `response.function_calls()` returns them as `FunctionCallInfo { id, name,
args }`. Send results back as `Step::function_result(name, call_id, result)`,
where `result` is any `Into<FunctionResultPayload>` (a `serde_json::Value`,
`&str`, `String`, or `Vec<Content>`):

```rust,no_run
use genai_rs::{Client, FunctionDeclaration, Step};
use serde_json::json;

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
# let client = Client::new("api-key".to_string());
# fn execute_my_function(_name: &str, _args: &serde_json::Value) -> serde_json::Value {
#     json!({"temp": "22C"})
# }
// Define declarations (schemas only)
let get_weather = FunctionDeclaration::builder("get_weather")
    .description("Get weather for a city")
    .parameter("city", json!({"type": "string"}))
    .required(vec!["city".to_string()])
    .build();

// Initial request
let mut response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("What's the weather in Tokyo?")
    .add_functions(vec![get_weather])
    .create()  // NOT create_with_auto_functions
    .await?;

// Manual execution loop
while response.has_function_calls() {
    let mut results = Vec::new();

    for call in response.function_calls() {
        // YOUR execution logic here
        let result = execute_my_function(call.name, call.args);

        results.push(Step::function_result(
            call.name,
            call.id,  // Required; correlates the result to its call
            result,
        ));
    }

    // Send results back as function_result steps
    response = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_previous_interaction(response.id.as_ref().unwrap())
        .with_history(results)
        .create()
        .await?;
}

// Final text response
println!("{}", response.as_text().unwrap());
# Ok(())
# }
```

For a failed execution, `Step::function_result_error(name, call_id, result)`
sets `is_error: true` on the step. A real loop should also cap its iterations.

**Parallel calls.** The model may return several calls in one response. Run
them concurrently if you like, and send one `function_result` step per call
in a single request. Results are matched to calls by `call_id`, not by
position.

## `FunctionDeclaration` builder

```rust
use genai_rs::FunctionDeclaration;
use serde_json::json;

let declaration = FunctionDeclaration::builder("search_products")
    .description("Search for products by query")
    .parameter("query", json!({"type": "string", "description": "Search query"}))
    .parameter("limit", json!({"type": "integer", "description": "Max results (1-100)"}))
    .parameter("category", json!({"type": "string", "enum": ["books", "music", "games"]}))
    .required(vec!["query".to_string()])
    .build();

assert_eq!(declaration.name(), "search_products");
```

Each `parameter()` value is a JSON-schema fragment, so nested objects and
arrays work as in JSON Schema.

## Function calling modes

| Mode | Model behavior | Wire value |
|------|---------------|------------|
| `FunctionCallingMode::Auto` (default) | Decides whether to call | `"auto"` |
| `FunctionCallingMode::Any` | Must call a function | `"any"` |
| `FunctionCallingMode::None` | Cannot call functions | `"none"` |
| `FunctionCallingMode::Validated` | Schema adherence for both calls and text | `"validated"` |

`with_function_calling_mode()` is a convenience over the underlying
`generation_config.tool_choice` union, which is `Option<ToolChoice>`:

- `ToolChoice::Mode(FunctionCallingMode)` serializes as a plain lowercase string
  (e.g., `"auto"`)
- `ToolChoice::AllowedTools(AllowedTools)` serializes as
  `{"allowed_tools": {"mode": ..., "tools": [...]}}` and restricts the model to a
  named subset of the declared tools

```rust,no_run
use genai_rs::{AllowedTools, Client, FunctionCallingMode, ToolChoice};

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
# let client = Client::new("api-key".to_string());
// Set the union directly
let builder = client.interaction()
    .with_tool_choice(ToolChoice::Mode(FunctionCallingMode::Auto));

// Restrict to a subset of tools (convenience method)
let builder = client.interaction()
    .with_allowed_tools(vec!["get_weather".to_string()]);

// Restrict to a subset AND force a call among them
let builder = client.interaction()
    .with_tool_choice(ToolChoice::allowed_tools(
        Some(FunctionCallingMode::Any),
        vec!["get_weather".to_string(), "get_time".to_string()],
    ));

// Equivalent, via the AllowedTools builder
let builder = client.interaction()
    .with_tool_choice(ToolChoice::AllowedTools(
        AllowedTools::new(vec!["get_weather".to_string()])
            .with_mode(FunctionCallingMode::Any),
    ));
# let _ = builder;
# Ok(())
# }
```

`with_allowed_tools(Vec<String>)` sets the `AllowedTools` form of
`tool_choice`, keeping any mode set before it.

## Streaming function-call arguments

When streaming, arguments arrive incrementally as `StepDelta::ArgumentsDelta`
JSON fragments. The completed `StreamChunk::Completed(response)` has them
assembled and parsed, so `response.function_calls()` works after streaming
exactly as it does after `create()`. See [Streaming API](STREAMING_API.md).

## Examples

| Example | Demonstrates |
|---------|-------------|
| `auto_function_calling` | `#[tool]` macro, auto-discovery, modes |
| `tool_service` | Stateful functions, dependency injection |
| `manual_function_calling` | Manual loop, parallel calls via `join_all`, dependent calls across rounds |
| `streaming_auto_functions` | Streaming with auto execution |

```bash
cargo run --example <name>
```

## Related

- [Multi-Turn Function Calling](MULTI_TURN_FUNCTION_CALLING.md): functions across turns, stateless history
- [Error Handling](ERROR_HANDLING.md#function-calling-errors): what the model receives when a function fails
- [Streaming API](STREAMING_API.md): streaming with functions
