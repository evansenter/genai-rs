//! Unknown variant preservation tests
//!
//! Tests that all types with Unknown variants properly:
//! 1. Deserialize unrecognized values into Unknown variants
//! 2. Preserve the original type string and data
//! 3. Roundtrip correctly (serialize back to original form)
//!
//! These tests are SKIPPED when `strict-unknown` is enabled because
//! strict mode causes deserialization errors instead of Unknown variants.
//!
//! Run tests:
//! - Default: `cargo test --test unknown_variant_tests`
//! - Strict (should skip): `cargo test --test unknown_variant_tests --features strict-unknown`

#![cfg(not(feature = "strict-unknown"))]

use genai_rs::{
    Annotation, Content, FunctionCallingMode, InteractionStatus, Resolution, Role, ServiceTier,
    Step, StepDelta, StreamChunk, ThinkingLevel, ThinkingSummaries, ToolChoice,
};
use serde_json::json;

// =============================================================================
// Resolution Unknown Variant Tests
// =============================================================================

mod resolution {
    use super::*;

    #[test]
    fn unknown_resolution_deserializes() {
        let json = json!("super_ultra_high");
        let value: Resolution = serde_json::from_value(json).unwrap();
        assert!(value.is_unknown());
        assert_eq!(value.unknown_resolution_type(), Some("super_ultra_high"));
    }

    #[test]
    fn unknown_resolution_roundtrips() {
        let json = json!("future_resolution");
        let value: Resolution = serde_json::from_value(json.clone()).unwrap();
        let back = serde_json::to_value(&value).unwrap();
        assert_eq!(back, "future_resolution");
    }
}

// =============================================================================
// Content Unknown Variant Tests
// =============================================================================

mod interaction_content {
    use super::*;

    #[test]
    fn unknown_content_type_deserializes() {
        let json = json!({
            "type": "future_content_type",
            "some_field": "value",
            "nested": {"a": 1}
        });
        let content: Content = serde_json::from_value(json).unwrap();
        assert!(content.is_unknown());
        assert_eq!(content.unknown_content_type(), Some("future_content_type"));
    }

    #[test]
    fn unknown_content_preserves_data() {
        let json = json!({
            "type": "new_feature",
            "field1": "value1",
            "field2": 42,
            "nested": {"key": "value"}
        });
        let content: Content = serde_json::from_value(json).unwrap();

        let data = content.unknown_data().unwrap();
        assert_eq!(data["field1"], "value1");
        assert_eq!(data["field2"], 42);
        assert_eq!(data["nested"]["key"], "value");
    }

    #[test]
    fn unknown_content_roundtrips() {
        let json = json!({
            "type": "experimental_type",
            "payload": {"data": "test"}
        });
        let content: Content = serde_json::from_value(json.clone()).unwrap();
        let back = serde_json::to_value(&content).unwrap();

        assert_eq!(back["type"], "experimental_type");
        assert_eq!(back["payload"]["data"], "test");
    }

    #[test]
    fn missing_type_field_becomes_unknown() {
        let json = json!({"foo": "bar", "baz": 42});
        let content: Content = serde_json::from_value(json).unwrap();
        assert!(content.is_unknown());
    }

    #[test]
    fn removed_tool_content_types_become_unknown() {
        // Tool call/result payloads moved from Content to Step in revision
        // 2026-05-20; when they appear in a Content position they are
        // preserved via the Unknown variant (Evergreen).
        let content: Content = serde_json::from_value(json!({
            "type": "function_call",
            "id": "call_1",
            "name": "get_weather",
            "arguments": {"city": "Paris"}
        }))
        .unwrap();
        assert!(content.is_unknown());
        assert_eq!(content.unknown_content_type(), Some("function_call"));
    }
}

// =============================================================================
// Step Unknown Variant Tests
// =============================================================================

mod step {
    use super::*;

    #[test]
    fn unknown_step_type_deserializes() {
        let json = json!({
            "type": "future_step_type",
            "some_field": "value",
            "nested": {"a": 1}
        });
        let step: Step = serde_json::from_value(json).unwrap();
        assert!(step.is_unknown());
        assert_eq!(step.unknown_step_type(), Some("future_step_type"));
    }

    #[test]
    fn unknown_step_preserves_data() {
        let json = json!({
            "type": "new_tool_step",
            "call_id": "call_42",
            "payload": {"key": "value"},
            "count": 7
        });
        let step: Step = serde_json::from_value(json).unwrap();

        let data = step.unknown_data().unwrap();
        assert_eq!(data["call_id"], "call_42");
        assert_eq!(data["payload"]["key"], "value");
        assert_eq!(data["count"], 7);
    }

