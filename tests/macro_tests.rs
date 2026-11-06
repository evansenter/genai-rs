//! The `#[tool]` macro: the declarations it generates from a function's
//! signature, doc comments and attribute, and calling the result.

#![allow(dead_code)] // Functions are used by the macro, not called directly
#![allow(clippy::needless_pass_by_value)] // Macro needs owned types for schema generation

use genai_rs::CallableFunction;
use genai_rs_macros::tool;

#[test]
fn test_basic_function_declaration() {
    #[tool]
    fn test_basic(name: String) -> String {
        name
    }

    let callable = TestBasicCallable;
    let decl = callable.declaration();
    assert_eq!(decl.name(), "test_basic");
    assert_eq!(decl.description(), ""); // Empty doc comments result in empty string
    assert_eq!(decl.parameters().required(), vec!["name"]);
}

#[test]
fn test_function_with_doc_comment() {
    /// This is a test function that does something
    #[tool]
    fn test_with_docs(name: String) -> String {
        name
    }

    let callable = TestWithDocsCallable;
    let decl = callable.declaration();
    assert_eq!(decl.name(), "test_with_docs");
    assert_eq!(
        decl.description(),
        "This is a test function that does something"
    );
}

#[test]
fn test_with_param_metadata() {
    /// Function to greet someone
    #[tool(
        name(description = "The person's name"),
        age(description = "The person's age")
    )]
    fn greet_person(name: String, age: i32) -> String {
        format!("Hello {name}, you are {age} years old")
    }

    let callable = GreetPersonCallable;
    let decl = callable.declaration();
    assert_eq!(decl.name(), "greet_person");
    assert_eq!(decl.description(), "Function to greet someone");
    assert_eq!(decl.parameters().required(), vec!["name", "age"]);

    // Check that parameters schema contains our descriptions
    let params_json = serde_json::to_string(&decl.parameters()).unwrap();
    assert!(params_json.contains("The person's name"));
    assert!(params_json.contains("The person's age"));
}

#[test]
fn test_optional_parameters() {
    #[tool(
        name(description = "Required name"),
        nickname(description = "Optional nickname")
    )]
    fn test_optional(name: String, nickname: Option<String>) -> String {
        nickname.unwrap_or(name)
    }

    let callable = TestOptionalCallable;
    let decl = callable.declaration();
    assert_eq!(decl.parameters().required(), vec!["name"]); // Only name should be required
}

#[test]
fn test_enum_values() {
    #[tool(
        unit(enum_values = ["celsius", "fahrenheit", "kelvin"])
    )]
    #[allow(unused_variables)]
    fn convert_temp(value: f64, unit: String) -> f64 {
        value
    }

    let callable = ConvertTempCallable;
    let decl = callable.declaration();
    let params_json = serde_json::to_string(&decl.parameters()).unwrap();
    assert!(params_json.contains("celsius"));
    assert!(params_json.contains("fahrenheit"));
    assert!(params_json.contains("kelvin"));
    assert_eq!(decl.parameters().required(), vec!["value", "unit"]);
}

#[test]
fn test_various_types() {
    #[tool]
    fn test_types(
        text: String,
        count: i32,
        amount: f64,
        flag: bool,
        items: Vec<String>,
        data: serde_json::Value,
    ) -> String {
        format!("{text} {count} {amount} {flag} {items:?} {data}")
    }

    let callable = TestTypesCallable;
    let decl = callable.declaration();
    assert_eq!(
        decl.parameters().required(),
        vec!["text", "count", "amount", "flag", "items", "data"]
    );

    let params_json = serde_json::to_string(&decl.parameters()).unwrap();
    // Check that the types are correctly mapped
    assert!(params_json.contains(r#""type":"string"#)); // for text
    assert!(params_json.contains(r#""type":"integer"#)); // for count
    assert!(params_json.contains(r#""type":"number"#)); // for amount
    assert!(params_json.contains(r#""type":"boolean"#)); // for flag
    assert!(params_json.contains(r#""type":"array"#)); // for items
    assert!(params_json.contains(r#""type":"object"#)); // for data
}

#[test]
fn test_multiline_doc_comment() {
    /// This is a function that
    /// does multiple things:
    /// - First thing
    /// - Second thing
    #[tool]
    fn test_multiline(x: String) -> String {
        x
    }

    let callable = TestMultilineCallable;
    let decl = callable.declaration();
    assert_eq!(
        decl.description(),
        "This is a function that\ndoes multiple things:\n- First thing\n- Second thing"
    );
}

// This test verifies the comment about parameter doc comments
// Since Rust doesn't allow doc comments on parameters, we need to verify
// that this comment in the code is accurate
#[test]
fn test_param_without_metadata_no_description() {
    #[tool]
    fn test_no_param_desc(
        // Regular comment - not a doc comment
        name: String,
    ) -> String {
        name
    }

    let callable = TestNoParamDescCallable;
    let decl = callable.declaration();
    let params_json = serde_json::to_string(&decl.parameters()).unwrap();
    // The parameter should have no description since doc comments aren't allowed on params
    // and we didn't provide one in the macro attribute
    assert!(!params_json.contains("description") || params_json.contains(r#""description":null"#));
}

// Test to demonstrate why parameter doc comments don't work
#[test]
fn test_param_docs_not_allowed() {
    // The following would NOT compile if uncommented:
    /*
    #[tool]
    fn test_param_doc(
        /// This doc comment would cause a compile error
        name: String
    ) -> String {
        name
    }
    */

    // Instead, descriptions must be provided via the macro attribute:
    #[tool(name(description = "This is the correct way to add param descriptions"))]
    fn test_correct_param_desc(name: String) -> String {
        name
    }

    let callable = TestCorrectParamDescCallable;
    let decl = callable.declaration();
    let params_json = serde_json::to_string(&decl.parameters()).unwrap();
    assert!(params_json.contains("This is the correct way to add param descriptions"));
}
