//! Function calling: manual and automatic execution, parallel and sequential
//! calls, streaming, stateless replay, thinking, and multi-turn rules.
//!
//! A turn that must produce a call uses `FunctionCallingMode::Any`, so the
//! test asserts on the call instead of skipping when the model answers
//! directly. The follow-up turn goes back to the default (auto) mode, or it
//! could never answer in text.
//!
//! ```bash
//! cargo nextest run --test function_calling_tests --run-ignored all
//! ```

mod common;

use common::{
    assert_response_semantic, consume_auto_function_stream, consume_stream, extended_test_timeout,
    get_client, get_time_function, get_weather_function, interaction_builder, stateful_builder,
    test_timeout, with_timeout,
};
use genai_rs::{
    CallableFunction, FunctionCallingMode, FunctionDeclaration, FunctionExecutionResult,
    GenaiError, InteractionStatus, Step, ThinkingLevel,
};
use genai_rs_macros::tool;
use serde_json::json;

// =============================================================================
// Registered Test Functions
// =============================================================================
//
// `#[allow(dead_code)]` because the macro registers these through `inventory`;
// nothing calls them by name.

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

/// Implementation behind `common::get_weather_function()`.
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

/// Implementation behind `common::get_time_function()`.
#[allow(dead_code)]
#[tool(timezone(description = "Timezone like PST, EST, JST"))]
fn get_time(timezone: String) -> String {
    format!(r#"{{"timezone": "{}", "time": "14:00"}}"#, timezone)
}

const SYSTEM_INSTRUCTION: &str = "You are a helpful assistant that uses available tools when appropriate. Always respond concisely.";

fn weather_decl(description: &str) -> FunctionDeclaration {
    FunctionDeclaration::builder("get_weather")
        .with_description(description)
        .add_parameter(
            "city",
            json!({"type": "string", "description": "City name"}),
        )
        .with_required(vec!["city".to_string()])
        .build()
}

fn time_decl() -> FunctionDeclaration {
    FunctionDeclaration::builder("get_time")
        .with_description("Get the current time in a timezone")
        .add_parameter(
            "timezone",
            json!({"type": "string", "description": "Timezone like UTC, PST, JST"}),
        )
        .with_required(vec!["timezone".to_string()])
        .build()
}

// =============================================================================
// Basic Function Calling
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
            .with_description("Get the current server status (no parameters needed)")
            .build();

        let response = stateful_builder(&client)
            .with_text("Check the server status")
            .add_function(status_func.clone())
            .with_function_calling_mode(FunctionCallingMode::Any)
            .create()
            .await
            .expect("Interaction failed");

        let calls = response.function_calls();
        let call = calls.first().expect("Any mode should force a call");
        assert_eq!(call.name, "get_server_status");
        assert!(!call.id.is_empty(), "Should have call ID");

        let result = Step::function_result(
            "get_server_status",
            call.id.to_string(),
            json!({"status": "online", "uptime": "99.9%"}),
        );
        let response2 = stateful_builder(&client)
            .with_previous_interaction(response.id.as_ref().expect("id should exist"))
            .with_history(vec![result])
            .add_function(status_func)
            .create()
            .await
            .expect("Second interaction failed");

        let text = response2.as_text().expect("Should have final response");
        assert_response_semantic(
            &client,
            "The get_server_status function returned status online with 99.9% uptime.",
            text,
            "Does this response report that the server is online?",
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_function_call_complex_args() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let search_func = FunctionDeclaration::builder("search_with_filters")
            .with_description("Search with optional filters")
            .add_parameter(
                "user_id",
                json!({"type": "string", "description": "User ID"}),
            )
            .add_parameter(
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
            .with_required(vec!["user_id".to_string()])
            .build();

        let response = stateful_builder(&client)
            .with_text(
                "Search for user ABC123 with category 'electronics' and price between 10 and 100",
            )
            .add_function(search_func)
            .with_function_calling_mode(FunctionCallingMode::Any)
            .create()
            .await
            .expect("Interaction failed");

        let calls = response.function_calls();
        let call = calls.first().expect("Any mode should force a call");
        assert_eq!(call.args["user_id"], "ABC123", "args: {}", call.args);
        // The nested object must arrive as an object, not a stringified blob.
        let filters = call.args["filters"]
            .as_object()
            .unwrap_or_else(|| panic!("filters should be an object: {}", call.args));
        assert_eq!(filters["category"], "electronics", "filters: {filters:?}");
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_function_call_error_response() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let failing_func = FunctionDeclaration::builder("get_secret_data")
            .with_description("Get secret data (may fail)")
            .add_parameter("key", json!({"type": "string"}))
            .with_required(vec!["key".to_string()])
            .build();

        let response1 = stateful_builder(&client)
            .with_text("Get the secret data for key 'test123'")
            .add_function(failing_func.clone())
            .with_function_calling_mode(FunctionCallingMode::Any)
            .create()
            .await
            .expect("First interaction failed");

        let calls = response1.function_calls();
        let call = calls.first().expect("Any mode should force a call");
        let error_result = Step::function_result(
            "get_secret_data",
            call.id.to_string(),
            json!({"error": "Access denied: insufficient permissions"}),
        );

        let response2 = stateful_builder(&client)
            .with_previous_interaction(response1.id.as_ref().expect("id should exist"))
            .with_history(vec![error_result])
            .add_function(failing_func)
            .create()
            .await
            .expect("Second interaction failed");

        let text = response2
            .as_text()
            .expect("Model should respond to the error");
        assert_response_semantic(
            &client,
            "User asked to get secret data for key 'test123'. The function returned an error: 'Access denied: insufficient permissions'.",
            text,
            "Does this response acknowledge or explain that the request failed due to access/permission issues?",
        )
        .await;
    }

    /// Pending calls leave the interaction in `requires_action`; supplying
    /// the result completes it.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_requires_action_status() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(test_timeout(), async {
            let get_time = FunctionDeclaration::builder("get_current_time")
                .with_description("Get the current time")
                .build();

            let response = interaction_builder(&client)
                .with_text("What time is it right now?")
                .add_function(get_time.clone())
                .with_function_calling_mode(FunctionCallingMode::Any)
                .create()
                .await
                .expect("Interaction failed");

            assert_eq!(response.status, InteractionStatus::RequiresAction);
            let calls = response.function_calls();
            let call = calls.first().expect("Any mode should force a call");

            let response2 = interaction_builder(&client)
                .with_previous_interaction(response.id.as_ref().expect("id should exist"))
                .with_history(vec![Step::function_result(
                    "get_current_time",
                    call.id,
                    json!({"time": "14:30:00", "timezone": "UTC"}),
                )])
                .add_function(get_time)
                .create()
                .await
                .expect("Second interaction failed");

            assert_eq!(response2.status, InteractionStatus::Completed);
        })
        .await;
    }

    #[test]
    fn test_function_execution_result_error_detection() {
        use std::time::Duration;

        let success = FunctionExecutionResult::new(
            "get_weather",
            "call-123",
            json!({"city": "Seattle"}),
            json!({"city": "Seattle", "temp": "65°F"}),
            Duration::from_millis(100),
        );
        assert!(success.is_success());
        assert!(!success.is_error());
        assert!(success.error_message().is_none());

        let not_found = FunctionExecutionResult::new(
            "missing_function",
            "call-456",
            json!({"some": "args"}),
            json!({"error": "Function 'missing_function' is not available or not found."}),
            Duration::from_millis(1),
        );
        assert!(not_found.is_error());
        assert!(!not_found.is_success());
        assert_eq!(
            not_found.error_message(),
            Some("Function 'missing_function' is not available or not found.")
        );
    }
}

