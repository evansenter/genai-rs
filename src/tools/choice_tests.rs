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

#[cfg(not(feature = "strict-unknown"))]
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

#[cfg(not(feature = "strict-unknown"))]
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
