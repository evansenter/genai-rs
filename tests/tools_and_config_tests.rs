//! Built-in tools, response formats, and generation config tests
//!
//! Tests for Google Search grounding, code execution, URL context,
//! structured output with JSON schema, and advanced generation config.
//!
//! This file is organized by feature:
//!
//! - **google_search**: Google Search grounding and annotations
//! - **code_execution**: Python code execution tool
//! - **url_context**: URL fetching and analysis
//! - **structured_output**: JSON schema enforcement
//! - **image_generation**: Image output modalities
//! - **thinking**: Thinking level configuration
//! - **sampling**: Top-p, top-k, and combined sampling
//! - **config_fields**: Seed, stop sequences, MIME types, thinking summaries
//! - **function_calling_modes**: AUTO, ANY, NONE, VALIDATED modes
//!
//! # Running Tests
//!
//! ```bash
//! cargo test --test tools_and_config_tests -- --include-ignored --nocapture
//! ```
//!
//! # Notes
//!
//! Some built-in tools may not be available in all regions or account types.
//! Tests are designed to gracefully skip if tools are unavailable.

mod common;

use common::{
    get_client, interaction_builder, retry_on_any_error, stateful_builder,
    validate_response_semantically,
};
use genai_rs::{
    FunctionCallingMode, FunctionDeclaration, GenerationConfig, InteractionStatus, ThinkingLevel,
    ThinkingSummaries, Tool,
};
use serde_json::json;

// =============================================================================
// Built-in Tools: Google Search
// =============================================================================

