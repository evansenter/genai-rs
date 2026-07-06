//! Unit tests for InteractionBuilder.

use super::*;
use crate::Tool;
use crate::{
    Client, FileSearchConfig, FunctionDeclaration, ImageAspectRatio, ImageConfig, ImageSize,
    McpServerConfig,
};
use serde_json::json;

fn create_test_client() -> Client {
    Client::builder("test-api-key".to_string())
        .build()
        .expect("test client should build")
}

#[test]
fn test_function_declaration_builder() {
    let func_decl = FunctionDeclaration::builder("my_func")
        .description("Does something")
        .parameter("arg1", json!({"type": "string"}))
        .required(vec!["arg1".to_string()])
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
        .description("Test function")
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
        .with_model("gemini-3-flash-preview")
        .with_text("Hello");

    assert_eq!(builder.model.as_deref(), Some("gemini-3-flash-preview"));
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
        .with_model("gemini-3-flash-preview")
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
        .with_model("gemini-3-flash-preview")
        .with_text("Hello")
        .with_system_instruction("You are a helpful assistant");

    // System instruction is a plain string per the 2026-05-20 API revision
    assert_eq!(
        builder.system_instruction.as_deref(),
        Some("You are a helpful assistant")
    );
}

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
        .with_model("gemini-3-flash-preview")
        .with_text("Hello")
        .with_generation_config(config.clone());

    assert!(builder.generation_config.is_some());
    assert_eq!(
        builder.generation_config.as_ref().unwrap().temperature,
        Some(0.7)
    );
}

#[test]
fn test_interaction_builder_with_function() {
    let client = create_test_client();
    let func = FunctionDeclaration::builder("test_func")
        .description("Test function")
        .parameter("location", json!({"type": "string"}))
        .required(vec!["location".to_string()])
        .build();

    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Call a function")
        .add_function(func);

    assert!(builder.tools.is_some());
    assert_eq!(builder.tools.as_ref().unwrap().len(), 1);
}

#[test]
fn test_interaction_builder_add_mcp_server() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Use MCP server")
        .add_tool(McpServerConfig::new(
            "my-server",
            "https://mcp.example.com/api",
        ));

    assert!(builder.tools.is_some());
    let tools = builder.tools.as_ref().unwrap();
    assert_eq!(tools.len(), 1);

    match &tools[0] {
        Tool::McpServer { name, url, .. } => {
            assert_eq!(name, "my-server");
            assert_eq!(url, "https://mcp.example.com/api");
        }
        _ => panic!("Expected Tool::McpServer variant"),
    }
}

#[test]
fn test_interaction_builder_with_multiple_mcp_servers() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Use multiple MCP servers")
        .add_tool(McpServerConfig::new("server-1", "https://mcp1.example.com"))
        .add_tool(McpServerConfig::new("server-2", "https://mcp2.example.com"));

    assert!(builder.tools.is_some());
    let tools = builder.tools.as_ref().unwrap();
    assert_eq!(tools.len(), 2);
}

#[test]
fn test_interaction_builder_add_mcp_server_and_other_tools() {
    let client = create_test_client();
    let func = FunctionDeclaration::builder("test_func")
        .description("Test function")
        .build();

    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Use MCP and other tools")
        .add_tool(McpServerConfig::new("my-server", "https://mcp.example.com"))
        .with_google_search()
        .add_function(func);

    assert!(builder.tools.is_some());
    let tools = builder.tools.as_ref().unwrap();
    assert_eq!(tools.len(), 3);
}

#[test]
fn test_interaction_builder_with_google_maps() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Find coffee shops")
        .with_google_maps();

    let tools = builder.tools.as_ref().unwrap();
    assert_eq!(tools.len(), 1);
    assert!(matches!(
        tools[0],
        Tool::GoogleMaps {
            enable_widget: None,
            latitude: None,
            longitude: None,
        }
    ));
}

