//! Unit tests for InteractionBuilder.

use super::*;
use crate::Tool;
use crate::{Client, FileSearchConfig, FunctionDeclaration, McpServerConfig};
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

    assert!(matches!(
        builder.system_instruction,
        Some(crate::InteractionInput::Text(_))
    ));
}

#[test]
fn test_interaction_builder_with_generation_config() {
    let client = create_test_client();
    let config = crate::GenerationConfig {
        temperature: Some(0.7),
        max_output_tokens: Some(1000),
        top_p: Some(0.9),
        top_k: Some(40),
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
            enable_widget: None
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
            enable_widget: Some(true)
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

    assert_eq!(
        builder.response_modalities.as_ref().unwrap(),
        &vec!["IMAGE".to_string()]
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

// --- Turn Array Input Tests ---

#[test]
fn test_interaction_builder_with_history() {
    use crate::Turn;

    let client = create_test_client();
    let turns = vec![
        Turn::user("What is 2+2?"),
        Turn::model("2+2 equals 4."),
        Turn::user("And what's that times 3?"),
    ];

    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_history(turns);

    assert_eq!(builder.history.len(), 3);
}

#[test]
fn test_interaction_builder_build_with_history() {
    use crate::Turn;

    let client = create_test_client();
    let turns = vec![
        Turn::user("Hello"),
        Turn::model("Hi there!"),
        Turn::user("How are you?"),
    ];

    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_history(turns);

    let result = builder.build();
    assert!(result.is_ok());

    let request = result.unwrap();
    assert_eq!(request.model.as_deref(), Some("gemini-3-flash-preview"));
    assert!(matches!(request.input, crate::InteractionInput::Turns(_)));
}

#[test]
fn test_interaction_builder_with_single_turn() {
    use crate::Turn;

    let client = create_test_client();
    let turns = vec![Turn::user("Hello")];

    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_history(turns);

    let result = builder.build();
    assert!(result.is_ok());
}

// --- History + Current Message Composition Tests ---

#[test]
fn test_with_history_then_with_text_composes_correctly() {
    use crate::{InteractionInput, Role, Turn};

    let client = create_test_client();
    let history = vec![Turn::user("Hello"), Turn::model("Hi there!")];

    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_history(history)
        .with_text("How are you?");

    // Build should compose history + current_message
    let request = builder.build().expect("Build should succeed");

    // Verify the input is Turns with 3 items
    match &request.input {
        InteractionInput::Turns(turns) => {
            assert_eq!(turns.len(), 3, "Should have 3 turns");
            assert_eq!(*turns[0].role(), Role::User);
            assert_eq!(*turns[1].role(), Role::Model);
            assert_eq!(*turns[2].role(), Role::User);
        }
        _ => panic!("Expected Turns input"),
    }
}

#[test]
fn test_with_text_then_with_history_composes_correctly() {
    use crate::{InteractionInput, Role, Turn};

    let client = create_test_client();
    let history = vec![Turn::user("Hello"), Turn::model("Hi there!")];

    // Order reversed - should produce same result
    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("How are you?")
        .with_history(history);

    let request = builder.build().expect("Build should succeed");

    // Verify the input is Turns with 3 items (history + current)
    match &request.input {
        InteractionInput::Turns(turns) => {
            assert_eq!(turns.len(), 3, "Should have 3 turns");
            assert_eq!(*turns[0].role(), Role::User);
            assert_eq!(*turns[1].role(), Role::Model);
            assert_eq!(*turns[2].role(), Role::User); // Current message appended
        }
        _ => panic!("Expected Turns input"),
    }
}

#[test]
fn test_history_and_text_order_independent() {
    use crate::Turn;

    let client = create_test_client();
    let history = vec![Turn::user("First"), Turn::model("Response")];

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
    use crate::{InteractionInput, Role};

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

    // Should have 3 turns: original 2 + appended current message
    match &request.input {
        InteractionInput::Turns(turns) => {
            assert_eq!(turns.len(), 3, "Should have 3 turns");
            assert_eq!(*turns[0].role(), Role::User);
            assert_eq!(*turns[1].role(), Role::Model);
            assert_eq!(*turns[2].role(), Role::User);
        }
        _ => panic!("Expected Turns input"),
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
fn test_with_history_only_produces_turns_input() {
    use crate::{InteractionInput, Turn};

    let client = create_test_client();
    let history = vec![Turn::user("Hello"), Turn::model("Hi!")];

    let request = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_history(history)
        .build()
        .expect("Build should succeed");

    // Should produce Turns input
    match &request.input {
        InteractionInput::Turns(turns) => {
            assert_eq!(turns.len(), 2);
        }
        _ => panic!("Expected Turns input"),
    }
}

#[test]
fn test_chained_preserves_history_and_current_message() {
    use crate::Turn;

    let client = create_test_client();
    let history = vec![Turn::user("Hello"), Turn::model("Hi!")];

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
    use crate::Turn;

    let client = create_test_client();
    let history = vec![Turn::user("Hello"), Turn::model("Hi!")];

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
    use crate::{Content, Turn};

    let client = create_test_client();
    let history = vec![Turn::user("Hello"), Turn::model("Hi!")];
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
    use crate::Role;

    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .conversation()
        .user("What is 2+2?")
        .model("2+2 equals 4.")
        .user("And what's that times 3?")
        .done();

    // Verify the history has correct length and roles
    assert_eq!(builder.history.len(), 3);
    assert_eq!(*builder.history[0].role(), Role::User);
    assert_eq!(*builder.history[1].role(), Role::Model);
    assert_eq!(*builder.history[2].role(), Role::User);
}

#[test]
fn test_conversation_builder_with_parts_content() {
    use crate::{Content, TurnContent};

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

    // Verify the history has 1 turn with parts content
    assert_eq!(builder.history.len(), 1);
    assert!(builder.history[0].content().is_parts());
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
