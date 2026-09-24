//! JSON Schema generation for function parameters.
//!
//! Maps a parameter's Rust type to the JSON Schema object the Gemini API
//! expects in a function declaration's `parameters.properties`.

use crate::parsing::ParamConfig;
use serde_json::{Map, Value, json};
use syn::{GenericArgument, PathArguments, Type};

/// Analyzes a type to determine if it's an `Option<T>` wrapper.
///
/// Returns a tuple of:
/// - `bool`: `true` if the type is `Option<T>` (by any path, e.g.
///   `std::option::Option<T>`), `false` otherwise
/// - `Type`: The inner type (unwrapped if `Option`, original otherwise)
pub fn get_type_info(ty: &Type) -> (bool, Type) {
    match single_generic_arg(ty, &["Option"]) {
        Some(inner) => (true, inner.clone()),
        None => (false, ty.clone()),
    }
}

/// The type argument of `ty` when its last path segment is one of `names`
/// with exactly one type argument (`Vec<T>`, `std::option::Option<T>`, ...).
fn single_generic_arg<'a>(ty: &'a Type, names: &[&str]) -> Option<&'a Type> {
    let Type::Path(type_path) = ty else {
        return None;
    };
    let segment = type_path.path.segments.last()?;
    if !names.iter().any(|name| segment.ident == name) {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    let mut types = args.args.iter().filter_map(|arg| match arg {
        GenericArgument::Type(ty) => Some(ty),
        _ => None,
    });
    match (types.next(), types.next()) {
        (Some(inner), None) => Some(inner),
        _ => None,
    }
}

/// Maps a Rust type to its JSON Schema.
///
/// Matching is by the last path segment, so `std::string::String` and
/// `String` agree. References, `Box`/`Rc`/`Arc` and `Option` are looked
/// through. Anything unrecognized (structs, maps, `serde_json::Value`) is an
/// `object`, which the model treats as free-form JSON.
fn type_schema(ty: &Type) -> Map<String, Value> {
    match ty {
        Type::Reference(reference) => return type_schema(&reference.elem),
        Type::Paren(paren) => return type_schema(&paren.elem),
        Type::Group(group) => return type_schema(&group.elem),
        Type::Array(array) => return array_schema(&array.elem),
        Type::Slice(slice) => return array_schema(&slice.elem),
        _ => {}
    }

    if let Some(inner) = single_generic_arg(ty, &["Option", "Box", "Rc", "Arc"]) {
        return type_schema(inner);
    }
    if let Some(inner) = single_generic_arg(ty, &["Vec", "VecDeque", "HashSet", "BTreeSet"]) {
        return array_schema(inner);
    }

    let json_type = match ty {
        Type::Path(type_path) => match type_path.path.segments.last() {
            Some(segment) => match segment.ident.to_string().as_str() {
                "String" | "str" | "char" => "string",
                "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64"
                | "u128" | "usize" => "integer",
                "f32" | "f64" => "number",
                "bool" => "boolean",
                _ => "object",
            },
            None => "object",
        },
        _ => "object",
    };
    let mut schema = Map::new();
    schema.insert("type".to_string(), json!(json_type));
    schema
}

fn array_schema(item: &Type) -> Map<String, Value> {
    let mut schema = Map::new();
    schema.insert("type".to_string(), json!("array"));
    schema.insert("items".to_string(), Value::Object(type_schema(item)));
    schema
}