// =============================================================================
// Parallel Function Calls
// =============================================================================

mod parallel {
    use super::*;

    /// Two independent lookups arrive as parallel calls. Their results go
    /// back in reverse order and without resending tools, both of which the
    /// API accepts.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_parallel_function_calls() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(extended_test_timeout(), async {
            let response1 = retry_request!([client] => {
                stateful_builder(&client)
                    .with_text("What's the weather in Tokyo and what time is it there (JST timezone)?")
                    .add_functions(vec![weather_decl("Get the current weather for a city"), time_decl()])
                    .with_function_calling_mode(FunctionCallingMode::Any)
                    .create()
                    .await
            })
            .expect("Interaction failed");

            let calls = response1.function_calls();
            let mut names: Vec<&str> = calls.iter().map(|c| c.name).collect();
            names.sort_unstable();
            assert_eq!(names, ["get_time", "get_weather"], "expected one call to each function");
            assert_ne!(calls[0].id, calls[1].id, "parallel calls need distinct ids");

            let results: Vec<Step> = calls
                .iter()
                .rev()
                .map(|call| {
                    let data = match call.name {
                        "get_weather" => json!({"city": "Tokyo", "temperature": "22°C", "conditions": "sunny"}),
                        _ => json!({"timezone": "JST", "time": "14:30"}),
                    };
                    Step::function_result(call.name, call.id, data)
                })
                .collect();

            let prev_id = response1.id.clone().expect("id should exist");
            let response2 = retry_request!([client, prev_id, results] => {
                stateful_builder(&client)
                    .with_previous_interaction(&prev_id)
                    .with_history(results.clone())
                    .create()
                    .await
            })
            .expect("Reversed results without tools should be accepted");

