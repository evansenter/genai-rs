//! Manual function calling: you run the loop and execute the calls.
//!
//! Use this instead of `create_with_auto_functions()` when execution needs
//! your own control: rate limits, caching, approval, or running calls
//! concurrently as done here. The loop:
//!
//! 1. `create()` with function declarations
//! 2. read `response.function_calls()` (several may arrive at once)
//! 3. execute them, here concurrently with `join_all`
//! 4. send a `Step::function_result` per call, chained with `with_previous_interaction`
//! 5. repeat until the model answers in text
//!
//! The prompt needs both shapes: independent calls in one round (weather
//! for three cities) and a dependent call in a later round (converting a
//! temperature the first round returned). Weather is stub data.
//!
//! Run with: `cargo run --example manual_function_calling`

use futures_util::future::join_all;
use genai_rs::{Client, FunctionDeclaration, Step};
use serde_json::{Value, json};
use std::env;
use std::error::Error;

const MAX_ROUNDS: usize = 5;

fn declarations() -> Vec<FunctionDeclaration> {
    vec![
        FunctionDeclaration::builder("get_weather")
            .description("Get the current weather for a city")
            .parameter(
                "city",
                json!({"type": "string", "description": "City name"}),
            )
            .required(vec!["city".to_string()])
            .build(),
        FunctionDeclaration::builder("convert_temperature")
            .description("Convert a temperature between celsius and fahrenheit")
            .parameter("value", json!({"type": "number"}))
            .parameter(
                "to_unit",
                json!({"type": "string", "enum": ["celsius", "fahrenheit"]}),
            )
            .required(vec!["value".to_string(), "to_unit".to_string()])
            .build(),
    ]
}

/// Your execution logic. Bad arguments go back to the model as an error
/// result it can react to, rather than being papered over with defaults.
async fn execute(name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "get_weather" => {
            let city = args["city"]
                .as_str()
                .ok_or("missing string argument 'city'")?;
            let temperature_c = match city.to_lowercase().as_str() {
                "tokyo" => 22.0,
                "london" => 15.0,
                "new york" => 18.0,
                _ => 20.0,
            };
            Ok(json!({"city": city, "temperature_c": temperature_c, "conditions": "clear"}))
        }
        "convert_temperature" => {
            let value = args["value"]
                .as_f64()
                .ok_or("missing numeric argument 'value'")?;
            match args["to_unit"].as_str() {
                Some("fahrenheit") => {
                    Ok(json!({"value": value * 9.0 / 5.0 + 32.0, "unit": "fahrenheit"}))
                }
                Some("celsius") => {
                    Ok(json!({"value": (value - 32.0) * 5.0 / 9.0, "unit": "celsius"}))
                }
                _ => Err("'to_unit' must be celsius or fahrenheit".to_string()),
            }
        }
        _ => Err(format!("unknown function: {name}")),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = Client::builder(api_key).build()?;

    let prompt = "What's the weather in Tokyo, London and New York? \
                  Then convert the warmest city's temperature to fahrenheit.";
    println!("User: {prompt}\n");

    let mut response = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_text(prompt)
        .add_functions(declarations())
        .create()
        .await?;

    for round in 1..=MAX_ROUNDS {
        let calls = response.function_calls();
        if calls.is_empty() {
            println!("\n{}", response.as_text().ok_or("no text in response")?);
            return Ok(());
        }

        println!("Round {round}: {} call(s)", calls.len());
        let results = join_all(calls.iter().map(|call| execute(call.name, call.args))).await;

        // Each result carries its call's ID; that is what the API matches on.
        let mut steps = Vec::with_capacity(calls.len());
        for (call, result) in calls.iter().zip(results) {
            steps.push(match result {
                Ok(value) => {
                    println!("  {}({}) -> {value}", call.name, call.args);
                    Step::function_result(call.name, call.id, value)
                }
                Err(message) => {
                    println!("  {}({}) -> error: {message}", call.name, call.args);
                    Step::function_result_error(call.name, call.id, json!({"error": message}))
                }
            });
        }

        let previous = response.id.clone().ok_or("stored interaction has no ID")?;
        response = client
            .interaction()
            .with_model(genai_rs::DEFAULT_MODEL)
            .with_previous_interaction(&previous)
            .with_history(steps)
            // Tools are not inherited across turns. The API tolerates omitting
            // them on a function-result turn, but resending keeps one code path.
            .add_functions(declarations())
            .create()
            .await?;
    }

    Err(format!("model was still calling functions after {MAX_ROUNDS} rounds").into())
}
