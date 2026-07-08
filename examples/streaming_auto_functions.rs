//! Streaming Auto Function Calling Example
//!
//! This example demonstrates streaming with automatic function calling.
//! Unlike regular auto function calling which returns only the final response,
//! this streams content as it arrives while automatically executing functions.
//!
//! Shows how to:
//! - Stream content in real-time with functions executing between streaming rounds
//! - Track function execution events during streaming
//! - Handle multiple function calling rounds
//! - Access event_id for potential stream resumption
//!
//! # Running
//!
//! ```bash
//! cargo run --example streaming_auto_functions
//! ```
//!
//! # Prerequisites
//!
//! Set the `GEMINI_API_KEY` environment variable with your API key.

use futures_util::StreamExt;
use genai_rs::{AutoFunctionStreamChunk, CallableFunction, Client, StepDelta};
use genai_rs_macros::tool;
use std::env;
use std::io::{Write, stdout};

// Define functions using the macro - automatically registered for auto-calling.

/// Gets the current weather for a city
#[tool(city(description = "The city to get weather for"))]
fn get_weather(city: String) -> String {
    // Simulate some processing time
    std::thread::sleep(std::time::Duration::from_millis(100));
    format!(
        r#"{{"city": "{}", "temperature": "22°C", "conditions": "partly cloudy", "humidity": "65%"}}"#,
        city
    )
}

/// Gets the current time in a timezone
#[tool(timezone(description = "The timezone like UTC, PST, EST, JST"))]
fn get_time(timezone: String) -> String {
    std::thread::sleep(std::time::Duration::from_millis(100));
    format!(
        r#"{{"timezone": "{}", "time": "14:30:00", "date": "2024-12-24"}}"#,
        timezone
    )
}