            let text = response2.as_text().expect("Expected a text answer");
            assert_response_semantic(
                &client,
                "Functions returned: Tokyo weather 22°C sunny, and JST time 14:30.",
                text,
                "Does this response give both the Tokyo weather and the time?",
            )
            .await;
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_parallel_function_partial_failure() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(extended_test_timeout(), async {
            let functions = vec![
                weather_decl("Get weather for a city"),
                FunctionDeclaration::builder("get_stock_price")
                    .with_description("Get stock price (may fail)")
                    .add_parameter("symbol", json!({"type": "string"}))
                    .with_required(vec!["symbol".to_string()])
                    .build(),
            ];

            let response1 = stateful_builder(&client)
                .with_text(
                    "What's the weather in Tokyo and what's the stock price of INVALID_STOCK?",
                )
                .add_functions(functions)
                .with_function_calling_mode(FunctionCallingMode::Any)
                .create()
                .await
                .expect("Initial request failed");

            let calls = response1.function_calls();
            assert!(
                calls.iter().any(|c| c.name == "get_stock_price"),
                "expected a get_stock_price call, got {:?}",
                calls.iter().map(|c| c.name).collect::<Vec<_>>()
            );

            let results: Vec<Step> = calls
                .iter()
                .map(|call| {
                    let data = match call.name {
                        "get_weather" => {
                            json!({"city": "Tokyo", "temp": "22°C", "conditions": "sunny"})
                        }
                        _ => json!({"error": "Stock symbol not found", "code": "NOT_FOUND"}),
                    };
                    Step::function_result(call.name, call.id, data)
                })
                .collect();

            let response2 = client
                .interaction()
                .with_model(genai_rs::DEFAULT_MODEL)
                .with_previous_interaction(response1.id.as_ref().expect("id required"))
                .with_history(results)
                .create()
                .await
                .expect("Function result turn failed");

            let text = response2
                .as_text()
                .expect("Expected text with partial results");
            assert_response_semantic(
                &client,
                "The stock price lookup for INVALID_STOCK returned 'Stock symbol not found'.",
                text,
                "Does this response say the stock price could not be found?",
            )
            .await;
        })
        .await;
    }
}

// =============================================================================
// Sequential Function Chains
// =============================================================================

mod sequential {
    use super::*;

    /// A dependent chain: weather first, then a conversion that needs its
    /// result. `with_allowed_tools` pins which function each step must call.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_sequential_function_chain() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(extended_test_timeout(), async {
            let convert_temp = FunctionDeclaration::builder("convert_temperature")
                .with_description("Convert temperature between Celsius and Fahrenheit")
                .add_parameter("value", json!({"type": "number"}))
                .add_parameter(
                    "from_unit",
                    json!({"type": "string", "enum": ["celsius", "fahrenheit"]}),
                )
                .add_parameter(
                    "to_unit",
                    json!({"type": "string", "enum": ["celsius", "fahrenheit"]}),
                )
                .with_required(vec![
                    "value".to_string(),
                    "from_unit".to_string(),
                    "to_unit".to_string(),
                ])
                .build();
            let functions = vec![
                weather_decl("Get the current weather for a city (returns temperature in Celsius)"),
                convert_temp,
            ];

            let response1 = stateful_builder(&client)
                .with_text("What's the weather in Tokyo? Tell me the temperature in Fahrenheit.")
                .add_functions(functions.clone())
                .with_function_calling_mode(FunctionCallingMode::Any)
                .with_allowed_tools(vec!["get_weather".to_string()])
                .create()
                .await
                .expect("Step 1 failed");
            let calls1 = response1.function_calls();
            let call1 = calls1.first().expect("step 1 should call get_weather");
            assert_eq!(call1.name, "get_weather");

            let response2 = stateful_builder(&client)
                .with_previous_interaction(response1.id.as_ref().expect("id should exist"))
                .with_history(vec![Step::function_result(
                    call1.name,
                    call1.id,
                    json!({"city": "Tokyo", "temperature": 22.0, "unit": "celsius"}),
                )])
                .add_functions(functions.clone())
                .with_function_calling_mode(FunctionCallingMode::Any)
                .with_allowed_tools(vec!["convert_temperature".to_string()])
                .create()
                .await
                .expect("Step 2 failed");
            let calls2 = response2.function_calls();
            let call2 = calls2
                .first()
                .expect("step 2 should call convert_temperature");
            assert_eq!(call2.name, "convert_temperature");
            assert_eq!(
                call2.args["value"].as_f64(),
                Some(22.0),
                "should convert step 1's result: {}",
                call2.args
            );

            let response3 = stateful_builder(&client)
                .with_previous_interaction(response2.id.as_ref().expect("id should exist"))
                .with_history(vec![Step::function_result(
                    call2.name,
                    call2.id,
                    json!({"value": 71.6, "unit": "fahrenheit"}),
                )])
                .add_functions(functions)
                .create()
                .await
                .expect("Step 3 failed");

            let text = response3.as_text().expect("Step 3 should answer in text");
            assert_response_semantic(
                &client,
                "Weather lookup gave 22°C for Tokyo; conversion gave 71.6°F.",
                text,
                "Does this response give Tokyo's temperature as about 71.6°F?",
            )
            .await;
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
            let response1 = stateful_builder(&client)
                .with_text("What's the weather in Tokyo?")
                .add_function(weather_decl("Get the current weather for a city"))
                .with_function_calling_mode(FunctionCallingMode::Any)
                .create()
                .await
                .expect("First interaction failed");

