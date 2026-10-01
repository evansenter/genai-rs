//! Files API: upload (from disk, streamed, and from memory), get, list
//! (one page and across pages), delete, readiness polling, and use in an
//! interaction.
//!
//! ```bash
//! cargo nextest run --test files_api_tests --run-ignored all
//! ```

mod common;
use futures_util::StreamExt;
use genai_rs::wire::{WireEvent, WireInspector};
use genai_rs::{Client, Content, FileMetadata, FileUpload, GenaiError, PollOptions};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn get_client() -> Client {
    common::get_client().expect("GEMINI_API_KEY must be set")
}

/// Polls every second, giving up after `secs` seconds.
fn poll_every_second_for(secs: u64) -> PollOptions {
    PollOptions::new()
        .with_poll_interval(Duration::from_secs(1))
        .with_timeout(Duration::from_secs(secs))
}

/// A missing file reads as 403 "…or it may not exist" (verified live
/// 2026-09-24): the API does not distinguish absent from inaccessible.
fn assert_file_gone(result: Result<FileMetadata, GenaiError>) {
    match result {
        Err(GenaiError::Api {
            status_code: 403,
            message,
            ..
        }) => assert!(message.contains("may not exist"), "{message}"),
        other => panic!("expected the 403 missing-file error, got: {other:?}"),
    }
}

/// Tests uploading a small text file.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_upload_text_file() {
    let client = get_client();

    // Create a temporary text file
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("test.txt");
    std::fs::write(&file_path, "Hello, this is a test file for the Files API.").unwrap();

    // Upload the file
    let file = client
        .files()
        .upload(FileUpload::from_path(&file_path).with_mime_type("text/plain"))
        .await
        .expect("Failed to upload file");

    // Verify file metadata
    assert!(
        file.name.starts_with("files/"),
        "File name should start with 'files/'"
    );
    assert_eq!(file.mime_type, "text/plain");
    assert!(
        file.display_name.as_deref() == Some("test.txt"),
        "Display name should be the filename"
    );
    assert!(!file.uri.is_empty(), "URI should not be empty");

    // Clean up
    client
        .files()
        .delete(&file.name)
        .await
        .expect("Failed to delete file");
}

/// Tests uploading bytes directly.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_upload_bytes() {
    let client = get_client();

    let content = b"This is some test content uploaded as bytes.";
    let file = client
        .files()
        .upload(
            FileUpload::from_bytes(content.to_vec(), "text/plain")
                .with_display_name("bytes-test.txt"),
        )
        .await
        .expect("Failed to upload bytes");

    assert!(file.name.starts_with("files/"));
    assert_eq!(file.mime_type, "text/plain");
    assert_eq!(file.display_name.as_deref(), Some("bytes-test.txt"));

    // Clean up
    client.files().delete(&file.name).await.unwrap();
}

/// Tests getting file metadata.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_files_get() {
    let client = get_client();

    // Upload a file
    let content = b"Test file for files().get test";
    let uploaded = client
        .files()
        .upload(
            FileUpload::from_bytes(content.to_vec(), "text/plain")
                .with_display_name("get-test.txt"),
        )
        .await
        .expect("Failed to upload file");

    // Retrieve file metadata
    let retrieved = client
        .files()
        .get(&uploaded.name)
        .await
        .expect("Failed to get file");

    assert_eq!(retrieved.name, uploaded.name);
    assert_eq!(retrieved.mime_type, uploaded.mime_type);

    // Clean up
    client.files().delete(&uploaded.name).await.unwrap();
}

/// Tests listing files.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_files_list_one_page() {
    let client = get_client();

    // Upload a file to ensure there's at least one
    let content = b"Test file for files().list test";
    let file = client
        .files()
        .upload(
            FileUpload::from_bytes(content.to_vec(), "text/plain")
                .with_display_name("list-test.txt"),
        )
        .await
        .expect("Failed to upload file");

    // List files (newest first)
    let response = client
        .files()
        .list()
        .with_page_size(10)
        .send()
        .await
        .expect("Failed to list files");

    // The uploaded file should be in the list
    let found = response.files.iter().any(|f| f.name == file.name);
    assert!(found, "Uploaded file should appear in file list");

    // Clean up
    client.files().delete(&file.name).await.unwrap();
}

