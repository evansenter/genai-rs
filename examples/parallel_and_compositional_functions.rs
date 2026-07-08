//! # Parallel and Compositional Function Calling
//!
//! This example demonstrates two advanced function calling patterns:
//!
//! 1. **Parallel function calls**: The model requests multiple independent functions
//!    at once, and we execute them concurrently.
//!
//! 2. **Compositional (sequential) function calls**: The model chains functions where
//!    the output of one becomes the input to another.
//!
//! ## Key Patterns Demonstrated
//!
//! - Detecting when the model returns multiple function calls
//! - Executing parallel calls with `futures_util::future::join_all()`
//! - Handling multi-step compositional chains
//! - Function result turns don't need tools resent
//!
//! ## Running
//!
//! ```bash
//! cargo run --example parallel_and_compositional_functions
//!
//! # With wire-level debugging to see the API calls
//! LOUD_WIRE=1 cargo run --example parallel_and_compositional_functions
//! ```

use futures_util::future::join_all;
use genai_rs::{Client, FunctionDeclaration, Step};
use serde_json::{Value, json};
use std::env;
use std::error::Error;

// ============================================================================
// Simulated Functions
// ============================================================================

/// Get weather for a city (simulated)
fn get_weather(city: &str) -> Value {
    // Simulate different weather for different cities
    let (temp, condition) = match city.to_lowercase().as_str() {
        "tokyo" => (22, "partly cloudy"),
        "london" => (15, "rainy"),
        "new york" => (18, "sunny"),
        "paris" => (17, "overcast"),
        _ => (20, "clear"),
    };
    json!({
        "city": city,
        "temperature_celsius": temp,
        "condition": condition
    })
}

/// Get current time in a timezone (simulated)
fn get_time(timezone: &str) -> Value {
    // Simulate different times
    let time = match timezone.to_uppercase().as_str() {
        "JST" | "ASIA/TOKYO" => "14:30",
        "GMT" | "EUROPE/LONDON" => "05:30",
        "EST" | "AMERICA/NEW_YORK" => "00:30",
        "CET" | "EUROPE/PARIS" => "06:30",
        _ => "12:00",
    };
    json!({
        "timezone": timezone,
        "current_time": time
    })
}

/// Get user's current location (simulated) - used for compositional calls
fn get_current_location() -> Value {
    json!({
        "city": "Tokyo",
        "country": "Japan",
        "timezone": "Asia/Tokyo"
    })
}

/// Get activity recommendations based on weather (simulated)
fn get_activities(city: &str, weather_condition: &str) -> Value {
    let activities = match weather_condition {
        "rainy" => vec!["Visit a museum", "Go to a cafe", "Watch a movie"],
        "sunny" | "clear" => vec!["Go for a walk", "Visit a park", "Outdoor dining"],
        _ => vec!["Explore the city", "Try local food", "Shopping"],
    };
    json!({
        "city": city,
        "recommended_activities": activities
    })
}

/// Execute a function by name
async fn execute_function(name: &str, args: &Value) -> Value {
    // Simulate some async work
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    match name {
        "get_weather" => {
            let city = args["city"].as_str().unwrap_or("unknown");
            get_weather(city)
        }
        "get_time" => {
            let timezone = args["timezone"].as_str().unwrap_or("UTC");
            get_time(timezone)
        }
        "get_current_location" => get_current_location(),
        "get_activities" => {
            let city = args["city"].as_str().unwrap_or("unknown");
            let condition = args["weather_condition"].as_str().unwrap_or("unknown");
            get_activities(city, condition)
        }
        _ => json!({"error": format!("Unknown function: {}", name)}),
    }
}

// ============================================================================
// Function Declarations
// ============================================================================

fn get_function_declarations() -> Vec<FunctionDeclaration> {
    vec![
        FunctionDeclaration::builder("get_weather")
            .description("Get current weather for a city")
            .parameter(
                "city",
                json!({
                    "type": "string",
                    "description": "City name"
                }),
            )
            .required(vec!["city".to_string()])
            .build(),
        FunctionDeclaration::builder("get_time")
            .description("Get current time in a timezone")
            .parameter(
                "timezone",
                json!({
                    "type": "string",
                    "description": "Timezone (e.g., 'Asia/Tokyo', 'EST', 'GMT')"
                }),
            )
            .required(vec!["timezone".to_string()])
            .build(),
        FunctionDeclaration::builder("get_current_location")
            .description("Get the user's current location")
            .build(),
        FunctionDeclaration::builder("get_activities")
            .description("Get activity recommendations based on city and weather")
            .parameter(
                "city",
                json!({
                    "type": "string",
                    "description": "City name"
                }),
            )
            .parameter(
                "weather_condition",
                json!({
                    "type": "string",
                    "description": "Current weather condition (e.g., 'sunny', 'rainy')"
                }),
            )
            .required(vec!["city".to_string(), "weather_condition".to_string()])
            .build(),
    ]
}

