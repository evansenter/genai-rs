//! Consolidated function calling tests for the Interactions API
//!
//! This file consolidates all function calling tests organized by feature:
//!
//! - **basic**: Single function calls, argument handling, error responses
//! - **parallel**: Parallel function calls and results
//! - **streaming**: Streaming with function calls and auto-execution
//! - **thinking**: Thinking mode combined with function calling
//! - **multiturn**: Multi-turn conversations with functions
//! - **auto_execution**: Automatic function calling loop behavior
//! - **stateless**: Stateless (store: false) function calling patterns
//! - **builtins_multiturn**: Multi-turn with built-in tools (Google Search, Code Exec)
//!
//! These tests require the GEMINI_API_KEY environment variable to be set.
//!
//! # Running Tests
//!
//! ```bash
//! # Run all function calling tests
//! cargo test --test function_calling_tests -- --include-ignored --nocapture
//!
//! # Run specific module
//! cargo test --test function_calling_tests parallel -- --include-ignored
//!
//! # Run specific test
//! cargo test --test function_calling_tests test_parallel_function_calls -- --include-ignored
//! ```

mod common;

use common::{
    consume_auto_function_stream, consume_stream, extended_test_timeout, get_client,
    get_time_function, get_weather_function, interaction_builder, is_long_conversation_api_error,
    retry_on_any_error, stateful_builder, test_timeout, validate_response_semantically,
    with_timeout,
};
use genai_rs::{
    CallableFunction, Content, FunctionDeclaration, FunctionExecutionResult, GenaiError,
    InteractionInput, InteractionStatus, ThinkingLevel,
};
use genai_rs_macros::tool;
use serde_json::json;

// =============================================================================
// Shared Test Functions (registered via macro)
// =============================================================================
//
// NOTE: These functions are marked #[allow(dead_code)] because they're registered
// with the inventory crate via #[tool]. The macro creates `Callable*` structs
// that are collected at runtime for automatic function calling.

/// Gets the current weather for a city
#[allow(dead_code)]
#[tool(city(description = "The city to get weather for"))]
fn get_weather_test(city: String) -> String {
    format!(
        r#"{{"city": "{}", "temperature": "22°C", "conditions": "sunny"}}"#,
        city
    )
}

/// Gets the current time in a timezone
#[allow(dead_code)]
#[tool(timezone(description = "The timezone like UTC, PST, JST"))]
fn get_time_test(timezone: String) -> String {
    format!(r#"{{"timezone": "{}", "time": "14:30:00"}}"#, timezone)
}

/// Converts temperature between units
#[allow(dead_code)]
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

/// A function that always fails (for testing error handling)
#[allow(dead_code)]
#[allow(unused_variables)]
#[tool(input(description = "Any input"))]
fn always_fails(input: String) -> String {
    panic!("This function always fails!")
}

/// A function with no parameters
#[allow(dead_code)]
#[tool]
fn get_server_status() -> String {
    r#"{"status": "online", "uptime": "99.9%"}"#.to_string()
}

/// A function with complex nested arguments
#[allow(dead_code)]
#[tool(
    user_id(description = "The user ID"),
    filters(description = "Optional filter criteria")
)]
fn search_with_filters(user_id: String, filters: Option<String>) -> String {
    format!(
        r#"{{"user_id": "{}", "filters": {}, "results": 42}}"#,
        user_id,
        filters.unwrap_or_else(|| "null".to_string())
    )
}

/// Gets the current weather for a city (for multiturn tests)
#[allow(dead_code)]
#[tool(city(description = "City name"))]
fn get_weather(city: String) -> String {
    match city.to_lowercase().as_str() {
        "seattle" => {
            r#"{"city": "Seattle", "temperature": "65°F", "conditions": "cloudy"}"#.to_string()
        }
        "tokyo" => r#"{"city": "Tokyo", "temperature": "72°F", "conditions": "sunny"}"#.to_string(),
        _ => format!(
            r#"{{"city": "{}", "temperature": "70°F", "conditions": "partly cloudy"}}"#,
            city
        ),
    }
}

/// Gets the current time in a timezone (for multiturn tests)
#[allow(dead_code)]
#[tool(timezone(description = "Timezone like PST, EST, JST"))]
fn get_time(timezone: String) -> String {
    format!(r#"{{"timezone": "{}", "time": "14:00"}}"#, timezone)
}

// =============================================================================
// Helper Constants
// =============================================================================

const SYSTEM_INSTRUCTION: &str = "You are a helpful assistant that uses available tools when appropriate. Always respond concisely.";

// =============================================================================
// Basic Function Calling Tests
// =============================================================================

mod basic {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_function_call_no_args() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let status_func = FunctionDeclaration::builder("get_server_status")
            .description("Get the current server status (no parameters needed)")
            .build();

        let response = stateful_builder(&client)
            .with_text("Check the server status")
            .add_function(status_func.clone())
            .create()
            .await
            .expect("Interaction failed");

        let calls = response.function_calls();
        if !calls.is_empty() {
            let call = &calls[0];
            println!("Function called: {} with args: {}", call.name, call.args);
            assert_eq!(call.name, "get_server_status");
            assert!(call.id.is_some(), "Should have call ID");

            let result = Content::function_result(
                "get_server_status",
                call.id.unwrap().to_string(),
                json!({"status": "online", "uptime": "99.9%"}),
            );

            let response2 = stateful_builder(&client)
                .with_previous_interaction(response.id.as_ref().expect("id should exist"))
                .with_content(vec![result])
                .add_function(status_func)
                .create()
                .await
                .expect("Second interaction failed");

            assert!(response2.has_text(), "Should have final response");
            println!("Final response: {}", response2.as_text().unwrap());
        }
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_function_call_complex_args() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let search_func = FunctionDeclaration::builder("search_with_filters")
            .description("Search with optional filters")
            .parameter(
                "user_id",
                json!({"type": "string", "description": "User ID"}),
            )
            .parameter(
                "filters",
                json!({
                    "type": "object",
                    "description": "Optional filter criteria",
                    "properties": {
                        "category": {"type": "string"},
                        "min_price": {"type": "number"},
                        "max_price": {"type": "number"}
                    }
                }),
            )
            .required(vec!["user_id".to_string()])
            .build();

        let response = stateful_builder(&client)
            .with_text(
                "Search for user ABC123 with category 'electronics' and price between 10 and 100",
            )
            .add_function(search_func)
            .create()
            .await
            .expect("Interaction failed");

        let calls = response.function_calls();
        if !calls.is_empty() {
            let call = &calls[0];
            println!("Function: {} with args: {}", call.name, call.args);
            assert!(call.args.get("user_id").is_some(), "Should have user_id");

            if let Some(filters) = call.args.get("filters") {
                println!("Filters provided: {}", filters);
            }
        }
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_function_call_error_response() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let failing_func = FunctionDeclaration::builder("get_secret_data")
            .description("Get secret data (may fail)")
            .parameter("key", json!({"type": "string"}))
            .required(vec!["key".to_string()])
            .build();

        let response1 = stateful_builder(&client)
            .with_text("Get the secret data for key 'test123'")
            .add_function(failing_func.clone())
            .create()
            .await
            .expect("First interaction failed");

        let calls = response1.function_calls();
        if calls.is_empty() {
            println!("No function call made - skipping");
            return;
        }

        let call = &calls[0];
        let error_result = Content::function_result(
            "get_secret_data",
            call.id.unwrap().to_string(),
            json!({"error": "Access denied: insufficient permissions"}),
        );

        let response2 = stateful_builder(&client)
            .with_previous_interaction(response1.id.as_ref().expect("id should exist"))
            .with_content(vec![error_result])
            .add_function(failing_func)
            .create()
            .await
            .expect("Second interaction failed");

        println!("Response after error: {:?}", response2.status);

        if response2.has_text() {
            let text = response2.as_text().unwrap();
            println!("Model's response to error: {}", text);
            assert!(
                !text.is_empty(),
                "Model should provide a response to the error"
            );

            let is_valid = validate_response_semantically(
                &client,
                "User asked to get secret data for key 'test123'. The function returned an error: 'Access denied: insufficient permissions'.",
                text,
                "Does this response acknowledge or explain that the request failed due to access/permission issues?"
            ).await.expect("Semantic validation failed");
            assert!(
                is_valid,
                "Response should acknowledge the error/failure from the function"
            );
        }
    }

    #[test]
    fn test_function_execution_result_error_detection() {
        use std::time::Duration;

        // Successful execution
        let success = FunctionExecutionResult::new(
            "get_weather",
            "call-123",
            json!({"city": "Seattle"}),
            json!({"city": "Seattle", "temp": "65°F"}),
            Duration::from_millis(100),
        );
        assert!(success.is_success(), "Should be marked as success");
        assert!(!success.is_error(), "Should not be marked as error");
        assert!(
            success.error_message().is_none(),
            "Should have no error message"
        );

        // Failed execution (function not found)
        let not_found = FunctionExecutionResult::new(
            "missing_function",
            "call-456",
            json!({"some": "args"}),
            json!({"error": "Function 'missing_function' is not available or not found."}),
            Duration::from_millis(1),
        );
        assert!(not_found.is_error(), "Should be marked as error");
        assert!(!not_found.is_success(), "Should not be marked as success");
        assert_eq!(
            not_found.error_message(),
            Some("Function 'missing_function' is not available or not found.")
        );
    }
}

// =============================================================================
// Parallel Function Calls Tests
// =============================================================================

