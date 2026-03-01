//! Unit tests for Content types, serialization, and Unknown variant handling.

use super::*;

// --- Basic Content Serialization/Deserialization ---

#[test]
fn test_serialize_interaction_content() {
    let content = Content::Text {
        text: Some("Hello".to_string()),
        annotations: None,
    };

    let json = serde_json::to_string(&content).expect("Serialization failed");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "text");
    assert_eq!(value["text"], "Hello");
}

#[test]
fn test_deserialize_function_call_content() {
    let content_json =
        r#"{"type": "function_call", "name": "get_weather", "arguments": {"location": "Paris"}}"#;

    let content: Content = serde_json::from_str(content_json).expect("Deserialization failed");

    match content {
        Content::FunctionCall { name, args, .. } => {
            assert_eq!(name, "get_weather");
            assert_eq!(args["location"], "Paris");
        }
        _ => panic!("Expected FunctionCall variant"),
    }
}

#[test]
fn test_deserialize_function_call_streaming_start() {
    // In streaming, content.start events may only have id and type (no name/arguments).
    // This simulates what the API sends in an SSE content.start event for function_call.
    let content_json = r#"{"type": "function_call", "id": "call-abc123"}"#;

    let content: Content = serde_json::from_str(content_json).expect("Deserialization failed");

    match content {
        Content::FunctionCall { id, name, args, .. } => {
            assert_eq!(id.as_deref(), Some("call-abc123"));
            // Name defaults to empty string when not provided
            assert_eq!(name, "");
            // Args defaults to null when not provided
            assert_eq!(args, serde_json::Value::Null);
        }
        _ => panic!("Expected FunctionCall variant, got {:?}", content),
    }
}

#[test]
fn test_content_empty_text_returns_none() {
    let content = Content::Text {
        text: Some(String::new()),
        annotations: None,
    };
    assert_eq!(content.as_text(), None);

    let content_none = Content::Text {
        text: None,
        annotations: None,
    };
    assert_eq!(content_none.as_text(), None);
}

#[test]
fn test_content_thought_signature_accessor() {
    // Non-empty thought signature returns Some
    let content = Content::Thought {
        signature: Some("EosFCogFAXLI2...".to_string()),
    };
    assert_eq!(content.thought_signature(), Some("EosFCogFAXLI2..."));

    // Empty signature returns None
    let empty = Content::Thought {
        signature: Some(String::new()),
    };
    assert_eq!(empty.thought_signature(), None);

    // None signature returns None
    let none = Content::Thought { signature: None };
    assert_eq!(none.thought_signature(), None);

    // Text variant returns None for thought_signature()
    let text_content = Content::Text {
        text: Some("hello".to_string()),
        annotations: None,
    };
    assert_eq!(text_content.thought_signature(), None);
}

// --- Unknown Variant Tests ---
// Note: Tests that rely on graceful unknown handling are disabled when strict-unknown is enabled,
// since strict mode causes deserialization errors for unknown types instead of capturing them.

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_deserialize_unknown_interaction_content() {
    // Simulate a new API content type that this library doesn't know about
    // Note: code_execution_result is now a known type, so use a truly unknown type
    let unknown_json = r#"{"type": "future_api_feature", "data_field": "some_value", "count": 42}"#;

    let content: Content =
        serde_json::from_str(unknown_json).expect("Should deserialize as Unknown");

    match &content {
        Content::Unknown { content_type, data } => {
            assert_eq!(content_type, "future_api_feature");
            assert_eq!(data["data_field"], "some_value");
            assert_eq!(data["count"], 42);
        }
        _ => panic!("Expected Unknown variant, got {:?}", content),
    }

    assert!(content.is_unknown());
    assert_eq!(content.unknown_content_type(), Some("future_api_feature"));
    assert!(content.unknown_data().is_some());
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_deserialize_unknown_streaming_content() {
    // Simulate a new streaming content type that this library doesn't know about
    let unknown_json = r#"{"type": "new_feature_delta", "data": "some_value"}"#;

    let content: Content =
        serde_json::from_str(unknown_json).expect("Should deserialize as Unknown");

    assert!(content.is_unknown());
    assert_eq!(content.unknown_content_type(), Some("new_feature_delta"));

    match &content {
        Content::Unknown { content_type, data } => {
            assert_eq!(content_type, "new_feature_delta");
            assert_eq!(data["data"], "some_value");
        }
        _ => panic!("Expected Unknown variant"),
    }
}

#[test]
fn test_known_types_still_work() {
    // Ensure adding Unknown doesn't break known types
    let text_json = r#"{"type": "text", "text": "Hello"}"#;
    let content: Content = serde_json::from_str(text_json).unwrap();
    assert!(matches!(content, Content::Text { .. }));
    assert!(!content.is_unknown());

    let thought_json = r#"{"type": "thought", "text": "Thinking..."}"#;
    let content: Content = serde_json::from_str(thought_json).unwrap();
    assert!(matches!(content, Content::Thought { .. }));
    assert!(!content.is_unknown());

    let signature_json = r#"{"type": "thought_signature", "signature": "sig123"}"#;
    let content: Content = serde_json::from_str(signature_json).unwrap();
    assert!(matches!(content, Content::ThoughtSignature { .. }));
    assert!(!content.is_unknown());

    let function_json = r#"{"type": "function_call", "name": "test", "arguments": {}}"#;
    let content: Content = serde_json::from_str(function_json).unwrap();
    assert!(matches!(content, Content::FunctionCall { .. }));
    assert!(!content.is_unknown());
}

#[test]
fn test_serialize_unknown_content_roundtrip() {
    // Create an Unknown content (simulating what we'd receive from API)
    let unknown = Content::Unknown {
        content_type: "code_execution_result".to_string(),
        data: serde_json::json!({
            "outcome": "success",
            "output": "42"
        }),
    };

    // Serialize it
    let json = serde_json::to_string(&unknown).expect("Serialization should work");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    // Verify the structure: type field + flattened data
    assert_eq!(value["type"], "code_execution_result");
    assert_eq!(value["outcome"], "success");
    assert_eq!(value["output"], "42");
}

#[test]
fn test_serialize_known_variant_with_none_fields() {
    // Test that known variants with None fields serialize correctly (omit None fields)
    let text = Content::Text {
        text: None,
        annotations: None,
    };
    let json = serde_json::to_string(&text).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["type"], "text");
    assert!(value.get("text").is_none());
    assert!(value.get("annotations").is_none());

    let image = Content::Image {
        data: Some("base64data".to_string()),
        uri: None,
        mime_type: None,
        resolution: None,
    };
    let json = serde_json::to_string(&image).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["type"], "image");
    assert_eq!(value["data"], "base64data");
    assert!(value.get("uri").is_none());
    assert!(value.get("mime_type").is_none());
    assert!(value.get("resolution").is_none());

    let fc = Content::FunctionCall {
        id: None,
        name: "test_fn".to_string(),
        args: serde_json::json!({"arg": "value"}),
        partial_args: None,
        will_continue: None,
    };
    let json = serde_json::to_string(&fc).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["type"], "function_call");
    assert_eq!(value["name"], "test_fn");
    assert!(value.get("id").is_none());
}

#[test]
fn test_serialize_unknown_with_non_object_data() {
    // Test that Unknown with non-object data (array, string, number) is preserved
    let unknown_array = Content::Unknown {
        content_type: "weird_type".to_string(),
        data: serde_json::json!([1, 2, 3]),
    };
    let json = serde_json::to_string(&unknown_array).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["type"], "weird_type");
    assert_eq!(value["data"], serde_json::json!([1, 2, 3]));

    let unknown_string = Content::Unknown {
        content_type: "string_type".to_string(),
        data: serde_json::json!("just a string"),
    };
    let json = serde_json::to_string(&unknown_string).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["type"], "string_type");
    assert_eq!(value["data"], "just a string");

    let unknown_null = Content::Unknown {
        content_type: "null_type".to_string(),
        data: serde_json::Value::Null,
    };
    let json = serde_json::to_string(&unknown_null).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["type"], "null_type");
    // Null data should be omitted
    assert!(value.get("data").is_none());
}

#[test]
fn test_serialize_unknown_with_duplicate_type_field() {
    // When data contains a "type" field, it should be ignored in serialization
    // (the content_type takes precedence)
    let unknown = Content::Unknown {
        content_type: "correct_type".to_string(),
        data: serde_json::json!({
            "type": "should_be_ignored",
            "field1": "value1",
            "field2": 42
        }),
    };

    let json = serde_json::to_string(&unknown).expect("Serialization should work");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    // The type should be from content_type, not from data
    assert_eq!(value["type"], "correct_type");
    // Other fields should be preserved
    assert_eq!(value["field1"], "value1");
    assert_eq!(value["field2"], 42);
    // There should be exactly one "type" field, not two
    let obj = value.as_object().unwrap();
    let type_count = obj.keys().filter(|k| *k == "type").count();
    assert_eq!(type_count, 1);
}

#[test]
fn test_serialize_unknown_with_empty_content_type() {
    // Empty content_type is allowed but not recommended
    let unknown = Content::Unknown {
        content_type: String::new(),
        data: serde_json::json!({"field": "value"}),
    };

    let json = serde_json::to_string(&unknown).expect("Serialization should work");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "");
    assert_eq!(value["field"], "value");
}

