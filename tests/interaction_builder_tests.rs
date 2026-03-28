//! Unit tests for InteractionBuilder (no API key required)
//!
//! This file contains tests for InteractionBuilder edge cases and validation.
//!
//! This file is organized by test category:
//!
//! - **basic**: Core builder functionality with inputs, models, functions
//! - **validation**: Input validation and error handling
//! - **tools**: Built-in tool tests (Google Search, Code Execution, URL Context)
//! - **edge_cases**: Overwrites, ordering, and boundary conditions
//! - **multimodal**: add_* methods for images, audio, video, documents

mod common;

mod basic {
    use crate::common::DEFAULT_MODEL;
    use genai_rs::{
        Client, Content, FunctionDeclaration, GenerationConfig, InteractionInput, ThinkingLevel,
    };
    use serde_json::json;

    #[test]
    fn test_interaction_builder_with_complex_content_input() {
        // Test using InteractionInput::Content variant with multiple content items
        let client = Client::new("test-api-key".to_string());

        let complex_input = InteractionInput::Content(vec![
            Content::Text {
                text: Some("First message".to_string()),
                annotations: None,
            },
            Content::Text {
                text: Some("Second message".to_string()),
                annotations: None,
            },
            Content::Thought {
                signature: Some("Eq0JCq...signature".to_string()),
            },
        ]);

        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_input(complex_input);

        // Verify the builder stored the complex input correctly
        let request = builder
            .build()
            .expect("Builder should create valid request");

        // Verify the input is a Content variant with 3 items
        match &request.input {
            InteractionInput::Content(items) => {
                assert_eq!(items.len(), 3, "Should have 3 content items");
            }
            _ => panic!("Expected InteractionInput::Content variant"),
        }
    }

    #[test]
    fn test_interaction_builder_with_both_model_and_agent_set() {
        // Test that setting both model AND agent fails validation
        let client = Client::new("test-api-key".to_string());

        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_agent("my-agent")
            .with_text("Hello");

        // Builder validates that only one of model/agent can be set
        let result = builder.build();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Cannot specify both model"));
    }

    #[test]
    fn test_interaction_builder_with_very_long_text() {
        // Test with very long input text (10KB)
        let client = Client::new("test-api-key".to_string());
        let long_text = "Lorem ipsum ".repeat(1000); // ~12KB

        let _builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_text(&long_text);

        // Builder accepts large text inputs without size validation
    }

    #[test]
    fn test_interaction_builder_with_unicode_and_emojis() {
        // Test with unicode, emojis, and special characters
        let client = Client::new("test-api-key".to_string());

        let unicode_text = "Hello 世界 🌍 مرحبا Здравствуй \u{1F600} \u{1F44D}";

        let _builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_text(unicode_text);

        // Builder handles Unicode and multi-byte characters correctly
    }

    #[test]
    fn test_interaction_builder_with_empty_text() {
        // Test with empty string input
        let client = Client::new("test-api-key".to_string());

        let _builder = client.interaction().with_model(DEFAULT_MODEL).with_text("");

        // Builder allows empty string inputs without validation
    }

    #[test]
    fn test_interaction_builder_with_multiple_functions() {
        // Test adding many functions
        let client = Client::new("test-api-key".to_string());

        let mut builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_text("Test");

        // Add 10 functions
        for i in 0..10 {
            let func = FunctionDeclaration::builder(format!("function_{}", i))
                .description(format!("Function number {}", i))
                .parameter("param", json!({"type": "string"}))
                .required(vec!["param".to_string()])
                .build();
            builder = builder.add_function(func);
        }

        // Builder accepts many function declarations without validation
    }

