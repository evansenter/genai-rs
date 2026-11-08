//! Procedural macros for `genai-rs`.
//!
//! # Version coupling with `genai-rs`
//!
//! The code `#[tool]` generates names `genai_rs::__private` (its
//! `inventory` registration and `serde_json`) and
//! `genai_rs::function_calling::FunctionFuture`, so this crate requires the
//! `genai-rs` release it ships with. Being a proc-macro crate it declares no
//! dependency on `genai-rs`, so Cargo cannot enforce that: a 0.13
//! `genai-rs-macros` names `genai_rs::function_calling::submit`, which 0.14
//! removed, and a 0.14 one names `__private::submit` and `FunctionFuture`,
//! which 0.13 lacks. Either mismatch fails at every `#[tool]` site. Bump
//! them together.

// `clippy.toml` exempts a `#[cfg(test)]` module from `unwrap_used`,
// `expect_used` and `panic`; clippy has no such setting for the other
// panicking macros, so the unit-test build allows them here. The library
// build still denies them (D-048).
#![cfg_attr(
    test,
    allow(
        clippy::unreachable,
        clippy::todo,
        clippy::unimplemented,
        reason = "a test reports a failure by panicking"
    )
)]

use proc_macro::TokenStream;
use syn::Pat;

mod codegen;
mod parsing;
mod schema;

use parsing::parse_input;
use schema::{build_param_schema, get_type_info};

/// Generates a function that returns a `FunctionDeclaration` for the annotated function.
///
/// # Example
///
/// <!-- ignore: genai-rs-macros does not depend on genai-rs, which the expansion names; docs/FUNCTION_CALLING.md compiles the same code -->
/// ```ignore
/// use genai_rs_macros::tool;
///
/// #[tool(
///     location(description = "The city and state"),
///     unit(enum_values = ["celsius", "fahrenheit"])
/// )]
/// fn get_weather(location: String, unit: Option<String>) -> String {
///     format!("Weather for {}", location)
/// }
///
/// // The macro generates:
/// // pub fn get_weather_declaration() -> genai_rs::FunctionDeclaration { ... }
/// ```
#[proc_macro_attribute]
pub fn tool(attr_input: TokenStream, item: TokenStream) -> TokenStream {
    let input = match parse_input(attr_input, item) {
        Ok(input) => input,
        Err(e) => return e.to_compile_error().into(),
    };

    let func = input.func;
    let config_map = input.param_configs;
    let func_name = func.sig.ident.to_string();

    // Collect actual function parameter names
    let mut actual_param_names = std::collections::HashSet::new();
    for fn_arg in &func.sig.inputs {
        if let syn::FnArg::Typed(pat_type) = fn_arg
            && let Pat::Ident(pat_ident) = &*pat_type.pat
        {
            actual_param_names.insert(pat_ident.ident.to_string());
        }
    }

    // Check that all macro-referenced parameters actually exist in the function
    for referenced_param in config_map.keys() {
        if !actual_param_names.contains(referenced_param) {
            return syn::Error::new(
                func.sig.ident.span(),
                format!(
                    "Parameter '{}' referenced in #[tool] attribute does not exist in function '{}'. \
                     Available parameters: {:?}",
                    referenced_param,
                    func_name,
                    actual_param_names.iter().collect::<Vec<_>>()
                ),
            )
            .to_compile_error()
            .into();
        }
    }

    let func_description = parsing::extract_doc_comments(&func.attrs);
    let mut properties = serde_json::Map::new();
    let mut required_params_for_struct_field = Vec::new();

    for fn_arg in &func.sig.inputs {
        if let syn::FnArg::Typed(pat_type) = fn_arg
            && let Pat::Ident(pat_ident) = &*pat_type.pat
        {
            let param_name = pat_ident.ident.to_string();
            let config = config_map.get(&param_name);
            properties.insert(param_name.clone(), build_param_schema(pat_type, config));

            let (is_option, _) = get_type_info(&pat_type.ty);
            if !is_option {
                required_params_for_struct_field.push(param_name.clone());
            }
        }
    }

    codegen::generate_declaration_function(
        &func,
        &func_name,
        &func_description,
        &serde_json::Value::Object(properties),
        &required_params_for_struct_field,
    )
    .into()
}
