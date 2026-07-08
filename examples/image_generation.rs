//! Example: Image Generation
//!
//! This example demonstrates how to generate images using Gemini's
//! image generation capabilities and saves them to disk.
//!
//! # Running
//!
//! ```bash
//! cargo run --example image_generation
//! ```
//!
//! # Prerequisites
//!
//! Set the `GEMINI_API_KEY` environment variable with your API key.
//!
//! # Model Support
//!
//! Image generation requires a model that supports the "image" response modality.
//! Currently supported: `gemini-3-pro-image-preview`
//!
//! # Regional Availability
//!
//! Image generation may not be available in all regions. If you receive
//! a "model not found" error, your API key may not have access to the
//! image generation model in your region.

use genai_rs::{Client, GenaiError, InteractionStatus};
use std::env;
use std::path::PathBuf;

/// Save image bytes to a file and return the path
fn save_image(
    bytes: &[u8],
    extension: &str,
    prefix: &str,
    index: usize,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let filename = format!("{}_{}.{}", prefix, index, extension);
    let path = std::env::temp_dir().join(filename);

    std::fs::write(&path, bytes)?;

    Ok(path)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get API key from environment
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY environment variable not set");

    let client = Client::builder(api_key).build()?;

    println!("=== IMAGE GENERATION EXAMPLE ===\n");

    // ==========================================================================
    // Example 1: Basic Image Generation (using new convenience API)
    // ==========================================================================
    println!("--- Example 1: Birman Cat ---\n");

    let model = "gemini-3-pro-image-preview";
    let prompt = "A white and orange Birman cat sitting cozily on an electric blanket on a couch.";

    println!("Model: {}", model);
    println!("Prompt: {}\n", prompt);

    let result = client
        .interaction()
        .with_model(model)
        .with_text(prompt)
        .with_image_output() // New convenience method!
        .with_store_enabled()
        .create()
        .await;

    match result {
        Ok(response) => {
            println!("Status: {:?}", response.status);

            if response.status == InteractionStatus::Completed {
                // Extract generated image
                if let Some(bytes) = response.first_image_bytes()? {
                    let path = save_image(&bytes, "png", "birman_cat", 1)?;
                    println!("\n  Image saved!");
                    println!("  Size: {} bytes", bytes.len());
                    println!("  Path: {}", path.display());
                    println!("  Open: file://{}", path.display());
                } else {
                    println!("\nNo image content in response.");
                }
            }

            if let Some(usage) = &response.usage {
                println!(
                    "\n  Tokens: {} in / {} out",
                    usage.total_input_tokens.unwrap_or(0),
                    usage.total_output_tokens.unwrap_or(0)
                );
            }
        }
        Err(e) => {
            handle_image_generation_error(&e);
        }
    }

    // ==========================================================================
    // Example 2: Using the images() iterator (for multiple images or metadata access)
    // ==========================================================================
    println!("\n--- Example 2: Watercolor Birman with Motorcycle ---\n");

    let prompt = "A watercolor painting of a white and orange Birman cat standing by a Triumph cafe racer motorcycle on a scenic road.";

    println!("Prompt: {}\n", prompt);

    let result = client
        .interaction()
        .with_model(model)
        .with_text(prompt)
        .with_image_output() // New convenience method!
        .with_store_enabled()
        .create()
        .await;

    match result {
        Ok(response) => {
            println!("Status: {:?}", response.status);

            if response.status == InteractionStatus::Completed {
                // Using images() iterator provides access to MIME type and extension
                let mut image_count = 0;
                for image in response.images() {
                    image_count += 1;
                    let bytes = image.bytes()?;
                    let path = save_image(
                        &bytes,
                        image.extension(),
                        "watercolor_birman_motorcycle",
                        image_count,
                    )?;
                    println!("\n  Image {} saved!", image_count);
                    println!("  Size: {} bytes", bytes.len());
                    println!("  MIME: {:?}", image.mime_type());
                    println!("  Path: {}", path.display());
                    println!("  Open: file://{}", path.display());
                }

                if image_count == 0 {
                    println!("\nNo image content in response.");
                }
            }

            if let Some(usage) = &response.usage {
                println!(
                    "\n  Tokens: {} in / {} out",
                    usage.total_input_tokens.unwrap_or(0),
                    usage.total_output_tokens.unwrap_or(0)
                );
            }
        }
        Err(e) => {
            handle_image_generation_error(&e);
        }
    }

    // =========================================================================
    // Summary
    // =========================================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Image Generation Demo Complete\n");

    println!("--- Key Takeaways ---");
    println!("• with_image_output() sets response modality to \"image\"");
    println!("• Requires gemini-3-pro-image-preview model");
    println!("• response.first_image_bytes() extracts the first image");
    println!("• response.images() iterator for multiple images with metadata\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("  [REQ#1] POST with input + response_modalities:[\"image\"]");
    println!("  [RES#1] completed: base64-encoded image data (truncated in logs)\n");
    println!("  [REQ#2] POST with input + response_modalities:[\"image\"]");
    println!("  [RES#2] completed: base64-encoded image data\n");

    println!("--- Production Considerations ---");
    println!("• Image generation may not be available in all regions");
    println!("• Images are returned as base64 - decode and save to disk");
    println!("• Check response.status == Completed before extracting images");
    println!("• MIME type is typically image/png or image/jpeg");

    Ok(())
}

/// Handle common image generation errors with helpful messages
fn handle_image_generation_error(e: &GenaiError) {
    match e {
        GenaiError::Api {
            status_code,
            message,
            ..
        } => {
            eprintln!("API Error (HTTP {}): {}", status_code, message);

            if message.contains("not found") || message.contains("not supported") {
                eprintln!(
                    "\nNote: Image generation requires the gemini-3-pro-image-preview model."
                );
                eprintln!("This model may not be available in all regions or API configurations.");
            }
        }
        GenaiError::Http(http_err) => {
            eprintln!("HTTP Error: {}", http_err);
        }
        GenaiError::Json(json_err) => {
            eprintln!("JSON Error: {}", json_err);
        }
        _ => {
            eprintln!("Error: {}", e);
        }
    }
}