    #[test]
    fn unknown_step_roundtrips() {
        let json = json!({
            "type": "experimental_step",
            "payload": {"data": "test"},
            "signature": "sig123"
        });
        let step: Step = serde_json::from_value(json.clone()).unwrap();
        let back = serde_json::to_value(&step).unwrap();

        assert_eq!(back["type"], "experimental_step");
        assert_eq!(back["payload"]["data"], "test");
        assert_eq!(back["signature"], "sig123");
    }

    #[test]
    fn missing_type_field_becomes_unknown() {
        let json = json!({"content": [{"type": "text", "text": "hi"}]});
        let step: Step = serde_json::from_value(json).unwrap();
        assert!(step.is_unknown());
        assert_eq!(step.unknown_step_type(), Some("<missing type>"));
    }

    #[test]
    fn known_step_is_not_unknown() {
        let step: Step = serde_json::from_value(json!({
            "type": "model_output",
            "content": [{"type": "text", "text": "hi"}]
        }))
        .unwrap();
        assert!(!step.is_unknown());
        assert_eq!(step.unknown_step_type(), None);
        assert!(step.unknown_data().is_none());
    }

    #[test]
    fn unknown_step_in_step_list_preserved() {
        let steps: Vec<Step> = serde_json::from_value(json!([
            {"type": "user_input", "content": [{"type": "text", "text": "hi"}]},
            {"type": "hologram_render", "scene": "nebula"},
            {"type": "model_output", "content": [{"type": "text", "text": "hello"}]}
        ]))
        .unwrap();

        assert_eq!(steps.len(), 3);
        assert!(!steps[0].is_unknown());
        assert!(steps[1].is_unknown());
        assert_eq!(steps[1].unknown_step_type(), Some("hologram_render"));
        assert!(!steps[2].is_unknown());
    }
}

// =============================================================================
// StepDelta Unknown Variant Tests
// =============================================================================

mod step_delta {
    use super::*;

    #[test]
    fn unknown_delta_type_deserializes() {
        let json = json!({
            "type": "future_delta",
            "fragment": "abc"
        });
        let delta: StepDelta = serde_json::from_value(json).unwrap();
        assert!(delta.is_unknown());
        assert_eq!(delta.unknown_delta_type(), Some("future_delta"));
    }

    #[test]
    fn unknown_delta_preserves_data() {
        let json = json!({
            "type": "new_delta_kind",
            "field1": "value1",
            "nested": {"a": 1}
        });
        let delta: StepDelta = serde_json::from_value(json).unwrap();

        let data = delta.unknown_data().unwrap();
        assert_eq!(data["field1"], "value1");
        assert_eq!(data["nested"]["a"], 1);
    }

    #[test]
    fn unknown_delta_roundtrips() {
        let json = json!({
            "type": "experimental_delta",
            "payload": {"data": "test"}
        });
        let delta: StepDelta = serde_json::from_value(json.clone()).unwrap();
        let back = serde_json::to_value(&delta).unwrap();

        assert_eq!(back["type"], "experimental_delta");
        assert_eq!(back["payload"]["data"], "test");
    }

    #[test]
    fn known_delta_is_not_unknown() {
        let delta: StepDelta =
            serde_json::from_value(json!({"type": "text", "text": "hi"})).unwrap();
        assert!(!delta.is_unknown());
        assert_eq!(delta.unknown_delta_type(), None);
        assert!(delta.unknown_data().is_none());
        assert_eq!(delta.as_text(), Some("hi"));
    }
}

// =============================================================================
// Annotation Unknown Variant Tests
// =============================================================================

mod annotation {
    use super::*;

    #[test]
    fn unknown_annotation_type_deserializes() {
        let json = json!({
            "type": "future_citation",
            "source_id": "src_1",
            "start_index": 0,
            "end_index": 5
        });
        let annotation: Annotation = serde_json::from_value(json).unwrap();
        assert!(annotation.is_unknown());
        assert_eq!(
            annotation.unknown_annotation_type(),
            Some("future_citation")
        );
    }

    #[test]
    fn unknown_annotation_preserves_data_and_indices() {
        let json = json!({
            "type": "hologram_citation",
            "scene": "nebula",
            "start_index": 3,
            "end_index": 9
        });
        let annotation: Annotation = serde_json::from_value(json).unwrap();

        let data = annotation.unknown_data().unwrap();
        assert_eq!(data["scene"], "nebula");

        // Index accessors fall back to the preserved data for Unknown.
        assert_eq!(annotation.start_index(), Some(3));
        assert_eq!(annotation.end_index(), Some(9));
        assert_eq!(annotation.source(), None);
    }

