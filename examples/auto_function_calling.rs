//! Automatic function calling with the `#[tool]` macro.
//!
//! `#[tool]` generates a `FunctionDeclaration` from the signature and doc
//! comment, and registers the function globally. `create_with_auto_functions()`
//! then runs the loop for you: send, execute the requested calls, send the
//! results back, repeat until the model answers in text.
//!
//! The weather tools return stub data; a real tool would call a weather API.
//!
//! Run with: `cargo run --example auto_function_calling`

use genai_rs::{AutoFunctionResult, CallableFunction, Client, FunctionCallingMode};
use genai_rs_macros::tool;
use std::env;
use std::error::Error;

/// Lists the cities weather data is available for
#[tool]
fn list_cities() -> Vec<String> {
    vec!["Tokyo".into(), "Paris".into(), "Lima".into()]
}

/// Gets the current weather for a city
#[tool(city(description = "The city to get weather for"))]
fn get_weather(city: String) -> serde_json::Value {
    serde_json::json!({"city": city, "temperature_c": 22, "conditions": "partly cloudy"})
}

/// Gets a daily forecast for a city
#[tool(
    city(description = "The city to forecast"),
    days(description = "Number of days, 1 to 7"),
    unit(enum_values = ["celsius", "fahrenheit"])
)]
fn get_forecast(city: String, days: i32, unit: Option<String>) -> serde_json::Value {
    let fahrenheit = unit.as_deref() == Some("fahrenheit");
    let highs: Vec<i32> = (0..days.clamp(1, 7))
        .map(|day| 20 + day)
        .map(|c| if fahrenheit { c * 9 / 5 + 32 } else { c })
        .collect();
    serde_json::json!({
        "city": city,
        "unit": if fahrenheit { "fahrenheit" } else { "celsius" },
        "daily_highs": highs,
    })
}

fn print_result(result: &AutoFunctionResult) -> Result<(), Box<dyn Error>> {
    for exec in &result.executions {
        println!("  called {}({}) -> {}", exec.name, exec.args, exec.result);
    }
    if result.reached_max_loops {
        return Err("model was still calling functions when the loop limit hit".into());
    }
    println!(
        "{}\n",
        result.response.as_text().ok_or("no text in response")?
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = Client::builder(api_key).build()?;
    let model = genai_rs::DEFAULT_MODEL;

    // With no functions given, every #[tool] in the binary is offered.
    println!("--- Auto-discovery ---");
    let result = client
        .interaction()
        .with_model(model)
        .with_text("Which cities do you have weather for? Give me the weather in the first one.")
        .create_with_auto_functions()
        .await?;
    print_result(&result)?;

    // add_function()/add_functions() limit the model to an explicit subset.
    // Typed parameters (i32, Option<String> with enum values) are converted
    // from the model's JSON arguments before the function runs.
    println!("--- Explicit subset, typed parameters ---");
    let result = client
        .interaction()
        .with_model(model)
        .with_text("Give me a 3-day forecast for Tokyo in fahrenheit.")
        .add_function(GetForecastCallable.declaration())
        .create_with_auto_functions()
        .await?;
    print_result(&result)?;

    // Calling modes, with create() so the calls come back unexecuted.
    // Auto (the default) lets the model decide.
    println!("--- Calling modes ---");
    let forced = client
        .interaction()
        .with_model(model)
        .with_text("Say hello.")
        .add_function(GetWeatherCallable.declaration())
        .with_function_calling_mode(FunctionCallingMode::Any)
        .create()
        .await?;
    let call = forced
        .function_calls()
        .into_iter()
        .next()
        .ok_or("Any mode returned no function call")?;
    println!(
        "Any: model had to call a function, chose {}({})",
        call.name, call.args
    );

    let disabled = client
        .interaction()
        .with_model(model)
        .with_text("What's the weather in Paris?")
        .add_function(GetWeatherCallable.declaration())
        .with_function_calling_mode(FunctionCallingMode::None)
        .create()
        .await?;
    if disabled.has_function_calls() {
        return Err("None mode still produced a function call".into());
    }
    println!(
        "None: answered without calling: {}",
        disabled.as_text().ok_or("no text in response")?
    );

    Ok(())
}
