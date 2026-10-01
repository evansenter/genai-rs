//! Evergreen `extra` on list envelopes and `SigningSecret` (#460).
//!
//! Offline: no API key needed. Written from outside the crate so it only uses
//! what a downstream caller can.

use serde_json::json;

/// Deserializes `{<token_key>: "t", "future_field": {...}}` into `$ty`, then
/// asserts three things: the unknown key lands in `extra`, the modeled
/// page-token key does *not* (it must not be double-captured), and both
/// survive re-serialization.
macro_rules! assert_envelope_keeps_unknown_fields {
    ($ty:ty, $token_key:literal) => {{
        let wire = json!({ $token_key: "t", "future_field": {"a": 1} });
        let parsed: $ty = serde_json::from_value(wire).unwrap_or_else(|e| {
            panic!("{}: failed to deserialize: {e}", stringify!($ty))
        });
        assert_eq!(
            parsed.extra.get("future_field"),
            Some(&json!({"a": 1})),
            "{}: unknown field not captured in extra",
            stringify!($ty)
        );
        assert_eq!(
            parsed.extra.len(),
            1,
            "{}: extra captured a modeled field: {:?}",
            stringify!($ty),
            parsed.extra.keys().collect::<Vec<_>>()
        );
        assert_eq!(parsed.next_page_token.as_deref(), Some("t"));

        let back = serde_json::to_value(&parsed).unwrap();
        assert_eq!(back["future_field"], json!({"a": 1}), "{}", stringify!($ty));
        assert_eq!(back[$token_key], json!("t"), "{}", stringify!($ty));
    }};
}

#[test]
fn list_envelopes_keep_unknown_fields() {
    assert_envelope_keeps_unknown_fields!(genai_rs::AgentListResponse, "next_page_token");
    assert_envelope_keeps_unknown_fields!(genai_rs::CredentialListResponse, "next_page_token");
    assert_envelope_keeps_unknown_fields!(genai_rs::EnvironmentListResponse, "next_page_token");
    assert_envelope_keeps_unknown_fields!(genai_rs::TriggerListResponse, "next_page_token");
    assert_envelope_keeps_unknown_fields!(
        genai_rs::TriggerExecutionListResponse,
        "next_page_token"
    );
    assert_envelope_keeps_unknown_fields!(genai_rs::VoiceListResponse, "next_page_token");
    assert_envelope_keeps_unknown_fields!(genai_rs::WebhookListResponse, "next_page_token");
    // camelCase on the wire (see the module docs of each).
    assert_envelope_keeps_unknown_fields!(genai_rs::FileSearchStoreListResponse, "nextPageToken");
    assert_envelope_keeps_unknown_fields!(genai_rs::DocumentListResponse, "nextPageToken");
    assert_envelope_keeps_unknown_fields!(genai_rs::ListFilesResponse, "nextPageToken");
}

#[test]
fn signing_secret_keeps_unknown_fields() {
    let wire = json!({"truncated_secret": "abc…", "future_field": "value"});
    let secret: genai_rs::SigningSecret = serde_json::from_value(wire).unwrap();
    assert_eq!(secret.extra.get("future_field"), Some(&json!("value")));
    assert_eq!(secret.extra.len(), 1);
    let back = serde_json::to_value(&secret).unwrap();
    assert_eq!(back["future_field"], json!("value"));
}

#[test]
fn signing_secret_debug_prints_extra_keys_not_values() {
    // An unmodeled field on a secret-bearing type is the likeliest place for
    // the API to add something sensitive, so Debug must not echo its value.
    let wire = json!({"truncated_secret": "abc…", "secret": "whsec_full_value"});
    let secret: genai_rs::SigningSecret = serde_json::from_value(wire).unwrap();
    let debug = format!("{secret:?}");
    assert!(!debug.contains("whsec_full_value"), "value leaked: {debug}");
    assert!(
        debug.contains("\"secret\""),
        "key should stay visible: {debug}"
    );
    assert!(debug.contains("abc…"), "modeled field missing: {debug}");
}
