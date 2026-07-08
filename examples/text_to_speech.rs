//! Example: Text-to-Speech with Gemini
//!
//! This example demonstrates how to use Gemini's text-to-speech capabilities
//! to convert text into spoken audio.
//!
//! Run with: cargo run --example text_to_speech

use genai_rs::{Client, SpeechConfig};
use std::env;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in environment");
    let client = Client::builder(api_key).build()?;

    // TTS model - use the appropriate model for text-to-speech
    let model_name = "gemini-2.5-pro-preview-tts";

    // =========================================================================
    // Example 1: Basic Text-to-Speech
    // =========================================================================
    println!("=== Example 1: Basic Text-to-Speech ===\n");

    let response = client
        .interaction()
        .with_model(model_name)
        .with_text("Hello! This is a demonstration of text-to-speech capabilities.")
        .with_audio_output()
        .with_voice("Kore")
        .create()
        .await?;

    if let Some(audio) = response.first_audio() {
        let bytes = audio.bytes()?;
        let filename = format!("output_basic.{}", audio.extension());
        std::fs::write(&filename, &bytes)?;
        println!("Saved audio ({} bytes) to: {}", bytes.len(), filename);
        println!("MIME type: {:?}", audio.mime_type());
        // The API reports PCM parameters alongside the audio data
        if let Some(rate) = audio.sample_rate() {
            println!("Sample rate: {} Hz", rate);
        }
        if let Some(channels) = audio.channels() {
            println!("Channels: {}", channels);
        }
        println!();
    } else {
        println!("No audio in response\n");
    }

    // =========================================================================
    // Example 2: Using SpeechConfig for Full Control
    // =========================================================================
    println!("=== Example 2: SpeechConfig with Voice and Language ===\n");

    let speech_config = SpeechConfig {
        voice: Some("Puck".to_string()),
        language: Some("en-US".to_string()),
        speaker: None,
    };

    let response = client
        .interaction()
        .with_model(model_name)
        .with_text("Welcome to the future of AI-powered speech synthesis!")
        .with_audio_output()
        .with_speech_config(speech_config)
        .create()
        .await?;

    if let Some(audio) = response.first_audio() {
        let bytes = audio.bytes()?;
        let filename = format!("output_puck.{}", audio.extension());
        std::fs::write(&filename, &bytes)?;
        println!("Saved audio ({} bytes) to: {}\n", bytes.len(), filename);
    }

    // =========================================================================
    // Example 3: Using Convenience Constructors
    // =========================================================================
    println!("=== Example 3: SpeechConfig Constructors ===\n");

    // Simple voice-only config
    let config1 = SpeechConfig::with_voice("Aoede");
    println!("Voice-only config: {:?}", config1);

    // Voice with language
    let config2 = SpeechConfig::with_voice_and_language("Charon", "en-GB");
    println!("Voice + language config: {:?}\n", config2);

    // =========================================================================
    // Example 4: Processing Multiple Audio Outputs
    // =========================================================================
    println!("=== Example 4: Iterating Over Audio Outputs ===\n");

    let response = client
        .interaction()
        .with_model(model_name)
        .with_text("One. Two. Three.")
        .with_audio_output()
        .with_voice("Fenrir")
        .create()
        .await?;

    println!("Response has audio: {}", response.has_audio());

    for (i, audio) in response.audios().enumerate() {
        let bytes = audio.bytes()?;
        println!(
            "  Audio {}: {} bytes, MIME: {:?}, extension: {}, sample_rate: {:?}, channels: {:?}",
            i,
            bytes.len(),
            audio.mime_type(),
            audio.extension(),
            audio.sample_rate(),
            audio.channels()
        );
    }
    println!();

    // =========================================================================
    // Reference: Available Voices
    // =========================================================================
    println!("=== Available Voices ===\n");
    println!("Common voices include:");
    println!("  - Aoede   - Warm and friendly");
    println!("  - Charon  - Deep and authoritative");
    println!("  - Fenrir  - Clear and professional");
    println!("  - Kore    - Bright and energetic");
    println!("  - Puck    - Playful and expressive");
    println!();
    println!("See Google's TTS documentation for the complete list of voices.");
    println!();

    // =========================================================================
    // Reference: Code Patterns
    // =========================================================================
    println!("=== Code Patterns ===\n");

    println!("1. SIMPLE TTS (most common):");
    println!(
        r#"
   let response = client
       .interaction()
       .with_model("gemini-2.5-pro-preview-tts")
       .with_text("Your text here")
       .with_audio_output()
       .with_voice("Kore")
       .create()
       .await?;

   if let Some(audio) = response.first_audio() {{
       std::fs::write("output.wav", audio.bytes()?)?;
   }}
"#
    );

    println!("2. FULL SPEECH CONFIG:");
    println!(
        r#"
   let config = SpeechConfig {{
       voice: Some("Puck".to_string()),
       language: Some("en-US".to_string()),
       speaker: None,  // For multi-speaker scenarios
   }};

   let response = client
       .interaction()
       .with_model("gemini-2.5-pro-preview-tts")
       .with_text("Your text here")
       .with_audio_output()
       .with_speech_config(config)
       .create()
       .await?;
"#
    );

    println!("3. CONVENIENCE CONSTRUCTORS:");
    println!(
        r#"
   // Voice only
   let config = SpeechConfig::with_voice("Kore");

   // Voice with language
   let config = SpeechConfig::with_voice_and_language("Charon", "en-GB");
"#
    );

    // =========================================================================
    // Summary
    // =========================================================================
    println!("\n{}", "=".repeat(78));
    println!("Text-to-Speech Demo Complete\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("  [REQ#1] POST with text + response_modalities=[\"audio\"] + speechConfig");
    println!("  [RES#1] completed: audio content with base64 data\n");

    println!("--- Production Considerations ---");
    println!("  - Use appropriate TTS model (gemini-2.5-pro-preview-tts)");
    println!("  - Voice selection affects tone and style");
    println!("  - Language setting should match content language");
    println!("  - Audio is returned as base64-encoded data");
    println!("  - Check audio.mime_type() for actual format");
    println!("  - Use audio.extension() for appropriate file extension");

    Ok(())
}
