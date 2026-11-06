//! Tests for `genai_strict_unknown` cfg behavior
//!
//! These tests verify that the `genai_strict_unknown` cfg correctly modifies
//! deserialization behavior for unknown content and step types.
//!
//! When `genai_strict_unknown` is DISABLED (default):
//! - Unknown content types are captured in `Content::Unknown` variants
//! - Unknown step types are captured in `Step::Unknown` variants
//! - Deserialization succeeds even for unrecognized types
//! - Unknown variants can be serialized back (round-trip support)
//!
//! When `genai_strict_unknown` is ENABLED:
//! - Unknown content types cause deserialization errors
//! - Unknown step types cause deserialization errors
//! - Error messages clearly indicate the unknown type and strict mode
//!
//! # Test Organization
//!
//! Tests are organized into three modules:
//! - `graceful_handling`: Tests for default mode (strict mode DISABLED)
//! - `strict_mode`: Tests for strict mode (strict mode ENABLED)
//! - `common`: Tests that work in BOTH modes
//!
//! # Running Tests
//!
//! Default mode (graceful handling):
//! ```sh
//! cargo test --test strict_unknown_tests
//! ```
//!
//! Strict mode (fail on unknown):
//! ```sh
//! RUSTFLAGS='--cfg genai_strict_unknown' cargo test --test strict_unknown_tests
//! ```

use genai_rs::{Content, Step};
use serde_json::json;

// =============================================================================
// Module: graceful_handling - Tests for DEFAULT behavior (strict mode DISABLED)
// =============================================================================

#[cfg(not(genai_strict_unknown))]
mod graceful_handling {
    use super::*;

    #[test]
    fn unknown_type_deserializes_successfully() {
        let json = r#"{"type": "future_feature", "data": "test", "extra_field": 42}"#;
        let result: Result<Content, _> = serde_json::from_str(json);

        assert!(
            result.is_ok(),
            "Unknown type should deserialize successfully"
        );

        let content = result.unwrap();
        assert!(
            matches!(&content, Content::Unknown { content_type, .. } if content_type == "future_feature"),
            "Should be Unknown variant with correct content_type"
        );
    }

    #[test]
    fn unknown_variant_preserves_all_data() {
        let json = json!({
            "type": "new_api_feature",
            "field1": "value1",
            "field2": 42,
            "nested": {"a": 1, "b": 2}
        });

        let content: Content = serde_json::from_value(json.clone()).unwrap();

        if let Content::Unknown { content_type, data } = content {
            assert_eq!(content_type, "new_api_feature");
            assert_eq!(data["field1"], "value1");
            assert_eq!(data["field2"], 42);
            assert_eq!(data["nested"]["a"], 1);
        } else {
            panic!("Expected Unknown variant");
        }
    }

    #[test]
    fn unknown_variant_roundtrip_serialization() {
        let original_json = json!({
            "type": "experimental_type",
            "value": 123,
            "metadata": {"version": "1.0"}
        });

        // Deserialize
        let content: Content = serde_json::from_value(original_json.clone()).unwrap();

        // Serialize back
        let serialized = serde_json::to_value(&content).unwrap();

        // Verify key fields are preserved
        assert_eq!(serialized["type"], "experimental_type");
        assert_eq!(serialized["value"], 123);
        assert_eq!(serialized["metadata"]["version"], "1.0");
    }

    #[test]
    fn multiple_unknown_types_all_captured() {
        let items: Vec<Content> = serde_json::from_value(json!([
            {"type": "unknown_type_a", "data": "a"},
            {"type": "text", "text": "Hello"},
            {"type": "unknown_type_b", "data": "b"}
        ]))
        .unwrap();

        assert_eq!(items.len(), 3);

        // First is unknown
        assert!(matches!(
            &items[0],
            Content::Unknown { content_type, .. } if content_type == "unknown_type_a"
        ));

        // Second is known (Text)
        assert!(matches!(&items[1], Content::Text { .. }));

        // Third is unknown
        assert!(matches!(
            &items[2],
            Content::Unknown { content_type, .. } if content_type == "unknown_type_b"
        ));
    }

