//! Unit tests for `InteractionBuilder`'s core: model, agent and instruction
//! setters, storage and background rules, and `build()` validation. The
//! input, tool, output and generation setters are tested beside their files
//! (`input_tests.rs`, `tools_tests.rs`, `output_tests.rs`,
//! `generation_tests.rs`).

use super::*;
use crate::Tool;
use crate::{Client, DeepResearchConfig, FunctionDeclaration, ThinkingSummaries};
use serde_json::json;

pub(super) fn create_test_client() -> Client {
    Client::builder("test-api-key".to_string())
        .build()
        .expect("test client should build")
}

#[test]
fn test_function_declaration_builder() {
    let func_decl = FunctionDeclaration::builder("my_func")
        .with_description("Does something")
        .add_parameter("arg1", json!({"type": "string"}))
        .with_required(vec!["arg1".to_string()])
        .build();

    assert_eq!(func_decl.name(), "my_func");
    assert_eq!(func_decl.description(), "Does something");
    assert_eq!(func_decl.parameters().type_(), "object");
    assert_eq!(
        func_decl
            .parameters()
            .properties()
            .get("arg1")
            .unwrap()
            .get("type")
            .unwrap()
            .as_str(),
        Some("string")
    );
    assert_eq!(func_decl.parameters().required(), vec!["arg1".to_string()]);
}

#[test]
fn test_function_declaration_into_tool() {
    let func_decl = FunctionDeclaration::builder("test")
        .with_description("Test function")
        .build();

    let tool = func_decl.into_tool();
    match tool {
        Tool::Function { name, .. } => {
            assert_eq!(name, "test");
        }
        _ => panic!("Expected Tool::Function variant"),
    }
}

// --- InteractionBuilder Tests ---

#[test]
fn test_interaction_builder_with_model() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Hello");

    assert_eq!(builder.model.as_deref(), Some(crate::DEFAULT_MODEL));
    assert!(builder.agent.is_none());
    assert_eq!(builder.current_message.as_deref(), Some("Hello"));
}

#[test]
fn test_interaction_builder_with_agent() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_agent("deep-research-pro")
        .with_text("Research topic");

    assert!(builder.model.is_none());
    assert_eq!(builder.agent.as_deref(), Some("deep-research-pro"));
}

#[test]
fn test_interaction_builder_with_previous_interaction() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Follow-up question")
        .with_previous_interaction("interaction_123");

    assert_eq!(
        builder.previous_interaction_id.as_deref(),
        Some("interaction_123")
    );
}

#[test]
fn test_interaction_builder_with_system_instruction() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Hello")
        .with_system_instruction("You are a helpful assistant");

    // System instruction is a plain string per the 2026-05-20 API revision
    assert_eq!(
        builder.system_instruction.as_deref(),
        Some("You are a helpful assistant")
    );
}

#[test]
fn test_interaction_builder_with_background() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_agent("deep-research-pro")
        .with_text("Long running task")
        .with_background(true);

    assert_eq!(builder.background, Some(true));
}

#[test]
fn test_interaction_builder_with_store_disabled() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Temporary interaction")
        .with_store_disabled();

    // with_store_disabled() sets store = Some(false)
    // Invalid combinations are validated at build() time
    assert_eq!(builder.store, Some(false));
}

#[test]
fn test_interaction_builder_with_store_enabled() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Stored interaction")
        .with_store_enabled();

    assert_eq!(builder.store, Some(true));
}

#[test]
fn test_interaction_builder_build_success() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Hello");

    let result = builder.build();
    assert!(result.is_ok());

    let request = result.unwrap();
    assert_eq!(request.model.as_deref(), Some(crate::DEFAULT_MODEL));
    assert!(matches!(
        request.input,
        Some(crate::InteractionInput::Text(_))
    ));
}

#[test]
fn test_interaction_builder_build_missing_input() {
    let client = create_test_client();
    let builder = client.interaction().with_model(crate::DEFAULT_MODEL);

    let result = builder.build();
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        crate::GenaiError::InvalidInput(_)
    ));
}

#[test]
fn test_interaction_builder_build_missing_model_and_agent() {
    let client = create_test_client();
    let builder = client.interaction().with_text("Hello");

    let result = builder.build();
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        crate::GenaiError::InvalidInput(_)
    ));
}

// --- Builder State Tests ---
//
// These tests verify runtime validation of API constraints.
// Invalid combinations are caught at build time with descriptive errors.

#[test]
fn test_builder_system_instruction_available() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Hello")
        .with_system_instruction("Be helpful");

    assert!(builder.system_instruction.is_some());
}

#[test]
fn test_builder_chained_preserves_fields() {
    // All fields are preserved when chaining methods
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Hello")
        .with_system_instruction("Be helpful")
        .with_previous_interaction("prev-123");

    assert_eq!(builder.model.as_deref(), Some(crate::DEFAULT_MODEL));
    assert!(builder.system_instruction.is_some());
    assert_eq!(builder.previous_interaction_id.as_deref(), Some("prev-123"));
}

#[test]
fn test_builder_store_disabled_sets_store_false() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Hello")
        .with_store_disabled();

    assert_eq!(builder.store, Some(false));
}

#[test]
fn test_store_disabled_with_background_validation_error() {
    // store=false + background=true is invalid (background needs storage)
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Hello")
        .with_background(true)
        .with_store_disabled();

    // Building should fail with validation error
    let result = builder.build();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string()
            .contains("Background execution requires storage"),
        "Expected background+store error, got: {}",
        err
    );
}

