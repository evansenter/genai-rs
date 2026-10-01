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
