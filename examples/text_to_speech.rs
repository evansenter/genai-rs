//! Text-to-speech: one voice, then a two-speaker dialogue.
//!
//! Writes the audio to `$TMPDIR/genai-rs-tts/`. `DEFAULT_TTS_MODEL` returns
//! WAV, so the files play as-is.
//!
//! Run with: cargo run --example text_to_speech

use genai_rs::{Client, Content, InteractionInput, InteractionResponse, SpeechConfig};
use std::error::Error;
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = std::env::var("GEMINI_API_KEY").map_err(|_| "GEMINI_API_KEY is not set")?;
    let client = Client::builder(api_key).build()?;
    let out_dir = std::env::temp_dir().join("genai-rs-tts");
    std::fs::create_dir_all(&out_dir)?;

    let single = client
        .interaction()
        .with_model(genai_rs::DEFAULT_TTS_MODEL)
        .with_text("Hello from genai-rs.")
        .with_audio_output()
        .with_voice("Kore")
        .with_store_disabled()
        .create()
        .await?;
    save(&single, &out_dir, "single")?;

    // Each turn names a speaker defined in the speech configs.
    let dialogue = client
        .interaction()
        .with_model(genai_rs::DEFAULT_TTS_MODEL)
        .with_input(InteractionInput::Content(vec![
            Content::speaker_text("Alice", "Did the build pass?"),
            Content::speaker_text("Bob", "It did, on the first try."),
        ]))
        .with_audio_output()
        .with_speech_configs(vec![
            SpeechConfig::for_speaker("Alice", "Kore", "en-US"),
            SpeechConfig::for_speaker("Bob", "Puck", "en-US"),
        ])
        .with_store_disabled()
        .create()
        .await?;
    save(&dialogue, &out_dir, "dialogue")?;

    Ok(())
}

fn save(response: &InteractionResponse, dir: &Path, name: &str) -> Result<(), Box<dyn Error>> {
    let audio = response
        .first_audio()
        .ok_or("the response contained no audio")?;
    let path = dir.join(format!("{name}.{}", audio.extension()));
    std::fs::write(&path, audio.bytes()?)?;
    println!(
        "{} ({})",
        path.display(),
        audio.mime_type().unwrap_or("unknown type")
    );
    Ok(())
}
