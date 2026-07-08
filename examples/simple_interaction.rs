use genai_rs::{Client, GenaiError};
use std::env;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // 1. Get API Key from environment variable
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in environment");

    // Create the client
    let client = Client::builder(api_key).build()?;

    // 2. Create an interaction using the builder pattern
    let model_name = "gemini-3-flash-preview";
    let prompt = "Explain the concept of recursion in programming in one paragraph.";

    println!("Creating interaction with model: {model_name}");
    println!("Prompt: {prompt}\n");

    // 3. Send the interaction request using the fluent builder API
    match client
        .interaction()
        .with_model(model_name)
        .with_text(prompt)
        .with_store_enabled() // Store for potential follow-up
        .create()
        .await
    {
        Ok(response) => {
            println!("--- Interaction Response ---");
            println!("Interaction ID: {:?}", response.id);
            println!("Status: {:?}", response.status);

            if !response.steps.is_empty() {
                println!("\nModel Output:");
                for step in &response.steps {
                    match step {
                        genai_rs::Step::ModelOutput { .. } => {
                            if let Some(t) = step.as_text() {
                                println!("{t}");
                            }
                        }
                        genai_rs::Step::Thought {
                            signature: Some(_), ..
                        } => {
                            println!("[Thought] (signature present)");
                        }
                        _ => {}
                    }
                }
            }

            if let Some(usage) = response.usage {
                println!("\nToken Usage:");
                if let Some(total) = usage.total_tokens {
                    println!("  Total tokens: {total}");
                }
            }
            println!("--- End Response ---");

            // Summary
            println!(
                "\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            );
            println!("✅ Simple Interaction Demo Complete\n");

            println!("--- Key Takeaways ---");
            println!("• Client::builder(api_key).build()? creates the API client");
            println!("• client.interaction().with_model().with_text().create() sends a request");
            println!("• response.as_text() extracts the model's text output");
            println!("• with_store_enabled() saves the interaction for potential follow-ups\n");

            println!("--- What You'll See with LOUD_WIRE=1 ---");
            println!("  [REQ#1] POST with input text + model + store:true");
            println!("  [RES#1] completed: text response with usage stats\n");

            println!("--- Production Considerations ---");
            println!("• Handle all GenaiError variants for robust error handling");
            println!("• Monitor token usage for cost tracking");
            println!("• Use with_store_enabled() only when follow-up turns are needed");
            println!("• Consider implementing retry logic for transient API errors");
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
                }
                GenaiError::Http(http_err) => eprintln!("HTTP Error: {http_err}"),
                GenaiError::Json(json_err) => eprintln!("JSON Error: {json_err}"),
                GenaiError::Parse(p_err) => eprintln!("Parse Error: {p_err}"),
                GenaiError::Utf8(u_err) => eprintln!("UTF8 Error: {u_err}"),
                GenaiError::Internal(i_err) => eprintln!("Internal Error: {i_err}"),
                GenaiError::InvalidInput(input_err) => eprintln!("Invalid Input: {input_err}"),
                GenaiError::MalformedResponse(msg) => eprintln!("Malformed Response: {msg}"),
                // Wildcard arm required for #[non_exhaustive] forward compatibility
                _ => eprintln!("Error: {e}"),
            }
            return Err(e.into());
        }
    }

    Ok(())
}