    #[test]
    fn is_unknown_method_works() {
        let unknown: Content =
            serde_json::from_value(json!({"type": "new_type", "data": 1})).unwrap();

        let known: Content =
            serde_json::from_value(json!({"type": "text", "text": "hello"})).unwrap();

        assert!(unknown.is_unknown());
        assert!(!known.is_unknown());
    }

    #[test]
    fn unknown_type_accessor_returns_content_type() {
        let content: Content =
            serde_json::from_value(json!({"type": "brand_new_type", "x": 1})).unwrap();

        assert_eq!(content.unknown_content_type(), Some("brand_new_type"));

        let text: Content = serde_json::from_value(json!({"type": "text", "text": "hi"})).unwrap();

        assert_eq!(text.unknown_content_type(), None);
    }

    #[test]
    fn unknown_data_accessor_returns_raw_json() {
        let content: Content =
            serde_json::from_value(json!({"type": "custom_type", "value": 42})).unwrap();

        let data = content.unknown_data().expect("Should have data");
        assert_eq!(data["value"], 42);

        let text: Content = serde_json::from_value(json!({"type": "text", "text": "hi"})).unwrap();

        assert!(text.unknown_data().is_none());
    }

    #[test]
    fn missing_type_field_handled_gracefully() {
        let json = json!({"no_type_field": "value"});
        let result: Result<Content, _> = serde_json::from_value(json);

        // Should succeed but result in Unknown with "<missing type>" marker
        assert!(result.is_ok());
        if let Content::Unknown { content_type, .. } = result.unwrap() {
            assert_eq!(content_type, "<missing type>");
        } else {
            panic!("Expected Unknown variant for missing type");
        }
    }

    // -------------------------------------------------------------------------
    // Step graceful handling (revision 2026-05-20)
    // -------------------------------------------------------------------------

    #[test]
    fn unknown_step_type_deserializes_successfully() {
        let json = r#"{"type": "future_step", "data": "test", "extra_field": 42}"#;
        let result: Result<Step, _> = serde_json::from_str(json);

        assert!(
            result.is_ok(),
            "Unknown step type should deserialize successfully"
        );

        let step = result.unwrap();
        assert!(
            matches!(&step, Step::Unknown { step_type, .. } if step_type == "future_step"),
            "Should be Unknown variant with correct step_type"
        );
    }

    #[test]
    fn unknown_step_preserves_all_data() {
        let json = json!({
            "type": "new_tool_step",
            "call_id": "call_1",
            "payload": {"a": 1}
        });

        let step: Step = serde_json::from_value(json).unwrap();

        if let Step::Unknown { step_type, data } = step {
            assert_eq!(step_type, "new_tool_step");
            assert_eq!(data["call_id"], "call_1");
            assert_eq!(data["payload"]["a"], 1);
        } else {
            panic!("Expected Unknown variant");
        }
    }

    #[test]
    fn unknown_step_roundtrip_serialization() {
        let original_json = json!({
            "type": "experimental_step",
            "value": 123,
            "signature": "sig_abc"
        });

        let step: Step = serde_json::from_value(original_json.clone()).unwrap();
        let serialized = serde_json::to_value(&step).unwrap();

        assert_eq!(serialized["type"], "experimental_step");
        assert_eq!(serialized["value"], 123);
        assert_eq!(serialized["signature"], "sig_abc");
    }

    #[test]
    fn mixed_known_and_unknown_steps_all_captured() {
        let steps: Vec<Step> = serde_json::from_value(json!([
            {"type": "user_input", "content": [{"type": "text", "text": "hi"}]},
            {"type": "unknown_step_a", "data": "a"},
            {"type": "model_output", "content": [{"type": "text", "text": "hello"}]}
        ]))
        .unwrap();

        assert_eq!(steps.len(), 3);
        assert!(matches!(&steps[0], Step::UserInput { .. }));
        assert!(matches!(
            &steps[1],
            Step::Unknown { step_type, .. } if step_type == "unknown_step_a"
        ));
        assert!(matches!(&steps[2], Step::ModelOutput { .. }));
    }