#[test]
fn test_serialize_unknown_with_special_characters() {
    // Type names with special characters should be preserved
    let unknown = Content::Unknown {
        content_type: "special/type:with.chars-and_underscores".to_string(),
        data: serde_json::json!({"key": "value"}),
    };

    let json = serde_json::to_string(&unknown).expect("Serialization should work");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "special/type:with.chars-and_underscores");
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_unknown_manual_construction_roundtrip() {
    // Test that manually constructed Unknown variants can round-trip through JSON
    let original = Content::Unknown {
        content_type: "manual_test".to_string(),
        data: serde_json::json!({
            "nested": {"deeply": {"nested": "value"}},
            "array": [1, 2, 3],
            "number": 42,
            "boolean": true,
            "null_field": null
        }),
    };

    // Serialize
    let json = serde_json::to_string(&original).expect("Serialization should work");

    // Deserialize back
    let deserialized: Content = serde_json::from_str(&json).expect("Deserialization should work");

    // Verify it's still Unknown with same type
    assert!(deserialized.is_unknown());
    assert_eq!(deserialized.unknown_content_type(), Some("manual_test"));

    // Verify the data was preserved (check a few fields)
    if let Content::Unknown { data, .. } = deserialized {
        assert_eq!(data["nested"]["deeply"]["nested"], "value");
        assert_eq!(data["array"], serde_json::json!([1, 2, 3]));
        assert_eq!(data["number"], 42);
        assert_eq!(data["boolean"], true);
        // null_field should be present with null value (not stripped during serialization)
        assert_eq!(data.get("null_field"), Some(&serde_json::Value::Null));
    } else {
        panic!("Expected Unknown variant");
    }
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_deserialize_unknown_with_missing_type() {
    // Edge case: JSON object without a type field
    let malformed_json = r#"{"foo": "bar", "baz": 42}"#;
    let content: Content = serde_json::from_str(malformed_json).unwrap();
    match content {
        Content::Unknown { content_type, data } => {
            assert_eq!(content_type, "<missing type>");
            assert_eq!(data["foo"], "bar");
            assert_eq!(data["baz"], 42);
        }
        _ => panic!("Expected Unknown variant"),
    }
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_deserialize_unknown_with_null_type() {
    // Edge case: JSON object with null type field
    let null_type_json = r#"{"type": null, "content": "test"}"#;
    let content: Content = serde_json::from_str(null_type_json).unwrap();
    match content {
        Content::Unknown { content_type, data } => {
            assert_eq!(content_type, "<missing type>");
            assert_eq!(data["content"], "test");
        }
        _ => panic!("Expected Unknown variant"),
    }
}

// --- Built-in Tool Content Tests ---

#[test]
fn test_deserialize_code_execution_call() {
    // Test deserialization from the API format (arguments object)
    let json = r#"{"type": "code_execution_call", "id": "call_123", "arguments": {"code": "print(42)", "language": "PYTHON"}}"#;
    let content: Content = serde_json::from_str(json).expect("Should deserialize");

    match &content {
        Content::CodeExecutionCall { id, language, code } => {
            assert_eq!(*id, Some("call_123".to_string()));
            assert_eq!(*language, CodeExecutionLanguage::Python);
            assert_eq!(code, "print(42)");
        }
        _ => panic!("Expected CodeExecutionCall variant, got {:?}", content),
    }

    assert!(content.is_code_execution_call());
    assert!(!content.is_unknown());
}

#[test]
fn test_deserialize_code_execution_call_direct_fields() {
    // Test deserialization from direct fields (new format)
    let json = r#"{"type": "code_execution_call", "id": "call_123", "language": "PYTHON", "code": "print(42)"}"#;
    let content: Content = serde_json::from_str(json).expect("Should deserialize");

    match &content {
        Content::CodeExecutionCall { id, language, code } => {
            assert_eq!(*id, Some("call_123".to_string()));
            assert_eq!(*language, CodeExecutionLanguage::Python);
            assert_eq!(code, "print(42)");
        }
        _ => panic!("Expected CodeExecutionCall variant, got {:?}", content),
    }
}

#[test]
fn test_deserialize_code_execution_call_malformed_becomes_unknown() {
    // Issue #186: Malformed CodeExecutionCall (missing both direct fields and arguments)
    // should become Unknown variant per Evergreen philosophy, not silently fall back to empty code.
    let json =
        r#"{"type": "code_execution_call", "id": "call_malformed", "extra_field": "unexpected"}"#;
    let content: Content = serde_json::from_str(json).expect("Should deserialize");

    // Should be Unknown, not a CodeExecutionCall with empty code
    match &content {
        Content::Unknown { content_type, data } => {
            assert_eq!(content_type, "code_execution_call");
            // Verify the original data is preserved for debugging
            assert_eq!(data["id"], "call_malformed");
            assert_eq!(data["extra_field"], "unexpected");
            assert_eq!(data["type"], "code_execution_call");
        }
        Content::CodeExecutionCall { code, .. } => {
            panic!(
                "Should NOT be CodeExecutionCall with empty code (got code={:?}). \
                 Malformed responses should become Unknown variant.",
                code
            );
        }
        _ => panic!("Expected Unknown variant, got {:?}", content),
    }

    assert!(content.is_unknown());
    assert!(!content.is_code_execution_call());
    assert_eq!(content.unknown_content_type(), Some("code_execution_call"));
}

#[test]
fn test_deserialize_code_execution_call_malformed_roundtrip() {
    // Verify malformed CodeExecutionCall can roundtrip through Unknown variant
    let json =
        r#"{"type": "code_execution_call", "id": "call_malformed", "custom": {"nested": true}}"#;
    let content: Content = serde_json::from_str(json).expect("Should deserialize");

    assert!(content.is_unknown());

    // Serialize back
    let reserialized = serde_json::to_string(&content).expect("Should serialize");
    let value: serde_json::Value =
        serde_json::from_str(&reserialized).expect("Should parse as JSON");

    // Verify key fields are preserved
    assert_eq!(value["type"], "code_execution_call");
    assert_eq!(value["id"], "call_malformed");
    assert_eq!(value["custom"]["nested"], true);
}

#[test]
fn test_deserialize_code_execution_call_arguments_missing_code_becomes_unknown() {
    // Arguments path: when arguments object exists but code is missing, treat as Unknown
    // This is the same Evergreen pattern as the direct fields path
    let json = r#"{"type": "code_execution_call", "id": "call_args_no_code", "arguments": {"language": "PYTHON"}}"#;
    let content: Content = serde_json::from_str(json).expect("Should deserialize");

    // Should be Unknown, not CodeExecutionCall with empty code
    match &content {
        Content::Unknown { content_type, data } => {
            assert_eq!(content_type, "code_execution_call");
            assert_eq!(data["id"], "call_args_no_code");
            assert_eq!(data["arguments"]["language"], "PYTHON");
        }
        Content::CodeExecutionCall { code, .. } => {
            panic!(
                "Should NOT be CodeExecutionCall with empty code (got code={:?}). \
                 Arguments path should also treat missing code as Unknown.",
                code
            );
        }
        _ => panic!("Expected Unknown variant, got {:?}", content),
    }

    assert!(content.is_unknown());
}

#[test]
fn test_deserialize_code_execution_call_arguments_valid() {
    // Arguments path: when arguments object has code, should work normally
    let json = r#"{"type": "code_execution_call", "id": "call_valid", "arguments": {"language": "PYTHON", "code": "print('hi')"}}"#;
    let content: Content = serde_json::from_str(json).expect("Should deserialize");

    match &content {
        Content::CodeExecutionCall { id, language, code } => {
            assert_eq!(*id, Some("call_valid".to_string()));
            assert_eq!(*language, CodeExecutionLanguage::Python);
            assert_eq!(code, "print('hi')");
        }
        _ => panic!("Expected CodeExecutionCall variant, got {:?}", content),
    }

    assert!(content.is_code_execution_call());
    assert!(!content.is_unknown());
}

#[test]
fn test_deserialize_code_execution_result() {
    // Test deserialization from API wire format (is_error + result)
    let json = r#"{"type": "code_execution_result", "call_id": "call_123", "is_error": false, "result": "42\n"}"#;
    let content: Content = serde_json::from_str(json).expect("Should deserialize");

    match &content {
        Content::CodeExecutionResult {
            call_id,
            is_error,
            result,
        } => {
            assert_eq!(*call_id, Some("call_123".to_string()));
            assert!(!is_error);
            assert_eq!(result, "42\n");
        }
        _ => panic!("Expected CodeExecutionResult variant, got {:?}", content),
    }

    assert!(content.is_code_execution_result());
    assert!(!content.is_unknown());
}

#[test]
fn test_deserialize_code_execution_result_error() {
    let json = r#"{"type": "code_execution_result", "call_id": "call_456", "is_error": true, "result": "NameError: x not defined"}"#;
    let content: Content = serde_json::from_str(json).expect("Should deserialize");

    match &content {
        Content::CodeExecutionResult {
            call_id,
            is_error,
            result,
        } => {
            assert_eq!(*call_id, Some("call_456".to_string()));
            assert!(is_error);
            assert!(result.contains("NameError"));
        }
        _ => panic!("Expected CodeExecutionResult variant, got {:?}", content),
    }
}

// =============================================================================
// CodeExecutionLanguage Unknown Variant Tests
// =============================================================================

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_code_execution_language_unknown_deserialization() {
    // Simulate a new language the library doesn't know about
    let unknown_json = r#""JAVASCRIPT""#;
    let language: CodeExecutionLanguage =
        serde_json::from_str(unknown_json).expect("Should deserialize as Unknown");

    assert!(language.is_unknown());

    // Verify helper methods
    assert_eq!(language.unknown_language_type(), Some("JAVASCRIPT"));
    assert!(language.unknown_data().is_some());

    // Verify roundtrip serialization preserves the value
    let reserialized = serde_json::to_string(&language).expect("Should serialize");
    assert_eq!(reserialized, r#""JAVASCRIPT""#);
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_code_execution_language_unknown_display() {
    let unknown = CodeExecutionLanguage::Unknown {
        language_type: "RUST".to_string(),
        data: serde_json::Value::String("RUST".to_string()),
    };
    assert_eq!(format!("{}", unknown), "RUST");
}

#[test]
fn test_code_execution_language_known_variants_serde() {
    // Verify Python roundtrips correctly
    let language = CodeExecutionLanguage::Python;
    let serialized = serde_json::to_string(&language).expect("Should serialize");
    assert_eq!(serialized, r#""PYTHON""#);

    let deserialized: CodeExecutionLanguage =
        serde_json::from_str(&serialized).expect("Should deserialize");
    assert_eq!(deserialized, language);
    assert!(!deserialized.is_unknown());
}

#[test]
fn test_deserialize_google_search_call() {
    // Test the actual API format: arguments.queries is an array
    let json = r#"{"type": "google_search_call", "id": "call123", "arguments": {"queries": ["Rust programming", "latest version"]}}"#;
    let content: Content = serde_json::from_str(json).expect("Should deserialize");

    match &content {
        Content::GoogleSearchCall { id, queries } => {
            assert_eq!(id, "call123");
            assert_eq!(queries.len(), 2);
            assert_eq!(queries[0], "Rust programming");
            assert_eq!(queries[1], "latest version");
        }
        _ => panic!("Expected GoogleSearchCall variant, got {:?}", content),
    }

    assert!(content.is_google_search_call());
    assert!(!content.is_unknown());
}

#[test]
fn test_deserialize_google_search_result() {
    // Test the actual API format: result is an array of objects with title/url
    let json = r#"{"type": "google_search_result", "call_id": "call123", "result": [{"title": "Rust", "url": "https://rust-lang.org", "rendered_content": "Some content"}]}"#;
    let content: Content = serde_json::from_str(json).expect("Should deserialize");

    match &content {
        Content::GoogleSearchResult { call_id, result } => {
            assert_eq!(call_id, "call123");
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].title, "Rust");
            assert_eq!(result[0].url, "https://rust-lang.org");
            assert_eq!(result[0].rendered_content.as_deref(), Some("Some content"));
        }
        _ => panic!("Expected GoogleSearchResult variant, got {:?}", content),
    }

    assert!(content.is_google_search_result());
    assert!(!content.is_unknown());
}

#[test]
fn test_deserialize_url_context_call() {
    // Wire format from LOUD_WIRE: {"type": "url_context_call", "id": "...", "arguments": {"urls": [...]}}
    let json = r#"{"type": "url_context_call", "id": "ctx_123", "arguments": {"urls": ["https://example.com", "https://example.org"]}}"#;
    let content: Content = serde_json::from_str(json).expect("Should deserialize");

    match &content {
        Content::UrlContextCall { id, urls } => {
            assert_eq!(id, "ctx_123");
            assert_eq!(urls.len(), 2);
            assert_eq!(urls[0], "https://example.com");
            assert_eq!(urls[1], "https://example.org");
        }
        _ => panic!("Expected UrlContextCall variant, got {:?}", content),
    }

    assert!(content.is_url_context_call());
    assert!(!content.is_unknown());
}

#[test]
fn test_deserialize_url_context_result() {
    // Wire format from LOUD_WIRE: {"type": "url_context_result", "call_id": "...", "result": [{"url": "...", "status": "..."}]}
    let json = r#"{"type": "url_context_result", "call_id": "ctx_123", "result": [{"url": "https://example.com", "status": "success"}, {"url": "https://example.org", "status": "error"}]}"#;
    let content: Content = serde_json::from_str(json).expect("Should deserialize");

    match &content {
        Content::UrlContextResult { call_id, result } => {
            assert_eq!(call_id, "ctx_123");
            assert_eq!(result.len(), 2);
            assert_eq!(result[0].url, "https://example.com");
            assert_eq!(result[0].status, "success");
            assert!(result[0].is_success());
            assert_eq!(result[1].url, "https://example.org");
            assert_eq!(result[1].status, "error");
            assert!(result[1].is_error());
        }
        _ => panic!("Expected UrlContextResult variant, got {:?}", content),
    }

    assert!(content.is_url_context_result());
    assert!(!content.is_unknown());
}

#[test]
fn test_url_context_result_with_empty_result_array() {
    // Test UrlContextResult with empty result array
    let content = Content::UrlContextResult {
        call_id: "ctx_empty".to_string(),
        result: vec![],
    };

    // Serialize and verify structure
    let json = serde_json::to_string(&content).expect("Serialization should work");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "url_context_result");
    assert_eq!(value["call_id"], "ctx_empty");
    assert!(value["result"].as_array().unwrap().is_empty());

    // Deserialize with empty result array
    let json_empty_result =
        r#"{"type": "url_context_result", "call_id": "ctx_empty", "result": []}"#;
    let deserialized: Content =
        serde_json::from_str(json_empty_result).expect("Should deserialize");

    match &deserialized {
        Content::UrlContextResult { call_id, result } => {
            assert_eq!(call_id, "ctx_empty");
            assert!(result.is_empty());
        }
        _ => panic!("Expected UrlContextResult variant"),
    }
}

#[test]
fn test_url_context_result_item_status_helpers() {
    use crate::UrlContextResultItem;

    let success_item = UrlContextResultItem::new("https://example.com", "success");
    assert!(success_item.is_success());
    assert!(!success_item.is_error());
    assert!(!success_item.is_unsafe());

    let error_item = UrlContextResultItem::new("https://example.org", "error");
    assert!(!error_item.is_success());
    assert!(error_item.is_error());
    assert!(!error_item.is_unsafe());

    let unsafe_item = UrlContextResultItem::new("https://malware.example", "unsafe");
    assert!(!unsafe_item.is_success());
    assert!(!unsafe_item.is_error());
    assert!(unsafe_item.is_unsafe());
}

#[test]
fn test_serialize_code_execution_call() {
    let content = Content::CodeExecutionCall {
        id: Some("call_123".to_string()),
        language: CodeExecutionLanguage::Python,
        code: "print(42)".to_string(),
    };

    let json = serde_json::to_string(&content).expect("Serialization should work");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "code_execution_call");
    assert_eq!(value["id"], "call_123");
    // Wire format nests language and code inside arguments
    assert_eq!(value["arguments"]["language"], "PYTHON");
    assert_eq!(value["arguments"]["code"], "print(42)");
}

#[test]
fn test_serialize_code_execution_result() {
    let content = Content::CodeExecutionResult {
        call_id: Some("call_123".to_string()),
        is_error: false,
        result: "42".to_string(),
    };

    let json = serde_json::to_string(&content).expect("Serialization should work");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "code_execution_result");
    assert_eq!(value["call_id"], "call_123"); // serializes without wrapping
    assert_eq!(value["is_error"], false);
    assert_eq!(value["result"], "42");
}

