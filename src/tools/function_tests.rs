use super::*;
use serde_json;

#[test]
fn test_serialize_function_declaration() {
    let function = FunctionDeclaration::builder("get_weather")
        .with_description("Get the current weather in a given location")
        .add_parameter(
            "location",
            serde_json::json!({
                "type": "string",
                "description": "The city and state, e.g. San Francisco, CA"
            }),
        )
        .with_required(vec!["location".to_string()])
        .build();

    let json_string = serde_json::to_string(&function).expect("Serialization failed");
    let parsed: FunctionDeclaration =
        serde_json::from_str(&json_string).expect("Deserialization failed");

    assert_eq!(parsed.name(), "get_weather");
    assert_eq!(
        parsed.description(),
        "Get the current weather in a given location"
    );
}

#[test]
fn test_add_parameter_takes_an_owned_or_borrowed_name() {
    // A name built at run time is passed as is, with no `&` (#504).
    let names: Vec<String> = (1..=2).map(|i| format!("param{i}")).collect();
    let mut builder = FunctionDeclaration::builder("f");
    for name in names {
        builder = builder.add_parameter(name, serde_json::json!({"type": "string"}));
    }
    let function = builder
        .add_parameter("literal", serde_json::json!({"type": "number"}))
        .build();
    let properties = function.parameters().properties();
    assert_eq!(properties["param1"], serde_json::json!({"type": "string"}));
    assert_eq!(properties["param2"], serde_json::json!({"type": "string"}));
    assert_eq!(properties["literal"], serde_json::json!({"type": "number"}));
}
