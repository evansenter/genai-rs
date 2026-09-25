//! Stateful tools via `ToolService`: dependency injection for function calling.
//!
//! `#[tool]` functions are free functions with no access to your application
//! state. A `ToolService` hands the auto-function loop `CallableFunction`s
//! that can hold anything — DB pools, API clients, configuration. Here that
//! is a precision setting shared through `Arc<RwLock<_>>` and changed between
//! two requests on the same service instance.
//!
//! Run with: `cargo run --example tool_service`

use async_trait::async_trait;
use genai_rs::{CallableFunction, Client, FunctionDeclaration, FunctionError, ToolService};
use serde_json::{Value, json};
use std::env;
use std::error::Error;
use std::sync::{Arc, RwLock};

struct Calculator {
    precision: Arc<RwLock<usize>>,
}

/// Parses `"<number> <op> <number>"`. A real tool would use a proper parser.
fn evaluate(expression: &str) -> Result<f64, String> {
    let parts: Vec<&str> = expression.split_whitespace().collect();
    let [a, op, b] = parts[..] else {
        return Err(format!(
            "expected '<number> <op> <number>', got '{expression}'"
        ));
    };
    let a: f64 = a.parse().map_err(|_| format!("not a number: {a}"))?;
    let b: f64 = b.parse().map_err(|_| format!("not a number: {b}"))?;
    match op {
        "+" => Ok(a + b),
        "-" => Ok(a - b),
        "*" => Ok(a * b),
        "/" if b != 0.0 => Ok(a / b),
        "/" => Err("division by zero".to_string()),
        _ => Err(format!("unsupported operator: {op}")),
    }
}

#[async_trait]
impl CallableFunction for Calculator {
    fn declaration(&self) -> FunctionDeclaration {
        FunctionDeclaration::builder("calculate")
            .with_description("Evaluate a binary arithmetic expression at the configured precision")
            .add_parameter(
                "expression",
                json!({
                    "type": "string",
                    "description": "Two numbers and one operator separated by spaces, e.g. '10 / 3'"
                }),
            )
            .with_required(vec!["expression".to_string()])
            .build()
    }

    async fn call(&self, args: Value) -> Result<Value, FunctionError> {
        let expression = args["expression"]
            .as_str()
            .ok_or_else(|| FunctionError::ArgumentMismatch("missing 'expression'".into()))?;
        // An Err here is sent back to the model as an error result, so it can
        // correct the expression instead of receiving a made-up number.
        let value = evaluate(expression).map_err(FunctionError::ArgumentMismatch)?;
        let precision = *self.precision.read().expect("precision lock poisoned");
        Ok(json!({"expression": expression, "result": format!("{value:.precision$}")}))
    }
}

struct MathService {
    precision: Arc<RwLock<usize>>,
}

impl ToolService for MathService {
    fn tools(&self) -> Vec<Arc<dyn CallableFunction>> {
        vec![Arc::new(Calculator {
            precision: Arc::clone(&self.precision),
        })]
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = Client::builder(api_key).build()?;

    let precision = Arc::new(RwLock::new(2));
    let service: Arc<dyn ToolService> = Arc::new(MathService {
        precision: Arc::clone(&precision),
    });

    for digits in [2, 8] {
        *precision.write().expect("precision lock poisoned") = digits;
        println!("--- precision = {digits} ---");

        let result = client
            .interaction()
            .with_model(genai_rs::DEFAULT_MODEL)
            .with_text("Use the calculate tool to compute 10 / 3, and report its exact result.")
            .with_tool_service(service.clone())
            .create_with_auto_functions()
            .await?;

        if result.executions.is_empty() {
            return Err("the model answered without calling the tool".into());
        }
        for exec in &result.executions {
            println!("  {}({}) -> {}", exec.name, exec.args, exec.result);
        }
        println!(
            "{}\n",
            result.response.as_text().ok_or("no text in response")?
        );
    }

    Ok(())
}
