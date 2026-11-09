//! Every query key the crate sends goes through `with_query`
//! (`src/http/common.rs`), whose values `query_options` traces to the
//! options that set them, and which the query test sees on the wire. A key
//! written into a URL string instead (`format!("{url}&chunked=true")`)
//! reaches the wire from a verb the query test may never call with it set,
//! so `src/http/`'s library code holds no string literal with a query key
//! in it: `?key=` or `&key=`, the key spelled out or interpolated
//! (`?{key}=`), in a macro's arguments too. Test modules and doc comments
//! are left out.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use syn::visit::{self, Visit};

/// Whether `attrs` mark test-only code: `#[test]`, `#[tokio::test]` or
/// `#[cfg(test)]`.
fn is_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        let path = attr.path();
        path.segments.last().is_some_and(|s| s.ident == "test")
            || (path.is_ident("cfg")
                && attr
                    .parse_args::<syn::Meta>()
                    .is_ok_and(|meta| meta.path().is_ident("test")))
    })
}

/// The expressions a macro's tokens hold, when they parse as expressions
/// separated by `,` (`format!(..)`, `concat!(..)`, `write!(..)`); nothing
/// for a macro whose arguments are not Rust expressions.
fn macro_exprs(mac: &syn::Macro) -> Vec<syn::Expr> {
    mac.parse_body_with(syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated)
        .map(|exprs| exprs.into_iter().collect())
        .unwrap_or_default()
}

/// The string literals holding a query key, as `"literal"`.
#[derive(Default)]
struct Literals {
    found: Vec<String>,
}

impl<'ast> Visit<'ast> for Literals {
    /// Doc comments name endpoints with their query strings; no attribute
    /// builds a URL.
    fn visit_attribute(&mut self, _: &'ast syn::Attribute) {}

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if !is_test(&node.attrs) {
            visit::visit_item_mod(self, node);
        }
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if !is_test(&node.attrs) {
            visit::visit_item_fn(self, node);
        }
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if !is_test(&node.attrs) {
            visit::visit_impl_item_fn(self, node);
        }
    }

    fn visit_lit_str(&mut self, node: &'ast syn::LitStr) {
        let text = node.value();
        if holds_query_key(&text) {
            self.found.push(format!("{text:?}"));
        }
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        for expr in macro_exprs(node) {
            self.visit_expr(&expr);
        }
    }
}

/// Whether `text` holds `?key=` or `&key=`, the key an identifier or a
/// `{..}` format argument.
fn holds_query_key(text: &str) -> bool {
    static PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"[?&](?:[A-Za-z_][A-Za-z0-9_.]*|\{[^{}]*\})=").expect("a valid pattern")
    });
    PATTERN.is_match(text)
}

/// Every string literal holding a query key in `file`'s library code.
fn query_keys_in(file: &syn::File) -> Vec<String> {
    let mut literals = Literals::default();
    literals.visit_file(file);
    literals.found
}

/// `src/http/`'s source files.
fn http_sources() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/http");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .collect();
    paths.sort();
    paths
}

#[test]
fn query_keys_are_added_only_through_with_query() {
    let paths = http_sources();
    assert!(paths.len() > 10, "read too few files: {paths:?}");
    let mut found = Vec::new();
    for path in &paths {
        let source = std::fs::read_to_string(path).expect("read a source file");
        let file =
            syn::parse_file(&source).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        found.extend(
            query_keys_in(&file)
                .into_iter()
                .map(|literal| format!("src/http/{name}: {literal}")),
        );
    }
    assert!(
        found.is_empty(),
        "query keys written into a string, out of the query checks' sight (add each \
         through `with_query`, which the query test traces and checks):\n  {}",
        found.join("\n  ")
    );
}

/// The verifier's mutation is caught, in a `format!` and in a bare literal,
/// with the key spelled out or interpolated; a test module's literal, a doc
/// comment, a path, and `with_query`'s own separators are not.
#[test]
fn a_query_key_in_a_string_is_found() {
    let file: syn::File = syn::parse_quote! {
        /// Starts an upload (`PUT /upload/v1beta/environments/{id}/files/{path}?overwrite=true`).
        fn start_upload(start_url: &str) -> String {
            let url = format!("{start_url}&chunked=true");
            let other = format!("{}?{}={}", base, key, value);
            tracing::debug!("Uploading to {url}");
            url.push(if url.contains('?') { '&' } else { '?' });
            let path = format!("{}:rotateSigningSecret", webhook_url(ctx, id));
            const SUFFIX: &str = "?alt=sse";
            format!("{}={}", key, value)
        }
        #[cfg(test)]
        mod tests {
            fn url() { let _ = format!("{base}?pageSize=10"); }
        }
        #[test]
        fn alone() { let _ = "x?alt=media"; }
    };
    assert_eq!(
        query_keys_in(&file),
        [
            r#""{start_url}&chunked=true""#,
            r#""{}?{}={}""#,
            r#""?alt=sse""#,
        ]
    );
}
