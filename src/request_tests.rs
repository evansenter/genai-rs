//! Unit tests for request types (InteractionRequest, GenerationConfig, etc.)

use super::*;

#[test]
fn test_serialize_create_interaction_request_with_model() {
    let request = InteractionRequest {
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        agent_config: None,
        input: InteractionInput::Text("Hello, world!".to_string()),
        previous_interaction_id: None,
        tools: None,
        response_modalities: None,
        response_format: None,
        response_mime_type: None,
        generation_config: None,
        stream: None,
        background: None,
        store: None,
        system_instruction: None,
    };

    let json = serde_json::to_string(&request).expect("Serialization failed");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["model"], "gemini-3-flash-preview");
    assert_eq!(value["input"], "Hello, world!");
    assert!(value.get("agent").is_none());
}

#[test]
fn test_generation_config_serialization() {
    let config = GenerationConfig {
        temperature: Some(0.7),
        max_output_tokens: Some(500),
        top_p: Some(0.9),
        top_k: Some(40),
        thinking_level: Some(ThinkingLevel::Medium),
        seed: None,
        stop_sequences: None,
        thinking_summaries: None,
        tool_choice: None,
        allowed_tools: None,
        speech_config: None,
        image_config: None,
    };

    let json = serde_json::to_string(&config).expect("Serialization failed");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["temperature"], 0.7);
    assert_eq!(value["max_output_tokens"], 500);
    assert_eq!(value["thinking_level"], "medium");
}

#[test]
fn test_generation_config_new_fields_serialization() {
    let config = GenerationConfig {
        temperature: None,
        max_output_tokens: None,
        top_p: None,
        top_k: None,
        thinking_level: Some(ThinkingLevel::High),
        seed: Some(42),
        stop_sequences: Some(vec!["END".to_string(), "---".to_string()]),
        thinking_summaries: Some(ThinkingSummaries::Auto),
        tool_choice: None,
        allowed_tools: None,
        speech_config: None,
        image_config: None,
    };

    let json = serde_json::to_string(&config).expect("Serialization failed");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["seed"], 42);
    assert_eq!(value["stop_sequences"][0], "END");
    assert_eq!(value["stop_sequences"][1], "---");
    assert_eq!(value["thinking_summaries"], "auto");
    assert_eq!(value["thinking_level"], "high");
}

#[test]
fn test_generation_config_roundtrip() {
    let config = GenerationConfig {
        temperature: Some(0.5),
        max_output_tokens: Some(1000),
        top_p: Some(0.95),
        top_k: Some(50),
        thinking_level: Some(ThinkingLevel::Low),
        seed: Some(123456789),
        stop_sequences: Some(vec!["STOP".to_string()]),
        thinking_summaries: Some(ThinkingSummaries::None),
        tool_choice: None,
        allowed_tools: None,
        speech_config: None,
        image_config: None,
    };

    let json = serde_json::to_string(&config).expect("Serialization failed");
    let deserialized: GenerationConfig =
        serde_json::from_str(&json).expect("Deserialization failed");

    assert_eq!(deserialized.temperature, config.temperature);
    assert_eq!(deserialized.max_output_tokens, config.max_output_tokens);
    assert_eq!(deserialized.top_p, config.top_p);
    assert_eq!(deserialized.top_k, config.top_k);
    assert_eq!(deserialized.thinking_level, config.thinking_level);
    assert_eq!(deserialized.seed, config.seed);
    assert_eq!(deserialized.stop_sequences, config.stop_sequences);
    assert_eq!(deserialized.thinking_summaries, config.thinking_summaries);
    assert_eq!(deserialized.tool_choice, config.tool_choice);
}

#[test]
fn test_thinking_summaries_serialization() {
    // GenerationConfig wire format uses lowercase (auto/none)
    // Note: AgentConfig uses THINKING_SUMMARIES_* via to_agent_config_value() - see agent_config.rs tests
    assert_eq!(
        serde_json::to_string(&ThinkingSummaries::Auto).unwrap(),
        "\"auto\""
    );

    assert_eq!(
        serde_json::to_string(&ThinkingSummaries::None).unwrap(),
        "\"none\""
    );
}

