//! Multimodal input tests for the Interactions API
//!
//! Tests for image, audio, video, and mixed media inputs.
//!
//! These tests require the GEMINI_API_KEY environment variable to be set.
//!
//! This file is organized by input type:
//!
//! - **image**: Image input from URI and base64, multiple images, follow-up
//! - **audio**: Audio input from URI and base64
//! - **video**: Video input from URI and base64
//! - **mixed_content**: Text and image interleaved, image comparison
//! - **mixed_media**: Multiple media types (image + audio, all three)
//! - **document**: PDF document input
//! - **streaming**: Streaming with multimodal input
//! - **file_loading**: Builder pattern add_image_file() methods
//! - **bytes_loading**: Builder pattern add_*_bytes() methods
//! - **text_to_speech**: Audio output generation
//!
//! # Running Tests
//!
//! ```bash
//! cargo test --test multimodal_tests -- --include-ignored --nocapture
//! ```
//!
//! # Test Assets
//!
//! Tests primarily use base64-encoded media since the Interactions API does NOT
//! support Google Cloud Storage (gs://) URIs. URI-based tests gracefully handle
//! the expected "unsupported file uri" error.

mod common;

mod image {
    use crate::common::{
        SAMPLE_IMAGE_URL, TINY_BLUE_PNG_BASE64, TINY_RED_PNG_BASE64, assert_response_semantic,
        get_client, stateful_builder, test_timeout, with_timeout,
    };
    use genai_rs::{Content, InteractionInput, InteractionStatus};

    /// Tests image input via GCS URI (gs://) which may not be supported by the Interactions API.
    /// This test documents the expected "Unsupported file uri" error behavior when the API rejects
    /// such URIs, but also handles success gracefully if the API accepts the format.
    /// For reliable image input, use base64 encoding (see `test_image_input_from_base64`).
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_image_input_gcs_uri_unsupported() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let contents = vec![
            Content::text("What is in this image? Describe it briefly in 1-2 sentences."),
            Content::image_uri(SAMPLE_IMAGE_URL, "image/jpeg"),
        ];

        let result = stateful_builder(&client)
            .with_input(InteractionInput::Content(contents))
            .create()
            .await;

        match result {
            Ok(response) => {
                assert_eq!(response.status, InteractionStatus::Completed);
                assert!(
                    response.has_text(),
                    "Should have text response describing image"
                );
                let text = response.as_text().unwrap().to_lowercase();
                println!("Image description: {}", text);
            }
            Err(e) => {
                // GCS URIs are expected to fail with a 400-class error.
                // Don't check specific message - API error text changes over time.
                match &e {
                    genai_rs::GenaiError::Api { status_code, .. } if *status_code == 400 => {
                        println!(
                            "Expected: GCS URIs not supported directly (400 error). Use base64 encoding or FileService.RegisterFile."
                        );
                    }
                    _ => panic!("Unexpected error type: {:?}", e),
                }
            }
        }
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_image_input_from_base64() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Use tiny red PNG for testing base64 input
        let contents = vec![
            Content::text("What color is this image? Answer with just the color name."),
            Content::image_data(TINY_RED_PNG_BASE64, "image/png"),
        ];

        let response = stateful_builder(&client)
            .with_input(InteractionInput::Content(contents))
            .create()
            .await
            .expect("Base64 image interaction failed");

        assert_eq!(response.status, InteractionStatus::Completed);
        assert!(response.has_text(), "Should have text response");

        let text = response.as_text().unwrap();
        println!("Color response: {}", text);