/// Converts temperature between units
#[tool(
    value(description = "The temperature value"),
    from_unit(description = "Source unit: celsius or fahrenheit"),
    to_unit(description = "Target unit: celsius or fahrenheit")
)]
fn convert_temperature(value: f64, from_unit: String, to_unit: String) -> String {
    let result = if from_unit.to_lowercase() == "celsius" && to_unit.to_lowercase() == "fahrenheit"
    {
        value * 9.0 / 5.0 + 32.0
    } else if from_unit.to_lowercase() == "fahrenheit" && to_unit.to_lowercase() == "celsius" {
        (value - 32.0) * 5.0 / 9.0
    } else {
        value
    };
    format!(r#"{{"value": {:.1}, "unit": "{}"}}"#, result, to_unit)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY environment variable not set");

    let client = Client::builder(api_key).build()?;

    println!("=== STREAMING AUTO FUNCTION CALLING ===\n");

    // Get function declarations
    let weather_func = GetWeatherCallable.declaration();
    let time_func = GetTimeCallable.declaration();
    let convert_func = ConvertTemperatureCallable.declaration();

    println!("Registered functions:");
    println!(
        "  - {}: {}",
        weather_func.name(),
        weather_func.description()
    );
    println!("  - {}: {}", time_func.name(), time_func.description());
    println!(
        "  - {}: {}",
        convert_func.name(),
        convert_func.description()
    );
    println!();

    // Ask a question that requires multiple function calls
    let prompt = "What's the weather in Tokyo? Also tell me what time it is there (JST timezone).";
    println!("User: {}\n", prompt);
    println!("Response (streaming with auto function execution):\n");

    // Use create_stream_with_auto_functions - combines streaming with auto function calling
    // Returns AutoFunctionStreamEvent which wraps chunk + event_id for resume support
    let mut stream = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text(prompt)
        .add_functions(vec![weather_func, time_func, convert_func])
        .create_stream_with_auto_functions();

    let mut function_count = 0;
    let mut delta_count = 0;
    let mut arguments_delta_count = 0;
    let mut last_event_id: Option<String> = None;

    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => {
                // Track event_id for potential resume (only API events have event_id,
                // client-generated events like ExecutingFunctions have None)
                if event.event_id.is_some() {
                    last_event_id = event.event_id.clone();
                }

                match &event.chunk {
                    AutoFunctionStreamChunk::Delta(delta) => {
                        delta_count += 1;
                        // Print text content as it arrives
                        if let Some(t) = delta.as_text() {
                            print!("{}", t);
                            stdout().flush()?;
                        }
                        // NEW in revision 2026-05-20: function-call arguments stream
                        // incrementally as ArgumentsDelta fragments before execution
                        if let Some(fragment) = delta.as_arguments_delta() {
                            arguments_delta_count += 1;
                            print!("[args… {}]", fragment);
                            stdout().flush()?;
                        }
                        // Show thoughts if present (signatures are verification tokens, not readable)
                        if let StepDelta::ThoughtSignature {
                            signature: Some(_), ..
                        } = delta
                        {
                            print!("\n[Thinking...]");
                            stdout().flush()?;
                        }
                    }
                    AutoFunctionStreamChunk::ExecutingFunctions {
                        response,
                        pending_calls,
                    } => {
                        // Notification that functions are about to execute
                        // pending_calls contains the validated function calls that will be executed
                        println!("\n--- Executing Functions ---");
                        println!("  Status: {:?}", response.status);
                        for call in pending_calls {
                            function_count += 1;
                            println!("  [{}] {}({})", function_count, call.name, call.args);
                        }
                        println!("---------------------------");
                    }
                    AutoFunctionStreamChunk::FunctionResults(results) => {
                        // Function execution completed - includes timing info
                        println!("--- Function Results ---");
                        for result in results {
                            println!(
                                "  {} ({:?}) -> {}",
                                result.name, result.duration, result.result
                            );
                        }
                        println!("------------------------\n");
                        println!("Continuing response...\n");
                    }
                    AutoFunctionStreamChunk::Complete(response) => {
                        println!("\n\n--- Stream Complete ---");
                        println!("Interaction ID: {:?}", response.id);
                        println!("Status: {:?}", response.status);
                        if let Some(usage) = &response.usage {
                            println!(
                                "Tokens: {} input, {} output",
                                usage.total_input_tokens.unwrap_or(0),
                                usage.total_output_tokens.unwrap_or(0)
                            );
                        }
                    }
                    _ => {
                        // Handle unknown future variants gracefully
                        println!("[Unknown event type]");
                    }
                }
            }
            Err(e) => {
                eprintln!("\nStream error: {e}");
                break;
            }
        }
    }

    println!("\n--- Statistics ---");
    println!("Total delta chunks: {}", delta_count);
    println!(
        "Function-argument delta fragments: {}",
        arguments_delta_count
    );
    println!("Functions executed: {}", function_count);
    if let Some(event_id) = &last_event_id {
        println!("Last event_id: {}", event_id);
    }

    // =========================================================================
    // Summary
    // =========================================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Streaming Auto Function Calling Demo Complete\n");

    println!("--- Key Takeaways ---");
    println!("• create_stream_with_auto_functions() combines streaming + auto-execution");
    println!("• AutoFunctionStreamEvent wraps chunk + event_id for resume support");
    println!("• Delta chunks carry StepDelta values (text, thought, arguments_delta, ...)");
    println!("• StepDelta::ArgumentsDelta streams function-call arguments as they generate");
    println!("• ExecutingFunctions/FunctionResults show function lifecycle events");
    println!("• Functions execute between streaming rounds, then response continues\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("  [REQ#1] POST streaming with input + 3 function declarations");
    println!("  [RES#1] SSE stream → requires_action: get_weather(), get_time()");
    println!("  (library auto-executes functions)");
    println!("  [REQ#2] POST streaming with function_results + previousInteractionId (no tools)");
    println!("  [RES#2] SSE stream: text deltas → completed\n");

    println!("--- Production Considerations ---");
    println!("• ExecutingFunctions.pending_calls always has the calls that will execute");
    println!("• FunctionResults includes execution timing for performance monitoring");
    println!("• Handle stream errors gracefully - partial responses may have been sent");
    println!("• Use buffering for high-frequency UI updates");

    Ok(())
}