#[test]
fn test_serialize_code_execution_result_error() {
    let content = Content::CodeExecutionResult {
        call_id: Some("call_456".to_string()),
        is_error: true,
        result: "NameError: x not defined".to_string(),
    };

    let json = serde_json::to_string(&content).expect("Serialization should work");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "code_execution_result");
    assert_eq!(value["call_id"], "call_456");
    assert_eq!(value["is_error"], true);
    assert!(value["result"].as_str().unwrap().contains("NameError"));
}

#[test]
fn test_roundtrip_built_in_tool_content() {
    // CodeExecutionCall roundtrip
    let original = Content::CodeExecutionCall {
        id: Some("call_123".to_string()),
        language: CodeExecutionLanguage::Python,
        code: "print('hello')".to_string(),
    };
    let json = serde_json::to_string(&original).unwrap();
    let restored: Content = serde_json::from_str(&json).unwrap();
    assert!(matches!(restored, Content::CodeExecutionCall { .. }));

    // CodeExecutionResult roundtrip
    let original = Content::CodeExecutionResult {
        call_id: Some("call_123".to_string()),
        is_error: false,
        result: "hello\n".to_string(),
    };
    let json = serde_json::to_string(&original).unwrap();
    let restored: Content = serde_json::from_str(&json).unwrap();
    assert!(matches!(restored, Content::CodeExecutionResult { .. }));

    // GoogleSearchCall roundtrip
    let original = Content::GoogleSearchCall {
        id: "call123".to_string(),
        queries: vec!["test query".to_string()],
    };
    let json = serde_json::to_string(&original).unwrap();
    let restored: Content = serde_json::from_str(&json).unwrap();
    assert!(matches!(restored, Content::GoogleSearchCall { .. }));

    // GoogleSearchResult roundtrip
    let original = Content::GoogleSearchResult {
        call_id: "call123".to_string(),
        result: vec![],
    };
    let json = serde_json::to_string(&original).unwrap();
    let restored: Content = serde_json::from_str(&json).unwrap();
    assert!(matches!(restored, Content::GoogleSearchResult { .. }));

    // UrlContextCall roundtrip
    let original = Content::UrlContextCall {
        id: "ctx_123".to_string(),
        urls: vec!["https://example.com".to_string()],
    };
    let json = serde_json::to_string(&original).unwrap();
    let restored: Content = serde_json::from_str(&json).unwrap();
    assert!(matches!(restored, Content::UrlContextCall { .. }));

    // UrlContextResult roundtrip
    let original = Content::UrlContextResult {
        call_id: "ctx_123".to_string(),
        result: vec![UrlContextResultItem::new("https://example.com", "success")],
    };
    let json = serde_json::to_string(&original).unwrap();
    let restored: Content = serde_json::from_str(&json).unwrap();
    assert!(matches!(restored, Content::UrlContextResult { .. }));
}

#[test]
fn test_edge_cases_empty_values() {
    // Empty code in CodeExecutionCall
    let content = Content::CodeExecutionCall {
        id: Some("call_empty".to_string()),
        language: CodeExecutionLanguage::Python,
        code: "".to_string(),
    };
    let json = serde_json::to_string(&content).unwrap();
    let restored: Content = serde_json::from_str(&json).unwrap();
    match restored {
        Content::CodeExecutionCall { id, language, code } => {
            assert_eq!(id, Some("call_empty".to_string()));
            assert_eq!(language, CodeExecutionLanguage::Python);
            assert!(code.is_empty());
        }
        _ => panic!("Expected CodeExecutionCall"),
    }

    // Empty results in GoogleSearchResult
    let content = Content::GoogleSearchResult {
        call_id: "call_empty".to_string(),
        result: vec![],
    };
    let json = serde_json::to_string(&content).unwrap();
    let restored: Content = serde_json::from_str(&json).unwrap();
    assert!(matches!(restored, Content::GoogleSearchResult { .. }));

    // UrlContextResult with unsafe status item
    let content = Content::UrlContextResult {
        call_id: "ctx_unsafe".to_string(),
        result: vec![UrlContextResultItem::new(
            "https://blocked.example.com",
            "unsafe",
        )],
    };
    let json = serde_json::to_string(&content).unwrap();
    let restored: Content = serde_json::from_str(&json).unwrap();
    match restored {
        Content::UrlContextResult { call_id, result } => {
            assert_eq!(call_id, "ctx_unsafe");
            assert_eq!(result.len(), 1);
            assert!(result[0].is_unsafe());
        }
        _ => panic!("Expected UrlContextResult"),
    }

    // Empty result string in CodeExecutionResult
    let content = Content::CodeExecutionResult {
        call_id: Some("call_no_output".to_string()),
        is_error: false,
        result: "".to_string(),
    };
    let json = serde_json::to_string(&content).unwrap();
    let restored: Content = serde_json::from_str(&json).unwrap();
    match restored {
        Content::CodeExecutionResult {
            call_id, result, ..
        } => {
            assert_eq!(call_id, Some("call_no_output".to_string()));
            assert!(result.is_empty());
        }
        _ => panic!("Expected CodeExecutionResult"),
    }
}

