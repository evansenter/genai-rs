use super::*;
use serde_json;

#[test]
fn test_search_type_roundtrip() {
    let types = vec![SearchType::WebSearch, SearchType::ImageSearch];
    let json = serde_json::to_string(&types).expect("Serialization failed");
    assert_eq!(json, r#"["web_search","image_search"]"#);

    let parsed: Vec<SearchType> = serde_json::from_str(&json).expect("Deserialization failed");
    assert_eq!(parsed, types);
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_search_type_unknown_roundtrip() {
    let json = r#""future_search""#;
    let parsed: SearchType = serde_json::from_str(json).expect("Deserialization failed");
    assert!(parsed.is_unknown());
    assert_eq!(parsed.unknown_search_type(), Some("future_search"));
    assert_eq!(
        parsed.unknown_data(),
        Some(&serde_json::Value::String("future_search".to_string()))
    );

    let reserialized = serde_json::to_string(&parsed).expect("Serialization failed");
    assert_eq!(reserialized, json);
}

#[test]
fn test_google_search_config_into_tool() {
    let tool: Tool = GoogleSearchConfig::new().into();
    assert!(matches!(tool, Tool::GoogleSearch { search_types: None }));

    let tool: Tool = GoogleSearchConfig::new()
        .with_search_types(vec![SearchType::ImageSearch])
        .into();
    match tool {
        Tool::GoogleSearch { search_types } => {
            let types = search_types.expect("Should have search_types");
            assert_eq!(types, vec![SearchType::ImageSearch]);
        }
        other => panic!("Expected GoogleSearch, got {:?}", other),
    }
}

#[test]
fn test_google_maps_config_into_tool() {
    let tool: Tool = GoogleMapsConfig::new().into();
    assert!(matches!(
        tool,
        Tool::GoogleMaps {
            enable_widget: None,
            ..
        }
    ));

    let tool: Tool = GoogleMapsConfig::new().with_widget().into();
    assert!(matches!(
        tool,
        Tool::GoogleMaps {
            enable_widget: Some(true),
            ..
        }
    ));
}

#[test]
fn test_mcp_server_config_into_tool() {
    let tool: Tool = McpServerConfig::new("server", "https://example.com").into();
    match tool {
        Tool::McpServer {
            name,
            url,
            allowed_tools,
            headers,
        } => {
            assert_eq!(name, "server");
            assert_eq!(url, "https://example.com");
            assert_eq!(allowed_tools, None);
            assert_eq!(headers, None);
        }
        other => panic!("Expected McpServer, got {:?}", other),
    }
}

#[test]
fn test_computer_use_config_into_tool() {
    let tool: Tool = ComputerUseConfig::new().into();
    match tool {
        Tool::ComputerUse {
            environment,
            excluded_predefined_functions,
            ..
        } => {
            assert_eq!(environment, "browser");
            assert!(excluded_predefined_functions.is_empty());
        }
        other => panic!("Expected ComputerUse, got {:?}", other),
    }

    let tool: Tool = ComputerUseConfig::new()
        .with_excluded_predefined_functions(vec!["download".to_string()])
        .into();
    match tool {
        Tool::ComputerUse {
            excluded_predefined_functions,
            ..
        } => {
            assert_eq!(excluded_predefined_functions, vec!["download"]);
        }
        other => panic!("Expected ComputerUse, got {:?}", other),
    }
}

#[test]
fn test_file_search_config_into_tool() {
    let tool: Tool = FileSearchConfig::new(vec!["store".to_string()])
        .with_top_k(5)
        .with_metadata_filter("cat:tech")
        .into();
    match tool {
        Tool::FileSearch {
            store_names,
            top_k,
            metadata_filter,
        } => {
            assert_eq!(store_names, vec!["store"]);
            assert_eq!(top_k, Some(5));
            assert_eq!(metadata_filter, Some("cat:tech".to_string()));
        }
        other => panic!("Expected FileSearch, got {:?}", other),
    }
}