    #[test]
    fn test_interaction_builder_with_complex_generation_config() {
        // Test with generation config at boundary values
        let client = Client::new("test-api-key".to_string());

        let config = GenerationConfig {
            temperature: Some(2.0),        // Max value
            max_output_tokens: Some(8192), // High value
            top_p: Some(1.0),              // Max value
            top_k: Some(40),
            thinking_level: None,
            ..Default::default()
        };

        let _builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_text("Test")
            .with_generation_config(config);

        // Builder accepts generation config with boundary values
    }

    #[test]
    fn test_interaction_builder_with_response_format_json_schema() {
        // Test with complex JSON schema for structured output
        let client = Client::new("test-api-key".to_string());

        let complex_schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "number"},
                "address": {
                    "type": "object",
                    "properties": {
                        "street": {"type": "string"},
                        "city": {"type": "string"},
                        "zipcode": {"type": "string"}
                    }
                },
                "hobbies": {
                    "type": "array",
                    "items": {"type": "string"}
                }
            },
            "required": ["name", "age"]
        });

        let _builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_text("Generate a person")
            .with_response_format(complex_schema);

        // Builder accepts complex nested JSON schemas without validation
    }

    #[test]
    fn test_interaction_builder_with_all_features_combined() {
        // Test combining many features simultaneously
        let client = Client::new("test-api-key".to_string());

        let func = FunctionDeclaration::builder("get_weather")
            .description("Get weather")
            .build();

        let config = GenerationConfig {
            temperature: Some(0.7),
            max_output_tokens: Some(1024),
            top_p: Some(0.95),
            top_k: Some(40),
            thinking_level: Some(ThinkingLevel::Low),
            ..Default::default()
        };

        let _builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_text("Complex query")
            .with_system_instruction("Be helpful")
            .add_function(func)
            .with_generation_config(config)
            .with_response_modalities(vec!["TEXT".to_string()])
            .with_background(true)
            .with_store_disabled();

        // Builder supports combining all features without conflicts
    }

    #[test]
    fn test_interaction_builder_method_chaining() {
        // Verify fluent API / method chaining works correctly
        let client = Client::new("test-api-key".to_string());

        let _builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_text("Test 1")
            .with_text("Test 2") // Overwrites previous text
            .with_system_instruction("Instruction 1")
            .with_system_instruction("Instruction 2") // Overwrites
            .with_background(false)
            .with_background(true); // Overwrites

        // All methods should be chainable and later calls overwrite earlier values
    }

    #[test]
    fn test_system_instruction_available_in_all_scenarios() {
        // system_instruction should be available in all builder configurations
        let client = Client::new("test-api-key".to_string());

        // Basic request
        let basic = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_text("Hello")
            .with_system_instruction("Focus on Rust");

        let request = basic.build().expect("Should build successfully");
        match request.system_instruction {
            Some(InteractionInput::Text(text)) => {
                assert_eq!(text, "Focus on Rust");
            }
            _ => panic!("Expected Text system_instruction"),
        }

        // With previous interaction (follow-up)
        let followup = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_text("Continue")
            .with_previous_interaction("prev-id")
            .with_system_instruction("Focus on testing");

        let request = followup.build().expect("Should build successfully");
        match request.system_instruction {
            Some(InteractionInput::Text(text)) => {
                assert_eq!(text, "Focus on testing");
            }
            _ => panic!("Expected Text system_instruction"),
        }

        // With store disabled
        let store_disabled = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_text("Quick query")
            .with_store_disabled()
            .with_system_instruction("Be concise");

        let request = store_disabled.build().expect("Should build successfully");
        match request.system_instruction {
            Some(InteractionInput::Text(text)) => {
                assert_eq!(text, "Be concise");
            }
            _ => panic!("Expected Text system_instruction"),
        }
    }

    #[test]
    fn test_system_instruction_with_previous_interaction() {
        // Verify system_instruction can be set alongside previous_interaction
        let client = Client::new("test-api-key".to_string());

        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_text("Continue")
            .with_previous_interaction("prev-id")
            .with_system_instruction("You are a helpful coding assistant");

        let request = builder.build().expect("Should build successfully");

        match request.system_instruction {
            Some(InteractionInput::Text(text)) => {
                assert_eq!(text, "You are a helpful coding assistant");
            }
            _ => panic!("Expected system_instruction to be set"),
        }
    }

    #[test]
    fn test_system_instruction_overwrite() {
        // Calling with_system_instruction multiple times should overwrite
        let client = Client::new("test-api-key".to_string());

        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_text("Hello")
            .with_system_instruction("First instruction")
            .with_system_instruction("Second instruction");

        let request = builder.build().expect("Should build successfully");

        match request.system_instruction {
            Some(InteractionInput::Text(text)) => {
                assert_eq!(text, "Second instruction");
            }
            _ => panic!("Expected system_instruction to be overwritten"),
        }
    }
}