#[test]
fn test_deserialize_response_with_built_in_tool_outputs() {
    // Test deserializing a full response that contains built-in tool content
    // Note: code_execution_call and code_execution_result are now known types
    let response_json = r#"{
        "id": "interaction_789",
        "model": "gemini-3-flash-preview",
        "input": [{"type": "text", "text": "Execute some code"}],
        "outputs": [
            {"type": "text", "text": "Here's the result:"},
            {"type": "code_execution_call", "id": "call_abc", "arguments": {"code": "print(42)", "language": "python"}},
            {"type": "code_execution_result", "call_id": "call_abc", "is_error": false, "result": "42"}
        ],
        "status": "completed"
    }"#;

    let response: InteractionResponse =
        serde_json::from_str(response_json).expect("Should deserialize with built-in tool types");

    assert_eq!(response.id.as_deref(), Some("interaction_789"));
    assert_eq!(response.outputs.len(), 3);
    assert!(response.has_text());
    assert!(response.has_code_execution_calls());
    assert!(response.has_code_execution_results());
    assert!(!response.has_unknown()); // These are now known types

    let summary = response.content_summary();
    assert_eq!(summary.text_count, 1);
    assert_eq!(summary.code_execution_call_count, 1);
    assert_eq!(summary.code_execution_result_count, 1);
    assert_eq!(summary.unknown_count, 0);
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_deserialize_response_with_unknown_in_outputs() {
    // Test deserializing a full response that contains truly unknown content
    let response_json = r#"{
        "id": "interaction_789",
        "model": "gemini-3-flash-preview",
        "input": [{"type": "text", "text": "Do something"}],
        "outputs": [
            {"type": "text", "text": "Result:"},
            {"type": "future_tool_result", "data": "some_data"},
            {"type": "another_unknown_type", "value": 123}
        ],
        "status": "completed"
    }"#;

    let response: InteractionResponse =
        serde_json::from_str(response_json).expect("Should deserialize with unknown types");

    assert_eq!(response.id.as_deref(), Some("interaction_789"));
    assert_eq!(response.outputs.len(), 3);
    assert!(response.has_text());
    assert!(response.has_unknown());

    let summary = response.content_summary();
    assert_eq!(summary.text_count, 1);
    assert_eq!(summary.unknown_count, 2);
    assert!(
        summary
            .unknown_types
            .contains(&"future_tool_result".to_string())
    );
    assert!(
        summary
            .unknown_types
            .contains(&"another_unknown_type".to_string())
    );
}

// --- Annotation Tests ---

#[test]
fn test_annotation_struct_basics() {
    let annotation = Annotation {
        start_index: 0,
        end_index: 10,
        source: Some("https://example.com".to_string()),
    };

    assert_eq!(annotation.byte_len(), 10);
    assert!(annotation.has_source());

    let no_source = Annotation {
        start_index: 5,
        end_index: 15,
        source: None,
    };
    assert_eq!(no_source.byte_len(), 10);
    assert!(!no_source.has_source());
}

#[test]
fn test_annotation_extract_span() {
    let text = "Hello, world!";
    let annotation = Annotation {
        start_index: 0,
        end_index: 5,
        source: None,
    };
    assert_eq!(annotation.extract_span(text), Some("Hello"));

    let annotation_mid = Annotation {
        start_index: 7,
        end_index: 12,
        source: None,
    };
    assert_eq!(annotation_mid.extract_span(text), Some("world"));

    // Out of bounds
    let out_of_bounds = Annotation {
        start_index: 100,
        end_index: 200,
        source: None,
    };
    assert_eq!(out_of_bounds.extract_span(text), None);
}

#[test]
fn test_annotation_extract_span_utf8() {
    // Test with UTF-8 text - annotations use byte indices
    let text = "Héllo, 世界!"; // "Héllo" = 6 bytes (H=1, é=2, l=1, l=1, o=1), ", " = 2 bytes, "世界" = 6 bytes
    let annotation = Annotation {
        start_index: 0,
        end_index: 6, // "Héllo" in bytes
        source: None,
    };
    assert_eq!(annotation.extract_span(text), Some("Héllo"));

    // Extract Chinese characters
    let world_annotation = Annotation {
        start_index: 8, // After "Héllo, "
        end_index: 14,  // "世界" is 6 bytes
        source: None,
    };
    assert_eq!(world_annotation.extract_span(text), Some("世界"));
}

#[test]
fn test_serialize_text_with_annotations() {
    let annotations = vec![
        Annotation {
            start_index: 0,
            end_index: 5,
            source: Some("https://example.com".to_string()),
        },
        Annotation {
            start_index: 10,
            end_index: 20,
            source: None,
        },
    ];

    let content = Content::Text {
        text: Some("Hello, world! This is grounded text.".to_string()),
        annotations: Some(annotations),
    };

    let json = serde_json::to_string(&content).expect("Serialization failed");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "text");
    assert_eq!(value["text"], "Hello, world! This is grounded text.");
    assert!(value["annotations"].is_array());
    assert_eq!(value["annotations"].as_array().unwrap().len(), 2);
    assert_eq!(value["annotations"][0]["start_index"], 0);
    assert_eq!(value["annotations"][0]["end_index"], 5);
    assert_eq!(value["annotations"][0]["source"], "https://example.com");
    assert_eq!(value["annotations"][1]["start_index"], 10);
    assert_eq!(value["annotations"][1]["end_index"], 20);
    assert!(value["annotations"][1].get("source").is_none());
}

#[test]
fn test_serialize_text_with_empty_annotations_omitted() {
    // Empty annotations array should not be serialized
    let content = Content::Text {
        text: Some("Plain text".to_string()),
        annotations: Some(vec![]),
    };

    let json = serde_json::to_string(&content).expect("Serialization failed");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "text");
    assert_eq!(value["text"], "Plain text");
    // Empty annotations should not be serialized
    assert!(value.get("annotations").is_none());
}

#[test]
fn test_deserialize_text_with_annotations() {
    let json = r#"{
        "type": "text",
        "text": "This is grounded text.",
        "annotations": [
            {"start_index": 0, "end_index": 4, "source": "https://example.com"},
            {"start_index": 8, "end_index": 16}
        ]
    }"#;

    let content: Content = serde_json::from_str(json).expect("Deserialization failed");

    match content {
        Content::Text { text, annotations } => {
            assert_eq!(text, Some("This is grounded text.".to_string()));
            let annots = annotations.expect("Should have annotations");
            assert_eq!(annots.len(), 2);
            assert_eq!(annots[0].start_index, 0);
            assert_eq!(annots[0].end_index, 4);
            assert_eq!(annots[0].source, Some("https://example.com".to_string()));
            assert_eq!(annots[1].start_index, 8);
            assert_eq!(annots[1].end_index, 16);
            assert_eq!(annots[1].source, None);
        }
        _ => panic!("Expected Text variant"),
    }
}

#[test]
fn test_deserialize_text_without_annotations() {
    let json = r#"{"type": "text", "text": "Plain text"}"#;

    let content: Content = serde_json::from_str(json).expect("Deserialization failed");

    match content {
        Content::Text { text, annotations } => {
            assert_eq!(text, Some("Plain text".to_string()));
            assert!(annotations.is_none());
        }
        _ => panic!("Expected Text variant"),
    }
}

#[test]
fn test_text_with_annotations_roundtrip() {
    let annotations = vec![Annotation {
        start_index: 5,
        end_index: 15,
        source: Some("https://source.example.com".to_string()),
    }];

    let original = Content::Text {
        text: Some("Some grounded content here.".to_string()),
        annotations: Some(annotations),
    };

    let json = serde_json::to_string(&original).expect("Serialization failed");
    let deserialized: Content = serde_json::from_str(&json).expect("Deserialization failed");

    // Compare by re-serializing and checking the JSON
    let roundtrip_json = serde_json::to_string(&deserialized).expect("Serialization failed");
    assert_eq!(json, roundtrip_json);

    // Also verify content matches
    match deserialized {
        Content::Text { text, annotations } => {
            assert_eq!(text, Some("Some grounded content here.".to_string()));
            let annots = annotations.expect("Should have annotations");
            assert_eq!(annots.len(), 1);
            assert_eq!(annots[0].start_index, 5);
            assert_eq!(annots[0].end_index, 15);
            assert_eq!(
                annots[0].source,
                Some("https://source.example.com".to_string())
            );
        }
        _ => panic!("Expected Text variant"),
    }
}

#[test]
fn test_annotations_helper_method() {
    let content_with_annotations = Content::Text {
        text: Some("Hello".to_string()),
        annotations: Some(vec![Annotation {
            start_index: 0,
            end_index: 5,
            source: None,
        }]),
    };

    assert!(content_with_annotations.annotations().is_some());
    assert_eq!(content_with_annotations.annotations().unwrap().len(), 1);

    let content_without_annotations = Content::Text {
        text: Some("Hello".to_string()),
        annotations: None,
    };

    assert!(content_without_annotations.annotations().is_none());

    // Non-text content returns None
    let thought = Content::Thought {
        signature: Some("sig_thinking".to_string()),
    };
    assert!(thought.annotations().is_none());
}

#[test]
fn test_annotation_extract_span_inverted_indices() {
    // Edge case: start_index > end_index (malformed annotation)
    // The implementation should gracefully return None
    let inverted = Annotation {
        start_index: 10,
        end_index: 5, // start > end - invalid range
        source: None,
    };

    let text = "Hello, world!";
    assert_eq!(
        inverted.extract_span(text),
        None,
        "Inverted indices should return None"
    );

    // byte_len should use saturating_sub to avoid underflow
    assert_eq!(
        inverted.byte_len(),
        0,
        "byte_len of inverted indices should be 0 (saturating_sub)"
    );
}

#[test]
fn test_annotation_extract_span_zero_length() {
    // Edge case: start_index == end_index (zero-length span)
    let zero_len = Annotation {
        start_index: 5,
        end_index: 5,
        source: Some("https://example.com".to_string()),
    };

    let text = "Hello, world!";
    assert_eq!(
        zero_len.extract_span(text),
        Some(""),
        "Zero-length span should return empty string"
    );
    assert_eq!(zero_len.byte_len(), 0);
}

#[test]
fn test_annotation_extract_span_mid_utf8_boundary() {
    // Edge case: indices that land in the middle of a multi-byte character
    // "世" is a 3-byte UTF-8 character (E4 B8 96)
    let text = "Hello, 世界!"; // "世" starts at byte 7, "界" starts at byte 10

    // Try to slice starting in the middle of "世" (byte 8)
    let mid_start = Annotation {
        start_index: 8, // Middle of "世"
        end_index: 13,
        source: None,
    };
    assert_eq!(
        mid_start.extract_span(text),
        None,
        "Slicing from middle of UTF-8 character should return None"
    );

    // Try to slice ending in the middle of "界" (byte 11)
    let mid_end = Annotation {
        start_index: 7, // Start of "世"
        end_index: 11,  // Middle of "界"
        source: None,
    };
    assert_eq!(
        mid_end.extract_span(text),
        None,
        "Slicing to middle of UTF-8 character should return None"
    );

    // Valid slice of "世界" (bytes 7-13)
    let valid_cjk = Annotation {
        start_index: 7,
        end_index: 13,
        source: None,
    };
    assert_eq!(
        valid_cjk.extract_span(text),
        Some("世界"),
        "Valid CJK character slice should work"
    );
}

