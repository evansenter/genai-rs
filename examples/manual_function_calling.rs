//! Manual Function Calling Example
//!
//! This example demonstrates how to handle function calls manually, giving you
//! full control over the execution loop. Use this approach when you need:
//!
//! - Custom execution logic (rate limiting, caching, circuit breakers)
//! - Complex error handling and recovery
//! - Integration with external systems that require special handling
//! - Logging, metrics, or tracing around function execution
//!
//! # How It Works
//!
//! 1. Send a request with function declarations using `create()` (not `create_with_auto_functions()`)
//! 2. Check if the response contains function calls with `response.has_function_calls()`
//! 3. Execute the functions yourself
//! 4. Send results back using `Step::function_result()` and `with_previous_interaction()`
//! 5. Repeat until the model returns a text response
//!
//! # Comparison with Auto Function Calling
//!
//! | Aspect | Manual | Auto |
//! |--------|--------|------|
//! | Control | Full - you handle everything | Library handles the loop |
//! | Complexity | More code | Less code |
//! | Use case | Custom logic needed | Simple execution |
//! | Method | `create()` / `create_stream()` | `create_with_auto_functions()` |
//!
//! # Running
//!
//! ```bash
//! cargo run --example manual_function_calling
//! ```
//!
//! # Prerequisites
//!
//! Set the `GEMINI_API_KEY` environment variable with your API key.

use genai_rs::{Client, FunctionDeclaration, Step};
use serde_json::json;
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY environment variable not set");
    let client = Client::builder(api_key).build()?;

    println!("=== MANUAL FUNCTION CALLING EXAMPLE ===\n");

    // Define function declarations (schemas only - no execution logic here)
    let get_weather = FunctionDeclaration::builder("get_weather")
        .description("Get the current weather for a city")
        .parameter(
            "city",
            json!({
                "type": "string",
                "description": "The city to get weather for"
            }),
        )
        .required(vec!["city".to_string()])
        .build();

    let convert_temperature = FunctionDeclaration::builder("convert_temperature")
        .description("Convert temperature between Celsius and Fahrenheit")
        .parameter(
            "value",
            json!({
                "type": "number",
                "description": "The temperature value to convert"
            }),
        )
        .parameter(
            "from_unit",
            json!({
                "type": "string",
                "enum": ["celsius", "fahrenheit"]
            }),
        )
        .parameter(
            "to_unit",
            json!({
                "type": "string",
                "enum": ["celsius", "fahrenheit"]
            }),
        )
        .required(vec![
            "value".to_string(),
            "from_unit".to_string(),
            "to_unit".to_string(),
        ])
        .build();

    let functions = vec![get_weather, convert_temperature];

    // ==========================================================================
    // Manual Function Calling Loop
    // ==========================================================================

    let prompt = "What's the weather in Tokyo? Tell me the temperature in Fahrenheit.";
    println!("User: {}\n", prompt);

    // Step 1: Initial request with function declarations
    let mut response = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text(prompt)
        .add_functions(functions.clone())
        .create() // NOT create_with_auto_functions() - we handle execution
        .await?;

    let mut loop_count = 0;
    const MAX_LOOPS: usize = 5;

    // Manual execution loop
    while response.has_function_calls() && loop_count < MAX_LOOPS {
        loop_count += 1;
        println!("--- Loop {} ---", loop_count);

        let function_calls = response.function_calls();
        println!("Model requested {} function(s):", function_calls.len());

        // Execute each requested function
        let mut results = Vec::new();
        for call in &function_calls {
            println!("  - {}({})", call.name, call.args);

            // Execute the function (YOUR custom logic here)
            let result = execute_function(call.name, call.args);
            println!("    Result: {}", result);

            // Build a function result step (call.id links the result to the call)
            results.push(Step::function_result(call.name, call.id, result));
        }

        // Send results back to the model
        // Note: previous_interaction_id requires stored interactions (store=true, the default)
        let prev_id = response
            .id
            .as_ref()
            .ok_or("Response missing ID. Multi-turn requires store=true (the default).")?;
        response = client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_previous_interaction(prev_id) // Continue the conversation
            .with_history(results) // function_result steps as input
            .add_functions(functions.clone()) // Keep functions available
            .create()
            .await?;
    }

    // Final response
    println!("\n--- Final Response ---");
    println!("Loops executed: {}", loop_count);
    println!("Status: {:?}", response.status);

    if let Some(text) = response.as_text() {
        println!("\nAssistant: {}", text);
    }

    if loop_count >= MAX_LOOPS && response.has_function_calls() {
        println!("\n(Warning: Max loops reached, model may still want to call functions)");
    }

    // =========================================================================
    // Summary
    // =========================================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Manual Function Calling Demo Complete\n");

    println!("--- Key Takeaways ---");
    println!("• Use create() (not create_with_auto_functions()) for manual control");
    println!("• Check response.has_function_calls() to detect pending calls");
    println!("• Execute functions yourself, then send results with Step::function_result()");
    println!("• Use with_previous_interaction() to maintain conversation context\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("  [REQ#1] POST with input + 2 function declarations");
    println!("  [RES#1] requires_action: get_weather(Tokyo)");
    println!("  [REQ#2] POST with function_result + previousInteractionId + tools");
    println!("  [RES#2] requires_action: convert_temperature(22, celsius, fahrenheit)");
    println!("  [REQ#3] POST with function_result + previousInteractionId + tools");
    println!("  [RES#3] completed: text response with converted temperature\n");

    println!("--- Production Considerations ---");
    println!("• Implement MAX_LOOPS to prevent infinite function call chains");
    println!("• Add custom error handling, logging, and metrics in execute_function()");
    println!("• Consider rate limiting and circuit breakers for external API calls");
    println!("• Validate function arguments before execution");

    Ok(())
}

/// Execute a function by name with the given arguments.
///
/// This is where YOU implement your custom logic. In a real application,
/// this might:
/// - Call external APIs
/// - Query databases
/// - Apply rate limiting
/// - Log/trace execution
/// - Handle errors with custom recovery
fn execute_function(name: &str, args: &serde_json::Value) -> serde_json::Value {
    match name {
        "get_weather" => {
            let city = args
                .get("city")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");

            // Simulated weather data
            json!({
                "city": city,
                "temperature": 22.0,
                "unit": "celsius",
                "conditions": "partly cloudy",
                "humidity": "65%"
            })
        }
        "convert_temperature" => {
            let value = args.get("value").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let from = args
                .get("from_unit")
                .and_then(|v| v.as_str())
                .unwrap_or("celsius");
            let to = args
                .get("to_unit")
                .and_then(|v| v.as_str())
                .unwrap_or("fahrenheit");

            let converted = if from == "celsius" && to == "fahrenheit" {
                value * 9.0 / 5.0 + 32.0
            } else if from == "fahrenheit" && to == "celsius" {
                (value - 32.0) * 5.0 / 9.0
            } else {
                value
            };

            json!({
                "value": converted,
                "unit": to
            })
        }
        _ => {
            json!({
                "error": format!("Unknown function: {}", name)
            })
        }
    }
}
