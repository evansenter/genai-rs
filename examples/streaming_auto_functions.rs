//! Streaming with automatic function execution.
//!
//! `create_stream_with_auto_functions()` streams model output as it arrives,
//! pauses to run the requested `#[tool]` functions between rounds, and
//! resumes streaming the model's answer. Function-call arguments stream too,
//! as `StepDelta::ArgumentsDelta` fragments before the call executes.
//!
//! The tools return stub data; a real tool would call a weather API.
//!
//! Run with: `cargo run --example streaming_auto_functions`

use futures_util::StreamExt;
use genai_rs::{AutoFunctionStreamChunk, CallableFunction, Client};
use genai_rs_macros::tool;
use std::env;
use std::error::Error;
use std::io::{Write, stdout};

/// Gets the current weather for a city
#[tool(city(description = "The city to get weather for"))]
fn get_weather(city: String) -> serde_json::Value {
    serde_json::json!({"city": city, "temperature_c": 22, "conditions": "partly cloudy"})
}

/// Gets the local time for an IANA timezone
#[tool(timezone(description = "IANA timezone, e.g. Asia/Tokyo"))]
fn get_time(timezone: String) -> serde_json::Value {
    serde_json::json!({"timezone": timezone, "local_time": "14:30"})
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = Client::builder(api_key).build()?;

    let prompt = "What's the weather in Tokyo, and what time is it there?";
    println!("User: {prompt}\n");

    let mut stream = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_text(prompt)
        .add_functions(vec![
            GetWeatherCallable.declaration(),
            GetTimeCallable.declaration(),
        ])
        .create_stream_with_auto_functions();

    let mut executed = 0;
    while let Some(event) = stream.next().await {
        match event?.chunk {
            AutoFunctionStreamChunk::Delta(delta) => {
                if let Some(text) = delta.as_text() {
                    print!("{text}");
                } else if let Some(fragment) = delta.as_arguments_delta() {
                    eprint!("[args {fragment}]");
                }
                stdout().flush()?;
            }
            AutoFunctionStreamChunk::ExecutingFunctions { pending_calls, .. } => {
                for call in &pending_calls {
                    eprintln!("\n[executing {}({})]", call.name, call.args);
                }
            }
            AutoFunctionStreamChunk::FunctionResults(results) => {
                for result in &results {
                    eprintln!(
                        "[{} -> {} in {:?}]",
                        result.name, result.result, result.duration
                    );
                }
                executed += results.len();
            }
            AutoFunctionStreamChunk::Complete(response) => {
                eprintln!("\n[complete] status={:?}", response.status);
            }
            AutoFunctionStreamChunk::MaxLoopsReached(_) => {
                return Err("model was still calling functions when the loop limit hit".into());
            }
            // Non-exhaustive: new event types land here.
            other => eprintln!("[unrecognized event] {:?}", other.unknown_chunk_type()),
        }
    }

    if executed == 0 {
        return Err("the model answered without calling either tool".into());
    }
    Ok(())
}
