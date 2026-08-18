//! Code generation for the `#[tool]` macro.
//!
//! Generates the `FunctionDeclaration`, `CallableFunction` implementation,
//! and auto-registration code for functions annotated with `#[tool]`.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{ItemFn, Pat};
use utoipa::openapi::RefOr;
use utoipa::openapi::schema::Schema;

use crate::schema::get_type_info;

/// Converts a `serde_json::Value` to a `TokenStream` that constructs an equivalent
/// `Value` at runtime.
///
/// This is used to embed parameter schemas directly in the generated code, avoiding
/// runtime JSON parsing.
fn json_value_to_tokens(value: &serde_json::Value) -> TokenStream {
    match value {
        serde_json::Value::Null => quote! { ::genai_rs::__private::serde_json::Value::Null },
        serde_json::Value::Bool(b) => quote! { ::genai_rs::__private::serde_json::Value::Bool(#b) },
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                quote! { ::genai_rs::__private::serde_json::json!(#i) }
            } else if let Some(f) = n.as_f64() {
                quote! { ::genai_rs::__private::serde_json::json!(#f) }
            } else if let Some(u) = n.as_u64() {
                // u64 that doesn't fit in i64 - convert to f64 at compile time
                let f = u as f64;
                quote! { ::genai_rs::__private::serde_json::json!(#f) }
            } else {
                // Should not happen for valid JSON numbers
                quote! { ::genai_rs::__private::serde_json::Value::Null }
            }
        }
        serde_json::Value::String(s) => {
            quote! { ::genai_rs::__private::serde_json::Value::String(#s.to_string()) }
        }
        serde_json::Value::Array(arr) => {
            let items = arr.iter().map(json_value_to_tokens);
            quote! { ::genai_rs::__private::serde_json::Value::Array(vec![#(#items),*]) }
        }
        serde_json::Value::Object(obj) => {
            let pairs = obj.iter().map(|(k, v)| {
                let v_tokens = json_value_to_tokens(v);
                quote! { (#k.to_string(), #v_tokens) }
            });
            quote! {
                ::genai_rs::__private::serde_json::Value::Object({
                    let mut map = ::genai_rs::__private::serde_json::Map::new();
                    #(
                        let (k, v) = #pairs;
                        map.insert(k, v);
                    )*
                    map
                })
            }
        }
    }
}

/// Generates all the code artifacts for a function annotated with `#[tool]`.
///
/// This includes:
/// - The original function (unchanged)
/// - A `{FuncName}Callable` struct implementing `CallableFunction`
/// - A `{func_name}_declaration()` function returning the `FunctionDeclaration`
/// - A `{func_name}_callable_factory()` function for the registry
/// - Automatic registration via `inventory::submit!`
#[allow(clippy::too_many_lines)]
pub fn generate_declaration_function(
    func: &ItemFn,
    func_name: &str,
    func_description: &str,
    parameters_schema_ref: &RefOr<Schema>,
    required_params_for_struct_field: &[String],
) -> TokenStream {
    let generated_fn_name =
        syn::Ident::new(&format!("{func_name}_declaration"), func.sig.ident.span());

    let callable_struct_name_str = func_name
        .split('_')
        .map(|s| {
            s.chars()
                .next()
                .map_or_else(|| s.to_string(), |c| c.to_uppercase().to_string() + &s[1..])
        })
        .collect::<String>()
        + "Callable";
    let callable_struct_name = syn::Ident::new(&callable_struct_name_str, func.sig.ident.span());

    let generated_callable_factory_fn_name = syn::Ident::new(
        &format!("{func_name}_callable_factory"),
        func.sig.ident.span(),
    );

    let required_field_tokens = if required_params_for_struct_field.is_empty() {
        quote! { ::std::vec::Vec::new() }
    } else {
        quote! {
            ::std::vec![
                #( #required_params_for_struct_field.to_string() ),*
            ]
        }
    };

    // Convert the schema to a JSON Value to extract properties.
    // This avoids runtime string parsing - properties are embedded directly in generated code.
    let parameters_schema_value = match serde_json::to_value(parameters_schema_ref) {
        Ok(v) => v,
        Err(e) => {
            return syn::Error::new(
                func.sig.ident.span(),
                format!(
                    "Failed to serialize parameter schema for '{}': {}",
                    func_name, e
                ),
            )
            .to_compile_error();
        }
    };

    // Extract properties from the schema. For functions with parameters, this should always exist.
    let properties_value = match parameters_schema_value.get("properties") {
        Some(props) => props.clone(),
        None => {
            // Functions with no parameters will have no properties - that's fine
            if !required_params_for_struct_field.is_empty() {
                return syn::Error::new(
                    func.sig.ident.span(),
                    format!(
                        "Internal error: generated schema for '{}' has no properties despite having required parameters",
                        func_name
                    ),
                )
                .to_compile_error();
            }
            serde_json::json!({})
        }
    };

    let properties_tokens = json_value_to_tokens(&properties_value);

    let mut arg_names = Vec::new();
    let mut arg_extraction_tokens = Vec::new();

    for fn_arg in &func.sig.inputs {
        if let syn::FnArg::Typed(pat_type) = fn_arg
            && let Pat::Ident(pat_ident) = &*pat_type.pat
        {
            let param_name_str = pat_ident.ident.to_string();
            let param_ident = &pat_ident.ident;
            let param_type = &pat_type.ty;
            arg_names.push(param_ident.clone());

            let (is_option, _inner_type) = get_type_info(param_type);

            if is_option {
                arg_extraction_tokens.push(quote! {
                    let #param_ident: #param_type = match args.get(#param_name_str) {
                        Some(val) if !val.is_null() => {
                            ::genai_rs::__private::serde_json::from_value(val.clone()).map_err(|e| {
                                ::genai_rs::function_calling::FunctionError::ArgumentMismatch(
                                    format!("Failed to deserialize optional argument '{}': {}", #param_name_str, e)
                                )
                            })?
                        }
                        _ => None,
                    };
                });
            } else {
                arg_extraction_tokens.push(quote! {
                    let #param_ident: #param_type = args.get(#param_name_str)
                        .ok_or_else(|| ::genai_rs::function_calling::FunctionError::ArgumentMismatch(format!("Missing required argument '{}'", #param_name_str)))
                        .and_then(|val| ::genai_rs::__private::serde_json::from_value(val.clone()).map_err(|e| {
                            ::genai_rs::function_calling::FunctionError::ArgumentMismatch(
                                format!("Failed to deserialize argument '{}': {}", #param_name_str, e)
                            )
                        }))?;
                });
            }
        }
    }

    let original_fn_ident = &func.sig.ident;
    let fn_call_args = quote! { #(#arg_names),* };

    let is_async_fn = func.sig.asyncness.is_some();
    let fn_call_expr = if is_async_fn {
        quote! { #original_fn_ident(#fn_call_args).await }
    } else {
        quote! { #original_fn_ident(#fn_call_args) }
    };

    let output = quote! {
        #func

        #[derive(Debug, Clone, Copy)]
        pub struct #callable_struct_name;

        impl #callable_struct_name {
            pub const fn new() -> Self {
                Self
            }
        }

        #[::genai_rs::__private::async_trait::async_trait]
        impl ::genai_rs::function_calling::CallableFunction for #callable_struct_name {
            fn declaration(&self) -> ::genai_rs::FunctionDeclaration {
                ::genai_rs::FunctionDeclaration::new(
                    #func_name.to_string(),
                    #func_description.to_string(),
                    ::genai_rs::FunctionParameters::new(
                        "object".to_string(),
                        #properties_tokens,
                        #required_field_tokens,
                    ),
                )
            }

            async fn call(&self, args: ::genai_rs::__private::serde_json::Value) -> Result<::genai_rs::__private::serde_json::Value, ::genai_rs::function_calling::FunctionError> {
                #(#arg_extraction_tokens)*

                let original_fn_result = #fn_call_expr;

                match ::genai_rs::__private::serde_json::to_value(original_fn_result) {
                    Ok(value_from_fn_result) => {
                        // If the value is already a JSON object, return it as is.
                        // Otherwise, wrap it in a {"result": ...} object.
                        if value_from_fn_result.is_object() {
                            Ok(value_from_fn_result)
                        } else {
                            Ok(::genai_rs::__private::serde_json::json!({ "result": value_from_fn_result }))
                        }
                    }
                    Err(e) => Err(::genai_rs::function_calling::FunctionError::ExecutionError(Box::new(e)))
                }
            }
        }

        pub fn #generated_fn_name() -> ::genai_rs::FunctionDeclaration {
             // Fully-qualified rather than method position: in method
             // position this needs `CallableFunction` in scope at every
             // expansion site, which is a requirement the caller cannot
             // see from the `#[tool]` attribute alone (#402).
             <#callable_struct_name as ::genai_rs::function_calling::CallableFunction>
                 ::declaration(&#callable_struct_name::new())
        }

        pub fn #generated_callable_factory_fn_name() -> Box<dyn ::genai_rs::function_calling::CallableFunction> {
            Box::new(#callable_struct_name::new())
        }

        ::genai_rs::function_calling::submit! {
            ::genai_rs::function_calling::CallableFunctionFactory::new(#generated_callable_factory_fn_name)
        }
    };

    output
}