#[test]
fn test_thinking_summaries_deserialization() {
    // Test wire format (THINKING_SUMMARIES_*)
    assert_eq!(
        serde_json::from_str::<ThinkingSummaries>("\"THINKING_SUMMARIES_AUTO\"").unwrap(),
        ThinkingSummaries::Auto
    );
    assert_eq!(
        serde_json::from_str::<ThinkingSummaries>("\"THINKING_SUMMARIES_NONE\"").unwrap(),
        ThinkingSummaries::None
    );

    // Also accept lowercase for flexibility
    assert_eq!(
        serde_json::from_str::<ThinkingSummaries>("\"auto\"").unwrap(),
        ThinkingSummaries::Auto
    );
    assert_eq!(
        serde_json::from_str::<ThinkingSummaries>("\"none\"").unwrap(),
        ThinkingSummaries::None
    );

    // Test unknown value deserializes to Unknown with data preserved (Evergreen principle)
    let unknown: ThinkingSummaries = serde_json::from_str("\"future_variant\"").unwrap();
    assert!(unknown.is_unknown());
    assert_eq!(unknown.unknown_summaries_type(), Some("future_variant"));
    assert_eq!(
        unknown.unknown_data(),
        Some(&serde_json::Value::String("future_variant".to_string()))
    );
}

#[test]
fn test_thinking_summaries_unknown_roundtrip() {
    // Test that unknown values roundtrip correctly
    let unknown = ThinkingSummaries::Unknown {
        summaries_type: "new_mode".to_string(),
        data: serde_json::Value::String("new_mode".to_string()),
    };

    let json = serde_json::to_string(&unknown).expect("Serialization failed");
    assert_eq!(json, "\"new_mode\"");

    let deserialized: ThinkingSummaries = serde_json::from_str(&json).unwrap();
    assert!(deserialized.is_unknown());
    assert_eq!(deserialized.unknown_summaries_type(), Some("new_mode"));
}

#[test]
fn test_thinking_level_deserialization() {
    // Test known values
    assert_eq!(
        serde_json::from_str::<ThinkingLevel>("\"minimal\"").unwrap(),
        ThinkingLevel::Minimal
    );
    assert_eq!(
        serde_json::from_str::<ThinkingLevel>("\"low\"").unwrap(),
        ThinkingLevel::Low
    );
    assert_eq!(
        serde_json::from_str::<ThinkingLevel>("\"medium\"").unwrap(),
        ThinkingLevel::Medium
    );
    assert_eq!(
        serde_json::from_str::<ThinkingLevel>("\"high\"").unwrap(),
        ThinkingLevel::High
    );

    // Test unknown value deserializes to Unknown with data preserved (Evergreen principle)
    let unknown: ThinkingLevel = serde_json::from_str("\"extreme\"").unwrap();
    assert!(unknown.is_unknown());
    assert_eq!(unknown.unknown_level_type(), Some("extreme"));
    assert_eq!(
        unknown.unknown_data(),
        Some(&serde_json::Value::String("extreme".to_string()))
    );
}

#[test]
fn test_thinking_level_serialization() {
    // Test known variants serialize correctly
    assert_eq!(
        serde_json::to_string(&ThinkingLevel::Minimal).unwrap(),
        "\"minimal\""
    );
    assert_eq!(
        serde_json::to_string(&ThinkingLevel::Low).unwrap(),
        "\"low\""
    );
    assert_eq!(
        serde_json::to_string(&ThinkingLevel::Medium).unwrap(),
        "\"medium\""
    );
    assert_eq!(
        serde_json::to_string(&ThinkingLevel::High).unwrap(),
        "\"high\""
    );
}

