//! Video input, and clipping what the model ingests with `VideoProcessing`.
//!
//! `Content::video_data(base64, mime_type)` sends video inline;
//! `video_from_file(path)` loads and encodes a file. Videos past a few
//! megabytes belong in the Files API (`files_api`), referenced by URI.
//!
//! `VideoProcessing::segment()` restricts the model to a time window and
//! frame rate. The window is what reduces video input tokens (see the
//! measurements on `VideoProcessing`), which matters on long videos. The API
//! accepts `processing` only on video inside a `user_input` step, hence
//! `with_history(vec![Step::user_input(..)])` rather than `with_content`.
//!
//! Run with: `cargo run --example video_input`

use genai_rs::{Client, Content, Step, VideoProcessing};
use std::env;
use std::error::Error;

// A 1-second 64x64 red H.264 clip.
const DEMO_MP4_BASE64: &str = "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";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = Client::builder(api_key).build()?;

    println!("--- Whole clip ---");
    let response = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_content(vec![
            Content::text("Describe this video in one sentence."),
            Content::video_data(DEMO_MP4_BASE64, "video/mp4"),
        ])
        .create()
        .await?;
    println!("{}", response.as_text().ok_or("no text in response")?);
    print_video_tokens(&response);

    // This clip is one second long, so both requests ingest about one frame
    // and report similar counts; the window pays off on long videos.
    println!("\n--- First half-second at 1 fps ---");
    let clipped = Content::video_data(DEMO_MP4_BASE64, "video/mp4").with_processing(
        VideoProcessing::segment()
            .start_offset("0s")
            .end_offset("0.5s")
            .fps(1.0)
            .build(),
    );
    let response = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_history(vec![Step::user_input(vec![
            Content::text("Describe this video in one sentence."),
            clipped,
        ])])
        .create()
        .await?;
    println!("{}", response.as_text().ok_or("no text in response")?);
    print_video_tokens(&response);

    Ok(())
}

fn print_video_tokens(response: &genai_rs::InteractionResponse) {
    let tokens = response
        .usage
        .as_ref()
        .and_then(|u| u.input_tokens_for_modality("video"));
    println!("Video input tokens: {tokens:?}");
}
