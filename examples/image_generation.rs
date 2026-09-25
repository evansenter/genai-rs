//! Image generation with `DEFAULT_IMAGE_MODEL`.
//!
//! `with_image_output()` asks for the image modality and `with_image_config()`
//! sets aspect ratio and size. Images come back as base64 content; `images()`
//! iterates them with their MIME type, and `first_image_bytes()` is the
//! shortcut when you only want one. The default text model will not produce
//! images.
//!
//! Run with: `cargo run --example image_generation`

use genai_rs::{Client, ImageAspectRatio, ImageConfig, ImageSize};
use std::env;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = Client::builder(api_key).build()?;

    println!("Model: {}", genai_rs::DEFAULT_IMAGE_MODEL);
    let response = client
        .interaction()
        .with_model(genai_rs::DEFAULT_IMAGE_MODEL)
        .with_text("A watercolor painting of a white and orange cat beside a vintage motorcycle.")
        .with_image_output()
        .with_image_config(ImageConfig {
            aspect_ratio: Some(ImageAspectRatio::Widescreen16x9),
            image_size: Some(ImageSize::Hd1k),
        })
        .create()
        .await?;

    let mut saved = 0;
    for (i, image) in response.images().enumerate() {
        let bytes = image.bytes()?;
        let path = env::temp_dir().join(format!("genai_rs_image_{i}.{}", image.extension()));
        std::fs::write(&path, &bytes)?;
        println!(
            "Saved {} bytes ({:?}) to {}",
            bytes.len(),
            image.mime_type(),
            path.display()
        );
        saved += 1;
    }
    if saved == 0 {
        return Err("the response contained no image".into());
    }
    // Image models may also return a caption or commentary.
    if let Some(text) = response.as_text() {
        println!("Model text: {text}");
    }

    Ok(())
}
