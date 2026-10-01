use super::*;
use crate::wire::TRACING_TARGET;

#[test]
fn test_tracing_forwarder_body_rendering_redacts() {
    // TracingForwarder must apply the same redaction guarantees as
    // LoudWirePrinter to JSON bodies and raw string payloads.
    let body = serde_json::json!({"secret": "whsec_x", "api_key": "k"});
    let rendered = redacted_body_string(&body);
    assert!(!rendered.contains("whsec_x"));
    assert!(!rendered.contains("\"k\""));

    let raw_json = r#"{"new_signing_secret":"whsec_y"}"#;
    assert!(!redacted_raw_string(raw_json).contains("whsec_y"));

    let credential = serde_json::json!({
        "type": "environment_variable", "value": "val-z", "token": "tok-z",
        "env": {"V": {"value": "env-z"}},
    });
    let rendered = redacted_body_string(&credential);
    for secret in ["val-z", "tok-z", "env-z"] {
        assert!(!rendered.contains(secret), "{secret} leaked: {rendered}");
    }

    // Non-JSON payloads pass through unchanged.
    assert_eq!(redacted_raw_string("plain text"), "plain text");
}

#[test]
fn test_tracing_forwarder_emits_to_wire_target() {
    let targets = crate::test_subscriber::capture_targets(|| {
        TracingForwarder::new().on_event(&WireEvent::ResponseStatus { id: 7, status: 200 });
    });
    assert_eq!(targets.as_slice(), [TRACING_TARGET]);
}
