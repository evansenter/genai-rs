//! `ToolService`: runtime-registered, stateful tools, with and without
//! streaming, and their precedence over `#[tool]` registrations.
//!
//! ```bash
//! cargo nextest run --test tool_service_tests --run-ignored all
//! ```

mod common;

use async_trait::async_trait;
use common::{
    assert_response_semantic, consume_auto_function_stream, extended_test_timeout, get_client,
    interaction_builder, test_timeout, with_timeout,
};
use genai_rs::{CallableFunction, FunctionDeclaration, FunctionError, ToolService};
use genai_rs_macros::tool;
use serde_json::json;
use std::sync::Arc;

/// A global `#[tool]` registration that `CustomWeatherTool` shadows. It must
/// live in this binary: `inventory` registrations are per test binary.
#[allow(dead_code)]
#[tool(city(description = "The city name"))]
fn get_weather_test(city: String) -> String {
    format!(r#"{{"city": "{city}", "source": "global_registry"}}"#)
}

// =============================================================================
// ToolService Helper Types
// =============================================================================

/// Configuration for the calculator tool
struct CalculatorConfig {
    precision: u32,
}

/// A calculator tool that uses injected configuration
struct CalculatorTool {
    config: Arc<CalculatorConfig>,
}

#[async_trait]
impl CallableFunction for CalculatorTool {
    fn declaration(&self) -> FunctionDeclaration {
        FunctionDeclaration::builder("calculate")
            .description("Performs arithmetic calculations")
            .parameter(
                "operation",
                json!({"type": "string", "enum": ["add", "subtract", "multiply"]}),
            )
            .parameter(
                "a",
                json!({"type": "number", "description": "First operand"}),
            )
            .parameter(
                "b",
                json!({"type": "number", "description": "Second operand"}),
            )
            .required(vec![
                "operation".to_string(),
                "a".to_string(),
                "b".to_string(),
            ])
            .build()
    }

    async fn call(&self, args: serde_json::Value) -> Result<serde_json::Value, FunctionError> {
        let op = args
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| FunctionError::ArgumentMismatch("Missing 'operation'".into()))?;
        let a = args
            .get("a")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| FunctionError::ArgumentMismatch("Missing 'a'".into()))?;
        let b = args
            .get("b")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| FunctionError::ArgumentMismatch("Missing 'b'".into()))?;

        let result = match op {
            "add" => a + b,
            "subtract" => a - b,
            "multiply" => a * b,
            _ => return Err(FunctionError::ArgumentMismatch("Invalid operation".into())),
        };

        // Apply precision from config
        let formatted = format!("{:.prec$}", result, prec = self.config.precision as usize);

        Ok(json!({
            "result": formatted,
            "precision": self.config.precision
        }))
    }
}

/// A service that provides the calculator tool with injected dependencies
struct MathToolService {
    config: Arc<CalculatorConfig>,
}

impl MathToolService {
    fn new(precision: u32) -> Self {
        Self {
            config: Arc::new(CalculatorConfig { precision }),
        }
    }
}

impl ToolService for MathToolService {
    fn tools(&self) -> Vec<Arc<dyn CallableFunction>> {
        vec![Arc::new(CalculatorTool {
            config: self.config.clone(),
        })]
    }
}

// =============================================================================
// ToolService Non-Streaming Tests
// =============================================================================

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_tool_service_non_streaming() {
    // Test that ToolService works with create_with_auto_functions()
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    with_timeout(test_timeout(), async {
        // Create a tool service with specific configuration
        let service = Arc::new(MathToolService::new(4)); // 4 decimal places

        let result = interaction_builder(&client)
            .with_text("What is 123.456 + 789.012? Use the calculate function.")
            .with_tool_service(service)
            .create_with_auto_functions()
            .await
            .expect("Auto function calling with ToolService failed");

        println!("Function executions: {:?}", result.executions);

        let exec = result
            .executions
            .iter()
            .find(|e| e.name == "calculate")
            .unwrap_or_else(|| panic!("calculate was not executed: {:?}", result.executions));
        // The service's precision config shapes the result: 4 decimal places.
        assert_eq!(exec.result["precision"], 4, "{}", exec.result);
        assert_eq!(exec.result["result"], "912.4680", "{}", exec.result);

        let text = result
            .response
            .as_text()
            .expect("Should have text response");
        assert_response_semantic(
            &client,
            "The calculate tool returned 912.4680 for 123.456 + 789.012.",
            text,
            "Does this response give the sum as about 912.468?",
        )
        .await;
    })
    .await;
}

// =============================================================================
// ToolService Streaming Tests
// =============================================================================

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_tool_service_streaming() {
    // Test that ToolService works with create_stream_with_auto_functions()
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    with_timeout(test_timeout(), async {
        // Create a tool service with specific configuration
        let service = Arc::new(MathToolService::new(2)); // 2 decimal places

        let stream = interaction_builder(&client)
            .with_text("Calculate 50 * 3. Use the calculate function.")
            .with_tool_service(service)
            .create_stream_with_auto_functions();

        let result = consume_auto_function_stream(stream).await;

        println!("\n--- Results ---");
        println!("Delta count: {}", result.delta_count);
        println!(
            "Executing functions count: {}",
            result.executing_functions_count
        );
        println!("Functions executed: {:?}", result.executed_function_names);

        assert_eq!(
            result.executed_function_names,
            ["calculate"],
            "the ToolService function should be executed"
        );
        let response = result
            .final_response
            .expect("Should receive a complete response");
        assert!(
            response.has_text() || !result.collected_text.is_empty(),
            "Should have text response"
        );

        let text = response.as_text().unwrap_or(&result.collected_text);
        println!("Final response: {}", text);

        // Should mention 150 (the result of 50 * 3)
        assert!(
            text.contains("150"),
            "Response should mention the calculation result (150)"
        );
    })
    .await;
}