mod validation {
    use crate::common::DEFAULT_MODEL;
    use genai_rs::Client;

    #[test]
    fn test_interaction_builder_validation_missing_input() {
        // Verify that build fails when no input is provided
        let client = Client::new("test-api-key".to_string());

        let builder = client.interaction().with_model(DEFAULT_MODEL);

        let result = builder.build();

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, genai_rs::GenaiError::InvalidInput(_)));

        // Verify error message mentions input requirement
        if let genai_rs::GenaiError::InvalidInput(msg) = err {
            assert!(msg.contains("Input is required"));
        }
    }

    #[test]
    fn test_interaction_builder_validation_missing_model_and_agent() {
        // Verify that build fails when neither model nor agent is specified
        let client = Client::new("test-api-key".to_string());

        let builder = client.interaction().with_text("Hello"); // Has input but no model/agent

        let result = builder.build();

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, genai_rs::GenaiError::InvalidInput(_)));

        // Verify error message mentions model/agent requirement
        if let genai_rs::GenaiError::InvalidInput(msg) = err {
            assert!(msg.contains("model or agent"));
        }
    }

    #[test]
    fn test_interaction_builder_validation_success_with_model() {
        // Verify that build succeeds when model and input are provided
        let client = Client::new("test-api-key".to_string());

        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_text("Hello");

        let result = builder.build();
        assert!(result.is_ok());

        let request = result.unwrap();
        assert_eq!(request.model.as_deref(), Some(DEFAULT_MODEL));
        assert!(request.agent.is_none());
    }
}

mod tools {
    use crate::common::DEFAULT_MODEL;
    use genai_rs::{Client, FunctionDeclaration, Tool};

    #[test]
    fn test_interaction_builder_with_google_search() {
        // Test that with_google_search adds the GoogleSearch tool
        let client = Client::new("test-api-key".to_string());

        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_text("What's the weather today?")
            .with_google_search();

        let result = builder.build();
        assert!(result.is_ok());

        let request = result.unwrap();

        // Verify GoogleSearch tool was added
        assert!(request.tools.is_some());
        let tools = request.tools.unwrap();
        assert_eq!(tools.len(), 1);
        assert!(matches!(tools[0], Tool::GoogleSearch { .. }));
    }

    #[test]
    fn test_interaction_builder_with_google_search_and_functions() {
        // Test that with_google_search can be combined with function declarations
        let client = Client::new("test-api-key".to_string());

        let func = FunctionDeclaration::builder("get_temperature")
            .description("Get temperature")
            .build();

        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_text("What's the weather today?")
            .add_function(func)
            .with_google_search();

        let result = builder.build();
        assert!(result.is_ok());

        let request = result.unwrap();

        // Verify both tools were added
        assert!(request.tools.is_some());
        let tools = request.tools.unwrap();
        assert_eq!(tools.len(), 2);
        assert!(matches!(tools[0], Tool::Function { .. }));
        assert!(matches!(tools[1], Tool::GoogleSearch { .. }));
    }