    #[test]
    fn step_missing_type_field_handled_gracefully() {
        let json = json!({"no_type_field": "value"});
        let result: Result<Step, _> = serde_json::from_value(json);

        assert!(result.is_ok());
        if let Step::Unknown { step_type, .. } = result.unwrap() {
            assert_eq!(step_type, "<missing type>");
        } else {
            panic!("Expected Unknown variant for missing type");
        }
    }
}

// =============================================================================
// Module: strict_mode - Tests for STRICT behavior (strict mode ENABLED)
// =============================================================================

#[cfg(genai_strict_unknown)]
mod strict_mode {
    use super::*;

    #[test]
    fn unknown_type_causes_deserialization_error() {
        let json = r#"{"type": "future_feature", "data": "test"}"#;
        let result: Result<Content, _> = serde_json::from_str(json);

        assert!(
            result.is_err(),
            "Unknown type should fail deserialization in strict mode"
        );
    }

    #[test]
    fn error_message_contains_unknown_content_type() {
        let json = r#"{"type": "experimental_api_type", "data": "test"}"#;
        let result: Result<Content, _> = serde_json::from_str(json);

        let err = result.expect_err("Should fail in strict mode");
        let err_msg = err.to_string();

        // Verify the type name is in the error message
        assert!(
            err_msg.contains("experimental_api_type"),
            "Error message should contain the unknown type name. Got: {}",
            err_msg
        );
    }

    #[test]
    fn error_message_mentions_strict_mode() {
        let json = r#"{"type": "new_type", "data": "test"}"#;
        let result: Result<Content, _> = serde_json::from_str(json);

        let err = result.expect_err("Should fail in strict mode");
        let err_msg = err.to_string();

        // Verify strict mode is mentioned
        assert!(
            err_msg.contains("strict") || err_msg.contains("Strict"),
            "Error message should mention strict mode. Got: {}",
            err_msg
        );
    }

    #[test]
    fn error_message_format_is_actionable() {
        let json = r#"{"type": "unknown_content_type", "data": "test"}"#;
        let result: Result<Content, _> = serde_json::from_str(json);

        let err = result.expect_err("Should fail in strict mode");
        let err_msg = err.to_string();

        // Verify the error message contains actionable guidance
        // The error message should mention how to resolve the issue
        assert!(
            err_msg.contains("genai_strict_unknown"),
            "Error message should mention the feature flag. Got: {}",
            err_msg
        );

        // Should also mention updating the library or disabling strict mode
        assert!(
            err_msg.contains("update") || err_msg.contains("disable"),
            "Error message should provide actionable guidance. Got: {}",
            err_msg
        );
    }

    #[test]
    fn known_types_still_deserialize_correctly() {
        // Text
        let text: Content = serde_json::from_value(json!({"type": "text", "text": "hello"}))
            .expect("Text should deserialize in strict mode");
        assert!(matches!(text, Content::Text { .. }));

        // Image
        let image: Content = serde_json::from_value(json!({
            "type": "image",
            "data": "base64data",
            "mime_type": "image/png"
        }))
        .expect("Image should deserialize in strict mode");
        assert!(matches!(image, Content::Image { .. }));

        // Audio (with the new sample_rate/channels fields)
        let audio: Content = serde_json::from_value(json!({
            "type": "audio",
            "data": "base64data",
            "mime_type": "audio/wav",
            "sample_rate": 24000,
            "channels": 1
        }))
        .expect("Audio should deserialize in strict mode");
        assert!(matches!(audio, Content::Audio { .. }));

        // Document
        let document: Content = serde_json::from_value(json!({
            "type": "document",
            "data": "base64data",
            "mime_type": "application/pdf"
        }))
        .expect("Document should deserialize in strict mode");
        assert!(matches!(document, Content::Document { .. }));
    }