#[test]
fn test_interaction_builder_add_tool_with_configs() {
    use crate::{ComputerUseConfig, FileSearchConfig, GoogleMapsConfig, GoogleSearchConfig};

    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Test all configs")
        .add_tool(GoogleSearchConfig::new().with_search_types(vec![crate::SearchType::WebSearch]))
        .add_tool(GoogleMapsConfig::new().with_widget())
        .add_tool(ComputerUseConfig::new().excluding(vec!["download".to_string()]))
        .add_tool(FileSearchConfig::new(vec!["store".to_string()]).with_top_k(5));

    let tools = builder.tools.as_ref().unwrap();
    assert_eq!(tools.len(), 4);
    assert!(matches!(tools[0], Tool::GoogleSearch { .. }));
    assert!(matches!(
        tools[1],
        Tool::GoogleMaps {
            enable_widget: Some(true),
            ..
        }
    ));
    assert!(matches!(tools[2], Tool::ComputerUse { .. }));
    assert!(matches!(tools[3], Tool::FileSearch { .. }));
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
        .with_model("gemini-3-flash-preview")
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
        .with_model("gemini-3-flash-preview")
        .with_text("Stored interaction")
        .with_store_enabled();

    assert_eq!(builder.store, Some(true));
}

#[test]
fn test_interaction_builder_build_success() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Hello");

    let result = builder.build();
    assert!(result.is_ok());

    let request = result.unwrap();
    assert_eq!(request.model.as_deref(), Some("gemini-3-flash-preview"));
    assert!(matches!(request.input, crate::InteractionInput::Text(_)));
}

#[test]
fn test_interaction_builder_build_missing_input() {
    let client = create_test_client();
    let builder = client.interaction().with_model("gemini-3-flash-preview");

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

#[test]
fn test_interaction_builder_with_response_modalities() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Generate an image")
        .with_response_modalities(vec!["IMAGE".to_string()]);

    // Modalities are normalized to lowercase - the API is case-sensitive and
    // rejects uppercase values (verified live).
    assert_eq!(
        builder.response_modalities.as_ref().unwrap(),
        &vec!["image".to_string()]
    );
}

#[test]
fn test_interaction_builder_with_max_function_call_loops() {
    let client = create_test_client();

    // Test default value
    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Test");
    assert_eq!(
        builder.max_function_call_loops,
        super::auto_functions::DEFAULT_MAX_FUNCTION_CALL_LOOPS
    );

    // Test custom value
    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Test")
        .with_max_function_call_loops(10);
    assert_eq!(builder.max_function_call_loops, 10);

    // Test setting to minimum (1)
    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Test")
        .with_max_function_call_loops(1);
    assert_eq!(builder.max_function_call_loops, 1);
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
        .with_model("gemini-3-flash-preview")
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
        .with_model("gemini-3-flash-preview")
        .with_text("Hello")
        .with_system_instruction("Be helpful")
        .with_previous_interaction("prev-123");

    assert_eq!(builder.model.as_deref(), Some("gemini-3-flash-preview"));
    assert!(builder.system_instruction.is_some());
    assert_eq!(builder.previous_interaction_id.as_deref(), Some("prev-123"));
}

#[test]
fn test_builder_store_disabled_sets_store_false() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
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
        .with_model("gemini-3-flash-preview")
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
        .with_model("gemini-3-flash-preview")
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
        .with_model("gemini-3-flash-preview")
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
        .with_model("gemini-3-flash-preview")
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
        .description("Test function")
        .build();

    let result = client
        .interaction()
        .with_model("gemini-3-flash-preview")
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
        .description("Test function")
        .build();

    let mut stream = client
        .interaction()
        .with_model("gemini-3-flash-preview")
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
        .description("Test function")
        .build();

    let result = client
        .interaction()
        .with_model("gemini-3-flash-preview")
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
        .description("Test function")
        .build();

    let result = client
        .interaction()
        .with_model("gemini-3-flash-preview")
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

// --- Step Array Input Tests ---

