//! Procedural macros for `genai-rs`.
//!
//! # Version coupling with `genai-rs`
//!
//! The code `#[tool]` generates references `genai_rs::__private`, so this
//! crate requires a matching-or-newer `genai-rs`. Being a proc-macro crate it
//! declares no dependency on `genai-rs`, so Cargo cannot enforce that —
//! pinning the two independently with this crate ahead of the library fails
//! at every `#[tool]` site with `could not find __private in genai_rs`.
//! Bump them together.

#![cfg_attr(test, allow(dead_code))]

use proc_macro::TokenStream;
use syn::Pat;
use utoipa::openapi::RefOr;
use utoipa::openapi::schema::{ObjectBuilder, Schema};

mod codegen;
mod parsing;
mod schema;

use parsing::parse_input;
use schema::{build_param_schema, get_type_info};

/// Generates a function that returns a `FunctionDeclaration` for the annotated function.
///
/// # Example
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
    let mut object_builder = ObjectBuilder::new();
    let mut required_params_for_struct_field = Vec::new();

    for fn_arg in &func.sig.inputs {
        if let syn::FnArg::Typed(pat_type) = fn_arg
            && let Pat::Ident(pat_ident) = &*pat_type.pat
        {
            let param_name = pat_ident.ident.to_string();
            let config = config_map.get(&param_name);
            let param_schema = build_param_schema(pat_type, config);

            object_builder = object_builder.property(param_name.clone(), param_schema);

            let (is_option, _) = get_type_info(&pat_type.ty);
            if !is_option {
                required_params_for_struct_field.push(param_name.clone());
            }
        }
    }

    let parameters_schema_obj = object_builder.build();
    let parameters_schema_ref_or: RefOr<Schema> = RefOr::T(Schema::Object(parameters_schema_obj));

    codegen::generate_declaration_function(
        &func,
        &func_name,
        &func_description,
        &parameters_schema_ref_or,
        &required_params_for_struct_field,
    )
    .into()
}