            let calls = response1.function_calls();
            let call = calls.first().expect("Any mode should force a call");

            // Function-result turns do not need the tools resent.
            let response2 = stateful_builder(&client)
                .with_previous_interaction(response1.id.as_ref().expect("id should exist"))
                .with_history(vec![Step::function_result(
                    call.name,
                    call.id,
                    json!({"city": "Tokyo", "temperature": "22°C", "conditions": "sunny"}),
                )])
                .create()
                .await
                .expect("Function result turn failed - tools should not be required");

            assert!(
                response2.has_text(),
                "the result turn should answer in text"
            );
        })
        .await;
    }
}

// =============================================================================
// Streaming with Function Calls
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
            let stream = stateful_builder(&client)
                .with_text("What's the weather in London?")
                .add_function(weather_decl("Get the current weather for a city"))
                .with_function_calling_mode(FunctionCallingMode::Any)
                .create_stream();

            let result = consume_stream(stream).await;

            assert!(
                result.saw_function_call,
                "no function-call step or delta was streamed"
            );
            let response = result
                .final_response
                .expect("Should receive a complete response");
            let calls = response.function_calls();
            let call = calls
                .first()
                .expect("the accumulated response should carry the call");
            assert_eq!(call.name, "get_weather");
            assert!(call.args["city"].is_string(), "args: {}", call.args);
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

        let stream = interaction_builder(&client)
            .with_text("What's the weather in Tokyo?")
            .add_function(GetWeatherTestCallable.declaration())
            .create_stream_with_auto_functions();

        let result = consume_auto_function_stream(stream).await;

        assert!(
            result.executing_functions_count > 0,
            "no function execution was streamed"
        );
        assert!(
            result.function_results_count > 0,
            "no function results were streamed"
        );
        assert_eq!(result.executed_function_names, ["get_weather_test"]);
        let response = result
            .final_response
            .expect("Should receive a complete response");
        assert!(response.has_text(), "the loop should end in a text answer");
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
        // A computed value, so a substring check is deterministic.
        assert!(
            result.collected_text.contains('4'),
            "Response should contain the answer: {}",
            result.collected_text
        );
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_streaming_auto_functions_multiple_calls() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let stream = interaction_builder(&client)
            .with_text("What's the weather in London and what time is it there (GMT timezone)?")
            .add_functions(vec![
                GetWeatherTestCallable.declaration(),
                GetTimeTestCallable.declaration(),
            ])
            .create_stream_with_auto_functions();

        let result = consume_auto_function_stream(stream).await;

        let mut names = result.executed_function_names.clone();
        names.sort_unstable();
        assert_eq!(names, ["get_time_test", "get_weather_test"]);
        assert!(
            result.final_response.is_some(),
            "Should receive a complete response"
        );
    }

    /// With every turn forced to call, the loop can only stop at the limit.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_streaming_auto_functions_max_loops() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let stream = interaction_builder(&client)
            .with_text("What's the weather in Paris?")
            .add_function(GetWeatherTestCallable.declaration())
            .with_function_calling_mode(FunctionCallingMode::Any)
            .with_max_function_call_loops(1)
            .create_stream_with_auto_functions();

        let result = consume_auto_function_stream(stream).await;

        assert!(
            result.reached_max_loops,
            "the stream should end with MaxLoopsReached"
        );
        assert_eq!(result.executed_function_names, ["get_weather_test"]);
    }
}

// =============================================================================
// Automatic Function Execution
// =============================================================================

mod auto_execution {
    use super::*;
    use std::time::Duration;

    /// With every turn forced to call, the loop can only stop at the limit;
    /// hitting it is an `Ok` that says so, carrying the executions so far.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_auto_function_calling_max_loops() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let result = interaction_builder(&client)
            .with_text("What's the weather in Tokyo?")
            .add_function(GetWeatherTestCallable.declaration())
            .with_function_calling_mode(FunctionCallingMode::Any)
            .with_max_function_call_loops(1)
            .create_with_auto_functions()
            .await
            .expect("hitting the loop limit should not be an error");