// --- Resolution Tests ---

#[test]
fn test_resolution_enum_serialization() {
    // Test that Resolution serializes to snake_case
    assert_eq!(serde_json::to_string(&Resolution::Low).unwrap(), "\"low\"");
    assert_eq!(
        serde_json::to_string(&Resolution::Medium).unwrap(),
        "\"medium\""
    );
    assert_eq!(
        serde_json::to_string(&Resolution::High).unwrap(),
        "\"high\""
    );
    assert_eq!(
        serde_json::to_string(&Resolution::UltraHigh).unwrap(),
        "\"ultra_high\""
    );
}

#[test]
fn test_resolution_enum_deserialization() {
    assert_eq!(
        serde_json::from_str::<Resolution>("\"low\"").unwrap(),
        Resolution::Low
    );
    assert_eq!(
        serde_json::from_str::<Resolution>("\"medium\"").unwrap(),
        Resolution::Medium
    );
    assert_eq!(
        serde_json::from_str::<Resolution>("\"high\"").unwrap(),
        Resolution::High
    );
    assert_eq!(
        serde_json::from_str::<Resolution>("\"ultra_high\"").unwrap(),
        Resolution::UltraHigh
    );
}

#[test]
fn test_resolution_default_is_medium() {
    assert_eq!(Resolution::default(), Resolution::Medium);
}

#[test]
fn test_image_with_resolution_serialization() {
    let image = Content::Image {
        data: Some("base64data".to_string()),
        uri: None,
        mime_type: Some("image/png".to_string()),
        resolution: Some(Resolution::High),
    };

    let json = serde_json::to_string(&image).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "image");
    assert_eq!(value["data"], "base64data");
    assert_eq!(value["mime_type"], "image/png");
    assert_eq!(value["resolution"], "high");
}

#[test]
fn test_image_with_ultra_high_resolution_serialization() {
    let image = Content::Image {
        data: None,
        uri: Some("https://example.com/image.png".to_string()),
        mime_type: Some("image/png".to_string()),
        resolution: Some(Resolution::UltraHigh),
    };

    let json = serde_json::to_string(&image).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "image");
    assert_eq!(value["uri"], "https://example.com/image.png");
    assert_eq!(value["resolution"], "ultra_high");
}

#[test]
fn test_video_with_resolution_serialization() {
    let video = Content::Video {
        data: Some("videobytes".to_string()),
        uri: None,
        mime_type: Some("video/mp4".to_string()),
        resolution: Some(Resolution::Low),
    };

    let json = serde_json::to_string(&video).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "video");
    assert_eq!(value["data"], "videobytes");
    assert_eq!(value["mime_type"], "video/mp4");
    assert_eq!(value["resolution"], "low");
}

#[test]
fn test_image_with_resolution_deserialization() {
    let json = r#"{"type": "image", "data": "base64data", "mime_type": "image/png", "resolution": "high"}"#;
    let content: Content = serde_json::from_str(json).unwrap();

    match content {
        Content::Image {
            data,
            uri,
            mime_type,
            resolution,
        } => {
            assert_eq!(data, Some("base64data".to_string()));
            assert_eq!(uri, None);
            assert_eq!(mime_type, Some("image/png".to_string()));
            assert_eq!(resolution, Some(Resolution::High));
        }
        _ => panic!("Expected Image variant"),
    }
}

#[test]
fn test_video_with_resolution_deserialization() {
    let json = r#"{"type": "video", "uri": "https://example.com/video.mp4", "mime_type": "video/mp4", "resolution": "ultra_high"}"#;
    let content: Content = serde_json::from_str(json).unwrap();

    match content {
        Content::Video {
            data,
            uri,
            mime_type,
            resolution,
        } => {
            assert_eq!(data, None);
            assert_eq!(uri, Some("https://example.com/video.mp4".to_string()));
            assert_eq!(mime_type, Some("video/mp4".to_string()));
            assert_eq!(resolution, Some(Resolution::UltraHigh));
        }
        _ => panic!("Expected Video variant"),
    }
}

#[test]
fn test_image_without_resolution_deserialization() {
    // Verify backward compatibility: images without resolution field should work
    let json = r#"{"type": "image", "data": "base64data", "mime_type": "image/png"}"#;
    let content: Content = serde_json::from_str(json).unwrap();

    match content {
        Content::Image { resolution, .. } => {
            assert_eq!(resolution, None);
        }
        _ => panic!("Expected Image variant"),
    }
}

#[test]
fn test_image_with_resolution_roundtrip() {
    let original = Content::Image {
        data: Some("testdata".to_string()),
        uri: None,
        mime_type: Some("image/jpeg".to_string()),
        resolution: Some(Resolution::Medium),
    };

    let json = serde_json::to_string(&original).unwrap();
    let restored: Content = serde_json::from_str(&json).unwrap();

    match restored {
        Content::Image {
            data,
            uri,
            mime_type,
            resolution,
        } => {
            assert_eq!(data, Some("testdata".to_string()));
            assert_eq!(uri, None);
            assert_eq!(mime_type, Some("image/jpeg".to_string()));
            assert_eq!(resolution, Some(Resolution::Medium));
        }
        _ => panic!("Expected Image variant"),
    }
}

#[test]
fn test_video_with_resolution_roundtrip() {
    let original = Content::Video {
        data: None,
        uri: Some("gs://bucket/video.mp4".to_string()),
        mime_type: Some("video/mp4".to_string()),
        resolution: Some(Resolution::High),
    };

    let json = serde_json::to_string(&original).unwrap();
    let restored: Content = serde_json::from_str(&json).unwrap();

    match restored {
        Content::Video {
            data,
            uri,
            mime_type,
            resolution,
        } => {
            assert_eq!(data, None);
            assert_eq!(uri, Some("gs://bucket/video.mp4".to_string()));
            assert_eq!(mime_type, Some("video/mp4".to_string()));
            assert_eq!(resolution, Some(Resolution::High));
        }
        _ => panic!("Expected Video variant"),
    }
}

// --- Resolution Unknown Tests ---

#[test]
fn test_resolution_unknown_deserialization() {
    // Test that unrecognized resolution strings deserialize to Unknown
    let json = r#""super_high""#;
    let resolution: Resolution = serde_json::from_str(json).unwrap();

    assert!(resolution.is_unknown());
    assert_eq!(resolution.unknown_resolution_type(), Some("super_high"));
}

#[test]
fn test_resolution_unknown_roundtrip() {
    // Test that Unknown variant roundtrips correctly
    let unknown = Resolution::Unknown {
        resolution_type: "extreme".to_string(),
        data: serde_json::Value::String("extreme".to_string()),
    };

    let json = serde_json::to_string(&unknown).expect("Serialization failed");
    assert_eq!(json, "\"extreme\"");

    let deserialized: Resolution = serde_json::from_str(&json).unwrap();
    assert!(deserialized.is_unknown());
    assert_eq!(deserialized.unknown_resolution_type(), Some("extreme"));
}

#[test]
fn test_resolution_unknown_helper_methods() {
    let known = Resolution::High;
    assert!(!known.is_unknown());
    assert_eq!(known.unknown_resolution_type(), None);
    assert!(known.unknown_data().is_none());

    let unknown = Resolution::Unknown {
        resolution_type: "future_res".to_string(),
        data: serde_json::json!({"level": "future_res", "extra": true}),
    };
    assert!(unknown.is_unknown());
    assert_eq!(unknown.unknown_resolution_type(), Some("future_res"));
    let data = unknown.unknown_data().unwrap();
    assert_eq!(data.get("extra").unwrap(), true);
}

#[test]
fn test_resolution_unknown_in_image_content() {
    // Test that unknown resolution works within Image content
    let json =
        r#"{"type": "image", "data": "base64", "mime_type": "image/png", "resolution": "auto"}"#;
    let content: Content = serde_json::from_str(json).unwrap();

    match content {
        Content::Image { resolution, .. } => {
            let res = resolution.expect("resolution should be present");
            assert!(res.is_unknown());
            assert_eq!(res.unknown_resolution_type(), Some("auto"));
        }
        _ => panic!("Expected Image variant"),
    }
}

#[test]
fn test_resolution_unknown_object_form() {
    // Test that object-form resolution values are handled (future API compatibility)
    // Non-string values get "<non-string: ...>" as the resolution_type
    let json = r#"{"level": "ultra_ultra_high", "tokens": 5000}"#;
    let resolution: Resolution = serde_json::from_str(json).expect("Should deserialize");

    assert!(resolution.is_unknown());
    // Object form gets formatted as "<non-string: ...>" in resolution_type
    assert!(
        resolution
            .unknown_resolution_type()
            .unwrap()
            .starts_with("<non-string:")
    );

    // Verify the full object is preserved in data
    let data = resolution.unknown_data().unwrap();
    assert_eq!(data.get("level").unwrap(), "ultra_ultra_high");
    assert_eq!(data.get("tokens").unwrap(), 5000);
}

// --- File Search Tests ---

#[test]
fn test_deserialize_file_search_result() {
    // Test the actual API format
    let json = r#"{"type": "file_search_result", "call_id": "call123", "result": [{"title": "Document.pdf", "text": "Relevant content", "file_search_store": "store-1"}]}"#;
    let content: Content = serde_json::from_str(json).expect("Should deserialize");

    match &content {
        Content::FileSearchResult { call_id, result } => {
            assert_eq!(call_id, "call123");
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].title, "Document.pdf");
            assert_eq!(result[0].text, "Relevant content");
            assert_eq!(result[0].store, "store-1");
        }
        _ => panic!("Expected FileSearchResult variant, got {:?}", content),
    }

    assert!(content.is_file_search_result());
    assert!(!content.is_unknown());
}

