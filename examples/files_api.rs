//! Example demonstrating the Files API for uploading and referencing files.
//!
//! The Files API allows you to upload files once and reference them across multiple
//! interactions. This is more efficient than inline base64 encoding for:
//! - Large files (up to 2GB)
//! - Files used in multiple interactions
//!
//! Files are automatically deleted after 48 hours.
//!
//! Run with: `cargo run --example files_api`

use genai_rs::{Client, Content};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = Client::new(api_key);

    // Create a sample text file with some content
    let temp_dir = tempfile::tempdir()?;
    let file_path = temp_dir.path().join("sample.txt");
    std::fs::write(
        &file_path,
        r#"
The Files API in Google's Generative AI allows developers to:

1. Upload files up to 2GB in size
2. Reference uploaded files across multiple interactions
3. Reduce bandwidth by uploading once and referencing many times
4. Store files for up to 48 hours

Supported file types include:
- Images (PNG, JPEG, GIF, WebP)
- Audio (MP3, WAV, AIFF, AAC, OGG, FLAC)
- Video (MP4, MPEG, MOV, AVI, FLV, MKV, WEBM)
- Documents (PDF, TXT, HTML, CSS, JS, MD, CSV, XML, RTF)

This makes it ideal for processing large media files or documents that you want
to analyze multiple times without resending the data.
"#,
    )?;

    println!("=== Files API Example ===\n");

    // 1. Upload a file
    println!("1. Uploading file...");
    let file = client.upload_file(&file_path).await?;
    println!("   Uploaded: {}", file.name);
    println!("   Display name: {:?}", file.display_name);
    println!("   MIME type: {}", file.mime_type);
    println!("   URI: {}", file.uri);
    println!("   State: {:?}", file.state);
    println!();

    // 2. Wait for file to be ready (if needed)
    println!("2. Waiting for file to be ready...");
    let ready_file = client
        .wait_for_file_ready(&file, Duration::from_secs(1), Duration::from_secs(60))
        .await?;
    println!("   File is now active!");
    println!();

    // 3. Use the file in an interaction
    println!("3. Using file in interaction...");
    let response = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_content(vec![
            Content::from_file(&ready_file),
            Content::text("What are the main points about the Files API in this document?"),
        ])
        .create()
        .await?;

    println!("   Response:");
    if let Some(text) = response.as_text() {
        for line in text.lines() {
            println!("   {line}");
        }
    }
    println!();

    // 4. Use the same file in another interaction (efficient - no re-upload)
    println!("4. Reusing file in another interaction...");
    let response2 = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_content(vec![
            Content::from_file(&ready_file),
            Content::text("What file types are supported according to this document?"),
        ])
        .create()
        .await?;

    println!("   Response:");
    if let Some(text) = response2.as_text() {
        for line in text.lines() {
            println!("   {line}");
        }
    }
    println!();

    // 5. List all uploaded files
    println!("5. Listing files...");
    let list_response = client.list_files(Some(5), None).await?;
    println!("   Found {} file(s)", list_response.files.len());
    for f in &list_response.files {
        println!(
            "   - {} ({}) - {:?}",
            f.display_name.as_deref().unwrap_or(&f.name),
            f.mime_type,
            f.state
        );
    }
    println!();

    // 6. Get file metadata
    println!("6. Getting file metadata...");
    let metadata = client.get_file(&ready_file.name).await?;
    println!("   Name: {}", metadata.name);
    println!("   Size: {:?} bytes", metadata.size_bytes);
    println!("   Created: {:?}", metadata.create_time);
    println!("   Expires: {:?}", metadata.expiration_time);
    println!("   State: {:?}", metadata.state);
    println!();

    // 7. Delete the file when done
    println!("7. Cleaning up - deleting file...");
    client.delete_file(&ready_file.name).await?;
    println!("   File deleted successfully!");
    println!();

    // Alternative: Upload bytes directly
    println!("=== Alternative: Upload bytes directly ===\n");

    let content = b"This is content uploaded directly from memory as bytes.";
    let bytes_file = client
        .upload_file_bytes(content.to_vec(), "text/plain", Some("memory-file.txt"))
        .await?;
    println!("Uploaded from bytes: {}", bytes_file.name);

    // Clean up
    client.delete_file(&bytes_file.name).await?;
    println!("Cleaned up.");

    // =========================================================================
    // Summary
    // =========================================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Files API Demo Complete\n");

    println!("--- Key Takeaways ---");
    println!("• upload_file() uploads files up to 2GB to server storage");
    println!("• Content::from_file() references uploaded files in interactions");
    println!("• Files can be reused across multiple interactions (efficient)");
    println!("• Files auto-expire after 48 hours; use delete_file() to clean up\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("Upload:");
    println!("  [REQ#1] POST to files.upload endpoint");
    println!("  [RES#1] file metadata with name, uri, state\n");
    println!("Use in interaction:");
    println!("  [REQ#2] POST with file reference (uri) + input text");
    println!("  [RES#2] completed: text analyzing file content\n");
    println!("List/Get/Delete:");
    println!("  [REQ#N] GET files, GET file/:name, DELETE file/:name\n");

    println!("--- Production Considerations ---");
    println!("• Wait for file state=ACTIVE before using (wait_for_file_ready)");
    println!("• Use for large files or files reused across interactions");
    println!("• For small inline content, use add_image_data() etc.");
    println!("• upload_file_bytes() uploads from memory without disk I/O");

    Ok(())
}