        assert!(result.reached_max_loops);
        assert_eq!(result.executions.len(), 1, "{:?}", result.executions);
        assert_eq!(result.executions[0].name, "get_weather_test");
        assert!(result.all_executions_succeeded());
    }

    /// A declared function with no registered implementation produces a
    /// recoverable error result sent back to the model, not a hard failure.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_auto_function_with_unregistered_function() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let undefined_func = FunctionDeclaration::builder("undefined_function")
            .with_description("A function that doesn't have a registered handler")
            .add_parameter("input", json!({"type": "string"}))
            .build();

        let result = interaction_builder(&client)
            .with_text("Call the undefined_function with input 'test'")
            .add_function(undefined_func)
            .with_function_calling_mode(FunctionCallingMode::Any)
            .with_max_function_call_loops(1)
            .create_with_auto_functions()
            .await
            .expect("a missing implementation should not fail the loop");

        let failed = result.failed_executions();
        assert_eq!(failed.len(), 1, "{:?}", result.executions);
        assert_eq!(failed[0].name, "undefined_function");
        assert!(
            failed[0]
                .error_message()
                .is_some_and(|m| m.contains("undefined_function")),
            "the error should name the function: {:?}",
            failed[0].result
        );
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_auto_function_calling_multi_round_accumulation() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let result = interaction_builder(&client)
            .with_text(
                "What's the weather in Tokyo? I need the temperature in Fahrenheit, not Celsius. \
                 Use the convert_temperature function to convert the result.",
            )
            .add_functions(vec![
                GetWeatherTestCallable.declaration(),
                ConvertTemperatureCallable.declaration(),
            ])
            .create_with_auto_functions()
            .await
            .expect("Auto function calling failed");

        let names: Vec<&str> = result.executions.iter().map(|e| e.name.as_str()).collect();
        assert!(
            names.contains(&"get_weather_test") && names.contains(&"convert_temperature"),
            "both rounds should be accumulated, got {names:?}"
        );
        assert!(
            result.all_executions_succeeded(),
            "{:?}",
            result.failed_executions()
        );
        assert!(
            result.response.has_text(),
            "Should have final text response"
        );
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_auto_functions_timeout_returns_error() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

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
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_auto_functions_stream_timeout_returns_error() {
        use futures_util::StreamExt;

        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let mut stream = interaction_builder(&client)
            .with_text("What is 2 + 2?")
            .with_timeout(Duration::from_millis(1))
            .create_stream_with_auto_functions();

        let first_error = loop {
            match stream.next().await {
                Some(Ok(_)) => continue,
                Some(Err(e)) => break e,
                None => panic!("stream ended without the timeout error"),
            }
        };
        assert!(
            matches!(first_error, GenaiError::Timeout(_)),
            "Expected GenaiError::Timeout, got: {first_error:?}"
        );
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_auto_function_result_success_detection() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let result = client
            .interaction()
            .with_model(genai_rs::DEFAULT_MODEL)
            .with_text("What's the weather in Seattle?")
            .add_functions(vec![get_weather_function()])
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
}

// =============================================================================
// Stateless (store: false) Function Calling
// =============================================================================

mod stateless {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_stateless_function_calling_multi_turn() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(extended_test_timeout(), async {
            let functions = vec![
                weather_decl("Get the current weather for a city"),
                time_decl(),
            ];
            let mut history: Vec<Step> = vec![Step::user_text("What's the weather in Tokyo?")];

            let response1 = client
                .interaction()
                .with_model(genai_rs::DEFAULT_MODEL)
                .with_history(history.clone())
                .add_functions(functions.clone())
                .with_function_calling_mode(FunctionCallingMode::Any)
                .with_store_disabled()
                .create()
                .await
                .expect("First turn failed");

            let calls = response1.function_calls();
            assert!(!calls.is_empty(), "Any mode should force a call");

            // Replay the model's output (calls plus any thought signatures),
            // then the results.
            history.extend(response1.output_steps());
            for call in &calls {
                history.push(Step::function_result(
                    call.name,
                    call.id,
                    json!({"city": "Tokyo", "temperature": "22°C", "conditions": "sunny"}),
                ));
            }

            let response2 = client
                .interaction()
                .with_model(genai_rs::DEFAULT_MODEL)
                .with_history(history)
                .add_functions(functions)
                .with_store_disabled()
                .create()
                .await
                .expect("Function result turn failed");

            assert!(
                response2.has_text(),
                "Expected text response after function result"
            );
        })
        .await;
    }

    /// Stateless replay with thinking on: the turn-1 output, thought
    /// signatures included, must be accepted back as history.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_stateless_with_thinking_function_calling() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(extended_test_timeout(), async {
            let get_weather = weather_decl("Get the current weather for a city");
            let mut history: Vec<Step> = vec![Step::user_text("What's the weather in Paris?")];

            let response1 = client
                .interaction()
                .with_model(genai_rs::DEFAULT_MODEL)
                .with_history(history.clone())
                .add_function(get_weather.clone())
                .with_thinking_level(ThinkingLevel::Medium)
                .with_function_calling_mode(FunctionCallingMode::Any)
                .with_store_disabled()
                .create()
                .await
                .expect("Stateless thinking request failed");

            let calls = response1.function_calls();
            let call = calls.first().expect("Any mode should force a call");
            assert!(
                response1.thought_signatures().next().is_some(),
                "a thinking turn should return a thought signature to replay"
            );

            history.extend(response1.output_steps());
            history.push(Step::function_result(
                call.name,
                call.id,
                json!({"city": "Paris", "temperature": "18°C", "conditions": "cloudy"}),
            ));

            let response2 = client
                .interaction()
                .with_model(genai_rs::DEFAULT_MODEL)
                .with_history(history)
                .add_function(get_weather)
                .with_thinking_level(ThinkingLevel::Medium)
                .with_store_disabled()
                .create()
                .await
                .expect("Replaying thought signatures should be accepted");

            assert!(response2.has_text(), "Expected a text answer");
        })
        .await;
    }
}

