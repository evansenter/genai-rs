//! Integration tests for multimodal file loading functions.
//!
//! These tests serve dual purposes:
//! 1. Verify file loading, base64 encoding, and MIME type detection
//! 2. Validate that the Gemini API successfully accepts and processes the encoded content
//!
//! # Running Tests
//!
//! ```bash
//! cargo test --test temp_file_tests -- --include-ignored --nocapture
//! ```
//!
//! # Prerequisites
//!
//! - `GEMINI_API_KEY` environment variable must be set (exception:
//!   `example_fixtures_match_test_fixtures` needs no key and runs in the
//!   keyless unit-test CI pass)
//! - Tests create temporary files that are automatically cleaned up

mod common;

use base64::Engine;
use common::{
    TINY_BLUE_PNG_BASE64, TINY_MP4_BASE64, TINY_PDF_BASE64, TINY_RED_PNG_BASE64, TINY_WAV_BASE64,
    assert_response_semantic, get_client, stateful_builder,
};
use genai_rs::{
    Content, InteractionInput, InteractionStatus, audio_from_file, document_from_file,
    image_from_file, video_from_file,
};
use tempfile::TempDir;

// =============================================================================
// Image File Tests
// =============================================================================

/// Tests loading an image from a temp file using image_from_file().
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_image_from_temp_file() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    // Create temp directory and file
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let image_path = temp_dir.path().join("test_image.png");

    // Decode base64 and write to file
    let image_bytes = base64::engine::general_purpose::STANDARD
        .decode(TINY_RED_PNG_BASE64)
        .expect("Failed to decode base64");
    std::fs::write(&image_path, &image_bytes).expect("Failed to write image");

    // Load using image_from_file()
    let image_content = image_from_file(&image_path)
        .await
        .expect("Failed to load image from file");

    // Send to API
    let contents = vec![
        Content::text("What color is this image? Answer with just the color name."),
        image_content,
    ];

    let response = crate::retry_request!([client, contents] => {
        stateful_builder(&client)
            .with_input(InteractionInput::Content(contents))
            .create()
            .await
    })
    .expect("Image interaction failed");

    assert_eq!(response.status, InteractionStatus::Completed);
    assert!(response.has_text(), "Should have text response");

    let text = response.as_text().unwrap();
    println!("Color response: {}", text);

    // The tiny PNG is red - use semantic validation
    assert_response_semantic(
        &client,
        "Asked what color a red 1x1 pixel PNG image is",
        text,
        "Does this response identify the color as red or a shade of red (like pink, magenta)?",
    )
    .await;
}

/// Tests that image_from_file() handles mismatched content and extension.
///
/// This test writes PNG data with a .jpg extension to verify the API
/// processes the actual content regardless of the declared MIME type.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_image_mismatched_mime() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Write PNG data with .jpg extension - MIME will be image/jpeg but content is PNG
    let image_path = temp_dir.path().join("test_image.jpg");
    let image_bytes = base64::engine::general_purpose::STANDARD
        .decode(TINY_RED_PNG_BASE64)
        .expect("Failed to decode base64");
    std::fs::write(&image_path, &image_bytes).expect("Failed to write image");

    let image_content = image_from_file(&image_path)
        .await
        .expect("Failed to load image from file");

    let contents = vec![
        Content::text("Is this an image? Answer yes or no."),
        image_content,
    ];

    let response = crate::retry_request!([client, contents] => {
        stateful_builder(&client)
            .with_input(InteractionInput::Content(contents))
            .create()
            .await
    })
    .expect("Image interaction failed");

    assert_eq!(response.status, InteractionStatus::Completed);
    println!("Response: {:?}", response.as_text());
}

// =============================================================================
// Document File Tests (PDF)
// =============================================================================

/// Tests loading a PDF from a temp file using document_from_file().
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_pdf_from_temp_file() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let pdf_path = temp_dir.path().join("test_document.pdf");

    // Decode base64 and write to file
    let pdf_bytes = base64::engine::general_purpose::STANDARD
        .decode(TINY_PDF_BASE64)
        .expect("Failed to decode base64");
    std::fs::write(&pdf_path, &pdf_bytes).expect("Failed to write PDF");

    // Load using document_from_file()
    let doc_content = document_from_file(&pdf_path)
        .await
        .expect("Failed to load PDF from file");

    let contents = vec![
        Content::text("What text does this PDF contain? Answer with just the text."),
        doc_content,
    ];

    let response = crate::retry_request!([client, contents] => {
        stateful_builder(&client)
            .with_input(InteractionInput::Content(contents))
            .create()
            .await
    })
    .expect("PDF interaction failed");

    assert_eq!(response.status, InteractionStatus::Completed);
    assert!(response.has_text(), "Should have text response");

    let text = response.as_text().unwrap();
    println!("PDF response: {}", text);

    // The minimal PDF contains "Hello World" - use semantic validation
    assert_response_semantic(
        &client,
        "Asked about text in a PDF that contains 'Hello World'",
        text,
        "Does this response mention 'Hello' or 'World' or indicate those words were found?",
    )
    .await;
}