/// Tests deleting a file.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_files_delete() {
    let client = get_client();

    // Upload a file
    let content = b"Test file to be deleted";
    let file = client
        .files()
        .upload(
            FileUpload::from_bytes(content.to_vec(), "text/plain")
                .with_display_name("delete-test.txt"),
        )
        .await
        .expect("Failed to upload file");

    // Delete the file
    client
        .files()
        .delete(&file.name)
        .await
        .expect("Failed to delete file");

    assert_file_gone(client.files().get(&file.name).await);
}

/// An uploaded file's content reaches the model when referenced by URI.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_file_in_interaction() {
    let client = get_client();

    // Upload a text file with some content
    let content = b"The capital of France is Paris. The Eiffel Tower is 330 meters tall.";
    let file = client
        .files()
        .upload(
            FileUpload::from_bytes(content.to_vec(), "text/plain").with_display_name("facts.txt"),
        )
        .await
        .expect("Failed to upload file");

    // Wait for file to be ready (text files should be quick)
    let ready_file = client
        .files()
        .wait_until_active(&file.name, poll_every_second_for(30))
        .await
        .expect("File should become ready");

    assert!(
        ready_file.is_active(),
        "File should be active after waiting"
    );

    // Use the file in an interaction
    let response = retry_request!([client, ready_file] => {
        client
            .interaction()
            .with_model(genai_rs::DEFAULT_MODEL)
            .with_content(vec![
                Content::from_file(&ready_file),
                Content::text("What city is mentioned in this document?"),
            ])
            .create()
            .await
    })
    .expect("Interaction should succeed");

    let text = response.as_text().expect("Response should have text");
    common::assert_response_semantic(
        &client,
        "The uploaded file says 'The capital of France is Paris.' The user asked which city it mentions.",
        text,
        "Does this response name Paris?",
    )
    .await;

    client.files().delete(&file.name).await.unwrap();
}

/// Tests that Content::from_file() correctly infers content type.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_content_from_file_type_inference() {
    use genai_rs::Content;

    let client = get_client();

    // Upload files with different MIME types
    let upload = |data: &[u8], mime_type: &str, name: &str| {
        client
            .files()
            .upload(FileUpload::from_bytes(data.to_vec(), mime_type).with_display_name(name))
    };
    let video_file = upload(b"fake video data", "video/mp4", "test.mp4")
        .await
        .unwrap();
    let image_file = upload(b"fake image data", "image/png", "test.png")
        .await
        .unwrap();
    let audio_file = upload(b"fake audio data", "audio/mp3", "test.mp3")
        .await
        .unwrap();
    let doc_file = upload(b"fake pdf data", "application/pdf", "test.pdf")
        .await
        .unwrap();

    // Verify content type inference
    let video_content = Content::from_file(&video_file);
    assert!(
        matches!(video_content, Content::Video { .. }),
        "video/mp4 should create Video content"
    );

    let image_content = Content::from_file(&image_file);
    assert!(
        matches!(image_content, Content::Image { .. }),
        "image/png should create Image content"
    );

    let audio_content = Content::from_file(&audio_file);
    assert!(
        matches!(audio_content, Content::Audio { .. }),
        "audio/mp3 should create Audio content"
    );

    let doc_content = Content::from_file(&doc_file);
    assert!(
        matches!(doc_content, Content::Document { .. }),
        "application/pdf should create Document content"
    );

    // Clean up
    for file in [video_file, image_file, audio_file, doc_file] {
        client.files().delete(&file.name).await.unwrap();
    }
}

/// Records the URL of every GET the client sends.
#[derive(Debug, Default)]
struct GetUrls(Mutex<Vec<String>>);

impl WireInspector for GetUrls {
    fn on_event(&self, event: &WireEvent) {
        if let WireEvent::Request { method, url, .. } = event
            && method == "GET"
        {
            self.0.lock().unwrap().push(url.clone());
        }
    }
}