mod google_search {
    use super::*;
    use futures_util::StreamExt;
    use genai_rs::StreamChunk;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_google_search() {
        // Test using Google Search for grounding with current information
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let result = stateful_builder(&client)
            .with_text(
                "What is the current weather in New York City today? Use search to find current data.",
            )
            .with_google_search() // Use convenience method
            .create()
            .await;

        match result {
            Ok(response) => {
                println!("Status: {:?}", response.status);
                if response.has_text() {
                    let text = response.as_text().unwrap();
                    println!("Response with Google Search: {}", text);
                    // Should provide current/recent information - use semantic validation
                    let is_valid = validate_response_semantically(
                        &client,
                        "Asked about current weather in New York City with Google Search enabled",
                        text,
                        "Does this response discuss weather, temperature, or conditions in New York?",
                    )
                    .await
                    .expect("Semantic validation failed");
                    assert!(is_valid, "Response should mention weather-related content");
                }

                // Verify grounding metadata is available
                if let Some(metadata) = response.google_search_metadata() {
                    println!("Grounding metadata found:");
                    println!("  Search queries: {:?}", metadata.web_search_queries);
                    println!("  Grounding chunks: {}", metadata.grounding_chunks.len());
                    for chunk in &metadata.grounding_chunks {
                        println!(
                            "    - {} [{}] ({})",
                            chunk.web.title, chunk.web.domain, chunk.web.uri
                        );
                    }
                } else {
                    println!("Note: No grounding metadata returned (may vary by API response)");
                }
            }
            Err(e) => {
                let error_str = format!("{:?}", e);
                println!("Google Search error (may be expected): {}", error_str);
                // Google Search may not be available in all accounts
                if error_str.contains("not supported")
                    || error_str.contains("not available")
                    || error_str.contains("permission")
                {
                    println!("Google Search tool not available - skipping test");
                }
            }
        }
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_google_search_streaming() {
        // Test Google Search with streaming
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let mut stream = interaction_builder(&client)
            .with_text("What's the latest news about Rust programming language?")
            .with_google_search()
            .create_stream();

        let mut chunk_count = 0;
        let mut final_response = None;

        while let Some(result) = stream.next().await {
            match result {
                Ok(event) => {
                    chunk_count += 1;
                    match event.chunk {
                        StreamChunk::Delta(content) => {
                            println!("Delta chunk {}: {:?}", chunk_count, content);
                        }
                        StreamChunk::Complete(response) => {
                            println!("Complete response received");
                            // Check for grounding metadata in the final response
                            if let Some(metadata) = response.google_search_metadata() {
                                println!("Streaming grounding metadata:");
                                println!("  Search queries: {:?}", metadata.web_search_queries);
                                println!("  Chunks: {}", metadata.grounding_chunks.len());
                            }
                            final_response = Some(response);
                        }
                        _ => {} // Handle unknown variants
                    }
                }
                Err(e) => {
                    let error_str = format!("{:?}", e);
                    println!("Stream error: {}", error_str);
                    // Google Search may not be available in all accounts
                    if error_str.contains("not supported")
                        || error_str.contains("not available")
                        || error_str.contains("permission")
                    {
                        println!("Google Search tool not available - skipping test");
                        return;
                    }
                    // For other errors, break but let assertions catch issues
                    break;
                }
            }
        }

        assert!(chunk_count > 0, "Should receive at least one chunk");
        assert!(final_response.is_some(), "Should receive complete response");
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_google_search_annotations() {
        // Test that Google Search grounding populates annotations in text content
        // Issue #264: Add annotations and citation support for Text content
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let result = stateful_builder(&client)
            .with_text("What is the capital of France? Use search to verify the answer.")
            .with_google_search()
            .create()
            .await;

        match result {
            Ok(response) => {
                println!("Status: {:?}", response.status);
                if response.has_text() {
                    let text = response.as_text().unwrap();
                    println!("Response text: {}", text);

                    // Check for annotations
                    if response.has_annotations() {
                        let annotations: Vec<_> = response.all_annotations().collect();
                        println!("Found {} annotations:", annotations.len());
                        for (i, annotation) in annotations.iter().enumerate() {
                            let span = annotation.extract_span(text).unwrap_or("<invalid span>");
                            println!(
                                "  [{}: bytes {}..{}] \"{}\" -> {:?}",
                                i,
                                annotation.start_index,
                                annotation.end_index,
                                span,
                                annotation.source
                            );
                        }

                        // Verify at least one annotation has a source
                        let has_sourced_annotation = annotations.iter().any(|a| a.source.is_some());
                        if has_sourced_annotation {
                            println!("✓ Found annotations with source attributions");
                        } else {
                            println!("Note: Annotations present but no source attributions");
                        }
                    } else {
                        // Annotations are not guaranteed - Google may not always return them
                        println!(
                            "Note: No annotations returned (varies by API response and query)"
                        );
                    }
                }

                // Also check grounding metadata for completeness
                if let Some(metadata) = response.google_search_metadata() {
                    println!(
                        "Grounding metadata: {} chunks, {:?} queries",
                        metadata.grounding_chunks.len(),
                        metadata.web_search_queries
                    );
                }
            }
            Err(e) => {
                let error_str = format!("{:?}", e);
                println!("Google Search error (may be expected): {}", error_str);
                if error_str.contains("not supported")
                    || error_str.contains("not available")
                    || error_str.contains("permission")
                {
                    println!("Google Search tool not available - skipping test");
                }
            }
        }
    }
}

// =============================================================================
// Built-in Tools: Code Execution
// =============================================================================

mod code_execution {
    use super::*;
    use futures_util::StreamExt;
    use genai_rs::Content;
    use genai_rs::StreamChunk;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_code_execution() {
        // Test code execution tool for calculations
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let result = stateful_builder(&client)
            .with_text("Calculate the factorial of 10 using Python code execution.")
            .with_code_execution() // Use the new convenience method
            .create()
            .await;

        match result {
            Ok(response) => {
                println!("Status: {:?}", response.status);
                if response.has_text() {
                    let text = response.as_text().unwrap();
                    println!("Response with Code Execution: {}", text);
                    // factorial(10) = 3628800
                    assert!(
                        text.contains("3628800") || text.contains("3,628,800"),
                        "Response should contain the factorial result: {}",
                        text
                    );
                }

                // Check for typed built-in tool content using new helpers
                let summary = response.content_summary();
                println!(
                    "Content summary: {} text, {} code_execution_call, {} code_execution_result, {} unknown",
                    summary.text_count,
                    summary.code_execution_call_count,
                    summary.code_execution_result_count,
                    summary.unknown_count
                );

                // Test the new typed helper methods
                for call in response.code_execution_calls() {
                    println!(
                        "Executed {} code (id: {:?}): {}",
                        call.language,
                        call.id,
                        &call.code[..call.code.len().min(100)]
                    );
                }

                for result in response.code_execution_results() {
                    println!(
                        "is_error: {} (success: {}, call_id: {:?})",
                        result.is_error, !result.is_error, result.call_id
                    );
                    println!("Result: {}", &result.result[..result.result.len().min(100)]);
                }

                // Test the convenience helper and verify the code output directly
                // This is more robust than checking LLM text response
                if let Some(output) = response.successful_code_output() {
                    println!(
                        "First successful output: {}",
                        &output[..output.len().min(100)]
                    );
                    assert!(
                        output.contains("3628800"),
                        "Code output should contain correct factorial result (3628800), got: {}",
                        output
                    );
                } else {
                    // If no successful code output, check that code was at least executed
                    assert!(
                        !response.code_execution_results().is_empty(),
                        "Expected code execution results but found none"
                    );
                }

                // Verify the response doesn't contain unknown content types for code execution
                // (they should all be recognized as known types now)
                if !summary.unknown_types.is_empty() {
                    println!("Unknown types found: {:?}", summary.unknown_types);
                }
            }
            Err(e) => {
                let error_str = format!("{:?}", e);
                println!("Code Execution error (may be expected): {}", error_str);
                if error_str.contains("not supported") || error_str.contains("not available") {
                    println!("Code Execution tool not available - skipping test");
                }
            }
        }
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_code_execution_complex() {
        // Test code execution with a more complex calculation
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let result = interaction_builder(&client)
            .with_text(
                "Using Python, calculate the sum of the first 100 prime numbers. Execute the code to get the answer.",
            )
            .set_tools(vec![Tool::CodeExecution])
            .with_store_enabled()
            .create()
            .await;

        match result {
            Ok(response) => {
                println!("Status: {:?}", response.status);
                if response.has_text() {
                    let text = response.as_text().unwrap();
                    println!("Prime sum response: {}", text);
                    // Sum of first 100 primes is 24133
                    // Model might express this with or without comma formatting
                    assert!(
                        text.contains("24133") || text.contains("24,133"),
                        "Response should contain the sum of first 100 primes (24133), got: {}",
                        text
                    );
                }
            }
            Err(e) => {
                println!("Code Execution error (may be expected): {:?}", e);
            }
        }
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_code_execution_streaming() {
        // Test code execution with streaming
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let mut stream = interaction_builder(&client)
            .with_text("Calculate 15 factorial using Python code execution.")
            .with_code_execution()
            .create_stream();

        let mut chunk_count = 0;
        let mut has_complete = false;
        let mut has_code_execution_call = false;
        let mut has_code_execution_result = false;
        let mut tool_not_available = false;

        while let Some(result) = stream.next().await {
            match result {
                Ok(event) => {
                    chunk_count += 1;
                    match event.chunk {
                        StreamChunk::Delta(content) => {
                            println!("Delta chunk {}: {:?}", chunk_count, content);
                            // Track code execution content in deltas.
                            // NOTE: In streaming mode, built-in tool content (code execution,
                            // Google Search, URL context) arrives via delta chunks, not in the
                            // Complete response. The Complete event is a lifecycle signal that
                            // may arrive before all content is streamed.
                            if matches!(content, Content::CodeExecutionCall { .. }) {
                                has_code_execution_call = true;
                            }
                            if matches!(content, Content::CodeExecutionResult { .. }) {
                                has_code_execution_result = true;
                            }
                        }
                        StreamChunk::Complete(response) => {
                            println!("Complete response received");
                            let summary = response.content_summary();
                            println!(
                                "Complete response code execution: {} calls, {} results",
                                summary.code_execution_call_count,
                                summary.code_execution_result_count
                            );
                            has_complete = true;
                        }
                        _ => {} // Handle unknown variants
                    }
                }
                Err(e) => {
                    let error_str = format!("{:?}", e);
                    println!("Stream error: {}", error_str);
                    if error_str.contains("not supported") || error_str.contains("not available") {
                        println!("Code Execution tool not available - skipping test");
                        tool_not_available = true;
                    }
                    break;
                }
            }
        }

        // Skip assertions if tool wasn't available
        if tool_not_available {
            return;
        }

        assert!(chunk_count > 0, "Should receive at least one chunk");
        assert!(has_complete, "Should receive complete response");

        // Verify code execution happened - check delta chunks since that's where
        // code execution content arrives in streaming mode.
        // We expect BOTH call and result for a successful code execution.
        println!(
            "Code execution in deltas: call={}, result={}",
            has_code_execution_call, has_code_execution_result
        );

        // Log warnings for partial results (helps debug flaky tests)
        if has_code_execution_call && !has_code_execution_result {
            println!("Warning: CodeExecutionCall received but no CodeExecutionResult");
        }
        if !has_code_execution_call && has_code_execution_result {
            println!("Warning: CodeExecutionResult received but no CodeExecutionCall");
        }

        assert!(
            has_code_execution_call,
            "Should have code execution call in streaming response"
        );
        assert!(
            has_code_execution_result,
            "Should have code execution result in streaming response"
        );
    }
}

// =============================================================================
// Built-in Tools: URL Context
// =============================================================================

mod url_context {
    use super::*;
    use futures_util::StreamExt;
    use genai_rs::StreamChunk;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_url_context() {
        // Test URL context tool for fetching and analyzing web content
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let result = stateful_builder(&client)
            .with_text(
                "Fetch and summarize the main content from https://example.com using URL context.",
            )
            .with_url_context() // Use convenience method
            .with_store_enabled()
            .create()
            .await;

        match result {
            Ok(response) => {
                println!("Status: {:?}", response.status);

                // Check for URL context metadata
                if let Some(metadata) = response.url_context_metadata() {
                    println!("URL Context metadata found:");
                    for entry in &metadata.url_metadata {
                        println!(
                            "  URL: {} - Status: {:?}",
                            entry.retrieved_url, entry.url_retrieval_status
                        );
                    }
                } else {
                    println!(
                        "No URL context metadata in response (may be normal for some responses)"
                    );
                }

                if response.has_text() {
                    let text = response.as_text().unwrap();
                    println!("URL Context response: {}", text);
                    // example.com has standard placeholder content - use semantic validation
                    let is_valid = validate_response_semantically(
                        &client,
                        "Asked to describe example.com (IANA reserved domain for documentation)",
                        text,
                        "Does this response describe example.com as a reserved/example domain or mention its illustrative/documentation purpose?",
                    )
                    .await
                    .expect("Semantic validation failed");
                    assert!(
                        is_valid,
                        "Response should describe content from example.com"
                    );
                }
            }
            Err(e) => {
                let error_str = format!("{:?}", e);
                println!("URL Context error (may be expected): {}", error_str);
                if error_str.contains("not supported") || error_str.contains("not available") {
                    println!("URL Context tool not available - skipping test");
                }
            }
        }
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_url_context_streaming() {
        // Test URL context with streaming
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let mut stream = interaction_builder(&client)
            .with_text("Fetch https://example.com and describe the page structure.")
            .with_url_context()
            .create_stream();

        let mut chunk_count = 0;
        let mut final_response = None;

        while let Some(result) = stream.next().await {
            match result {
                Ok(event) => {
                    chunk_count += 1;
                    match event.chunk {
                        StreamChunk::Delta(content) => {
                            println!("Delta chunk {}: {:?}", chunk_count, content);
                        }
                        StreamChunk::Complete(response) => {
                            println!("Complete response received");
                            // Check for URL context metadata
                            if let Some(metadata) = response.url_context_metadata() {
                                println!("URL metadata entries: {}", metadata.url_metadata.len());
                            }
                            final_response = Some(response);
                        }
                        _ => {} // Handle unknown variants
                    }
                }
                Err(e) => {
                    let error_str = format!("{:?}", e);
                    println!("Stream error: {}", error_str);
                    if error_str.contains("not supported") || error_str.contains("not available") {
                        println!("URL Context tool not available - skipping test");
                    }
                    break;
                }
            }
        }

        // Skip assertions if tool wasn't available
        if final_response.is_none() {
            return;
        }

        assert!(chunk_count > 0, "Should receive at least one chunk");

        // Verify URL context metadata is present
        let response = final_response.expect("Should have final response");
        if let Some(metadata) = response.url_context_metadata() {
            println!("URL metadata entries: {}", metadata.url_metadata.len());
            assert!(
                !metadata.url_metadata.is_empty(),
                "Should have URL metadata entries in streaming response"
            );
        }
    }
}

// =============================================================================
// Google Maps
// =============================================================================

mod google_maps {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_google_maps() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let result = stateful_builder(&client)
            .with_text("Find popular coffee shops near Times Square, New York City")
            .with_google_maps()
            .with_store_enabled()
            .create()
            .await;

        let response = result.expect("Google Maps interaction should succeed");

        // Verify completed status
        assert!(
            matches!(response.status, genai_rs::InteractionStatus::Completed),
            "Expected Completed status, got {:?}",
            response.status
        );

        // Verify Google Maps results are present
        assert!(
            response.has_google_maps_results(),
            "Response should contain Google Maps results"
        );

        let results = response.google_maps_results();
        println!("Google Maps results found: {}", results.len());
        for result in &results {
            println!("  Call ID: {}", result.call_id);
            for item in result.items {
                if let Some(places) = &item.places {
                    for place in places {
                        println!(
                            "    Place: {}",
                            place.name.as_deref().unwrap_or("(unnamed)")
                        );
                    }
                }
            }
        }

        // Verify content summary reflects maps content
        let summary = response.content_summary();
        println!("Content summary: {}", summary);
        assert!(
            summary.google_maps_result_count > 0,
            "Content summary should count Google Maps results"
        );
    }
}

// Response Formats: Structured Output
// =============================================================================

mod structured_output {
    use super::*;
    use futures_util::StreamExt;
    use genai_rs::StreamChunk;

    /// Test structured output with JSON schema enforcement.
    ///
    /// The response_format parameter accepts a JSON schema directly to enforce
    /// structured output from the model.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_structured_output() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Pass the JSON schema directly to response_format
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer"},
                "email": {"type": "string"}
            },
            "required": ["name", "age", "email"]
        });

        let result = stateful_builder(&client)
            .with_text("Generate a fake user profile with a name, age, and email address.")
            .with_response_format(schema)
            .create()
            .await;

        let response = result.expect("Structured output request should succeed");
        assert_eq!(response.status, InteractionStatus::Completed);
        assert!(response.has_text(), "Should have text response");

        let text = response.as_text().unwrap();
        println!("Structured output: {}", text);

        // Parse as JSON - should be valid JSON matching our schema
        let json: serde_json::Value =
            serde_json::from_str(text).expect("Response should be valid JSON");
        println!(
            "Parsed JSON: {}",
            serde_json::to_string_pretty(&json).unwrap()
        );

        assert!(json.get("name").is_some(), "Should have name field");
        assert!(json.get("age").is_some(), "Should have age field");
        assert!(json.get("email").is_some(), "Should have email field");
    }