    #[test]
    fn unknown_annotation_roundtrips() {
        let json = json!({
            "type": "experimental_citation",
            "payload": {"data": "test"},
            "start_index": 1,
            "end_index": 2
        });
        let annotation: Annotation = serde_json::from_value(json.clone()).unwrap();
        let back = serde_json::to_value(&annotation).unwrap();

        assert_eq!(back["type"], "experimental_citation");
        assert_eq!(back["payload"]["data"], "test");
        assert_eq!(back["start_index"], 1);
        assert_eq!(back["end_index"], 2);
    }

    #[test]
    fn known_annotation_is_not_unknown() {
        let annotation = Annotation::url_citation("https://example.com", None, 0, 5);
        assert!(!annotation.is_unknown());
        assert_eq!(annotation.unknown_annotation_type(), None);
        assert!(annotation.unknown_data().is_none());
    }
}

// =============================================================================
// ToolChoice Unknown Variant Tests
// =============================================================================

mod tool_choice {
    use super::*;

    #[test]
    fn unknown_object_shape_deserializes() {
        // An object without the recognized "allowed_tools" key is preserved.
        let json = json!({"future_choice": {"foo": "bar"}});
        let choice: ToolChoice = serde_json::from_value(json).unwrap();
        assert!(choice.is_unknown());
        assert!(choice.unknown_choice_type().is_some());

        let data = choice.unknown_data().unwrap();
        assert_eq!(data["future_choice"]["foo"], "bar");
    }

    #[test]
    fn malformed_allowed_tools_becomes_unknown() {
        // allowed_tools present but with an unparseable payload.
        let json = json!({"allowed_tools": "not_an_object"});
        let choice: ToolChoice = serde_json::from_value(json).unwrap();
        assert!(choice.is_unknown());
        assert_eq!(choice.unknown_choice_type(), Some("allowed_tools"));
    }

    #[test]
    fn unknown_tool_choice_roundtrips() {
        let json = json!({"future_choice": {"tools": ["a", "b"]}});
        let choice: ToolChoice = serde_json::from_value(json.clone()).unwrap();
        let back = serde_json::to_value(&choice).unwrap();
        assert_eq!(back, json);
    }

    #[test]
    fn unknown_mode_string_becomes_unknown_mode() {
        // Unknown strings delegate to FunctionCallingMode's Unknown variant,
        // not ToolChoice::Unknown.
        let choice: ToolChoice = serde_json::from_value(json!("everything")).unwrap();
        assert!(!choice.is_unknown());
        match choice {
            ToolChoice::Mode(mode) => {
                assert!(mode.is_unknown());
                assert_eq!(mode.unknown_mode_type(), Some("everything"));
            }
            other => panic!("Expected Mode variant, got {:?}", other),
        }
    }

    #[test]
    fn known_tool_choice_shapes_are_not_unknown() {
        let mode: ToolChoice = serde_json::from_value(json!("auto")).unwrap();
        assert!(!mode.is_unknown());
        assert_eq!(mode.unknown_choice_type(), None);

        let allowed: ToolChoice = serde_json::from_value(json!({
            "allowed_tools": {"mode": "any", "tools": ["get_weather"]}
        }))
        .unwrap();
        assert!(!allowed.is_unknown());
        assert!(allowed.unknown_data().is_none());
    }
}

// =============================================================================
// ServiceTier Unknown Variant Tests
// =============================================================================

mod service_tier {
    use super::*;

    #[test]
    fn unknown_tier_deserializes() {
        let json = json!("turbo");
        let tier: ServiceTier = serde_json::from_value(json).unwrap();
        assert!(tier.is_unknown());
        assert_eq!(tier.unknown_tier_type(), Some("turbo"));
    }

    #[test]
    fn unknown_tier_roundtrips() {
        let json = json!("hyper_priority");
        let tier: ServiceTier = serde_json::from_value(json.clone()).unwrap();
        let back = serde_json::to_value(&tier).unwrap();
        assert_eq!(back, "hyper_priority");
    }

    #[test]
    fn known_tiers_are_not_unknown() {
        for (wire, expected) in [
            ("flex", ServiceTier::Flex),
            ("standard", ServiceTier::Standard),
            ("priority", ServiceTier::Priority),
        ] {
            let tier: ServiceTier = serde_json::from_value(json!(wire)).unwrap();
            assert!(!tier.is_unknown());
            assert_eq!(tier, expected);
        }
    }
}