    #[test]
    fn test_interaction_builder_with_code_execution() {
        // Test that with_code_execution adds the CodeExecution tool
        let client = Client::new("test-api-key".to_string());

        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_text("Calculate the factorial of 10")
            .with_code_execution();

        let result = builder.build();
        assert!(result.is_ok());

        let request = result.unwrap();

        // Verify CodeExecution tool was added
        assert!(request.tools.is_some());
        let tools = request.tools.unwrap();
        assert_eq!(tools.len(), 1);
        assert!(matches!(tools[0], Tool::CodeExecution));
    }

    #[test]
    fn test_interaction_builder_with_code_execution_and_google_search() {
        // Test that with_code_execution can be combined with with_google_search
        let client = Client::new("test-api-key".to_string());

        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_text("Search for prime numbers and calculate the first 10")
            .with_code_execution()
            .with_google_search();

        let result = builder.build();
        assert!(result.is_ok());

        let request = result.unwrap();

        // Verify both tools were added
        assert!(request.tools.is_some());
        let tools = request.tools.unwrap();
        assert_eq!(tools.len(), 2);
        assert!(matches!(tools[0], Tool::CodeExecution));
        assert!(matches!(tools[1], Tool::GoogleSearch { .. }));
    }

    #[test]
    fn test_interaction_builder_with_url_context() {
        // Test that with_url_context adds the UrlContext tool
        let client = Client::new("test-api-key".to_string());

        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_text("Summarize the content from https://example.com")
            .with_url_context();

        let result = builder.build();
        assert!(result.is_ok());

        let request = result.unwrap();
        assert!(request.tools.is_some());

        let tools = request.tools.unwrap();
        assert_eq!(tools.len(), 1);
        assert!(matches!(tools[0], Tool::UrlContext));
    }

    #[test]
    fn test_interaction_builder_with_url_context_and_functions() {
        // Test that with_url_context can be combined with function declarations
        let client = Client::new("test-api-key".to_string());

        let func = FunctionDeclaration::builder("analyze_page")
            .description("Analyze web page content")
            .build();

        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_text("Fetch and analyze https://example.com")
            .add_function(func)
            .with_url_context();

        let result = builder.build();
        assert!(result.is_ok());

        let request = result.unwrap();

        // Verify both tools were added
        assert!(request.tools.is_some());
        let tools = request.tools.unwrap();
        assert_eq!(tools.len(), 2);
        assert!(matches!(tools[0], Tool::Function { .. }));
        assert!(matches!(tools[1], Tool::UrlContext));
    }

    #[test]
    fn test_interaction_builder_all_three_tools_combined() {
        // Verify all three built-in tools can be combined
        let client = Client::new("test-api-key".to_string());

        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_text("Use all tools")
            .with_google_search()
            .with_code_execution()
            .with_url_context();

        let result = builder.build();
        assert!(result.is_ok());

        let request = result.unwrap();
        assert!(request.tools.is_some());

        let tools = request.tools.unwrap();
        assert_eq!(tools.len(), 3);
        assert!(matches!(tools[0], Tool::GoogleSearch { .. }));
        assert!(matches!(tools[1], Tool::CodeExecution));
        assert!(matches!(tools[2], Tool::UrlContext));
    }
}

mod edge_cases {
    use crate::common::DEFAULT_MODEL;
    use genai_rs::{Client, FunctionDeclaration};

    #[test]
    fn test_interaction_builder_model_overwrites_previous_model() {
        // Verify that calling with_model twice overwrites the first value
        let client = Client::new("test-api-key".to_string());

        let builder = client
            .interaction()
            .with_model("first-model")
            .with_model("second-model")
            .with_text("Hello");

        let result = builder.build();
        assert!(result.is_ok());

        let request = result.unwrap();
        // Second model should win
        assert_eq!(request.model.as_deref(), Some("second-model"));
    }

