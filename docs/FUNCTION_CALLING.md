# Function Calling Guide

This guide covers function calling fundamentals in `genai-rs`, including the `#[tool]` macro, `ToolService` for stateful functions, and manual handling patterns.

## Table of Contents

- [Overview](#overview)
- [Choosing an Approach](#choosing-an-approach)
- [The #[tool] Macro](#the-tool-macro)
- [ToolService for Stateful Functions](#toolservice-for-stateful-functions)
- [Manual Function Handling](#manual-function-handling)
- [FunctionDeclaration Builder](#functiondeclaration-builder)
- [Function Calling Modes](#function-calling-modes)
- [Streaming Function Call Arguments](#streaming-function-call-arguments)
- [Parallel and Compositional Calls](#parallel-and-compositional-calls)
- [Best Practices](#best-practices)

## Overview

Function calling lets the model invoke your code to get real-time data or perform actions. There are three approaches:

| Approach | Registration | State | Execution | Best For |
|----------|-------------|-------|-----------|----------|
| `#[tool]` macro | Compile-time | Stateless | Auto or manual | Simple, clean code |
| `ToolService` | Runtime | Stateful | Auto or manual | DB pools, API clients |
| Manual | Runtime | Flexible | Manual only | Custom execution logic |

## Choosing an Approach

```text
Need shared state (DB, API clients, config)?
├── Yes → Use ToolService
└── No
    ├── Simple function, want minimal code?
    │   └── Yes → Use #[tool] macro
    └── Need custom execution logic?
        └── Yes → Use manual handling
```

### Decision Matrix

| Need | Recommended Approach |
|------|---------------------|
| Quick prototype | `#[tool]` + `create_with_auto_functions()` |
| Production stateless | `#[tool]` + `create_with_auto_functions()` |
| Database access | `ToolService` |
| Per-request context | `ToolService` |
| Rate limiting | Manual handling |
| Circuit breakers | Manual handling |
| Custom logging/metrics | Manual handling |

## The #[tool] Macro

The simplest approach - define functions with the `#[tool]` attribute.

### Basic Usage

```rust,ignore
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
```

### Auto-Discovery and Execution

```rust,ignore
// Functions are auto-discovered from the global registry
let result = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("What's the weather in Tokyo?")
    .create_with_auto_functions()  // Auto-discovers and executes
    .await?;

println!("{}", result.response.as_text().unwrap());
```

### Limiting Available Functions

```rust,ignore
// Only expose specific functions (not all registered ones)
let result = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("What's the weather in Tokyo?")
    .add_function(GetWeatherCallable.declaration())  // Only weather
    .create_with_auto_functions()
    .await?;
```

### Multiple Parameters

```rust,ignore
#[tool(
    city(description = "The city name"),
    unit(description = "Temperature unit: celsius or fahrenheit")
)]
fn get_weather_detailed(city: String, unit: String) -> String {
    // Implementation
}
```

### Async Functions

```rust,ignore
#[tool(url(description = "URL to fetch"))]
async fn fetch_url(url: String) -> String {
    // Async operations supported
    reqwest::get(&url).await
        .map(|r| r.text().await.unwrap_or_default())
        .unwrap_or_else(|e| format!(r#"{{"error": "{}"}}"#, e))
}
```

### What the Macro Generates

The `#[tool]` macro generates:
1. A `FunctionDeclaration` from the signature
2. A callable type (e.g., `GetWeatherCallable`)
3. Registration in the global function registry

```rust,ignore
// You can access the generated declaration:
let declaration = GetWeatherCallable.declaration();
println!("Name: {}", declaration.name());
println!("Description: {}", declaration.description());
```

## ToolService for Stateful Functions

Use `ToolService` when functions need shared state like database connections or configuration.

### Implementing ToolService

```rust,ignore
use async_trait::async_trait;
use genai_rs::{CallableFunction, FunctionDeclaration, FunctionError, ToolService};
use std::sync::Arc;

// Your tool with state
struct WeatherTool {
    api_client: Arc<WeatherApiClient>,
}

#[async_trait]
impl CallableFunction for WeatherTool {
    fn declaration(&self) -> FunctionDeclaration {
        FunctionDeclaration::builder("get_weather")
            .description("Get current weather")
            .parameter("city", json!({"type": "string"}))
            .required(vec!["city".to_string()])
            .build()
    }

    async fn call(&self, args: serde_json::Value) -> Result<serde_json::Value, FunctionError> {
        let city = args["city"].as_str().unwrap_or("unknown");
        let weather = self.api_client.get_weather(city).await?;
        Ok(json!({"city": city, "temp": weather.temp}))
    }
}

// The service that provides tools
struct MyToolService {
    api_client: Arc<WeatherApiClient>,
}

impl ToolService for MyToolService {
    fn tools(&self) -> Vec<Arc<dyn CallableFunction>> {
        vec![Arc::new(WeatherTool {
            api_client: self.api_client.clone(),
        })]
    }
}
```

### Using the Service

```rust,ignore
let service = Arc::new(MyToolService {
    api_client: Arc::new(WeatherApiClient::new()),
});

let result = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("What's the weather in Tokyo?")
    .with_tool_service(service.clone())  // Inject the service
    .create_with_auto_functions()
    .await?;
```

### Dynamic Configuration

```rust,ignore
use std::sync::RwLock;

struct ConfigurableService {
    precision: Arc<RwLock<u32>>,
}

impl ConfigurableService {
    fn set_precision(&self, value: u32) {
        *self.precision.write().unwrap() = value;
    }
}

// Change config between requests
service.set_precision(8);
let result = client.interaction()
    .with_tool_service(service.clone())
    .create_with_auto_functions()
    .await?;
```

## Manual Function Handling

For full control over execution, handle function calls manually.

### Manual Loop Pattern

Function calls arrive as `Step::FunctionCall { id, name, arguments }` steps. Send
results back as `Step::FunctionResult` steps built with `Step::function_result()`,
which accepts any `Into<FunctionResultPayload>` (a `serde_json::Value`, `&str`,
`String`, or `Vec<Content>`):

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
    .with_model("gemini-3-flash-preview")
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
        .with_model("gemini-3-flash-preview")
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

For failed executions, use `Step::function_result_error(name, call_id, result)`
which sets `is_error: true` on the step.

### When to Use Manual Handling

| Use Case | Implementation |
|----------|----------------|
| Rate limiting | Add delays between calls |
| Circuit breakers | Track failures, skip calls |
| Caching | Check cache before execution |
| Logging/metrics | Wrap execution with instrumentation |
| Timeouts | Add per-function timeouts |

```rust,ignore
// Example: Rate limiting
for call in response.function_calls() {
    rate_limiter.acquire().await;  // Wait for rate limit
    let result = execute_function(call.name, call.args);
    // ...
}

// Example: Circuit breaker
for call in response.function_calls() {
    if circuit_breaker.is_open(call.name) {
        results.push(Step::function_result_error(call.name, call.id, "circuit open"));
        continue;
    }
    let result = execute_function(call.name, call.args);
    // ...
}
```

## FunctionDeclaration Builder

Build function schemas programmatically.

### Basic Builder

```rust,ignore
use genai_rs::FunctionDeclaration;
use serde_json::json;

let declaration = FunctionDeclaration::builder("search_products")
    .description("Search for products by query")
    .parameter("query", json!({
        "type": "string",
        "description": "Search query"
    }))
    .parameter("limit", json!({
        "type": "integer",
        "description": "Max results (1-100)"
    }))
    .required(vec!["query".to_string()])
    .build();
```

### Complex Parameter Types

```rust,ignore
// Enum parameter
let declaration = FunctionDeclaration::builder("convert_temp")
    .description("Convert temperature")
    .parameter("value", json!({"type": "number"}))
    .parameter("from_unit", json!({
        "type": "string",
        "enum": ["celsius", "fahrenheit", "kelvin"]
    }))
    .parameter("to_unit", json!({
        "type": "string",
        "enum": ["celsius", "fahrenheit", "kelvin"]
    }))
    .required(vec!["value", "from_unit", "to_unit"])
    .build();

// Object parameter
let declaration = FunctionDeclaration::builder("create_user")
    .description("Create a new user")
    .parameter("user", json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "email": {"type": "string"},
            "age": {"type": "integer"}
        },
        "required": ["name", "email"]
    }))
    .required(vec!["user".to_string()])
    .build();

// Array parameter
let declaration = FunctionDeclaration::builder("process_items")
    .description("Process a list of items")
    .parameter("items", json!({
        "type": "array",
        "items": {"type": "string"}
    }))
    .build();
```

### Accessing Declaration Properties

```rust,ignore
let decl = GetWeatherCallable.declaration();

println!("Name: {}", decl.name());
println!("Description: {}", decl.description());
println!("Parameters: {:?}", decl.parameters());
```

## Function Calling Modes

Control how the model uses functions.

### Available Modes

```rust,ignore
use genai_rs::FunctionCallingMode;

// Auto (default): Model decides whether to call
client.interaction()
    .add_function(decl)
    .with_function_calling_mode(FunctionCallingMode::Auto)

// Any: Model MUST call a function
client.interaction()
    .add_function(decl)
    .with_function_calling_mode(FunctionCallingMode::Any)

// None: Disable function calling
client.interaction()
    .add_function(decl)
    .with_function_calling_mode(FunctionCallingMode::None)

// Validated: Schema adherence for both calls and text
client.interaction()
    .add_function(decl)
    .with_function_calling_mode(FunctionCallingMode::Validated)
```

### Mode Comparison

| Mode | Model Behavior | Use Case |
|------|---------------|----------|
| `Auto` | Decides whether to call | General use |
| `Any` | Must call a function | Guarantee function execution |
| `None` | Cannot call functions | Disable temporarily |
| `Validated` | Schema-strict output | High reliability needs |

On the wire, modes serialize as lowercase strings (`"auto"`, `"any"`, `"none"`,
`"validated"`) in `generation_config.tool_choice`.

### Tool Choice

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

Note: `with_allowed_tools(Vec<String>)` sets the `AllowedTools` form of
`tool_choice` (preserving any previously set mode).

## Streaming Function Call Arguments

When streaming, function-call arguments arrive incrementally as
`StepDelta::ArgumentsDelta` chunks (JSON fragments). The HTTP layer assembles
the streamed `step.start`/`step.delta`/`step.stop` events into the final
`StreamChunk::Completed(response)`, including parsing the accumulated argument
fragments into `Step::FunctionCall { arguments, .. }` — so
`response.function_calls()` works on the completed response after streaming,
exactly as in the non-streaming case. See [Streaming API](STREAMING_API.md).

## Parallel and Compositional Calls

### Parallel Execution

The model may request multiple functions at once:

```rust,ignore
// Model might request: get_weather("Tokyo"), get_weather("London")
for call in response.function_calls() {
    // Execute in parallel using tokio::spawn or futures::join!
}
```

```rust,ignore
use futures::future::join_all;

let futures: Vec<_> = response.function_calls()
    .iter()
    .map(|call| async {
        let result = execute_function(call.name, call.args).await;
        (call.id.to_string(), call.name.to_string(), result)
    })
    .collect();

let results = join_all(futures).await;

// Build one Step::function_result per call and send them back together
let result_steps: Vec<_> = results.into_iter()
    .map(|(id, name, result)| Step::function_result(name, id, result))
    .collect();
```

### Compositional (Chained) Calls

The model chains function outputs:

```text
User: "Convert the temperature in Tokyo to Fahrenheit"
→ Model: get_weather("Tokyo") → {"temp": "22°C"}
→ Model: convert_temp(22, "celsius", "fahrenheit") → {"temp": "71.6°F"}
→ Model: "The temperature in Tokyo is 71.6°F"
```

This happens automatically across multiple loop iterations.

## Best Practices

### 1. Return JSON from Functions

```rust,ignore
#[tool(city(description = "City name"))]
fn get_weather(city: String) -> String {
    // Return JSON for structured data
    format!(r#"{{"city": "{}", "temp": "22°C", "conditions": "sunny"}}"#, city)
}
```

### 2. Handle Errors Gracefully

```rust,ignore
#[tool(id(description = "User ID"))]
fn get_user(id: i32) -> String {
    if id <= 0 {
        return r#"{"error": "Invalid user ID"}"#.to_string();
    }

    match database.find_user(id) {
        Some(user) => serde_json::to_string(&user).unwrap(),
        None => format!(r#"{{"error": "User {} not found"}}"#, id),
    }
}
```

### 3. Validate Inputs

```rust,ignore
#[tool(query(description = "Search query"))]
fn search(query: String) -> String {
    if query.len() > 1000 {
        return r#"{"error": "Query too long"}"#.to_string();
    }

    if query.trim().is_empty() {
        return r#"{"error": "Query cannot be empty"}"#.to_string();
    }

    // Proceed with search...
}
```

### 4. Use Descriptive Names and Descriptions

```rust,ignore
// Good: Clear, specific
#[tool(city(description = "City name (e.g., 'Tokyo', 'New York')"))]
fn get_current_weather(city: String) -> String

// Bad: Vague
#[tool(x(description = "input"))]
fn do_thing(x: String) -> String
```

### 5. Limit Function Count

```rust,ignore
// Provide only relevant functions to reduce model confusion
let result = client
    .interaction()
    .with_text("What's the weather?")
    .add_function(weather_func)  // Only weather, not all 20 functions
    .create_with_auto_functions()
    .await?;
```

### 6. Set Max Loops for Auto Execution

```rust,ignore
// Prevent infinite loops
let result = client
    .interaction()
    .with_text(prompt)
    .add_function(func)
    .with_max_function_call_loops(5)  // Default is 10
    .create_with_auto_functions()
    .await?;
```

## Examples

| Example | Demonstrates |
|---------|-------------|
| `auto_function_calling` | `#[tool]` macro, auto-discovery, modes |
| `tool_service` | Stateful functions, dependency injection |
| `manual_function_calling` | Manual loop, full control |
| `parallel_and_compositional_functions` | Parallel execution, chaining |
| `streaming_auto_functions` | Streaming with auto execution |

Run with:
```bash
cargo run --example <name>
```

## Related Documentation

- [Multi-Turn Function Calling](MULTI_TURN_FUNCTION_CALLING.md) - Multi-turn patterns
- [Streaming API](STREAMING_API.md) - Streaming with functions
- [Error Handling](ERROR_HANDLING.md) - Function errors
