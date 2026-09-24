//! Code execution: the model writes and runs Python in a server-side sandbox.
//!
//! The response interleaves the code the model ran (`code_execution_calls()`),
//! what it printed or raised (`code_execution_results()`), and its text.
//!
//! Run with: `cargo run --example code_execution`

use genai_rs::Client;
use std::env;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = Client::builder(api_key).build()?;

    let response = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_text("Use Python to compute the sum of the first 50 prime numbers.")
        .with_code_execution()
        .create()
        .await?;

    let calls = response.code_execution_calls();
    if calls.is_empty() {
        return Err("the model answered without running code".into());
    }
    for call in calls {
        println!("--- {} code ---\n{}", call.language, call.code);
    }
    for result in response.code_execution_results() {
        let label = if result.is_error { "error" } else { "output" };
        println!("--- {label} ---\n{}", result.result.trim_end());
    }
    println!(
        "--- answer ---\n{}",
        response.as_text().ok_or("no text in response")?
    );

    Ok(())
}