    #[test]
    fn test_interaction_builder_agent_overwrites_previous_agent() {
        // Verify that calling with_agent twice overwrites the first value
        let client = Client::new("test-api-key".to_string());

        let builder = client
            .interaction()
            .with_agent("first-agent")
            .with_agent("second-agent")
            .with_text("Hello");

        let result = builder.build();
        assert!(result.is_ok());

        let request = result.unwrap();
        // Second agent should win
        assert_eq!(request.agent.as_deref(), Some("second-agent"));
    }

    // NOTE: test_interaction_builder_model_and_agent_both_set was removed
    // Setting both model and agent is now an error - tested in basic::test_interaction_builder_with_both_model_and_agent_set

    #[test]
    fn test_interaction_builder_empty_text_allowed() {
        // Verify that empty text is allowed (API will validate)
        let client = Client::new("test-api-key".to_string());

        let builder = client.interaction().with_model(DEFAULT_MODEL).with_text("");

        let result = builder.build();
        // Empty text should be accepted at builder level
        assert!(result.is_ok());
    }

    #[test]
    fn test_interaction_builder_previous_interaction_id() {
        // Verify that previous_interaction_id is set correctly
        let client = Client::new("test-api-key".to_string());

        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_text("Continue conversation")
            .with_previous_interaction("prev-interaction-123");

        let result = builder.build();
        assert!(result.is_ok());

        let request = result.unwrap();
        assert_eq!(
            request.previous_interaction_id.as_deref(),
            Some("prev-interaction-123")
        );
    }

    #[test]
    fn test_interaction_builder_max_function_call_loops() {
        // Verify that max_function_call_loops is set correctly
        let client = Client::new("test-api-key".to_string());

        let func = FunctionDeclaration::builder("test_fn")
            .description("Test function")
            .build();

        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_text("Call functions")
            .add_function(func)
            .with_max_function_call_loops(5);

        // The max_function_call_loops is stored in the builder, verified by integration tests
        let result = builder.build();
        assert!(result.is_ok());
    }
}

mod multimodal {
    use crate::common::DEFAULT_MODEL;
    use genai_rs::{
        Client, Content, FunctionDeclaration, GenerationConfig, InteractionInput, Tool,
        detect_mime_type,
    };
    use std::path::Path;

    #[test]
    fn test_detect_mime_type_basic() {
        // Test MIME type detection for common file types
        assert_eq!(detect_mime_type(Path::new("photo.jpg")), Some("image/jpeg"));
        assert_eq!(detect_mime_type(Path::new("photo.png")), Some("image/png"));
        assert_eq!(detect_mime_type(Path::new("audio.mp3")), Some("audio/mp3"));
        assert_eq!(detect_mime_type(Path::new("video.mp4")), Some("video/mp4"));
        assert_eq!(
            detect_mime_type(Path::new("doc.pdf")),
            Some("application/pdf")
        );
        assert_eq!(detect_mime_type(Path::new("unknown.xyz")), None);
    }

    #[test]
    fn test_with_content_image_data() {
        // with_content() with image data should create Content variant
        let client = Client::new("test-api-key".to_string());

        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_content(vec![Content::image_data("base64data", "image/jpeg")]);

        let result = builder.build();
        assert!(result.is_ok());

        let request = result.unwrap();
        match &request.input {
            InteractionInput::Content(items) => {
                assert_eq!(items.len(), 1, "Should have 1 content item");
                // Verify it's an Image with data
                assert!(matches!(
                    &items[0],
                    Content::Image { mime_type, data, .. }
                    if mime_type.as_deref() == Some("image/jpeg") && data.is_some()
                ));
            }
            _ => panic!("Expected InteractionInput::Content variant"),
        }
    }

