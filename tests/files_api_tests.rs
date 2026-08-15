//! Integration tests for the Files API.
//!
//! These tests verify file upload, listing, deletion, and integration with interactions.

mod common;

use genai_rs::{Client, Content};
use std::time::Duration;

fn get_client() -> Client {
    let api_key = std::env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    Client::new(api_key)
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
        .upload_file_with_mime(&file_path, "text/plain")
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
        .delete_file(&file.name)
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
        .upload_file_bytes(content.to_vec(), "text/plain", Some("bytes-test.txt"))
        .await
        .expect("Failed to upload bytes");

    assert!(file.name.starts_with("files/"));
    assert_eq!(file.mime_type, "text/plain");
    assert_eq!(file.display_name.as_deref(), Some("bytes-test.txt"));

    // Clean up
    client.delete_file(&file.name).await.unwrap();
}

/// Tests getting file metadata.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_get_file() {
    let client = get_client();

    // Upload a file
    let content = b"Test file for get_file test";
    let uploaded = client
        .upload_file_bytes(content.to_vec(), "text/plain", Some("get-test.txt"))
        .await
        .expect("Failed to upload file");

    // Retrieve file metadata
    let retrieved = client
        .get_file(&uploaded.name)
        .await
        .expect("Failed to get file");

    assert_eq!(retrieved.name, uploaded.name);
    assert_eq!(retrieved.mime_type, uploaded.mime_type);

    // Clean up
    client.delete_file(&uploaded.name).await.unwrap();
}

/// Tests listing files.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_list_files() {
    let client = get_client();

    // Upload a file to ensure there's at least one
    let content = b"Test file for list_files test";
    let file = client
        .upload_file_bytes(content.to_vec(), "text/plain", Some("list-test.txt"))
        .await
        .expect("Failed to upload file");

    // List files
    let response = client
        .list_files(Some(10), None)
        .await
        .expect("Failed to list files");

    // The uploaded file should be in the list
    let found = response.files.iter().any(|f| f.name == file.name);
    assert!(found, "Uploaded file should appear in file list");

    // Clean up
    client.delete_file(&file.name).await.unwrap();
}

/// Tests deleting a file.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_delete_file() {
    let client = get_client();

    // Upload a file
    let content = b"Test file to be deleted";
    let file = client
        .upload_file_bytes(content.to_vec(), "text/plain", Some("delete-test.txt"))
        .await
        .expect("Failed to upload file");

    // Delete the file
    client
        .delete_file(&file.name)
        .await
        .expect("Failed to delete file");

    // Verify it's gone by trying to get it (should fail)
    let result = client.get_file(&file.name).await;
    assert!(result.is_err(), "Getting deleted file should fail");
}

/// Tests using an uploaded file in an interaction.
/// Note: The Files API works with the generateContent API but may have limitations
/// with the Interactions API. This test validates the upload and API mechanics work,
/// but the model may not always access the file content properly.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_file_in_interaction() {
    let client = get_client();

    // Upload a text file with some content
    let content = b"The capital of France is Paris. The Eiffel Tower is 330 meters tall.";
    let file = client
        .upload_file_bytes(content.to_vec(), "text/plain", Some("facts.txt"))
        .await
        .expect("Failed to upload file");

    // Wait for file to be ready (text files should be quick)
    let ready_file = client
        .wait_for_file_ready(&file, Duration::from_secs(1), Duration::from_secs(30))
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
            .with_model("gemini-3.6-flash")
            .with_content(vec![
                Content::from_file(&ready_file),
                Content::text("What city is mentioned in this document?"),
            ])
            .create()
            .await
    })
    .expect("Interaction should succeed");

    // Verify we got a response - the model should return something
    let text = response.as_text().expect("Response should have text");
    assert!(!text.is_empty(), "Response should not be empty");
    // Note: The model may or may not have access to the file content depending on
    // the API's file URI handling. We verify the mechanics work without asserting
    // on specific content.

    // Clean up
    client.delete_file(&file.name).await.unwrap();
}

/// Tests that Content::from_file() correctly infers content type.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_content_from_file_type_inference() {
    use genai_rs::Content;

    let client = get_client();

    // Upload files with different MIME types
    let video_file = client
        .upload_file_bytes(b"fake video data".to_vec(), "video/mp4", Some("test.mp4"))
        .await
        .unwrap();

    let image_file = client
        .upload_file_bytes(b"fake image data".to_vec(), "image/png", Some("test.png"))
        .await
        .unwrap();

    let audio_file = client
        .upload_file_bytes(b"fake audio data".to_vec(), "audio/mp3", Some("test.mp3"))
        .await
        .unwrap();

    let doc_file = client
        .upload_file_bytes(
            b"fake pdf data".to_vec(),
            "application/pdf",
            Some("test.pdf"),
        )
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
        client.delete_file(&file.name).await.unwrap();
    }
}

