//! Unit tests for the builder's generation and request settings: generation
//! config, webhook config, environment, safety settings, labels and
//! decoding continuation.

use super::super::tests::create_test_client;
use super::*;
use serde_json::json;

#[test]
fn test_interaction_builder_with_generation_config() {
    let client = create_test_client();
    let config = crate::GenerationConfig {
        temperature: Some(0.7),
        max_output_tokens: Some(1000),
        top_p: Some(0.9),
        thinking_level: Some(ThinkingLevel::Medium),
        ..Default::default()
    };

    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Hello")
        .with_generation_config(config.clone());

    assert!(builder.generation_config.is_some());
    assert_eq!(
        builder.generation_config.as_ref().unwrap().temperature,
        Some(0.7)
    );
}

#[test]
fn test_builder_with_webhook_config() {
    use crate::WebhookConfig;

    let client = create_test_client();
    let request = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Hello")
        .with_webhook_config(
            WebhookConfig::new()
                .with_uris(vec!["https://example.com/hook".to_string()])
                .with_user_metadata(json!({"job": 1})),
        )
        .build()
        .unwrap();

    let value = serde_json::to_value(&request).unwrap();
    assert_eq!(
        value["webhook_config"]["uris"][0],
        "https://example.com/hook"
    );
    assert_eq!(value["webhook_config"]["user_metadata"]["job"], 1);
}

#[test]
fn test_builder_with_environment_id() {
    let client = create_test_client();
    let request = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Hello")
        .with_environment("38aac1ae7f30fe9bd67afe42382ea041")
        .build()
        .unwrap();

    let value = serde_json::to_value(&request).unwrap();
    assert_eq!(value["environment"], "38aac1ae7f30fe9bd67afe42382ea041");
}

#[test]
fn test_builder_with_typed_remote_environment() {
    use crate::{AllowlistEntry, EnvironmentSource, NetworkConfig, RemoteEnvironment};

    let client = create_test_client();
    let request = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Hello")
        .with_environment(
            RemoteEnvironment::new()
                .add_source(EnvironmentSource::gcs("gs://bucket", "/data"))
                .with_network(NetworkConfig::allowlist(vec![AllowlistEntry::new(
                    "*.googleapis.com",
                )])),
        )
        .build()
        .unwrap();

    let value = serde_json::to_value(&request).unwrap();
    assert_eq!(value["environment"]["type"], "remote");
    assert_eq!(value["environment"]["sources"][0]["type"], "gcs");
    assert_eq!(
        value["environment"]["network"]["allowlist"][0]["domain"],
        "*.googleapis.com"
    );
}

#[test]
fn test_builder_safety_settings_replace_and_accumulate() {
    use crate::{HarmCategory, SafetySetting, SafetyThreshold};

    let client = create_test_client();
    // add_* accumulates: two adds yield two entries, appended after with_*.
    let request = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Hello")
        .with_safety_settings(vec![SafetySetting::new(
            HarmCategory::HateSpeech,
            SafetyThreshold::BlockLowAndAbove,
        )])
        .add_safety_setting(SafetySetting::new(
            HarmCategory::Harassment,
            SafetyThreshold::BlockOnlyHigh,
        ))
        .add_safety_setting(SafetySetting::new(
            HarmCategory::Jailbreak,
            SafetyThreshold::Off,
        ))
        .build()
        .unwrap();

    let value = serde_json::to_value(&request).unwrap();
    let settings = value["safety_settings"].as_array().unwrap();
    assert_eq!(settings.len(), 3, "add_* must accumulate onto with_*");
    assert_eq!(settings[0]["type"], "hate_speech");
    assert_eq!(settings[1]["type"], "harassment");
    assert_eq!(settings[1]["threshold"], "block_only_high");
    assert_eq!(settings[2]["threshold"], "off");
}

#[test]
fn test_builder_with_safety_settings_replaces() {
    use crate::{HarmCategory, SafetySetting, SafetyThreshold};

    let client = create_test_client();
    let request = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Hello")
        .add_safety_setting(SafetySetting::new(
            HarmCategory::HateSpeech,
            SafetyThreshold::Off,
        ))
        .with_safety_settings(vec![SafetySetting::new(
            HarmCategory::CivicIntegrity,
            SafetyThreshold::BlockNone,
        )])
        .build()
        .unwrap();

    let value = serde_json::to_value(&request).unwrap();
    let settings = value["safety_settings"].as_array().unwrap();
    assert_eq!(settings.len(), 1, "with_* must replace");
    assert_eq!(settings[0]["type"], "civic_integrity");
}

#[test]
fn test_builder_labels_merge_and_replace() {
    let client = create_test_client();
    let request = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Hello")
        .add_label("team", "search")
        .add_label("env", "ci")
        .add_label("team", "platform") // same key: last write wins
        .build()
        .unwrap();

    let value = serde_json::to_value(&request).unwrap();
    assert_eq!(value["labels"]["team"], "platform");
    assert_eq!(value["labels"]["env"], "ci");
    assert_eq!(value["labels"].as_object().unwrap().len(), 2);

    // with_labels replaces wholesale.
    let request = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Hello")
        .add_label("stale", "1")
        .with_labels([("fresh", "2")])
        .build()
        .unwrap();
    let value = serde_json::to_value(&request).unwrap();
    assert!(value["labels"].get("stale").is_none());
    assert_eq!(value["labels"]["fresh"], "2");
}

