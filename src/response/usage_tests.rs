//! Unit tests for `UsageMetadata`, `ModalityTokens` and `GroundingToolCount`.

use super::*;

// =========================================================================
// ModalityTokens / GroundingToolCount
// =========================================================================

#[test]
fn modality_tokens_new_sets_both_fields() {
    // `ModalityTokens` is `#[non_exhaustive]` with no `Default`, so this
    // constructor is the only non-serde route to a value — which makes
    // its signature, not just its behavior, part of the public API.
    let tokens = ModalityTokens::new("text", 42);
    assert_eq!(tokens.modality, "text");
    assert_eq!(tokens.tokens, 42);

    // Accepts both `&str` and `String` via `impl Into<String>`.
    let owned = ModalityTokens::new(String::from("image"), 7);
    assert_eq!(owned.modality, "image");
    assert_eq!(owned.tokens, 7);

    // And round-trips through serde like a deserialized one.
    let wire = serde_json::to_value(&tokens).unwrap();
    let back: ModalityTokens = serde_json::from_value(wire).unwrap();
    assert_eq!(back, tokens);
}

#[test]
fn test_modality_tokens_serialization() {
    let tokens = ModalityTokens {
        modality: "text".to_string(),
        tokens: 100,
    };

    let json = serde_json::to_string(&tokens).unwrap();
    assert!(json.contains("\"modality\":\"text\""));
    assert!(json.contains("\"tokens\":100"));

    // Roundtrip
    let deserialized: ModalityTokens = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.modality, "text");
    assert_eq!(deserialized.tokens, 100);
}

#[test]
fn test_grounding_tool_count_wire_format() {
    let json = r#"{"type": "google_maps", "count": 3}"#;
    let count: GroundingToolCount = serde_json::from_str(json).unwrap();
    assert_eq!(count.tool_type.as_deref(), Some("google_maps"));
    assert_eq!(count.count, Some(3));

    let out = serde_json::to_value(&count).unwrap();
    assert_eq!(out["type"], "google_maps");
    assert_eq!(out["count"], 3);
}

#[test]
fn test_input_tokens_for_modality() {
    let usage = UsageMetadata {
        input_tokens_by_modality: Some(vec![
            ModalityTokens {
                modality: "text".to_string(),
                tokens: 100,
            },
            ModalityTokens {
                modality: "image".to_string(),
                tokens: 500,
            },
        ]),
        ..Default::default()
    };

    assert_eq!(usage.input_tokens_for_modality("text"), Some(100));
    assert_eq!(usage.input_tokens_for_modality("image"), Some(500));
    assert_eq!(usage.input_tokens_for_modality("video"), None);
}

#[test]
fn test_cache_hit_rate() {
    // 25% cache hit rate
    let usage = UsageMetadata {
        total_input_tokens: Some(100),
        total_cached_tokens: Some(25),
        ..Default::default()
    };
    let rate = usage.cache_hit_rate().unwrap();
    assert!((rate - 0.25).abs() < f32::EPSILON);

    // Zero input tokens (avoid division by zero)
    let usage = UsageMetadata {
        total_input_tokens: Some(0),
        total_cached_tokens: Some(0),
        ..Default::default()
    };
    assert!(usage.cache_hit_rate().is_none());
}

#[test]
fn test_has_data_with_grounding_tool_count() {
    let usage = UsageMetadata {
        grounding_tool_count: Some(vec![GroundingToolCount {
            tool_type: Some("retrieval".into()),
            count: Some(1),
        }]),
        ..Default::default()
    };
    assert!(usage.has_data());
    assert!(!UsageMetadata::default().has_data());
}

// =========================================================================
// Token Count Deserialization Edge Cases
// =========================================================================

#[test]
fn test_negative_token_count_clamped_to_zero() {
    let json = r#"{"total_input_tokens": -100, "total_output_tokens": 50}"#;
    let usage: UsageMetadata = serde_json::from_str(json).unwrap();

    assert_eq!(usage.total_input_tokens, Some(0));
    assert_eq!(usage.total_output_tokens, Some(50));
}

#[test]
fn test_large_token_count_clamped_to_u32_max() {
    let json = r#"{"total_input_tokens": 5000000000}"#;
    let usage: UsageMetadata = serde_json::from_str(json).unwrap();

    assert_eq!(usage.total_input_tokens, Some(u32::MAX));
}

// =========================================================================
// Usage accumulation
// =========================================================================

#[test]
fn test_usage_metadata_accumulate_all_fields() {
    let mut usage1 = UsageMetadata {
        total_input_tokens: Some(100),
        total_output_tokens: Some(50),
        total_tokens: Some(150),
        total_cached_tokens: Some(20),
        total_thought_tokens: Some(5),
        total_tool_use_tokens: Some(15),
        ..Default::default()
    };
    let usage2 = UsageMetadata {
        total_input_tokens: Some(200),
        total_output_tokens: Some(100),
        total_tokens: Some(300),
        total_cached_tokens: Some(40),
        total_thought_tokens: Some(10),
        total_tool_use_tokens: Some(30),
        ..Default::default()
    };

    usage1.accumulate(&usage2);

    assert_eq!(usage1.total_input_tokens, Some(300));
    assert_eq!(usage1.total_output_tokens, Some(150));
    assert_eq!(usage1.total_tokens, Some(450));
    assert_eq!(usage1.total_cached_tokens, Some(60));
    assert_eq!(usage1.total_thought_tokens, Some(15));
    assert_eq!(usage1.total_tool_use_tokens, Some(45));
}

#[test]
fn test_usage_metadata_accumulate_saturating() {
    let mut usage1 = UsageMetadata {
        total_input_tokens: Some(u32::MAX - 10),
        ..Default::default()
    };
    let usage2 = UsageMetadata {
        total_input_tokens: Some(100),
        ..Default::default()
    };

    usage1.accumulate(&usage2);

    assert_eq!(usage1.total_input_tokens, Some(u32::MAX));
}

/// Shapes from a live `gemini-3.8-flash` response (2026-09-24).
#[test]
fn usage_preserves_unmodeled_live_fields() {
    let wire = serde_json::json!({
        "total_tokens": 123,
        "total_input_tokens": 7,
        "raw_prompt_token": 38,
        "model_invocation_token_counts": [{
            "prompt_tokens_details": [{"modality": "text", "tokens": 38}],
            "candidates_tokens_details": [{"modality": "text", "tokens": 5}]
        }]
    });
    let usage: UsageMetadata = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(usage.total_tokens, Some(123));
    assert_eq!(usage.extra["raw_prompt_token"], 38);
    assert_eq!(serde_json::to_value(&usage).unwrap(), wire);
}
