//! Example: Video Input with Gemini
//!
//! This example demonstrates how to send video files to Gemini for analysis,
//! including scene description, object detection, and content understanding.
//!
//! Supported video formats: MP4, MPEG, MOV, AVI, FLV, MPG, WEBM, WMV, 3GP
//!
//! Run with: cargo run --example video_input

use genai_rs::{Client, Content, GenaiError};
use std::env;
use std::error::Error;

// A tiny valid MP4 (one 64x64 red H.264 frame, ~1.5KB) - for demonstration
// purposes only. The API requires real media data; in real usage, load actual
// video files with content.
const DEMO_MP4_BASE64: &str = "AAAAIGZ0eXBpc29tAAACAGlzb21pc28yYXZjMW1wNDEAAAMWbW9vdgAAAGxtdmhkAAAAAAAAAAAAAAAAAAAD6AAAAMgAAQAAAQAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAAAkB0cmFrAAAAXHRraGQAAAADAAAAAAAAAAAAAAABAAAAAAAAAMgAAAAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAABAAAAAAEAAAABAAAAAAAAkZWR0cwAAABxlbHN0AAAAAAAAAAEAAADIAAAAAAABAAAAAAG4bWRpYQAAACBtZGhkAAAAAAAAAAAAAAAAAAAoAAAACABVxAAAAAAALWhkbHIAAAAAAAAAAHZpZGUAAAAAAAAAAAAAAABWaWRlb0hhbmRsZXIAAAABY21pbmYAAAAUdm1oZAAAAAEAAAAAAAAAAAAAACRkaW5mAAAAHGRyZWYAAAAAAAAAAQAAAAx1cmwgAAAAAQAAASNzdGJsAAAAv3N0c2QAAAAAAAAAAQAAAK9hdmMxAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAAAAEAAQABIAAAASAAAAAAAAAABFUxhdmM2MC4zMS4xMDIgbGlieDI2NAAAAAAAAAAAAAAAGP//AAAANWF2Y0MBZAAK/+EAGGdkAAqs2UQmwEQAAAMABAAAAwAoPEiWWAEABmjr48siwP34+AAAAAAQcGFzcAAAAAEAAAABAAAAFGJ0cnQAAAAAAAByOAAAcjgAAAAYc3R0cwAAAAAAAAABAAAAAQAACAAAAAAcc3RzYwAAAAAAAAABAAAAAQAAAAEAAAABAAAAFHN0c3oAAAAAAAAC2wAAAAEAAAAUc3RjbwAAAAAAAAABAAADRgAAAGJ1ZHRhAAAAWm1ldGEAAAAAAAAAIWhkbHIAAAAAAAAAAG1kaXJhcHBsAAAAAAAAAAAAAAAALWlsc3QAAAAlqXRvbwAAAB1kYXRhAAAAAQAAAABMYXZmNjAuMTYuMTAwAAAACGZyZWUAAALjbWRhdAAAAq0GBf//qdxF6b3m2Ui3lizYINkj7u94MjY0IC0gY29yZSAxNjQgcjMxMDggMzFlMTlmOSAtIEguMjY0L01QRUctNCBBVkMgY29kZWMgLSBDb3B5bGVmdCAyMDAzLTIwMjMgLSBodHRwOi8vd3d3LnZpZGVvbGFuLm9yZy94MjY0Lmh0bWwgLSBvcHRpb25zOiBjYWJhYz0xIHJlZj0zIGRlYmxvY2s9MTowOjAgYW5hbHlzZT0weDM6MHgxMTMgbWU9aGV4IHN1Ym1lPTcgcHN5PTEgcHN5X3JkPTEuMDA6MC4wMCBtaXhlZF9yZWY9MSBtZV9yYW5nZT0xNiBjaHJvbWFfbWU9MSB0cmVsbGlzPTEgOHg4ZGN0PTEgY3FtPTAgZGVhZHpvbmU9MjEsMTEgZmFzdF9wc2tpcD0xIGNocm9tYV9xcF9vZmZzZXQ9LTIgdGhyZWFkcz0yIGxvb2thaGVhZF90aHJlYWRzPTEgc2xpY2VkX3RocmVhZHM9MCBucj0wIGRlY2ltYXRlPTEgaW50ZXJsYWNlZD0wIGJsdXJheV9jb21wYXQ9MCBjb25zdHJhaW5lZF9pbnRyYT0wIGJmcmFtZXM9MyBiX3B5cmFtaWQ9MiBiX2FkYXB0PTEgYl9iaWFzPTAgZGlyZWN0PTEgd2VpZ2h0Yj0xIG9wZW5fZ29wPTAgd2VpZ2h0cD0yIGtleWludD0yNTAga2V5aW50X21pbj01IHNjZW5lY3V0PTQwIGludHJhX3JlZnJlc2g9MCByY19sb29rYWhlYWQ9NDAgcmM9Y3JmIG1idHJlZT0xIGNyZj0yMy4wIHFjb21wPTAuNjAgcXBtaW49MCBxcG1heD02OSBxcHN0ZXA9NCBpcF9yYXRpbz0xLjQwIGFxPTE6MS4wMACAAAAAJmWIhAA///7mdfgU0wgaSTL8Q84/MVcp5wFs500OH1UoDGdRcGNv";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in environment");
    let client = Client::builder(api_key).build()?;
    let model_name = "gemini-3-flash-preview";

    // =========================================================================
    // Example 1: Basic Video Analysis (Fluent Builder Pattern)
    // =========================================================================
    println!("=== Example 1: Video Analysis ===\n");

    // Note: This uses a tiny one-frame MP4 clip for demonstration.
    // In real usage, you would use video_from_file() for automatic file loading.
    let response = client
        .interaction()
        .with_model(model_name)
        .with_content(vec![
            Content::text(
                "This is a demo video file. In real usage, describe what you see. \
                 If the video is empty or corrupted, just say 'No video content detected.'",
            ),
            Content::video_data(DEMO_MP4_BASE64, "video/mp4"),
        ])
        .create()
        .await;

    match response {
        Ok(r) => {
            if let Some(text) = r.as_text() {
                println!("Response: {text}\n");
            }
        }
        Err(e) => {
            println!("Note: Demo MP4 may not be processable: {e}\n");
        }
    }

    // =========================================================================
    // Example 2: Code Patterns for Video Analysis
    // =========================================================================
    println!("=== Example 2: Video Analysis Patterns ===\n");

    println!("Here are common patterns for working with video:\n");

    println!("1. SCENE DESCRIPTION:");
    println!(
        r#"
   // Using with_content() for multimodal input
   let response = client
       .interaction()
       .with_model("gemini-3-flash-preview")
       .with_content(vec![
           Content::text("Describe the key scenes in this video. What's happening?"),
           Content::video_data(&base64_video, "video/mp4"),
       ])
       .create()
       .await?;

   // Or load from file with automatic encoding:
   use genai_rs::video_from_file;
   let video = video_from_file("video.mp4").await?;
   let response = client
       .interaction()
       .with_model("gemini-3-flash-preview")
       .with_content(vec![
           Content::text("Describe the key scenes in this video."),
           video,
       ])
       .create()
       .await?;
"#
    );

    println!("2. OBJECT/PERSON DETECTION:");
    println!(
        r#"
   let response = client
       .interaction()
       .with_model("gemini-3-flash-preview")
       .with_content(vec![
           Content::text("List all the objects and people visible in this video.
               For each, note when they first appear (approximate timestamp)."),
           Content::video_data(&base64_video, "video/mp4"),
       ])
       .create()
       .await?;
"#
    );

    println!("3. ACTION RECOGNITION:");
    println!(
        r#"
   let response = client
       .interaction()
       .with_model("gemini-3-flash-preview")
       .with_content(vec![
           Content::text("What actions or activities are being performed in this video?
               Describe the sequence of events."),
           Content::video_data(&base64_video, "video/mp4"),
       ])
       .create()
       .await?;
"#
    );

    println!("4. VIDEO Q&A:");
    println!(
        r#"
   let response = client
       .interaction()
       .with_model("gemini-3-flash-preview")
       .with_content(vec![
           Content::text("How many people are in this video? What are they wearing?"),
           Content::video_data(&base64_video, "video/mp4"),
       ])
       .create()
       .await?;
"#
    );

    // =========================================================================
    // Example 3: Multi-turn Conversation about Video
    // =========================================================================
    println!("=== Example 3: Multi-turn Video Conversation ===\n");

    println!("Use stateful conversations for follow-up questions:\n");
    println!(
        r#"
   // First turn: Send video and get initial analysis
   let first = client
       .interaction()
       .with_model("gemini-3-flash-preview")
       .with_content(vec![
           Content::text("Describe what's happening in this video."),
           Content::video_data(&base64_video, "video/mp4"),
       ])
       .with_store_enabled()  // Enable conversation storage
       .create()
       .await?;

   // Second turn: Ask follow-up (video is remembered)
   let second = client
       .interaction()
       .with_model("gemini-3-flash-preview")
       .with_text("What happens at the 30-second mark?")
       .with_previous_interaction(&first.id)
       .create()
       .await?;

   // Third turn: More specific questions
   let third = client
       .interaction()
       .with_model("gemini-3-flash-preview")
       .with_text("What color is the car in the background?")
       .with_previous_interaction(&second.id)
       .create()
       .await?;
"#
    );

    // =========================================================================
    // Example 4: Combining Video with Audio Analysis
    // =========================================================================
    println!("=== Example 4: Video + Audio Analysis ===\n");

    println!("Gemini can analyze both video and audio tracks:\n");
    println!(
        r#"
   let response = client
       .interaction()
       .with_model("gemini-3-flash-preview")
       .with_content(vec![
           Content::text("Analyze this video:
               1. What is shown visually?
               2. What sounds or speech can you hear?
               3. How do the visuals and audio relate?"),
           Content::video_data(&base64_video, "video/mp4"),
       ])
       .create()
       .await?;
"#
    );

    // =========================================================================
    // Example 5: Error Handling
    // =========================================================================
    println!("=== Example 5: Error Handling ===\n");

    // Demonstrate error handling with invalid video
    let invalid_base64 = "not_valid_video_data_at_all";

    match client
        .interaction()
        .with_model(model_name)
        .with_content(vec![
            Content::text("Describe this video."),
            Content::video_data(invalid_base64, "video/mp4"),
        ])
        .create()
        .await
    {
        Ok(response) => {
            if let Some(text) = response.as_text() {
                println!("Response: {text}\n");
            }
        }
        Err(e) => match &e {
            GenaiError::Api {
                status_code,
                message,
                ..
            } => {
                println!("API error for invalid video:");
                println!("  Status: {status_code}");
                println!("  Message: {message}\n");
            }
            _ => println!("Error: {e}\n"),
        },
    }

    // =========================================================================
    // Reference: Supported Video Formats
    // =========================================================================
    println!("=== Supported Video Formats ===\n");
    println!("Gemini supports these video formats:");
    println!("  - MP4  (video/mp4)");
    println!("  - MPEG (video/mpeg)");
    println!("  - MOV  (video/mov, video/quicktime)");
    println!("  - AVI  (video/avi, video/x-msvideo)");
    println!("  - FLV  (video/x-flv)");
    println!("  - MPG  (video/mpg)");
    println!("  - WEBM (video/webm)");
    println!("  - WMV  (video/wmv)");
    println!("  - 3GP  (video/3gpp)");
    println!();
    println!("Maximum video length: ~1 hour");
    println!("Maximum file size (base64): ~20MB recommended");
    println!("For larger files (20MB-2GB): Use Files API (not yet implemented in this SDK)\n");

    // =========================================================================
    // Reference: Loading Video Files
    // =========================================================================
    println!("=== Loading Video Files ===\n");
    println!("Option 1: Use the built-in file loading helper (recommended):\n");
    println!(
        r#"
   use genai_rs::video_from_file;

   // Load video file with automatic MIME detection and base64 encoding
   let video_content = video_from_file("path/to/video.mp4").await?;

   // Build the request using with_content
   let response = client
       .interaction()
       .with_model("gemini-3-flash-preview")
       .with_content(vec![
           Content::text("Describe what's happening in this video."),
           video_content,
       ])
       .create()
       .await?;
"#
    );

    println!("Option 2: Manual file loading and encoding:\n");
    println!(
        r#"
   use std::fs;
   use base64::Engine;

   // Read and encode
   let video_bytes = fs::read("path/to/video.mp4")?;
   let base64_video = base64::engine::general_purpose::STANDARD.encode(&video_bytes);

   // Send with with_content
   let response = client
       .interaction()
       .with_model("gemini-3-flash-preview")
       .with_content(vec![
           Content::text("Describe what's happening in this video."),
           Content::video_data(&base64_video, "video/mp4"),
       ])
       .create()
       .await?;
"#
    );

    println!("Note: For large videos (>20MB), use the Files API to upload first,");
    println!("then reference by URI. This avoids base64 encoding overhead.\n");

    // =========================================================================
    // Summary
    // =========================================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Video Input Demo Complete\n");

    println!("--- Key Takeaways ---");
    println!("• Content::video_data(base64, mime_type) for inline video content");
    println!("• video_from_file() helper loads and encodes files automatically");
    println!("• with_content() accepts mixed Content::text() + Content::video_data()");
    println!("• Model analyzes both visual content and audio track\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("  [REQ#1] POST with text + inlineData (video base64 truncated)");
    println!("  [RES#1] completed: scene description or analysis\n");
    println!("Multi-turn:");
    println!("  [REQ#2] POST with text + previousInteractionId");
    println!("  [RES#2] completed: timestamp-specific questions answered\n");

    println!("--- Production Considerations ---");
    println!("• Supports MP4, MPEG, MOV, AVI, FLV, WEBM, WMV, 3GP");
    println!("• Maximum video length: ~1 hour");
    println!("• For files >20MB, use Files API (upload_file)");
    println!("• Base64 encoding adds ~33% overhead");

    Ok(())
}