#[test]
fn test_interaction_builder_with_history() {
    use crate::Step;

    let client = create_test_client();
    let steps = vec![
        Step::user_text("What is 2+2?"),
        Step::model_text("2+2 equals 4."),
        Step::user_text("And what's that times 3?"),
    ];

    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_history(steps);

    assert_eq!(builder.history.len(), 3);
}

#[test]
fn test_interaction_builder_build_with_history() {
    use crate::Step;

    let client = create_test_client();
    let steps = vec![
        Step::user_text("Hello"),
        Step::model_text("Hi there!"),
        Step::user_text("How are you?"),
    ];

    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_history(steps);

    let result = builder.build();
    assert!(result.is_ok());

    let request = result.unwrap();
    assert_eq!(request.model.as_deref(), Some("gemini-3-flash-preview"));
    assert!(matches!(request.input, crate::InteractionInput::Steps(_)));
}

#[test]
fn test_interaction_builder_with_single_step() {
    use crate::Step;

    let client = create_test_client();
    let steps = vec![Step::user_text("Hello")];

    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_history(steps);

    let result = builder.build();
    assert!(result.is_ok());
}

// --- History + Current Message Composition Tests ---

#[test]
fn test_with_history_then_with_text_composes_correctly() {
    use crate::{InteractionInput, Step};

    let client = create_test_client();
    let history = vec![Step::user_text("Hello"), Step::model_text("Hi there!")];

    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_history(history)
        .with_text("How are you?");

    // Build should compose history + current_message
    let request = builder.build().expect("Build should succeed");

    // Verify the input is Steps with 3 items
    match &request.input {
        InteractionInput::Steps(steps) => {
            assert_eq!(steps.len(), 3, "Should have 3 steps");
            assert!(matches!(steps[0], Step::UserInput { .. }));
            assert!(matches!(steps[1], Step::ModelOutput { .. }));
            assert!(matches!(steps[2], Step::UserInput { .. }));
        }
        _ => panic!("Expected Steps input"),
    }
}

#[test]
fn test_with_text_then_with_history_composes_correctly() {
    use crate::{InteractionInput, Step};

    let client = create_test_client();
    let history = vec![Step::user_text("Hello"), Step::model_text("Hi there!")];

    // Order reversed - should produce same result
    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("How are you?")
        .with_history(history);

    let request = builder.build().expect("Build should succeed");

    // Verify the input is Steps with 3 items (history + current)
    match &request.input {
        InteractionInput::Steps(steps) => {
            assert_eq!(steps.len(), 3, "Should have 3 steps");
            assert!(matches!(steps[0], Step::UserInput { .. }));
            assert!(matches!(steps[1], Step::ModelOutput { .. }));
            assert!(matches!(steps[2], Step::UserInput { .. })); // Current message appended
        }
        _ => panic!("Expected Steps input"),
    }
}

#[test]
fn test_history_and_text_order_independent() {
    use crate::Step;

    let client = create_test_client();
    let history = vec![Step::user_text("First"), Step::model_text("Response")];

    // Build in one order
    let req1 = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_history(history.clone())
        .with_text("Current")
        .build()
        .expect("Build should succeed");

    // Build in reverse order
    let req2 = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Current")
        .with_history(history)
        .build()
        .expect("Build should succeed");

    // Both should produce equivalent requests
    let json1 = serde_json::to_string(&req1.input).unwrap();
    let json2 = serde_json::to_string(&req2.input).unwrap();
    assert_eq!(json1, json2, "Order should not affect result");
}

#[test]
fn test_conversation_builder_then_with_text() {
    use crate::{InteractionInput, Step};

    let client = create_test_client();

    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .conversation()
        .user("What is 2+2?")
        .model("4")
        .done()
        .with_text("And times 3?");

    let request = builder.build().expect("Build should succeed");

    // Should have 3 steps: original 2 + appended current message
    match &request.input {
        InteractionInput::Steps(steps) => {
            assert_eq!(steps.len(), 3, "Should have 3 steps");
            assert!(matches!(steps[0], Step::UserInput { .. }));
            assert!(matches!(steps[1], Step::ModelOutput { .. }));
            assert!(matches!(steps[2], Step::UserInput { .. }));
        }
        _ => panic!("Expected Steps input"),
    }
}

