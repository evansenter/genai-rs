use super::*;
use serde_json;

#[test]
fn test_tool_google_search_roundtrip() {
    let tool = Tool::GoogleSearch { search_types: None };
    let json = serde_json::to_string(&tool).expect("Serialization failed");
    assert!(json.contains("\"type\":\"google_search\""));
    assert!(!json.contains("search_types"));

    let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");
    assert!(matches!(parsed, Tool::GoogleSearch { .. }));
}

#[test]
fn test_tool_google_search_with_search_types_roundtrip() {
    let tool = Tool::GoogleSearch {
        search_types: Some(vec![SearchType::WebSearch, SearchType::ImageSearch]),
    };
    let json = serde_json::to_string(&tool).expect("Serialization failed");
    assert!(json.contains("\"search_types\""));
    assert!(json.contains("\"web_search\""));
    assert!(json.contains("\"image_search\""));

    let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");
    match parsed {
        Tool::GoogleSearch { search_types } => {
            let types = search_types.expect("Should have search_types");
            assert_eq!(types.len(), 2);
            assert_eq!(types[0], SearchType::WebSearch);
            assert_eq!(types[1], SearchType::ImageSearch);
        }
        other => panic!("Expected GoogleSearch variant, got {:?}", other),
    }
}

#[test]
fn test_tool_google_maps_roundtrip() {
    let tool = Tool::GoogleMaps {
        enable_widget: None,
        latitude: None,
        longitude: None,
    };
    let json = serde_json::to_string(&tool).expect("Serialization failed");
    assert!(json.contains("\"type\":\"google_maps\""));
    assert!(!json.contains("enable_widget"));

    let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");
    match parsed {
        Tool::GoogleMaps { enable_widget, .. } => assert_eq!(enable_widget, None),
        other => panic!("Expected GoogleMaps variant, got {:?}", other),
    }
}

#[test]
fn test_tool_google_maps_with_widget_roundtrip() {
    let tool = Tool::GoogleMaps {
        enable_widget: Some(true),
        latitude: Some(40.758),
        longitude: Some(-73.9855),
    };
    let json = serde_json::to_string(&tool).expect("Serialization failed");
    assert!(json.contains("\"enable_widget\":true"));
    assert!(json.contains("\"latitude\":40.758"));
    assert!(json.contains("\"longitude\":-73.9855"));

    let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");
    match parsed {
        Tool::GoogleMaps {
            enable_widget,
            latitude,
            longitude,
        } => {
            assert_eq!(enable_widget, Some(true));
            assert_eq!(latitude, Some(40.758));
            assert_eq!(longitude, Some(-73.9855));
        }
        other => panic!("Expected GoogleMaps variant, got {:?}", other),
    }
}

#[test]
fn test_tool_function_roundtrip() {
    let tool = Tool::Function {
        name: "get_weather".to_string(),
        description: "Get weather".to_string(),
        parameters: FunctionParameters::new("object".to_string(), serde_json::json!({}), vec![]),
    };
    let json = serde_json::to_string(&tool).expect("Serialization failed");
    let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");

    match parsed {
        Tool::Function { name, .. } => assert_eq!(name, "get_weather"),
        other => panic!("Expected Function variant, got {:?}", other),
    }
}

#[test]
fn test_tool_mcp_server_roundtrip() {
    let tool = Tool::McpServer {
        name: "my-server".to_string(),
        url: "https://mcp.example.com/api".to_string(),
        allowed_tools: None,
        headers: None,
    };
    let json = serde_json::to_string(&tool).expect("Serialization failed");
    assert!(json.contains("\"type\":\"mcp_server\""));
    assert!(json.contains("\"name\":\"my-server\""));
    assert!(json.contains("\"url\":\"https://mcp.example.com/api\""));
    assert!(!json.contains("allowed_tools"));
    assert!(!json.contains("headers"));

    let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");
    match parsed {
        Tool::McpServer {
            name,
            url,
            allowed_tools,
            headers,
        } => {
            assert_eq!(name, "my-server");
            assert_eq!(url, "https://mcp.example.com/api");
            assert_eq!(allowed_tools, None);
            assert_eq!(headers, None);
        }
        other => panic!("Expected McpServer variant, got {:?}", other),
    }
}