        // The tiny PNG is red
        assert_response_semantic(
            &client,
            "Showed a red 1x1 pixel image and asked what color it is",
            text,
            "Does this response identify the color as red or a shade of red (like pink, magenta, crimson)?",
        )
        .await;
    }

    /// Tests multiple images in a single request using base64.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_multiple_images_single_request() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Send two images in a single request (both base64)
        let contents = vec![
            Content::text(
                "I'm showing you two small colored images. What colors are they? List both.",
            ),
            Content::image_data(TINY_RED_PNG_BASE64, "image/png"),
            Content::image_data(TINY_BLUE_PNG_BASE64, "image/png"),
        ];

        let response = stateful_builder(&client)
            .with_input(InteractionInput::Content(contents))
            .create()
            .await
            .expect("Multiple images interaction failed");

        assert_eq!(response.status, InteractionStatus::Completed);
        assert!(response.has_text(), "Should have text response");

        let text = response.as_text().unwrap();
        println!("Multiple images response: {}", text);

        // Should mention the colors from both images
        assert_response_semantic(
            &client,
            "Showed two images (one red, one blue) and asked to describe the colors",
            text,
            "Does this response mention at least one of the colors red or blue?",
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_image_with_follow_up_question() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(test_timeout(), async {
            // First turn: describe the base64 image
            let contents = vec![
                Content::text("What color is this image?"),
                Content::image_data(TINY_RED_PNG_BASE64, "image/png"),
            ];

            let response1 = stateful_builder(&client)
                .with_input(InteractionInput::Content(contents))
                .create()
                .await
                .expect("First interaction failed");

            assert_eq!(response1.status, InteractionStatus::Completed);
            println!("First response: {:?}", response1.as_text());

            // Second turn: ask follow-up about the same image
            let response2 = stateful_builder(&client)
                .with_previous_interaction(response1.id.as_ref().expect("id should exist"))
                .with_text("Is that a warm or cool color?")
                .create()
                .await
                .expect("Follow-up interaction failed");

            assert_eq!(response2.status, InteractionStatus::Completed);
            assert!(response2.has_text(), "Should have follow-up response");

            let text = response2.as_text().unwrap();
            println!("Follow-up response: {}", text);

            // Red is a warm color - use semantic validation
            assert_response_semantic(
                &client,
                "Previous turn discussed a red image. Asked if the color is warm or cool.",
                text,
                "Does this response identify the color as warm (or mention red/hot)?",
            )
            .await;
        })
        .await;
    }
}

mod audio {
    use crate::common::{SAMPLE_AUDIO_URL, TINY_WAV_BASE64, get_client, stateful_builder};
    use genai_rs::{Content, InteractionInput};

    /// Tests audio input from URI.
    /// Note: GCS URIs are not supported by the Interactions API.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_audio_input_from_uri() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let contents = vec![
            Content::text("What is this audio about? Summarize briefly."),
            Content::audio_uri(SAMPLE_AUDIO_URL, "audio/mpeg"),
        ];

        let result = stateful_builder(&client)
            .with_input(InteractionInput::Content(contents))
            .create()
            .await;

        match result {
            Ok(response) => {
                println!("Audio response status: {:?}", response.status);
                if response.has_text() {
                    let text = response.as_text().unwrap();
                    println!("Audio transcription/summary: {}", text);
                }
            }
            Err(e) => {
                let error_str = format!("{:?}", e);
                if error_str.contains("Unsupported file uri") {
                    println!(
                        "Expected: GCS URIs not supported by Interactions API. Use base64 encoding instead."
                    );
                } else {
                    println!("Audio input error (may be expected): {:?}", e);
                }
            }
        }
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_audio_input_from_base64() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Use tiny WAV for testing base64 audio input
        // Note: This is a minimal header with no actual audio, so the model may report it's empty/silent
        let contents = vec![
            Content::text("Describe what you hear in this audio file."),
            genai_rs::Content::audio_data(TINY_WAV_BASE64, "audio/wav"),
        ];

        let result = stateful_builder(&client)
            .with_input(InteractionInput::Content(contents))
            .create()
            .await;

        match result {
            Ok(response) => {
                println!("Base64 audio response status: {:?}", response.status);
                if response.has_text() {
                    let text = response.as_text().unwrap();
                    println!("Audio response: {}", text);
                    // Just verify we got some response - the content can vary
                    assert!(!text.is_empty(), "Should get some response about the audio");
                }
            }
            Err(e) => {
                println!(
                    "Base64 audio error (may be expected for minimal WAV): {:?}",
                    e
                );
            }
        }
    }
}

mod video {
    use crate::common::{SAMPLE_VIDEO_URL, TINY_MP4_BASE64, get_client, stateful_builder};
    use genai_rs::{Content, InteractionInput};