#[test]
fn test_thinking_level_unknown_roundtrip() {
    // Test that unknown values roundtrip correctly
    let unknown = ThinkingLevel::Unknown {
        level_type: "extreme".to_string(),
        data: serde_json::Value::String("extreme".to_string()),
    };

    let json = serde_json::to_string(&unknown).expect("Serialization failed");
    assert_eq!(json, "\"extreme\"");

    let deserialized: ThinkingLevel = serde_json::from_str(&json).unwrap();
    assert!(deserialized.is_unknown());
    assert_eq!(deserialized.unknown_level_type(), Some("extreme"));
}

#[test]
fn test_generation_config_skip_serializing_none_fields() {
    let config = GenerationConfig::default();

    let json = serde_json::to_string(&config).expect("Serialization failed");

    // Default config should serialize to empty object
    assert_eq!(json, "{}");
}

#[test]
fn test_generation_config_partial_fields() {
    let config = GenerationConfig {
        seed: Some(42),
        stop_sequences: Some(vec!["DONE".to_string()]),
        ..Default::default()
    };

    let json = serde_json::to_string(&config).expect("Serialization failed");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    // Only set fields should be present
    assert_eq!(value["seed"], 42);
    assert_eq!(value["stop_sequences"][0], "DONE");
    assert!(value.get("temperature").is_none());
    assert!(value.get("thinking_level").is_none());
}

#[test]
fn test_thinking_level_object_form_deserialization() {
    // Test that object-form thinking levels are handled (future API compatibility)
    let json = r#"{"level": "ultra", "budget": 5000}"#;
    let parsed: ThinkingLevel = serde_json::from_str(json).expect("Deserialization should succeed");

    assert!(parsed.is_unknown());
    assert_eq!(parsed.unknown_level_type(), Some("ultra"));

    // Verify the full object is preserved
    let data = parsed.unknown_data().unwrap();
    assert_eq!(data.get("budget").unwrap(), 5000);
}

#[test]
fn test_thinking_summaries_object_form_deserialization() {
    // Test that object-form thinking summaries are handled (future API compatibility)
    let json = r#"{"summaries": "detailed", "format": "markdown"}"#;
    let parsed: ThinkingSummaries =
        serde_json::from_str(json).expect("Deserialization should succeed");

    assert!(parsed.is_unknown());
    assert_eq!(parsed.unknown_summaries_type(), Some("detailed"));

    // Verify the full object is preserved
    let data = parsed.unknown_data().unwrap();
    assert_eq!(data.get("format").unwrap(), "markdown");
}

// =============================================================================
// AgentConfig Tests
// =============================================================================

#[test]
fn test_deep_research_config_serialization() {
    let config: AgentConfig = DeepResearchConfig::new()
        .with_thinking_summaries(ThinkingSummaries::Auto)
        .into();

    let json = serde_json::to_string(&config).expect("Serialization failed");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "deep-research");
    assert_eq!(value["thinking_summaries"], "THINKING_SUMMARIES_AUTO");
}

#[test]
fn test_deep_research_config_without_thinking_summaries() {
    let config: AgentConfig = DeepResearchConfig::new().into();

    let json = serde_json::to_string(&config).expect("Serialization failed");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "deep-research");
    assert!(value.get("thinking_summaries").is_none());
}

#[test]
fn test_dynamic_config_serialization() {
    let config: AgentConfig = DynamicConfig::new().into();

    let json = serde_json::to_string(&config).expect("Serialization failed");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "dynamic");
}

#[test]
fn test_agent_config_deserialization_deep_research() {
    let json = r#"{"type": "deep-research", "thinkingSummaries": "auto"}"#;
    let parsed: AgentConfig = serde_json::from_str(json).expect("Deserialization should succeed");

    assert_eq!(parsed.config_type(), Some("deep-research"));
    assert_eq!(
        parsed
            .as_value()
            .get("thinkingSummaries")
            .and_then(|v| v.as_str()),
        Some("auto")
    );
}

#[test]
fn test_agent_config_deserialization_dynamic() {
    let json = r#"{"type": "dynamic"}"#;
    let parsed: AgentConfig = serde_json::from_str(json).expect("Deserialization should succeed");

    assert_eq!(parsed.config_type(), Some("dynamic"));
}

