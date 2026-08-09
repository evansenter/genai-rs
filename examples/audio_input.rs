//! Example: Audio Input with Gemini
//!
//! This example demonstrates how to send audio files to Gemini for transcription,
//! analysis, and question-answering.
//!
//! Supported audio formats: WAV, MP3, AIFF, AAC, OGG, FLAC
//!
//! Run with: cargo run --example audio_input

use genai_rs::{Client, Content, GenaiError, TranscriptionConfig};
use std::env;
use std::error::Error;

// A tiny valid WAV clip (100 frames of 16-bit mono silence) - for demonstration
// purposes only. The API requires a non-empty data chunk; in real usage, load
// actual audio files with content.
const DEMO_WAV_BASE64: &str = "UklGRuwAAABXQVZFZm10IBAAAAABAAEARKwAAIhYAQACABAAZGF0YcgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in environment");
    let client = Client::builder(api_key).build()?;
    let model_name = "gemini-3-flash-preview";

    // =========================================================================
    // Example 1: Basic Audio Transcription (Fluent Builder Pattern)
    // =========================================================================
    println!("=== Example 1: Audio Transcription ===\n");

    // Note: This uses a tiny silent WAV clip for demonstration.
    // In real usage, you would provide actual audio content.
    // Using with_content() with Content constructors.
    //
    // TranscriptionConfig tunes speech recognition: BCP-47 language hints
    // (omit for auto-detect), "speaker" diarization, and "word"-level
    // timestamps (the SDK-documented value sets — see the field docs).
    let response = client
        .interaction()
        .with_model(model_name)
        .with_content(vec![
            Content::text(
                "This is a short demo audio clip. Describe what you hear. \
                 If it is silent, just say 'Silent audio.'",
            ),
            Content::audio_data(DEMO_WAV_BASE64, "audio/wav"),
        ])
        .with_transcription_config(TranscriptionConfig {
            language_codes: Some(vec!["en-US".to_string()]),
            diarization_mode: Some("speaker".to_string()),
            timestamp_granularities: Some(vec!["word".to_string()]),
            ..Default::default()
        })
        .create()
        .await;

    // The demo fixture is known-valid (drift-guarded by tests), so a
    // non-transient failure is a real error — surface it. Transient blips
    // (per GenaiError::is_retryable) shouldn't hide the rest of the demo.
    match response {
        Ok(r) => {
            if let Some(text) = r.as_text() {
                println!("Response: {text}\n");
            }
        }
        Err(e) if e.is_retryable() => {
            println!("Note: transient API error, continuing demo: {e}\n");
        }
        Err(e) => return Err(e.into()),
    }

    // =========================================================================
    // Example 2: Code Patterns for Audio Analysis
    // =========================================================================
    println!("=== Example 2: Audio Analysis Patterns ===\n");

    println!("Here are common patterns for working with audio:\n");

    println!("1. TRANSCRIPTION:");
    println!(
        r#"
   use genai_rs::Content;

   let response = client
       .interaction()
       .with_model("gemini-3-flash-preview")
       .with_content(vec![
           Content::text("Transcribe this audio with proper punctuation."),
           Content::audio_data(&base64_audio, "audio/mp3"),
       ])
       .create()
       .await?;
"#
    );

    println!("2. SPEAKER ANALYSIS:");
    println!(
        r#"
   let response = client
       .interaction()
       .with_model("gemini-3-flash-preview")
       .with_content(vec![
           Content::text("Analyze this audio:
               - How many speakers are there?
               - What language(s) are spoken?
               - What is the emotional tone?"),
           Content::audio_data(&base64_audio, "audio/mp3"),
       ])
       .create()
       .await?;
"#
    );

    println!("3. CONTENT Q&A:");
    println!(
        r#"
   let response = client
       .interaction()
       .with_model("gemini-3-flash-preview")
       .with_content(vec![
           Content::text("In this podcast, what are the main topics discussed?"),
           Content::audio_data(&podcast_audio, "audio/mp3"),
       ])
       .create()
       .await?;
"#
    );

    // =========================================================================
    // Example 3: Multi-turn Conversation about Audio
    // =========================================================================
    println!("=== Example 3: Multi-turn Audio Conversation ===\n");

    println!("Use stateful conversations for follow-up questions:\n");
    println!(
        r#"
   // First turn: Send audio and get initial analysis
   let first = client
       .interaction()
       .with_model("gemini-3-flash-preview")
       .with_content(vec![
           Content::text("Summarize this audio recording."),
           Content::audio_data(&base64_audio, "audio/mp3"),
       ])
       .with_store_enabled()  // Enable conversation storage
       .create()
       .await?;

   // Second turn: Ask follow-up (audio is remembered)
   let second = client
       .interaction()
       .with_model("gemini-3-flash-preview")
       .with_text("What emotions did you detect in the speaker's voice?")
       .with_previous_interaction(&first.id)
       .create()
       .await?;
"#
    );

    // =========================================================================
    // Example 4: Error Handling
    // =========================================================================
    println!("=== Example 4: Error Handling ===\n");

    // Demonstrate error handling with invalid audio
    let invalid_base64 = "not_valid_audio_data_at_all";

    match client
        .interaction()
        .with_model(model_name)
        .with_content(vec![
            Content::text("Transcribe this audio."),
            Content::audio_data(invalid_base64, "audio/mp3"),
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
                println!("API error for invalid audio:");
                println!("  Status: {status_code}");
                println!("  Message: {message}\n");
            }
            _ => println!("Error: {e}\n"),
        },
    }

    // =========================================================================
    // Reference: Supported Audio Formats
    // =========================================================================
    println!("=== Supported Audio Formats ===\n");
    println!("Gemini supports these audio formats:");
    println!("  - WAV  (audio/wav)");
    println!("  - MP3  (audio/mp3, audio/mpeg)");
    println!("  - AIFF (audio/aiff)");
    println!("  - AAC  (audio/aac)");
    println!("  - OGG  (audio/ogg)");
    println!("  - FLAC (audio/flac)");
    println!();
    println!("Maximum audio length: ~9.5 hours");
    println!("For files larger than 20MB, use the Files API (not yet implemented).\n");

    // =========================================================================
    // Reference: Loading Audio Files
    // =========================================================================
    println!("=== Loading Audio Files ===\n");
    println!("Option 1: Use the built-in file loading helper (recommended):\n");
    println!(
        r#"
   use genai_rs::{{audio_from_file, Content}};

   // Load audio file with automatic MIME detection and base64 encoding
   let audio_content = audio_from_file("path/to/audio.mp3").await?;

   // Build the request using with_content
   let response = client
       .interaction()
       .with_model("gemini-3-flash-preview")
       .with_content(vec![
           Content::text("Transcribe this audio."),
           audio_content,
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
   use genai_rs::Content;

   // Read and encode
   let audio_bytes = fs::read("path/to/audio.mp3")?;
   let base64_audio = base64::engine::general_purpose::STANDARD.encode(&audio_bytes);

   // Send with with_content
   let response = client
       .interaction()
       .with_model("gemini-3-flash-preview")
       .with_content(vec![
           Content::text("Transcribe this audio."),
           Content::audio_data(&base64_audio, "audio/mp3"),
       ])
       .create()
       .await?;
"#
    );

    // =========================================================================
    // Summary
    // =========================================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Audio Input Demo Complete\n");

    println!("--- Key Takeaways ---");
    println!("• Content::audio_data(base64, mime_type) for inline audio content");
    println!("• audio_from_file(path) helper loads and encodes files automatically");
    println!("• Use with_content(vec![...]) to combine text and audio");
    println!("• Multi-turn conversations remember audio context\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!(
        "  [REQ#1] POST with text + inlineData (audio base64 truncated) + \
         generation_config.transcription_config"
    );
    println!("  [RES#1] completed: transcription or analysis\n");
    println!("Multi-turn:");
    println!("  [REQ#2] POST with text + previousInteractionId");
    println!("  [RES#2] completed: follow-up using audio context\n");

    println!("--- Production Considerations ---");
    println!("• Supports WAV, MP3, AIFF, AAC, OGG, FLAC formats");
    println!("• Maximum audio length: ~9.5 hours");
    println!("• For files >20MB, use Files API (upload_file)");
    println!("• MIME type must match actual audio format");
    println!("• TranscriptionConfig's diarization_mode/timestamp_granularities are");
    println!("  open strings with one documented value each today (\"speaker\", \"word\")");

    Ok(())
}
