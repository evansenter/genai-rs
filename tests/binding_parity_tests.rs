//! Live checks that request fields added in google-genai 2.19–2.25 are
//! accepted by the Gemini API, and stored as sent.
//!
//! Neither field changed the output observably when probed (2026-09-24),
//! so each test reads the stored interaction back and compares the field
//! with what it sent (#492), rather than trusting the 200.
//!
//! ```bash
//! cargo nextest run --test binding_parity_tests --run-ignored all
//! ```

mod common;

use common::{TINY_MP4_BASE64, TINY_WAV_BASE64, assert_stored, get_client, stored_echo};
use genai_rs::{Content, InteractionInput, TranscriptionConfig, TranscriptionMode};

/// Every object in `value` whose `type` is `kind`, depth-first.
fn objects_of_type<'a>(
    value: &'a serde_json::Value,
    kind: &str,
    out: &mut Vec<&'a serde_json::Value>,
) {
    match value {
        serde_json::Value::Object(map) => {
            if map.get("type").and_then(|t| t.as_str()) == Some(kind) {
                out.push(value);
            }
            map.values().for_each(|v| objects_of_type(v, kind, out));
        }
        serde_json::Value::Array(items) => items.iter().for_each(|v| objects_of_type(v, kind, out)),
        _ => {}
    }
}

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
        .create()
        .await
        .expect("video with a name was rejected");
    let id = response.id.clone().expect("stored interaction id");
    let input = stored_echo(&client, &id, "/input").await;
    let _ = client.interactions().delete(&id).await;
    assert!(response.as_text().is_some());

    let input = input.expect("the stored interaction has no input");
    let mut videos = Vec::new();
    objects_of_type(&input, "video", &mut videos);
    let names: Vec<_> = videos.iter().map(|v| v.get("name")).collect();
    assert_eq!(
        names,
        [Some(&serde_json::json!("clip.mp4"))],
        "the stored video's name differs from what was sent: {input}"
    );
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
        let response = client
            .interaction()
            .with_model(genai_rs::DEFAULT_MODEL)
            .with_input(InteractionInput::Content(vec![
                Content::text("Transcribe this audio."),
                Content::audio_data(TINY_WAV_BASE64, "audio/wav"),
            ]))
            .with_transcription_config(TranscriptionConfig::new().with_mode(mode.clone()))
            .create()
            .await
            .unwrap_or_else(|e| panic!("transcription mode {mode:?} was rejected: {e}"));
        let id = response.id.expect("stored interaction id");
        let stored =
            stored_echo(&client, &id, "/generation_config/transcription_config/mode").await;
        let _ = client.interactions().delete(&id).await;
        assert_stored(
            &format!("transcription mode {mode:?}"),
            &serde_json::to_value(&mode).unwrap(),
            stored.as_ref(),
        );
    }
}