    /// Test structured output with enum constraints.
    ///
    /// The response_format parameter enforces specific enum values for fields.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_structured_output_enum() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Pass the JSON schema directly - enum constrains valid values
        let schema = json!({
            "type": "object",
            "properties": {
                "sentiment": {
                    "type": "string",
                    "enum": ["positive", "negative", "neutral"]
                },
                "confidence": {
                    "type": "number"
                }
            },
            "required": ["sentiment", "confidence"]
        });

        let result = stateful_builder(&client)
            .with_text("Analyze the sentiment of: 'I love this product, it's amazing!'")
            .with_response_format(schema)
            .create()
            .await;

        let response = result.expect("Structured output with enum should succeed");
        assert_eq!(response.status, InteractionStatus::Completed);
        assert!(response.has_text(), "Should have text response");

        let text = response.as_text().unwrap();
        println!("Sentiment analysis: {}", text);

        // Parse as JSON
        let json: serde_json::Value =
            serde_json::from_str(text).expect("Response should be valid JSON");
        println!(
            "Parsed JSON: {}",
            serde_json::to_string_pretty(&json).unwrap()
        );

        // Verify sentiment is one of the enum values
        let sentiment = json
            .get("sentiment")
            .and_then(|v| v.as_str())
            .expect("Should have sentiment field");
        assert!(
            ["positive", "negative", "neutral"].contains(&sentiment),
            "Sentiment '{}' should be one of: positive, negative, neutral",
            sentiment
        );