#[test]
fn test_deserialize_file_search_result_multiple_items() {
    let json = r#"{
        "type": "file_search_result",
        "call_id": "call456",
        "result": [
            {"title": "First.pdf", "text": "First content", "file_search_store": "store-a"},
            {"title": "Second.pdf", "text": "Second content", "file_search_store": "store-b"}
        ]
    }"#;
    let content: Content = serde_json::from_str(json).expect("Should deserialize");

    match &content {
        Content::FileSearchResult { call_id, result } => {
            assert_eq!(call_id, "call456");
            assert_eq!(result.len(), 2);
            assert_eq!(result[0].title, "First.pdf");
            assert_eq!(result[1].title, "Second.pdf");
        }
        _ => panic!("Expected FileSearchResult variant"),
    }
}

#[test]
fn test_serialize_file_search_result() {
    let content = Content::FileSearchResult {
        call_id: "call789".to_string(),
        result: vec![FileSearchResultItem {
            title: "Results.pdf".to_string(),
            text: "Found text".to_string(),
            store: "my-store".to_string(),
        }],
    };

    let json = serde_json::to_string(&content).expect("Serialization should work");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "file_search_result");
    assert_eq!(value["call_id"], "call789");
    assert!(value["result"].is_array());
    assert_eq!(value["result"][0]["title"], "Results.pdf");
    assert_eq!(value["result"][0]["text"], "Found text");
    assert_eq!(value["result"][0]["file_search_store"], "my-store");
}

#[test]
fn test_file_search_result_roundtrip() {
    let original = Content::FileSearchResult {
        call_id: "roundtrip_test".to_string(),
        result: vec![
            FileSearchResultItem {
                title: "Doc1.pdf".to_string(),
                text: "Content one".to_string(),
                store: "store-1".to_string(),
            },
            FileSearchResultItem {
                title: "Doc2.pdf".to_string(),
                text: "Content two".to_string(),
                store: "store-2".to_string(),
            },
        ],
    };

    let json = serde_json::to_string(&original).unwrap();
    let restored: Content = serde_json::from_str(&json).unwrap();

    match restored {
        Content::FileSearchResult { call_id, result } => {
            assert_eq!(call_id, "roundtrip_test");
            assert_eq!(result.len(), 2);
            assert_eq!(result[0].title, "Doc1.pdf");
            assert_eq!(result[0].text, "Content one");
            assert_eq!(result[1].title, "Doc2.pdf");
        }
        _ => panic!("Expected FileSearchResult variant"),
    }
}

#[test]
fn test_file_search_result_empty_results() {
    // Test empty results array
    let content = Content::FileSearchResult {
        call_id: "no_results".to_string(),
        result: vec![],
    };

    let json = serde_json::to_string(&content).unwrap();
    let restored: Content = serde_json::from_str(&json).unwrap();

    match restored {
        Content::FileSearchResult { call_id, result } => {
            assert_eq!(call_id, "no_results");
            assert!(result.is_empty());
        }
        _ => panic!("Expected FileSearchResult variant"),
    }
}

#[test]
fn test_file_search_result_item_default() {
    let item = FileSearchResultItem::default();
    assert!(item.title.is_empty());
    assert!(item.text.is_empty());
    assert!(item.store.is_empty());
}

// =============================================================================
// Content Constructor Tests (new_*() methods)
// =============================================================================

#[test]
fn test_new_text_creates_correct_variant() {
    let content = Content::text("Hello world");
    match &content {
        Content::Text { text, annotations } => {
            assert_eq!(*text, Some("Hello world".to_string()));
            assert!(annotations.is_none());
        }
        _ => panic!("Expected Text variant"),
    }
    assert!(content.is_text());
    assert_eq!(content.as_text(), Some("Hello world"));
}

#[test]
fn test_new_text_with_empty_string() {
    let content = Content::text("");
    match &content {
        Content::Text { text, .. } => {
            assert_eq!(*text, Some(String::new()));
        }
        _ => panic!("Expected Text variant"),
    }
    // Empty string returns None from text() accessor
    assert_eq!(content.as_text(), None);
}

#[test]
fn test_new_thought_creates_correct_variant() {
    // Note: new_thought() now takes a signature value (Thought content contains signature, not text)
    let content = Content::thought("EosFCogFAXLI2...");
    match &content {
        Content::Thought { signature } => {
            assert_eq!(*signature, Some("EosFCogFAXLI2...".to_string()));
        }
        _ => panic!("Expected Thought variant"),
    }
    assert!(content.is_thought());
    assert_eq!(content.thought_signature(), Some("EosFCogFAXLI2..."));
}

#[test]
fn test_new_thought_with_empty_string() {
    let content = Content::thought("");
    match &content {
        Content::Thought { signature } => {
            assert_eq!(*signature, Some(String::new()));
        }
        _ => panic!("Expected Thought variant"),
    }
    // Empty string returns None from thought_signature() accessor
    assert_eq!(content.thought_signature(), None);
}

#[test]
fn test_new_function_call_creates_correct_variant() {
    let content = Content::function_call("get_weather", serde_json::json!({"location": "SF"}));
    match &content {
        Content::FunctionCall { id, name, args, .. } => {
            assert!(id.is_none());
            assert_eq!(name, "get_weather");
            assert_eq!(args["location"], "SF");
        }
        _ => panic!("Expected FunctionCall variant"),
    }
    assert!(content.is_function_call());
}

#[test]
fn test_new_function_call_with_id_creates_correct_variant() {
    let content = Content::function_call_with_id(
        Some("call_123"),
        "get_weather",
        serde_json::json!({"location": "San Francisco"}),
    );
    match content {
        Content::FunctionCall { id, name, args, .. } => {
            assert_eq!(id, Some("call_123".to_string()));
            assert_eq!(name, "get_weather");
            assert_eq!(args["location"], "San Francisco");
        }
        _ => panic!("Expected FunctionCall variant"),
    }
}

#[test]
fn test_new_function_call_with_id_none_id() {
    let content = Content::function_call_with_id(None::<String>, "my_func", serde_json::json!({}));
    match content {
        Content::FunctionCall { id, name, .. } => {
            assert!(id.is_none());
            assert_eq!(name, "my_func");
        }
        _ => panic!("Expected FunctionCall variant"),
    }
}

#[test]
fn test_new_function_result_creates_correct_variant() {
    let content = Content::function_result(
        "get_weather",
        "call_abc123",
        serde_json::json!({"temperature": "72F", "conditions": "sunny"}),
    );
    match content {
        Content::FunctionResult {
            name,
            call_id,
            result,
            ..
        } => {
            assert_eq!(name, Some("get_weather".to_string()));
            assert_eq!(call_id, "call_abc123");
            assert_eq!(result["temperature"], "72F");
            assert_eq!(result["conditions"], "sunny");
        }
        _ => panic!("Expected FunctionResult variant"),
    }
}

#[test]
fn test_new_image_data_creates_correct_variant() {
    let content = Content::image_data("base64encodeddata", "image/png");
    match content {
        Content::Image {
            data,
            uri,
            mime_type,
            resolution,
        } => {
            assert_eq!(data, Some("base64encodeddata".to_string()));
            assert!(uri.is_none());
            assert_eq!(mime_type, Some("image/png".to_string()));
            assert!(resolution.is_none());
        }
        _ => panic!("Expected Image variant"),
    }
}

#[test]
fn test_new_image_data_with_resolution_creates_correct_variant() {
    let content =
        Content::image_data_with_resolution("base64encodeddata", "image/png", Resolution::High);
    match content {
        Content::Image {
            data,
            uri,
            mime_type,
            resolution,
        } => {
            assert_eq!(data, Some("base64encodeddata".to_string()));
            assert!(uri.is_none());
            assert_eq!(mime_type, Some("image/png".to_string()));
            assert_eq!(resolution, Some(Resolution::High));
        }
        _ => panic!("Expected Image variant"),
    }
}

#[test]
fn test_new_image_uri_creates_correct_variant() {
    let content = Content::image_uri("https://example.com/image.png", "image/png");
    match content {
        Content::Image {
            data,
            uri,
            mime_type,
            resolution,
        } => {
            assert!(data.is_none());
            assert_eq!(uri, Some("https://example.com/image.png".to_string()));
            assert_eq!(mime_type, Some("image/png".to_string()));
            assert!(resolution.is_none());
        }
        _ => panic!("Expected Image variant"),
    }
}

#[test]
fn test_new_image_uri_with_resolution_creates_correct_variant() {
    let content = Content::image_uri_with_resolution(
        "https://example.com/image.png",
        "image/png",
        Resolution::Low,
    );
    match content {
        Content::Image {
            data,
            uri,
            resolution,
            ..
        } => {
            assert!(data.is_none());
            assert_eq!(uri, Some("https://example.com/image.png".to_string()));
            assert_eq!(resolution, Some(Resolution::Low));
        }
        _ => panic!("Expected Image variant"),
    }
}

#[test]
fn test_new_audio_data_creates_correct_variant() {
    let content = Content::audio_data("base64audiodata", "audio/mp3");
    match content {
        Content::Audio {
            data,
            uri,
            mime_type,
        } => {
            assert_eq!(data, Some("base64audiodata".to_string()));
            assert!(uri.is_none());
            assert_eq!(mime_type, Some("audio/mp3".to_string()));
        }
        _ => panic!("Expected Audio variant"),
    }
}

#[test]
fn test_new_audio_uri_creates_correct_variant() {
    let content = Content::audio_uri("https://example.com/audio.mp3", "audio/mp3");
    match content {
        Content::Audio {
            data,
            uri,
            mime_type,
        } => {
            assert!(data.is_none());
            assert_eq!(uri, Some("https://example.com/audio.mp3".to_string()));
            assert_eq!(mime_type, Some("audio/mp3".to_string()));
        }
        _ => panic!("Expected Audio variant"),
    }
}

#[test]
fn test_new_video_data_creates_correct_variant() {
    let content = Content::video_data("base64videodata", "video/mp4");
    match content {
        Content::Video {
            data,
            uri,
            mime_type,
            resolution,
        } => {
            assert_eq!(data, Some("base64videodata".to_string()));
            assert!(uri.is_none());
            assert_eq!(mime_type, Some("video/mp4".to_string()));
            assert!(resolution.is_none());
        }
        _ => panic!("Expected Video variant"),
    }
}

#[test]
fn test_new_video_data_with_resolution_creates_correct_variant() {
    let content =
        Content::video_data_with_resolution("base64videodata", "video/mp4", Resolution::Low);
    match content {
        Content::Video {
            data, resolution, ..
        } => {
            assert_eq!(data, Some("base64videodata".to_string()));
            assert_eq!(resolution, Some(Resolution::Low));
        }
        _ => panic!("Expected Video variant"),
    }
}