    /// Tests video input from URI.
    /// Note: GCS URIs are not supported by the Interactions API.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_video_input_from_uri() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let contents = vec![
            Content::text("What animals appear in this video? List them."),
            Content::video_uri(SAMPLE_VIDEO_URL, "video/mp4"),
        ];

        let result = stateful_builder(&client)
            .with_input(InteractionInput::Content(contents))
            .create()
            .await;

        match result {
            Ok(response) => {
                println!("Video response status: {:?}", response.status);
                if response.has_text() {
                    let text = response.as_text().unwrap();
                    println!("Video description: {}", text);
                }
            }
            Err(e) => {
                let error_str = format!("{:?}", e);
                if error_str.contains("Unsupported file uri") {
                    println!(
                        "Expected: GCS URIs not supported by Interactions API. Use base64 encoding instead."
                    );
                } else {
                    println!("Video input error (may be expected): {:?}", e);
                }
            }
        }
    }

    /// Tests video input from base64.
    /// Note: This uses a minimal MP4 header, so the model may report it's empty/corrupt.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_video_input_from_base64() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Use minimal MP4 for testing base64 video input
        // Note: This is a minimal header with no actual video frames, so the model may report it's empty
        let contents = vec![
            Content::text("Describe what you see in this video file."),
            Content::video_data(TINY_MP4_BASE64, "video/mp4"),
        ];

        let result = stateful_builder(&client)
            .with_input(InteractionInput::Content(contents))
            .create()
            .await;

        match result {
            Ok(response) => {
                println!("Base64 video response status: {:?}", response.status);
                if response.has_text() {
                    let text = response.as_text().unwrap();
                    println!("Video response: {}", text);
                    // Just verify we got some response - the content can vary
                    assert!(!text.is_empty(), "Should get some response about the video");
                }
            }
            Err(e) => {
                // A minimal MP4 header may not be accepted by the API
                println!(
                    "Base64 video error (may be expected for minimal MP4): {:?}",
                    e
                );
            }
        }
    }
}

mod mixed_content {
    use crate::common::{
        TINY_BLUE_PNG_BASE64, TINY_RED_PNG_BASE64, assert_response_semantic, get_client,
        stateful_builder,
    };
    use genai_rs::{Content, InteractionInput, InteractionStatus};

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_multimodal_text_and_image_interleaved() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Interleave text and base64 image content
        let contents = vec![
            Content::text("I'm going to show you an image."),
            Content::image_data(TINY_RED_PNG_BASE64, "image/png"),
            Content::text("Based on the color above, what emotion might it represent?"),
        ];

        let response = stateful_builder(&client)
            .with_input(InteractionInput::Content(contents))
            .create()
            .await
            .expect("Interleaved content interaction failed");

        assert_eq!(response.status, InteractionStatus::Completed);
        assert!(response.has_text(), "Should have text response");

        let text = response.as_text().unwrap();
        println!("Interleaved content response: {}", text);

        // Red is associated with passion, anger, love, energy - use semantic validation
        assert_response_semantic(
            &client,
            "Showed a red image and asked what emotion it might represent",
            text,
            "Does this response discuss emotions or feelings commonly associated with red (like passion, anger, love, energy, warmth, or intensity)?",
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_multimodal_comparison() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Ask model to compare two base64 images
        let contents = vec![
            Content::text(
                "Compare these two colored squares. What are their colors and how do they differ?",
            ),
            Content::image_data(TINY_RED_PNG_BASE64, "image/png"),
            Content::image_data(TINY_BLUE_PNG_BASE64, "image/png"),
        ];

        let response = stateful_builder(&client)
            .with_input(InteractionInput::Content(contents))
            .create()
            .await
            .expect("Comparison interaction failed");

        assert_eq!(response.status, InteractionStatus::Completed);
        assert!(response.has_text(), "Should have text response");

        let text = response.as_text().unwrap();
        println!("Comparison response: {}", text);

        // Should mention differences or colors - use semantic validation
        assert_response_semantic(
            &client,
            "Showed two colored squares (red and blue) and asked to compare them",
            text,
            "Does this response compare two colors or mention that the images are different?",
        )
        .await;
    }
}

mod mixed_media {
    use crate::common::{
        TINY_MP4_BASE64, TINY_RED_PNG_BASE64, TINY_WAV_BASE64, assert_response_semantic,
        get_client, stateful_builder,
    };
    use genai_rs::{Content, InteractionInput, InteractionStatus};

    /// Tests combining multiple media types (image + audio) in a single interaction.
    ///
    /// This is an **enforcing test** that expects the API to successfully process
    /// image + audio together. It asserts on the response content when successful,
    /// but allows known format errors from the minimal test files.
    ///
    /// Note: Video is excluded because the minimal MP4 test file often fails validation.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_mixed_image_and_audio() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Combine image and audio with a question about both
        let contents = vec![
            Content::text(
                "I'm sending you an image and an audio file. \
                 For the image, tell me what color it is. \
                 For the audio, describe what kind of audio file it appears to be. \
                 Keep your response brief.",
            ),
            Content::image_data(TINY_RED_PNG_BASE64, "image/png"),
            Content::audio_data(TINY_WAV_BASE64, "audio/wav"),
        ];