#[test]
fn test_with_text_only_produces_text_input() {
    use crate::InteractionInput;

    let client = create_test_client();

    let request = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Hello!")
        .build()
        .expect("Build should succeed");

    // Should produce Text input when no history
    assert!(
        matches!(request.input, InteractionInput::Text(_)),
        "Expected Text input, got {:?}",
        request.input
    );
}

#[test]
fn test_with_history_only_produces_steps_input() {
    use crate::{InteractionInput, Step};

    let client = create_test_client();
    let history = vec![Step::user_text("Hello"), Step::model_text("Hi!")];

    let request = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_history(history)
        .build()
        .expect("Build should succeed");

    // Should produce Steps input
    match &request.input {
        InteractionInput::Steps(steps) => {
            assert_eq!(steps.len(), 2);
        }
        _ => panic!("Expected Steps input"),
    }
}

#[test]
fn test_chained_preserves_history_and_current_message() {
    use crate::Step;

    let client = create_test_client();
    let history = vec![Step::user_text("Hello"), Step::model_text("Hi!")];

    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_history(history)
        .with_text("Current message")
        .with_previous_interaction("prev-123");

    // Fields should be preserved when chaining methods
    assert_eq!(builder.history.len(), 2);
    assert_eq!(builder.current_message.as_deref(), Some("Current message"));
    assert_eq!(builder.previous_interaction_id.as_deref(), Some("prev-123"));
}

#[test]
fn test_store_disabled_preserves_history_and_current_message() {
    use crate::Step;

    let client = create_test_client();
    let history = vec![Step::user_text("Hello"), Step::model_text("Hi!")];

    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_history(history)
        .with_text("Current message")
        .with_store_disabled();

    // Fields should be preserved when setting store disabled
    assert_eq!(builder.history.len(), 2);
    assert_eq!(builder.current_message.as_deref(), Some("Current message"));
    assert_eq!(builder.store, Some(false));
}

#[test]
fn test_with_content_cannot_combine_with_history() {
    use crate::{Content, Step};

    let client = create_test_client();
    let history = vec![Step::user_text("Hello"), Step::model_text("Hi!")];
    let content = vec![Content::Text {
        text: Some("test".to_string()),
        annotations: None,
    }];

    let result = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_history(history)
        .with_content(content)
        .build();

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("with_content()"),
        "Error should mention with_content(): {}",
        err
    );
}

#[test]
fn test_with_content_and_text_merge() {
    use crate::{Content, InteractionInput};

    let client = create_test_client();
    let image_content = Content::Image {
        data: Some("dGVzdA==".to_string()),
        uri: None,
        mime_type: Some("image/png".to_string()),
        resolution: None,
    };

    // with_text() then with_content() - text should be prepended to content
    let request = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Describe this image")
        .with_content(vec![image_content.clone()])
        .build()
        .expect("Should succeed");

    match &request.input {
        InteractionInput::Content(items) => {
            assert_eq!(items.len(), 2, "Should have 2 items (text + image)");
            // Text should be first (prepended)
            assert!(
                matches!(&items[0], Content::Text { text: Some(t), .. } if t == "Describe this image"),
                "First item should be the text"
            );
            assert!(
                matches!(&items[1], Content::Image { .. }),
                "Second item should be the image"
            );
        }
        _ => panic!("Expected Content input"),
    }

    // with_content() then with_text() - should also work (order-independent)
    let request2 = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_content(vec![image_content])
        .with_text("Describe this image")
        .build()
        .expect("Should succeed");

    match &request2.input {
        InteractionInput::Content(items) => {
            assert_eq!(items.len(), 2, "Should have 2 items (text + image)");
            // Text should still be first (prepended at build time)
            assert!(
                matches!(&items[0], Content::Text { text: Some(t), .. } if t == "Describe this image"),
                "First item should be the text"
            );
        }
        _ => panic!("Expected Content input"),
    }

    // Verify order-independence: both orders produce identical serialized output
    let json1 = serde_json::to_string(&request.input).unwrap();
    let json2 = serde_json::to_string(&request2.input).unwrap();
    assert_eq!(json1, json2, "Builder order should not affect result");
}