#[test]
fn test_new_video_uri_creates_correct_variant() {
    let content = Content::video_uri("https://example.com/video.mp4", "video/mp4");
    match content {
        Content::Video {
            data,
            uri,
            mime_type,
            resolution,
        } => {
            assert!(data.is_none());
            assert_eq!(uri, Some("https://example.com/video.mp4".to_string()));
            assert_eq!(mime_type, Some("video/mp4".to_string()));
            assert!(resolution.is_none());
        }
        _ => panic!("Expected Video variant"),
    }
}

#[test]
fn test_new_video_uri_with_resolution_creates_correct_variant() {
    let content = Content::video_uri_with_resolution(
        "https://example.com/video.mp4",
        "video/mp4",
        Resolution::Medium,
    );
    match content {
        Content::Video {
            uri, resolution, ..
        } => {
            assert_eq!(uri, Some("https://example.com/video.mp4".to_string()));
            assert_eq!(resolution, Some(Resolution::Medium));
        }
        _ => panic!("Expected Video variant"),
    }
}

#[test]
fn test_new_document_data_creates_correct_variant() {
    let content = Content::document_data("base64pdfdata", "application/pdf");
    match content {
        Content::Document {
            data,
            uri,
            mime_type,
        } => {
            assert_eq!(data, Some("base64pdfdata".to_string()));
            assert!(uri.is_none());
            assert_eq!(mime_type, Some("application/pdf".to_string()));
        }
        _ => panic!("Expected Document variant"),
    }
}

#[test]
fn test_new_document_uri_creates_correct_variant() {
    let content = Content::document_uri("https://example.com/doc.pdf", "application/pdf");
    match content {
        Content::Document {
            data,
            uri,
            mime_type,
        } => {
            assert!(data.is_none());
            assert_eq!(uri, Some("https://example.com/doc.pdf".to_string()));
            assert_eq!(mime_type, Some("application/pdf".to_string()));
        }
        _ => panic!("Expected Document variant"),
    }
}

#[test]
fn test_from_uri_and_mime_infers_image() {
    let content = Content::from_uri_and_mime("files/abc123", "image/png");
    match content {
        Content::Image { uri, mime_type, .. } => {
            assert_eq!(uri, Some("files/abc123".to_string()));
            assert_eq!(mime_type, Some("image/png".to_string()));
        }
        _ => panic!("Expected Image variant for image/* MIME type"),
    }
}

#[test]
fn test_from_uri_and_mime_infers_audio() {
    let content = Content::from_uri_and_mime("files/abc123", "audio/wav");
    match content {
        Content::Audio { uri, mime_type, .. } => {
            assert_eq!(uri, Some("files/abc123".to_string()));
            assert_eq!(mime_type, Some("audio/wav".to_string()));
        }
        _ => panic!("Expected Audio variant for audio/* MIME type"),
    }
}

#[test]
fn test_from_uri_and_mime_infers_video() {
    let content = Content::from_uri_and_mime("files/abc123", "video/webm");
    match content {
        Content::Video { uri, mime_type, .. } => {
            assert_eq!(uri, Some("files/abc123".to_string()));
            assert_eq!(mime_type, Some("video/webm".to_string()));
        }
        _ => panic!("Expected Video variant for video/* MIME type"),
    }
}

#[test]
fn test_from_uri_and_mime_infers_document_for_pdf() {
    let content = Content::from_uri_and_mime("files/abc123", "application/pdf");
    match content {
        Content::Document { uri, mime_type, .. } => {
            assert_eq!(uri, Some("files/abc123".to_string()));
            assert_eq!(mime_type, Some("application/pdf".to_string()));
        }
        _ => panic!("Expected Document variant for application/pdf MIME type"),
    }
}

#[test]
fn test_from_uri_and_mime_infers_document_for_text() {
    let content = Content::from_uri_and_mime("files/abc123", "text/plain");
    match content {
        Content::Document { uri, mime_type, .. } => {
            assert_eq!(uri, Some("files/abc123".to_string()));
            assert_eq!(mime_type, Some("text/plain".to_string()));
        }
        _ => panic!("Expected Document variant for text/* MIME type"),
    }
}

#[test]
fn test_constructors_accept_string_types() {
    // Test that constructors work with various string types via Into<String>
    let text1 = Content::text(String::from("owned string"));
    assert_eq!(text1.as_text(), Some("owned string"));

    let text2 = Content::text("&str literal");
    assert_eq!(text2.as_text(), Some("&str literal"));

    // Cow, Box<str>, etc. would also work via Into<String>
}

#[test]
fn test_constructor_serialization_roundtrip() {
    // Test that content created via constructors serializes correctly
    let content = Content::text("Test message");
    let json = serde_json::to_string(&content).expect("Should serialize");
    let deserialized: Content = serde_json::from_str(&json).expect("Should deserialize");
    assert_eq!(deserialized.as_text(), Some("Test message"));

    let image = Content::image_data_with_resolution("data", "image/png", Resolution::High);
    let json = serde_json::to_string(&image).expect("Should serialize");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["type"], "image");
    assert_eq!(value["data"], "data");
    assert_eq!(value["resolution"], "high");
}

// =============================================================================
// Annotation Constructor Tests
// =============================================================================

#[test]
fn test_annotation_new_constructor() {
    let annotation = Annotation::new(0, 10, Some("https://example.com".to_string()));
    assert_eq!(annotation.start_index, 0);
    assert_eq!(annotation.end_index, 10);
    assert!(annotation.has_source());
    assert_eq!(annotation.source, Some("https://example.com".to_string()));
}

#[test]
fn test_annotation_new_without_source() {
    let annotation = Annotation::new(5, 15, None);
    assert_eq!(annotation.start_index, 5);
    assert_eq!(annotation.end_index, 15);
    assert!(!annotation.has_source());
    assert!(annotation.source.is_none());
}

// =============================================================================
// GoogleSearchResultItem / FileSearchResultItem Constructor Tests
// =============================================================================

#[test]
fn test_google_search_result_item_new() {
    let item = GoogleSearchResultItem::new("Rust Lang", "https://rust-lang.org");
    assert_eq!(item.title, "Rust Lang");
    assert_eq!(item.url, "https://rust-lang.org");
    assert!(!item.has_rendered_content());
    assert!(item.rendered_content.is_none());
}

#[test]
fn test_file_search_result_item_new() {
    let item = FileSearchResultItem::new("Document Title", "Extracted text content", "my-store");
    assert_eq!(item.title, "Document Title");
    assert_eq!(item.text, "Extracted text content");
    assert_eq!(item.store, "my-store");
    assert!(item.has_text());
}

#[test]
fn test_file_search_result_item_has_text_empty() {
    let item = FileSearchResultItem::new("Title", "", "store");
    assert!(!item.has_text());
}

// =============================================================================
// Builder Method Tests (with_resolution)
// =============================================================================

#[test]
fn test_with_resolution_on_image() {
    let content = Content::image_uri("files/abc123", "image/png").with_resolution(Resolution::High);
    match content {
        Content::Image { resolution, .. } => {
            assert_eq!(resolution, Some(Resolution::High));
        }
        _ => panic!("Expected Image variant"),
    }
}

#[test]
fn test_with_resolution_on_video() {
    let content = Content::video_uri("files/def456", "video/mp4").with_resolution(Resolution::Low);
    match content {
        Content::Video { resolution, .. } => {
            assert_eq!(resolution, Some(Resolution::Low));
        }
        _ => panic!("Expected Video variant"),
    }
}

#[test]
fn test_with_resolution_preserves_other_fields() {
    let content =
        Content::image_data("base64data", "image/jpeg").with_resolution(Resolution::UltraHigh);
    match content {
        Content::Image {
            data,
            uri,
            mime_type,
            resolution,
        } => {
            assert_eq!(data, Some("base64data".to_string()));
            assert!(uri.is_none());
            assert_eq!(mime_type, Some("image/jpeg".to_string()));
            assert_eq!(resolution, Some(Resolution::UltraHigh));
        }
        _ => panic!("Expected Image variant"),
    }
}

#[test]
fn test_with_resolution_on_non_media_returns_unchanged() {
    // with_resolution on non-media content should return unchanged
    let original = Content::text("Hello");
    let after = original.clone().with_resolution(Resolution::High);
    assert_eq!(original, after);
}

#[test]
fn test_with_resolution_on_audio_returns_unchanged() {
    // Audio content doesn't support resolution (unlike Image/Video)
    let original = Content::audio_uri("files/abc123", "audio/mp3");
    let after = original.clone().with_resolution(Resolution::High);
    assert_eq!(original, after);
}

#[test]
fn test_with_resolution_overwrites_existing() {
    let content = Content::image_uri("files/abc123", "image/png")
        .with_resolution(Resolution::Low)
        .with_resolution(Resolution::High);
    match content {
        Content::Image { resolution, .. } => {
            assert_eq!(resolution, Some(Resolution::High));
        }
        _ => panic!("Expected Image variant"),
    }
}

#[test]
fn test_with_resolution_unknown_variant() {
    // Unknown resolution variants can be used in builder and roundtrip
    let unknown_res = Resolution::Unknown {
        resolution_type: "ULTRA_MEGA_HD".to_string(),
        data: serde_json::json!("ULTRA_MEGA_HD"),
    };
    let content = Content::image_uri("files/abc123", "image/png").with_resolution(unknown_res);

    match content {
        Content::Image { resolution, .. } => {
            let res = resolution.expect("Should have resolution");
            assert!(res.is_unknown());
            assert_eq!(res.unknown_resolution_type(), Some("ULTRA_MEGA_HD"));
        }
        _ => panic!("Expected Image variant"),
    }
}

// =============================================================================
// with_result() and with_result_error() Tests
// =============================================================================

#[test]
fn test_with_result_creates_function_result() {
    let call = Content::function_call_with_id(
        Some("call_123"),
        "get_weather",
        serde_json::json!({"location": "SF"}),
    );
    let result = call.with_result(serde_json::json!({"temp": 72}));

    match result {
        Content::FunctionResult {
            name,
            call_id,
            result,
            is_error,
        } => {
            assert_eq!(name, Some("get_weather".to_string()));
            assert_eq!(call_id, "call_123");
            assert_eq!(result["temp"], 72);
            assert!(is_error.is_none());
        }
        _ => panic!("Expected FunctionResult variant"),
    }
}