// ============================================================================
// Demo
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not set");
    let client = Client::builder(api_key).build()?;
    let functions = get_function_declarations();

    println!("=== Parallel and Compositional Function Calling ===\n");

    // -------------------------------------------------------------------------
    // Demo 1: Parallel Function Calls
    // -------------------------------------------------------------------------
    println!("--- Demo 1: Parallel Function Calls ---");
    println!("Prompt: \"What's the weather and time in Tokyo, London, and New York?\"\n");

    let mut response = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("What's the weather and time in Tokyo, London, and New York?")
        .add_functions(functions.clone())
        .with_store_enabled()
        .create()
        .await?;

    // Function calling loop
    const MAX_ITERATIONS: usize = 5;
    for iteration in 0..MAX_ITERATIONS {
        let calls = response.function_calls();
        if calls.is_empty() {
            break;
        }

        println!(
            "Iteration {}: Model requested {} function call(s)",
            iteration + 1,
            calls.len()
        );

        // Check if we got parallel calls
        if calls.len() > 1 {
            println!("  -> Executing {} calls in PARALLEL", calls.len());
        }

        // Execute all function calls in parallel using join_all
        let futures: Vec<_> = calls
            .iter()
            .map(|call| {
                let name = call.name.to_string();
                let args = call.args.clone();
                async move {
                    let result = execute_function(&name, &args).await;
                    println!("  Executed: {}({}) -> {}", name, args, result);
                    result
                }
            })
            .collect();

        let results = join_all(futures).await;

        // Build function result steps in the same order as the calls.
        // While the API appears to correlate by call_id (not position), this
        // behavior is undocumented, so we maintain order as a best practice.
        let result_steps: Vec<_> = calls
            .iter()
            .zip(results.iter())
            .map(|(call, result)| Step::function_result(call.name, call.id, result.clone()))
            .collect();

        // Send results back - no need to resend tools on function result turns
        response = client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_previous_interaction(response.id.as_ref().unwrap())
            .with_history(result_steps) // Just the result steps, no tools needed
            .create()
            .await?;
    }

    println!("\nFinal Response:");
    println!("{}\n", response.as_text().unwrap_or("(no text)"));

    // -------------------------------------------------------------------------
    // Demo 2: Compositional (Sequential) Function Calls
    // -------------------------------------------------------------------------
    println!("--- Demo 2: Compositional Function Calls ---");
    println!(
        "Prompt: \"What activities do you recommend for my current location based on the weather?\"\n"
    );
    println!(
        "Expected chain: get_current_location() -> get_weather(location) -> get_activities(location, weather)\n"
    );

    let mut response = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("What activities do you recommend for my current location based on the weather?")
        .add_functions(functions.clone())
        .with_store_enabled()
        .create()
        .await?;

    let mut step = 0;
    for _iteration in 0..MAX_ITERATIONS {
        let calls = response.function_calls();
        if calls.is_empty() {
            break;
        }

        step += 1;
        println!("Step {}: Model called {} function(s)", step, calls.len());

        // Execute functions (could be parallel within a step)
        let futures: Vec<_> = calls
            .iter()
            .map(|call| {
                let name = call.name.to_string();
                let args = call.args.clone();
                async move {
                    let result = execute_function(&name, &args).await;
                    println!("  {}({}) -> {}", name, args, result);
                    result
                }
            })
            .collect();

        let results = join_all(futures).await;

        let result_steps: Vec<_> = calls
            .iter()
            .zip(results.iter())
            .map(|(call, result)| Step::function_result(call.name, call.id, result.clone()))
            .collect();

        // For user message turns, we'd need to resend tools
        // But for function result turns, we don't need to
        response = client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_previous_interaction(response.id.as_ref().unwrap())
            .with_history(result_steps)
            .create()
            .await?;
    }

    println!("\nFinal Response (after {} step chain):", step);
    println!("{}\n", response.as_text().unwrap_or("(no text)"));

    // =========================================================================
    // Summary
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Parallel and Compositional Function Calling Demo Complete\n");

    println!("--- Key Takeaways ---");
    println!(
        "• Parallel calls: Model returns multiple function calls at once for independent operations"
    );
    println!("• Use futures_util::future::join_all() to execute parallel calls concurrently");
    println!("• Compositional calls: Model chains functions where output informs next function");
    println!(
        "• Function result turns don't need tools resent (API remembers via previous_interaction_id)\n"
    );

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("Demo 1: Parallel (6 calls at once)");
    println!("  [REQ#1] POST with input + 4 tools");
    println!("  [RES#1] requires_action: 6 function_calls (weather + time × 3 cities)");
    println!("  [REQ#2] POST with 6 function_results + previousInteractionId (no tools)");
    println!("  [RES#2] completed: text response\n");
    println!("Demo 2: Compositional (3-step chain)");
    println!("  [REQ#3] POST with input + 4 tools");
    println!("  [RES#3] requires_action: get_current_location()");
    println!("  [REQ#4] function_result + previousInteractionId (no tools)");
    println!("  [RES#4] requires_action: get_weather(Tokyo)");
    println!("  [REQ#5] function_result + previousInteractionId (no tools)");
    println!("  [RES#5] requires_action: get_activities(Tokyo, clear)");
    println!("  [REQ#6] function_result + previousInteractionId (no tools)");
    println!("  [RES#6] completed: text response\n");

    println!("--- Production Considerations ---");
    println!("• Add timeout protection for multi-step chains (see MAX_ITERATIONS)");
    println!("• Implement retry logic for transient function failures");
    println!("• Consider rate limiting when executing many parallel calls");
    println!("• Log function execution times for performance monitoring");
    println!("• Use structured error responses for better model recovery");

    Ok(())
}
