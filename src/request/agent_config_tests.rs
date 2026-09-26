use super::*;

// =========================================================================
// Agent Config Tests
// =========================================================================

#[test]
fn test_thinking_summaries_serialization() {
    // GenerationConfig wire format uses lowercase
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
fn test_thinking_summaries_agent_config_format() {
    // AgentConfig uses THINKING_SUMMARIES_* format via to_agent_config_value()
    assert_eq!(
        ThinkingSummaries::Auto.to_agent_config_value(),
        serde_json::Value::String("auto".to_string())
    );

    assert_eq!(
        ThinkingSummaries::None.to_agent_config_value(),
        serde_json::Value::String("none".to_string())
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
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_thinking_summaries_unknown_roundtrip() {
    let unknown: ThinkingSummaries = serde_json::from_str("\"future_variant\"").unwrap();
    assert!(unknown.is_unknown());
    assert_eq!(unknown.unknown_summaries_type(), Some("future_variant"));

    // Roundtrip preserves the unknown value
    let json = serde_json::to_string(&unknown).unwrap();
    assert_eq!(json, "\"future_variant\"");
}

#[test]
fn test_deep_research_config_serialization() {
    let config: AgentConfig = DeepResearchConfig::new()
        .with_thinking_summaries(ThinkingSummaries::Auto)
        .into();

    let json = serde_json::to_string(&config).expect("Serialization failed");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "deep-research");
    assert_eq!(value["thinking_summaries"], "auto");
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
fn test_agent_config_from_raw_json() {
    let config = AgentConfig::from_value(serde_json::json!({
        "type": "custom-agent",
        "option1": true,
        "option2": "value"
    }));

    assert_eq!(config.config_type(), Some("custom-agent"));
    assert_eq!(config.as_value()["option1"], true);
}

#[test]
fn test_agent_config_roundtrip() {
    let config: AgentConfig = DeepResearchConfig::new()
        .with_thinking_summaries(ThinkingSummaries::Auto)
        .into();

    let json = serde_json::to_string(&config).expect("Serialization failed");
    let parsed: AgentConfig = serde_json::from_str(&json).expect("Deserialization failed");

    assert_eq!(config, parsed);
}