/// Tests pagination when listing files.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_list_files_pagination() {
    let client = get_client();

    // Upload a few files
    let mut uploaded_files = Vec::new();
    for i in 0..3 {
        let file = client
            .upload_file_bytes(
                format!("Content {i}").into_bytes(),
                "text/plain",
                Some(&format!("paginate-{i}.txt")),
            )
            .await
            .expect("Failed to upload file");
        uploaded_files.push(file);
    }

    // List with page size of 1
    let first_page = client
        .list_files(Some(1), None)
        .await
        .expect("Failed to list files");

    assert!(
        !first_page.files.is_empty(),
        "First page should have at least one file"
    );

    // If there's a next page token, we can verify pagination works
    if let Some(token) = first_page.next_page_token {
        let _second_page = client
            .list_files(Some(1), Some(&token))
            .await
            .expect("Failed to list second page");

        // The request succeeding is sufficient validation - we don't assert on
        // content since other tests may have left files and the page size isn't
        // guaranteed to be exactly what we requested.
    }

    // Clean up
    for file in uploaded_files {
        client.delete_file(&file.name).await.unwrap();
    }
}

/// Tests the wait_for_file_ready function with an already active file.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_wait_for_file_ready_immediate() {
    let client = get_client();

    // Upload a small text file (should be immediately active)
    let file = client
        .upload_file_bytes(
            b"Small test content".to_vec(),
            "text/plain",
            Some("wait-test.txt"),
        )
        .await
        .expect("Failed to upload file");

    // Wait should return quickly for an already active file
    let ready = client
        .wait_for_file_ready(&file, Duration::from_millis(100), Duration::from_secs(10))
        .await
        .expect("File should become ready");

    assert!(ready.is_active());

    // Clean up
    client.delete_file(&file.name).await.unwrap();
}

/// Tests that wait_for_file_ready times out appropriately.
/// Note: This is a synthetic test since we can't easily create a file that stays processing.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_wait_for_file_ready_timeout() {
    // This test is more about verifying the timeout mechanism works
    // We can't easily test a real timeout without a file that processes slowly
    // So we just verify the function signature works correctly

    let client = get_client();

    // Upload and immediately wait with very short timeout
    let file = client
        .upload_file_bytes(
            b"Timeout test".to_vec(),
            "text/plain",
            Some("timeout-test.txt"),
        )
        .await
        .expect("Failed to upload file");

    // Text files are usually immediately active, so this should succeed
    let result = client
        .wait_for_file_ready(&file, Duration::from_millis(100), Duration::from_secs(5))
        .await;

    // Either it succeeds (file was active) or times out - both are valid
    match result {
        Ok(ready) => assert!(ready.is_active()),
        Err(e) => assert!(e.to_string().contains("Timeout")),
    }

    // Clean up
    let _ = client.delete_file(&file.name).await;
}

/// Tests that get_file returns an error for non-existent files.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_get_nonexistent_file_returns_error() {
    let client = get_client();

    // Try to get a file that doesn't exist
    let result = client.get_file("files/nonexistent_12345_xyz").await;

    assert!(result.is_err(), "Should return error for non-existent file");

    // Verify it's an API error (404-like)
    let err = result.unwrap_err();
    let err_string = err.to_string();
    assert!(
        err_string.contains("API error") || err_string.contains("not found"),
        "Error should indicate the file was not found: {}",
        err_string
    );
}

// =============================================================================
// Chunked Upload Tests
// =============================================================================

/// Tests chunked upload with a moderately sized file.
///
/// This test creates a file larger than the default chunk size to verify
/// the streaming mechanism works correctly across multiple chunks.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_upload_file_chunked() {
    let client = get_client();

    // Create a temporary file larger than the default chunk size (8MB)
    // Use 9MB to ensure at least 2 chunks are streamed
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("large_test.txt");

    // Generate 9MB of data
    let data: Vec<u8> = (0..9 * 1024 * 1024).map(|i| (i % 256) as u8).collect();
    std::fs::write(&file_path, &data).unwrap();

    // Upload using chunked method
    let (file, resumable_upload) = client
        .upload_file_chunked_with_mime(&file_path, "text/plain")
        .await
        .expect("Chunked upload failed");

    // Verify file metadata
    assert!(
        file.name.starts_with("files/"),
        "File name should start with 'files/'"
    );
    assert_eq!(file.mime_type, "text/plain");
    assert!(
        file.display_name.as_deref() == Some("large_test.txt"),
        "Display name should be the filename"
    );
    assert!(!file.uri.is_empty(), "URI should not be empty");

    // Verify file size if reported
    if let Some(size) = file.size_bytes_as_u64() {
        assert_eq!(
            size,
            data.len() as u64,
            "File size should match uploaded data"
        );
    }

    // Verify ResumableUpload metadata
    assert_eq!(
        resumable_upload.file_size(),
        data.len() as u64,
        "ResumableUpload should track file size"
    );
    assert_eq!(resumable_upload.mime_type(), "text/plain");
    assert!(
        !resumable_upload.upload_url().is_empty(),
        "Upload URL should not be empty"
    );

    // Clean up
    client
        .delete_file(&file.name)
        .await
        .expect("Failed to delete file");
}