/// `items()` follows `nextPageToken` across pages: at one file per page, it
/// streams until both fresh uploads have turned up (the list is newest
/// first, but other tests upload concurrently, so allow up to 50 pages).
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_list_items_follow_pages_until_both_uploads_appear() {
    let api_key = std::env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let urls = Arc::new(GetUrls::default());
    let client = Client::builder(api_key)
        .add_wire_inspector(urls.clone())
        .build()
        .expect("client");

    let mut uploaded = Vec::new();
    for i in 0..2 {
        let file = client
            .files()
            .upload(
                FileUpload::from_bytes(format!("Content {i}").into_bytes(), "text/plain")
                    .with_display_name(format!("paginate-{i}.txt")),
            )
            .await
            .expect("Failed to upload file");
        uploaded.push(file);
    }

    let listed = async {
        let mut missing: Vec<&str> = uploaded.iter().map(|f| f.name.as_str()).collect();
        let mut seen = Vec::new();
        let mut items = client.files().list().with_page_size(1).items().take(50);
        while !missing.is_empty() {
            let Some(file) = items.next().await else {
                break;
            };
            let file = file?;
            missing.retain(|name| *name != file.name);
            seen.push(file.name);
        }
        Ok::<_, GenaiError>((seen, missing.len()))
    }
    .await;

    for file in &uploaded {
        client.files().delete(&file.name).await.unwrap();
    }

    let (seen, missing) = listed.expect("files().list().items() failed");
    println!("Streamed {} file(s) to find both uploads", seen.len());
    assert_eq!(missing, 0, "not every upload was listed: {seen:?}");
    let mut unique = seen.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        seen.len(),
        "a file repeated across pages: {seen:?}"
    );

    // At most one file per page, so each item came from its own request;
    // every request after the first carried the previous page's token.
    let urls = urls.0.lock().unwrap().clone();
    let lists: Vec<&String> = urls.iter().filter(|u| u.contains("/files?")).collect();
    println!("Sent {} list request(s)", lists.len());
    assert!(
        lists.len() >= 2,
        "two uploads at pageSize=1 need two pages: {lists:?}"
    );
    assert!(
        lists.len() >= seen.len(),
        "a page held more than one file: {lists:?}"
    );
    for (i, url) in lists.iter().enumerate() {
        assert!(
            url.contains("pageSize=1"),
            "page {i} dropped the page size: {url}"
        );
        assert_eq!(url.contains("pageToken="), i > 0, "page {i}: {url}");
    }
}

/// `wait_until_active` with the default `PollOptions` returns an already
/// active file.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_wait_until_active_returns_an_active_file() {
    let client = get_client();

    // Upload a small text file (should be immediately active)
    let file = client
        .files()
        .upload(
            FileUpload::from_bytes(b"Small test content".to_vec(), "text/plain")
                .with_display_name("wait-test.txt"),
        )
        .await
        .expect("Failed to upload file");

    // Wait should return quickly for an already active file
    let ready = client
        .files()
        .wait_until_active(&file.name, PollOptions::new())
        .await
        .expect("File should become ready");

    assert!(ready.is_active());

    // Clean up
    client.files().delete(&file.name).await.unwrap();
}

/// Tests that `files().get()` returns an error for non-existent files.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_get_nonexistent_file_returns_error() {
    let client = get_client();
    assert_file_gone(client.files().get("files/abcdefghijkl").await);
}

// =============================================================================
// Streamed upload from disk
// =============================================================================

/// A file larger than the 8 MB read buffer streams up intact.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_path_upload_streams_a_file_larger_than_the_read_buffer() {
    let client = get_client();

    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("large_test.txt");
    let data: Vec<u8> = (0..9 * 1024 * 1024).map(|i| (i % 256) as u8).collect();
    std::fs::write(&file_path, &data).unwrap();

    let file = client
        .files()
        .upload(FileUpload::from_path(&file_path).with_mime_type("text/plain"))
        .await
        .expect("Streamed upload failed");

    assert!(
        file.name.starts_with("files/"),
        "File name should start with 'files/'"
    );
    assert_eq!(file.mime_type, "text/plain");
    assert_eq!(file.display_name.as_deref(), Some("large_test.txt"));
    assert!(!file.uri.is_empty(), "URI should not be empty");
    assert_eq!(
        file.size_bytes_as_u64(),
        Some(data.len() as u64),
        "File size should match uploaded data"
    );

    client
        .files()
        .delete(&file.name)
        .await
        .expect("Failed to delete file");
}