// =============================================================================
// Role Unknown Variant Tests
// =============================================================================

mod role {
    use super::*;

    #[test]
    fn unknown_role_deserializes() {
        let json = json!("assistant");
        let value: Role = serde_json::from_value(json).unwrap();
        assert!(value.is_unknown());
        assert_eq!(value.unknown_role_type(), Some("assistant"));
    }

    #[test]
    fn unknown_role_roundtrips() {
        let json = json!("supervisor");
        let value: Role = serde_json::from_value(json.clone()).unwrap();
        let back = serde_json::to_value(&value).unwrap();
        assert_eq!(back, "supervisor");
    }
}

// =============================================================================
// ThinkingLevel Unknown Variant Tests
// =============================================================================

mod thinking_level {
    use super::*;

    #[test]
    fn unknown_level_deserializes() {
        let json = json!("ultra_high");
        let value: ThinkingLevel = serde_json::from_value(json).unwrap();
        assert!(value.is_unknown());
        assert_eq!(value.unknown_level_type(), Some("ultra_high"));
    }

    #[test]
    fn unknown_level_roundtrips() {
        let json = json!("extreme");
        let value: ThinkingLevel = serde_json::from_value(json.clone()).unwrap();
        let back = serde_json::to_value(&value).unwrap();
        assert_eq!(back, "extreme");
    }
}

// =============================================================================
// ThinkingSummaries Unknown Variant Tests
// =============================================================================

mod thinking_summaries {
    use super::*;

    #[test]
    fn unknown_summaries_deserializes() {
        let json = json!("THINKING_SUMMARIES_VERBOSE");
        let value: ThinkingSummaries = serde_json::from_value(json).unwrap();
        assert!(value.is_unknown());
        assert_eq!(
            value.unknown_summaries_type(),
            Some("THINKING_SUMMARIES_VERBOSE")
        );
    }

    #[test]
    fn unknown_summaries_roundtrips() {
        let json = json!("THINKING_SUMMARIES_DETAILED");
        let value: ThinkingSummaries = serde_json::from_value(json.clone()).unwrap();
        let back = serde_json::to_value(&value).unwrap();
        assert_eq!(back, "THINKING_SUMMARIES_DETAILED");
    }
}

// =============================================================================
// FunctionCallingMode Unknown Variant Tests
// =============================================================================

mod function_calling_mode {
    use super::*;

    #[test]
    fn unknown_mode_deserializes() {
        let json = json!("REQUIRED");
        let value: FunctionCallingMode = serde_json::from_value(json).unwrap();
        assert!(value.is_unknown());
        assert_eq!(value.unknown_mode_type(), Some("REQUIRED"));
    }

    #[test]
    fn unknown_mode_roundtrips() {
        let json = json!("FORCED");
        let value: FunctionCallingMode = serde_json::from_value(json.clone()).unwrap();
        let back = serde_json::to_value(&value).unwrap();
        assert_eq!(back, "FORCED");
    }

    #[test]
    fn legacy_uppercase_modes_still_recognized() {
        // Wire format is lowercase in revision 2026-05-20, but legacy
        // uppercase values are still accepted (and are NOT unknown).
        let value: FunctionCallingMode = serde_json::from_value(json!("AUTO")).unwrap();
        assert!(!value.is_unknown());
        assert!(matches!(value, FunctionCallingMode::Auto));
    }
}

// =============================================================================
// InteractionStatus Unknown Variant Tests
// =============================================================================

mod interaction_status {
    use super::*;

    #[test]
    fn unknown_status_deserializes() {
        let json = json!("pending_review");
        let value: InteractionStatus = serde_json::from_value(json).unwrap();
        assert!(value.is_unknown());
        assert_eq!(value.unknown_status_type(), Some("pending_review"));
    }

    #[test]
    fn unknown_status_roundtrips() {
        let json = json!("queued");
        let value: InteractionStatus = serde_json::from_value(json.clone()).unwrap();
        let back = serde_json::to_value(&value).unwrap();
        assert_eq!(back, "queued");
    }

    #[test]
    fn budget_exceeded_is_now_a_known_status() {
        // Added in revision 2026-05-20 — must not fall into Unknown.
        let value: InteractionStatus = serde_json::from_value(json!("budget_exceeded")).unwrap();
        assert!(!value.is_unknown());
        assert_eq!(value, InteractionStatus::BudgetExceeded);

        let back = serde_json::to_value(&value).unwrap();
        assert_eq!(back, "budget_exceeded");
    }
}