/// Builds the JSON Schema for a function parameter, applying the
/// `description` and `enum_values` from the macro attribute.
///
/// On an array, `enum_values` constrains the items.
pub fn build_param_schema(pat_type: &syn::PatType, config: Option<&ParamConfig>) -> Value {
    let mut schema = type_schema(&pat_type.ty);

    if let Some(config) = config {
        if let Some(description) = config.description.as_ref().filter(|d| !d.is_empty()) {
            schema.insert("description".to_string(), json!(description));
        }
        if let Some(enum_values) = &config.enum_values {
            let target = match schema.get_mut("items") {
                Some(Value::Object(items)) => items,
                _ => &mut schema,
            };
            target.insert("enum".to_string(), Value::Array(enum_values.clone()));
        }
    }

    Value::Object(schema)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema_of(ty: &str) -> Value {
        let ty: Type = syn::parse_str(ty).unwrap();
        Value::Object(type_schema(&ty))
    }

    #[test]
    fn scalar_types() {
        for ty in [
            "String",
            "std::string::String",
            "&str",
            "&'static str",
            "char",
        ] {
            assert_eq!(schema_of(ty), json!({"type": "string"}), "{ty}");
        }
        for ty in [
            "i8", "u8", "i16", "u16", "i32", "u32", "i64", "u64", "usize", "isize",
        ] {
            assert_eq!(schema_of(ty), json!({"type": "integer"}), "{ty}");
        }
        for ty in ["f32", "f64"] {
            assert_eq!(schema_of(ty), json!({"type": "number"}), "{ty}");
        }
        assert_eq!(schema_of("bool"), json!({"type": "boolean"}));
    }

    #[test]
    fn wrappers_are_looked_through() {
        for ty in [
            "Option<i32>",
            "std::option::Option<i32>",
            "Box<i32>",
            "std::sync::Arc<i32>",
            "&u16",
        ] {
            assert_eq!(schema_of(ty), json!({"type": "integer"}), "{ty}");
        }
    }

    #[test]
    fn collections_are_arrays_with_typed_items() {
        assert_eq!(
            schema_of("Vec<String>"),
            json!({"type": "array", "items": {"type": "string"}})
        );
        assert_eq!(
            schema_of("std::vec::Vec<Vec<u8>>"),
            json!({"type": "array", "items": {"type": "array", "items": {"type": "integer"}}})
        );
        assert_eq!(
            schema_of("&[f64]"),
            json!({"type": "array", "items": {"type": "number"}})
        );
        assert_eq!(
            schema_of("[bool; 3]"),
            json!({"type": "array", "items": {"type": "boolean"}})
        );
        assert_eq!(
            schema_of("HashSet<i64>"),
            json!({"type": "array", "items": {"type": "integer"}})
        );
    }

    #[test]
    fn unrecognized_types_are_objects() {
        for ty in [
            "serde_json::Value",
            "HashMap<String, i32>",
            "MyStruct",
            "(i32, i32)",
        ] {
            assert_eq!(schema_of(ty), json!({"type": "object"}), "{ty}");
        }
    }

    #[test]
    fn option_detection_follows_the_last_segment() {
        let ty: Type = syn::parse_str("std::option::Option<String>").unwrap();
        let (is_option, inner) = get_type_info(&ty);
        assert!(is_option);
        assert_eq!(quote::quote!(#inner).to_string(), "String");

        let ty: Type = syn::parse_str("Vec<String>").unwrap();
        assert!(!get_type_info(&ty).0);
    }

    #[test]
    fn param_config_applies_description_and_enum() {
        let pat_type: syn::PatType = syn::parse_quote!(unit: String);
        let config = ParamConfig {
            description: Some("The unit".to_string()),
            enum_values: Some(vec![json!("c"), json!("f")]),
        };
        assert_eq!(
            build_param_schema(&pat_type, Some(&config)),
            json!({"type": "string", "description": "The unit", "enum": ["c", "f"]})
        );

        // On an array the enum constrains the items; the description stays
        // on the array.
        let pat_type: syn::PatType = syn::parse_quote!(units: Vec<String>);
        assert_eq!(
            build_param_schema(&pat_type, Some(&config)),
            json!({
                "type": "array",
                "description": "The unit",
                "items": {"type": "string", "enum": ["c", "f"]}
            })
        );

        // An empty description is omitted.
        let config = ParamConfig {
            description: Some(String::new()),
            enum_values: None,
        };
        let pat_type: syn::PatType = syn::parse_quote!(n: i32);
        assert_eq!(
            build_param_schema(&pat_type, Some(&config)),
            json!({"type": "integer"})
        );
    }
}