// =============================================================================
// Document File Tests (Text Formats)
// =============================================================================

/// Tests that document_from_file correctly rejects TXT files.
///
/// The Gemini Interactions API only supports PDF for document content type.
#[tokio::test]
async fn test_document_from_file_rejects_txt() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let txt_path = temp_dir.path().join("test.txt");

    std::fs::write(&txt_path, "Test content").expect("Failed to write TXT");

    let result = document_from_file(&txt_path).await;

    assert!(
        result.is_err(),
        "document_from_file should reject TXT files"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("text/plain") && err.contains("application/pdf"),
        "Error should mention text/plain and application/pdf: {}",
        err
    );
}

/// Tests sending plain text file content (the correct approach for text-based files).
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_txt_file_as_text_input() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let txt_path = temp_dir.path().join("test_document.txt");

    // Write plain text content
    let txt_content = "The quick brown fox jumps over the lazy dog.";
    std::fs::write(&txt_path, txt_content).expect("Failed to write TXT");

    // For text-based files, read the content and send as Content::text()
    let file_content = std::fs::read_to_string(&txt_path).expect("Failed to read TXT");

    let prompt = format!(
        "What animal jumps in this text? Answer with one word.\n\n{}",
        file_content
    );

    let response = crate::retry_request!([client, prompt] => {
        stateful_builder(&client)
            .with_text(&prompt)
            .create()
            .await
    })
    .expect("TXT interaction failed");

    assert_eq!(response.status, InteractionStatus::Completed);
    let text = response.as_text().unwrap();
    println!("TXT response: {}", text);

    // Use semantic validation - the text asks about which animal jumps
    assert_response_semantic(
        &client,
        "Asked which animal jumps in 'The quick brown fox jumps over the lazy dog'",
        text,
        "Does this response identify the fox as the jumping animal?",
    )
    .await;
}

// Note: JSON test removed - Gemini API does not support application/json MIME type
// for document inputs. The API returns 404 "No content type found for mime type: application/json"

/// Tests that document_from_file correctly rejects Markdown files.
///
/// The Gemini Interactions API only supports PDF for document content type.
#[tokio::test]
async fn test_document_from_file_rejects_markdown() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let md_path = temp_dir.path().join("README.md");

    std::fs::write(&md_path, "# Test").expect("Failed to write Markdown");

    let result = document_from_file(&md_path).await;

    assert!(
        result.is_err(),
        "document_from_file should reject Markdown files"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("text/markdown") && err.contains("application/pdf"),
        "Error should mention text/markdown and application/pdf: {}",
        err
    );
}

/// Tests sending Markdown file content as text (the correct approach for text-based files).
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_markdown_file_as_text_input() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let md_path = temp_dir.path().join("README.md");

    let md_content = r#"# Project Title

## Features
- Fast performance
- Easy to use
- Well documented
"#;
    std::fs::write(&md_path, md_content).expect("Failed to write Markdown");

    // For text-based files, read the content and send as Content::text()
    let file_content = std::fs::read_to_string(&md_path).expect("Failed to read Markdown");

    let prompt = format!(
        "How many features are listed in this markdown? Answer with just a number.\n\n{}",
        file_content
    );

    let response = crate::retry_request!([client, prompt] => {
        stateful_builder(&client)
            .with_text(&prompt)
            .create()
            .await
    })
    .expect("Markdown interaction failed");

    assert_eq!(response.status, InteractionStatus::Completed);
    let text = response.as_text().unwrap();
    println!("Markdown response: {}", text);

    // Use semantic validation for the count
    assert_response_semantic(
        &client,
        "Asked how many features are listed in a markdown file with 3 bullet points",
        text,
        "Does this response indicate there are 3 features (or 'three')?",
    )
    .await;
}