// =============================================================================
// StreamChunk Unknown Variant Tests
// =============================================================================

mod stream_chunk {
    use super::*;

    #[test]
    fn unknown_chunk_deserializes() {
        // StreamChunk uses "chunk_type" field, not "event_type"
        let json = json!({
            "chunk_type": "future_chunk",
            "data": {"key": "value"}
        });
        let value: StreamChunk = serde_json::from_value(json).unwrap();
        assert!(value.is_unknown());
        assert_eq!(value.unknown_chunk_type(), Some("future_chunk"));
    }

    #[test]
    fn unknown_chunk_roundtrips() {
        let json = json!({
            "chunk_type": "new_chunk_type",
            "data": {"payload": "test"}
        });
        let value: StreamChunk = serde_json::from_value(json.clone()).unwrap();
        let back = serde_json::to_value(&value).unwrap();
        assert_eq!(back["chunk_type"], "new_chunk_type");
    }
}

// =============================================================================
// Comprehensive: All Unknown Variants Have Helper Methods
// =============================================================================

mod helper_methods {
    use super::*;

    #[test]
    fn all_unknown_variants_have_is_unknown() {
        // Resolution
        let resolution: Resolution = serde_json::from_value(json!("future")).unwrap();
        assert!(resolution.is_unknown());

        // Content
        let content: Content = serde_json::from_value(json!({"type": "future"})).unwrap();
        assert!(content.is_unknown());

        // Step
        let step: Step = serde_json::from_value(json!({"type": "future"})).unwrap();
        assert!(step.is_unknown());

        // StepDelta
        let delta: StepDelta = serde_json::from_value(json!({"type": "future"})).unwrap();
        assert!(delta.is_unknown());

        // Annotation
        let annotation: Annotation = serde_json::from_value(json!({"type": "future"})).unwrap();
        assert!(annotation.is_unknown());

        // ToolChoice
        let choice: ToolChoice = serde_json::from_value(json!({"future": {}})).unwrap();
        assert!(choice.is_unknown());

        // ServiceTier
        let tier: ServiceTier = serde_json::from_value(json!("future")).unwrap();
        assert!(tier.is_unknown());

        // Role
        let role: Role = serde_json::from_value(json!("future")).unwrap();
        assert!(role.is_unknown());

        // ThinkingLevel
        let level: ThinkingLevel = serde_json::from_value(json!("future")).unwrap();
        assert!(level.is_unknown());

        // ThinkingSummaries
        let summaries: ThinkingSummaries = serde_json::from_value(json!("future")).unwrap();
        assert!(summaries.is_unknown());

        // FunctionCallingMode
        let mode: FunctionCallingMode = serde_json::from_value(json!("FUTURE")).unwrap();
        assert!(mode.is_unknown());

        // InteractionStatus
        let status: InteractionStatus = serde_json::from_value(json!("future")).unwrap();
        assert!(status.is_unknown());

        // StreamChunk (uses "chunk_type" field)
        let chunk: StreamChunk = serde_json::from_value(json!({"chunk_type": "future"})).unwrap();
        assert!(chunk.is_unknown());
    }