        let result = stateful_builder(&client)
            .with_input(InteractionInput::Content(contents))
            .create()
            .await;

        match result {
            Ok(response) => {
                assert_eq!(
                    response.status,
                    InteractionStatus::Completed,
                    "Mixed media interaction should complete"
                );
                assert!(response.has_text(), "Should have text response");

                let text = response.as_text().unwrap();
                println!("Mixed media response: {}", text);

                // Verify the model acknowledged at least one input using semantic validation
                // Note: Minimal test files may not provide enough data for the model to analyze both
                assert_response_semantic(
                    &client,
                    "Sent a red image and a WAV audio file, asked to describe both",
                    text,
                    "Does this response mention anything about an image (color, red) OR audio (sound, silent, empty)?",
                )
                .await;
            }
            Err(e) => {
                // The minimal test files might not be fully valid
                let error_str = format!("{:?}", e);
                println!(
                    "Mixed media error (may be expected for minimal files): {}",
                    error_str
                );
                // Don't fail the test for format errors with minimal test files
                assert!(
                    error_str.contains("format")
                        || error_str.contains("invalid")
                        || error_str.contains("empty")
                        || error_str.contains("audio"),
                    "Unexpected error: {}",
                    error_str
                );
            }
        }
    }

    /// Tests combining all three media types: image, audio, and video.
    ///
    /// This is an **exploratory test** that documents API behavior rather than enforcing
    /// specific outcomes. It may fail due to the minimal test files not being fully valid.
    ///
    /// - **Success**: Indicates the API accepts all three media types together
    /// - **Failure**: Documents which media types cause issues (helps debugging)
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_mixed_image_audio_video() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Combine all three media types
        let contents = vec![
            Content::text(
                "I'm sending you an image, an audio file, and a video file. \
                 Please briefly acknowledge each one.",
            ),
            Content::image_data(TINY_RED_PNG_BASE64, "image/png"),
            Content::audio_data(TINY_WAV_BASE64, "audio/wav"),
            Content::video_data(TINY_MP4_BASE64, "video/mp4"),
        ];

        let result = stateful_builder(&client)
            .with_input(InteractionInput::Content(contents))
            .create()
            .await;

        match result {
            Ok(response) => {
                println!("All media types response status: {:?}", response.status);
                if response.has_text() {
                    let text = response.as_text().unwrap();
                    println!("All media types response: {}", text);
                }
                // If we get here, the API accepted all three types
                assert_eq!(response.status, InteractionStatus::Completed);
            }
            Err(e) => {
                // The minimal test files are very likely to fail validation
                let error_str = format!("{:?}", e);
                println!(
                    "All media types error (expected for minimal files): {}",
                    error_str
                );
                // This test documents the API behavior with minimal files
                // A passing result would indicate the API accepted the format
            }
        }
    }
}

mod document {
    use crate::common::{TINY_PDF_BASE64, assert_response_semantic, get_client, stateful_builder};
    use genai_rs::{Content, InteractionInput, InteractionStatus};

    /// Tests PDF document input from base64.
    /// This tests the ability to send PDF documents to the model for analysis.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_pdf_document_input_from_base64() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Use minimal PDF containing "Hello World" text
        let contents = vec![
            Content::text(
                "What text does this PDF document contain? Answer with just the text you find.",
            ),
            Content::document_data(TINY_PDF_BASE64, "application/pdf"),
        ];

        let result = stateful_builder(&client)
            .with_input(InteractionInput::Content(contents))
            .create()
            .await;

        match result {
            Ok(response) => {
                println!("PDF document response status: {:?}", response.status);
                assert_eq!(response.status, InteractionStatus::Completed);
                if response.has_text() {
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
            }
            Err(e) => {
                // The minimal PDF might not be fully valid or the API might have restrictions
                println!(
                    "PDF document error (may be expected for minimal PDF): {:?}",
                    e
                );
            }
        }
    }