#[test]
fn test_tool_mcp_server_with_optional_fields_roundtrip() {
    let tool = Tool::McpServer {
        name: "my-server".to_string(),
        url: "https://mcp.example.com/api".to_string(),
        allowed_tools: Some(vec![
            AllowedTools::new(vec!["read_file".to_string(), "list_dir".to_string()])
                .with_mode(FunctionCallingMode::Auto),
        ]),
        headers: Some(HashMap::from([(
            "Authorization".to_string(),
            "Bearer token".to_string(),
        )])),
    };
    let json = serde_json::to_string(&tool).expect("Serialization failed");
    assert!(json.contains("\"allowed_tools\""));
    assert!(json.contains("\"headers\""));

    let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");
    match parsed {
        Tool::McpServer {
            allowed_tools,
            headers,
            ..
        } => {
            let tools = allowed_tools.expect("Should have allowed_tools");
            assert_eq!(tools.len(), 1);
            assert_eq!(tools[0].tools.len(), 2);
            assert_eq!(tools[0].mode, Some(FunctionCallingMode::Auto));
            let hdrs = headers.expect("Should have headers");
            assert_eq!(hdrs.get("Authorization").unwrap(), "Bearer token");
        }
        other => panic!("Expected McpServer variant, got {:?}", other),
    }
}

#[test]
fn test_tool_unknown_deserialization() {
    // Simulate an unknown tool type from the API
    let json = r#"{"type": "future_tool", "some_field": "value", "number": 42}"#;
    let parsed: Tool = serde_json::from_str(json).expect("Deserialization failed");

    match parsed {
        Tool::Unknown { tool_type, data } => {
            assert_eq!(tool_type, "future_tool");
            assert_eq!(data.get("some_field").unwrap(), "value");
            assert_eq!(data.get("number").unwrap(), 42);
        }
        _ => panic!("Expected Unknown variant"),
    }
}

#[test]
fn test_tool_unknown_roundtrip() {
    let tool = Tool::Unknown {
        tool_type: "new_tool".to_string(),
        data: serde_json::json!({"type": "new_tool", "config": {"enabled": true}}),
    };
    let json = serde_json::to_string(&tool).expect("Serialization failed");

    // Should contain the type and config, but not duplicate "type"
    assert!(json.contains("\"type\":\"new_tool\""));
    assert!(json.contains("\"config\""));

    let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");
    match parsed {
        Tool::Unknown { tool_type, .. } => assert_eq!(tool_type, "new_tool"),
        _ => panic!("Expected Unknown variant"),
    }
}

#[test]
fn test_tool_unknown_helper_methods() {
    // Test Unknown variant
    let unknown_tool = Tool::Unknown {
        tool_type: "future_tool".to_string(),
        data: serde_json::json!({"type": "future_tool", "setting": 123}),
    };

    assert!(unknown_tool.is_unknown());
    assert_eq!(unknown_tool.unknown_tool_type(), Some("future_tool"));
    let data = unknown_tool.unknown_data().expect("Should have data");
    assert_eq!(data.get("setting").unwrap(), 123);
}

#[test]
fn test_tool_computer_use_roundtrip() {
    let tool = Tool::ComputerUse {
        environment: "browser".to_string(),
        excluded_predefined_functions: vec!["submit_form".to_string(), "download".to_string()],
        enable_prompt_injection_detection: Some(true),
        disabled_safety_policies: vec!["data_modification".to_string()],
    };
    let json = serde_json::to_string(&tool).expect("Serialization failed");
    assert!(json.contains("\"type\":\"computer_use\""));
    assert!(json.contains("\"environment\":\"browser\""));
    assert!(json.contains("\"excluded_predefined_functions\""));
    assert!(!json.contains("excludedPredefinedFunctions"));
    assert!(json.contains("\"enable_prompt_injection_detection\":true"));
    assert!(json.contains("\"disabled_safety_policies\""));

    let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");
    match parsed {
        Tool::ComputerUse {
            environment,
            excluded_predefined_functions,
            enable_prompt_injection_detection,
            disabled_safety_policies,
        } => {
            assert_eq!(environment, "browser");
            assert_eq!(excluded_predefined_functions.len(), 2);
            assert!(excluded_predefined_functions.contains(&"submit_form".to_string()));
            assert_eq!(enable_prompt_injection_detection, Some(true));
            assert_eq!(
                disabled_safety_policies,
                vec!["data_modification".to_string()]
            );
        }
        other => panic!("Expected ComputerUse variant, got {:?}", other),
    }
}

#[test]
fn test_tool_computer_use_empty_exclusions() {
    // Test that empty exclusions don't serialize the field
    let tool = Tool::ComputerUse {
        environment: "browser".to_string(),
        excluded_predefined_functions: vec![],
        enable_prompt_injection_detection: None,
        disabled_safety_policies: vec![],
    };
    let json = serde_json::to_string(&tool).expect("Serialization failed");
    assert!(json.contains("\"type\":\"computer_use\""));
    assert!(json.contains("\"environment\":\"browser\""));
    assert!(!json.contains("excluded_predefined_functions"));

    let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");
    match parsed {
        Tool::ComputerUse {
            excluded_predefined_functions,
            ..
        } => {
            assert!(excluded_predefined_functions.is_empty());
        }
        other => panic!("Expected ComputerUse variant, got {:?}", other),
    }
}

