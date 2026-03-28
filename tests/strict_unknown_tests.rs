//! Tests for strict-unknown feature flag behavior
//!
//! These tests verify that the `strict-unknown` feature flag correctly modifies
//! deserialization behavior for unknown content types.
//!
//! When `strict-unknown` is DISABLED (default):
//! - Unknown content types are captured in `Content::Unknown` variants
//! - Deserialization succeeds even for unrecognized types
//! - Unknown variants can be serialized back (round-trip support)
//!
//! When `strict-unknown` is ENABLED:
//! - Unknown content types cause deserialization errors
//! - Error messages clearly indicate the unknown type and strict mode
//!
//! # Test Organization
//!
//! Tests are organized into three modules:
//! - `graceful_handling`: Tests for default mode (strict-unknown DISABLED)
//! - `strict_mode`: Tests for strict mode (strict-unknown ENABLED)
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
//! cargo test --test strict_unknown_tests --features strict-unknown
//! ```

use genai_rs::Content;
use serde_json::json;

// =============================================================================
// Module: graceful_handling - Tests for DEFAULT behavior (strict-unknown DISABLED)
// =============================================================================

#[cfg(not(feature = "strict-unknown"))]
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
}

// =============================================================================
// Module: strict_mode - Tests for STRICT behavior (strict-unknown ENABLED)
// =============================================================================

#[cfg(feature = "strict-unknown")]
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
            err_msg.contains("strict-unknown") || err_msg.contains("feature"),
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

        // Thought
        let thought: Content =
            serde_json::from_value(json!({"type": "thought", "text": "thinking..."}))
                .expect("Thought should deserialize in strict mode");
        assert!(matches!(thought, Content::Thought { .. }));

        // Image
        let image: Content = serde_json::from_value(json!({
            "type": "image",
            "data": "base64data",
            "mime_type": "image/png"
        }))
        .expect("Image should deserialize in strict mode");
        assert!(matches!(image, Content::Image { .. }));

        // FunctionCall
        let func_call: Content = serde_json::from_value(json!({
            "type": "function_call",
            "name": "test_func",
            "arguments": {}
        }))
        .expect("FunctionCall should deserialize in strict mode");
        assert!(matches!(func_call, Content::FunctionCall { .. }));
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

        // Thought (contains signature, not text - per wire format)
        let _: Content =
            serde_json::from_value(json!({"type": "thought", "signature": "Eq0JCqoJ..."})).unwrap();

        // ThoughtSignature
        let _: Content =
            serde_json::from_value(json!({"type": "thought_signature", "signature": "sig123"}))
                .unwrap();

        // Image
        let _: Content =
            serde_json::from_value(json!({"type": "image", "data": "x", "mime_type": "image/png"}))
                .unwrap();

        // Audio
        let _: Content =
            serde_json::from_value(json!({"type": "audio", "data": "x", "mime_type": "audio/mp3"}))
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

        // FunctionCall
        let _: Content =
            serde_json::from_value(json!({"type": "function_call", "name": "fn", "arguments": {}}))
                .unwrap();

        // FunctionResult
        let _: Content = serde_json::from_value(
            json!({"type": "function_result", "name": "fn", "call_id": "1", "result": {}}),
        )
        .unwrap();

        // CodeExecutionCall
        let _: Content = serde_json::from_value(
            json!({"type": "code_execution_call", "id": "1", "language": "PYTHON", "code": "print(1)"}),
        )
        .unwrap();

        // CodeExecutionResult
        let _: Content = serde_json::from_value(json!({
            "type": "code_execution_result",
            "call_id": "1",
            "outcome": "OUTCOME_OK",
            "output": "1"
        }))
        .unwrap();

        // GoogleSearchCall
        let _: Content = serde_json::from_value(json!({
            "type": "google_search_call",
            "id": "test-id",
            "arguments": {"queries": ["test query"]}
        }))
        .unwrap();

        // GoogleSearchResult
        let _: Content = serde_json::from_value(json!({
            "type": "google_search_result",
            "call_id": "test-id",
            "result": [{"title": "Test", "url": "https://example.com"}]
        }))
        .unwrap();

        // GoogleMapsCall
        let _: Content = serde_json::from_value(json!({
            "type": "google_maps_call",
            "id": "maps-call-123",
            "arguments": {"queries": ["coffee shops near Times Square"]},
            "signature": "ErIE..."
        }))
        .unwrap();

        // GoogleMapsResult
        let _: Content = serde_json::from_value(json!({
            "type": "google_maps_result",
            "call_id": "maps-123",
            "result": [{
                "places": [{"name": "Central Park", "place_id": "abc123"}],
                "widget_context_token": "token456"
            }],
            "signature": "SigABC..."
        }))
        .unwrap();

        // UrlContextCall (wire format has id + arguments.urls)
        let _: Content = serde_json::from_value(json!({
            "type": "url_context_call",
            "id": "ctx_123",
            "arguments": {"urls": ["https://example.com"]}
        }))
        .unwrap();

        // UrlContextResult (wire format has call_id + result array)
        let _: Content = serde_json::from_value(json!({
            "type": "url_context_result",
            "call_id": "ctx_123",
            "result": [{"url": "https://example.com", "status": "success"}]
        }))
        .unwrap();
    }

    #[test]
    fn known_types_roundtrip_correctly() {
        let text = Content::Text {
            text: Some("hello".to_string()),
            annotations: None,
        };
        let json = serde_json::to_value(&text).unwrap();
        assert_eq!(json["type"], "text");
        assert_eq!(json["text"], "hello");

        let thought = Content::Thought {
            signature: Some("Eq0JCqoJ...signature".to_string()),
        };
        let json = serde_json::to_value(&thought).unwrap();
        assert_eq!(json["type"], "thought");
        assert_eq!(json["signature"], "Eq0JCqoJ...signature");

        let image = Content::Image {
            data: Some("b64".to_string()),
            uri: None,
            mime_type: Some("image/png".to_string()),
            resolution: None,
        };
        let json = serde_json::to_value(&image).unwrap();
        assert_eq!(json["type"], "image");
        assert_eq!(json["data"], "b64");
        assert_eq!(json["mime_type"], "image/png");
    }
}