    /// Tests combining PDF document with text question.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_pdf_with_question() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let contents = vec![
            Content::text("I'm sending you a PDF document."),
            Content::document_data(TINY_PDF_BASE64, "application/pdf"),
            Content::text("Is this a valid PDF? What can you tell me about its structure?"),
        ];

        let result = stateful_builder(&client)
            .with_input(InteractionInput::Content(contents))
            .create()
            .await;

        match result {
            Ok(response) => {
                assert_eq!(response.status, InteractionStatus::Completed);
                assert!(response.has_text(), "Should have text response");
                let text = response.as_text().unwrap();
                println!("PDF question response: {}", text);
                // Should mention something about the PDF - use semantic validation
                assert_response_semantic(
                    &client,
                    "Sent a PDF and asked if it's valid and about its structure",
                    text,
                    "Does this response discuss the PDF, document structure, or pages?",
                )
                .await;
            }
            Err(e) => {
                println!("PDF with question error (may be expected): {:?}", e);
            }
        }
    }
}

mod streaming {
    use crate::common::{
        TINY_RED_PNG_BASE64, assert_response_semantic, consume_stream, get_client,
        interaction_builder, test_timeout, with_timeout,
    };
    use genai_rs::{Content, InteractionInput, InteractionStatus};

    /// Test streaming with multimodal (image) input.
    ///
    /// This validates that:
    /// - Streaming works correctly when images are part of the input
    /// - Text deltas are received incrementally
    /// - Final response correctly describes the image
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_multimodal_streaming() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(test_timeout(), async {
            println!("=== Multimodal + Streaming ===");

            // Create content with text and image
            let contents = vec![
                Content::text("What color is this image? Answer in one word."),
                Content::image_data(TINY_RED_PNG_BASE64, "image/png"),
            ];

            // Stream the response using with_input for multimodal content
            let stream = interaction_builder(&client)
                .with_input(InteractionInput::Content(contents))
                .create_stream();

            let result = consume_stream(stream).await;

            println!("\nDelta count: {}", result.delta_count);
            println!("Collected text: {}", result.collected_text);

            // Verify streaming worked
            assert!(
                result.has_output(),
                "Should receive streaming chunks or final response"
            );

            // Verify content describes the red image
            let text_to_check = if !result.collected_text.is_empty() {
                result.collected_text.clone()
            } else if let Some(ref response) = result.final_response {
                response.as_text().unwrap_or_default().to_string()
            } else {
                String::new()
            };

            // Use semantic validation for the color check
            assert_response_semantic(
                &client,
                "Asked what color a red 1x1 pixel image is",
                &text_to_check,
                "Does this response identify the color as red or a shade of red?",
            )
            .await;

            // Verify final response if present
            if let Some(ref response) = result.final_response {
                assert_eq!(
                    response.status,
                    InteractionStatus::Completed,
                    "Final response should be completed"
                );
            }

            println!("\n✓ Multimodal + streaming completed successfully");
        })
        .await;
    }
}

mod file_loading {
    use crate::common::{
        TINY_BLUE_PNG_BASE64, TINY_RED_PNG_BASE64, assert_response_semantic, get_client,
    };
    use base64::Engine;
    use genai_rs::{Content, InteractionStatus, image_from_file};

    /// Tests loading images from files with image_from_file() helper.
    ///
    /// This validates loading images from files using the image_from_file() helper,
    /// which auto-detects MIME type from the file extension and base64 encodes the content.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_add_image_file_builder() {
        use tempfile::TempDir;

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

        // Use image_from_file() helper to load and encode, then with_content()
        let image_content = image_from_file(&image_path)
            .await
            .expect("Failed to load image file");
        let response = client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_content(vec![
                Content::text("What color is this image? Answer with just the color name."),
                image_content,
            ])
            .create()
            .await
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

    /// Tests loading multiple images from files.
    ///
    /// Validates that with_content() correctly handles multiple images loaded
    /// using image_from_file() helpers.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_add_multiple_image_files_builder() {
        use tempfile::TempDir;

        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Create two image files
        let red_path = temp_dir.path().join("red.png");
        let blue_path = temp_dir.path().join("blue.png");

        let red_bytes = base64::engine::general_purpose::STANDARD
            .decode(TINY_RED_PNG_BASE64)
            .expect("Failed to decode red PNG");
        std::fs::write(&red_path, &red_bytes).expect("Failed to write red image");

        let blue_bytes = base64::engine::general_purpose::STANDARD
            .decode(TINY_BLUE_PNG_BASE64)
            .expect("Failed to decode blue PNG");
        std::fs::write(&blue_path, &blue_bytes).expect("Failed to write blue image");