// =============================================================================
// ToolService Override Tests
// =============================================================================

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_tool_service_overrides_global_registry() {
    // Test that ToolService functions take precedence over global registry
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    with_timeout(test_timeout(), async {
        // Create a custom weather tool that returns a distinct response
        struct CustomWeatherTool;

        #[async_trait]
        impl CallableFunction for CustomWeatherTool {
            fn declaration(&self) -> FunctionDeclaration {
                // Same name as the global get_weather_test function
                FunctionDeclaration::builder("get_weather_test")
                    .description("Get the current weather for a city")
                    .parameter(
                        "city",
                        json!({"type": "string", "description": "The city name"}),
                    )
                    .required(vec!["city".to_string()])
                    .build()
            }

            async fn call(
                &self,
                args: serde_json::Value,
            ) -> Result<serde_json::Value, FunctionError> {
                let city = args
                    .get("city")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown");

                // Return a distinctive response to prove this override was used
                Ok(json!({
                    "city": city,
                    "temperature": "999°C",
                    "conditions": "OVERRIDE_FROM_TOOL_SERVICE",
                    "source": "custom_service"
                }))
            }
        }

        struct CustomWeatherService;

        impl ToolService for CustomWeatherService {
            fn tools(&self) -> Vec<Arc<dyn CallableFunction>> {
                vec![Arc::new(CustomWeatherTool)]
            }
        }

        let service = Arc::new(CustomWeatherService);

        let result = interaction_builder(&client)
            .with_text("What's the weather in Seattle? Use the get_weather_test function.")
            .with_tool_service(service)
            .create_with_auto_functions()
            .await
            .expect("Auto function calling with override failed");

        println!("Function executions: {:?}", result.executions);

        // Verify the function was called
        assert!(
            !result.executions.is_empty(),
            "Should have at least one function execution"
        );
        assert_eq!(
            result.executions[0].name, "get_weather_test",
            "Should have called get_weather_test"
        );

        // Verify the custom service's response was used
        let exec_result = &result.executions[0].result;
        println!("Execution result: {}", exec_result);

        // The result should come from our custom tool (has "source": "custom_service")
        assert!(
            exec_result.get("source").is_some()
                || exec_result
                    .to_string()
                    .contains("OVERRIDE_FROM_TOOL_SERVICE"),
            "Result should come from the custom ToolService, not global registry"
        );
    })
    .await;
}

// =============================================================================
// ToolService Multiple Functions Tests
// =============================================================================

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_tool_service_streaming_with_multiple_functions() {
    // Test ToolService streaming with multiple functions available
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    with_timeout(extended_test_timeout(), async {
        // Create a service with multiple tools
        struct MultiToolService;

        struct AddTool;

        #[async_trait]
        impl CallableFunction for AddTool {
            fn declaration(&self) -> FunctionDeclaration {
                FunctionDeclaration::builder("add_numbers")
                    .description("Adds two numbers together")
                    .parameter("a", json!({"type": "number"}))
                    .parameter("b", json!({"type": "number"}))
                    .required(vec!["a".to_string(), "b".to_string()])
                    .build()
            }

            async fn call(&self, args: serde_json::Value) -> Result<serde_json::Value, FunctionError> {
                let a = args.get("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let b = args.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0);
                Ok(json!({ "sum": a + b }))
            }
        }

        struct MultiplyTool;

        #[async_trait]
        impl CallableFunction for MultiplyTool {
            fn declaration(&self) -> FunctionDeclaration {
                FunctionDeclaration::builder("multiply_numbers")
                    .description("Multiplies two numbers together")
                    .parameter("a", json!({"type": "number"}))
                    .parameter("b", json!({"type": "number"}))
                    .required(vec!["a".to_string(), "b".to_string()])
                    .build()
            }

            async fn call(&self, args: serde_json::Value) -> Result<serde_json::Value, FunctionError> {
                let a = args.get("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let b = args.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0);
                Ok(json!({ "product": a * b }))
            }
        }

        impl ToolService for MultiToolService {
            fn tools(&self) -> Vec<Arc<dyn CallableFunction>> {
                vec![Arc::new(AddTool), Arc::new(MultiplyTool)]
            }
        }

        let service = Arc::new(MultiToolService);

        // Ask a question that might trigger both functions
        let stream = interaction_builder(&client)
            .with_text("What is 5 + 3, and what is 4 * 7? Use the add_numbers and multiply_numbers functions.")
            .with_tool_service(service)
            .create_stream_with_auto_functions();

        let result = consume_auto_function_stream(stream).await;

        println!("\n--- Results ---");
        println!("Delta count: {}", result.delta_count);
        println!(
            "Executing functions count: {}",
            result.executing_functions_count
        );
        println!("Functions executed: {:?}", result.executed_function_names);

        let mut names = result.executed_function_names.clone();
        names.sort_unstable();
        assert_eq!(names, ["add_numbers", "multiply_numbers"]);

        let response = result.final_response.expect("Should receive a complete response");
        let text = response.as_text().unwrap_or(&result.collected_text);
        assert_response_semantic(
            &client,
            "add_numbers returned 8 for 5 + 3 and multiply_numbers returned 28 for 4 * 7.",
            text,
            "Does this response give both 8 and 28?",
        )
        .await;
    })
    .await;
}