#[test]
fn test_with_content_alone_works() {
    use crate::{Content, InteractionInput};

    let client = create_test_client();
    let content = vec![Content::Text {
        text: Some("test".to_string()),
        annotations: None,
    }];

    let result = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_content(content)
        .build();

    assert!(result.is_ok());
    let request = result.unwrap();
    assert!(matches!(request.input, InteractionInput::Content(_)));
}

#[test]
fn test_conversation_builder_fluent_api() {
    use crate::Step;

    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .conversation()
        .user("What is 2+2?")
        .model("2+2 equals 4.")
        .user("And what's that times 3?")
        .done();

    // Verify the history has correct length and step types
    assert_eq!(builder.history.len(), 3);
    assert!(matches!(builder.history[0], Step::UserInput { .. }));
    assert!(matches!(builder.history[1], Step::ModelOutput { .. }));
    assert!(matches!(builder.history[2], Step::UserInput { .. }));
}

#[test]
fn test_conversation_builder_with_parts_content() {
    use crate::{Content, Step, TurnContent};

    let client = create_test_client();
    let parts = vec![Content::Text {
        text: Some("What is in this image?".to_string()),
        annotations: None,
    }];

    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .conversation()
        .user(TurnContent::Parts(parts))
        .done();

    // Verify the history has 1 user_input step wrapping the parts
    assert_eq!(builder.history.len(), 1);
    assert!(matches!(builder.history[0], Step::UserInput { .. }));
    let content = builder.history[0]
        .content()
        .expect("user_input step should expose content");
    assert_eq!(content.len(), 1);
    assert_eq!(content[0].as_text(), Some("What is in this image?"));
}

#[test]
fn test_conversation_builder_with_turn_method() {
    use crate::Role;

    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .conversation()
        .turn(Role::User, "Hello")
        .turn(Role::Model, "Hi!")
        .done();

    assert_eq!(builder.history.len(), 2);
}

#[test]
fn test_conversation_builder_empty() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .conversation()
        .done();

    // Empty conversation results in empty history
    assert!(builder.history.is_empty());
}

#[test]
fn test_conversation_builder_preserves_model() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .conversation()
        .user("Hello")
        .done();

    // Model should be preserved through conversation builder
    assert_eq!(builder.model.as_deref(), Some("gemini-3-flash-preview"));
}

#[test]
fn test_conversation_builder_preserves_system_instruction() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_system_instruction("Be helpful")
        .conversation()
        .user("Hello")
        .done();

    // System instruction should be preserved through conversation builder
    assert!(builder.system_instruction.is_some());
}

// --- File Search Builder Tests ---

#[test]
fn test_interaction_builder_with_file_search() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Search my documents")
        .add_tool(FileSearchConfig::new(vec![
            "stores/store-123".to_string(),
            "stores/store-456".to_string(),
        ]));

    assert!(builder.tools.is_some());
    let tools = builder.tools.as_ref().unwrap();
    assert_eq!(tools.len(), 1);

    match &tools[0] {
        Tool::FileSearch {
            store_names,
            top_k,
            metadata_filter,
        } => {
            assert_eq!(store_names, &vec!["stores/store-123", "stores/store-456"]);
            assert_eq!(*top_k, None);
            assert_eq!(*metadata_filter, None);
        }
        _ => panic!("Expected Tool::FileSearch variant"),
    }
}