// =============================================================================
// Thinking + Function Calling
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

        let get_weather =
            weather_decl("Get the current weather for a city including temperature and conditions");

        let response1 = retry_request!([client, get_weather] => {
            stateful_builder(&client)
                .with_text("What's the weather in Tokyo? Should I bring an umbrella?")
                .add_function(get_weather)
                .with_thinking_level(ThinkingLevel::Medium)
                .with_function_calling_mode(FunctionCallingMode::Any)
                .create()
                .await
        })
        .expect("Turn 1 failed");

        let calls = response1.function_calls();
        let call = calls.first().expect("Any mode should force a call");
        let function_result = Step::function_result(
            "get_weather",
            call.id.to_string(),
            json!({"temperature": "18°C", "conditions": "rainy", "precipitation": "80%", "humidity": "85%"}),
        );

        let prev_id = response1.id.clone().expect("id should exist");
        let response2 = retry_request!([client, prev_id, get_weather, function_result] => {
            stateful_builder(&client)
                .with_previous_interaction(&prev_id)
                .with_history(vec![function_result])
                .add_function(get_weather)
                .with_thinking_level(ThinkingLevel::Medium)
                .create()
                .await
        })
        .expect("Turn 2 failed");

        let text2 = response2.as_text().expect("Turn 2 should have text");
        assert_response_semantic(
            &client,
            "User asked 'What's the weather in Tokyo? Should I bring an umbrella?' and received weather data showing 18°C, rainy conditions, 80% precipitation",
            text2,
            "Does this response address the weather conditions and whether an umbrella is needed?",
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_thinking_with_parallel_function_calls() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let functions = vec![
            weather_decl("Get the current weather for a city"),
            time_decl(),
        ];
        let response1 = retry_request!([client, functions] => {
            stateful_builder(&client)
                .with_text("What's the weather in Tokyo and what time is it there? I need both pieces of information.")
                .add_functions(functions)
                .with_thinking_level(ThinkingLevel::Medium)
                .with_function_calling_mode(FunctionCallingMode::Any)
                .create()
                .await
        })
        .expect("Turn 1 failed");

        let calls = response1.function_calls();
        assert!(
            calls.len() >= 2,
            "expected parallel calls, got {}",
            calls.len()
        );

        let results: Vec<Step> = calls
            .iter()
            .map(|call| {
                let data = match call.name {
                    "get_weather" => json!({"temperature": "22°C", "conditions": "partly cloudy"}),
                    _ => json!({"time": "14:30", "timezone": "JST"}),
                };
                Step::function_result(call.name, call.id, data)
            })
            .collect();

        let prev_id = response1.id.clone().expect("id should exist");
        let response2 = retry_request!([client, prev_id, functions, results] => {
            stateful_builder(&client)
                .with_previous_interaction(&prev_id)
                .with_history(results)
                .add_functions(functions)
                .with_thinking_level(ThinkingLevel::Medium)
                .create()
                .await
        })
        .expect("Turn 2 failed");

        assert!(response2.has_text(), "Turn 2 should combine both results");
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_thinking_levels_with_function_calling() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let get_weather = weather_decl("Get the current weather for a city");
        for level in [
            ThinkingLevel::Low,
            ThinkingLevel::Medium,
            ThinkingLevel::High,
        ] {
            let response1 = retry_request!([client, get_weather, level] => {
                stateful_builder(&client)
                    .with_text("What's the weather in Paris?")
                    .add_function(get_weather)
                    .with_thinking_level(level)
                    .with_function_calling_mode(FunctionCallingMode::Any)
                    .create()
                    .await
            })
            .unwrap_or_else(|e| panic!("Turn 1 failed for {level:?}: {e}"));

            let calls = response1.function_calls();
            let call = calls
                .first()
                .unwrap_or_else(|| panic!("Any mode should force a call at {level:?}"));
            let fn_result = Step::function_result(
                "get_weather",
                call.id,
                json!({"temperature": "15°C", "conditions": "sunny"}),
            );

            let prev_id = response1.id.clone().expect("id should exist");
            let response2 = retry_request!([client, prev_id, get_weather, fn_result, level] => {
                stateful_builder(&client)
                    .with_previous_interaction(&prev_id)
                    .with_history(vec![fn_result])
                    .add_function(get_weather)
                    .with_thinking_level(level)
                    .create()
                    .await
            })
            .unwrap_or_else(|e| panic!("Turn 2 failed for {level:?}: {e}"));

            assert!(
                response2.has_text(),
                "{level:?} should produce a text response"
            );
        }
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_streaming_with_thinking_and_function_calling() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let get_weather = weather_decl("Get the current weather for a city");

        let stream = stateful_builder(&client)
            .with_text("What's the weather in Tokyo? I need to know if I should bring an umbrella.")
            .add_function(get_weather.clone())
            .with_thinking_level(ThinkingLevel::Medium)
            .with_function_calling_mode(FunctionCallingMode::Any)
            .create_stream();
        let result = consume_stream(stream).await;

        assert!(result.saw_function_call, "no function call was streamed");
        let response1 = result
            .final_response
            .expect("Should receive complete response");
        let calls = response1.function_calls();
        let call = calls
            .first()
            .expect("the accumulated response should carry the call");

        let stream2 = stateful_builder(&client)
            .with_previous_interaction(response1.id.as_ref().expect("id should exist"))
            .with_history(vec![Step::function_result(
                "get_weather",
                call.id,
                json!({"temperature": "18°C", "conditions": "rainy", "precipitation": "85%"}),
            )])
            .add_function(get_weather)
            .with_thinking_level(ThinkingLevel::Medium)
            .create_stream();
        let result2 = consume_stream(stream2).await;

        assert!(
            !result2.collected_text.is_empty(),
            "Turn 2 should stream text content"
        );
    }

    /// Per the thought-signatures guide, each step of a sequential chain
    /// carries its own signature.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_sequential_function_calls_with_thinking() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(extended_test_timeout(), async {
            let get_weather = weather_decl("Get the current weather");

            let response1 = stateful_builder(&client)
                .with_text("What's the weather in Tokyo?")
                .add_function(get_weather.clone())
                .with_thinking_level(ThinkingLevel::Low)
                .with_function_calling_mode(FunctionCallingMode::Any)
                .create()
                .await
                .expect("First interaction failed");
            let calls1 = response1.function_calls();
            let call1 = calls1.first().expect("Any mode should force a call");
            let sig1: Vec<String> = response1.thought_signatures().map(str::to_string).collect();

            let response2 = stateful_builder(&client)
                .with_previous_interaction(response1.id.as_ref().expect("id should exist"))
                .with_history(vec![Step::function_result(
                    "get_weather",
                    call1.id.to_string(),
                    json!({"temperature": "22°C"}),
                )])
                .with_text("Now what about Paris?")
                .add_function(get_weather)
                .with_thinking_level(ThinkingLevel::Low)
                .with_function_calling_mode(FunctionCallingMode::Any)
                .create()
                .await
                .expect("Second interaction failed");
            let calls2 = response2.function_calls();
            assert!(!calls2.is_empty(), "Any mode should force a call");
            let sig2: Vec<String> = response2.thought_signatures().map(str::to_string).collect();

            assert!(!sig1.is_empty(), "step 1 should carry a thought signature");
            assert!(!sig2.is_empty(), "step 2 should carry a thought signature");
            assert!(
                sig2.iter().all(|s| !sig1.contains(s)),
                "step 2 should carry its own signature, not repeat step 1's"
            );
        })
        .await;
    }
}