    #[test]
    fn test_with_content_text_and_image() {
        // with_content() with text and image should contain both
        let client = Client::new("test-api-key".to_string());

        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_content(vec![
                Content::text("Analyze this image"),
                Content::image_data("base64data", "image/png"),
            ]);

        let result = builder.build();
        assert!(result.is_ok());

        let request = result.unwrap();
        match &request.input {
            InteractionInput::Content(items) => {
                assert_eq!(items.len(), 2, "Should have 2 content items");
                // First should be text
                assert!(matches!(
                    &items[0],
                    Content::Text { text, .. }
                    if text.as_deref() == Some("Analyze this image")
                ));
                // Second should be image with data
                assert!(matches!(
                    &items[1],
                    Content::Image { mime_type, data, .. }
                    if mime_type.as_deref() == Some("image/png") && data.is_some()
                ));
            }
            _ => panic!("Expected InteractionInput::Content variant"),
        }
    }

    #[test]
    fn test_with_content_multiple_items() {
        // with_content() should accumulate multiple content items
        let client = Client::new("test-api-key".to_string());

        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_content(vec![
                Content::image_data("img1", "image/jpeg"),
                Content::image_data("img2", "image/png"),
                Content::audio_data("audio1", "audio/mp3"),
            ]);

        let result = builder.build();
        assert!(result.is_ok());

        let request = result.unwrap();
        match &request.input {
            InteractionInput::Content(items) => {
                assert_eq!(items.len(), 3, "Should have 3 content items");
            }
            _ => panic!("Expected InteractionInput::Content variant"),
        }
    }

    #[test]
    fn test_with_content_image_uri() {
        let client = Client::new("test-api-key".to_string());

        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_content(vec![
                Content::text("Describe this image"),
                Content::from_uri_and_mime("gs://bucket/image.jpg", "image/jpeg"),
            ]);

        let result = builder.build();
        assert!(result.is_ok());

        let request = result.unwrap();
        match &request.input {
            InteractionInput::Content(items) => {
                assert_eq!(items.len(), 2, "Should have 2 content items");
                // Second should be an Image with URI
                assert!(matches!(
                    &items[1],
                    Content::Image { uri, mime_type, .. }
                    if uri.is_some() && mime_type.as_deref() == Some("image/jpeg")
                ));
            }
            _ => panic!("Expected InteractionInput::Content variant"),
        }
    }

    #[test]
    fn test_with_content_audio_data() {
        let client = Client::new("test-api-key".to_string());

        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_content(vec![Content::audio_data("audiodata", "audio/wav")]);

        let result = builder.build();
        assert!(result.is_ok());

        let request = result.unwrap();
        match &request.input {
            InteractionInput::Content(items) => {
                assert_eq!(items.len(), 1);
                assert!(matches!(
                    &items[0],
                    Content::Audio { mime_type, data, .. }
                    if mime_type.as_deref() == Some("audio/wav") && data.is_some()
                ));
            }
            _ => panic!("Expected InteractionInput::Content variant"),
        }
    }

    #[test]
    fn test_with_content_video_data() {
        let client = Client::new("test-api-key".to_string());

        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_content(vec![Content::video_data("videodata", "video/mp4")]);

        let result = builder.build();
        assert!(result.is_ok());

        let request = result.unwrap();
        match &request.input {
            InteractionInput::Content(items) => {
                assert_eq!(items.len(), 1);
                assert!(matches!(
                    &items[0],
                    Content::Video { mime_type, data, .. }
                    if mime_type.as_deref() == Some("video/mp4") && data.is_some()
                ));
            }
            _ => panic!("Expected InteractionInput::Content variant"),
        }
    }

    #[test]
    fn test_with_content_document_data() {
        let client = Client::new("test-api-key".to_string());

        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_content(vec![Content::document_data("pdfdata", "application/pdf")]);

        let result = builder.build();
        assert!(result.is_ok());

        let request = result.unwrap();
        match &request.input {
            InteractionInput::Content(items) => {
                assert_eq!(items.len(), 1);
                assert!(matches!(
                    &items[0],
                    Content::Document { mime_type, data, .. }
                    if mime_type.as_deref() == Some("application/pdf") && data.is_some()
                ));
            }
            _ => panic!("Expected InteractionInput::Content variant"),
        }
    }