#[test]
fn test_interaction_builder_with_file_search_config() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Search with config")
        .add_tool(
            FileSearchConfig::new(vec!["stores/my-docs".to_string()])
                .with_top_k(10)
                .with_metadata_filter("category = 'technical'"),
        );

    assert!(builder.tools.is_some());
    let tools = builder.tools.as_ref().unwrap();
    assert_eq!(tools.len(), 1);

    match &tools[0] {
        Tool::FileSearch {
            store_names,
            top_k,
            metadata_filter,
        } => {
            assert_eq!(store_names, &vec!["stores/my-docs"]);
            assert_eq!(*top_k, Some(10));
            assert_eq!(*metadata_filter, Some("category = 'technical'".to_string()));
        }
        _ => panic!("Expected Tool::FileSearch variant"),
    }
}

#[test]
fn test_interaction_builder_with_file_search_and_other_tools() {
    let client = create_test_client();
    let func = FunctionDeclaration::builder("process_result")
        .description("Process search result")
        .build();

    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Search and process")
        .add_tool(FileSearchConfig::new(vec!["stores/docs".to_string()]))
        .with_google_search()
        .add_function(func);

    assert!(builder.tools.is_some());
    let tools = builder.tools.as_ref().unwrap();
    assert_eq!(tools.len(), 3);

    // Verify FileSearch is present
    assert!(tools.iter().any(|t| matches!(t, Tool::FileSearch { .. })));
    // Verify GoogleSearch is present
    assert!(tools.iter().any(|t| matches!(t, Tool::GoogleSearch { .. })));
    // Verify Function is present
    assert!(tools.iter().any(|t| matches!(t, Tool::Function { .. })));
}

#[test]
fn test_interaction_builder_with_file_search_single_store() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Search single store")
        .add_tool(FileSearchConfig::new(vec!["stores/single".to_string()]));

    let tools = builder.tools.as_ref().unwrap();
    match &tools[0] {
        Tool::FileSearch { store_names, .. } => {
            assert_eq!(store_names.len(), 1);
            assert_eq!(store_names[0], "stores/single");
        }
        _ => panic!("Expected Tool::FileSearch variant"),
    }
}

#[test]
fn test_with_image_config() {
    let client = create_test_client();
    let config = ImageConfig {
        aspect_ratio: Some(ImageAspectRatio::Widescreen16x9),
        image_size: Some(ImageSize::Hd2k),
    };

    let builder = client
        .interaction()
        .with_model("gemini-3-pro-image-preview")
        .with_text("Generate a landscape")
        .with_image_config(config);

    let gen_config = builder.generation_config.as_ref().unwrap();
    let image_config = gen_config.image_config.as_ref().unwrap();
    assert_eq!(
        image_config.aspect_ratio,
        Some(ImageAspectRatio::Widescreen16x9)
    );
    assert_eq!(image_config.image_size, Some(ImageSize::Hd2k));
}

#[test]
fn test_with_image_config_merges_with_existing_generation_config() {
    let client = create_test_client();
    let config = ImageConfig {
        aspect_ratio: Some(ImageAspectRatio::Square),
        image_size: None,
    };

    let builder = client
        .interaction()
        .with_model("gemini-3-pro-image-preview")
        .with_text("Generate an image")
        .with_thinking_level(crate::ThinkingLevel::Low)
        .with_image_config(config);

    let gen_config = builder.generation_config.as_ref().unwrap();
    // Both thinking_level and image_config should be present
    assert!(gen_config.thinking_level.is_some());
    assert!(gen_config.image_config.is_some());
    assert_eq!(
        gen_config.image_config.as_ref().unwrap().aspect_ratio,
        Some(ImageAspectRatio::Square)
    );
}

#[test]
fn test_interaction_builder_with_allowed_tools() {
    use crate::ToolChoice;

    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Get weather")
        .with_allowed_tools(vec!["get_weather".to_string(), "get_time".to_string()]);

    // with_allowed_tools() now populates generation_config.tool_choice
    let config = builder.generation_config.as_ref().unwrap();
    assert_eq!(
        config.tool_choice,
        Some(ToolChoice::allowed_tools(
            None,
            vec!["get_weather".to_string(), "get_time".to_string()]
        ))
    );
}

