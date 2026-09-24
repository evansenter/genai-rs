//! Audio input with speech-recognition tuning.
//!
//! `Content::audio_data(base64, mime_type)` sends audio inline;
//! `audio_from_file(path)` loads and encodes a file, detecting the MIME type
//! from its extension. For large or reused recordings, upload once with the
//! Files API (`files_api`) and reference the file instead.
//!
//! `TranscriptionConfig` adds BCP-47 language hints (omit to auto-detect),
//! speaker diarization, and word-level timestamps.
//!
//! Run with: `cargo run --example audio_input`

use genai_rs::{Client, Content, TranscriptionConfig};
use std::env;
use std::error::Error;

// 100 frames of 16-bit mono silence: a valid WAV that keeps the example
// self-contained. Real audio makes the transcription settings visible.
const DEMO_WAV_BASE64: &str = "UklGRuwAAABXQVZFZm10IBAAAAABAAEARKwAAIhYAQACABAAZGF0YcgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = Client::builder(api_key).build()?;

    let response = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_content(vec![
            Content::text("Transcribe this clip. If it contains no speech, say 'Silent audio.'"),
            Content::audio_data(DEMO_WAV_BASE64, "audio/wav"),
        ])
        .with_transcription_config(
            TranscriptionConfig::new()
                .with_language_codes(["en-US"])
                .with_diarization_mode("speaker")
                .with_timestamp_granularities(["word"]),
        )
        .create()
        .await?;

    println!("{}", response.as_text().ok_or("no text in response")?);
    if let Some(tokens) = response
        .usage
        .as_ref()
        .and_then(|u| u.input_tokens_for_modality("audio"))
    {
        println!("Audio input tokens: {tokens}");
    }

    Ok(())
}
