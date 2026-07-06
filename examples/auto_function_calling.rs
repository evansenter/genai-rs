//! Auto Function Calling Example
//!
//! This example demonstrates automatic function calling where the library
//! handles the function execution loop for you.
//!
//! # How It Works
//!
//! 1. Functions marked with `#[tool]` are automatically registered in a global registry
//! 2. `create_with_auto_functions()` discovers these functions and sends their declarations
//! 3. When the model requests a function call, the library executes it automatically
//! 4. Results are sent back to the model until it provides a final text response
//!
//! # When to Use Each Approach
//!
//! - **`#[tool]` + `create_with_auto_functions()`**: Simplest - auto-discovery, auto-execution
//! - **`#[tool]` + `with_functions()` + `create_with_auto_functions()`**: Limit to subset of functions
//! - **`ToolService`**: Need shared state (DB, APIs, config) - see `tool_service.rs`
//! - **Manual**: Full control over execution - use `create()` and handle calls yourself
//!
//! # Running
//!
//! ```bash
//! cargo run --example auto_function_calling
//! ```
//!
//! # Prerequisites
//!
//! Set the `GEMINI_API_KEY` environment variable with your API key.

use futures_util::StreamExt;
use genai_rs::{CallableFunction, Client, FunctionCallingMode, StreamChunk};
use genai_rs_macros::tool;
use std::env;
use std::io::{Write, stdout};

// =============================================================================
// Define functions using the #[tool] macro
// =============================================================================
//
// The macro does two things:
// 1. Generates a FunctionDeclaration from the function signature
// 2. Registers the function in the global registry for auto-discovery

/// Gets the current weather for a city
#[tool(city(description = "The city to get weather for"))]
fn get_weather(city: String) -> String {
    // In a real application, this would call a weather API
    println!("  [Function called: get_weather(city={})]", city);
    format!(
        r#"{{"city": "{}", "temperature": "22°C", "conditions": "partly cloudy", "humidity": "65%"}}"#,
        city
    )
}