/// Tests that document_from_file correctly rejects non-PDF files.
///
/// The Gemini Interactions API only supports PDF for document content type.
/// For text-based files like CSV, the proper approach is to read the file
/// and send it as Content::text().
#[tokio::test]
async fn test_document_from_file_rejects_csv() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let csv_path = temp_dir.path().join("data.csv");

    let csv_content = "name,score\nAlice,95\nBob,87\nCarol,92";
    std::fs::write(&csv_path, csv_content).expect("Failed to write CSV");

    let result = document_from_file(&csv_path).await;

    assert!(
        result.is_err(),
        "document_from_file should reject CSV files"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("text/csv") && err.contains("application/pdf"),
        "Error should mention text/csv and application/pdf: {}",
        err
    );
}

/// Tests sending CSV data as text content (the correct approach for text-based files).
///
/// Since document_from_file only supports PDF, text-based files like CSV should
/// be read and sent as Content::text() instead.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_csv_file_as_text_input() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let csv_path = temp_dir.path().join("data.csv");

    let csv_content = "name,score\nAlice,95\nBob,87\nCarol,92";
    std::fs::write(&csv_path, csv_content).expect("Failed to write CSV");

    // For text-based files, read the content and send as Content::text()
    let file_content = std::fs::read_to_string(&csv_path).expect("Failed to read CSV");

    let prompt = format!(
        "Who has the highest score in this CSV? Answer with just the name.\n\n{}",
        file_content
    );

    let response = crate::retry_request!([client, prompt] => {
        stateful_builder(&client)
            .with_text(&prompt)
            .create()
            .await
    })
    .expect("CSV interaction failed");

    assert_eq!(response.status, InteractionStatus::Completed);
    let text = response.as_text().unwrap();
    println!("CSV response: {}", text);

    // Use semantic validation - Alice has score 95 (highest)
    assert_response_semantic(
        &client,
        "Asked who has highest score in CSV: Alice=95, Bob=87, Carol=92",
        text,
        "Does this response identify Alice as having the highest score?",
    )
    .await;
}

// =============================================================================
// Audio File Tests
// =============================================================================

/// Tests loading an audio file from a temp file using audio_from_file().
///
/// Note: We only verify `has_text()` rather than specific content because the tiny
/// synthetic audio file may not produce reliable content descriptions. The important
/// validation is that the API accepts and processes the audio format correctly.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_audio_from_temp_file() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let audio_path = temp_dir.path().join("test_audio.wav");

    let audio_bytes = base64::engine::general_purpose::STANDARD
        .decode(TINY_WAV_BASE64)
        .expect("Failed to decode base64");
    std::fs::write(&audio_path, &audio_bytes).expect("Failed to write audio");

    let audio_content = audio_from_file(&audio_path)
        .await
        .expect("Failed to load audio from file");

    let contents = vec![
        Content::text("Is this an audio file? Answer yes or no."),
        audio_content,
    ];

    let response = crate::retry_request!([client, contents] => {
        stateful_builder(&client)
            .with_input(InteractionInput::Content(contents))
            .create()
            .await
    })
    .expect("Audio interaction failed");

    assert_eq!(response.status, InteractionStatus::Completed);
    assert!(response.has_text(), "Should have text response");
    println!("Audio response: {:?}", response.as_text());
}

// =============================================================================
// Video File Tests
// =============================================================================

/// Tests loading a video file from a temp file using video_from_file().
///
/// Uses the TINY_MP4_BASE64 fixture (a real one-frame 64x64 H.264 clip, ~1.5KB),
/// so the API accepts it as valid video data.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_video_from_temp_file() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let video_path = temp_dir.path().join("test_video.mp4");

    let video_bytes = base64::engine::general_purpose::STANDARD
        .decode(TINY_MP4_BASE64)
        .expect("Failed to decode base64");
    std::fs::write(&video_path, &video_bytes).expect("Failed to write video");

    let video_content = video_from_file(&video_path)
        .await
        .expect("Failed to load video from file");

    let contents = vec![
        Content::text("Is this a video file? Answer yes or no."),
        video_content,
    ];

    let response = crate::retry_request!([client, contents] => {
        stateful_builder(&client)
            .with_input(InteractionInput::Content(contents))
            .create()
            .await
    })
    .expect("Video interaction failed");

    assert_eq!(response.status, InteractionStatus::Completed);
    assert!(response.has_text(), "Should have text response");
    println!("Video response: {:?}", response.as_text());
}

// =============================================================================
// Fixture Drift Guard
// =============================================================================

