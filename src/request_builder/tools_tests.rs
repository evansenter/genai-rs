//! Unit tests for the builder's tool setters: functions, MCP servers, built-in
//! tools, file search, retrieval, allowed tools and the function-call loop cap.

use super::super::tests::create_test_client;
use super::*;
use crate::{FileSearchConfig, McpServerConfig, Tool};
use serde_json::json;

#[test]
fn test_interaction_builder_with_function() {
    let client = create_test_client();
    let func = FunctionDeclaration::builder("test_func")
        .with_description("Test function")
        .add_parameter("location", json!({"type": "string"}))
        .with_required(vec!["location".to_string()])
        .build();

    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Call a function")
        .add_function(func);

    assert!(builder.tools.is_some());
    assert_eq!(builder.tools.as_ref().unwrap().len(), 1);
}

#[test]
fn test_add_functions_takes_any_iterator_of_declarations() {
    let client = create_test_client();
    let declaration = |name: &str| FunctionDeclaration::builder(name).build();
    // An array, and a mapped iterator, with no `Vec` in between (#504).
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .add_functions([declaration("a"), declaration("b")])
        .add_functions(["c", "d"].into_iter().map(declaration));

    let names: Vec<&str> = builder
        .tools
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter_map(|tool| match tool {
            Tool::Function { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(names, ["a", "b", "c", "d"]);
}

#[test]
fn test_interaction_builder_add_mcp_server() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
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
        .with_model(crate::DEFAULT_MODEL)
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
        .with_description("Test function")
        .build();

    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
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
        .with_model(crate::DEFAULT_MODEL)
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
            ..
        }
    ));
}

#[test]
fn test_built_in_tool_setters_replace_instead_of_duplicating() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Test")
        .with_google_search()
        .with_code_execution()
        .with_url_context()
        .with_google_maps()
        .with_google_search()
        .with_code_execution()
        .with_url_context()
        .with_google_maps();

    let tools = builder.tools.as_ref().unwrap();
    assert_eq!(tools.len(), 4, "one of each kind: {tools:?}");
    let kinds: [fn(&Tool) -> bool; 4] = [
        |t| matches!(t, Tool::GoogleSearch { .. }),
        |t| matches!(t, Tool::CodeExecution { .. }),
        |t| matches!(t, Tool::UrlContext { .. }),
        |t| matches!(t, Tool::GoogleMaps { .. }),
    ];
    for kind in kinds {
        assert_eq!(tools.iter().filter(|t| kind(t)).count(), 1, "{tools:?}");
    }
}

#[test]
fn test_with_google_search_replaces_a_configured_search_tool() {
    use crate::GoogleSearchConfig;

    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Test")
        .add_tool(GoogleSearchConfig::new().with_search_types(vec![crate::SearchType::WebSearch]))
        .add_function(FunctionDeclaration::builder("f").build())
        .with_google_search();

    let tools = builder.tools.as_ref().unwrap();
    assert_eq!(tools.len(), 2);
    assert!(matches!(tools[0], Tool::Function { .. }));
    assert!(matches!(
        tools[1],
        Tool::GoogleSearch {
            search_types: None,
            ..
        }
    ));
}

#[test]
fn test_add_tool_still_accumulates_same_kind() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Test")
        .add_tool(Tool::code_execution())
        .add_tool(Tool::code_execution());
    assert_eq!(builder.tools.as_ref().unwrap().len(), 2);
}

#[test]
fn test_with_tools_replaces_all_tools() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Test")
        .with_google_search()
        .with_tools(vec![Tool::url_context()]);
    assert_eq!(builder.tools, Some(vec![Tool::url_context()]));
}

#[test]
fn test_interaction_builder_add_tool_with_configs() {
    use crate::{ComputerUseConfig, FileSearchConfig, GoogleMapsConfig, GoogleSearchConfig};

    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Test all configs")
        .add_tool(GoogleSearchConfig::new().with_search_types(vec![crate::SearchType::WebSearch]))
        .add_tool(GoogleMapsConfig::new().with_widget())
        .add_tool(
            ComputerUseConfig::new()
                .with_excluded_predefined_functions(vec!["download".to_string()]),
        )
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
fn test_interaction_builder_with_max_function_call_loops() {
    let client = create_test_client();

    // Test default value
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Test");
    assert_eq!(
        builder.max_function_call_loops,
        super::super::auto_functions::DEFAULT_MAX_FUNCTION_CALL_LOOPS
    );

    // Test custom value
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Test")
        .with_max_function_call_loops(10);
    assert_eq!(builder.max_function_call_loops, 10);

    // Test setting to minimum (1)
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Test")
        .with_max_function_call_loops(1);
    assert_eq!(builder.max_function_call_loops, 1);
}

// --- File Search Builder Tests ---

#[test]
fn test_interaction_builder_with_file_search() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
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
            ..
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
        .with_model(crate::DEFAULT_MODEL)
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
            ..
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
        .with_description("Process search result")
        .build();

    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
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
        .with_model(crate::DEFAULT_MODEL)
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
fn test_interaction_builder_with_allowed_tools() {
    use crate::ToolChoice;

    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
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
        .with_model(crate::DEFAULT_MODEL)
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

#[test]
fn test_builder_retrieval_tool() {
    use crate::{RetrievalConfig, VertexAiSearchConfig};

    let client = create_test_client();
    let request = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
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