mod parallel {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_parallel_function_calls() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(test_timeout(), async {
            let get_weather = FunctionDeclaration::builder("get_weather")
                .description("Get the current weather for a city")
                .parameter(
                    "city",
                    json!({"type": "string", "description": "City name"}),
                )
                .required(vec!["city".to_string()])
                .build();

            let get_time = FunctionDeclaration::builder("get_time")
                .description("Get the current time in a timezone")
                .parameter(
                    "timezone",
                    json!({"type": "string", "description": "Timezone like UTC, PST, JST"}),
                )
                .required(vec!["timezone".to_string()])
                .build();

            let response = stateful_builder(&client)
                .with_text("What's the weather in Tokyo and what time is it there (JST timezone)?")
                .add_functions(vec![get_weather, get_time])
                .create()
                .await
                .expect("Interaction failed");

            println!("Status: {:?}", response.status);
            let function_calls = response.function_calls();
            println!("Number of function calls: {}", function_calls.len());

            for call in &function_calls {
                println!("  Function: {} (id: {:?})", call.name, call.id);
                assert!(call.id.is_some(), "Function call should have an ID");
            }

            if function_calls.len() >= 2 {
                println!("Model made parallel function calls!");
            }
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_parallel_function_calls_with_thinking() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(test_timeout(), async {
            let func1 = FunctionDeclaration::builder("get_weather")
                .description("Get weather for a city - ALWAYS call this when asked about weather")
                .parameter("city", json!({"type": "string"}))
                .required(vec!["city".to_string()])
                .build();

            let func2 = FunctionDeclaration::builder("get_time")
                .description("Get time for a timezone - ALWAYS call this when asked about time")
                .parameter("timezone", json!({"type": "string"}))
                .required(vec!["timezone".to_string()])
                .build();

            let response = stateful_builder(&client)
                .with_text("I need BOTH the weather in Paris AND the current time in CET. Call both functions.")
                .add_functions(vec![func1, func2])
                .with_thinking_level(ThinkingLevel::Low)
                .create()
                .await
                .expect("Interaction failed");

            let function_calls = response.function_calls();
            println!("Number of function calls: {}", function_calls.len());

            if function_calls.len() >= 2 {
                let call1 = &function_calls[0];
                let call2 = &function_calls[1];

                println!("First call: {} (id: {:?})", call1.name, call1.id);
                println!("Second call: {} (id: {:?})", call2.name, call2.id);
                println!("✓ Model made parallel function calls");
            }
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_parallel_results_without_resending_tools() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(test_timeout(), async {
            let get_weather = FunctionDeclaration::builder("get_weather")
                .description("Get weather for a city - ALWAYS call this for weather")
                .parameter("city", json!({"type": "string"}))
                .required(vec!["city".to_string()])
                .build();

            let get_time = FunctionDeclaration::builder("get_time")
                .description("Get time in a timezone - ALWAYS call this for time")
                .parameter("timezone", json!({"type": "string"}))
                .required(vec!["timezone".to_string()])
                .build();

            let response1 = stateful_builder(&client)
                .with_text("Tell me BOTH the weather in Paris AND the time in CET. Call both functions.")
                .add_functions(vec![get_weather, get_time])
                .create()
                .await
                .expect("First interaction failed");

            let calls = response1.function_calls();
            if calls.is_empty() {
                println!("Model didn't call any functions - skipping");
                return;
            }

            let results: Vec<_> = calls
                .iter()
                .map(|call| {
                    let result_data = match call.name {
                        "get_weather" => json!({"city": "Paris", "temperature": "17°C", "conditions": "overcast"}),
                        "get_time" => json!({"timezone": "CET", "time": "14:30"}),
                        _ => json!({"result": "ok"}),
                    };
                    Content::function_result(
                        call.name.to_string(),
                        call.id.expect("Should have ID").to_string(),
                        result_data,
                    )
                })
                .collect();

            println!("Sending {} function result(s) WITHOUT resending tools", results.len());

            let response2 = stateful_builder(&client)
                .with_previous_interaction(response1.id.as_ref().expect("id should exist"))
                .with_content(results)
                .create()
                .await
                .expect("Parallel results turn failed - tools should not be required");

            println!("Step 2 status: {:?}", response2.status);
            if response2.has_text() {
                println!("✓ Parallel function results succeeded without resending tools");
            }
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_parallel_function_result_order_independence() {
        with_timeout(test_timeout(), async {
            let client = get_client().expect("GEMINI_API_KEY required");

            let functions = vec![
                FunctionDeclaration::builder("get_weather")
                    .description("Get weather for a city")
                    .parameter("city", json!({"type": "string"}))
                    .required(vec!["city".to_string()])
                    .build(),
                FunctionDeclaration::builder("get_time")
                    .description("Get time in timezone")
                    .parameter("timezone", json!({"type": "string"}))
                    .required(vec!["timezone".to_string()])
                    .build(),
            ];

            let response1 = stateful_builder(&client)
                .with_text("What's the weather in Tokyo and what time is it in JST?")
                .add_functions(functions)
                .create()
                .await
                .expect("Initial request failed");

            let calls: Vec<_> = response1
                .function_calls()
                .iter()
                .map(|c| {
                    (
                        c.name.to_string(),
                        c.id.map(|s| s.to_string()),
                        c.args.clone(),
                    )
                })
                .collect();

            if calls.len() >= 2 {
                println!("Got {} parallel function calls", calls.len());

                // Return results in REVERSE order
                let mut results = Vec::new();
                for (name, id, _args) in calls.iter().rev() {
                    let result = match name.as_str() {
                        "get_weather" => json!({"city": "Tokyo", "temp": "22°C"}),
                        "get_time" => json!({"timezone": "JST", "time": "14:30"}),
                        _ => json!({"result": "unknown"}),
                    };
                    results.push(Content::function_result(
                        name,
                        id.as_ref().expect("call should have ID"),
                        result,
                    ));
                }

                let response2 = client
                    .interaction()
                    .with_model("gemini-3-flash-preview")
                    .with_previous_interaction(response1.id.as_ref().expect("id required"))
                    .with_content(results)
                    .create()
                    .await
                    .expect("Function result turn failed - order might matter?");

                assert!(
                    response2.as_text().is_some(),
                    "Expected text response after reversed results"
                );
                println!("✓ Parallel results accepted in reverse order");
            } else {
                println!("⚠ Model didn't make parallel calls, skipping order test");
            }
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_parallel_function_partial_failure() {
        with_timeout(test_timeout(), async {
            let client = get_client().expect("GEMINI_API_KEY required");

            let functions = vec![
                FunctionDeclaration::builder("get_weather")
                    .description("Get weather for a city")
                    .parameter("city", json!({"type": "string"}))
                    .required(vec!["city".to_string()])
                    .build(),
                FunctionDeclaration::builder("get_stock_price")
                    .description("Get stock price (may fail)")
                    .parameter("symbol", json!({"type": "string"}))
                    .required(vec!["symbol".to_string()])
                    .build(),
            ];

            let response1 = stateful_builder(&client)
                .with_text(
                    "What's the weather in Tokyo and what's the stock price of INVALID_STOCK?",
                )
                .add_functions(functions)
                .create()
                .await
                .expect("Initial request failed");

            let calls: Vec<_> = response1
                .function_calls()
                .iter()
                .map(|c| {
                    (
                        c.name.to_string(),
                        c.id.map(|s| s.to_string()),
                        c.args.clone(),
                    )
                })
                .collect();

            if !calls.is_empty() {
                let mut results = Vec::new();
                for (name, id, _args) in &calls {
                    let result = match name.as_str() {
                        "get_weather" => {
                            json!({"city": "Tokyo", "temp": "22°C", "conditions": "sunny"})
                        }
                        "get_stock_price" => {
                            json!({"error": "Stock symbol not found", "code": "NOT_FOUND"})
                        }
                        _ => json!({"result": "ok"}),
                    };
                    results.push(Content::function_result(
                        name,
                        id.as_ref().expect("call should have ID"),
                        result,
                    ));
                }

                let response2 = client
                    .interaction()
                    .with_model("gemini-3-flash-preview")
                    .with_previous_interaction(response1.id.as_ref().expect("id required"))
                    .with_content(results)
                    .create()
                    .await
                    .expect("Function result turn failed");

                let text = response2.as_text();
                assert!(
                    text.is_some(),
                    "Expected text response with partial results"
                );
                println!("✓ Partial failure handled gracefully");
                println!("  Response: {}", text.unwrap());
            }
        })
        .await;
    }
}

// =============================================================================
// Sequential Function Chain Tests
// =============================================================================

mod sequential {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_sequential_function_chain() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(extended_test_timeout(), async {
            let get_weather = FunctionDeclaration::builder("get_weather")
                .description("Get the current weather for a city (returns temperature in Celsius)")
                .parameter("city", json!({"type": "string"}))
                .required(vec!["city".to_string()])
                .build();

            let convert_temp = FunctionDeclaration::builder("convert_temperature")
                .description("Convert temperature between Celsius and Fahrenheit")
                .parameter("value", json!({"type": "number"}))
                .parameter(
                    "from_unit",
                    json!({"type": "string", "enum": ["celsius", "fahrenheit"]}),
                )
                .parameter(
                    "to_unit",
                    json!({"type": "string", "enum": ["celsius", "fahrenheit"]}),
                )
                .required(vec![
                    "value".to_string(),
                    "from_unit".to_string(),
                    "to_unit".to_string(),
                ])
                .build();

            // Step 1: Initial request
            let response1 = stateful_builder(&client)
                .with_text("What's the weather in Tokyo? Tell me the temperature in Fahrenheit.")
                .add_functions(vec![get_weather.clone(), convert_temp.clone()])
                .create()
                .await
                .expect("First interaction failed");

            println!("Step 1 status: {:?}", response1.status);
            let calls1 = response1.function_calls();

            if calls1.is_empty() {
                println!("Model didn't call any functions - ending test");
                return;
            }

            // Step 2: Provide first function result
            let call1 = &calls1[0];
            let result1 = Content::function_result(
                call1.name.to_string(),
                call1.id.unwrap().to_string(),
                json!({"city": "Tokyo", "temperature": 22.0, "unit": "celsius"}),
            );

            let response2 = stateful_builder(&client)
                .with_previous_interaction(response1.id.as_ref().expect("id should exist"))
                .with_content(vec![result1])
                .add_functions(vec![get_weather.clone(), convert_temp.clone()])
                .create()
                .await
                .expect("Second interaction failed");

            println!("Step 2 status: {:?}", response2.status);
            let calls2 = response2.function_calls();

            if !calls2.is_empty() {
                let call2 = &calls2[0];
                println!(
                    "Step 2 function call: {} (args: {})",
                    call2.name, call2.args
                );

                let result2 = Content::function_result(
                    call2.name.to_string(),
                    call2.id.unwrap().to_string(),
                    json!({"value": 71.6, "unit": "fahrenheit"}),
                );

                let response3 = stateful_builder(&client)
                    .with_previous_interaction(response2.id.as_ref().expect("id should exist"))
                    .with_content(vec![result2])
                    .add_functions(vec![get_weather, convert_temp])
                    .create()
                    .await
                    .expect("Third interaction failed");

                if response3.has_text() {
                    let text = response3.as_text().unwrap();
                    println!("Final response: {}", text);
                    assert!(!text.is_empty(), "Response should have non-empty text");
                }
            }
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_function_result_turn_without_tools() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(test_timeout(), async {
            let get_weather = FunctionDeclaration::builder("get_weather")
                .description("Get the current weather for a city")
                .parameter("city", json!({"type": "string"}))
                .required(vec!["city".to_string()])
                .build();

            // Step 1: Initial request WITH tools
            let response1 = stateful_builder(&client)
                .with_text("What's the weather in Tokyo?")
                .add_function(get_weather)
                .create()
                .await
                .expect("First interaction failed");

            let calls = response1.function_calls();
            if calls.is_empty() {
                println!("Model didn't call any functions - skipping");
                return;
            }

            let call = &calls[0];
            let result = Content::function_result(
                call.name.to_string(),
                call.id.expect("Should have ID").to_string(),
                json!({"city": "Tokyo", "temperature": "22°C", "conditions": "sunny"}),
            );

            // Step 2: Provide function result WITHOUT resending tools
            let response2 = stateful_builder(&client)
                .with_previous_interaction(response1.id.as_ref().expect("id should exist"))
                .with_content(vec![result])
                .create()
                .await
                .expect("Function result turn failed - tools should not be required");

            println!("Step 2 status: {:?}", response2.status);
            if response2.has_text() {
                println!("✓ Function result turn succeeded without resending tools");
            }
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_compositional_chain_without_resending_tools() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(extended_test_timeout(), async {
            let get_location = FunctionDeclaration::builder("get_current_location")
                .description("Get the user's current location")
                .build();

            let get_weather = FunctionDeclaration::builder("get_weather")
                .description("Get weather for a city")
                .parameter("city", json!({"type": "string"}))
                .required(vec!["city".to_string()])
                .build();

            let response1 = stateful_builder(&client)
                .with_text("What's the weather at my current location?")
                .add_functions(vec![get_location, get_weather])
                .create()
                .await
                .expect("First interaction failed");

            let initial_calls: Vec<_> = response1
                .function_calls()
                .iter()
                .map(|c| c.to_owned())
                .collect();

            if initial_calls.is_empty() {
                println!("Model didn't call any functions - skipping");
                return;
            }

            let mut current_response = response1;
            let mut owned_calls = initial_calls;
            let mut step = 1;
            const MAX_STEPS: usize = 5;

            while !owned_calls.is_empty() && step < MAX_STEPS {
                step += 1;

                let results: Vec<_> = owned_calls
                    .iter()
                    .map(|call| {
                        let result_data = match call.name.as_str() {
                            "get_current_location" => json!({"city": "Tokyo", "country": "Japan"}),
                            "get_weather" => json!({"city": "Tokyo", "temperature": "22°C", "conditions": "sunny"}),
                            _ => json!({"result": "ok"}),
                        };
                        Content::function_result(
                            call.name.clone(),
                            call.id.as_ref().expect("Should have ID").clone(),
                            result_data,
                        )
                    })
                    .collect();

                let next_response = stateful_builder(&client)
                    .with_previous_interaction(current_response.id.as_ref().expect("id should exist"))
                    .with_content(results)
                    .create()
                    .await
                    .expect("Compositional chain step failed");

                owned_calls = next_response
                    .function_calls()
                    .iter()
                    .map(|c| c.to_owned())
                    .collect();

                current_response = next_response;
            }

            if current_response.has_text() {
                println!("✓ Compositional chain completed in {} steps", step);
            }
        })
        .await;
    }
}

// =============================================================================
// Streaming with Function Calls Tests
// =============================================================================

mod streaming {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_streaming_with_function_calls() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(test_timeout(), async {
            let get_weather = FunctionDeclaration::builder("get_weather")
                .description("Get the current weather for a city")
                .parameter("city", json!({"type": "string"}))
                .required(vec!["city".to_string()])
                .build();

            let stream = stateful_builder(&client)
                .with_text("What's the weather in London?")
                .add_function(get_weather)
                .create_stream();

            let result = consume_stream(stream).await;

            println!(
                "Deltas: {}, Saw function_call delta: {}",
                result.delta_count, result.saw_function_call
            );

            let response = result
                .final_response
                .expect("Should receive a complete response");
            println!("Final status: {:?}", response.status);

            if result.saw_function_call {
                println!("SUCCESS: Function call deltas were properly parsed during streaming");
                return;
            }

            assert!(
                response.has_text() || response.has_function_calls(),
                "Streaming response should have text or function calls"
            );
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_streaming_long_response() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(extended_test_timeout(), async {
            let stream = stateful_builder(&client)
                .with_text("Write a detailed 500-word essay about the history of the Internet.")
                .create_stream();

            let result = consume_stream(stream).await;

            println!("Total deltas received: {}", result.delta_count);
            println!("Total text length: {} chars", result.collected_text.len());

            assert!(
                result.delta_count > 5,
                "Long response should produce many delta chunks"
            );
            assert!(
                result.collected_text.len() > 500,
                "Response should be substantial length"
            );
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_streaming_auto_functions_simple() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let weather_func = GetWeatherTestCallable.declaration();

        let stream = interaction_builder(&client)
            .with_text("What's the weather in Tokyo?")
            .add_function(weather_func)
            .create_stream_with_auto_functions();

        let result = consume_auto_function_stream(stream).await;

        println!("Delta count: {}", result.delta_count);
        println!("Functions executed: {:?}", result.executed_function_names);

        assert!(
            result.final_response.is_some(),
            "Should receive a complete response"
        );

        if result.executing_functions_count > 0 {
            println!("✓ Function execution was streamed");
            assert!(
                result.function_results_count > 0,
                "Should have function results after execution"
            );
        }
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_streaming_auto_functions_no_function_call() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let stream = interaction_builder(&client)
            .with_text("What is 2 + 2?")
            .create_stream_with_auto_functions();

        let result = consume_auto_function_stream(stream).await;

        assert!(
            result.final_response.is_some(),
            "Should receive a complete response"
        );
        assert_eq!(
            result.executing_functions_count, 0,
            "No functions should be executed for simple math"
        );
        assert!(
            result.collected_text.contains('4')
                || result.collected_text.to_lowercase().contains("four"),
            "Response should contain the answer"
        );
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_streaming_auto_functions_multiple_calls() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let weather_func = GetWeatherTestCallable.declaration();
        let time_func = GetTimeTestCallable.declaration();

        let stream = interaction_builder(&client)
            .with_text("What's the weather in London and what time is it there (GMT timezone)?")
            .add_functions(vec![weather_func, time_func])
            .create_stream_with_auto_functions();

        let result = consume_auto_function_stream(stream).await;

        assert!(
            result.final_response.is_some(),
            "Should receive a complete response"
        );
        assert!(result.delta_count > 0, "Should have received delta chunks");
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_streaming_auto_functions_max_loops() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let weather_func = GetWeatherTestCallable.declaration();

        let stream = interaction_builder(&client)
            .with_text("What's the weather in Paris?")
            .add_function(weather_func)
            .with_max_function_call_loops(1)
            .create_stream_with_auto_functions();

        let result = consume_auto_function_stream(stream).await;

        if result.final_response.is_some() {
            println!("✓ Completed within max loop limit");
        }
    }
}

// =============================================================================
// Auto Function Calling Tests
// =============================================================================

mod auto_execution {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_auto_function_calling_max_loops() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let weather_func = GetWeatherTestCallable.declaration();

        let result = interaction_builder(&client)
            .with_text("What's the weather in Tokyo?")
            .add_function(weather_func)
            .with_max_function_call_loops(1)
            .create_with_auto_functions()
            .await;

        match result {
            Ok(auto_result) => {
                println!("Completed within 1 loop: {:?}", auto_result.response.status);
            }
            Err(e) => {
                let error_msg = format!("{:?}", e);
                if error_msg.contains("maximum function call loops") {
                    println!("✓ Max loops limit was respected");
                }
            }
        }
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_auto_function_calling_multi_round_accumulation() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let weather_func = GetWeatherTestCallable.declaration();
        let convert_func = ConvertTemperatureCallable.declaration();

        let result = interaction_builder(&client)
            .with_text(
                "What's the weather in Tokyo? I need the temperature in Fahrenheit, not Celsius. \
                 Use the convert_temperature function to convert the result.",
            )
            .add_functions(vec![weather_func, convert_func])
            .create_with_auto_functions()
            .await
            .expect("Auto function calling failed");

        println!("Function executions ({} total):", result.executions.len());
        for (i, exec) in result.executions.iter().enumerate() {
            println!("  {}: {} -> {}", i + 1, exec.name, exec.result);
        }

        assert!(
            !result.executions.is_empty(),
            "Should have at least one function execution"
        );
        assert!(
            result.response.has_text(),
            "Should have final text response"
        );
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_auto_functions_timeout_returns_error() {
        with_timeout(test_timeout(), async {
            let client = get_client().expect("GEMINI_API_KEY required");

            let result = interaction_builder(&client)
                .with_text("What is 2 + 2?")
                .with_timeout(Duration::from_millis(1))
                .create_with_auto_functions()
                .await;

            assert!(
                matches!(result, Err(GenaiError::Timeout(_))),
                "Expected GenaiError::Timeout, got: {:?}",
                result
            );
            println!("✓ create_with_auto_functions correctly returns timeout error");
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_auto_functions_stream_timeout_returns_error() {
        use futures_util::StreamExt;

        with_timeout(test_timeout(), async {
            let client = get_client().expect("GEMINI_API_KEY required");

            let mut stream = interaction_builder(&client)
                .with_text("What is 2 + 2?")
                .with_timeout(Duration::from_millis(1))
                .create_stream_with_auto_functions();

            let mut got_timeout = false;
            while let Some(result) = stream.next().await {
                match result {
                    Ok(_) => continue,
                    Err(GenaiError::Timeout(_)) => {
                        got_timeout = true;
                        break;
                    }
                    Err(e) => panic!("Expected GenaiError::Timeout, got: {:?}", e),
                }
            }

            assert!(got_timeout, "Stream should have yielded a timeout error");
            println!("✓ create_stream_with_auto_functions correctly returns timeout error");
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_auto_function_result_success_detection() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let functions = vec![get_weather_function()];

        let result = client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_text("What's the weather in Seattle?")
            .add_functions(functions)
            .with_store_enabled()
            .create_with_auto_functions()
            .await
            .expect("Should succeed");

        assert!(
            result.all_executions_succeeded(),
            "All executions should succeed with registered function. Failed: {:?}",
            result.failed_executions()
        );

        assert!(
            result.executions.iter().any(|e| e.name == "get_weather"),
            "Should have executed get_weather"
        );
    }

    /// Test that timeout correctly fires even with registered functions.
    ///
    /// Validates that the timeout mechanism works with `create_with_auto_functions()`
    /// when functions are registered via the `#[tool]` macro.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_auto_functions_timeout_with_registered_functions() {
        use std::time::Duration;

        with_timeout(test_timeout(), async {
            let client = get_client().expect("GEMINI_API_KEY required");

            // Use an impossibly short timeout with functions registered
            // The functions are registered via #[tool] macro (get_weather_test, etc.)
            let result = interaction_builder(&client)
                .with_text("What's the weather in Tokyo and what time is it in JST?")
                .with_timeout(Duration::from_millis(1))
                .create_with_auto_functions()
                .await;

            // Should return a timeout error even with functions registered
            assert!(
                matches!(result, Err(GenaiError::Timeout(_))),
                "Expected GenaiError::Timeout with registered functions, got: {:?}",
                result
            );

            println!("✓ Timeout correctly fires with registered functions");
            println!("  Note: Any function calls that completed before timeout are");
            println!("  preserved on the API side via previous_interaction_id chain");
        })
        .await;
    }
}

// =============================================================================
// Stateless Function Calling Tests
// =============================================================================

mod stateless {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_stateless_function_calling_multi_turn() {
        with_timeout(extended_test_timeout(), async {
            let client = get_client().expect("GEMINI_API_KEY required");

            let functions = vec![
                FunctionDeclaration::builder("get_weather")
                    .description("Get the current weather for a city")
                    .parameter(
                        "city",
                        json!({"type": "string", "description": "City name"}),
                    )
                    .required(vec!["city".to_string()])
                    .build(),
                FunctionDeclaration::builder("get_time")
                    .description("Get the current time in a timezone")
                    .parameter(
                        "timezone",
                        json!({"type": "string", "description": "Timezone"}),
                    )
                    .required(vec!["timezone".to_string()])
                    .build(),
            ];

            let mut history: Vec<genai_rs::Content> =
                vec![Content::text("What's the weather in Tokyo?")];

            let response1 = client
                .interaction()
                .with_model("gemini-3-flash-preview")
                .with_input(InteractionInput::Content(history.clone()))
                .add_functions(functions.clone())
                .with_store_disabled()
                .create()
                .await
                .expect("First turn failed");

            let calls = response1.function_calls();
            assert!(
                !calls.is_empty(),
                "Expected function call for weather query"
            );

            for call in &calls {
                let call_id = call.id.expect("Function call should have ID");
                history.push(Content::function_call(call.name, call.args.clone()));
                let result = json!({"city": "Tokyo", "temperature": "22°C", "conditions": "sunny"});
                history.push(Content::function_result(call.name, call_id, result));
            }

            let response2 = client
                .interaction()
                .with_model("gemini-3-flash-preview")
                .with_input(InteractionInput::Content(history.clone()))
                .add_functions(functions.clone())
                .with_store_disabled()
                .create()
                .await
                .expect("Function result turn failed");

            let text = response2.as_text();
            assert!(
                text.is_some(),
                "Expected text response after function result"
            );
            println!(
                "✓ Stateless function call turn 1 complete: {}",
                text.unwrap()
            );
        })
        .await;
    }

    /// Test stateless mode with thinking enabled for function calling.
    ///
    /// Note: Thought signatures appear on Thought content blocks, not on function calls.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_stateless_with_thinking_function_calling() {
        use genai_rs::FunctionCallingMode;

        let client = get_client().expect("GEMINI_API_KEY required");

        let get_weather = FunctionDeclaration::builder("get_weather")
            .description("Get the current weather for a city")
            .parameter(
                "city",
                json!({"type": "string", "description": "City name"}),
            )
            .required(vec!["city".to_string()])
            .build();

        let history: Vec<genai_rs::Content> = vec![Content::text("What's the weather in Paris?")];

        // Stateless with thinking enabled, force function calling
        let response = client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_input(InteractionInput::Content(history))
            .add_function(get_weather)
            .with_thinking_level(ThinkingLevel::Medium)
            .with_function_calling_mode(FunctionCallingMode::Any)
            .with_store_disabled()
            .create()
            .await
            .expect("Stateless thinking request failed");

        println!("Has thoughts in output: {}", response.has_thoughts());

        let calls = response.function_calls();
        assert!(
            !calls.is_empty(),
            "Model should call function with FunctionCallingMode::Any"
        );

        let call = &calls[0];
        println!(
            "Stateless + thinking function call: {} (id: {:?})",
            call.name, call.id
        );
    }
}

// =============================================================================
// Thinking + Function Calling Tests
// =============================================================================

mod thinking {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_thinking_with_function_calling_multi_turn() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let get_weather = FunctionDeclaration::builder("get_weather")
            .description("Get the current weather for a city including temperature and conditions")
            .parameter(
                "city",
                json!({"type": "string", "description": "The city name"}),
            )
            .required(vec!["city".to_string()])
            .build();

        // Turn 1: Enable thinking + trigger function call
        let response1 = retry_request!([client, get_weather] => {
            stateful_builder(&client)
                .with_text("What's the weather in Tokyo? Should I bring an umbrella?")
                .add_function(get_weather)
                .with_thinking_level(ThinkingLevel::Medium)
                .with_store_enabled()
                .create()
                .await
        })
        .expect("Turn 1 failed");

        println!("Turn 1 status: {:?}", response1.status);

        let function_calls = response1.function_calls();
        if function_calls.is_empty() {
            println!("Model chose not to call function - skipping rest of test");
            return;
        }

        let call = &function_calls[0];
        println!("Turn 1 function call: {} (id: {:?})", call.name, call.id);
        assert!(call.id.is_some(), "Function call must have an id");

        // Turn 2: Provide function result
        let function_result = Content::function_result(
            "get_weather",
            call.id.expect("call_id should exist").to_string(),
            json!({"temperature": "18°C", "conditions": "rainy", "precipitation": "80%", "humidity": "85%"}),
        );

        let prev_id = response1.id.clone().expect("id should exist");
        let response2 = retry_request!([client, prev_id, get_weather, function_result] => {
            stateful_builder(&client)
                .with_previous_interaction(&prev_id)
                .with_content(vec![function_result])
                .add_function(get_weather)
                .with_thinking_level(ThinkingLevel::Medium)
                .with_store_enabled()
                .create()
                .await
        })
        .expect("Turn 2 failed");

        println!("Turn 2 status: {:?}", response2.status);
        println!("Turn 2 has_thoughts: {}", response2.has_thoughts());
        assert!(
            response2.has_text(),
            "Turn 2 should have text response about the weather"
        );

        let text2 = response2.as_text().unwrap();
        let is_valid = validate_response_semantically(
            &client,
            "User asked 'What's the weather in Tokyo? Should I bring an umbrella?' and received weather data showing 18°C, rainy conditions, 80% precipitation",
            text2,
            "Does this response address the weather conditions and whether an umbrella is needed?"
        ).await.expect("Semantic validation failed");
        assert!(
            is_valid,
            "Turn 2 response should meaningfully address the weather question"
        );
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_thinking_with_parallel_function_calls() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let get_weather = FunctionDeclaration::builder("get_weather")
            .description("Get the current weather for a city")
            .parameter(
                "city",
                json!({"type": "string", "description": "City name"}),
            )
            .required(vec!["city".to_string()])
            .build();

        let get_time = FunctionDeclaration::builder("get_time")
            .description("Get the current time in a timezone")
            .parameter(
                "timezone",
                json!({"type": "string", "description": "Timezone like UTC, PST, JST"}),
            )
            .required(vec!["timezone".to_string()])
            .build();

        let response1 = retry_request!([client, get_weather, get_time] => {
            stateful_builder(&client)
                .with_text("What's the weather in Tokyo and what time is it there? I need both pieces of information.")
                .add_functions(vec![get_weather, get_time])
                .with_thinking_level(ThinkingLevel::Medium)
                .with_store_enabled()
                .create()
                .await
        })
        .expect("Turn 1 failed");

        let function_calls = response1.function_calls();
        println!("Number of function calls: {}", function_calls.len());

        if function_calls.is_empty() {
            println!("Model chose not to call functions - skipping rest of test");
            return;
        }

        if function_calls.len() >= 2 {
            println!("✓ Model made parallel function calls");
        }

        // Provide results for all function calls
        let mut results = Vec::new();
        for call in &function_calls {
            let result_data = match call.name {
                "get_weather" => {
                    json!({"temperature": "22°C", "conditions": "partly cloudy", "humidity": "65%"})
                }
                "get_time" => json!({"time": "14:30", "timezone": "JST", "date": "2025-01-15"}),
                _ => json!({"status": "unknown function"}),
            };
            results.push(Content::function_result(
                call.name,
                call.id.expect("call should have ID"),
                result_data,
            ));
        }

        let prev_id = response1.id.clone().expect("id should exist");
        let response2 = retry_request!([client, prev_id, get_weather, get_time, results] => {
            stateful_builder(&client)
                .with_previous_interaction(&prev_id)
                .with_content(results)
                .add_functions(vec![get_weather, get_time])
                .with_thinking_level(ThinkingLevel::Medium)
                .with_store_enabled()
                .create()
                .await
        })
        .expect("Turn 2 failed");

        assert!(
            response2.has_text(),
            "Turn 2 should have text response combining weather and time info"
        );
        println!("✓ Parallel function calls with thinking completed successfully");
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_thinking_levels_with_function_calling() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let get_weather = FunctionDeclaration::builder("get_weather")
            .description("Get the current weather for a city")
            .parameter(
                "city",
                json!({"type": "string", "description": "City name"}),
            )
            .required(vec!["city".to_string()])
            .build();

        let levels = [
            (ThinkingLevel::Low, "Low"),
            (ThinkingLevel::Medium, "Medium"),
            (ThinkingLevel::High, "High"),
        ];

        for (level, level_name) in levels {
            println!("\n=== Testing ThinkingLevel::{} ===", level_name);

            let get_weather_fn = get_weather.clone();
            let level_clone = level.clone();
            let response1 = retry_request!([client, get_weather_fn, level_clone] => {
                stateful_builder(&client)
                    .with_text("What's the weather in Paris?")
                    .add_function(get_weather_fn)
                    .with_thinking_level(level_clone)
                    .with_store_enabled()
                    .create()
                    .await
            })
            .unwrap_or_else(|e| panic!("Turn 1 failed for ThinkingLevel::{}: {}", level_name, e));

            let function_calls = response1.function_calls();
            if function_calls.is_empty() {
                println!("  Model chose not to call function - skipping this level");
                continue;
            }

            let call = &function_calls[0];
            let function_result = Content::function_result(
                "get_weather",
                call.id.expect("call should have ID"),
                json!({"temperature": "15°C", "conditions": "sunny"}),
            );

            let prev_id = response1.id.clone().expect("id should exist");
            let get_weather_fn = get_weather.clone();
            let fn_result = function_result.clone();
            let level_clone = level.clone();
            let response2 =
                retry_request!([client, prev_id, get_weather_fn, fn_result, level_clone] => {
                    stateful_builder(&client)
                        .with_previous_interaction(&prev_id)
                        .with_content(vec![fn_result])
                        .add_function(get_weather_fn)
                        .with_thinking_level(level_clone)
                        .create()
                        .await
                })
                .unwrap_or_else(|e| {
                    panic!("Turn 2 failed for ThinkingLevel::{}: {}", level_name, e)
                });

            assert!(
                response2.has_text(),
                "ThinkingLevel::{} should produce text response",
                level_name
            );
            println!("  ✓ ThinkingLevel::{} completed successfully", level_name);
        }

        println!("\n✓ All ThinkingLevel variants work with function calling");
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_function_calling_without_thinking() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let get_weather = FunctionDeclaration::builder("get_weather")
            .description("Get the current weather for a city")
            .parameter(
                "city",
                json!({"type": "string", "description": "The city name"}),
            )
            .required(vec!["city".to_string()])
            .build();

        // Turn 1 WITHOUT thinking mode
        let get_weather_fn = get_weather.clone();
        let response1 = retry_request!([client, get_weather_fn] => {
            stateful_builder(&client)
                .with_text("What's the weather in Tokyo?")
                .add_function(get_weather_fn)
                .with_store_enabled()
                .create()
                .await
        })
        .expect("Turn 1 failed");

        println!("Turn 1 has_thoughts: {}", response1.has_thoughts());

        let function_calls = response1.function_calls();
        if function_calls.is_empty() {
            println!("Model chose not to call function - skipping rest of test");
            return;
        }

        let call = &function_calls[0];
        println!("Function call: {} (id: {:?})", call.name, call.id);

        // Turn 2: Provide result
        let function_result = Content::function_result(
            "get_weather",
            call.id.expect("call_id should exist"),
            json!({"temperature": "22°C", "conditions": "clear", "humidity": "45%"}),
        );

        let prev_id = response1.id.clone().expect("id should exist");
        let get_weather_fn = get_weather.clone();
        let fn_result = function_result.clone();
        let response2 = retry_request!([client, prev_id, get_weather_fn, fn_result] => {
            stateful_builder(&client)
                .with_previous_interaction(&prev_id)
                .with_content(vec![fn_result])
                .add_function(get_weather_fn)
                .create()
                .await
        })
        .expect("Turn 2 failed");

        assert!(
            response2.has_text(),
            "Turn 2 should have text response about the weather"
        );
        println!("✓ Function calling without thinking completed successfully");
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_streaming_with_thinking_and_function_calling() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let get_weather = FunctionDeclaration::builder("get_weather")
            .description("Get the current weather for a city")
            .parameter(
                "city",
                json!({"type": "string", "description": "The city name"}),
            )
            .required(vec!["city".to_string()])
            .build();

        // Turn 1: Stream with thinking + function call
        let stream = stateful_builder(&client)
            .with_text("What's the weather in Tokyo? I need to know if I should bring an umbrella.")
            .add_function(get_weather.clone())
            .with_thinking_level(ThinkingLevel::Medium)
            .with_store_enabled()
            .create_stream();

        let result = consume_stream(stream).await;

        println!("Total deltas: {}", result.delta_count);
        println!("Saw thought deltas: {}", result.saw_thought);
        println!("Saw function call: {}", result.saw_function_call);

        assert!(result.has_output(), "Should receive streaming chunks");

        let response1 = result
            .final_response
            .expect("Should receive complete response");

        let function_calls = response1.function_calls();
        if function_calls.is_empty() {
            if result.saw_function_call {
                println!("✓ Function call deltas detected in stream");
                return;
            }
            println!("Model chose not to call function - skipping rest of test");
            return;
        }

        // Turn 2: Stream the follow-up
        let call = &function_calls[0];
        let function_result = Content::function_result(
            "get_weather",
            call.id.expect("call should have ID"),
            json!({"temperature": "18°C", "conditions": "rainy", "precipitation": "85%", "humidity": "90%"}),
        );

        let stream2 = stateful_builder(&client)
            .with_previous_interaction(response1.id.as_ref().expect("id should exist"))
            .with_content(vec![function_result])
            .add_function(get_weather)
            .with_thinking_level(ThinkingLevel::Medium)
            .with_store_enabled()
            .create_stream();

        let result2 = consume_stream(stream2).await;

        println!("Turn 2 deltas: {}", result2.delta_count);
        assert!(result2.has_output(), "Should receive streaming chunks");
        assert!(
            !result2.collected_text.is_empty(),
            "Turn 2 should stream text content"
        );

        println!("✓ Streaming with thinking + function calling completed successfully");
    }

    /// Test streaming with thinking mode but no function calling.
    ///
    /// Validates that streaming with thinking enabled works independently
    /// of function calling (pure thinking + streaming).
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_streaming_with_thinking_only() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        println!("=== Streaming with thinking (no function calling) ===");

        let stream = stateful_builder(&client)
            .with_text("Explain briefly why the sky is blue.")
            .with_thinking_level(ThinkingLevel::Medium)
            .create_stream();

        let result = consume_stream(stream).await;

        println!("\n--- Streaming Results ---");
        println!("Total deltas: {}", result.delta_count);
        println!("Saw thought deltas: {}", result.saw_thought);
        println!("Saw thought signature: {}", result.saw_thought_signature);
        println!("Collected text length: {}", result.collected_text.len());

        // Verify streaming worked
        assert!(result.has_output(), "Should receive streaming chunks");

        // Log thinking observations
        if result.saw_thought {
            println!("✓ Thought deltas received during streaming");
            if !result.collected_thoughts.is_empty() {
                println!(
                    "  Thoughts preview: {}...",
                    &result.collected_thoughts[..result.collected_thoughts.len().min(100)]
                );
            }
        } else {
            println!("ℹ Thoughts processed internally");
        }

        // Verify we got text
        assert!(
            !result.collected_text.is_empty(),
            "Should stream text content"
        );

        // Verify content is about the sky/light/scattering - use semantic validation
        let is_valid = validate_response_semantically(
            &client,
            "Asked 'Why is the sky blue?' with thinking mode enabled",
            &result.collected_text,
            "Does this response explain the scientific reason for the sky appearing blue (light, scattering, wavelengths)?",
        )
        .await
        .expect("Semantic validation failed");
        assert!(
            is_valid,
            "Response should explain why sky is blue. Got: {}",
            result.collected_text
        );

        println!("\n✓ Streaming with thinking (no function calling) completed successfully");
    }

    /// Test that sequential function calls each have their own thought signature.
    ///
    /// Per docs: "For sequential function calls, each function call will have its
    /// own signature that must be returned."
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_sequential_function_calls_with_thinking() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(test_timeout(), async {
            let get_weather = FunctionDeclaration::builder("get_weather")
                .description("Get the current weather")
                .parameter("city", json!({"type": "string"}))
                .required(vec!["city".to_string()])
                .build();

            // Step 1
            let response1 = stateful_builder(&client)
                .with_text("What's the weather in Tokyo?")
                .add_function(get_weather.clone())
                .with_thinking_level(ThinkingLevel::Low)
                .create()
                .await
                .expect("First interaction failed");

            let calls1 = response1.function_calls();
            if calls1.is_empty() {
                println!("No function call in step 1 - skipping");
                return;
            }

            let call1 = &calls1[0];
            println!("Step 1 function call: {} (id: {:?})", call1.name, call1.id);

            // Provide result
            let result1 = Content::function_result(
                "get_weather",
                call1.id.unwrap().to_string(),
                json!({"temperature": "22°C"}),
            );

            // Step 2
            let response2 = stateful_builder(&client)
                .with_previous_interaction(response1.id.as_ref().expect("id should exist"))
                .with_content(vec![result1])
                .with_text("Now what about Paris?")
                .add_function(get_weather.clone())
                .with_thinking_level(ThinkingLevel::Low)
                .create()
                .await
                .expect("Second interaction failed");

            let calls2 = response2.function_calls();
            if !calls2.is_empty() {
                let call2 = &calls2[0];
                println!("Step 2 function call: {} (id: {:?})", call2.name, call2.id);
                println!("✓ Sequential function calls with thinking completed");
            }
        })
        .await;
    }

    /// Test thinking with sequential and parallel function call chains.
    ///
    /// This comprehensive test validates complex multi-step function calling
    /// with thinking mode enabled, including parallel calls and sequential chains.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_thinking_with_sequential_parallel_function_chain() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Define all functions we'll use
        let get_weather = FunctionDeclaration::builder("get_current_weather")
            .description("Get the current weather conditions for a city")
            .parameter(
                "city",
                json!({"type": "string", "description": "City name"}),
            )
            .required(vec!["city".to_string()])
            .build();

        let get_time = FunctionDeclaration::builder("get_local_time")
            .description("Get the current local time in a city")
            .parameter(
                "city",
                json!({"type": "string", "description": "City name"}),
            )
            .required(vec!["city".to_string()])
            .build();

        let get_forecast = FunctionDeclaration::builder("get_weather_forecast")
            .description("Get the weather forecast for the next few days")
            .parameter(
                "city",
                json!({"type": "string", "description": "City name"}),
            )
            .required(vec!["city".to_string()])
            .build();

        let get_activities = FunctionDeclaration::builder("get_recommended_activities")
            .description("Get recommended activities based on weather conditions")
            .parameter(
                "weather_condition",
                json!({"type": "string", "description": "Current weather like sunny, rainy, cloudy"}),
            )
            .required(vec!["weather_condition".to_string()])
            .build();

        let all_functions = vec![
            get_weather.clone(),
            get_time.clone(),
            get_forecast.clone(),
            get_activities.clone(),
        ];

        // =========================================================================
        // Step 1: Initial request - expect parallel calls for weather and time
        // =========================================================================
        println!("=== Step 1: Initial request ===");

        let functions = all_functions.clone();
        let response1 = retry_request!([client, functions] => {
            stateful_builder(&client)
                .with_text(
                    "I'm planning a trip to Tokyo. I need to know the current weather, \
                     current local time, the forecast for the next few days, and what \
                     activities you'd recommend. Please gather all this information.",
                )
                .add_functions(functions)
                .with_thinking_level(ThinkingLevel::Medium)
                .with_store_enabled()
                .create()
                .await
        })
        .expect("Step 1 failed");

        println!("Step 1 status: {:?}", response1.status);

        let calls1 = response1.function_calls();
        println!("Step 1 function calls: {}", calls1.len());

        if calls1.is_empty() {
            println!("Model chose not to call functions - skipping rest of test");
            return;
        }

        for (i, call) in calls1.iter().enumerate() {
            println!("  Call {}: {} (id: {:?})", i + 1, call.name, call.id);
        }

        // Verify all calls have IDs
        for call in &calls1 {
            assert!(call.id.is_some(), "Function call should have ID");
        }

        // =========================================================================
        // Step 2: Provide results for step 1, expect more function calls
        // =========================================================================
        println!("\n=== Step 2: Provide first results ===");

        let mut results1 = Vec::new();
        for call in &calls1 {
            let result_data = match call.name {
                "get_current_weather" => json!({
                    "temperature": "24°C",
                    "conditions": "partly cloudy",
                    "humidity": "60%",
                    "wind": "10 km/h"
                }),
                "get_local_time" => json!({
                    "time": "10:30 AM",
                    "timezone": "JST",
                    "date": "2025-01-15"
                }),
                "get_weather_forecast" => json!({
                    "tomorrow": "sunny, 26°C",
                    "day_after": "cloudy, 22°C",
                    "in_3_days": "rainy, 18°C"
                }),
                "get_recommended_activities" => json!({
                    "outdoor": ["visit Senso-ji Temple", "walk in Ueno Park"],
                    "indoor": ["explore TeamLab", "shop in Shibuya"],
                    "food": ["try ramen in Shinjuku", "sushi at Tsukiji"]
                }),
                _ => json!({"status": "unknown function"}),
            };

            results1.push(Content::function_result(
                call.name,
                call.id.expect("call should have ID"),
                result_data,
            ));
        }

        let prev_id = response1.id.clone().expect("id should exist");
        let functions = all_functions.clone();
        let results = results1.clone();
        let response2 = retry_request!([client, prev_id, functions, results] => {
            stateful_builder(&client)
                .with_previous_interaction(&prev_id)
                .with_content(results)
                .add_functions(functions)
                .with_thinking_level(ThinkingLevel::Medium)
                .with_store_enabled()
                .create()
                .await
        })
        .expect("Step 2 failed");

        println!("Step 2 status: {:?}", response2.status);
        println!("Step 2 has_thoughts: {}", response2.has_thoughts());
        println!("Step 2 has_text: {}", response2.has_text());

        let calls2 = response2.function_calls();
        println!("Step 2 function calls: {}", calls2.len());

        for (i, call) in calls2.iter().enumerate() {
            println!("  Call {}: {} (id: {:?})", i + 1, call.name, call.id);
        }

        // The model might either:
        // 1. Call more functions (sequential chain continues)
        // 2. Return final text (it has enough info)
        //
        // Both are valid outcomes - we test the chain if it continues

        if !calls2.is_empty() {
            // =====================================================================
            // Step 3: Provide second round of results, expect final response
            // =====================================================================
            println!("\n=== Step 3: Provide second results ===");

            let mut results2 = Vec::new();
            for call in &calls2 {
                let result_data = match call.name {
                    "get_current_weather" => json!({
                        "temperature": "24°C",
                        "conditions": "partly cloudy"
                    }),
                    "get_local_time" => json!({
                        "time": "10:35 AM",
                        "timezone": "JST"
                    }),
                    "get_weather_forecast" => json!({
                        "tomorrow": "sunny, 26°C",
                        "day_after": "cloudy, 22°C"
                    }),
                    "get_recommended_activities" => json!({
                        "outdoor": ["temple visits", "park walks"],
                        "indoor": ["museums", "shopping"]
                    }),
                    _ => json!({"status": "ok"}),
                };

                results2.push(Content::function_result(
                    call.name,
                    call.id.expect("call should have ID"),
                    result_data,
                ));
            }

            let prev_id = response2.id.clone().expect("id should exist");
            let functions = all_functions.clone();
            let results = results2.clone();
            let response3 = retry_request!([client, prev_id, functions, results] => {
                stateful_builder(&client)
                    .with_previous_interaction(&prev_id)
                    .with_content(results)
                    .add_functions(functions)
                    .with_thinking_level(ThinkingLevel::Medium)
                    .with_store_enabled()
                    .create()
                    .await
            })
            .expect("Step 3 failed");

            println!("Step 3 status: {:?}", response3.status);
            println!("Step 3 has_thoughts: {}", response3.has_thoughts());
            println!("Step 3 has_text: {}", response3.has_text());

            if response3.has_thoughts() {
                println!("✓ Thoughts visible in Step 3");
            } else {
                println!("ℹ Thoughts processed internally");
            }

            let calls3 = response3.function_calls();
            if calls3.is_empty() {
                println!("✓ No more function calls - chain complete");
            } else {
                println!("ℹ Model requested {} more function calls", calls3.len());
            }

            if response3.has_text() {
                let text = response3.as_text().unwrap();
                println!("Step 3 text preview: {}...", &text[..text.len().min(200)]);

                // Use semantic validation instead of brittle keyword matching
                let is_valid = validate_response_semantically(
                    &client,
                    "User asked about planning a trip to Tokyo. The assistant gathered information about current weather, local time, forecast, and recommended activities via function calls.",
                    text,
                    "Does this response provide helpful trip planning information about Tokyo (weather, timing, activities)?",
                )
                .await
                .expect("Semantic validation failed");

                assert!(
                    is_valid,
                    "Final response should reference gathered information"
                );
            }

            println!("\n✓ Sequential parallel function chain (3 steps) completed successfully");
        } else {
            // Model returned text in step 2 (gathered all info in first round)
            println!("ℹ Model completed in 2 steps (no sequential chain needed)");

            if response2.has_text() {
                let text = response2.as_text().unwrap();
                println!("Step 2 text preview: {}...", &text[..text.len().min(200)]);
            }

            assert!(
                response2.has_text(),
                "Step 2 should have text if no more function calls"
            );

            println!("\n✓ Function calls with thinking completed in 2 steps");
        }
    }
}

// =============================================================================
// Multi-turn Function Calling Tests
// =============================================================================

mod multiturn {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_multiturn_auto_functions_happy_path() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let functions = vec![get_weather_function()];

        // Turn 1: Initial request with system instruction, trigger function call
        println!("--- Turn 1: Initial request with system instruction ---");
        let result1 = client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_text("What's the weather in Seattle?")
            .add_functions(functions.clone())
            .with_store_enabled()
            .with_system_instruction(SYSTEM_INSTRUCTION)
            .create_with_auto_functions()
            .await
            .expect("Turn 1 should succeed");

        println!("Turn 1 status: {:?}", result1.response.status);
        println!("Turn 1 function executions: {}", result1.executions.len());

        assert!(
            result1.executions.iter().any(|e| e.name == "get_weather"),
            "Should have executed get_weather function"
        );
        assert!(
            result1.all_executions_succeeded(),
            "All function executions should succeed. Failed: {:?}",
            result1.failed_executions()
        );

        let response1 = result1.response;
        let turn1_id = response1.id.clone().expect("Turn 1 should have ID");

        // Turn 2: Follow-up (must resend tools)
        println!("\n--- Turn 2: Follow-up conversation ---");
        let result2 = client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_text("How about in Tokyo?")
            .add_functions(functions.clone())
            .with_store_enabled()
            .with_previous_interaction(&turn1_id)
            .create_with_auto_functions()
            .await
            .expect("Turn 2 should succeed");

        assert!(
            result2.executions.iter().any(|e| e.name == "get_weather"),
            "Should have executed get_weather function for Tokyo"
        );

        let response2 = result2.response;
        let turn2_id = response2.id.clone().expect("Turn 2 should have ID");

        // Turn 3: Follow-up question
        println!("\n--- Turn 3: Follow-up without function call ---");
        let result3 = client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_text("Based on the weather you just told me about Seattle and Tokyo, which city is warmer right now?")
            .add_functions(functions)
            .with_store_enabled()
            .with_previous_interaction(&turn2_id)
            .create_with_auto_functions()
            .await
            .expect("Turn 3 should succeed");

        let response3 = result3.response;
        assert!(response3.has_text(), "Turn 3 should have text response");

        let text = response3.as_text().unwrap();
        println!("Turn 3 response: {}", text);

        let is_valid = validate_response_semantically(
            &client,
            "Previous turns retrieved weather: Seattle (65°F) and Tokyo (72°F). User asked which city is warmer.",
            text,
            "Does this response correctly identify which city is warmer based on temperature comparison?"
        ).await.expect("Semantic validation failed");
        assert!(is_valid, "Response should correctly compare temperatures");
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_multiturn_tools_not_inherited() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let functions = vec![get_weather_function()];

        // Turn 1: Initial request with functions
        let result1 = retry_request!([client, functions] => {
            stateful_builder(&client)
                .with_text("Remember: I'm interested in weather data.")
                .add_functions(functions)
                .create()
                .await
        })
        .expect("Turn 1 should succeed");

        let turn1_id = result1.id.clone().expect("Turn 1 should have ID");

        // Turn 2: Follow-up WITHOUT resending tools
        let result2 = retry_request!([client, turn1_id] => {
            stateful_builder(&client)
                .with_text("What's the weather in Paris?")
                .with_previous_interaction(&turn1_id)
                .create()
                .await
        })
        .expect("Turn 2 should succeed");

        // Model should NOT have any function calls since we didn't provide tools
        let function_calls = result2.function_calls();
        assert!(
            function_calls.is_empty(),
            "Model should not make function calls when tools not provided (got {} calls)",
            function_calls.len()
        );
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_multiturn_manual_functions_happy_path() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let functions = vec![get_weather_function(), get_time_function()];

        // Turn 1: Initial request with system instruction
        let response1 = client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_text("What's the weather in London and what time is it there?")
            .add_functions(functions.clone())
            .with_store_enabled()
            .with_system_instruction(SYSTEM_INSTRUCTION)
            .create()
            .await
            .expect("Turn 1 should succeed");

        let function_calls = response1.function_calls();
        assert!(
            !function_calls.is_empty(),
            "Turn 1 should trigger function calls"
        );

        // Execute functions manually
        let mut results = Vec::new();
        for call in &function_calls {
            let call_id = call.id.expect("Function call should have ID");
            let result = match call.name {
                "get_weather" => {
                    json!({"city": "London", "temperature": "15°C", "conditions": "cloudy"})
                }
                "get_time" => json!({"timezone": "GMT", "time": "14:30:00"}),
                _ => json!({"error": "Unknown function"}),
            };
            results.push(Content::function_result(
                call.name.to_string(),
                call_id.to_string(),
                result,
            ));
        }

        // Send function results back
        let response2 = client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_previous_interaction(response1.id.as_ref().expect("Should have ID"))
            .with_content(results)
            .add_functions(functions.clone())
            .with_store_enabled()
            .create()
            .await
            .expect("Function result submission should succeed");

        assert!(
            response2.has_text(),
            "Should have text response after function results"
        );
        println!("Response: {}", response2.as_text().unwrap());

        // Turn 3: Follow-up
        let response3 = client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_text("Is it a good time to call someone there?")
            .add_functions(functions)
            .with_store_enabled()
            .with_previous_interaction(response2.id.as_ref().expect("Should have ID"))
            .create()
            .await
            .expect("Turn 3 should succeed");

        if response3.has_text() {
            let text = response3.as_text().unwrap();
            println!("Turn 3 response: {}", text);
        }
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_system_instruction_inheritance() {
        use std::time::Duration;

        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        type BoxError = Box<dyn std::error::Error + Send + Sync>;
        let result: Result<(), BoxError> = retry_on_any_error(2, Duration::from_secs(3), || {
            let client = client.clone();
            async move {
                // Turn 1: Set system instruction to always respond in haiku format
                let response1 = client
                    .interaction()
                    .with_model("gemini-3-flash-preview")
                    .with_text("Hello!")
                    .with_store_enabled()
                    .with_system_instruction("You are a haiku poet. Always respond in haiku format (5-7-5 syllables). Never break from this format.")
                    .create()
                    .await?;

                let turn1_id = response1.id.clone().ok_or("Turn 1 should have ID")?;

                // Turn 2: Follow-up - system instruction should still be in effect
                let response2 = client
                    .interaction()
                    .with_model("gemini-3-flash-preview")
                    .with_text("Tell me about the ocean.")
                    .with_store_enabled()
                    .with_previous_interaction(&turn1_id)
                    .create()
                    .await?;

                let text = response2.as_text().ok_or("Turn 2 should have text")?;

                let is_valid = validate_response_semantically(
                    &client,
                    "The model was given a system instruction to 'always respond in haiku format'. User asked 'Tell me about the ocean.' in a follow-up turn.",
                    text,
                    "Does this response contain a haiku or show evidence of trying to follow a haiku/poetry constraint?"
                ).await?;

                if is_valid {
                    Ok(())
                } else {
                    Err("Response did not show evidence of inherited haiku system instruction".into())
                }
            }
        })
        .await;

        match result {
            Ok(()) => println!("✓ System instruction inheritance test passed"),
            Err(e) => panic!(
                "System instruction inheritance test failed after retries: {}",
                e
            ),
        }
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_multiturn_streaming_auto_functions() {
        use futures_util::StreamExt;
        use genai_rs::AutoFunctionStreamChunk;

        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let functions = vec![get_weather_function()];

        // Turn 1: Initial request streaming with auto functions
        let mut stream = client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_text("What's the weather in Miami?")
            .add_functions(functions.clone())
            .with_store_enabled()
            .with_system_instruction(SYSTEM_INSTRUCTION)
            .create_stream_with_auto_functions();

        let mut final_response = None;
        let mut function_count = 0;
        let mut delta_count = 0;

        while let Some(result) = stream.next().await {
            match result {
                Ok(event) => match event.chunk {
                    AutoFunctionStreamChunk::Delta(_) => delta_count += 1,
                    AutoFunctionStreamChunk::FunctionResults(execs) => {
                        function_count += execs.len()
                    }
                    AutoFunctionStreamChunk::Complete(response) => final_response = Some(response),
                    _ => {}
                },
                Err(e) => panic!("Stream error: {:?}", e),
            }
        }

        println!(
            "Turn 1: {} deltas, {} functions",
            delta_count, function_count
        );

        let response1 = final_response.expect("Should have final response");
        assert!(
            function_count > 0,
            "Should have executed at least one function"
        );

        // Turn 2: Follow-up streaming
        let mut stream2 = client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_text("Compare that to New York.")
            .add_functions(functions)
            .with_store_enabled()
            .with_previous_interaction(response1.id.as_ref().expect("Should have ID"))
            .create_stream_with_auto_functions();

        let mut turn2_functions = 0;

        while let Some(result) = stream2.next().await {
            if let Ok(event) = result
                && let AutoFunctionStreamChunk::FunctionResults(execs) = event.chunk
            {
                turn2_functions += execs.len();
            }
        }

        assert!(
            turn2_functions > 0,
            "Should have executed function for comparison"
        );
    }

    #[test]
    fn test_storage_constraints_documented() {
        // The builder enforces storage-related constraints at runtime via build():
        //
        // - with_store_disabled() + with_previous_interaction() = error (chaining needs storage)
        // - with_store_disabled() + with_background(true) = error (background needs storage)
        // - with_store_disabled() + create_with_auto_functions() = error (auto-functions need storage)
        //
        // See src/request_builder/tests.rs for validation tests
        println!("Storage constraints are enforced at runtime in build().");
    }

    /// Test multi-turn function calling with error handling.
    ///
    /// Validates that the model correctly explains function errors to users
    /// when a function returns an error result.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_multiturn_manual_functions_error_handling() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let secret_function = FunctionDeclaration::builder("get_secret_data")
            .description("Get secret data that requires special permissions")
            .parameter(
                "key",
                json!({"type": "string", "description": "The secret key"}),
            )
            .required(vec!["key".to_string()])
            .build();

        // Turn 1: Trigger function that will "fail"
        println!("--- Turn 1: Trigger function that will fail ---");
        let response1 = client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_text("Get the secret data for key 'test123'")
            .add_function(secret_function.clone())
            .with_store_enabled()
            .with_system_instruction(
                "You help users access data. When functions fail, explain the error to the user.",
            )
            .create()
            .await
            .expect("Turn 1 should succeed");

        let function_calls = response1.function_calls();
        assert!(!function_calls.is_empty(), "Should trigger function call");

        // Return error result
        let call = &function_calls[0];
        let error_result = Content::function_result(
            "get_secret_data".to_string(),
            call.id.expect("Should have ID").to_string(),
            json!({"error": "Permission denied: insufficient privileges for key 'test123'"}),
        );

        println!("\n--- Sending error result ---");
        let response2 = client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_previous_interaction(response1.id.as_ref().expect("Should have ID"))
            .with_content(vec![error_result])
            .add_function(secret_function)
            .with_store_enabled()
            .create()
            .await
            .expect("Error result submission should succeed");

        println!("After error status: {:?}", response2.status);
        assert!(
            response2.has_text(),
            "Should have text response explaining error"
        );

        let text = response2.as_text().unwrap();
        println!("Response: {}", text);

        // Model should explain the error to user - use semantic validation to avoid brittle keyword matching
        let is_valid = validate_response_semantically(
            &client,
            "A function call to get_secret_data returned an error: 'Permission denied: insufficient privileges for key test123'. The system instruction says to explain errors to the user.",
            text,
            "Does this response appropriately explain to the user that accessing the secret data failed due to a permissions/privileges issue?",
        )
        .await
        .expect("Semantic validation should succeed");
        assert!(is_valid, "Response should explain the permission error");
    }

    /// Test ThinkingLevel::High with function calling.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_thinking_level_high_function_calling() {
        use genai_rs::FunctionCallingMode;

        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let get_weather = FunctionDeclaration::builder("get_weather")
            .description("Get the current weather for a city")
            .parameter(
                "city",
                json!({"type": "string", "description": "City name"}),
            )
            .required(vec!["city".to_string()])
            .build();

        // Stateful with HIGH thinking level, force function calling
        let response = stateful_builder(&client)
            .with_text("What's the weather in Berlin?")
            .add_function(get_weather)
            .with_thinking_level(ThinkingLevel::High)
            .with_function_calling_mode(FunctionCallingMode::Any)
            .create()
            .await
            .expect("High thinking request failed");

        println!("Has thoughts in output: {}", response.has_thoughts());

        let calls = response.function_calls();
        assert!(
            !calls.is_empty(),
            "Model should call function with FunctionCallingMode::Any"
        );

        let call = &calls[0];
        println!(
            "ThinkingLevel::High function call: {} (id: {:?})",
            call.name, call.id
        );
    }

    /// Test FunctionCallingMode::Any forces function calling.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_function_calling_mode_any() {
        use genai_rs::FunctionCallingMode;

        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let get_weather = FunctionDeclaration::builder("get_weather")
            .description("Get the current weather for a city")
            .parameter(
                "city",
                json!({"type": "string", "description": "City name"}),
            )
            .required(vec!["city".to_string()])
            .build();

        // FC mode ANY forces function calling
        let response = stateful_builder(&client)
            .with_text("What's the weather in Tokyo?")
            .add_function(get_weather)
            .with_thinking_level(ThinkingLevel::Medium)
            .with_function_calling_mode(FunctionCallingMode::Any)
            .create()
            .await
            .expect("FC mode Any request failed");

        println!("Has thoughts in output: {}", response.has_thoughts());

        let calls = response.function_calls();
        assert!(
            !calls.is_empty(),
            "Model should call function with FunctionCallingMode::Any"
        );

        let call = &calls[0];
        println!(
            "FunctionCallingMode::Any function call: {} (id: {:?})",
            call.name, call.id
        );
    }
}

// =============================================================================
// Built-in Tools Multi-turn Tests
// =============================================================================

mod builtins_multiturn {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_google_search_multi_turn() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        println!("=== Google Search + Multi-turn ===");

        // Turn 1: Ask about current weather (requires real-time data)
        let result1 = stateful_builder(&client)
            .with_text("What is the current weather in Tokyo, Japan today? Use search to find current data.")
            .with_google_search()
            .with_store_enabled()
            .create()
            .await;

        let response1 = match result1 {
            Ok(response) => {
                println!("Turn 1 status: {:?}", response.status);
                if let Some(text) = response.as_text() {
                    println!("Turn 1 response: {}", text);
                }
                response
            }
            Err(e) => {
                let error_str = format!("{:?}", e);
                if error_str.contains("not supported") || error_str.contains("not available") {
                    println!("Google Search tool not available - skipping test");
                    return;
                }
                panic!("Turn 1 failed unexpectedly: {:?}", e);
            }
        };

        assert_eq!(
            response1.status,
            InteractionStatus::Completed,
            "Turn 1 should complete successfully"
        );

        // Turn 2: Ask follow-up referencing the search results
        let prev_id = response1.id.clone().expect("id should exist");
        let result2 = retry_request!([client, prev_id] => {
            stateful_builder(&client)
                .with_previous_interaction(&prev_id)
                .with_text("Based on the weather information you just found, should I bring an umbrella if I visit Tokyo today?")
                .with_store_enabled()
                .create()
                .await
        });

        match result2 {
            Ok(response2) => {
                println!("Turn 2 status: {:?}", response2.status);
                if let Some(text) = response2.as_text() {
                    println!("Turn 2 response: {}", text);
                    assert!(
                        !text.is_empty(),
                        "Turn 2 should have non-empty text response"
                    );
                }
                assert_eq!(
                    response2.status,
                    InteractionStatus::Completed,
                    "Turn 2 should complete successfully"
                );
            }
            Err(e) => {
                if is_long_conversation_api_error(&e) {
                    println!("API limitation encountered: {:?}", e);
                    return;
                }
                panic!("Turn 2 failed unexpectedly: {:?}", e);
            }
        }

        println!("✓ Google Search + multi-turn completed successfully");
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_url_context_multi_turn() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        println!("=== URL Context + Multi-turn ===");

        // Turn 1: Fetch example.com content
        let result1 = stateful_builder(&client)
            .with_text(
                "Fetch and summarize the main content from https://example.com using URL context.",
            )
            .with_url_context()
            .with_store_enabled()
            .create()
            .await;

        let response1 = match result1 {
            Ok(response) => {
                println!("Turn 1 status: {:?}", response.status);
                if let Some(text) = response.as_text() {
                    println!("Turn 1 response: {}", text);
                }
                response
            }
            Err(e) => {
                let error_str = format!("{:?}", e);
                if error_str.contains("not supported") || error_str.contains("not available") {
                    println!("URL Context tool not available - skipping test");
                    return;
                }
                panic!("Turn 1 failed unexpectedly: {:?}", e);
            }
        };

        assert_eq!(
            response1.status,
            InteractionStatus::Completed,
            "Turn 1 should complete successfully"
        );

        // Turn 2: Ask follow-up about the fetched content
        let prev_id = response1.id.clone().expect("id should exist");
        let result2 = retry_request!([client, prev_id] => {
            stateful_builder(&client)
                .with_previous_interaction(&prev_id)
                .with_text("What is the main purpose of that website you just fetched? Is it a real company or an example domain?")
                .create()
                .await
        });

        match result2 {
            Ok(response2) => {
                println!("Turn 2 status: {:?}", response2.status);
                if let Some(text) = response2.as_text() {
                    println!("Turn 2 response: {}", text);
                    assert!(
                        !text.is_empty(),
                        "Turn 2 should have non-empty text response"
                    );
                }
                assert_eq!(
                    response2.status,
                    InteractionStatus::Completed,
                    "Turn 2 should complete successfully"
                );
            }
            Err(e) => {
                if is_long_conversation_api_error(&e) {
                    println!("API limitation encountered: {:?}", e);
                    return;
                }
                panic!("Turn 2 failed unexpectedly: {:?}", e);
            }
        }

        println!("✓ URL Context + multi-turn completed successfully");
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_code_execution_multi_turn() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        println!("=== Code Execution + Multi-turn ===");

        // Turn 1: Calculate factorial of 5
        let result1 = retry_request!([client] => {
            stateful_builder(&client)
                .with_text("Calculate the factorial of 5 using code execution. Return just the number.")
                .with_code_execution()
                .with_store_enabled()
                .create()
                .await
        });

        let response1 = match result1 {
            Ok(response) => {
                println!("Turn 1 status: {:?}", response.status);
                if let Some(text) = response.as_text() {
                    println!("Turn 1 response: {}", text);
                }
                response
            }
            Err(e) => {
                let error_str = format!("{:?}", e);
                if error_str.contains("not supported") || error_str.contains("not available") {
                    println!("Code Execution tool not available - skipping test");
                    return;
                }
                panic!("Turn 1 failed unexpectedly: {:?}", e);
            }
        };

        assert_eq!(
            response1.status,
            InteractionStatus::Completed,
            "Turn 1 should complete successfully"
        );

        // Turn 2: Multiply the result by 2
        let prev_id = response1.id.clone().expect("id should exist");
        let result2 = retry_request!([client, prev_id] => {
            stateful_builder(&client)
                .with_previous_interaction(&prev_id)
                .with_text("Multiply the factorial result you just calculated by 2. What is the answer?")
                .with_code_execution()
                .with_store_enabled()
                .create()
                .await
        });

        match result2 {
            Ok(response2) => {
                println!("Turn 2 status: {:?}", response2.status);
                if let Some(text) = response2.as_text() {
                    println!("Turn 2 response: {}", text);
                }
                // Check for the expected calculation (5! * 2 = 240)
                let results = response2.code_execution_results();
                let has_correct_result = results.iter().any(|r| r.result.contains("240"))
                    || response2.as_text().is_some_and(|t| t.contains("240"));
                assert!(has_correct_result, "Turn 2 should calculate 120 * 2 = 240");
                assert_eq!(
                    response2.status,
                    InteractionStatus::Completed,
                    "Turn 2 should complete successfully"
                );
            }
            Err(e) => {
                if is_long_conversation_api_error(&e) {
                    println!("API limitation encountered: {:?}", e);
                    return;
                }
                panic!("Turn 2 failed unexpectedly: {:?}", e);
            }
        }

        println!("✓ Code Execution + multi-turn completed successfully");
    }
}