    #[test]
    fn all_unknown_variants_have_type_getter() {
        // Resolution
        let resolution: Resolution = serde_json::from_value(json!("test_res")).unwrap();
        assert_eq!(resolution.unknown_resolution_type(), Some("test_res"));

        // Content
        let content: Content = serde_json::from_value(json!({"type": "test_content"})).unwrap();
        assert_eq!(content.unknown_content_type(), Some("test_content"));

        // Step
        let step: Step = serde_json::from_value(json!({"type": "test_step"})).unwrap();
        assert_eq!(step.unknown_step_type(), Some("test_step"));

        // StepDelta
        let delta: StepDelta = serde_json::from_value(json!({"type": "test_delta"})).unwrap();
        assert_eq!(delta.unknown_delta_type(), Some("test_delta"));

        // Annotation
        let annotation: Annotation =
            serde_json::from_value(json!({"type": "test_annotation"})).unwrap();
        assert_eq!(
            annotation.unknown_annotation_type(),
            Some("test_annotation")
        );

        // ToolChoice
        let choice: ToolChoice =
            serde_json::from_value(json!({"allowed_tools": "malformed"})).unwrap();
        assert_eq!(choice.unknown_choice_type(), Some("allowed_tools"));

        // ServiceTier
        let tier: ServiceTier = serde_json::from_value(json!("test_tier")).unwrap();
        assert_eq!(tier.unknown_tier_type(), Some("test_tier"));

        // Role
        let role: Role = serde_json::from_value(json!("test_role")).unwrap();
        assert_eq!(role.unknown_role_type(), Some("test_role"));

        // ThinkingLevel
        let level: ThinkingLevel = serde_json::from_value(json!("test_level")).unwrap();
        assert_eq!(level.unknown_level_type(), Some("test_level"));

        // ThinkingSummaries
        let summaries: ThinkingSummaries = serde_json::from_value(json!("test_summaries")).unwrap();
        assert_eq!(summaries.unknown_summaries_type(), Some("test_summaries"));

        // FunctionCallingMode
        let mode: FunctionCallingMode = serde_json::from_value(json!("TEST_MODE")).unwrap();
        assert_eq!(mode.unknown_mode_type(), Some("TEST_MODE"));

        // InteractionStatus
        let status: InteractionStatus = serde_json::from_value(json!("test_status")).unwrap();
        assert_eq!(status.unknown_status_type(), Some("test_status"));

        // StreamChunk (uses "chunk_type" field)
        let chunk: StreamChunk =
            serde_json::from_value(json!({"chunk_type": "test_chunk"})).unwrap();
        assert_eq!(chunk.unknown_chunk_type(), Some("test_chunk"));
    }

    #[test]
    fn all_unknown_variants_have_data_getter() {
        // Resolution
        let resolution: Resolution = serde_json::from_value(json!("test")).unwrap();
        assert!(resolution.unknown_data().is_some());

        // Content
        let content: Content =
            serde_json::from_value(json!({"type": "test", "extra": 42})).unwrap();
        let data = content.unknown_data().unwrap();
        assert_eq!(data["extra"], 42);

        // Step
        let step: Step = serde_json::from_value(json!({"type": "test", "extra": 7})).unwrap();
        let data = step.unknown_data().unwrap();
        assert_eq!(data["extra"], 7);

        // StepDelta
        let delta: StepDelta =
            serde_json::from_value(json!({"type": "test", "extra": "x"})).unwrap();
        let data = delta.unknown_data().unwrap();
        assert_eq!(data["extra"], "x");

        // Annotation
        let annotation: Annotation =
            serde_json::from_value(json!({"type": "test", "extra": true})).unwrap();
        let data = annotation.unknown_data().unwrap();
        assert_eq!(data["extra"], true);

        // ToolChoice
        let choice: ToolChoice = serde_json::from_value(json!({"future": {"a": 1}})).unwrap();
        let data = choice.unknown_data().unwrap();
        assert_eq!(data["future"]["a"], 1);

        // ServiceTier
        let tier: ServiceTier = serde_json::from_value(json!("test")).unwrap();
        assert!(tier.unknown_data().is_some());

        // StreamChunk (uses "chunk_type" field, data in "data" field)
        let chunk: StreamChunk =
            serde_json::from_value(json!({"chunk_type": "test", "data": {"payload": "data"}}))
                .unwrap();
        let data = chunk.unknown_data().unwrap();
        assert!(data.get("payload").is_some() || data.get("chunk_type").is_some());
    }
}

// =============================================================================
// Edge Cases
// =============================================================================

mod edge_cases {
    use super::*;

    #[test]
    fn empty_type_string_becomes_unknown() {
        let json = json!("");
        let role: Role = serde_json::from_value(json).unwrap();
        assert!(role.is_unknown());
        assert_eq!(role.unknown_role_type(), Some(""));
    }

    #[test]
    fn whitespace_type_string_becomes_unknown() {
        let json = json!("   ");
        let level: ThinkingLevel = serde_json::from_value(json).unwrap();
        assert!(level.is_unknown());
    }

    #[test]
    fn special_characters_preserved() {
        let json = json!("type-with-dashes_and_underscores.and.dots");
        let role: Role = serde_json::from_value(json.clone()).unwrap();
        assert!(role.is_unknown());

        let back = serde_json::to_value(&role).unwrap();
        assert_eq!(back, "type-with-dashes_and_underscores.and.dots");
    }

    #[test]
    fn unicode_type_string_preserved() {
        let json = json!("タイプ");
        let role: Role = serde_json::from_value(json.clone()).unwrap();
        assert!(role.is_unknown());

        let back = serde_json::to_value(&role).unwrap();
        assert_eq!(back, "タイプ");
    }
}