/// Gets the current time in a timezone
#[tool(timezone(description = "The timezone like UTC, PST, EST, JST"))]
fn get_time(timezone: String) -> String {
    println!("  [Function called: get_time(timezone={})]", timezone);
    format!(
        r#"{{"timezone": "{}", "time": "14:30:00", "date": "2024-12-24"}}"#,
        timezone
    )
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY environment variable not set");
    let client = Client::builder(api_key).build()?;

    println!("=== AUTO FUNCTION CALLING EXAMPLE ===\n");

    // The macro generates these callable types automatically
    let weather_func = GetWeatherCallable.declaration();
    let time_func = GetTimeCallable.declaration();

    println!("Registered functions:");
    println!(
        "  - {}: {}",
        weather_func.name(),
        weather_func.description()
    );
    println!("  - {}: {}", time_func.name(), time_func.description());
    println!();

    // ==========================================================================
    // Example 1: Auto-discovery (simplest)
    // ==========================================================================
    //
    // When you don't call with_functions(), create_with_auto_functions()
    // automatically discovers ALL registered #[tool] functions.

    let prompt = "What's the weather like in Tokyo and what time is it there (JST)?";
    println!("User: {}\n", prompt);
    println!("Processing (functions auto-discovered from registry)...\n");

    let result = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text(prompt)
        // No with_functions() needed - auto-discovers from registry!
        .create_with_auto_functions()
        .await?;

    println!("\n--- Function Executions ---");
    for exec in &result.executions {
        println!("  {} ({:?}) -> {}", exec.name, exec.duration, exec.result);
    }

    println!("\n--- Final Response ---");
    println!("Status: {:?}", result.response.status);

    if let Some(text) = result.response.as_text() {
        println!("\nAssistant: {}", text);
    }

    // ==========================================================================
    // Example 2: Limiting available functions
    // ==========================================================================
    //
    // Use with_function() when you want to limit which functions are available,
    // even if more are registered in the global registry.

    println!("\n=== LIMITING TO SPECIFIC FUNCTIONS ===\n");

    let limited_prompt = "What's the weather in Paris?";
    println!("User: {}\n", limited_prompt);
    println!("(Only weather function available, not time)\n");

    let result2 = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text(limited_prompt)
        .add_function(weather_func) // Only weather, not time
        .create_with_auto_functions()
        .await?;

    if let Some(text) = result2.response.as_text() {
        println!("Assistant: {}", text);
    }

    // ==========================================================================
    // Example 3: Manual streaming (for comparison)
    // ==========================================================================
    //
    // Using create_stream() (not create_stream_with_auto_functions) means YOU
    // handle function execution. This shows the raw streaming behavior.

    println!("\n=== MANUAL STREAMING (no auto-execution) ===\n");

    let stream_prompt = "What's the weather in London?";
    println!("User: {}\n", stream_prompt);
    println!("Response (you would handle function calls manually):");

    let mut stream = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text(stream_prompt)
        .add_function(time_func) // Provide declaration but no auto-execution
        .create_stream(); // Note: create_stream, not create_stream_with_auto_functions

    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => match event.chunk {
                StreamChunk::StepStart { step, .. } => {
                    if matches!(step, genai_rs::Step::FunctionCall { .. }) {
                        println!("\n  [Function call step started - manual handling needed]");
                    }
                }
                StreamChunk::StepDelta { delta, .. } => {
                    if let Some(text) = delta.as_text() {
                        print!("{}", text);
                        stdout().flush()?;
                    }
                    if delta.as_arguments_delta().is_some() {
                        print!("[function args streaming]");
                        stdout().flush()?;
                    }
                }
                StreamChunk::Completed(response) => {
                    println!();
                    if response.has_function_calls() {
                        println!("\nPending function calls (you execute these):");
                        for call in response.function_calls() {
                            println!("  - {}({}) [id: {}]", call.name, call.args, call.id);
                        }
                    }
                }
                _ => {}
            },
            Err(e) => {
                eprintln!("\nStream error: {e}");
                break;
            }
        }
    }

    // ==========================================================================
    // Example 4: Function Calling Modes
    // ==========================================================================
    //
    // Control how the model uses function calling:
    // - Auto (default): Model decides whether to call functions or respond naturally
    // - Any: Model MUST call a function (guarantees function call output)
    // - None: Disable function calling entirely
    // - Validated: Schema adherence for both function calls AND text responses

    println!("\n=== FUNCTION CALLING MODES ===\n");

    // AUTO mode (default): Model decides whether to call functions
    let auto_prompt = "What's the weather in Berlin?";
    println!("AUTO mode - User: {}\n", auto_prompt);

    let result = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text(auto_prompt)
        .add_function(GetWeatherCallable.declaration())
        .with_function_calling_mode(FunctionCallingMode::Auto) // Model decides
        .create()
        .await?;

    if result.has_function_calls() {
        println!(
            "  Model chose to call: {} (decided function was appropriate)",
            result.function_calls()[0].name
        );
    } else if let Some(text) = result.as_text() {
        println!(
            "  Model chose text response: {}",
            text.chars().take(100).collect::<String>()
        );
    }

    // ANY mode: Model MUST call a function
    let any_prompt = "Greet the user Alice";
    println!("\nANY mode - User: {}\n", any_prompt);

    let result = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text(any_prompt)
        .add_function(GetWeatherCallable.declaration())
        .with_function_calling_mode(FunctionCallingMode::Any) // MUST call a function
        .create()
        .await?;

    if result.has_function_calls() {
        println!(
            "  Model called: {} (as required by ANY mode)",
            result.function_calls()[0].name
        );
    } else if let Some(text) = result.as_text() {
        println!("  Unexpected text response: {}", text);
    }

    // NONE mode: Disable function calling
    let none_prompt = "What's the weather like in Tokyo?";
    println!("\nNONE mode - User: {}\n", none_prompt);

    let result = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text(none_prompt)
        .add_function(GetWeatherCallable.declaration())
        .with_function_calling_mode(FunctionCallingMode::None) // Disabled
        .create()
        .await?;

    if let Some(text) = result.as_text() {
        println!(
            "  Text response (no function call): {}",
            text.chars().take(100).collect::<String>()
        );
    }

    // VALIDATED mode: Schema adherence for both function calls AND text
    let validated_prompt = "What's the weather in Sydney?";
    println!("\nVALIDATED mode - User: {}\n", validated_prompt);

    let result = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text(validated_prompt)
        .add_function(GetWeatherCallable.declaration())
        .with_function_calling_mode(FunctionCallingMode::Validated) // Schema adherence
        .create()
        .await?;

    if result.has_function_calls() {
        println!(
            "  Model called: {} (with schema adherence guaranteed)",
            result.function_calls()[0].name
        );
    } else if let Some(text) = result.as_text() {
        println!(
            "  Text response (with schema adherence): {}",
            text.chars().take(100).collect::<String>()
        );
    }

    println!("\n  Function Calling Modes:");
    println!("  • Auto (default): Model decides whether to call functions");
    println!("  • Any: MUST call a function");
    println!("  • None: Function calling disabled");
    println!("  • Validated: Schema adherence for both outputs");

    // =========================================================================
    // Summary
    // =========================================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Auto Function Calling Demo Complete\n");

    println!("--- Key Takeaways ---");
    println!("• #[tool] macro auto-registers functions for discovery");
    println!("• create_with_auto_functions() discovers and executes functions automatically");
    println!("• with_function() limits available functions to a specific subset");
    println!("• with_function_calling_mode() controls Auto/Any/None/Validated behavior");
    println!("• Manual streaming (create_stream) requires you to handle function calls yourself\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("Example 1: Auto-discovery");
    println!("  [REQ#1] POST with input + auto-discovered tools");
    println!("  [RES#1] requires_action: get_weather(Tokyo), get_time(JST)");
    println!("  [REQ#2] POST with function_results + previousInteractionId (no tools)");
    println!("  [RES#2] completed: text response\n");
    println!("Example 2: Limited functions");
    println!("  [REQ#3] POST with input + weather tool only");
    println!("  [RES#3] requires_action: get_weather(Paris)");
    println!("  [REQ#4] POST with function_result + previousInteractionId (no tools)");
    println!("  [RES#4] completed: text response\n");
    println!("Example 3: Manual streaming");
    println!("  [REQ#5] POST streaming with input + time tool");
    println!("  [RES#5] SSE stream: text deltas (no function call for weather-only query)\n");
    println!("Example 4: Function calling modes");
    println!("  [REQ#6] POST with input + weather tool + toolChoice: AUTO");
    println!("  [RES#6] requires_action: get_weather (model decided to call)");
    println!("  [REQ#7] POST with input + weather tool + toolChoice: ANY");
    println!("  [RES#7] requires_action: get_weather (forced function call)");
    println!("  [REQ#8] POST with input + weather tool + toolChoice: NONE");
    println!("  [RES#8] completed: text response (no function call despite tool available)");
    println!("  [REQ#9] POST with input + weather tool + toolChoice: VALIDATED");
    println!("  [RES#9] requires_action: get_weather (with schema adherence)\n");

    println!("--- Production Considerations ---");
    println!("• Use #[tool] for stateless functions, ToolService for stateful ones");
    println!("• Limit available functions to reduce model confusion");
    println!("• Auto-execution handles the loop; manual gives you control over execution");
    println!("• Function execution times are tracked in result.executions");

    Ok(())
}