/// The token rides at the top level beside the original input, which a
/// continuation must resend unchanged (live 2026-11-01).
#[test]
fn test_with_continuation_token_serializes_top_level() {
    let request = create_test_client()
        .interaction()
        .with_model(crate::CONTINUATION_MODEL)
        .with_text("Write a sonnet about the sea.")
        .with_continuation_token("CiQxYzQ5ZGIwZi0=")
        .build()
        .unwrap();
    let wire = serde_json::to_value(&request).unwrap();
    assert_eq!(wire["continuation_token"], "CiQxYzQ5ZGIwZi0=");
    assert_eq!(wire["input"], "Write a sonnet about the sea.");

    let plain = create_test_client()
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Hi")
        .build()
        .unwrap();
    assert!(
        serde_json::to_value(&plain)
            .unwrap()
            .get("continuation_token")
            .is_none()
    );
}

/// The response a decode stopped in, as the API returns it.
fn cut_off_response() -> crate::InteractionResponse {
    serde_json::from_value(json!({
        "id": "v1_stored",
        "model": crate::CONTINUATION_MODEL,
        "status": "incomplete",
        "continuation_token": "CtYBCtEB"
    }))
    .unwrap()
}

/// A stored decode continues from its id and token, with no `input` key
/// (live 2026-11-05).
#[test]
fn test_continue_decode_sends_no_input() {
    let request = create_test_client()
        .interaction()
        .continue_decode(&cut_off_response())
        .build()
        .unwrap();
    assert_eq!(request.input, None);
    assert_eq!(
        serde_json::to_value(&request).unwrap(),
        json!({
            "model": crate::CONTINUATION_MODEL,
            "previous_interaction_id": "v1_stored",
            "continuation_token": "CtYBCtEB"
        })
    );
}

#[test]
fn test_continue_decode_replaces_the_model_and_keeps_other_settings() {
    let request = create_test_client()
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_generation_config(crate::GenerationConfig {
            max_output_tokens: Some(500),
            ..Default::default()
        })
        .continue_decode(&cut_off_response())
        .build()
        .unwrap();
    assert_eq!(request.model.as_deref(), Some(crate::CONTINUATION_MODEL));
    assert_eq!(
        request
            .generation_config
            .as_ref()
            .and_then(|c| c.max_output_tokens),
        Some(500)
    );
}

#[test]
fn test_no_input_without_a_continuation_is_an_error() {
    let err = create_test_client()
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .build()
        .unwrap_err();
    assert!(err.to_string().contains("Input is required"), "{err}");
}

#[test]
fn test_continuation_without_input_needs_the_stored_id() {
    // A response from a `store: false` request has no id.
    let mut response = cut_off_response();
    response.id = None;
    let err = create_test_client()
        .interaction()
        .continue_decode(&response)
        .build()
        .unwrap_err();
    assert!(err.to_string().contains("previous_interaction"), "{err}");

    // Resending the original input is the store-disabled form.
    let request = create_test_client()
        .interaction()
        .continue_decode(&response)
        .with_text("Write a sonnet about the sea.")
        .with_store_disabled()
        .build()
        .unwrap();
    assert!(request.input.is_some() && request.continuation_token.is_some());
}

/// `continue_decode` replaces the builder's previous interaction and token
/// with the response's, clearing them when the response has none: a token
/// belongs to the interaction that issued it, so keeping either would pair
/// it with another one.
#[test]
fn test_continue_decode_replaces_the_id_and_token_even_with_none() {
    let client = create_test_client();
    let continued = |response: &crate::InteractionResponse| {
        client
            .interaction()
            .with_text("Write a sonnet about the sea.")
            .with_previous_interaction("v1_earlier_turn")
            .with_continuation_token("CiQxYzQ5ZGIwZi0=")
            .continue_decode(response)
            .build()
            .unwrap()
    };

    let request = continued(&cut_off_response());
    assert_eq!(
        request.previous_interaction_id.as_deref(),
        Some("v1_stored")
    );
    assert_eq!(request.continuation_token.as_deref(), Some("CtYBCtEB"));

    // A `store: false` response: no id, so no previous interaction.
    let mut no_id = cut_off_response();
    no_id.id = None;
    let request = continued(&no_id);
    assert_eq!(request.previous_interaction_id, None);
    assert_eq!(request.continuation_token.as_deref(), Some("CtYBCtEB"));

    // Nothing to continue: no token, so this is a plain follow-up turn.
    let mut no_token = cut_off_response();
    no_token.continuation_token = None;
    let request = continued(&no_token);
    assert_eq!(
        request.previous_interaction_id.as_deref(),
        Some("v1_stored")
    );
    assert_eq!(request.continuation_token, None);
}

#[test]
fn test_continue_decode_on_a_response_without_a_token_needs_input() {
    let mut response = cut_off_response();
    response.continuation_token = None;
    let err = create_test_client()
        .interaction()
        .continue_decode(&response)
        .build()
        .unwrap_err();
    assert!(err.to_string().contains("Input is required"), "{err}");
}
