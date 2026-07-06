//! Example: Code Execution with Gemini
//!
//! This example demonstrates how to use Gemini's built-in code execution
//! capability to run Python code in a sandboxed environment.
//!
//! Shows both non-streaming and streaming usage.
//!
//! Run with: cargo run --example code_execution

use futures_util::StreamExt;
use genai_rs::{Client, GenaiError, StreamChunk};
use std::env;
use std::error::Error;
use std::io::{Write, stdout};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // 1. Get API Key from environment variable
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in environment");

    // Create the client
    let client = Client::builder(api_key).build()?;

    // 2. Create an interaction with code execution enabled
    let model_name = "gemini-3-flash-preview";
    let prompt = "Calculate the first 20 prime numbers using Python. \
                  Show both the code and explain the results.";

    println!("Creating interaction with model: {model_name}");
    println!("Prompt: {prompt}\n");

    // 3. Send the interaction request with code execution enabled
    match client
        .interaction()
        .with_model(model_name)
        .with_text(prompt)
        .with_code_execution() // Enable Python code execution
        .with_store_enabled()
        .create()
        .await
    {
        Ok(response) => {
            println!("--- Code Execution Response ---");
            println!("Interaction ID: {:?}", response.id);
            println!("Status: {:?}\n", response.status);

            // 4. Display the model's text response
            if let Some(text) = response.as_text() {
                println!("Model Explanation:\n{text}\n");
            }

            // 5. Display the executed code
            println!("--- Executed Code ---");
            for call in response.code_execution_calls() {
                println!("Language: {}", call.language);
                println!("```python\n{}\n```\n", call.code);
            }

            // 6. Display the code execution results
            println!("--- Execution Results ---");
            for result in response.code_execution_results() {
                if result.is_error {
                    println!("Status: FAILED");
                    println!("Error:\n{}", result.result);
                } else {
                    println!("Status: SUCCESS");
                    println!("Output:\n{}", result.result);
                }
            }

            // 7. Use convenience helper to get the first successful output
            if let Some(output) = response.successful_code_output() {
                println!("\n--- Quick Result ---");
                println!("First successful output: {output}");
            }

            // 8. Show step summary
            let summary = response.step_summary();
            println!("\n--- Step Summary ---");
            println!("  Text blocks: {}", summary.text_count);
            println!(
                "  Code execution calls: {}",
                summary.code_execution_call_count
            );
            println!(
                "  Code execution results: {}",
                summary.code_execution_result_count
            );

            if let Some(usage) = response.usage {
                println!("\n--- Token Usage ---");
                if let Some(input) = usage.total_input_tokens {
                    println!("  Input tokens: {input}");
                }
                if let Some(output) = usage.total_output_tokens {
                    println!("  Output tokens: {output}");
                }
            }
        }
        Err(e) => {
            match &e {
                GenaiError::Api {
                    status_code,
                    message,
                    request_id,
                    ..
                } => {
                    eprintln!("API Error (HTTP {}): {}", status_code, message);
                    if let Some(id) = request_id {
                        eprintln!("  Request ID: {}", id);
                    }
                    if message.contains("not supported") {
                        eprintln!("Note: Code execution may not be available in all regions.");
                    }
                }
                GenaiError::Http(http_err) => eprintln!("HTTP Error: {http_err}"),
                GenaiError::Json(json_err) => eprintln!("JSON Error: {json_err}"),
                _ => eprintln!("Error: {e}"),
            }
            return Err(e.into());
        }
    }

    println!("\n--- End Non-Streaming Response ---");

    // 9. Streaming example with Code Execution
    println!("\n=== Streaming with Code Execution ===\n");

    let stream_prompt = "Calculate the Fibonacci sequence up to 15 terms using Python.";
    println!("Prompt: {stream_prompt}\n");
    println!("Response (streaming):");

    let mut stream = client
        .interaction()
        .with_model(model_name)
        .with_text(stream_prompt)
        .with_code_execution()
        .create_stream();

    let mut final_response = None;

    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => match event.chunk {
                StreamChunk::StepDelta { delta, .. } => {
                    if let Some(text) = delta.as_text() {
                        print!("{}", text);
                        stdout().flush()?;
                    }
                }
                StreamChunk::Completed(response) => {
                    println!("\n");
                    final_response = Some(response);
                }
                _ => {} // Handle unknown variants
            },
            Err(e) => {
                eprintln!("\nStream error: {e}");
                break;
            }
        }
    }

    // Display code execution details from final response
    if let Some(output) = final_response
        .as_ref()
        .and_then(|r| r.successful_code_output())
    {
        println!("Code Output: {output}");
    }

    // =========================================================================
    // Summary
    // =========================================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Code Execution Demo Complete\n");

    println!("--- Key Takeaways ---");
    println!("• with_code_execution() enables server-side Python execution");
    println!("• response.code_execution_calls() shows executed code");
    println!("• response.code_execution_results() shows output/errors");
    println!("• Use result.is_error to check if execution failed\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("Non-streaming:");
    println!("  [REQ#1] POST with input + codeExecution tool");
    println!(
        "  [RES#1] completed: code_execution_call + code_execution_result + model_output steps\n"
    );
    println!("Streaming:");
    println!("  [REQ#2] POST streaming with input + codeExecution tool");
    println!("  [RES#2] SSE stream: step deltas (code + text) → completed with results\n");

    println!("--- Production Considerations ---");
    println!("• Code execution has a 30-second timeout (DeadlineExceeded)");
    println!("• Only Python is supported in the sandboxed environment");
    println!("• Use successful_code_output() helper for quick result access");
    println!("• Code execution may not be available in all regions");

    Ok(())
}