/// Guards against example fixture copies drifting from the canonical test
/// fixtures. Examples embed verbatim copies of these constants and are never
/// run in CI, so a fixture update that misses one silently sends that
/// example down its error branch. Runs without an API key.
///
/// Self-maintaining: walks examples/**/*.rs for `_BASE64: <type> = <str>`
/// declarations and extracts the quoted literal, so it also catches raw
/// strings, `&'static str`, a literal rustfmt moved to its own line, and
/// stale copies in *new* examples that follow the naming convention. The
/// `checked` floor and the every-canonical-seen check below backstop the
/// matcher itself: reformatting or removing a known copy fails here.
#[test]
fn example_fixtures_match_test_fixtures() {
    let canonical = [
        ("TINY_WAV_BASE64", TINY_WAV_BASE64),
        ("TINY_MP4_BASE64", TINY_MP4_BASE64),
        ("TINY_PDF_BASE64", TINY_PDF_BASE64),
        ("TINY_RED_PNG_BASE64", TINY_RED_PNG_BASE64),
        ("TINY_BLUE_PNG_BASE64", TINY_BLUE_PNG_BASE64),
    ];
    let examples_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/examples");
    let mut checked = 0;
    let mut seen = std::collections::HashSet::new();
    let mut stack = vec![std::path::PathBuf::from(examples_dir)];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read examples dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let src = std::fs::read_to_string(&path).expect("read example");
                let mut from = 0;
                while let Some(hit) = src[from..].find("_BASE64") {
                    let after = from + hit + "_BASE64".len();
                    from = after;
                    // Declarations look like `X_BASE64: <type> = <literal>`:
                    // a colon, then an `=` before the opening quote, all
                    // within a short span. Format-string interpolation
                    // (`{X_BASE64:?}`) and doc-comment mentions have the
                    // colon but not the `= ... "` shape, so they're skipped.
                    let tail = &src[after..];
                    if !tail.trim_start().starts_with(':') {
                        continue;
                    }
                    let window_end = tail.char_indices().nth(200).map_or(tail.len(), |(i, _)| i);
                    let window = &tail[..window_end];
                    let (Some(eq), Some(q1)) = (window.find('='), window.find('"')) else {
                        continue;
                    };
                    if eq > q1 {
                        continue;
                    }
                    // Base64 contains no quotes or escapes, so the literal is
                    // exactly the span between the next two quote characters
                    // (works for both "..." and r#"..."# forms).
                    let lit = &tail[q1 + 1..];
                    let q2 = lit.find('"').expect("unterminated string literal");
                    let value = &lit[..q2];
                    let matched = canonical.iter().find(|(_, v)| *v == value);
                    assert!(
                        matched.is_some(),
                        "{} embeds a base64 fixture this guard doesn't recognize: {}... — \
                         if it's a copy of a tests/common fixture it has drifted; if it's \
                         a deliberately new fixture, add it to this guard's canonical list",
                        path.display(),
                        value.chars().take(40).collect::<String>()
                    );
                    seen.insert(matched.expect("asserted above").0);
                    checked += 1;
                }
            }
        }
    }
    assert!(
        checked >= 5,
        "expected at least the five known fixture copies, found {checked} — \
         did the declaration shape change?"
    );
    for (name, _) in &canonical {
        assert!(
            seen.contains(name),
            "no example embeds {name} — if the example that used it was \
             removed or switched fixtures, update this guard's canonical list"
        );
    }
}

// =============================================================================
// Error Handling Tests
// =============================================================================

/// Tests that image_from_file() returns appropriate error for missing file.
#[tokio::test]
async fn test_image_from_file_not_found() {
    let result = image_from_file("/nonexistent/path/image.png").await;

    assert!(result.is_err(), "Should return error for missing file");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Failed to read file") || err.contains("No such file"),
        "Error should mention file not found: {}",
        err
    );
}

/// Tests that document_from_file() returns appropriate error for unsupported extension.
#[tokio::test]
async fn test_document_from_file_unsupported_extension() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let docx_path = temp_dir.path().join("document.docx");
    std::fs::write(&docx_path, "fake content").expect("Failed to write file");

    let result = document_from_file(&docx_path).await;

    assert!(
        result.is_err(),
        "Should return error for unsupported extension"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Unsupported document extension"),
        "Error should mention unsupported extension: {}",
        err
    );
}

/// Tests that image_from_file() returns appropriate error for wrong category.
#[tokio::test]
async fn test_image_from_file_wrong_category() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let mp3_path = temp_dir.path().join("audio.mp3");
    std::fs::write(&mp3_path, "fake content").expect("Failed to write file");

    let result = image_from_file(&mp3_path).await;

    assert!(
        result.is_err(),
        "Should return error for audio file used as image"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("not an image type") || err.contains("audio_from_file"),
        "Error should suggest correct function: {}",
        err
    );
}