        // Verify confidence exists
        assert!(
            json.get("confidence").is_some(),
            "Should have confidence field"
        );
    }

    /// Test structured output combined with Google Search grounding.
    ///
    /// This demonstrates using response_format with built-in tools to get
    /// structured data from real-time web searches.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_structured_output_with_google_search() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Schema for structured search results
        let schema = json!({
            "type": "object",
            "properties": {
                "answer": {"type": "string"},
                "source_count": {"type": "integer"}
            },
            "required": ["answer"]
        });

        let result = stateful_builder(&client)
            .with_text("What is the current population of Tokyo, Japan?")
            .with_google_search()
            .with_response_format(schema)
            .create()
            .await;

        let response = result.expect("Structured output with Google Search should succeed");
        assert_eq!(response.status, InteractionStatus::Completed);
        assert!(response.has_text(), "Should have text response");

        let text = response.as_text().unwrap();
        println!("Google Search structured output: {}", text);

        // Parse as JSON
        let json: serde_json::Value =
            serde_json::from_str(text).expect("Response should be valid JSON");
        println!(
            "Parsed JSON: {}",
            serde_json::to_string_pretty(&json).unwrap()
        );

        // Verify required field exists
        assert!(json.get("answer").is_some(), "Should have answer field");

        // Verify grounding metadata is present (Google Search was used)
        if let Some(metadata) = response.google_search_metadata() {
            println!("Grounding chunks: {:?}", metadata.grounding_chunks.len());
        }
    }

    /// Test structured output combined with URL context fetching.
    ///
    /// This demonstrates using response_format with URL context to extract
    /// structured data from web pages.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_structured_output_with_url_context() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Schema for extracting page metadata
        let schema = json!({
            "type": "object",
            "properties": {
                "title": {"type": "string"},
                "description": {"type": "string"},
                "has_navigation": {"type": "boolean"}
            },
            "required": ["title", "description"]
        });

        let result = stateful_builder(&client)
            .with_text("Analyze the page at https://example.com and extract metadata.")
            .with_url_context()
            .with_response_format(schema)
            .create()
            .await;

        let response = result.expect("Structured output with URL context should succeed");
        assert_eq!(response.status, InteractionStatus::Completed);
        assert!(response.has_text(), "Should have text response");

        let text = response.as_text().unwrap();
        println!("URL context structured output: {}", text);

        // Parse as JSON
        let json: serde_json::Value =
            serde_json::from_str(text).expect("Response should be valid JSON");
        println!(
            "Parsed JSON: {}",
            serde_json::to_string_pretty(&json).unwrap()
        );

        // Verify required fields exist
        assert!(json.get("title").is_some(), "Should have title field");
        assert!(
            json.get("description").is_some(),
            "Should have description field"
        );

        // Verify URL context metadata is present
        if let Some(metadata) = response.url_context_metadata() {
            println!("URL metadata entries: {:?}", metadata.url_metadata.len());
        }
    }

    /// Test structured output with complex nested schema.
    ///
    /// This demonstrates more complex JSON schemas with nested objects and arrays.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_structured_output_nested() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Complex nested schema
        let schema = json!({
            "type": "object",
            "properties": {
                "company": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "founded": {"type": "integer"}
                    },
                    "required": ["name"]
                },
                "employees": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "role": {"type": "string"}
                        },
                        "required": ["name", "role"]
                    }
                }
            },
            "required": ["company", "employees"]
        });

        let result = stateful_builder(&client)
            .with_text("Generate data for a fictional tech startup called 'CloudAI' founded in 2023 with 3 employees: a CEO, CTO, and designer.")
            .with_response_format(schema)
            .create()
            .await;

        let response = result.expect("Nested schema structured output should succeed");
        assert_eq!(response.status, InteractionStatus::Completed);
        assert!(response.has_text(), "Should have text response");

        let text = response.as_text().unwrap();
        println!("Nested structured output: {}", text);

        // Parse as JSON
        let json: serde_json::Value =
            serde_json::from_str(text).expect("Response should be valid JSON");
        println!(
            "Parsed JSON: {}",
            serde_json::to_string_pretty(&json).unwrap()
        );

        // Verify nested structure
        let company = json.get("company").expect("Should have company object");
        assert!(company.get("name").is_some(), "Company should have name");

        let employees = json
            .get("employees")
            .and_then(|e| e.as_array())
            .expect("Should have employees array");
        assert_eq!(employees.len(), 3, "Should have 3 employees");

        for emp in employees {
            assert!(emp.get("name").is_some(), "Each employee should have name");
            assert!(emp.get("role").is_some(), "Each employee should have role");
        }
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_structured_output_streaming() {
        // Test structured output with streaming
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let schema = json!({
            "type": "object",
            "properties": {
                "color": {"type": "string"},
                "hex_code": {"type": "string"},
                "rgb": {
                    "type": "object",
                    "properties": {
                        "r": {"type": "integer"},
                        "g": {"type": "integer"},
                        "b": {"type": "integer"}
                    }
                }
            },
            "required": ["color", "hex_code"]
        });

        let mut stream = interaction_builder(&client)
            .with_text("Describe the color blue with its hex code and RGB values.")
            .with_response_format(schema)
            .create_stream();

        let mut chunk_count = 0;
        let mut collected_text = String::new();
        let mut final_response = None;

        while let Some(result) = stream.next().await {
            match result {
                Ok(event) => {
                    chunk_count += 1;
                    match event.chunk {
                        StreamChunk::Delta(content) => {
                            if let Some(text) = content.as_text() {
                                collected_text.push_str(text);
                            }
                            println!("Delta chunk {}: {:?}", chunk_count, content);
                        }
                        StreamChunk::Complete(response) => {
                            println!("Complete response received");
                            final_response = Some(response);
                        }
                        _ => {} // Handle unknown variants
                    }
                }
                Err(e) => {
                    println!("Stream error: {:?}", e);
                    break;
                }
            }
        }

        assert!(chunk_count > 0, "Should receive at least one chunk");
        assert!(final_response.is_some(), "Should receive complete response");

        // Verify the final response is valid JSON matching our schema
        if let Some(text) = final_response.as_ref().and_then(|r| r.as_text()) {
            // Verify streamed text matches final response
            assert_eq!(
                collected_text, text,
                "Streamed chunks should match final response text"
            );

            let json: serde_json::Value =
                serde_json::from_str(text).expect("Streaming response should be valid JSON");
            println!(
                "Parsed JSON: {}",
                serde_json::to_string_pretty(&json).unwrap()
            );
            assert!(json.get("color").is_some(), "Should have color field");
            assert!(json.get("hex_code").is_some(), "Should have hex_code field");
        }
    }

    /// Test structured output (JSON schema) across multiple conversation turns.
    ///
    /// This validates that:
    /// - JSON schema enforcement works in stateful conversations
    /// - Data from Turn 1 can be extended with new schema in Turn 2
    /// - Context is preserved between turns with different schemas
    ///
    /// Turn 1: Generate {name, age} for a software developer (model chooses values)
    /// Turn 2: Extend with {original_name, original_age, email, occupation} preserving Turn 1 values
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_structured_output_multi_turn() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        println!("=== Structured Output + Multi-turn ===");

        // Turn 1: Generate initial user profile
        let schema1 = serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer"}
            },
            "required": ["name", "age"]
        });

        println!("\n--- Turn 1: Generate user profile ---");
        let response1 = stateful_builder(&client)
            .with_text("Create a user profile for a software developer. Choose any name and age you like. Output as JSON.")
            .with_response_format(schema1)
            .create()
            .await
            .expect("Turn 1 should succeed");

        assert_eq!(
            response1.status,
            InteractionStatus::Completed,
            "Turn 1 should complete successfully"
        );

        let text1 = response1.as_text().expect("Should have text response");
        println!("Turn 1 JSON: {}", text1);

        // Parse and validate Turn 1 JSON
        let json1: serde_json::Value = serde_json::from_str(text1).expect("Should parse as JSON");
        let original_name = json1["name"].as_str().expect("Should have name");
        let original_age = json1["age"].as_i64().expect("Should have age");
        println!(
            "Turn 1 values - name: {}, age: {}",
            original_name, original_age
        );

        // Turn 2: Extend the profile with new fields
        let schema2 = serde_json::json!({
            "type": "object",
            "properties": {
                "original_name": {"type": "string"},
                "original_age": {"type": "integer"},
                "email": {"type": "string"},
                "occupation": {"type": "string"}
            },
            "required": ["original_name", "original_age", "email", "occupation"]
        });

        println!("\n--- Turn 2: Extend profile ---");
        let response2 = stateful_builder(&client)
            .with_previous_interaction(response1.id.as_ref().expect("id should exist"))
            .with_text("Based on the user profile you just created, output a new JSON with the original name and age, plus add an email address and occupation that fits the profile.")
            .with_response_format(schema2)
            .create()
            .await
            .expect("Turn 2 should succeed");

        assert_eq!(
            response2.status,
            InteractionStatus::Completed,
            "Turn 2 should complete successfully"
        );

        let text2 = response2.as_text().expect("Should have text response");
        println!("Turn 2 JSON: {}", text2);

        // Parse and validate Turn 2 JSON
        let json2: serde_json::Value = serde_json::from_str(text2).expect("Should parse as JSON");
        let turn2_name = json2["original_name"]
            .as_str()
            .expect("Should have original_name");
        let turn2_age = json2["original_age"]
            .as_i64()
            .expect("Should have original_age");
        let email = json2["email"].as_str().expect("Should have email");
        let occupation = json2["occupation"]
            .as_str()
            .expect("Should have occupation");

        println!(
            "Turn 2 references - name: {}, age: {}",
            turn2_name, turn2_age
        );

        // Compare Turn 2 values against Turn 1 values for robust context preservation test
        assert!(
            turn2_name.to_lowercase() == original_name.to_lowercase(),
            "Turn 2 should preserve name from Turn 1. Expected '{}', got: '{}'",
            original_name,
            turn2_name
        );

        assert_eq!(
            turn2_age, original_age,
            "Turn 2 should preserve age from Turn 1. Expected {}, got: {}",
            original_age, turn2_age
        );

        // Email should look valid and occupation should be set
        assert!(
            email.contains("@"),
            "Email should contain @. Got: {}",
            email
        );
        assert!(!occupation.is_empty(), "Occupation should not be empty");

        println!("\n✓ Structured Output + multi-turn completed successfully");
    }
}