/// Tests chunked upload with automatic MIME type detection.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_upload_file_chunked_auto_mime() {
    let client = get_client();

    // Create a temporary file with a known extension
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("test.mp4");

    // Write some fake video data (just bytes, won't actually play)
    let data = vec![0u8; 1024]; // 1KB fake video
    std::fs::write(&file_path, &data).unwrap();

    // Upload using chunked method with auto MIME detection
    let (file, _) = client
        .upload_file_chunked(&file_path)
        .await
        .expect("Chunked upload failed");

    // Verify MIME type was detected correctly
    assert_eq!(
        file.mime_type, "video/mp4",
        "MIME type should be auto-detected from extension"
    );
    assert_eq!(file.display_name.as_deref(), Some("test.mp4"));

    // Clean up
    client.delete_file(&file.name).await.unwrap();
}

/// Tests chunked upload with a custom chunk size.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_upload_file_chunked_custom_chunk_size() {
    let client = get_client();

    // Create a temporary file
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("chunk_test.txt");

    // Generate 5MB of data
    let data: Vec<u8> = (0..5 * 1024 * 1024).map(|i| (i % 256) as u8).collect();
    std::fs::write(&file_path, &data).unwrap();

    // Upload with a custom chunk size (1MB)
    let chunk_size = 1024 * 1024; // 1MB
    let (file, resumable_upload) = client
        .upload_file_chunked_with_options(&file_path, "text/plain", chunk_size)
        .await
        .expect("Chunked upload with custom chunk size failed");

    // Verify upload succeeded
    assert!(file.name.starts_with("files/"));
    assert_eq!(resumable_upload.file_size(), data.len() as u64);

    // Clean up
    client.delete_file(&file.name).await.unwrap();
}

/// Tests chunked upload validates empty files.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_upload_file_chunked_empty_file_error() {
    let client = get_client();

    // Create an empty temporary file
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("empty.txt");
    std::fs::write(&file_path, b"").unwrap();

    // Chunked upload should fail for empty files
    let result = client
        .upload_file_chunked_with_mime(&file_path, "text/plain")
        .await;

    assert!(result.is_err(), "Should fail for empty file");
    let err_string = result.unwrap_err().to_string();
    assert!(
        err_string.contains("empty"),
        "Error should mention empty file: {}",
        err_string
    );
}

/// Tests chunked upload with nonexistent file returns appropriate error.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_upload_file_chunked_nonexistent_file_error() {
    let client = get_client();

    // Try to stream a file that doesn't exist
    let result = client
        .upload_file_chunked_with_mime("/nonexistent/path/to/file.txt", "text/plain")
        .await;

    assert!(result.is_err(), "Should fail for nonexistent file");
    let err_string = result.unwrap_err().to_string();
    assert!(
        err_string.contains("Failed to access") || err_string.contains("No such file"),
        "Error should indicate file access failure: {}",
        err_string
    );
}

/// Tests that file uploaded via chunked method can be used in an interaction.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_chunked_upload_in_interaction() {
    let client = get_client();

    // Create a text file with content
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("interact.txt");
    let content =
        "The quick brown fox jumps over the lazy dog. This file was uploaded via chunked upload.";
    std::fs::write(&file_path, content).unwrap();

    // Upload using chunked method
    let (file, _) = client
        .upload_file_chunked_with_mime(&file_path, "text/plain")
        .await
        .expect("Chunked upload failed");

    // Wait for file to be ready
    let ready_file = client
        .wait_for_file_ready(&file, Duration::from_secs(1), Duration::from_secs(30))
        .await
        .expect("File should become ready");

    assert!(ready_file.is_active(), "File should be active");

    // Use in interaction
    let response = retry_request!([client, ready_file] => {
        client
            .interaction()
            .with_model("gemini-3.6-flash")
            .with_content(vec![
                Content::from_file(&ready_file),
                Content::text("What does the file say about a fox?"),
            ])
            .create()
            .await
    })
    .expect("Interaction should succeed");

    // Verify we got a response
    let text = response.as_text().expect("Response should have text");
    assert!(!text.is_empty(), "Response should not be empty");

    // Clean up
    client.delete_file(&file.name).await.unwrap();
}
