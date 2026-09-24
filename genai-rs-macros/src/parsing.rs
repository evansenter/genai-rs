//! Parsing utilities for the `#[tool]` procedural macro.
//!
//! This module handles parsing of:
//! - Macro attribute arguments (parameter descriptions and enum values)
//! - Function signatures and their doc comments

use proc_macro::TokenStream;
use quote::ToTokens;
use std::collections::HashMap;
use syn::parse::{Parse, ParseStream, Result as ParseResult};
use syn::{
    Attribute, Expr, ExprArray, ExprLit, Ident, ItemFn, Lit, LitStr, Meta, Token, parenthesized,
    punctuated::Punctuated,
};

/// Configuration for a single function parameter extracted from the macro attribute.
#[derive(Default, Debug, Clone)]
pub struct ParamConfig {
    /// Optional description for this parameter (used in function declaration).
    pub description: Option<String>,
    /// Optional list of allowed values for this parameter.
    pub enum_values: Option<Vec<serde_json::Value>>,
}

/// Parses individual parameter config like: `name(description = "...", enum_values = [...])`
struct SingleParamConfigInput {
    name: Ident,
    description: Option<LitStr>,
    enum_values: Option<ExprArray>,
}

impl Parse for SingleParamConfigInput {
    fn parse(input: ParseStream) -> ParseResult<Self> {
        let name: Ident = input.parse()?;
        let content;
        parenthesized!(content in input);

        let mut description: Option<LitStr> = None;
        let mut enum_values: Option<ExprArray> = None;

        let metas: Punctuated<Meta, Token![,]> =
            content.parse_terminated(Meta::parse, Token![,])?;

        for meta in metas {
            if let Meta::NameValue(nv) = meta {
                if nv.path.is_ident("description") {
                    if let Expr::Lit(ExprLit {
                        lit: Lit::Str(s), ..
                    }) = nv.value
                    {
                        description = Some(s);
                    } else {
                        return Err(syn::Error::new_spanned(
                            nv.value,
                            "Expected string literal for description",
                        ));
                    }
                } else if nv.path.is_ident("enum_values") {
                    if let Expr::Array(arr) = nv.value {
                        enum_values = Some(arr);
                    } else {
                        return Err(syn::Error::new_spanned(
                            nv.value,
                            "Expected array for enum_values",
                        ));
                    }
                } else {
                    return Err(syn::Error::new_spanned(
                        &nv.path,
                        format!(
                            "Unknown attribute '{}'. Valid attributes are: description, enum_values",
                            nv.path.to_token_stream()
                        ),
                    ));
                }
            }
        }

        Ok(Self {
            name,
            description,
            enum_values,
        })
    }
}

/// Parses the complete macro attribute: `param1(...), param2(...)`
struct AllParamsConfigInput {
    configs: Punctuated<SingleParamConfigInput, Token![,]>,
}

impl Parse for AllParamsConfigInput {
    fn parse(input: ParseStream) -> ParseResult<Self> {
        if input.is_empty() {
            Ok(Self {
                configs: Punctuated::new(),
            })
        } else {
            let configs = input.parse_terminated(SingleParamConfigInput::parse, Token![,])?;
            Ok(Self { configs })
        }
    }
}

/// The parsed output of the `#[tool]` macro containing the function and its parameter configs.
pub struct MacroInput {
    /// The function item the macro was applied to.
    pub func: ItemFn,
    /// Map of parameter name to its configuration (description, enum_values).
    pub param_configs: HashMap<String, ParamConfig>,
}

/// Parses both the macro attribute and the function item.
///
/// This is the main entry point for parsing the `#[tool]` macro input.
pub fn parse_input(attr_input: TokenStream, item: TokenStream) -> syn::Result<MacroInput> {
    let all_params_config = syn::parse::<AllParamsConfigInput>(attr_input)?;
    let func = syn::parse::<ItemFn>(item)?;

    let mut param_configs = HashMap::new();
    for config in all_params_config.configs {
        let name = config.name.to_string();
        let mut details = ParamConfig::default();

        if let Some(desc_lit) = config.description {
            details.description = Some(desc_lit.value());
        }

        if let Some(enums_array) = config.enum_values {
            let mut enums = Vec::new();
            for elem_expr in enums_array.elems {
                if let Expr::Lit(ExprLit {
                    lit: Lit::Str(s), ..
                }) = elem_expr
                {
                    enums.push(serde_json::Value::String(s.value()));
                } else {
                    return Err(syn::Error::new_spanned(
                        elem_expr,
                        format!("Enum values for param '{name}' must be string literals."),
                    ));
                }
            }
            if !enums.is_empty() {
                details.enum_values = Some(enums);
            }
        }

        param_configs.insert(name, details);
    }

    Ok(MacroInput {
        func,
        param_configs,
    })
}

/// Extracts `/// doc` comments from a function's attributes.
///
/// Doc comments are joined with newlines to form the function's description.
pub fn extract_doc_comments(attrs: &[Attribute]) -> String {
    attrs
        .iter()
        .filter_map(|attr| {
            if attr.path().is_ident("doc")
                && let Meta::NameValue(mnv) = &attr.meta
                && let Expr::Lit(ExprLit {
                    lit: Lit::Str(lit_str),
                    ..
                }) = &mnv.value
            {
                return Some(lit_str.value().trim().to_string());
            }
            None
        })
        .collect::<Vec<String>>()
        .join("\n")
}