#[test]
fn test_agent_config_deserialization_unknown() {
    // Test that unknown agent config types deserialize successfully (Evergreen principle)
    let json = r#"{"type": "future-agent", "customField": 42}"#;
    let parsed: AgentConfig = serde_json::from_str(json).expect("Deserialization should succeed");

    assert_eq!(parsed.config_type(), Some("future-agent"));

    // Verify the full object is preserved
    let value = parsed.as_value();
    assert_eq!(value.get("customField").unwrap(), 42);
}

#[test]
fn test_agent_config_roundtrip() {
    // Test that values roundtrip correctly
    let config = AgentConfig::from_value(serde_json::json!({
        "type": "future-agent",
        "customField": 42
    }));

    let json = serde_json::to_string(&config).expect("Serialization failed");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    // Should preserve the type and data
    assert_eq!(value["type"], "future-agent");
    assert_eq!(value["customField"], 42);

    // Should roundtrip back correctly
    let deserialized: AgentConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.config_type(), Some("future-agent"));
}

#[test]
fn test_agent_config_helper_methods() {
    // Test config_type() method
    let deep_research: AgentConfig = DeepResearchConfig::new().into();
    assert_eq!(deep_research.config_type(), Some("deep-research"));

    let dynamic: AgentConfig = DynamicConfig::new().into();
    assert_eq!(dynamic.config_type(), Some("dynamic"));

    let custom = AgentConfig::from_value(serde_json::json!({"type": "custom"}));
    assert_eq!(custom.config_type(), Some("custom"));

    // Test as_value() method
    let config: AgentConfig = DeepResearchConfig::new()
        .with_thinking_summaries(ThinkingSummaries::Auto)
        .into();
    let value = config.as_value();
    assert_eq!(value.get("type").unwrap(), "deep-research");
    assert_eq!(
        value.get("thinking_summaries").unwrap(),
        "THINKING_SUMMARIES_AUTO"
    );
}

#[test]
fn test_create_interaction_request_with_agent_config() {
    let config: AgentConfig = DeepResearchConfig::new()
        .with_thinking_summaries(ThinkingSummaries::Auto)
        .into();

    let request = InteractionRequest {
        model: None,
        agent: Some("deep-research-pro-preview-12-2025".to_string()),
        agent_config: Some(config),
        input: InteractionInput::Text("Research question".to_string()),
        previous_interaction_id: None,
        tools: None,
        response_modalities: None,
        response_format: None,
        response_mime_type: None,
        generation_config: None,
        stream: None,
        background: Some(true),
        store: Some(true),
        system_instruction: None,
    };

    let json = serde_json::to_string(&request).expect("Serialization failed");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["agent"], "deep-research-pro-preview-12-2025");
    assert_eq!(value["agent_config"]["type"], "deep-research");
    assert_eq!(
        value["agent_config"]["thinking_summaries"],
        "THINKING_SUMMARIES_AUTO"
    );
    assert_eq!(value["background"], true);
    assert_eq!(value["store"], true);
}