    #[test]
    fn fails_on_any_unknown_type_in_array() {
        // Array with unknown type in the middle
        let result: Result<Vec<Content>, _> = serde_json::from_value(json!([
            {"type": "text", "text": "Hello"},
            {"type": "unknown_middle", "data": "x"},
            {"type": "text", "text": "World"}
        ]));

        assert!(
            result.is_err(),
            "Array containing unknown type should fail in strict mode"
        );
    }

    // -------------------------------------------------------------------------
    // Step strict handling (revision 2026-05-20): strict mode applies to
    // unknown Step types the same way it applies to Content.
    // -------------------------------------------------------------------------

    #[test]
    fn unknown_step_type_causes_deserialization_error() {
        let json = r#"{"type": "future_step", "data": "test"}"#;
        let result: Result<Step, _> = serde_json::from_str(json);

        assert!(
            result.is_err(),
            "Unknown step type should fail deserialization in strict mode"
        );
    }

    #[test]
    fn step_error_message_contains_type_and_guidance() {
        let json = r#"{"type": "experimental_step_type", "data": "test"}"#;
        let result: Result<Step, _> = serde_json::from_str(json);

        let err = result.expect_err("Should fail in strict mode");
        let err_msg = err.to_string();

        assert!(
            err_msg.contains("experimental_step_type"),
            "Error message should contain the unknown step type. Got: {}",
            err_msg
        );
        assert!(
            err_msg.contains("strict") || err_msg.contains("Strict"),
            "Error message should mention strict mode. Got: {}",
            err_msg
        );
        assert!(
            err_msg.contains("genai_strict_unknown"),
            "Error message should mention the feature flag. Got: {}",
            err_msg
        );
    }

    #[test]
    fn known_steps_still_deserialize_correctly() {
        let user: Step = serde_json::from_value(json!({
            "type": "user_input",
            "content": [{"type": "text", "text": "hi"}]
        }))
        .expect("user_input should deserialize in strict mode");
        assert!(matches!(user, Step::UserInput { .. }));

        let model: Step = serde_json::from_value(json!({
            "type": "model_output",
            "content": [{"type": "text", "text": "hello"}]
        }))
        .expect("model_output should deserialize in strict mode");
        assert!(matches!(model, Step::ModelOutput { .. }));

        let call: Step = serde_json::from_value(json!({
            "type": "function_call",
            "id": "call_1",
            "name": "get_weather",
            "arguments": {"city": "Paris"}
        }))
        .expect("function_call should deserialize in strict mode");
        assert!(matches!(call, Step::FunctionCall { .. }));
    }

    #[test]
    fn fails_on_any_unknown_step_in_array() {
        let result: Result<Vec<Step>, _> = serde_json::from_value(json!([
            {"type": "user_input", "content": [{"type": "text", "text": "hi"}]},
            {"type": "unknown_middle_step", "data": "x"},
            {"type": "model_output", "content": []}
        ]));

        assert!(
            result.is_err(),
            "Array containing unknown step type should fail in strict mode"
        );
    }