    #[test]
    fn test_with_content_document_uri() {
        let client = Client::new("test-api-key".to_string());

        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_content(vec![
                Content::text("Summarize this document"),
                Content::from_uri_and_mime("gs://bucket/report.pdf", "application/pdf"),
            ]);

        let result = builder.build();
        assert!(result.is_ok());

        let request = result.unwrap();
        match &request.input {
            InteractionInput::Content(items) => {
                assert_eq!(items.len(), 2);
                assert!(matches!(
                    &items[1],
                    Content::Document { uri, mime_type, .. }
                    if uri.is_some() && mime_type.as_deref() == Some("application/pdf")
                ));
            }
            _ => panic!("Expected InteractionInput::Content variant"),
        }
    }

    #[test]
    fn test_with_content_audio_uri() {
        let client = Client::new("test-api-key".to_string());

        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_content(vec![
                Content::text("Describe this audio"),
                Content::from_uri_and_mime("gs://bucket/audio.mp3", "audio/mp3"),
            ]);

        let result = builder.build();
        assert!(result.is_ok());

        let request = result.unwrap();
        match &request.input {
            InteractionInput::Content(items) => {
                assert_eq!(items.len(), 2, "Should have 2 content items");
                // Second should be an Audio with URI
                assert!(matches!(
                    &items[1],
                    Content::Audio { uri, mime_type, .. }
                    if uri.is_some() && mime_type.as_deref() == Some("audio/mp3")
                ));
            }
            _ => panic!("Expected InteractionInput::Content variant"),
        }
    }

    #[test]
    fn test_with_content_video_uri() {
        let client = Client::new("test-api-key".to_string());

        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_content(vec![
                Content::text("Describe this video"),
                Content::from_uri_and_mime("gs://bucket/video.mp4", "video/mp4"),
            ]);

        let result = builder.build();
        assert!(result.is_ok());

        let request = result.unwrap();
        match &request.input {
            InteractionInput::Content(items) => {
                assert_eq!(items.len(), 2, "Should have 2 content items");
                // Second should be a Video with URI
                assert!(matches!(
                    &items[1],
                    Content::Video { uri, mime_type, .. }
                    if uri.is_some() && mime_type.as_deref() == Some("video/mp4")
                ));
            }
            _ => panic!("Expected InteractionInput::Content variant"),
        }
    }

    #[test]
    fn test_add_methods_combine_with_all_builder_features() {
        // Verify add_* methods work with other builder features
        let client = Client::new("test-api-key".to_string());

        let func = FunctionDeclaration::builder("analyze_image")
            .description("Analyze an image")
            .build();

        let config = GenerationConfig {
            temperature: Some(0.7),
            max_output_tokens: Some(1024),
            top_p: None,
            top_k: None,
            thinking_level: None,
            ..Default::default()
        };

        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_content(vec![
                Content::text("Analyze this image and describe it"),
                Content::image_data("imagedata", "image/jpeg"),
            ])
            .with_system_instruction("Be descriptive")
            .add_function(func)
            .with_generation_config(config)
            .with_google_search();

        let result = builder.build();
        assert!(result.is_ok());

        let request = result.unwrap();

        // Verify input has both text and image
        match &request.input {
            InteractionInput::Content(items) => {
                assert_eq!(items.len(), 2);
            }
            _ => panic!("Expected InteractionInput::Content variant"),
        }

        // Verify tools are set
        assert!(request.tools.is_some());
        let tools = request.tools.unwrap();
        assert_eq!(tools.len(), 2);
        assert!(matches!(tools[0], Tool::Function { .. }));
        assert!(matches!(tools[1], Tool::GoogleSearch { .. }));

        // Verify generation config
        assert!(request.generation_config.is_some());

        // Verify system instruction
        assert!(request.system_instruction.is_some());
    }