/// Test that verifies the field naming conventions used in AgentConfig serialization.
///
/// This test explicitly documents the casing decisions:
/// - `type` key uses kebab-case for values: "deep-research", "dynamic"
/// - `thinking_summaries` key uses snake_case per API documentation
/// - Values use SCREAMING_SNAKE_CASE: "THINKING_SUMMARIES_AUTO", "THINKING_SUMMARIES_NONE"
///
/// Note: The Gemini Interactions API uses snake_case for field names.
#[test]
fn test_agent_config_field_naming_conventions() {
    // Verify the exact JSON structure matches API expectations
    let config: AgentConfig = DeepResearchConfig::new()
        .with_thinking_summaries(ThinkingSummaries::Auto)
        .into();

    let json = serde_json::to_string(&config).expect("Serialization failed");

    // Expected: {"type":"deep-research","thinking_summaries":"THINKING_SUMMARIES_AUTO"}
    assert!(
        json.contains("thinking_summaries"),
        "Field should be snake_case 'thinking_summaries', got: {}",
        json
    );
    assert!(
        !json.contains("thinkingSummaries"),
        "Field should NOT be camelCase 'thinkingSummaries', got: {}",
        json
    );

    // Verify value uses wire format THINKING_SUMMARIES_*
    assert!(
        json.contains(r#""THINKING_SUMMARIES_AUTO""#),
        "ThinkingSummaries::Auto should serialize to 'THINKING_SUMMARIES_AUTO', got: {}",
        json
    );
}

/// Verifies that `InteractionRequest` roundtrips correctly through JSON.
///
/// This test ensures the `Deserialize` derive (added for retry/serialization support)
/// correctly reconstructs requests from JSON. Important for:
/// - Loading requests from config files
/// - Deserializing from dead-letter queues
/// - Request replay/debugging scenarios
#[test]
fn test_interaction_request_roundtrip() {
    let original = InteractionRequest {
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        agent_config: None,
        input: InteractionInput::Text("Hello, world!".to_string()),
        previous_interaction_id: Some("interaction-123".to_string()),
        tools: None,
        response_modalities: None,
        response_format: None,
        response_mime_type: Some("text/plain".to_string()),
        generation_config: Some(GenerationConfig {
            temperature: Some(0.7),
            max_output_tokens: Some(100),
            ..Default::default()
        }),
        stream: Some(true),
        background: None,
        store: Some(true),
        system_instruction: Some(InteractionInput::Text("Be helpful.".to_string())),
    };

    // Serialize to JSON
    let json = serde_json::to_string(&original).expect("Serialization failed");

    // Deserialize back
    let deserialized: InteractionRequest =
        serde_json::from_str(&json).expect("Deserialization failed");

    // Verify all fields match
    assert_eq!(original.model, deserialized.model);
    assert_eq!(original.agent, deserialized.agent);
    assert_eq!(
        original.previous_interaction_id,
        deserialized.previous_interaction_id
    );
    assert_eq!(original.response_mime_type, deserialized.response_mime_type);
    assert_eq!(original.stream, deserialized.stream);
    assert_eq!(original.store, deserialized.store);

    // Verify generation_config roundtrips
    let orig_config = original.generation_config.as_ref().unwrap();
    let deser_config = deserialized.generation_config.as_ref().unwrap();
    assert_eq!(orig_config.temperature, deser_config.temperature);
    assert_eq!(
        orig_config.max_output_tokens,
        deser_config.max_output_tokens
    );

    // Verify input roundtrips (using Debug comparison since InteractionInput doesn't impl PartialEq)
    assert_eq!(
        format!("{:?}", original.input),
        format!("{:?}", deserialized.input)
    );
    assert_eq!(
        format!("{:?}", original.system_instruction),
        format!("{:?}", deserialized.system_instruction)
    );
}

#[test]
fn test_response_format_serializes_as_snake_case() {
    let request = InteractionRequest {
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        agent_config: None,
        input: InteractionInput::Text("test".to_string()),
        previous_interaction_id: None,
        tools: None,
        response_modalities: None,
        response_format: Some(serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            }
        })),
        response_mime_type: None,
        generation_config: None,
        stream: None,
        background: None,
        store: None,
        system_instruction: None,
    };

    let json = serde_json::to_string(&request).expect("Serialization failed");

    // The Gemini Interactions API requires snake_case for response_format.
    // Verify the struct serializes field names as snake_case (matching Rust field names).
    assert!(
        json.contains("\"response_format\""),
        "Expected snake_case 'response_format' in JSON, got: {json}"
    );
    assert!(
        !json.contains("\"responseFormat\""),
        "Must NOT contain camelCase 'responseFormat' in JSON, got: {json}"
    );

    // Verify deserialization also uses snake_case key.
    // This guards against regressions where removing the rename would cause
    // incoming JSON with "response_format" to silently drop the field.
    let roundtripped: InteractionRequest =
        serde_json::from_str(&json).expect("Deserialization failed");
    assert!(
        roundtripped.response_format.is_some(),
        "response_format should survive serialization roundtrip"
    );
}