#[test]
fn test_with_result_error_sets_is_error() {
    let call =
        Content::function_call_with_id(Some("call_456"), "api_request", serde_json::json!({}));
    let result = call.with_result_error(serde_json::json!({"error": "timeout"}));

    match result {
        Content::FunctionResult {
            name,
            call_id,
            is_error,
            ..
        } => {
            assert_eq!(name, Some("api_request".to_string()));
            assert_eq!(call_id, "call_456");
            assert_eq!(is_error, Some(true));
        }
        _ => panic!("Expected FunctionResult variant"),
    }
}

#[test]
fn test_with_result_on_non_function_call_returns_unchanged() {
    let text = Content::text("Hello");
    let after = text.clone().with_result(serde_json::json!({"data": 1}));
    assert_eq!(text, after);
}

#[test]
fn test_with_result_uses_empty_call_id_when_none() {
    // Function call without ID should use empty string for call_id
    let call = Content::function_call("get_data", serde_json::json!({}));
    let result = call.with_result(serde_json::json!({"value": 42}));

    match result {
        Content::FunctionResult { call_id, .. } => {
            assert_eq!(call_id, "");
        }
        _ => panic!("Expected FunctionResult variant"),
    }
}

#[test]
fn test_function_result_error_constructor() {
    let content = Content::function_result_error(
        "api_call",
        "call_123",
        serde_json::json!({"error": "timeout", "code": 504}),
    );

    match content {
        Content::FunctionResult {
            name,
            call_id,
            result,
            is_error,
        } => {
            assert_eq!(name, Some("api_call".to_string()));
            assert_eq!(call_id, "call_123");
            assert_eq!(result, serde_json::json!({"error": "timeout", "code": 504}));
            assert_eq!(is_error, Some(true));
        }
        _ => panic!("Expected FunctionResult variant"),
    }
}

// =============================================================================
// from_uri_and_mime MIME Type Routing Tests
// =============================================================================

#[test]
fn test_from_uri_and_mime_image_types() {
    for mime in ["image/png", "image/jpeg", "image/gif", "image/webp"] {
        let content = Content::from_uri_and_mime("files/abc123", mime);
        assert!(
            matches!(content, Content::Image { .. }),
            "Expected Image for {mime}"
        );
    }
}

#[test]
fn test_from_uri_and_mime_audio_types() {
    for mime in ["audio/mp3", "audio/wav", "audio/ogg", "audio/mpeg"] {
        let content = Content::from_uri_and_mime("files/abc123", mime);
        assert!(
            matches!(content, Content::Audio { .. }),
            "Expected Audio for {mime}"
        );
    }
}

#[test]
fn test_from_uri_and_mime_video_types() {
    for mime in ["video/mp4", "video/webm", "video/quicktime"] {
        let content = Content::from_uri_and_mime("files/abc123", mime);
        assert!(
            matches!(content, Content::Video { .. }),
            "Expected Video for {mime}"
        );
    }
}

#[test]
fn test_from_uri_and_mime_document_fallback() {
    // PDFs, text files, and unknown types all become Document
    for mime in [
        "application/pdf",
        "text/plain",
        "text/csv",
        "application/json",
        "application/octet-stream",
    ] {
        let content = Content::from_uri_and_mime("files/abc123", mime);
        assert!(
            matches!(content, Content::Document { .. }),
            "Expected Document for {mime}"
        );
    }
}

#[test]
fn test_from_uri_and_mime_preserves_values() {
    let content = Content::from_uri_and_mime("files/test123", "image/png");
    match content {
        Content::Image { uri, mime_type, .. } => {
            assert_eq!(uri, Some("files/test123".to_string()));
            assert_eq!(mime_type, Some("image/png".to_string()));
        }
        _ => panic!("Expected Image variant"),
    }
}

// =============================================================================
// PartialArg Tests
// =============================================================================

#[test]
fn test_partial_arg_string_value() {
    let arg = PartialArg {
        json_path: "$.location".to_string(),
        string_value: Some("San Francisco".to_string()),
        number_value: None,
        bool_value: None,
        null_value: None,
        will_continue: Some(true),
    };

    assert!(arg.is_string());
    assert!(!arg.is_number());
    assert!(!arg.is_bool());
    assert!(!arg.is_null());
    assert!(arg.will_continue());
    assert_eq!(
        arg.value(),
        serde_json::Value::String("San Francisco".to_string())
    );
}

#[test]
fn test_partial_arg_number_value() {
    let arg = PartialArg {
        json_path: "$.temperature".to_string(),
        string_value: None,
        number_value: Some(72.5),
        bool_value: None,
        null_value: None,
        will_continue: None,
    };

    assert!(arg.is_number());
    assert!(!arg.is_string());
    assert!(!arg.will_continue());
    assert_eq!(arg.value(), serde_json::json!(72.5));
}

#[test]
fn test_partial_arg_bool_value() {
    let arg = PartialArg {
        json_path: "$.verbose".to_string(),
        string_value: None,
        number_value: None,
        bool_value: Some(true),
        null_value: None,
        will_continue: None,
    };

    assert!(arg.is_bool());
    assert_eq!(arg.value(), serde_json::Value::Bool(true));
}

#[test]
fn test_partial_arg_null_value() {
    let arg = PartialArg {
        json_path: "$.optional_field".to_string(),
        string_value: None,
        number_value: None,
        bool_value: None,
        null_value: Some("NULL_VALUE".to_string()),
        will_continue: None,
    };

    assert!(arg.is_null());
    assert_eq!(arg.value(), serde_json::Value::Null);
}

#[test]
fn test_partial_arg_serialization_roundtrip() {
    let arg = PartialArg {
        json_path: "$.location".to_string(),
        string_value: Some("Tokyo".to_string()),
        number_value: None,
        bool_value: None,
        null_value: None,
        will_continue: Some(true),
    };

    let json = serde_json::to_string(&arg).expect("Serialization should succeed");
    assert!(json.contains("json_path"));
    assert!(json.contains("$.location"));
    assert!(json.contains("string_value"));
    assert!(json.contains("Tokyo"));
    assert!(json.contains("will_continue"));

    // None fields should be skipped
    assert!(!json.contains("number_value"));
    assert!(!json.contains("bool_value"));
    assert!(!json.contains("null_value"));

    let deserialized: PartialArg =
        serde_json::from_str(&json).expect("Deserialization should succeed");
    assert_eq!(arg, deserialized);
}

#[test]
fn test_partial_arg_deserialization_from_wire() {
    // Simulate the wire format from the API
    let wire_json = r#"{
        "json_path": "$.city",
        "string_value": "San Fran",
        "will_continue": true
    }"#;

    let arg: PartialArg = serde_json::from_str(wire_json).expect("Should deserialize");
    assert_eq!(arg.json_path, "$.city");
    assert_eq!(arg.string_value, Some("San Fran".to_string()));
    assert!(arg.will_continue());
}

// =============================================================================
// Content::FunctionCall with partial_args Tests
// =============================================================================

#[test]
fn test_function_call_with_partial_args_serialization() {
    let content = Content::FunctionCall {
        id: Some("call_123".to_string()),
        name: "get_weather".to_string(),
        args: serde_json::Value::Null,
        partial_args: Some(vec![PartialArg {
            json_path: "$.location".to_string(),
            string_value: Some("Tokyo".to_string()),
            number_value: None,
            bool_value: None,
            null_value: None,
            will_continue: Some(false),
        }]),
        will_continue: Some(true),
    };

    let json = serde_json::to_string(&content).expect("Serialization should succeed");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "function_call");
    assert_eq!(value["id"], "call_123");
    assert_eq!(value["name"], "get_weather");
    assert!(value["partial_args"].is_array());
    assert_eq!(value["partial_args"][0]["json_path"], "$.location");
    assert_eq!(value["partial_args"][0]["string_value"], "Tokyo");
    assert_eq!(value["will_continue"], true);
}

#[test]
fn test_function_call_without_partial_args_no_extra_fields() {
    // Normal function calls should not include partial_args or will_continue
    let content = Content::function_call("test_fn", serde_json::json!({"key": "value"}));

    let json = serde_json::to_string(&content).expect("Serialization should succeed");
    assert!(!json.contains("partial_args"));
    assert!(!json.contains("will_continue"));
}

#[test]
fn test_function_call_with_partial_args_deserialization() {
    let wire_json = r#"{
        "type": "function_call",
        "id": "call_abc",
        "name": "search",
        "arguments": {},
        "partial_args": [
            {
                "json_path": "$.query",
                "string_value": "weather in ",
                "will_continue": true
            },
            {
                "json_path": "$.limit",
                "number_value": 10
            }
        ],
        "will_continue": true
    }"#;

    let content: Content = serde_json::from_str(wire_json).expect("Should deserialize");
    match content {
        Content::FunctionCall {
            id,
            name,
            partial_args,
            will_continue,
            ..
        } => {
            assert_eq!(id, Some("call_abc".to_string()));
            assert_eq!(name, "search");
            let pa = partial_args.expect("Should have partial_args");
            assert_eq!(pa.len(), 2);
            assert_eq!(pa[0].json_path, "$.query");
            assert_eq!(pa[0].string_value, Some("weather in ".to_string()));
            assert!(pa[0].will_continue());
            assert_eq!(pa[1].json_path, "$.limit");
            assert_eq!(pa[1].number_value, Some(10.0));
            assert!(!pa[1].will_continue());
            assert_eq!(will_continue, Some(true));
        }
        _ => panic!("Expected FunctionCall variant"),
    }
}

#[test]
fn test_function_call_streaming_first_chunk() {
    // First chunk: has function name and will_continue, but no partial_args
    let wire_json = r#"{
        "type": "function_call",
        "name": "get_weather",
        "arguments": {},
        "will_continue": true
    }"#;

    let content: Content = serde_json::from_str(wire_json).expect("Should deserialize");
    match content {
        Content::FunctionCall {
            name,
            partial_args,
            will_continue,
            ..
        } => {
            assert_eq!(name, "get_weather");
            assert!(partial_args.is_none());
            assert_eq!(will_continue, Some(true));
        }
        _ => panic!("Expected FunctionCall variant"),
    }
}

#[test]
fn test_function_call_streaming_final_chunk() {
    // Final chunk: empty function call, will_continue is false/absent
    let wire_json = r#"{
        "type": "function_call",
        "arguments": {}
    }"#;

    let content: Content = serde_json::from_str(wire_json).expect("Should deserialize");
    match content {
        Content::FunctionCall {
            name,
            partial_args,
            will_continue,
            ..
        } => {
            assert!(name.is_empty()); // Default empty string
            assert!(partial_args.is_none());
            assert!(will_continue.is_none());
        }
        _ => panic!("Expected FunctionCall variant"),
    }
}