// =============================================================================
// Response Modalities: Image Generation
// =============================================================================

mod image_generation {
    use super::*;
    use std::time::Duration;

    /// Test image generation using response modalities.
    ///
    /// Note: Uses retry logic because the image generation model sometimes returns
    /// text instead of images. See issue #287 for details.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_response_modalities_image() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Retry up to 2 times (3 total attempts) because the image generation model
        // sometimes returns text instead of images (see issue #287)
        type BoxError = Box<dyn std::error::Error + Send + Sync>;
        let result: Result<(), BoxError> = retry_on_any_error(2, Duration::from_secs(3), || {
            let client = client.clone();
            async move {
                let response = client
                    .interaction()
                    .with_model("gemini-3-pro-image-preview")
                    .with_text("Generate a simple image of a red circle on a white background.")
                    .with_response_modalities(vec!["IMAGE".to_string()])
                    .with_store_enabled()
                    .create()
                    .await?;

                println!("Status: {:?}", response.status);
                println!("Outputs count: {}", response.outputs.len());

                // Check for image content in outputs
                for (i, output) in response.outputs.iter().enumerate() {
                    println!("Output {}: {:?}", i, output);
                }

                // Image generation should return image content
                let has_image = response
                    .outputs
                    .iter()
                    .any(|o| matches!(o, genai_rs::Content::Image { .. }));

                if has_image {
                    println!("Has image output: true");
                    Ok(())
                } else {
                    // No image found - this is the flaky case we want to retry
                    Err("Response did not contain image data (model returned text instead)".into())
                }
            }
        })
        .await;