#[test]
fn test_store_disabled_with_previous_interaction_validation_error() {
    // store=false + previous_interaction_id is invalid (chaining needs storage)
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Hello")
        .with_previous_interaction("prev-123")
        .with_store_disabled();

    // Building should fail with validation error
    let result = builder.build();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string()
            .contains("Chained interactions require storage"),
        "Expected chaining+store error, got: {}",
        err
    );
}

#[test]
fn test_builder_chained_can_set_background() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Hello")
        .with_previous_interaction("prev-123")
        .with_background(true);

    assert_eq!(builder.background, Some(true));
}

#[test]
fn test_builder_can_set_background() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Hello")
        .with_background(true);

    assert_eq!(builder.background, Some(true));
}

// NOTE: The following constraints are enforced by runtime validation in build():
//
// 1. store=false cannot combine with:
//    - with_previous_interaction() - chaining requires storage
//    - with_background(true) - background requires storage
//    - create_with_auto_functions() - auto-function loop requires storage
//    - create_stream_with_auto_functions() - auto-function loop requires storage

#[tokio::test]
async fn test_auto_functions_rejects_store_disabled() {
    // Auto-function execution requires storage to maintain conversation context
    let client = create_test_client();
    let func = FunctionDeclaration::builder("test_func")
        .with_description("Test function")
        .build();

    let result = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Test")
        .add_function(func)
        .with_store_disabled() // This should be rejected
        .create_with_auto_functions()
        .await;

    // Should fail with validation error, not API error
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string()
            .contains("create_with_auto_functions() requires storage"),
        "Expected auto-functions+store error, got: {}",
        err
    );
}

#[tokio::test]
async fn test_stream_auto_functions_rejects_store_disabled() {
    use futures_util::StreamExt;

    // Streaming auto-function execution also requires storage
    let client = create_test_client();
    let func = FunctionDeclaration::builder("test_func")
        .with_description("Test function")
        .build();

    let mut stream = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Test")
        .add_function(func)
        .with_store_disabled() // This should be rejected
        .create_stream_with_auto_functions();

    // Should return an error stream immediately
    let first_item = stream.next().await;
    assert!(first_item.is_some(), "Stream should have at least one item");
    let result = first_item.unwrap();
    assert!(result.is_err(), "First stream item should be an error");
    let err = result.unwrap_err();
    assert!(
        err.to_string()
            .contains("create_with_auto_functions() requires storage"),
        "Expected auto-functions+store error, got: {}",
        err
    );
}

#[tokio::test]
async fn test_auto_functions_allows_store_true() {
    // This test verifies that store=true (explicit) doesn't trigger the validation error.
    // The actual API call will fail (invalid key), but validation should pass.
    let client = create_test_client();
    let func = FunctionDeclaration::builder("test_func")
        .with_description("Test function")
        .build();

    let result = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Test")
        .add_function(func)
        .with_store_enabled() // Explicitly true
        .create_with_auto_functions()
        .await;

    // Should fail with API error (invalid key), not InvalidInput validation error
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        !matches!(err, crate::GenaiError::InvalidInput(_)),
        "Should not be an InvalidInput error (validation passed), got: {:?}",
        err
    );
}

#[tokio::test]
async fn test_auto_functions_allows_store_default() {
    // This test verifies that store=None (default) doesn't trigger the validation error.
    // The actual API call will fail (invalid key), but validation should pass.
    let client = create_test_client();
    let func = FunctionDeclaration::builder("test_func")
        .with_description("Test function")
        .build();

    let result = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Test")
        .add_function(func)
        // No .with_store() call - uses default (None, which means true on server)
        .create_with_auto_functions()
        .await;

    // Should fail with API error (invalid key), not InvalidInput validation error
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        !matches!(err, crate::GenaiError::InvalidInput(_)),
        "Should not be an InvalidInput error (validation passed), got: {:?}",
        err
    );
}

#[test]
fn test_builder_deep_research_config_new_fields() {
    use crate::Visualization;

    let client = create_test_client();
    let request = client
        .interaction()
        .with_agent(crate::DEFAULT_DEEP_RESEARCH_AGENT)
        .with_text("Research something")
        .with_agent_config(
            DeepResearchConfig::new()
                .with_thinking_summaries(ThinkingSummaries::Auto)
                .with_visualization(Visualization::Auto)
                .with_collaborative_planning(true)
                .with_bigquery_tool(false),
        )
        .build()
        .unwrap();

    let value = serde_json::to_value(&request).unwrap();
    let config = &value["agent_config"];
    assert_eq!(config["type"], "deep-research");
    assert_eq!(config["thinking_summaries"], "auto");
    assert_eq!(config["visualization"], "auto");
    assert_eq!(config["collaborative_planning"], true);
    assert_eq!(config["enable_bigquery_tool"], false);
}

#[test]
fn test_builder_request_roundtrip_with_new_fields() {
    use crate::{EnvironmentSpec, ResponseFormat, WebhookConfig};

    let client = create_test_client();
    let request = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Hello")
        .with_webhook_config(WebhookConfig::new().with_uris(vec!["https://x.example".into()]))
        .with_environment("env-1")
        .with_response_format(ResponseFormat::json_schema(json!({"type": "object"})))
        .build()
        .unwrap();

    let json = serde_json::to_string(&request).unwrap();
    let back: InteractionRequest = serde_json::from_str(&json).unwrap();
    assert!(matches!(
        back.environment,
        Some(EnvironmentSpec::Id(ref id)) if id == "env-1"
    ));
    assert!(back.webhook_config.is_some());
    assert!(back.response_format.is_some());
}
