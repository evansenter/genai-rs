//! Unit tests for `StepSummary` and `InteractionResponse::step_summary`.

use super::*;
use crate::{CodeExecutionLanguage, InteractionStatus};

#[test]
fn test_step_summary_counts() {
    let response = InteractionResponse {
        status: InteractionStatus::Completed,
        steps: vec![
            Step::user_text("hi"),
            Step::thought("sig"),
            Step::model_output(vec![Content::text("a"), Content::text("b")]),
            Step::Unknown {
                step_type: "future".into(),
                data: serde_json::Value::Null,
            },
        ],
        ..Default::default()
    };
    let summary = response.step_summary();
    assert_eq!(summary.user_input_count, 1);
    assert_eq!(summary.model_output_count, 1);
    assert_eq!(summary.text_count, 2);
    assert_eq!(summary.thought_count, 1);
    assert_eq!(summary.unknown_count, 1);
    assert_eq!(summary.unknown_types, vec!["future".to_string()]);
    let display = summary.to_string();
    assert!(display.contains("2 text"));
    assert!(display.contains("1 unknown"));
}

// --- StepSummary Tests ---

#[test]
fn test_step_summary() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::model_output(vec![Content::text("Text 1"), Content::text("Text 2")]),
            Step::thought("sig_thinking"),
            Step::function_call("call_1", "test_fn", serde_json::json!({})),
            Step::Unknown {
                step_type: "type_a".to_string(),
                data: serde_json::json!({"type": "type_a"}),
            },
            Step::Unknown {
                step_type: "type_b".to_string(),
                data: serde_json::json!({"type": "type_b"}),
            },
            Step::Unknown {
                step_type: "type_a".to_string(), // Duplicate type
                data: serde_json::json!({"type": "type_a", "extra": true}),
            },
        ],
        ..Default::default()
    };

    let summary = response.step_summary();

    assert_eq!(summary.model_output_count, 1);
    assert_eq!(summary.text_count, 2);
    assert_eq!(summary.thought_count, 1);
    assert_eq!(summary.function_call_count, 1);
    assert_eq!(summary.unknown_count, 3);
    // Unknown types should be deduplicated and sorted
    assert_eq!(summary.unknown_types.len(), 2);
    assert_eq!(summary.unknown_types, vec!["type_a", "type_b"]);
}

#[test]
fn test_step_summary_empty() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![],
        ..Default::default()
    };

    let summary = response.step_summary();

    assert_eq!(summary.text_count, 0);
    assert_eq!(summary.unknown_count, 0);
    assert!(summary.unknown_types.is_empty());
}

#[test]
fn test_step_summary_display() {
    // Test Display for StepSummary with various counts
    let summary = StepSummary {
        text_count: 2,
        thought_count: 1,
        code_execution_call_count: 1,
        code_execution_result_count: 1,
        ..Default::default()
    };
    let display = format!("{summary}");
    assert!(display.contains("2 text"));
    assert!(display.contains("1 thought"));
    assert!(display.contains("1 code_execution_call"));
    assert!(display.contains("1 code_execution_result"));
    // Should not contain zero-count items
    assert!(!display.contains("image"));
    assert!(!display.contains("audio"));
}

#[test]
fn test_step_summary_display_empty() {
    let summary = StepSummary::default();
    assert_eq!(format!("{summary}"), "empty");
}

#[test]
fn test_step_summary_display_with_unknown() {
    let summary = StepSummary {
        unknown_count: 2,
        unknown_types: vec!["new_type_a".to_string(), "new_type_b".to_string()],
        ..Default::default()
    };
    let display = format!("{summary}");
    assert!(display.contains("2 unknown"));
    assert!(display.contains("new_type_a"));
    assert!(display.contains("new_type_b"));
}

#[test]
fn test_step_summary_with_built_in_tools() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::CodeExecutionCall {
                id: "call_1".to_string(),
                language: CodeExecutionLanguage::Python,
                code: "print(1)".to_string(),
                signature: None,
                arguments_extra: Default::default(),
                extra: Default::default(),
            },
            Step::CodeExecutionCall {
                id: "call_2".to_string(),
                language: CodeExecutionLanguage::Python,
                code: "print(2)".to_string(),
                signature: None,
                arguments_extra: Default::default(),
                extra: Default::default(),
            },
            Step::CodeExecutionResult {
                call_id: "call_1".to_string(),
                is_error: false,
                result: "1\n2\n".into(),
                signature: None,
                extra: Default::default(),
            },
            Step::GoogleSearchCall {
                id: "search1".to_string(),
                queries: vec!["test".to_string()],
                search_type: None,
                signature: None,
                arguments_extra: Default::default(),
                extra: Default::default(),
            },
            Step::GoogleSearchResult {
                call_id: "search1".to_string(),
                result: vec![],
                is_error: None,
                signature: None,
                extra: Default::default(),
            },
            Step::UrlContextCall {
                id: "ctx_123".to_string(),
                urls: vec!["https://example.com".to_string()],
                signature: None,
                arguments_extra: Default::default(),
                extra: Default::default(),
            },
            Step::UrlContextResult {
                call_id: "ctx_123".to_string(),
                result: vec![],
                is_error: None,
                signature: None,
                extra: Default::default(),
            },
        ],
        ..Default::default()
    };

    let summary = response.step_summary();

    assert_eq!(summary.code_execution_call_count, 2);
    assert_eq!(summary.code_execution_result_count, 1);
    assert_eq!(summary.google_search_call_count, 1);
    assert_eq!(summary.google_search_result_count, 1);
    assert_eq!(summary.url_context_call_count, 1);
    assert_eq!(summary.url_context_result_count, 1);
    assert_eq!(summary.unknown_count, 0);
}