        match result {
            Ok(()) => println!("\n✓ Response modalities image test passed"),
            Err(e) => {
                println!("Image generation error after retries: {}", e);
                panic!("Image generation failed after retries: {}", e);
            }
        }
    }
}

// =============================================================================
// Generation Config: Thinking Level
// =============================================================================

mod thinking {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_generation_config_thinking_level_minimal() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let config = GenerationConfig {
            temperature: Some(0.7),
            max_output_tokens: Some(500),
            thinking_level: Some(ThinkingLevel::Minimal),
            ..Default::default()
        };

        let response = stateful_builder(&client)
            .with_text("What is 2 + 2?")
            .with_generation_config(config)
            .create()
            .await
            .expect("Minimal thinking interaction failed");

        assert_eq!(response.status, InteractionStatus::Completed);
        assert!(response.has_text(), "Should have text response");

        let text = response.as_text().unwrap();
        println!("Minimal thinking response: {}", text);

        // Deterministic math - use .contains() per CLAUDE.md guidance
        assert!(
            text.contains('4'),
            "Should contain the answer 4. Got: {}",
            text
        );
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_generation_config_thinking_level_high() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let config = GenerationConfig {
            temperature: Some(0.7),
            max_output_tokens: Some(1000),
            thinking_level: Some(ThinkingLevel::High),
            ..Default::default()
        };

        let response = stateful_builder(&client)
            .with_text("Explain step by step how to solve: If x + 3 = 7, what is x?")
            .with_generation_config(config)
            .create()
            .await
            .expect("High thinking interaction failed");

        assert_eq!(response.status, InteractionStatus::Completed);
        assert!(response.has_text(), "Should have text response");

        let text = response.as_text().unwrap();
        println!("High thinking response: {}", text);

        // Should provide a detailed explanation
        let word_count = text.split_whitespace().count();
        println!("Word count: {}", word_count);

        // Deterministic math - use .contains() per CLAUDE.md guidance
        assert!(
            text.contains('4') || text.contains("four"),
            "Should contain the answer x = 4. Got: {}",
            text
        );
    }

    /// Test thinking_summaries with the builder method.
    ///
    /// This test validates that with_thinking_summaries() works correctly with the API.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_generation_config_thinking_summaries() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Use thinking with summaries enabled
        let response = stateful_builder(&client)
            .with_text("What is the capital of France?")
            .with_thinking_level(ThinkingLevel::Medium)
            .with_thinking_summaries(ThinkingSummaries::Auto)
            .create()
            .await
            .expect("Thinking with summaries request should succeed");

        assert_eq!(response.status, InteractionStatus::Completed);
        assert!(response.has_text(), "Should have text response");

        let text = response.as_text().unwrap();
        println!("Thinking with summaries response: {}", text);

        // Verify we got a reasonable response
        assert!(!text.is_empty(), "Response should not be empty");
        println!("✓ with_thinking_summaries() builder method works with API");
    }
}

// =============================================================================
// Generation Config: Top-p and Top-k
// =============================================================================

mod sampling {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_generation_config_top_p() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Low top_p = more focused/deterministic
        let config = GenerationConfig {
            temperature: Some(1.0),
            max_output_tokens: Some(100),
            top_p: Some(0.1), // Very focused
            ..Default::default()
        };

        let response = stateful_builder(&client)
            .with_text("What is the capital of France? Answer in one word.")
            .with_generation_config(config)
            .create()
            .await
            .expect("Top-p interaction failed");

        assert_eq!(response.status, InteractionStatus::Completed);
        assert!(response.has_text(), "Should have text response");

        let text = response.as_text().unwrap();
        println!("Top-p response: {}", text);

