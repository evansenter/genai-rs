//! The Files API: upload once, reference from many interactions.
//!
//! Inline base64 (`Content::image_data` and friends) re-sends the bytes on
//! every request. An uploaded file is sent once and referenced by URI until
//! it expires (48 hours) or you delete it. `upload_file` streams from disk,
//! so a large file is never loaded into memory.
//!
//! Run with: `cargo run --example files_api`

use genai_rs::{Client, Content};
use std::env;
use std::error::Error;
use std::time::Duration;

const NOTES: &str = "\
Ferrymead Engineering release notes, v4.2
- Edge services moved to 6 worker threads after the latency review.
- The billing export now runs hourly instead of nightly.
- Deprecated: the v1 webhook payload format, removed in v5.0.
";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = Client::builder(api_key).build()?;

    let dir = tempfile::tempdir()?;
    let path = dir.path().join("release-notes.txt");
    std::fs::write(&path, NOTES)?;

    let file = client.upload_file(&path).await?;
    println!(
        "Uploaded {} ({}, state {:?})",
        file.name, file.mime_type, file.state
    );

    // Delete the upload whether or not the rest succeeds.
    let result = use_file(&client, &file).await;
    client.delete_file(&file.name).await?;
    println!("Deleted {}", file.name);
    result?;

    // Bytes already in memory can skip the filesystem.
    let bytes_file = client
        .upload_file_bytes(
            NOTES.as_bytes().to_vec(),
            "text/plain",
            Some("notes-from-memory.txt"),
        )
        .await?;
    println!("\nUploaded from memory: {}", bytes_file.name);
    client.delete_file(&bytes_file.name).await?;

    Ok(())
}

async fn use_file(client: &Client, file: &genai_rs::FileMetadata) -> Result<(), Box<dyn Error>> {
    // Uploads are processed asynchronously; wait until the file is usable.
    let file = client
        .wait_for_file_ready(file, Duration::from_secs(1), Duration::from_secs(60))
        .await?;

    // Two interactions, one upload.
    for question in [
        "How many worker threads do edge services use now? Just the number.",
        "What is deprecated, and when is it removed? One sentence.",
    ] {
        let response = client
            .interaction()
            .with_model(genai_rs::DEFAULT_MODEL)
            .with_content(vec![Content::from_file(&file), Content::text(question)])
            .create()
            .await?;
        println!(
            "Q: {question}\nA: {}",
            response.as_text().ok_or("no text in response")?
        );
    }

    let listed = client.list_files(Some(5), None).await?;
    println!("\nFirst page of your files: {} entries", listed.files.len());

    let metadata = client.get_file(&file.name).await?;
    println!(
        "{}: {:?} bytes, expires {:?}",
        metadata.name, metadata.size_bytes, metadata.expiration_time
    );
    Ok(())
}