    /// A known step whose modeled field cannot be read is an error in
    /// strict mode. The error names the field and why, not an unknown type,
    /// and gives the strict-mode guidance once.
    #[test]
    fn malformed_known_step_error_names_the_field() {
        let err = serde_json::from_value::<Step>(json!({
            "type": "code_execution_call",
            "id": "c",
            "arguments": {"language": "javascript", "code": "x"}
        }))
        .expect_err("an unknown language fails in strict mode");
        assert_eq!(
            err.to_string(),
            "Step 'code_execution_call' has a field this crate cannot read: \
             field `arguments.language`: Unknown CodeExecutionLanguage 'javascript'. \
             Strict mode is enabled via the 'genai_strict_unknown' cfg flag. \
             Either update the library or disable strict mode."
        );

        // Kept whole as `Step::Unknown` without strict mode.
        let err = serde_json::from_value::<Step>(json!({
            "type": "url_context_call",
            "id": "c",
            "arguments": "https://a.example"
        }))
        .expect_err("a non-object `arguments` fails in strict mode");
        assert_eq!(
            err.to_string(),
            "Step 'url_context_call' has a field this crate cannot read: \
             field `arguments`: expected an object, got \"https://a.example\". \
             Strict mode is enabled via the 'genai_strict_unknown' cfg flag, which \
             rejects what would otherwise be kept whole as Step::Unknown. \
             Either update the library or disable strict mode."
        );
    }

    /// `StepDelta` is not strict as a union: an unknown delta type, or a
    /// known one with a malformed field, is still kept whole. But a value
    /// strict mode rejects inside a known delta fails, naming the field, as
    /// it does in a step: here an unknown content type in a thought summary.
    /// (`src/steps/accumulator_tests.rs` covers an unknown `language`.)
    #[test]
    fn delta_fails_only_on_a_value_strict_mode_rejects() {
        use genai_rs::StepDelta;

        let err = serde_json::from_value::<StepDelta>(json!({
            "type": "thought_summary",
            "content": {"type": "hologram", "data": "x"}
        }))
        .expect_err("an unknown content type in a delta fails in strict mode");
        assert_eq!(
            err.to_string(),
            "StepDelta 'thought_summary' has a field this crate cannot read: \
             field `content`: Unknown Content type 'hologram'. Strict mode is \
             enabled via the 'genai_strict_unknown' cfg flag. Either update the \
             library or disable strict mode."
        );

        for (wire, delta_type) in [
            (json!({"type": "future_delta", "x": 1}), "future_delta"),
            (
                json!({"type": "url_context_call", "arguments": "https://a.example"}),
                "url_context_call",
            ),
        ] {
            let delta: StepDelta = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(delta.unknown_delta_type(), Some(delta_type));
            assert_eq!(delta.unknown_data(), Some(&wire));
        }
    }

    /// The same for content, and for content nested in a step: the cause
    /// chain reaches the field that failed.
    #[test]
    fn malformed_known_content_error_names_the_field() {
        let content_error = "Content 'text' has a field this crate cannot read: \
             field `text`: invalid type: integer `7`, expected a string. \
             Strict mode is enabled via the 'genai_strict_unknown' cfg flag, which \
             rejects what would otherwise be kept whole as Content::Unknown. \
             Either update the library or disable strict mode.";
        let err = serde_json::from_value::<Content>(json!({"type": "text", "text": 7}))
            .expect_err("a non-string `text` fails in strict mode");
        assert_eq!(err.to_string(), content_error);

        let err = serde_json::from_value::<Step>(json!({
            "type": "model_output",
            "content": [{"type": "text", "text": 7}]
        }))
        .expect_err("nested malformed content fails in strict mode");
        assert_eq!(
            err.to_string(),
            format!(
                "Step 'model_output' has a field this crate cannot read: \
                 field `content`: {content_error}"
            )
        );
    }

    #[test]
    fn model_output_with_unknown_content_fails() {
        // Unknown Content nested inside a known Step also fails in strict mode.
        let result: Result<Step, _> = serde_json::from_value(json!({
            "type": "model_output",
            "content": [{"type": "future_content", "data": "x"}]
        }));

        assert!(
            result.is_err(),
            "Step containing unknown content should fail in strict mode"
        );
    }
}

// =============================================================================
// Module: common - Tests that work in BOTH modes
// =============================================================================

mod common {
    use super::*;

