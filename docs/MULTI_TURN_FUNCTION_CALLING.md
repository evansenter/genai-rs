# Multi-Turn Function Calling Guide

This guide covers everything you need to know about multi-turn conversations and function calling in `genai-rs`. It explains the patterns, trade-offs, and design decisions behind the API.

## Table of Contents

- [Overview](#overview)
- [Stateful vs Stateless](#stateful-vs-stateless)
- [Function Declaration Approaches](#function-declaration-approaches)
- [Auto vs Manual Function Calling](#auto-vs-manual-function-calling)
- [Parallel and Compositional Function Calling](#parallel-and-compositional-function-calling)
- [API Inheritance Behavior](#api-inheritance-behavior)
- [Thought Signatures](#thought-signatures)
- [Design Patterns](#design-patterns)
- [Decision Matrix](#decision-matrix)
- [Examples Reference](#examples-reference)

## Overview

Multi-turn conversations allow you to build agents that maintain context across multiple exchanges. Combined with function calling, you can create sophisticated tools that interact with external systems.

The key decision points are:

1. **State management**: Server-side (stateful) vs client-side (stateless)
2. **Function execution**: Automatic vs manual control
3. **Function registration**: Compile-time (`#[tool]`) vs runtime (`ToolService` / `FunctionDeclaration`)

## Stateful vs Stateless

The Gemini Interactions API supports two modes controlled by the `store` parameter:

### Stateful Mode (`store: true`)

```rust,ignore
// First turn
let response = client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Hi, I'm Alice")
    .with_system_instruction("You are a helpful assistant")
    .with_store_enabled()  // Server stores conversation
    .create()
    .await?;

// Subsequent turns - just chain with previous_interaction_id
let response = client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("What's my name?")  // Model remembers: "Alice"
    .with_previous_interaction(&response.id.unwrap())
    .with_store_enabled()
    .create()
    .await?;
```

**Characteristics:**
- Server maintains conversation history
- Use `previous_interaction_id` to chain turns
- System instruction is inherited - only send on first turn (observed behavior, not explicitly documented by Google)
- Tools are NOT inherited - must resend on new user message turns, but not on function result turns (observed behavior, not explicitly documented by Google)
- Enables `create_with_auto_functions()` for automatic function execution
- Enables `with_background(true)` for async/agent execution (see `examples/deep_research.rs`)

### Stateless Mode (`store: false`)

Conversation history is a `Vec<Step>`. Build it with `Step` constructors and
extend it with `response.output_steps()` after each turn:

```rust,no_run
use genai_rs::{Client, Step};

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
# let client = Client::new("api-key".to_string());
let mut history: Vec<Step> = vec![];

// First turn
history.push(Step::user_text("Hi, I'm Alice"));

let response = client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_history(history.clone())
    .with_system_instruction("You are a helpful assistant")
    .with_store_disabled()  // No server state
    .create()
    .await?;

// Replay the model's steps (text, thoughts, tool calls) into history
history.extend(response.output_steps());

// Second turn - must include full history
history.push(Step::user_text("What's my name?"));

let response = client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_history(history.clone())
    .with_system_instruction("You are a helpful assistant")
    .with_store_disabled()
    .create()
    .await?;
# let _ = response;
# Ok(())
# }
```

**Characteristics:**
- No server-side conversation storage
- Must manually build and send conversation history
- Cannot use `previous_interaction_id`
- `create_with_auto_functions()` returns a runtime error from `build()`
- `with_background(true)` returns a runtime error (requires storage to retrieve results)
- Must use manual function calling with `create()`

### When to Use Each

| Use Case | Stateful | Stateless |
|----------|----------|-----------|
| Most agent applications | ✅ | |
| Privacy-sensitive applications | | ✅ |
| Custom conversation persistence | | ✅ |
| Conversation filtering/modification | | ✅ |
| Testing and debugging | | ✅ |
| Automatic function execution | ✅ | |
| Background/async execution | ✅ | |

## Function Declaration Approaches

Three ways to declare functions, each suited to different needs:

### 1. `#[tool]` Macro (Compile-time, Stateless)

```rust,ignore
use genai_rs_macros::tool;

/// Look up customer information by ID or email
#[tool(identifier(description = "Customer ID or email"))]
fn lookup_customer(identifier: String) -> String {
    // Function body - no access to external state
    format!("Found customer: {}", identifier)
}

// Using it:
let result = client.interaction()
    .with_text("Look up customer alice@example.com")
    .create_with_auto_functions()  // Auto-discovers #[tool] functions
    .await?;
```

**When to use:**
- Pure functions with no external dependencies
- Simple tools where statelessness is acceptable
- Quick prototyping

**Limitations:**
- Cannot access database connections, API clients, or shared state
- Global registration via `inventory` crate
- Compile-time fixed

### 2. `ToolService` (Runtime, Stateful)

```rust,ignore
use genai_rs::{ToolService, CallableFunction, FunctionDeclaration};

struct MyToolService {
    db: Arc<DatabasePool>,
    config: Arc<RwLock<Config>>,
}

impl ToolService for MyToolService {
    fn tools(&self) -> Vec<Arc<dyn CallableFunction>> {
        vec![
            Arc::new(CustomerLookupTool { db: self.db.clone() }),
            Arc::new(OrderTool { config: self.config.clone() }),
        ]
    }
}

// Using it:
let service = Arc::new(MyToolService { db, config });

let result = client.interaction()
    .with_text("Look up customer alice@example.com")
    .with_tool_service(service.clone())  // Inject stateful service
    .create_with_auto_functions()
    .await?;
```

**When to use:**
- Tools need database connections
- Tools need HTTP clients
- Tools need runtime configuration
- Tools need per-request context (user ID, auth tokens)

**Pattern:** Use `Arc<RwLock<T>>` for interior mutability. Clone the Arc, not the service—cloning an `Arc` just increments a reference count (cheap), and all clones share the same underlying state.

### 3. `FunctionDeclaration` Builder (Manual)

```rust,ignore
use genai_rs::FunctionDeclaration;

let functions = vec![
    FunctionDeclaration::builder("lookup_customer")
        .description("Look up customer by ID")
        .parameter("id", json!({
            "type": "string",
            "description": "Customer ID"
        }))
        .required(vec!["id".to_string()])
        .build(),
];

let response = client.interaction()
    .with_text("Look up customer CUST-001")
    .add_functions(functions)
    .create()  // Manual handling required
    .await?;

// Manually execute function calls
for call in response.function_calls() {
    let result = execute_my_function(call.name, call.args);
    // Send result back...
}
```

**When to use:**
- Full control over function execution
- Dynamic function definitions
- Custom execution logic (rate limiting, logging, etc.)
- Stateless mode where `create_with_auto_functions()` is blocked

## Auto vs Manual Function Calling

### Auto (`create_with_auto_functions()`)

```rust,ignore
// With #[tool] functions
let result = client.interaction()
    .with_text("Calculate 2 + 2")
    .create_with_auto_functions()
    .await?;

// With ToolService
let result = client.interaction()
    .with_text("Calculate 2 + 2")
    .with_tool_service(service)
    .create_with_auto_functions()
    .await?;

// Result includes function executions
for exec in &result.executions {
    println!("{} -> {}", exec.name, exec.result);
}
println!("Final: {}", result.response.as_text().unwrap());
```

**Characteristics:**
- Automatic function execution loop
- Built-in retry and error handling
- Configurable max iterations
- Only available with `store: true`

### Manual (`create()`)

```rust,ignore
let mut response = client.interaction()
    .with_text("What's the weather in Tokyo?")
    .add_functions(functions)
    .create()
    .await?;

// Manual function calling loop
const MAX_ITERATIONS: usize = 5;
for _ in 0..MAX_ITERATIONS {
    let calls = response.function_calls();
    if calls.is_empty() { break; }

    let mut results = Vec::new();
    for call in &calls {
        // call.id is a required &str on FunctionCallInfo
        let result = execute_function(call.name, call.args);
        results.push(Step::function_result(call.name, call.id, result));
    }

    response = client.interaction()
        .with_model("gemini-3-flash-preview")
        .with_previous_interaction(response.id.as_ref().unwrap())
        .with_history(results)
        .create()
        .await?;
}
```

**Characteristics:**
- Full control over execution
- Custom error handling, logging, rate limiting
- Works with both stateful and stateless modes
- Required for `store: false`

## Parallel and Compositional Function Calling

The Gemini API supports advanced function calling patterns:

### Parallel Function Calls

The model can request multiple independent functions in a single response:

```rust,ignore
// Model might return multiple function calls at once
let calls = response.function_calls();
// calls = [get_weather("Tokyo"), get_weather("London"), get_time("UTC")]

// Execute all in parallel
let results: Vec<_> = futures::future::join_all(
    calls.iter().map(|call| execute_function(call))
).await;

// Send all results back together as function_result steps
let result_steps: Vec<_> = calls.iter().zip(results.iter())
    .map(|(call, result)| Step::function_result(
        call.name,
        call.id,
        result.clone(),
    ))
    .collect();

// Function result turn - no need to resend tools
response = client.interaction()
    .with_previous_interaction(&response.id.unwrap())
    .with_history(result_steps)
    .create()
    .await?;
```

**When parallel calls occur:**
- Functions are independent (no data dependencies)
- Model determines parallelism automatically based on the request
- **Order independence**: The API correlates results to their calls via `call_id`,
  not by position. This behavior is validated in `test_parallel_function_result_order_independence`
  in `function_calling_tests.rs`. However, since this is undocumented by Google,
  we recommend maintaining the same order as the original calls as a best practice

### Compositional (Sequential) Function Calls

The model can chain functions where output of one becomes input to another:

```rust,ignore
// User: "Get the weather in my current location"
// Step 1: Model calls get_current_location()
// Step 2: After receiving location, model calls get_weather(location)

// This happens automatically across turns - the model orchestrates the chain
// You just need to execute each function call and return results
```

**Key behaviors:**
- Model manages the chain logic internally
- Each step is a separate function call turn
- Results from earlier steps inform later function selection

### Thought Signatures with Parallel Calls

> **⚠️ Uncertainty**: The [Thought Signatures Guide](https://ai.google.dev/gemini-api/docs/thought-signatures.md.txt) documents behavior for the `generateContent` API, not the Interactions API. Our testing shows no thought signatures appearing on function calls in the Interactions API. The expected behavior below may not apply.

Per the `generateContent` documentation:
- **Parallel calls**: Only the first function call in a parallel batch has a thought signature
- **Sequential calls**: Each step in a sequence has its own thought signature

See `tests/function_calling_tests.rs` for related tests (`test_thought_signature_*`).

## API Inheritance Behavior

When using `previous_interaction_id` (stateful mode), some settings are inherited:

| Setting | Inherited? | Notes |
|---------|-----------|-------|
| System instruction | ❌ No | Must set explicitly on each turn if needed |
| Conversation history | ✅ Yes | Model remembers context |
| Tools/Functions | ❌ No | Must resend on new user message turns (not function result turns) |
| Model | ❌ No | Must specify each request |

> **Important:** `system_instruction` is NOT inherited by the API via `previousInteractionId`. If you want the same system instruction on every turn, you must set it explicitly.

### System Instruction

`with_system_instruction()` can be called at any point in builder chaining. Set it on each turn where you want it to apply.

```rust,ignore
// First turn
let response = client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Hello!")
    .with_system_instruction("You are a helpful assistant")
    .with_store_enabled()
    .create()
    .await?;

// Subsequent turn - set system_instruction again if needed
let response = client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("What can you help me with?")
    .with_previous_interaction(&response.id.unwrap())
    .with_system_instruction("You are a helpful assistant")  // Set explicitly
    .with_store_enabled()
    .create()
    .await?;
```

### Auto-Function Calling

For `create_with_auto_functions()`, the system instruction is automatically included on all turns within the auto-function loop. The SDK reuses the same request object internally, so you only need to set it once:

```rust,ignore
// System instruction is sent on every internal iteration automatically
let result = client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("What's the weather?")
    .with_system_instruction("You are a weather assistant")
    .create_with_auto_functions()
    .await?;
```

### Recommended Pattern for Manual Multi-Turn

```rust,ignore
// Store the system instruction to reuse across turns
let system_prompt = "You are a helpful assistant";

// Each turn: always include system_instruction and tools
let response = client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_text(message)
    .add_functions(functions.clone())
    .with_system_instruction(system_prompt)
    .with_previous_interaction(&last_id)  // if not first turn
    .create()
    .await?;
```

### Important: Function Result Turns

When sending function results back to the model, you do NOT need to resend tools:

```rust,ignore
// After model requests function calls...
let results: Vec<Step> = execute_functions(&calls);  // Step::function_result steps

// Function result turn - no tools needed
response = client.interaction()
    .with_previous_interaction(&response.id.unwrap())
    .with_history(results)  // Just the function_result steps
    .create()
    .await?;
```

The model remembers available tools within the same interaction chain. Only new user message turns need tools resent.

## Thought Signatures

> **Key Finding**: Thought signatures ARE present in Interactions API responses, but in a **different location** than the `generateContent` docs describe. See [INTERACTIONS_API_FEEDBACK.md](./INTERACTIONS_API_FEEDBACK.md#1-thought-signatures-location-differs-from-generatecontent) for details.

The Gemini API returns `thought` steps when thinking is enabled. Here's what we know:

### What They Are

Thought signatures are cryptographic proofs that thoughts haven't been modified. In the **Interactions API**, they appear on the `thought` step — in the SDK, `Step::Thought { signature, summary }`. Tool-call steps (e.g., `code_execution_call`, `google_search_call`) also carry an optional `signature` field:

```json
{
  "steps": [
    {
      "type": "thought",
      "signature": "EtYFCtMF..."    // <-- Signature is HERE
    },
    {
      "type": "function_call",
      "id": "call_abc123",
      "name": "get_weather",        // <-- NOT here (differs from generateContent)
      "arguments": {"city": "Paris"}
    }
  ]
}
```

### Interactions API vs generateContent

| API | Signature Location |
|-----|-------------------|
| `generateContent` (per docs) | On `function_call` as `thought_signature` field |
| Interactions API (actual) | On the `thought` step as `signature` field |

Our testing confirms signatures are present on thought steps across all configurations:

| Configuration | Sig on Thought? | Sig on FC? | Test |
|---------------|-----------------|------------|------|
| `store: true` (stateful) | ✅ Yes | ❌ No | `test_thought_signature_parallel_only_first` |
| `store: false` (stateless) | ✅ Yes | ❌ No | `test_stateless_with_thinking_thought_signatures` |
| Parallel function calls | ✅ Yes | ❌ No | `test_thought_signature_parallel_only_first` |
| Sequential function calls | ✅ Yes | ❌ No | `test_thought_signature_sequential_each_step` |
| ThinkingLevel::High | ✅ Yes | ❌ No | `test_thinking_level_high_thought_signatures` |
| FunctionCallingMode::Any | ✅ Yes | ❌ No | `test_function_calling_mode_any_thought_signatures` |

### Practical Implications

**For stateless multi-turn**: Replay signatures by extending your history with `response.output_steps()`. This includes the `Step::Thought` steps verbatim (carrying their `signature`) alongside the `Step::FunctionCall` steps, so the reasoning chain validates on the next request:

```rust,ignore
// Replay the model's full turn - Thought steps carry the signature
history.extend(response.output_steps());

// Then execute and append your function results
for call in response.function_calls() {
    let result = execute_function(call.name, call.args);
    history.push(Step::function_result(call.name, call.id, result));
}
```

**For thought echo**: Echoing thoughts back to the model works **without** including signatures. See `test_thought_echo_manual_history` in `tests/interactions_api_tests.rs`.

**For compliance/auditing**: Capture signatures from `Step::Thought { signature, .. }`, or use the `response.thought_signatures()` helper to iterate all populated signatures. The signature IS populated on thought steps.

### Tests

> - `test_thought_signature_parallel_only_first` in `tests/function_calling_tests.rs` - Tests parallel calls (currently shows no signatures)
> - `test_thought_signature_sequential_each_step` in `tests/function_calling_tests.rs` - Tests sequential calls (currently shows no signatures)
> - `test_thought_echo_manual_history` in `tests/interactions_api_tests.rs` - Echoing thoughts back

## Design Patterns

### Pattern 1: First Turn vs Subsequent Turns (Match)

System instructions are NOT inherited across turns via `previousInteractionId`, so you must set them explicitly on each turn if needed:

```rust,ignore
struct Agent {
    client: Client,
    last_id: Option<String>,
    functions: Vec<FunctionDeclaration>,
}

impl Agent {
    async fn process(&mut self, message: &str) -> Result<String, Error> {
        let response = match &self.last_id {
            Some(prev_id) => {
                self.client.interaction()
                    .with_model("gemini-3-flash-preview")
                    .with_text(message)
                    .add_functions(self.functions.clone())
                    .with_previous_interaction(prev_id)
                    .create()
                    .await?
            }
            None => {
                self.client.interaction()
                    .with_model("gemini-3-flash-preview")
                    .with_text(message)
                    .add_functions(self.functions.clone())
                    .with_system_instruction("...")
                    .create()
                    .await?
            }
        };

        self.last_id = response.id.clone();
        Ok(response.as_text().unwrap_or("").to_string())
    }
}
```

**Why not simplify with optional?**

A method like `with_optional_previous_interaction(Option<&str>)` would:
1. Hide the fact that system instruction behavior changes
2. Still require tracking `last_id: Option<String>` somewhere
3. Make the API less explicit about what's happening

The match pattern is verbose but clear about the two distinct states.

### Pattern 2: Stateless History Builder

```rust,ignore
struct StatelessSession {
    client: Client,
    history: Vec<Step>,
    functions: Vec<FunctionDeclaration>,
    system_instruction: String,
}

impl StatelessSession {
    async fn process(&mut self, message: &str) -> Result<String, Error> {
        self.history.push(Step::user_text(message));

        let response = self.client.interaction()
            .with_model("gemini-3-flash-preview")
            .with_history(self.history.clone())
            .add_functions(self.functions.clone())
            .with_system_instruction(&self.system_instruction)
            .with_store_disabled()
            .create()
            .await?;

        // Handle function calls (execute, push Step::function_result, re-send)...

        // Replay the model's steps (including thoughts with signatures)
        self.history.extend(response.output_steps());

        Ok(response.as_text().unwrap_or("").to_string())
    }
}
```

### Pattern 3: ToolService with Shared State

```rust,ignore
struct ProductionToolService {
    db: Arc<DatabasePool>,
    http: Arc<reqwest::Client>,
    config: Arc<RwLock<AppConfig>>,
}

impl ToolService for ProductionToolService {
    fn tools(&self) -> Vec<Arc<dyn CallableFunction>> {
        vec![
            Arc::new(DatabaseTool { pool: self.db.clone() }),
            Arc::new(ApiTool { client: self.http.clone() }),
            Arc::new(ConfigTool { config: self.config.clone() }),
        ]
    }
}

// Usage
let service = Arc::new(ProductionToolService { db, http, config });

// Multiple requests share the same service instance
let r1 = client.interaction()
    .with_tool_service(service.clone())
    .create_with_auto_functions().await?;

let r2 = client.interaction()
    .with_tool_service(service.clone())  // Same instance
    .create_with_auto_functions().await?;
```

## Decision Matrix

### Choosing State Management

```text
Need privacy/no server storage? ─────────────────> Stateless
Need custom conversation persistence? ───────────> Stateless
Need automatic function execution? ──────────────> Stateful
Building a typical agent? ───────────────────────> Stateful
```

### Choosing Function Declaration

```text
Pure functions, no dependencies? ────────────────> #[tool] macro
Need database/API access? ───────────────────────> ToolService
Need full control/custom execution? ─────────────> FunctionDeclaration + manual
Stateless mode? ─────────────────────────────────> FunctionDeclaration + manual
```

### Choosing Execution Mode

```text
store: false? ───────────────────────────────────> Manual (auto is blocked)
Need custom logging/rate limiting? ──────────────> Manual
Need simplest code? ─────────────────────────────> Auto
Standard agent? ─────────────────────────────────> Auto
```

## Examples Reference

Examples are ordered from simple to complex:

| Example | State | Functions | Execution |
|---------|-------|-----------|-----------|
| **Single-Turn (Getting Started)** |
| `auto_function_calling` | Single-turn | `#[tool]` | Auto |
| `manual_function_calling` | Single-turn | `FunctionDeclaration` | Manual |
| `streaming_auto_functions` | Single-turn | `#[tool]` | Auto + Streaming |
| **Multi-Turn Stateful** |
| `multi_turn_agent_auto` | Stateful | `#[tool]` | Auto |
| `multi_turn_agent_manual` | Stateful | `FunctionDeclaration` | Manual |
| `tool_service` | Stateful | `ToolService` | Auto |
| **Multi-Turn Stateless** |
| `multi_turn_agent_manual_stateless` | Stateless | `FunctionDeclaration` | Manual |
| **Advanced Patterns** |
| `parallel_and_compositional_functions` | Stateful | `FunctionDeclaration` | Manual + Parallel |

Run any example:

```bash
cargo run --example multi_turn_agent_auto

# With wire-level debugging
LOUD_WIRE=1 cargo run --example multi_turn_agent_manual_stateless
```
