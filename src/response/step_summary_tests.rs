//! Unit tests for `StepSummary` and `InteractionResponse::step_summary`.

use super::*;
use crate::InteractionStatus;

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
