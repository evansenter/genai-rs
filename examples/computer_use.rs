//! Example demonstrating Computer Use (browser automation) capability.
//!
//! This example shows how to enable the Computer Use tool for browser automation tasks.
//!
//! **Security Warning**: Computer Use allows the model to control a browser environment.
//! Always review excluded functions carefully and avoid exposing to untrusted input.
//!
//! Note: This feature may require specific model versions or API access.
//! If you receive an error, verify that computer use is available for your account.
//!
//! Run with: cargo run --example computer_use

use genai_rs::{Client, ComputerUseConfig, GenaiError};
use std::env;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // 1. Get API Key from environment variable
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in environment");

    // Create the client
    let client = Client::builder(api_key).build()?;

    let model_name = "gemini-3-flash-preview";

    // 2. Basic Computer Use - Enable browser automation
    println!("=== Computer Use: Basic Browser Automation ===\n");

    let prompt = "Navigate to example.com and describe what you see on the page.";
    println!("Prompt: {prompt}\n");

    match client
        .interaction()
        .with_model(model_name)
        .with_text(prompt)
        .add_tool(ComputerUseConfig::new()) // Enable browser automation
        .create()
        .await
    {
        Ok(response) => {
            println!("Status: {:?}", response.status);

            // Check for computer use calls in the response
            for content in &response.outputs {
                if content.is_computer_use_call() {
                    println!("\nComputer Use Call detected:");
                    println!("  Content: {:?}", content);
                }
                if content.is_computer_use_result() {
                    println!("\nComputer Use Result:");
                    println!("  Content: {:?}", content);
                }
            }

            // Display the model's response
            if let Some(text) = response.as_text() {
                println!("\nModel Response:");
                println!("{text}");
            }
        }
        Err(e) => {
            handle_error(&e)?;
        }
    }

    // 3. Computer Use with Exclusions - Restrict dangerous actions
    println!("\n=== Computer Use: With Excluded Functions ===\n");

    let prompt2 = "Check the current weather on weather.gov for Washington DC.";
    println!("Prompt: {prompt2}\n");
    println!("Excluded functions: submit_form, download\n");

    match client
        .interaction()
        .with_model(model_name)
        .with_text(prompt2)
        .add_tool(
            ComputerUseConfig::new()
                .excluding(vec!["submit_form".to_string(), "download".to_string()]),
        )
        .create()
        .await
    {
        Ok(response) => {
            println!("Status: {:?}", response.status);

            // Display summary of response contents
            let summary = response.content_summary();
            println!("Content summary: {}", summary);

            // Display the model's response
            if let Some(text) = response.as_text() {
                println!("\nModel Response:");
                println!("{text}");
            }
        }
        Err(e) => {
            handle_error(&e)?;
        }
    }

    // =========================================================================
    // Summary
    // =========================================================================
    println!("\n=== Computer Use Demo Complete ===\n");

    println!("--- Key Takeaways ---");
    println!("  add_tool(ComputerUseConfig::new()) enables server-side browser automation");
    println!("  ComputerUseConfig::new().excluding(...) restricts specific browser actions");
    println!("  is_computer_use_call() checks if model requested a browser action");
    println!("  is_computer_use_result() checks for action results");
    println!("  content_summary() shows computer_use_call/result counts\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("  [REQ#1] POST with input + computerUse tool");
    println!("  [RES#1] completed: computer_use_call → computer_use_result → text");
    println!("  [REQ#2] POST with input + computerUse (excludedPredefinedFunctions)");
    println!("  [RES#2] completed: actions within allowed functions\n");

    println!("--- Production Considerations ---");
    println!("  SECURITY: Review all browser actions before execution");
    println!("  SECURITY: Use ComputerUseConfig::new().excluding() to block dangerous actions");
    println!("  SECURITY: Never expose computer use to untrusted user input");
    println!("  AUDIT: Log all computer use activities for compliance");
    println!("  AVAILABILITY: Feature may require specific model/account access");

    Ok(())
}

fn handle_error(e: &GenaiError) -> Result<(), Box<dyn std::error::Error>> {
    match e {
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
            if message.contains("not supported") || message.contains("not available") {
                eprintln!("\nNote: Computer Use may not be available for this model or account.");
                eprintln!("Check your API access level and model availability.");
            }
        }
        GenaiError::Http(http_err) => eprintln!("HTTP Error: {http_err}"),
        _ => eprintln!("Error: {e}"),
    }
    // Return Ok since feature may not be available yet
    Ok(())
}
