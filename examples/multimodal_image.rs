//! Image input: inline base64 images, several at once, and `Resolution`.
//!
//! `Content::image_data(base64, mime_type)` sends image bytes inline; for
//! files on disk `image_from_file(path)` loads and encodes them, and for large
//! or reused files the Files API (`files_api`) avoids re-sending bytes.
//! `.with_resolution()` trades image detail against input tokens.
//!
//! Run with: `cargo run --example multimodal_image`

use genai_rs::{Client, Content, Resolution};
use std::env;
use std::error::Error;

// 1x1 PNGs, one red and one blue.
const TINY_RED_PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";
const TINY_BLUE_PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = Client::builder(api_key).build()?;
    let model = genai_rs::DEFAULT_MODEL;

    println!("--- Two images in one request ---");
    let comparison = client
        .interaction()
        .with_model(model)
        .with_content(vec![
            Content::text("What color is each of these two images? One line each."),
            Content::image_data(TINY_RED_PNG_BASE64, "image/png"),
            Content::image_data(TINY_BLUE_PNG_BASE64, "image/png"),
        ])
        .create()
        .await?;
    println!("{}\n", comparison.as_text().ok_or("no text in response")?);

    // The image stays in the stored conversation; the follow-up needn't resend it.
    println!("--- Follow-up about the same images ---");
    let follow_up = client
        .interaction()
        .with_model(model)
        .with_previous_interaction(
            comparison
                .id
                .as_deref()
                .ok_or("stored interaction has no ID")?,
        )
        .with_text("Which of those two colors is warmer? One sentence.")
        .create()
        .await?;
    println!("{}\n", follow_up.as_text().ok_or("no text in response")?);

    // Resolution sets the image's token budget, independent of its pixel size.
    println!("--- Resolution vs. image input tokens ---");
    for resolution in [Resolution::Low, Resolution::High] {
        let response = client
            .interaction()
            .with_model(model)
            .with_content(vec![
                Content::text("What color is this image? One word."),
                Content::image_data(TINY_RED_PNG_BASE64, "image/png")
                    .with_resolution(resolution.clone()),
            ])
            .create()
            .await?;
        let image_tokens = response
            .usage
            .as_ref()
            .and_then(|u| u.input_tokens_for_modality("image"));
        println!(
            "{resolution:?}: image tokens {image_tokens:?}, answer: {}",
            response.as_text().ok_or("no text in response")?.trim()
        );
    }

    Ok(())
}