#[test]
fn test_interaction_builder_with_allowed_tools_preserves_mode() {
    use crate::{FunctionCallingMode, ToolChoice};

    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Get weather")
        .with_tool_choice(ToolChoice::Mode(FunctionCallingMode::Any))
        .with_allowed_tools(vec!["get_weather".to_string()]);

    // A previously-set mode is preserved when upgrading to the object form
    let config = builder.generation_config.as_ref().unwrap();
    assert_eq!(
        config.tool_choice,
        Some(ToolChoice::allowed_tools(
            Some(FunctionCallingMode::Any),
            vec!["get_weather".to_string()]
        ))
    );
}

// ============================================================================
// Webhook config / environment / response format / speech / video builders
// ============================================================================

#[test]
fn test_builder_with_webhook_config() {
    use crate::WebhookConfig;

    let client = create_test_client();
    let request = client
        .interaction()
        .with_model("gemini-3-flash-preview")
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
        .with_model("gemini-3-flash-preview")
        .with_text("Hello")
        .with_environment("environments/env-123")
        .build()
        .unwrap();

    let value = serde_json::to_value(&request).unwrap();
    assert_eq!(value["environment"], "environments/env-123");
}

#[test]
fn test_builder_with_typed_remote_environment() {
    use crate::{AllowlistEntry, EnvironmentSource, NetworkConfig, RemoteEnvironment};

    let client = create_test_client();
    let request = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Hello")
        .with_environment(
            RemoteEnvironment::new()
                .add_source(EnvironmentSource::gcs("gs://bucket", "/data"))
                .with_network(NetworkConfig::Allowlist(vec![AllowlistEntry::new(
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
fn test_builder_with_response_format_raw_schema_maps_to_text() {
    // Backward compatibility: raw serde_json::Value schemas keep working and
    // now serialize inside the typed text format.
    let client = create_test_client();
    let request = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Generate data")
        .with_response_format(json!({
            "type": "object",
            "properties": {"name": {"type": "string"}}
        }))
        .build()
        .unwrap();

    let value = serde_json::to_value(&request).unwrap();
    assert_eq!(value["response_format"]["type"], "text");
    assert_eq!(value["response_format"]["mime_type"], "application/json");
    assert_eq!(
        value["response_format"]["schema"]["properties"]["name"]["type"],
        "string"
    );
}

#[test]
fn test_builder_with_response_format_typed_variant() {
    use crate::{ResponseDelivery, ResponseFormat};

    let client = create_test_client();
    let request = client
        .interaction()
        .with_model("gemini-2.5-pro-preview-tts")
        .with_text("Read aloud")
        .with_response_format(ResponseFormat::Audio {
            mime_type: Some("audio/mp3".to_string()),
            delivery: Some(ResponseDelivery::Inline),
            sample_rate: Some(24000),
            bit_rate: None,
        })
        .build()
        .unwrap();

    let value = serde_json::to_value(&request).unwrap();
    assert_eq!(value["response_format"]["type"], "audio");
    assert_eq!(value["response_format"]["mime_type"], "audio/mp3");
    assert_eq!(value["response_format"]["delivery"], "inline");
    assert_eq!(value["response_format"]["sample_rate"], 24000);
}

#[test]
fn test_builder_with_response_formats_list() {
    use crate::ResponseFormat;

    let client = create_test_client();
    let request = client
        .interaction()
        .with_model("gemini-3-pro-image-preview")
        .with_text("A diagram")
        .with_response_formats(vec![
            ResponseFormat::text_plain(),
            ResponseFormat::Image {
                mime_type: Some("image/jpeg".to_string()),
                delivery: None,
                aspect_ratio: None,
                image_size: None,
            },
        ])
        .build()
        .unwrap();

    let value = serde_json::to_value(&request).unwrap();
    let formats = value["response_format"].as_array().expect("list form");
    assert_eq!(formats.len(), 2);
    assert_eq!(formats[0]["type"], "text");
    assert_eq!(formats[1]["type"], "image");
}

#[test]
fn test_builder_with_speech_config_serializes_as_single_entry_list() {
    let client = create_test_client();
    let request = client
        .interaction()
        .with_model("gemini-2.5-pro-preview-tts")
        .with_text("Hello")
        .with_speech_config(SpeechConfig::with_voice_and_language("Kore", "en-US"))
        .build()
        .unwrap();

    let value = serde_json::to_value(&request).unwrap();
    let speech = value["generation_config"]["speech_config"]
        .as_array()
        .expect("speech_config must be a list on the wire");
    assert_eq!(speech.len(), 1);
    assert_eq!(speech[0]["voice"], "Kore");
    assert_eq!(speech[0]["language"], "en-US");
}

#[test]
fn test_builder_multi_speaker_speech_configs() {
    let client = create_test_client();
    let request = client
        .interaction()
        .with_model("gemini-2.5-pro-preview-tts")
        .with_text("Alice: hi\nBob: hey")
        .add_speech_config(SpeechConfig {
            voice: Some("Kore".to_string()),
            language: Some("en-US".to_string()),
            speaker: Some("Alice".to_string()),
        })
        .add_speech_config(SpeechConfig {
            voice: Some("Puck".to_string()),
            language: Some("en-US".to_string()),
            speaker: Some("Bob".to_string()),
        })
        .build()
        .unwrap();

    let value = serde_json::to_value(&request).unwrap();
    let speech = value["generation_config"]["speech_config"]
        .as_array()
        .expect("speech_config must be a list on the wire");
    assert_eq!(speech.len(), 2);
    assert_eq!(speech[0]["speaker"], "Alice");
    assert_eq!(speech[1]["speaker"], "Bob");
}

#[test]
fn test_builder_with_speech_configs_replaces() {
    let client = create_test_client();
    let request = client
        .interaction()
        .with_model("gemini-2.5-pro-preview-tts")
        .with_text("Hello")
        .add_speech_config(SpeechConfig::with_voice("Kore"))
        .with_speech_configs(vec![SpeechConfig::with_voice("Puck")])
        .build()
        .unwrap();

    let value = serde_json::to_value(&request).unwrap();
    let speech = value["generation_config"]["speech_config"]
        .as_array()
        .unwrap();
    assert_eq!(speech.len(), 1, "with_speech_configs replaces");
    assert_eq!(speech[0]["voice"], "Puck");
}

#[test]
fn test_builder_with_video_config_and_output() {
    use crate::{VideoConfig, VideoTask};

    let client = create_test_client();
    let request = client
        .interaction()
        .with_model("veo-3.1-generate-preview")
        .with_text("A hummingbird in slow motion")
        .with_video_output()
        .with_video_config(VideoConfig::new().with_task(VideoTask::TextToVideo))
        .build()
        .unwrap();

    let value = serde_json::to_value(&request).unwrap();
    assert_eq!(value["response_modalities"][0], "video");
    assert_eq!(
        value["generation_config"]["video_config"]["task"],
        "text_to_video"
    );
}

#[test]
fn test_builder_retrieval_tool() {
    use crate::{RetrievalConfig, VertexAiSearchConfig};

    let client = create_test_client();
    let request = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Search internal docs")
        .add_tool(
            RetrievalConfig::new()
                .with_vertex_ai_search(VertexAiSearchConfig::new().with_engine("engines/e")),
        )
        .build()
        .unwrap();

    let value = serde_json::to_value(&request).unwrap();
    assert_eq!(value["tools"][0]["type"], "retrieval");
    assert_eq!(value["tools"][0]["retrieval_types"][0], "vertex_ai_search");
    assert_eq!(
        value["tools"][0]["vertex_ai_search_config"]["engine"],
        "engines/e"
    );
}

#[test]
fn test_builder_deep_research_config_new_fields() {
    use crate::Visualization;

    let client = create_test_client();
    let request = client
        .interaction()
        .with_agent("deep-research-preview-04-2026")
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
    assert_eq!(config["thinking_summaries"], "THINKING_SUMMARIES_AUTO");
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
        .with_model("gemini-3-flash-preview")
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