    #[test]
    fn all_known_content_types_deserialize() {
        // Text
        let _: Content = serde_json::from_value(json!({"type": "text", "text": "hello"})).unwrap();

        // Text with annotations
        let _: Content = serde_json::from_value(json!({
            "type": "text",
            "text": "hello",
            "annotations": [{
                "type": "url_citation",
                "url": "https://example.com",
                "title": "Example",
                "start_index": 0,
                "end_index": 5
            }]
        }))
        .unwrap();

        // Image
        let _: Content =
            serde_json::from_value(json!({"type": "image", "data": "x", "mime_type": "image/png"}))
                .unwrap();

        // Audio (including the new sample_rate/channels fields)
        let _: Content = serde_json::from_value(json!({
            "type": "audio",
            "data": "x",
            "mime_type": "audio/wav",
            "sample_rate": 24000,
            "channels": 1
        }))
        .unwrap();

        // Video
        let _: Content =
            serde_json::from_value(json!({"type": "video", "data": "x", "mime_type": "video/mp4"}))
                .unwrap();

        // Document
        let _: Content = serde_json::from_value(
            json!({"type": "document", "data": "x", "mime_type": "application/pdf"}),
        )
        .unwrap();
    }

    #[test]
    fn all_known_step_types_deserialize() {
        // UserInput
        let _: Step = serde_json::from_value(json!({
            "type": "user_input",
            "content": [{"type": "text", "text": "hi"}]
        }))
        .unwrap();

        // ModelOutput
        let _: Step = serde_json::from_value(json!({
            "type": "model_output",
            "content": [{"type": "text", "text": "hello"}]
        }))
        .unwrap();

        // Thought (signature + optional summary)
        let _: Step = serde_json::from_value(json!({
            "type": "thought",
            "signature": "Eq0JCqoJ...",
            "summary": [{"type": "text", "text": "Reasoning about the question"}]
        }))
        .unwrap();

        // FunctionCall
        let _: Step = serde_json::from_value(json!({
            "type": "function_call",
            "id": "call_1",
            "name": "get_weather",
            "arguments": {"city": "Paris"}
        }))
        .unwrap();

        // FunctionResult
        let _: Step = serde_json::from_value(json!({
            "type": "function_result",
            "call_id": "call_1",
            "name": "get_weather",
            "result": {"temp": 22},
            "is_error": false
        }))
        .unwrap();

        // CodeExecutionCall (language/code nested under arguments)
        let _: Step = serde_json::from_value(json!({
            "type": "code_execution_call",
            "id": "exec_1",
            "arguments": {"language": "python", "code": "print(1)"},
            "signature": "sig1"
        }))
        .unwrap();

        // CodeExecutionResult
        let _: Step = serde_json::from_value(json!({
            "type": "code_execution_result",
            "call_id": "exec_1",
            "result": "1",
            "is_error": false,
            "signature": "sig2"
        }))
        .unwrap();

        // UrlContextCall (urls nested under arguments)
        let _: Step = serde_json::from_value(json!({
            "type": "url_context_call",
            "id": "ctx_1",
            "arguments": {"urls": ["https://example.com"]}
        }))
        .unwrap();

        // UrlContextResult
        let _: Step = serde_json::from_value(json!({
            "type": "url_context_result",
            "call_id": "ctx_1",
            "result": [{"url": "https://example.com", "status": "success"}]
        }))
        .unwrap();

        // GoogleSearchCall (queries nested under arguments)
        let _: Step = serde_json::from_value(json!({
            "type": "google_search_call",
            "id": "search_1",
            "arguments": {"queries": ["test query"]},
            "search_type": "web_search"
        }))
        .unwrap();

        // GoogleSearchResult
        let _: Step = serde_json::from_value(json!({
            "type": "google_search_result",
            "call_id": "search_1",
            "result": [{"title": "Test", "url": "https://example.com"}]
        }))
        .unwrap();

        // McpServerToolCall
        let _: Step = serde_json::from_value(json!({
            "type": "mcp_server_tool_call",
            "id": "mcp_1",
            "name": "lookup",
            "server_name": "kb",
            "arguments": {"q": "rust"}
        }))
        .unwrap();

        // McpServerToolResult
        let _: Step = serde_json::from_value(json!({
            "type": "mcp_server_tool_result",
            "call_id": "mcp_1",
            "name": "lookup",
            "server_name": "kb",
            "result": {"answer": 42}
        }))
        .unwrap();

        // FileSearchCall
        let _: Step = serde_json::from_value(json!({
            "type": "file_search_call",
            "id": "fs_1"
        }))
        .unwrap();

        // ToolCall — the generic server-side call the API emits for MCP.
        // Under `genai_strict_unknown` an unmodeled type is a hard error rather
        // than a silent Unknown, so this list is what asserts it stays known.
        let _: Step = serde_json::from_value(json!({
            "type": "tool_call",
            "id": "call_1",
            "signature": "sig"
        }))
        .unwrap();

        // FileSearchResult
        let _: Step = serde_json::from_value(json!({
            "type": "file_search_result",
            "call_id": "fs_1",
            "result": [{"text": "chunk"}]
        }))
        .unwrap();

        // GoogleMapsCall (queries nested under arguments)
        let _: Step = serde_json::from_value(json!({
            "type": "google_maps_call",
            "id": "maps_1",
            "arguments": {"queries": ["coffee near Times Square"]},
            "signature": "ErIE..."
        }))
        .unwrap();

        // GoogleMapsResult
        let _: Step = serde_json::from_value(json!({
            "type": "google_maps_result",
            "call_id": "maps_1",
            "result": [{
                "places": [{"name": "Central Park", "place_id": "abc123"}],
                "widget_context_token": "token456"
            }],
            "signature": "SigABC..."
        }))
        .unwrap();
    }