#[test]
fn test_tool_known_types_helper_methods() {
    // Test known types return None for unknown helpers
    let google_search = Tool::GoogleSearch { search_types: None };
    assert!(!google_search.is_unknown());
    assert_eq!(google_search.unknown_tool_type(), None);
    assert_eq!(google_search.unknown_data(), None);

    let google_maps = Tool::GoogleMaps {
        enable_widget: None,
        latitude: None,
        longitude: None,
    };
    assert!(!google_maps.is_unknown());
    assert_eq!(google_maps.unknown_tool_type(), None);
    assert_eq!(google_maps.unknown_data(), None);

    let code_execution = Tool::CodeExecution;
    assert!(!code_execution.is_unknown());
    assert_eq!(code_execution.unknown_tool_type(), None);
    assert_eq!(code_execution.unknown_data(), None);

    let url_context = Tool::UrlContext;
    assert!(!url_context.is_unknown());
    assert_eq!(url_context.unknown_tool_type(), None);
    assert_eq!(url_context.unknown_data(), None);

    let computer_use = Tool::ComputerUse {
        environment: "browser".to_string(),
        excluded_predefined_functions: vec![],
        enable_prompt_injection_detection: None,
        disabled_safety_policies: vec![],
    };
    assert!(!computer_use.is_unknown());
    assert_eq!(computer_use.unknown_tool_type(), None);
    assert_eq!(computer_use.unknown_data(), None);

    let function = Tool::Function {
        name: "test".to_string(),
        description: "Test function".to_string(),
        parameters: FunctionParameters::new("object".to_string(), serde_json::json!({}), vec![]),
    };
    assert!(!function.is_unknown());
    assert_eq!(function.unknown_tool_type(), None);
    assert_eq!(function.unknown_data(), None);
}

#[test]
fn test_tool_file_search_roundtrip() {
    let tool = Tool::FileSearch {
        store_names: vec!["store1".to_string(), "store2".to_string()],
        top_k: Some(5),
        metadata_filter: Some("category:technical".to_string()),
    };
    let json = serde_json::to_string(&tool).expect("Serialization failed");
    assert!(json.contains("\"type\":\"file_search\""));
    assert!(json.contains("\"file_search_store_names\"")); // Wire format uses full name
    assert!(json.contains("\"top_k\":5"));
    assert!(json.contains("\"metadata_filter\":\"category:technical\""));

    let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");
    match parsed {
        Tool::FileSearch {
            store_names,
            top_k,
            metadata_filter,
        } => {
            assert_eq!(store_names, vec!["store1", "store2"]);
            assert_eq!(top_k, Some(5));
            assert_eq!(metadata_filter, Some("category:technical".to_string()));
        }
        other => panic!("Expected FileSearch variant, got {:?}", other),
    }
}

#[test]
fn test_tool_file_search_minimal() {
    // Test with only required field (store names)
    let tool = Tool::FileSearch {
        store_names: vec!["my-store".to_string()],
        top_k: None,
        metadata_filter: None,
    };
    let json = serde_json::to_string(&tool).expect("Serialization failed");
    assert!(json.contains("\"type\":\"file_search\""));
    assert!(json.contains("\"file_search_store_names\"")); // Wire format uses full name
    // Optional fields should not appear
    assert!(!json.contains("\"top_k\""));
    assert!(!json.contains("\"metadata_filter\""));

    let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");
    match parsed {
        Tool::FileSearch {
            store_names,
            top_k,
            metadata_filter,
        } => {
            assert_eq!(store_names, vec!["my-store"]);
            assert_eq!(top_k, None);
            assert_eq!(metadata_filter, None);
        }
        other => panic!("Expected FileSearch variant, got {:?}", other),
    }
}

#[test]
fn test_tool_file_search_helper_methods() {
    let file_search = Tool::FileSearch {
        store_names: vec!["store".to_string()],
        top_k: None,
        metadata_filter: None,
    };
    assert!(!file_search.is_unknown());
    assert_eq!(file_search.unknown_tool_type(), None);
    assert_eq!(file_search.unknown_data(), None);
}

#[test]
fn test_tool_retrieval_minimal_serializes_type_only() {
    let tool = Tool::Retrieval {
        retrieval_types: None,
        vertex_ai_search_config: None,
        exa_ai_search_config: None,
        parallel_ai_search_config: None,
        rag_store_config: None,
    };
    let json = serde_json::to_string(&tool).expect("Serialization failed");
    assert_eq!(json, r#"{"type":"retrieval"}"#);

    let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");
    assert!(matches!(parsed, Tool::Retrieval { .. }));
    assert!(!parsed.is_unknown());
}
