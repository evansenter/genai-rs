//! Multimodal Image Input Example
//!
//! This example demonstrates sending images to the Gemini API for analysis
//! using base64-encoded image data. It shows the fluent builder pattern,
//! manual content vector approach, and resolution control for managing
//! token costs vs. image quality trade-offs.
//!
//! # Running
//!
//! ```bash
//! cargo run --example multimodal_image
//! ```
//!
//! # Prerequisites
//!
//! Set the `GEMINI_API_KEY` environment variable with your API key.

use genai_rs::{Client, Content, Resolution};
use std::env;

// A tiny red PNG image (1x1 pixel) encoded as base64
const TINY_RED_PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";

// A tiny blue PNG image (1x1 pixel) encoded as base64
const TINY_BLUE_PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get API key from environment
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY environment variable not set");

    let client = Client::builder(api_key).build()?;

    println!("=== MULTIMODAL IMAGE INPUT EXAMPLE ===\n");

    // Method 1: Fluent builder pattern with Content::image_data()
    // This is the most ergonomic approach for inline multimodal content
    println!("Sending image to Gemini for analysis...\n");

    let response = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_content(vec![
            Content::text("What color is this image? Describe it."),
            Content::image_data(TINY_RED_PNG_BASE64, "image/png"),
        ])
        .with_store_enabled()
        .create()
        .await?;

    println!("--- Response ---");
    println!("Status: {:?}", response.status);
    println!();

    if let Some(text) = response.as_text() {
        println!("Image Description:");
        println!("{}", text);
    }

    if let Some(usage) = response.usage {
        println!();
        if let Some(total) = usage.total_tokens {
            println!("Total tokens: {}", total);
        }
    }

    println!("\n--- End ---");

    // Method 2: Using with_content() for multiple items
    // Useful when building content programmatically
    println!("\n=== IMAGE COMPARISON ===\n");

    let comparison_contents = vec![
        Content::text("Compare these two colored images. What are their colors?"),
        Content::image_data(TINY_RED_PNG_BASE64, "image/png"),
        Content::image_data(TINY_BLUE_PNG_BASE64, "image/png"),
    ];

    let comparison = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_content(comparison_contents)
        .with_store_enabled()
        .create()
        .await?;

    if let Some(text) = comparison.as_text() {
        println!("Comparison: {}", text);
    }

    // Method 3: Resolution control for cost vs. quality trade-off
    // Use Low resolution for simple tasks (fewer tokens = lower cost)
    // Use High/UltraHigh for detailed analysis
    println!("\n=== RESOLUTION CONTROL ===\n");

    // Low resolution - good for simple color/shape detection, costs fewer tokens
    let low_res_response = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_content(vec![
            Content::text("What color is this image?"),
            Content::image_data(TINY_RED_PNG_BASE64, "image/png").with_resolution(Resolution::Low),
        ])
        .create()
        .await?;

    println!("Low resolution analysis:");
    if let Some(text) = low_res_response.as_text() {
        println!("  {}", text);
    }
    if let Some(usage) = &low_res_response.usage {
        println!("  Tokens used: {:?}", usage.total_tokens);
    }

    // High resolution - for detailed analysis of complex images
    let high_res_response = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_content(vec![
            Content::text("What color is this image?"),
            Content::image_data(TINY_RED_PNG_BASE64, "image/png").with_resolution(Resolution::High),
        ])
        .create()
        .await?;

    println!("\nHigh resolution analysis:");
    if let Some(text) = high_res_response.as_text() {
        println!("  {}", text);
    }
    if let Some(usage) = &high_res_response.usage {
        println!("  Tokens used: {:?}", usage.total_tokens);
    }

    // Using chained builder with resolution
    println!("\nUsing Content::image_data().with_resolution() builder:");
    let helper_response = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_content(vec![
            Content::text("Describe this image briefly."),
            Content::image_data(TINY_BLUE_PNG_BASE64, "image/png")
                .with_resolution(Resolution::Medium), // Default balance of cost and quality
        ])
        .create()
        .await?;

    if let Some(text) = helper_response.as_text() {
        println!("  {}", text);
    }

    // Demonstrate a follow-up question using conversation context
    println!("\n=== FOLLOW-UP QUESTION ===\n");
    println!("User: Which of those colors is warmer?\n");

    let follow_up = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_previous_interaction(
            comparison
                .id
                .as_ref()
                .expect("id should exist when store=true"),
        )
        .with_text("Which of those colors is warmer?")
        .with_store_enabled()
        .create()
        .await?;

    if let Some(text) = follow_up.as_text() {
        println!("Assistant: {}", text);
    }

    // =========================================================================
    // Summary
    // =========================================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Multimodal Image Input Demo Complete\n");

    println!("--- Key Takeaways ---");
    println!("• Content::image_data(base64, mime_type) for inline image content");
    println!("• .with_resolution(Resolution::*) for quality vs. cost control");
    println!("• Resolution levels: Low, Medium (default), High, UltraHigh");
    println!("• with_content() accepts mixed Content::text() + Content::image_data() vectors");
    println!("• Follow-up questions work with with_previous_interaction()\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("Single image:");
    println!("  [REQ#1] POST with text + inlineData (base64 truncated in logs)");
    println!("  [RES#1] completed: text describing the image\n");
    println!("Multiple images:");
    println!("  [REQ#2] POST with text + 2x inlineData");
    println!("  [RES#2] completed: comparison of both images\n");
    println!("Resolution control:");
    println!("  [REQ#3-5] POST with text + inlineData + resolution field");
    println!("  Note: resolution field appears in wire format when set\n");
    println!("Follow-up:");
    println!("  [REQ#6] POST with text + previousInteractionId");
    println!("  [RES#6] completed: answer using image context\n");

    println!("--- Production Considerations ---");
    println!("• Resolution::Low uses fewer tokens - ideal for simple tasks");
    println!("• Resolution::High/UltraHigh for detailed image analysis");
    println!("• Default (no resolution) or Medium balances cost and quality");
    println!("• Base64 encoding increases payload size ~33%");
    println!("• For large/repeated files, use Files API instead (upload_file)");
    println!("• MIME type must match actual image format");
    println!("• Model supports PNG, JPEG, GIF, WebP images");

    Ok(())
}