        // Load multiple images with image_from_file() and combine with with_content()
        let red_content = image_from_file(&red_path)
            .await
            .expect("Failed to load red image");
        let blue_content = image_from_file(&blue_path)
            .await
            .expect("Failed to load blue image");
        let response = client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_content(vec![
                Content::text(
                    "I'm showing you two small colored images. What colors are they? List both.",
                ),
                red_content,
                blue_content,
            ])
            .create()
            .await
            .expect("Multiple images interaction failed");

        assert_eq!(response.status, InteractionStatus::Completed);
        assert!(response.has_text(), "Should have text response");

        let text = response.as_text().unwrap();
        println!("Multiple images response: {}", text);

        // Should mention at least one color - use semantic validation
        assert_response_semantic(
            &client,
            "Showed two images (red and blue) and asked to list both colors",
            text,
            "Does this response mention at least one color (red, blue, pink, etc.)?",
        )
        .await;
    }

    /// Tests image_from_file() error handling for missing file.
    #[tokio::test]
    async fn test_image_from_file_not_found() {
        // This test doesn't require an API key - just tests local file loading error
        let result = image_from_file("/nonexistent/path/image.png").await;

        assert!(result.is_err(), "Should return error for missing file");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Failed to read file") || err.contains("No such file"),
            "Error should mention file not found: {}",
            err
        );
    }
}

mod bytes_loading {
    use crate::common::{
        TINY_MP4_BASE64, TINY_PDF_BASE64, TINY_RED_PNG_BASE64, TINY_WAV_BASE64,
        assert_response_semantic, get_client, validate_response_semantically,
    };
    use genai_rs::{Content, InteractionStatus};

    /// Tests image input with base64-encoded data using Content::image_data().
    ///
    /// This validates that base64-encoded image data works correctly.
    /// Uses semantic validation to verify the model correctly interprets the image.
    ///
    /// Note: This test uses `.expect()` (strict assertion) because the PNG fixture
    /// is a complete, well-formed image that the API should always accept.
    /// Compare to audio/video tests which use lenient `match result` because those
    /// minimal fixtures may be rejected by the API.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_image_data_roundtrip() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Use Content::image_data() with base64-encoded data
        // The tiny PNG is well-formed and should always be processable
        let response = client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_content(vec![
                Content::text("What color is this image? Answer with just the color name."),
                Content::image_data(TINY_RED_PNG_BASE64, "image/png"),
            ])
            .create()
            .await
            .expect("Image data interaction failed");

        assert_eq!(response.status, InteractionStatus::Completed);
        assert!(response.has_text(), "Should have text response");

        let text = response.as_text().unwrap();
        println!("Color response: {}", text);

