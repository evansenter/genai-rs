//! The Files API: upload once, reference from many interactions.
//!
//! Inline base64 (`Content::image_data` and friends) re-sends the bytes on
//! every request. An uploaded file is sent once and referenced by URI until
//! it expires (48 hours) or you delete it. `FileUpload::from_path` streams
//! from disk, so a large file is never loaded into memory.
//!
//! Run with: `cargo run --example files_api`

use futures_util::TryStreamExt;
use genai_rs::{Client, Content, FileMetadata, FileUpload, PollOptions};
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

    let file = client.files().upload(FileUpload::from_path(&path)).await?;
    println!(
        "Uploaded {} ({}, state {:?})",
        file.name, file.mime_type, file.state
    );

    // Delete the upload whether or not the rest succeeds.
    let result = use_file(&client, &file).await;
    client.files().delete(&file.name).await?;
    println!("Deleted {}", file.name);
    result?;

    // Bytes already in memory can skip the filesystem.
    let bytes_file = client
        .files()
        .upload(
            FileUpload::from_bytes(NOTES.as_bytes().to_vec(), "text/plain")
                .with_display_name("notes-from-memory.txt"),
        )
        .await?;
    println!("\nUploaded from memory: {}", bytes_file.name);
    client.files().delete(&bytes_file.name).await?;

    Ok(())
}

async fn use_file(client: &Client, file: &FileMetadata) -> Result<(), Box<dyn Error>> {
    // Uploads are processed asynchronously; wait until the file is usable.
    let poll = PollOptions::new()
        .with_poll_interval(Duration::from_secs(1))
        .with_timeout(Duration::from_secs(60));
    let file = client.files().wait_until_active(&file.name, poll).await?;

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

    // Newest first, so the upload is on the first page unless something
    // else was uploaded meanwhile; `items()` follows the pages either way.
    let listed = client.files().list().with_page_size(5).send().await?;
    println!("\nFirst page of your files: {} entries", listed.files.len());
    let all: Vec<FileMetadata> = client.files().list().items().try_collect().await?;
    if !all.iter().any(|f| f.name == file.name) {
        return Err(format!("{} is missing from the file list", file.name).into());
    }
    println!("All pages: {} files, including {}", all.len(), file.name);

    let metadata = client.files().get(&file.name).await?;
    println!(
        "{}: {:?} bytes, expires {:?}",
        metadata.name, metadata.size_bytes, metadata.expiration_time
    );
    Ok(())
}