// =============================================================================
// Multi-turn Function Calling
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

        let result1 = client
            .interaction()
            .with_model(genai_rs::DEFAULT_MODEL)
            .with_text("What's the weather in Seattle?")
            .add_functions(functions.clone())
            .with_store_enabled()
            .with_system_instruction(SYSTEM_INSTRUCTION)
            .create_with_auto_functions()
            .await
            .expect("Turn 1 should succeed");

        assert!(
            result1.executions.iter().any(|e| e.name == "get_weather"),
            "Should have executed get_weather function"
        );
        assert!(
            result1.all_executions_succeeded(),
            "All function executions should succeed. Failed: {:?}",
            result1.failed_executions()
        );
        let turn1_id = result1.response.id.clone().expect("Turn 1 should have ID");

        // New user turns must resend tools.
        let result2 = client
            .interaction()
            .with_model(genai_rs::DEFAULT_MODEL)
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
        let turn2_id = result2.response.id.clone().expect("Turn 2 should have ID");

        let result3 = client
            .interaction()
            .with_model(genai_rs::DEFAULT_MODEL)
            .with_text("Based on the weather you just told me about Seattle and Tokyo, which city is warmer right now?")
            .add_functions(functions)
            .with_store_enabled()
            .with_previous_interaction(&turn2_id)
            .create_with_auto_functions()
            .await
            .expect("Turn 3 should succeed");

        let text = result3
            .response
            .as_text()
            .expect("Turn 3 should have text response");
        assert_response_semantic(
            &client,
            "Previous turns retrieved weather: Seattle (65°F) and Tokyo (72°F). User asked which city is warmer.",
            text,
            "Does this response correctly identify which city is warmer based on temperature comparison?",
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_multiturn_tools_not_inherited() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let functions = vec![get_weather_function()];

        let result1 = retry_request!([client, functions] => {
            stateful_builder(&client)
                .with_text("Remember: I'm interested in weather data.")
                .add_functions(functions)
                .create()
                .await
        })
        .expect("Turn 1 should succeed");
        let turn1_id = result1.id.clone().expect("Turn 1 should have ID");

        // Deliberately not a weather lookup: asked for one without the tool,
        // the model reliably emits a malformed call the API rejects as
        // `400 invalid JSON syntax`, which says nothing about inheritance.
        let result2 = retry_request!([client, turn1_id] => {
            stateful_builder(&client)
                .with_text("In one word, what topic did I say I was interested in?")
                .with_previous_interaction(&turn1_id)
                .create()
                .await
        })
        .expect("Turn 2 should succeed");

        // Tools are not inherited; history is.
        let function_calls = result2.function_calls();
        assert!(
            function_calls.is_empty(),
            "Model should not make function calls when tools not provided (got {} calls)",
            function_calls.len()
        );
        let text = result2.as_text().unwrap_or_default().to_string();
        assert_response_semantic(
            &client,
            "The user previously said they were interested in weather data, then asked \
             what topic they had mentioned.",
            &text,
            "Does this identify weather (or an equivalent term like forecasts) as the topic?",
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_multiturn_manual_functions_happy_path() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let functions = vec![get_weather_function(), get_time_function()];

        let response1 = client
            .interaction()
            .with_model(genai_rs::DEFAULT_MODEL)
            .with_text("What's the weather in London and what time is it there?")
            .add_functions(functions.clone())
            .with_function_calling_mode(FunctionCallingMode::Any)
            .with_store_enabled()
            .with_system_instruction(SYSTEM_INSTRUCTION)
            .create()
            .await
            .expect("Turn 1 should succeed");

        let function_calls = response1.function_calls();
        assert!(!function_calls.is_empty(), "Any mode should force a call");

        let results: Vec<Step> = function_calls
            .iter()
            .map(|call| {
                let data = match call.name {
                    "get_weather" => {
                        json!({"city": "London", "temperature": "15°C", "conditions": "cloudy"})
                    }
                    _ => json!({"timezone": "GMT", "time": "14:30:00"}),
                };
                Step::function_result(call.name, call.id, data)
            })
            .collect();

        let response2 = client
            .interaction()
            .with_model(genai_rs::DEFAULT_MODEL)
            .with_previous_interaction(response1.id.as_ref().expect("Should have ID"))
            .with_history(results)
            .add_functions(functions.clone())
            .with_store_enabled()
            .create()
            .await
            .expect("Function result submission should succeed");
        assert!(
            response2.has_text(),
            "Should have text response after function results"
        );

        let response3 = client
            .interaction()
            .with_model(genai_rs::DEFAULT_MODEL)
            .with_text("Is it a good time to call someone there?")
            .add_functions(functions)
            .with_store_enabled()
            .with_previous_interaction(response2.id.as_ref().expect("Should have ID"))
            .create()
            .await
            .expect("Turn 3 should succeed");

        assert!(
            response3.has_text() || response3.has_function_calls(),
            "Turn 3 produced nothing"
        );
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_multiturn_streaming_auto_functions() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let functions = vec![get_weather_function()];

        let result1 = consume_auto_function_stream(
            client
                .interaction()
                .with_model(genai_rs::DEFAULT_MODEL)
                .with_text("What's the weather in Miami?")
                .add_functions(functions.clone())
                .with_store_enabled()
                .with_system_instruction(SYSTEM_INSTRUCTION)
                .create_stream_with_auto_functions(),
        )
        .await;
        assert!(
            result1.function_results_count > 0,
            "Turn 1 should execute a function"
        );
        let response1 = result1.final_response.expect("Should have final response");

        let result2 = consume_auto_function_stream(
            client
                .interaction()
                .with_model(genai_rs::DEFAULT_MODEL)
                .with_text("Compare that to New York.")
                .add_functions(functions)
                .with_store_enabled()
                .with_previous_interaction(response1.id.as_ref().expect("Should have ID"))
                .create_stream_with_auto_functions(),
        )
        .await;
        assert!(
            result2.function_results_count > 0,
            "Turn 2 should execute a function for New York"
        );
        assert!(result2.final_response.is_some(), "Turn 2 should complete");
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_function_calling_mode_any() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let response = stateful_builder(&client)
            .with_text("What's the weather in Tokyo?")
            .add_function(weather_decl("Get the current weather for a city"))
            .with_thinking_level(ThinkingLevel::Medium)
            .with_function_calling_mode(FunctionCallingMode::Any)
            .create()
            .await
            .expect("FC mode Any request failed");

        let calls = response.function_calls();
        let call = calls
            .first()
            .expect("Model should call function with FunctionCallingMode::Any");
        assert_eq!(call.name, "get_weather");
        assert!(!response.has_text() || response.has_function_calls());
    }
}