        // Use semantic validation instead of brittle content checks
        assert_response_semantic(
            &client,
            "User asked about the color of a 1x1 red PNG image",
            text,
            "Does this response describe a red, pink, magenta, or similar warm color?",
        )
        .await;
    }

    /// Tests audio input with base64-encoded data using Content::audio_data().
    ///
    /// This validates that base64-encoded audio data works correctly.
    /// Note: The minimal WAV test file may not contain actual audio, so the model
    /// may report it's empty/silent.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_audio_data_roundtrip() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Use Content::audio_data() with base64-encoded data
        let result = client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_content(vec![
                Content::text("Describe what you hear in this audio file."),
                Content::audio_data(TINY_WAV_BASE64, "audio/wav"),
            ])
            .create()
            .await;

        match result {
            Ok(response) => {
                println!("Audio bytes response status: {:?}", response.status);
                if response.has_text() {
                    let text = response.as_text().unwrap();
                    println!("Audio response: {}", text);
                    // Just verify we got some response - the content can vary
                    assert!(!text.is_empty(), "Should get some response about the audio");
                }
            }
            Err(e) => {
                // The minimal WAV might not be accepted
                println!(
                    "Audio bytes error (may be expected for minimal WAV): {:?}",
                    e
                );
            }
        }
    }

    /// Tests video input with base64-encoded data using Content::video_data().
    ///
    /// This validates that base64-encoded video data works correctly.
    /// Note: The minimal MP4 test file is just a container header with no frames,
    /// so the model may report it's empty/corrupt.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_video_data_roundtrip() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Use Content::video_data() with base64-encoded data
        let result = client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_content(vec![
                Content::text("Describe what you see in this video file."),
                Content::video_data(TINY_MP4_BASE64, "video/mp4"),
            ])
            .create()
            .await;

        match result {
            Ok(response) => {
                println!("Video bytes response status: {:?}", response.status);
                if response.has_text() {
                    let text = response.as_text().unwrap();
                    println!("Video response: {}", text);
                    // Just verify we got some response - the content can vary
                    assert!(!text.is_empty(), "Should get some response about the video");
                }
            }
            Err(e) => {
                // The minimal MP4 might not be accepted
                println!(
                    "Video bytes error (may be expected for minimal MP4): {:?}",
                    e
                );
            }
        }
    }

    /// Tests document input with base64-encoded data using Content::document_data().
    ///
    /// This validates that base64-encoded document data (PDF) works correctly.
    /// The test PDF contains "Hello World" text.
    /// Uses semantic validation to verify the model correctly interprets the document.
    ///
    /// Note: Like audio/video tests, this uses lenient error handling because the
    /// minimal PDF fixture or the semantic validation call might fail.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_document_data_roundtrip() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Use Content::document_data() with base64-encoded data
        let result = client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_content(vec![
                Content::text(
                    "What text does this PDF document contain? Answer with just the text you find.",
                ),
                Content::document_data(TINY_PDF_BASE64, "application/pdf"),
            ])
            .create()
            .await;

        match result {
            Ok(response) => {
                println!("PDF bytes response status: {:?}", response.status);
                assert_eq!(response.status, InteractionStatus::Completed);
                assert!(response.has_text(), "Should have text response");

                let text = response.as_text().unwrap();
                println!("PDF response: {}", text);

                // Use semantic validation instead of brittle content checks
                // Handle validation failure gracefully since it makes an additional API call
                match validate_response_semantically(
                    &client,
                    "User asked about text in a PDF that contains 'Hello World'",
                    text,
                    "Does this response mention 'Hello', 'World', or indicate these words were found in the document?",
                )
                .await
                {
                    Ok(is_valid) => {
                        assert!(
                            is_valid,
                            "Response should mention the PDF content: {}",
                            text
                        );
                    }
                    Err(e) => {
                        println!("Semantic validation error (non-fatal): {:?}", e);
                    }
                }
            }
            Err(e) => {
                // The minimal PDF might not be fully valid
                println!("PDF bytes error (may be expected for minimal PDF): {:?}", e);
            }
        }
    }
}

mod text_to_speech {
    use crate::common::{extended_test_timeout, get_client, with_timeout};

    /// Tests basic text-to-speech audio output
    #[tokio::test]
    #[ignore = "Requires API key and TTS model access"]
    async fn test_text_to_speech_basic() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // TTS requires a specific model
        let tts_model = "gemini-2.5-pro-preview-tts";

