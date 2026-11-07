use super::*;
use serde_json;

#[test]
fn test_function_calling_mode_serialization() {
    // Wire format is lowercase per API revision 2026-05-20
    let test_cases = [
        (FunctionCallingMode::Auto, "\"auto\""),
        (FunctionCallingMode::Any, "\"any\""),
        (FunctionCallingMode::None, "\"none\""),
        (FunctionCallingMode::Validated, "\"validated\""),
    ];

    for (mode, expected_json) in test_cases {
        let json = serde_json::to_string(&mode).expect("Serialization failed");
        assert_eq!(json, expected_json);

        let parsed: FunctionCallingMode =
            serde_json::from_str(&json).expect("Deserialization failed");
        assert_eq!(parsed, mode);
    }
}

#[cfg(not(genai_strict_unknown))]
#[test]
fn test_function_calling_mode_unknown_roundtrip() {
    // Test that unknown modes are preserved
    let json = "\"FUTURE_MODE\"";
    let parsed: FunctionCallingMode = serde_json::from_str(json).expect("Deserialization failed");

    assert!(parsed.is_unknown());
    assert_eq!(parsed.unknown_mode_type(), Some("FUTURE_MODE"));

    // Roundtrip should preserve the mode type
    let reserialized = serde_json::to_string(&parsed).expect("Serialization failed");
    assert_eq!(reserialized, json);
}

#[test]
fn test_function_calling_mode_helper_methods() {
    // Known modes should not be unknown
    assert!(!FunctionCallingMode::Auto.is_unknown());
    assert!(!FunctionCallingMode::Any.is_unknown());
    assert!(!FunctionCallingMode::None.is_unknown());
    assert!(!FunctionCallingMode::Validated.is_unknown());

    assert!(FunctionCallingMode::Auto.unknown_mode_type().is_none());
    assert!(FunctionCallingMode::Auto.unknown_data().is_none());

    // Unknown mode should report its type
    let unknown = FunctionCallingMode::Unknown {
        mode_type: "NEW_MODE".to_string(),
        data: serde_json::json!("NEW_MODE"),
    };
    assert!(unknown.is_unknown());
    assert_eq!(unknown.unknown_mode_type(), Some("NEW_MODE"));
    assert!(unknown.unknown_data().is_some());
}

#[cfg(not(genai_strict_unknown))]
#[test]
fn test_function_calling_mode_non_string_value() {
    // Test that non-string JSON values are handled gracefully
    let json = "123";
    let parsed: FunctionCallingMode =
        serde_json::from_str(json).expect("Deserialization should succeed");

    assert!(parsed.is_unknown());
    // The mode_type should indicate it was a non-string value
    assert!(parsed.unknown_mode_type().unwrap().contains("<non-string:"));
}

/// A key beside `allowed_tools` is kept and written back after it, as
/// `NetworkConfig::Allowlist` keeps the keys beside `allowlist` (D-001): a
/// stored interaction's `tool_choice` used to lose it.
#[test]
fn allowed_tools_keeps_its_sibling_keys() {
    let wire = serde_json::json!({
        "allowed_tools": {"mode": "any", "tools": ["f"], "inner_knob": 1},
        "future_knob": {"a": [1]},
        "type": "kept as data"
    });
    let choice: ToolChoice = serde_json::from_value(wire.clone()).unwrap();
    let ToolChoice::AllowedTools {
        allowed_tools,
        extra,
    } = &choice
    else {
        panic!("{choice:?}");
    };
    assert_eq!(allowed_tools.tools, ["f"]);
    assert_eq!(allowed_tools.extra["inner_knob"], 1);
    assert_eq!(extra["future_knob"], serde_json::json!({"a": [1]}));
    assert_eq!(choice.extra(), Some(extra));
    assert_eq!(serde_json::to_value(&choice).unwrap(), wire);
    assert_eq!(
        serde_json::to_string(&choice).unwrap(),
        r#"{"allowed_tools":{"mode":"any","tools":["f"],"inner_knob":1},"future_knob":{"a":[1]},"type":"kept as data"}"#
    );
}

/// A hand-set `allowed_tools` entry in `extra` cannot shadow the modeled one.
#[test]
fn allowed_tools_extra_never_shadows_the_restriction() {
    let mut choice = ToolChoice::from(AllowedTools::new(vec!["f".into()]));
    let extra = choice.extra_mut().unwrap();
    extra.insert("allowed_tools".into(), serde_json::json!({"tools": ["g"]}));
    extra.insert("future_knob".into(), serde_json::json!(true));
    assert_eq!(
        serde_json::to_string(&choice).unwrap(),
        r#"{"allowed_tools":{"tools":["f"]},"future_knob":true}"#
    );
}

#[test]
fn tool_choice_from_allowed_tools_and_modes() {
    let choice = ToolChoice::from(AllowedTools::new(vec!["f".into()]));
    assert_eq!(
        choice,
        ToolChoice::allowed_tools(None, vec!["f".into()]),
        "From<AllowedTools> matches the constructor"
    );
    assert_eq!(choice.extra().map(serde_json::Map::len), Some(0));
    let mode = ToolChoice::from(FunctionCallingMode::Any);
    assert!(mode.extra().is_none());
}
