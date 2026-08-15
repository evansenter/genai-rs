# Multi-Turn Customer Support Agent (Manual Functions)

A stateful customer support chatbot using manual function calling with `create()`.

See also: [multi_turn_agent_auto](../multi_turn_agent_auto/) for automatic function calling.

## Overview

This example demonstrates the same customer support agent as `multi_turn_agent_auto`, but uses manual function calling instead of `create_with_auto_functions()`. This approach gives you full control over:

- When and how functions are executed
- Result formatting and error handling
- Custom logging and monitoring
- Rate limiting and circuit breaking

## Key Differences from Auto Version

| Aspect | Auto (`create_with_auto_functions()`) | Manual (`create()` + loop) |
|--------|---------------------------------------|---------------------------|
| Function execution | Automatic | You control the loop |
| Function discovery | Registry + service auto-discovery | Explicit declarations |
| Error handling | Errors converted to JSON for model | Full control over error handling |
| Iteration limit | Configurable via `with_max_function_call_loops()` | Your loop controls this |
| Use case | Simple, most common scenarios | Custom logic, fine control |

## Manual Function Calling Pattern

```rust
// Build initial request with functions
let mut response = client
    .interaction()
    .with_model("gemini-3.6-flash")
    .with_text(message)
    .add_functions(declarations)
    .with_store_enabled()
    .create()
    .await?;

// Manual function calling loop
for _ in 0..MAX_ITERATIONS {
    let function_calls = response.function_calls();

    // No function calls = done
    if function_calls.is_empty() {
        break;
    }

    // Execute functions manually
    let mut results = Vec::new();
    for call in &function_calls {
        let result = execute_function(call.name, &call.args);
        results.push(function_result_content(
            call.name.to_string(),
            call.id.unwrap().to_string(),
            result,
        ));
    }

    // Send results back
    response = client
        .interaction()
        .with_model("gemini-3.6-flash")
        .with_previous_interaction(response.id.as_ref().unwrap())
        .set_content(results)
        .with_store_enabled()
        .create()
        .await?;
}
```

## When to Use Manual Function Calling

Choose manual function calling when you need:

1. **Custom execution logic**: Rate limiting, caching, or circuit breaking
2. **Parallel execution**: Execute multiple function calls concurrently
3. **Conditional execution**: Skip certain functions based on context
4. **Custom error handling**: Different error strategies per function
5. **Detailed logging**: Monitor function calls with custom telemetry
6. **Testing/Mocking**: Easier to inject mock functions

## Running

```bash
export GEMINI_API_KEY=your_api_key
cargo run --example multi_turn_agent_manual
```

## Function Declarations

This example builds function declarations manually using `FunctionDeclaration::builder()`:

```rust
FunctionDeclaration::builder("lookup_customer")
    .description("Look up customer information by ID or email")
    .parameter("identifier", json!({
        "type": "string",
        "description": "Customer ID or email address"
    }))
    .required(vec!["identifier".to_string()])
    .build()
```

Compare this to the `#[tool]` macro approach in `multi_turn_agent_auto`.

## Sample Output

```
=== Multi-Turn Customer Support Agent (Manual Functions) ===

Simulating a customer support conversation...

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
👤 Customer (Turn 1): Hi, I'm Alice Johnson and I need help with my order.
  [Tool: lookup_customer(Alice Johnson)]

🤖 Agent:
Hello Alice! I found your account - you're a premium customer with us.
I can see you have 2 orders. How can I assist you today?
```