        // TTS can be slow - use extended timeout
        with_timeout(extended_test_timeout(), async {
            let response = client
                .interaction()
                .with_model(tts_model)
                .with_text("Hello, world!")
                .with_audio_output()
                .with_voice("Kore")
                .create()
                .await;

            match response {
                Ok(r) => {
                    println!("TTS response status: {:?}", r.status);
                    assert!(r.has_audio(), "Response should contain audio output");

                    if let Some(audio) = r.first_audio() {
                        let bytes = audio.bytes().expect("Should decode audio");
                        println!("Audio size: {} bytes", bytes.len());
                        println!("Audio MIME type: {:?}", audio.mime_type());
                        println!("Audio extension: {}", audio.extension());
                        assert!(!bytes.is_empty(), "Audio should not be empty");
                    }
                }
                Err(e) => {
                    // TTS model might not be available in all regions
                    println!("TTS test error (may be expected): {:?}", e);
                }
            }
        })
        .await;
    }

    /// Tests text-to-speech with speech configuration
    #[tokio::test]
    #[ignore = "Requires API key and TTS model access"]
    async fn test_text_to_speech_with_speech_config() {
        use genai_rs::SpeechConfig;

        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let tts_model = "gemini-2.5-pro-preview-tts";

        // TTS can be slow - use extended timeout
        with_timeout(extended_test_timeout(), async {
            let config = SpeechConfig {
                voice: Some("Puck".to_string()),
                language: Some("en-US".to_string()),
                speaker: None,
            };

            let response = client
                .interaction()
                .with_model(tts_model)
                .with_text("Testing speech configuration.")
                .with_audio_output()
                .with_speech_config(config)
                .create()
                .await;

            match response {
                Ok(r) => {
                    println!("TTS with config status: {:?}", r.status);
                    assert!(r.has_audio(), "Response should contain audio output");
                }
                Err(e) => {
                    println!("TTS with config error (may be expected): {:?}", e);
                }
            }
        })
        .await;
    }

    /// Verifies that nested SpeechConfig format fails and flat format succeeds.
    ///
    /// Documentation shows a nested format:
    /// ```json
    /// {"speechConfig": {"voiceConfig": {"prebuiltVoiceConfig": {"voiceName": "Kore"}}}}
    /// ```
    ///
    /// We use a flat format: `{"voice": "Kore", "language": "en-US"}`
    ///
    /// This test documents API behavior: nested format returns 400, flat format works.
    /// See docs/INTERACTIONS_API_FEEDBACK.md Issue #7.
    #[tokio::test]
    #[ignore = "Requires API key and TTS model access"]
    async fn test_speech_config_nested_format_fails_flat_succeeds() {
        use genai_rs::{GenerationConfig, InteractionInput, InteractionRequest};
        use reqwest::Client as ReqwestClient;
        use serde_json::json;
        use std::env;

        let api_key = match env::var("GEMINI_API_KEY") {
            Ok(key) => key,
            Err(_) => {
                println!("Skipping: GEMINI_API_KEY not set");
                return;
            }
        };

        let http_client = ReqwestClient::new();
        let tts_model = "gemini-2.5-pro-preview-tts";
        let url = "https://generativelanguage.googleapis.com/v1beta/interactions";

        // Test 1: Nested format (should FAIL with 400)
        let nested_speech_config: serde_json::Value = json!({
            "voiceConfig": {
                "prebuiltVoiceConfig": {
                    "voiceName": "Kore"
                }
            }
        });

        println!("=== Testing NESTED SpeechConfig format ===");

        let request = InteractionRequest {
            model: Some(tts_model.to_string()),
            agent: None,
            agent_config: None,
            input: InteractionInput::Text("Hello from nested config test.".to_string()),
            previous_interaction_id: None,
            tools: None,
            response_modalities: Some(vec!["AUDIO".to_string()]),
            response_format: None,
            response_mime_type: None,
            generation_config: None,
            stream: None,
            background: None,
            store: None,
            system_instruction: None,
        };

        let mut request_json = serde_json::to_value(&request).expect("Serialize request");
        request_json["generation_config"] = json!({
            "speech_config": nested_speech_config
        });

        let nested_response = http_client
            .post(url)
            .header("x-goog-api-key", &api_key)
            .header("Content-Type", "application/json")
            .json(&request_json)
            .send()
            .await
            .expect("Nested format request failed to send");

        let nested_status = nested_response.status();
        let nested_body = nested_response.text().await.unwrap_or_default();
        println!(
            "Nested format status: {} - {}",
            nested_status,
            &nested_body[..nested_body.len().min(200)]
        );

        // Assert: Nested format should fail with 400
        assert!(
            nested_status.is_client_error(),
            "Nested SpeechConfig format should return 400 error, got {}",
            nested_status
        );

        // Test 2: Flat format (should SUCCEED)
        println!("\n=== Testing FLAT SpeechConfig format ===");

        let flat_gen_config = GenerationConfig {
            speech_config: Some(genai_rs::SpeechConfig::with_voice_and_language(
                "Kore", "en-US",
            )),
            ..Default::default()
        };

        let flat_request = InteractionRequest {
            model: Some(tts_model.to_string()),
            agent: None,
            agent_config: None,
            input: InteractionInput::Text("Hello from flat config test.".to_string()),
            previous_interaction_id: None,
            tools: None,
            response_modalities: Some(vec!["AUDIO".to_string()]),
            response_format: None,
            response_mime_type: None,
            generation_config: Some(flat_gen_config),
            stream: None,
            background: None,
            store: None,
            system_instruction: None,
        };

        let flat_json = serde_json::to_value(&flat_request).expect("Serialize flat request");

        let flat_response = http_client
            .post(url)
            .header("x-goog-api-key", &api_key)
            .header("Content-Type", "application/json")
            .json(&flat_json)
            .send()
            .await
            .expect("Flat format request failed to send");

        let flat_status = flat_response.status();
        println!("Flat format status: {}", flat_status);

        // Assert: Flat format should succeed
        assert!(
            flat_status.is_success(),
            "Flat SpeechConfig format should succeed, got {}",
            flat_status
        );

        println!("\n✓ Verified: Nested format fails (400), flat format succeeds (200)");
    }
}