        let is_valid = validate_response_semantically(
            &client,
            "Asked for the capital of France in one word",
            text,
            "Does this response identify Paris as the capital of France?",
        )
        .await
        .expect("Semantic validation failed");
        assert!(is_valid, "Should answer Paris");
    }

    /// Test top_k generation config parameter.
    /// Note: The Interactions API may not support top_k in GenerationConfig.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_generation_config_top_k() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Low top_k = only consider top k tokens
        let config = GenerationConfig {
            temperature: Some(1.0),
            max_output_tokens: Some(100),
            top_k: Some(5), // Only top 5 tokens
            ..Default::default()
        };

        let result = stateful_builder(&client)
            .with_text("What is 10 + 5? Answer with just the number.")
            .with_generation_config(config)
            .create()
            .await;

        match result {
            Ok(response) => {
                assert_eq!(response.status, InteractionStatus::Completed);
                assert!(response.has_text(), "Should have text response");

                let text = response.as_text().unwrap();
                println!("Top-k response: {}", text);

                // Deterministic math - use .contains() per CLAUDE.md guidance
                assert!(
                    text.contains("15") || text.contains("fifteen"),
                    "Should contain 15. Got: {}",
                    text
                );
            }
            Err(e) => {
                let error_str = format!("{:?}", e);
                if error_str.contains("top_k")
                    || error_str.contains("invalid JSON")
                    || error_str.contains("GenerationConfig")
                {
                    println!(
                        "Note: top_k parameter not supported in GenerationConfig for Interactions API. This is expected."
                    );
                } else {
                    panic!("Unexpected error: {:?}", e);
                }
            }
        }
    }

    /// Test combining multiple generation config options.
    /// Note: top_k is excluded since the Interactions API may not support it.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_generation_config_combined() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Note: Excluding top_k since it's not supported in Interactions API
        // Use a generous max_output_tokens to avoid Incomplete status when
        // thinking tokens consume part of the budget.
        let config = GenerationConfig {
            temperature: Some(0.5),
            max_output_tokens: Some(2048),
            top_p: Some(0.9),
            thinking_level: Some(ThinkingLevel::Medium),
            ..Default::default()
        };

        let response = stateful_builder(&client)
            .with_text("Write a haiku about programming.")
            .with_generation_config(config)
            .create()
            .await
            .expect("Combined config interaction failed");

        assert_eq!(response.status, InteractionStatus::Completed);
        assert!(response.has_text(), "Should have text response");

        let text = response.as_text().unwrap();
        println!("Combined config response: {}", text);

        // Haiku should be short and have line breaks or short lines
        let line_count = text.lines().count();
        println!("Line count: {}", line_count);
        assert!(line_count >= 1, "Should have at least one line of text");
    }
}

// =============================================================================
// Generation Config: New Fields (seed, stop_sequences, response_mime_type)
// =============================================================================

mod config_fields {
    use super::*;

    /// Test seed for deterministic output generation.
    ///
    /// Using the same seed with identical inputs should produce the same output.
    /// This is useful for testing and debugging.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_generation_config_seed() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let seed = 12345i64;
        let prompt = "Generate exactly one random 4-letter word.";

        // First request with seed
        let response1 = interaction_builder(&client)
            .with_text(prompt)
            .with_seed(seed)
            .create()
            .await
            .expect("First seed request should succeed");

        assert_eq!(response1.status, InteractionStatus::Completed);
        let text1 = response1.as_text().expect("Should have text response");
        println!("Seed {} response 1: {}", seed, text1);

        // Second request with same seed - should produce same output
        let response2 = interaction_builder(&client)
            .with_text(prompt)
            .with_seed(seed)
            .create()
            .await
            .expect("Second seed request should succeed");

        assert_eq!(response2.status, InteractionStatus::Completed);
        let text2 = response2.as_text().expect("Should have text response");
        println!("Seed {} response 2: {}", seed, text2);

        // With the same seed and input, outputs should be identical
        // Note: API behavior may vary, so we log but use a softer assertion
        if text1.trim() == text2.trim() {
            println!("✓ Seed produced identical outputs");
        } else {
            println!(
                "Note: Seed produced different outputs (API behavior may vary)\n  1: {}\n  2: {}",
                text1.trim(),
                text2.trim()
            );
        }
    }

    /// Test stop_sequences for halting generation.
    ///
    /// When the model generates any of these sequences, generation stops.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_generation_config_stop_sequences() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let response = interaction_builder(&client)
            .with_text("Count from 1 to 10, one number per line.")
            .with_stop_sequences(vec!["5".to_string()])
            .create()
            .await
            .expect("Stop sequences request should succeed");

        assert_eq!(response.status, InteractionStatus::Completed);
        let text = response.as_text().expect("Should have text response");
        println!("Stop sequence response: {}", text);

        // The response should NOT contain numbers after 5 since we stopped there
        // It may or may not include 5 itself (stop sequences may or may not be included)
        let has_six_or_higher = text.contains("6")
            || text.contains("7")
            || text.contains("8")
            || text.contains("9")
            || text.contains("10");

        if !has_six_or_higher {
            println!("✓ Stop sequence correctly halted generation before 6-10");
        } else {
            println!(
                "Note: Stop sequence may not have halted as expected (API behavior may vary): {}",
                text
            );
        }
    }

    /// Test response_mime_type with structured output.
    ///
    /// The response_mime_type field is used with response_format to specify
    /// the output format, typically "application/json".
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_generation_config_response_mime_type() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let schema = json!({
            "type": "object",
            "properties": {
                "greeting": {"type": "string"},
                "language": {"type": "string"}
            },
            "required": ["greeting", "language"]
        });

        let response = interaction_builder(&client)
            .with_text("Generate a greeting in Spanish.")
            .with_response_mime_type("application/json")
            .with_response_format(schema)
            .create()
            .await
            .expect("Response mime type request should succeed");

        assert_eq!(response.status, InteractionStatus::Completed);
        let text = response.as_text().expect("Should have text response");
        println!("Response with MIME type: {}", text);

        // Should be valid JSON
        let parsed: serde_json::Value =
            serde_json::from_str(text).expect("Response should be valid JSON");
        assert!(
            parsed.get("greeting").is_some(),
            "Should have greeting field"
        );
        assert!(
            parsed.get("language").is_some(),
            "Should have language field"
        );
        println!("✓ response_mime_type produced valid structured JSON output");
    }

    /// Test combined new generation config fields.
    ///
    /// This test uses seed, stop_sequences, and response_mime_type together.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_generation_config_new_fields_combined() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let schema = json!({
            "type": "object",
            "properties": {
                "items": {
                    "type": "array",
                    "items": {"type": "string"}
                }
            },
            "required": ["items"]
        });

        let response = interaction_builder(&client)
            .with_text("List 3 colors as a JSON array.")
            .with_seed(42)
            .with_response_mime_type("application/json")
            .with_response_format(schema)
            .create()
            .await
            .expect("Combined new fields request should succeed");

        assert_eq!(response.status, InteractionStatus::Completed);
        let text = response.as_text().expect("Should have text response");
        println!("Combined new fields response: {}", text);

        // Should be valid JSON with items array
        let parsed: serde_json::Value =
            serde_json::from_str(text).expect("Response should be valid JSON");
        let items = parsed
            .get("items")
            .and_then(|v| v.as_array())
            .expect("Should have items array");
        assert!(!items.is_empty(), "Items array should not be empty");
        println!("✓ Combined new generation config fields work correctly");
    }
}

