//! Unit tests for FunctionDeclarationBuilder (no API key required)
//!
//! This file contains tests for FunctionDeclarationBuilder edge cases and validation.

use genai_rs::FunctionDeclaration;
use serde_json::json;

#[test]
fn test_function_builder_with_empty_name() {
    // Builder accepts empty names but logs a warning
    let func = FunctionDeclaration::builder("")
        .with_description("Test function")
        .build();

    assert_eq!(func.name(), "");
    // Note: This will likely be rejected by the API, but builder allows it
}

#[test]
fn test_function_builder_with_whitespace_only_name() {
    let func = FunctionDeclaration::builder("   ")
        .with_description("Test function")
        .build();

    assert_eq!(func.name(), "   ");
}

#[test]
fn test_function_builder_parameter_overwrites_on_duplicate() {
    // When the same parameter name is added twice, the second should overwrite
    let func = FunctionDeclaration::builder("test_func")
        .add_parameter(
            "location",
            json!({"type": "string", "description": "First"}),
        )
        .add_parameter(
            "location",
            json!({"type": "string", "description": "Second"}),
        )
        .build();

    // Verify the second parameter overwrote the first
    let location_desc = func
        .parameters()
        .properties()
        .get("location")
        .and_then(|l| l.get("description"))
        .and_then(|d| d.as_str());

    assert_eq!(location_desc, Some("Second"));
}

#[test]
fn test_function_builder_required_non_existent_parameter() {
    // Builder allows requiring parameters that don't exist but logs a warning
    let func = FunctionDeclaration::builder("test_func")
        .add_parameter("existing_param", json!({"type": "string"}))
        .with_required(vec!["nonexistent_param".to_string()])
        .build();

    assert_eq!(func.parameters().required(), vec!["nonexistent_param"]);
    // Note: This will likely cause API errors, but builder allows it
}

#[test]
fn test_function_builder_method_order_independence() {
    // Verify that calling methods in different orders produces identical results
    let func1 = FunctionDeclaration::builder("test")
        .with_description("A test function")
        .add_parameter("param1", json!({"type": "string"}))
        .with_required(vec!["param1".to_string()])
        .build();

    let func2 = FunctionDeclaration::builder("test")
        .with_required(vec!["param1".to_string()])
        .add_parameter("param1", json!({"type": "string"}))
        .with_description("A test function")
        .build();

    // Compare serialized forms since FunctionDeclaration doesn't implement PartialEq
    let json1 = serde_json::to_value(&func1).unwrap();
    let json2 = serde_json::to_value(&func2).unwrap();

    assert_eq!(json1, json2);
}

#[test]
fn test_function_builder_with_no_parameters() {
    let func = FunctionDeclaration::builder("no_params")
        .with_description("Function with no parameters")
        .build();

    assert_eq!(func.parameters().type_(), "object");
    assert!(func.parameters().properties().is_object());
    assert!(func.parameters().required().is_empty());
}

#[test]
fn test_function_builder_with_many_parameters() {
    let mut builder = FunctionDeclaration::builder("many_params");

    // Add 20 parameters
    for i in 0..20 {
        builder = builder.add_parameter(
            &format!("param_{}", i),
            json!({"type": "string", "description": format!("Parameter {}", i)}),
        );
    }

    let func = builder.build();

    // Verify all parameters were added
    let properties = func.parameters().properties();

    for i in 0..20 {
        assert!(properties.get(format!("param_{}", i)).is_some());
    }
}

#[test]
fn test_function_builder_required_all_parameters() {
    let func = FunctionDeclaration::builder("all_required")
        .add_parameter("param1", json!({"type": "string"}))
        .add_parameter("param2", json!({"type": "number"}))
        .add_parameter("param3", json!({"type": "boolean"}))
        .with_required(vec![
            "param1".to_string(),
            "param2".to_string(),
            "param3".to_string(),
        ])
        .build();

    assert_eq!(func.parameters().required().len(), 3);
}

#[test]
fn test_function_builder_required_subset_of_parameters() {
    let func = FunctionDeclaration::builder("partial_required")
        .add_parameter("required_param", json!({"type": "string"}))
        .add_parameter("optional_param", json!({"type": "string"}))
        .with_required(vec!["required_param".to_string()])
        .build();

    assert_eq!(func.parameters().required().len(), 1);
    assert_eq!(func.parameters().required()[0], "required_param");
}

#[test]
fn test_function_builder_complex_nested_schema() {
    let func = FunctionDeclaration::builder("nested_schema")
        .add_parameter(
            "complex_param",
            json!({
                "type": "object",
                "properties": {
                    "nested": {
                        "type": "object",
                        "properties": {
                            "deep": {"type": "string"}
                        }
                    }
                }
            }),
        )
        .build();

    // Verify nested structure is preserved
    let param = func.parameters().properties().get("complex_param");

    assert!(param.is_some());
    assert!(param.unwrap().get("properties").is_some());
}

#[test]
fn test_function_builder_with_array_parameter() {
    let func = FunctionDeclaration::builder("array_param")
        .add_parameter(
            "items",
            json!({
                "type": "array",
                "items": {"type": "string"}
            }),
        )
        .build();

    let param = func
        .parameters()
        .properties()
        .get("items")
        .and_then(|i| i.get("type"))
        .and_then(|t| t.as_str());

    assert_eq!(param, Some("array"));
}

#[test]
fn test_function_builder_with_enum_values() {
    let func = FunctionDeclaration::builder("enum_param")
        .add_parameter(
            "unit",
            json!({
                "type": "string",
                "enum": ["celsius", "fahrenheit", "kelvin"]
            }),
        )
        .build();

    let enum_values = func
        .parameters()
        .properties()
        .get("unit")
        .and_then(|u| u.get("enum"))
        .and_then(|e| e.as_array());

    assert_eq!(enum_values.unwrap().len(), 3);
}

#[test]
fn test_function_builder_description_can_be_empty() {
    let func = FunctionDeclaration::builder("test")
        .with_description("")
        .build();

    assert_eq!(func.description(), "");
}

#[test]
fn test_function_builder_description_with_unicode() {
    let func = FunctionDeclaration::builder("test")
        .with_description("测试函数 with émojis 🎉")
        .build();

    assert!(func.description().contains("🎉"));
}

#[test]
fn test_function_builder_very_long_description() {
    let long_desc = "x".repeat(10000);
    let func = FunctionDeclaration::builder("test")
        .with_description(&long_desc)
        .build();

    assert_eq!(func.description().len(), 10000);
}