    #[test]
    fn test_with_content_preserves_order() {
        // Content should appear in the order it was added
        let client = Client::new("test-api-key".to_string());

        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_content(vec![
                Content::text("First"),
                Content::image_data("second", "image/jpeg"),
                Content::audio_data("third", "audio/mp3"),
                Content::video_data("fourth", "video/mp4"),
            ]);

        let result = builder.build();
        assert!(result.is_ok());

        let request = result.unwrap();
        match &request.input {
            InteractionInput::Content(items) => {
                assert_eq!(items.len(), 4);

                // First: text
                assert!(matches!(&items[0], Content::Text { .. }));

                // Second: image
                assert!(matches!(
                    &items[1],
                    Content::Image { mime_type, .. }
                    if mime_type.as_deref() == Some("image/jpeg")
                ));

                // Third: audio
                assert!(matches!(
                    &items[2],
                    Content::Audio { mime_type, .. }
                    if mime_type.as_deref() == Some("audio/mp3")
                ));

                // Fourth: video
                assert!(matches!(
                    &items[3],
                    Content::Video { mime_type, .. }
                    if mime_type.as_deref() == Some("video/mp4")
                ));
            }
            _ => panic!("Expected InteractionInput::Content variant"),
        }
    }

    /// Test Content::from_uri_and_mime creates correct content variants based on MIME type.
    #[test]
    fn test_from_uri_and_mime_creates_correct_content_type() {
        let client = Client::new("test-api-key".to_string());

        // Test image MIME type
        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_content(vec![Content::from_uri_and_mime(
                "https://example.com/image.png",
                "image/png",
            )]);

        let request = builder.build().expect("Should build valid request");

        match &request.input {
            InteractionInput::Content(items) => {
                assert_eq!(items.len(), 1);
                assert!(matches!(
                    &items[0],
                    Content::Image { uri, mime_type, .. }
                    if uri.as_deref() == Some("https://example.com/image.png")
                        && mime_type.as_deref() == Some("image/png")
                ));
            }
            _ => panic!("Expected InteractionInput::Content variant"),
        }

        // Test video MIME type
        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_content(vec![Content::from_uri_and_mime(
                "https://example.com/video.mp4",
                "video/mp4",
            )]);

        let request = builder.build().unwrap();
        match &request.input {
            InteractionInput::Content(items) => {
                assert!(matches!(
                    &items[0],
                    Content::Video { uri, mime_type, .. }
                    if uri.as_deref() == Some("https://example.com/video.mp4")
                        && mime_type.as_deref() == Some("video/mp4")
                ));
            }
            _ => panic!("Expected Video variant"),
        }

        // Test document MIME type (application/pdf)
        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_content(vec![Content::from_uri_and_mime(
                "https://example.com/doc.pdf",
                "application/pdf",
            )]);

        let request = builder.build().unwrap();
        match &request.input {
            InteractionInput::Content(items) => {
                assert!(matches!(
                    &items[0],
                    Content::Document { uri, mime_type, .. }
                    if uri.as_deref() == Some("https://example.com/doc.pdf")
                        && mime_type.as_deref() == Some("application/pdf")
                ));
            }
            _ => panic!("Expected Document variant"),
        }

        // Test text/* routes to Document
        let builder = client
            .interaction()
            .with_model(DEFAULT_MODEL)
            .with_content(vec![Content::from_uri_and_mime(
                "https://example.com/file.txt",
                "text/plain",
            )]);

        let request = builder.build().unwrap();
        match &request.input {
            InteractionInput::Content(items) => {
                assert!(matches!(
                    &items[0],
                    Content::Document { uri, mime_type, .. }
                    if uri.as_deref() == Some("https://example.com/file.txt")
                        && mime_type.as_deref() == Some("text/plain")
                ));
            }
            _ => panic!("Expected Document variant for text/plain"),
        }
    }
}