    #[test]
    fn known_types_roundtrip_correctly() {
        let text = Content::Text {
            text: Some("hello".to_string()),
            annotations: None,
            extra: Default::default(),
        };
        let json = serde_json::to_value(&text).unwrap();
        assert_eq!(json["type"], "text");
        assert_eq!(json["text"], "hello");

        let image = Content::Image {
            data: Some("b64".to_string()),
            uri: None,
            mime_type: Some("image/png".to_string()),
            resolution: None,
            extra: Default::default(),
        };
        let json = serde_json::to_value(&image).unwrap();
        assert_eq!(json["type"], "image");
        assert_eq!(json["data"], "b64");
        assert_eq!(json["mime_type"], "image/png");

        let audio = Content::Audio {
            data: Some("b64".to_string()),
            uri: None,
            mime_type: Some("audio/wav".to_string()),
            sample_rate: Some(24000),
            channels: Some(1),
            extra: Default::default(),
        };
        let json = serde_json::to_value(&audio).unwrap();
        assert_eq!(json["type"], "audio");
        assert_eq!(json["sample_rate"], 24000);
        assert_eq!(json["channels"], 1);
    }

    #[test]
    fn known_steps_roundtrip_correctly() {
        let thought = Step::Thought {
            signature: Some("Eq0JCqoJ...signature".to_string()),
            summary: vec![],
            extra: Default::default(),
        };
        let json = serde_json::to_value(&thought).unwrap();
        assert_eq!(json["type"], "thought");
        assert_eq!(json["signature"], "Eq0JCqoJ...signature");

        let call = Step::function_call("call_1", "get_weather", json!({"city": "Paris"}));
        let json = serde_json::to_value(&call).unwrap();
        assert_eq!(json["type"], "function_call");
        assert_eq!(json["id"], "call_1");
        assert_eq!(json["name"], "get_weather");
        assert_eq!(json["arguments"]["city"], "Paris");

        let model = Step::model_text("hello");
        let json = serde_json::to_value(&model).unwrap();
        assert_eq!(json["type"], "model_output");
        assert_eq!(json["content"][0]["type"], "text");
        assert_eq!(json["content"][0]["text"], "hello");
    }
}