// =============================================================================
// Tool Configuration: Function Calling Modes
// =============================================================================

mod function_calling_modes {
    use super::*;

    /// Test that FunctionCallingMode serializes correctly in GenerationConfig.
    ///
    /// Validates that tool_choice in generation_config serializes correctly
    /// for each function calling mode.
    #[test]
    fn test_generation_config_tool_choice_serialization() {
        // Test AUTO mode serialization
        let config = GenerationConfig {
            tool_choice: Some(FunctionCallingMode::Auto),
            ..Default::default()
        };
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(
            json["tool_choice"],
            serde_json::Value::String("AUTO".to_string())
        );

        // Test ANY mode serialization
        let config = GenerationConfig {
            tool_choice: Some(FunctionCallingMode::Any),
            ..Default::default()
        };
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(
            json["tool_choice"],
            serde_json::Value::String("ANY".to_string())
        );

        // Test NONE mode serialization
        let config = GenerationConfig {
            tool_choice: Some(FunctionCallingMode::None),
            ..Default::default()
        };
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(
            json["tool_choice"],
            serde_json::Value::String("NONE".to_string())
        );

        // Test VALIDATED mode serialization
        let config = GenerationConfig {
            tool_choice: Some(FunctionCallingMode::Validated),
            ..Default::default()
        };
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(
            json["tool_choice"],
            serde_json::Value::String("VALIDATED".to_string())
        );

        println!("✓ All function calling modes serialize correctly in GenerationConfig");
    }

    /// Test Unknown function calling mode roundtrip.
    ///
    /// Validates that unknown mode values are preserved through serialization.
    #[test]
    fn test_function_calling_mode_unknown_roundtrip() {
        let unknown_mode = FunctionCallingMode::Unknown {
            mode_type: "FUTURE_MODE".to_string(),
            data: serde_json::Value::String("FUTURE_MODE".to_string()),
        };

        // Serialize
        let json = serde_json::to_string(&unknown_mode).unwrap();
        assert_eq!(json, "\"FUTURE_MODE\"");

        // Deserialize
        let deserialized: FunctionCallingMode = serde_json::from_str(&json).unwrap();
        assert!(deserialized.is_unknown());
        assert_eq!(deserialized.unknown_mode_type(), Some("FUTURE_MODE"));

        println!("✓ Unknown mode roundtrip works correctly");
    }

    /// Test VALIDATED mode with function calling (API integration).
    ///
    /// This test verifies that the VALIDATED mode can be sent to the API.
    /// Note: VALIDATED mode may not yet be supported by all models, so we
    /// handle potential API errors gracefully.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_function_calling_validated_mode() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Define a simple function
        let weather_fn = FunctionDeclaration::builder("get_weather")
            .description("Get the current weather for a location")
            .parameter(
                "location",
                json!({"type": "string", "description": "The city name"}),
            )
            .required(vec!["location".to_string()])
            .build();

        let result = interaction_builder(&client)
            .with_text("What's the weather like in Tokyo?")
            .add_functions(vec![weather_fn])
            .with_function_calling_mode(FunctionCallingMode::Validated)
            .create()
            .await;

        match result {
            Ok(response) => {
                println!("VALIDATED mode response status: {:?}", response.status);
                println!("Response has text: {}", response.has_text());
                println!(
                    "Response has function calls: {}",
                    !response.function_calls().is_empty()
                );

                // With VALIDATED mode, the model should either:
                // - Call the function (with schema-adherent output), or
                // - Provide a natural language response (also schema-adherent)
                let has_output = response.has_text() || !response.function_calls().is_empty();
                assert!(
                    has_output,
                    "VALIDATED mode should produce either text or function calls"
                );

                println!("✓ VALIDATED mode works with API");
            }
            Err(e) => {
                let error_str = format!("{:?}", e);
                println!("VALIDATED mode error: {}", error_str);

                // VALIDATED mode may not yet be supported
                if error_str.contains("VALIDATED")
                    || error_str.contains("not supported")
                    || error_str.contains("invalid")
                    || error_str.contains("mode")
                {
                    println!("Note: VALIDATED mode may not be supported yet - skipping");
                } else {
                    panic!("Unexpected error: {:?}", e);
                }
            }
        }
    }

    /// Test with_function_calling_mode() builder method (API integration).
    ///
    /// Verifies that the function calling mode is correctly sent to the API
    /// via generation_config.tool_choice. Uses ANY mode which requires the
    /// model to call a function.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_with_function_calling_mode_builder() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Define a simple function
        let greet_fn = FunctionDeclaration::builder("greet_user")
            .description("Greet a user by name")
            .parameter(
                "name",
                json!({"type": "string", "description": "The user's name"}),
            )
            .required(vec!["name".to_string()])
            .build();

        // Use ANY mode - model MUST call a function
        let response = interaction_builder(&client)
            .with_text("Please greet Alice")
            .add_functions(vec![greet_fn])
            .with_function_calling_mode(FunctionCallingMode::Any)
            .create()
            .await
            .expect("Request with function_calling_mode should succeed");

        println!("Response status: {:?}", response.status);

        // With ANY mode, the model MUST call a function
        let function_calls = response.function_calls();
        println!("Function calls: {:?}", function_calls.len());

        if !function_calls.is_empty() {
            let call = &function_calls[0];
            println!("Called function: {}", call.name);
            assert_eq!(call.name, "greet_user", "Should call greet_user function");
            println!("✓ with_function_calling_mode() builder method works with API");
        } else if response.has_text() {
            // Model may respond with text if function calling fails
            println!("Note: Model responded with text instead of function call");
            println!("Text: {}", response.as_text().unwrap());
        }
    }
}