/// A path upload infers the MIME type from the extension, and the display
/// name from the file name.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_path_upload_auto_mime() {
    let client = get_client();

    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("test.mp4");
    std::fs::write(&file_path, vec![0u8; 1024]).unwrap();

    let file = client
        .files()
        .upload(FileUpload::from_path(&file_path))
        .await
        .expect("Upload failed");

    assert_eq!(
        file.mime_type, "video/mp4",
        "MIME type should be auto-detected from extension"
    );
    assert_eq!(file.display_name.as_deref(), Some("test.mp4"));

    client.files().delete(&file.name).await.unwrap();
}

/// `with_display_name` replaces a path upload's default display name.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_path_upload_with_display_name() {
    let client = get_client();

    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("q4.csv");
    std::fs::write(&file_path, "region,total\nnorth,12\n").unwrap();

    let file = client
        .files()
        .upload(FileUpload::from_path(&file_path).with_display_name("Q4 sales"))
        .await
        .expect("Upload failed");
    let fetched = client.files().get(&file.name).await;
    client.files().delete(&file.name).await.unwrap();

    assert_eq!(file.display_name.as_deref(), Some("Q4 sales"));
    assert_eq!(file.mime_type, "text/csv");
    let fetched = fetched.expect("files().get failed");
    assert_eq!(fetched.display_name.as_deref(), Some("Q4 sales"));
}

/// Empty files are rejected before any request is made.
#[tokio::test]
async fn test_path_upload_empty_file_error() {
    let client = Client::new("test-api-key".to_string());

    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("empty.txt");
    std::fs::write(&file_path, b"").unwrap();

    let err = client
        .files()
        .upload(FileUpload::from_path(&file_path).with_mime_type("text/plain"))
        .await
        .expect_err("Should fail for empty file");
    assert!(matches!(err, GenaiError::InvalidInput(_)), "{err:?}");
    assert!(err.to_string().contains("empty"), "{err}");
}

/// A missing file fails on open, before any request is made.
#[tokio::test]
async fn test_path_upload_nonexistent_file_error() {
    let client = Client::new("test-api-key".to_string());

    let err = client
        .files()
        .upload(FileUpload::from_path("/nonexistent/path/to/file.txt").with_mime_type("text/plain"))
        .await
        .expect_err("Should fail for nonexistent file");
    assert!(matches!(err, GenaiError::InvalidInput(_)), "{err:?}");
    assert!(err.to_string().contains("Failed to read file"), "{err}");
}

/// A file uploaded from disk can be used in an interaction.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_path_upload_in_interaction() {
    let client = get_client();

    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("interact.txt");
    let content = "The quick brown fox jumps over the lazy dog. This file was streamed from disk.";
    std::fs::write(&file_path, content).unwrap();

    let file = client
        .files()
        .upload(FileUpload::from_path(&file_path).with_mime_type("text/plain"))
        .await
        .expect("Upload failed");

    // Wait for file to be ready
    let ready_file = client
        .files()
        .wait_until_active(&file.name, poll_every_second_for(30))
        .await
        .expect("File should become ready");

    assert!(ready_file.is_active(), "File should be active");

    // Use in interaction
    let response = retry_request!([client, ready_file] => {
        client
            .interaction()
            .with_model(genai_rs::DEFAULT_MODEL)
            .with_content(vec![
                Content::from_file(&ready_file),
                Content::text("What does the file say about a fox?"),
            ])
            .create()
            .await
    })
    .expect("Interaction should succeed");

    let text = response.as_text().expect("Response should have text");
    common::assert_response_semantic(
        &client,
        "The uploaded file says 'The quick brown fox jumps over the lazy dog.' The user asked what it says about a fox.",
        text,
        "Does this response say the fox jumps over the lazy dog?",
    )
    .await;

    client.files().delete(&file.name).await.unwrap();
}
