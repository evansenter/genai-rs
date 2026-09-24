//! Live checks that request fields added in google-genai 2.19–2.25 are
//! accepted by the Gemini API.
//!
//! These pin acceptance only: neither field changed the output observably
//! when probed (2026-09-24).
//!
//! ```bash
//! cargo nextest run --test binding_parity_tests --run-ignored all
//! ```

mod common;

use common::{TINY_MP4_BASE64, TINY_WAV_BASE64, get_client};
use genai_rs::{Content, InteractionInput, TranscriptionConfig, TranscriptionMode};

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_video_name_is_accepted() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let video = Content::video_data(TINY_MP4_BASE64, "video/mp4").with_video_name("clip.mp4");
    let response = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_input(InteractionInput::Content(vec![
            Content::text("What color is this video? One word."),
            video,
        ]))
        .with_store_disabled()
        .create()
        .await
        .expect("video with a name was rejected");
    assert!(response.as_text().is_some());
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_transcription_modes_are_accepted() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    for mode in [
        TranscriptionMode::Smart,
        TranscriptionMode::Verbatim {
            diarization_mode: Some("speaker".to_string()),
            timestamp_granularities: Some(vec!["word".to_string()]),
        },
    ] {
        client
            .interaction()
            .with_model(genai_rs::DEFAULT_MODEL)
            .with_input(InteractionInput::Content(vec![
                Content::text("Transcribe this audio."),
                Content::audio_data(TINY_WAV_BASE64, "audio/wav"),
            ]))
            .with_transcription_config(TranscriptionConfig::new().with_mode(mode.clone()))
            .with_store_disabled()
            .create()
            .await
            .unwrap_or_else(|e| panic!("transcription mode {mode:?} was rejected: {e}"));
    }
}
