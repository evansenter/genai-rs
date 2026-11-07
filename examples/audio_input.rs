//! Speech-to-text with `DEFAULT_TRANSCRIPTION_MODEL`.
//!
//! `Content::audio_data(base64, mime_type)` sends audio inline;
//! `audio_from_file(path)` loads and encodes a file, detecting the MIME type
//! from its extension. For large or reused recordings, upload once with the
//! Files API (`files_api`) and reference the file instead. Here the clip is
//! synthesized with `DEFAULT_TTS_MODEL`, so the example needs no audio file.
//!
//! `TranscriptionConfig` takes effect on the transcription model: a verbatim
//! mode annotates each word with its timing and speaker
//! (`Annotation::WordInfo`), and a smart mode's `source_text`, the script the
//! speaker read, steers the transcript's wording. The model only
//! transcribes: it ignores a text prompt beside the audio.
//!
//! Run with: `cargo run --example audio_input`

use genai_rs::{Annotation, Client, Content, TranscriptionConfig, TranscriptionMode};
use std::env;
use std::error::Error;

const SCRIPT: &str = "The quick brown fox jumps over the lazy dog.";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = Client::builder(api_key).build()?;

    let speech = client
        .interaction()
        .with_model(genai_rs::DEFAULT_TTS_MODEL)
        .with_text(SCRIPT)
        .with_audio_output()
        .with_store_disabled()
        .create()
        .await?;
    let audio = speech
        .output_contents()
        .into_iter()
        .find(|c| matches!(c, Content::Audio { .. }))
        .cloned()
        .ok_or("the TTS response carried no audio")?;

    // Verbatim, with a timing and a speaker for every word.
    let verbatim = client
        .interaction()
        .with_model(genai_rs::DEFAULT_TRANSCRIPTION_MODEL)
        .with_content(vec![audio.clone()])
        .with_transcription_config(
            TranscriptionConfig::new()
                .with_language_codes(["en-US"])
                .with_mode(TranscriptionMode::Verbatim {
                    diarization_mode: Some("speaker".to_string()),
                    timestamp_granularities: Some(vec!["word".to_string()]),
                }),
        )
        .with_store_disabled()
        .create()
        .await?;
    println!("{}", verbatim.as_text().ok_or("no transcript")?);
    let mut words = 0;
    for annotation in verbatim.all_annotations() {
        if let Annotation::WordInfo {
            text: Some(word),
            start_offset: Some(start),
            end_offset: Some(end),
            speaker,
            ..
        } = annotation
        {
            let speaker = speaker.as_deref().unwrap_or("?");
            println!("  {start:>7} - {end:<7} {speaker}  {word}");
            words += 1;
        }
    }
    if words == 0 {
        return Err("the verbatim transcript carried no word timings".into());
    }

    // Smart, guided by the script the speech was read from.
    let guided = client
        .interaction()
        .with_model(genai_rs::DEFAULT_TRANSCRIPTION_MODEL)
        .with_content(vec![audio])
        .with_transcription_config(
            TranscriptionConfig::new().with_mode(TranscriptionMode::Smart {
                source_text: Some(SCRIPT.to_string()),
            }),
        )
        .with_store_disabled()
        .create()
        .await?;
    println!("{}", guided.as_text().ok_or("no transcript")?);
    if let Some(tokens) = guided
        .usage
        .as_ref()
        .and_then(|u| u.input_tokens_for_modality("audio"))
    {
        println!("Audio input tokens: {tokens}");
    }

    Ok(())
}
