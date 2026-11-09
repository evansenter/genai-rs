//! Query keys go on the wire through `with_query` (`src/http/common.rs`),
//! whose values `query_options` traces to the options that set them, and
//! which the query test sees on the wire. A key added any other way
//! (`format!("{url}&chunked=true")`) reaches the wire from a verb the query
//! test may never call with it set, so `src/http/`'s library code holds,
//! outside `with_query` itself:
//!
//! - no string literal with a query key in it: `?key=` or `&key=`, the key
//!   spelled out or interpolated (`?{key}=`);
//! - no query separator, which a key written without one would need: a
//!   `'?'` or `'&'` character, or a string ending in `?` or `&`
//!   (`url.push('&'); url.push_str("chunked=true")`);
//! - no other query builder: reqwest's `.query(..)`, `Url::set_query` or
//!   `Url::query_pairs_mut`.
//!
//! Macro arguments are read too; test modules and doc comments are left
//! out. The scan finds the usual ways to write a query, not every one: a
//! separator the code computes (`char::from(38)`) gets past it.

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

/// The literals and calls that add to a query string other than through
/// `with_query`: a literal as Rust spells it, a call as `.method(..)`.
#[derive(Default)]
struct Literals {
    found: Vec<String>,
    /// Inside `with_query`, whose separators are the sanctioned ones.
    in_with_query: bool,
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
            let outer = self.in_with_query;
            self.in_with_query = node.sig.ident == "with_query";
            visit::visit_item_fn(self, node);
            self.in_with_query = outer;
        }
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if !is_test(&node.attrs) {
            visit::visit_impl_item_fn(self, node);
        }
    }

    fn visit_lit_str(&mut self, node: &'ast syn::LitStr) {
        let text = node.value();
        let separator = !self.in_with_query && (text.ends_with('?') || text.ends_with('&'));
        if holds_query_key(&text) || separator {
            self.found.push(format!("{text:?}"));
        }
    }

    fn visit_lit_char(&mut self, node: &'ast syn::LitChar) {
        let c = node.value();
        if !self.in_with_query && (c == '?' || c == '&') {
            self.found.push(format!("{c:?}"));
        }
    }

    /// reqwest's `RequestBuilder::query` and `url`'s query setters; a
    /// `.query()` that takes nothing reads a parsed URL's query.
    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let method = node.method.to_string();
        let builds = match method.as_str() {
            "query" => !node.args.is_empty(),
            "set_query" | "query_pairs_mut" => true,
            _ => false,
        };
        if builds {
            self.found.push(format!(".{method}(..)"));
        }
        visit::visit_expr_method_call(self, node);
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

/// Every literal and call in `file`'s library code that adds to a query
/// string other than through `with_query`.
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
        "query keys or separators written outside `with_query`, out of the query checks' \
         sight (add each key through `with_query`, which the query test traces and \
         checks):\n  {}",
        found.join("\n  ")
    );
}

/// The verifier's mutation is caught, in a `format!` and in a bare literal,
/// with the key spelled out or interpolated, and so are a separator held in
/// a variable and reqwest's own query builder; a test module's literal, a
/// doc comment, a path, a parsed URL's query read, and `with_query`'s own
/// separators are not.
#[test]
fn a_query_key_in_a_string_is_found() {
    let file: syn::File = syn::parse_quote! {
        /// Starts an upload (`PUT /upload/v1beta/environments/{id}/files/{path}?overwrite=true`).
        fn start_upload(start_url: &str) -> String {
            let url = format!("{start_url}&chunked=true");
            let other = format!("{}?{}={}", base, key, value);
            tracing::debug!("Uploading to {url}");
            let path = format!("{}:rotateSigningSecret", webhook_url(ctx, id));
            const SUFFIX: &str = "?alt=sse";
            format!("{}={}", key, value)
        }
        fn with_separator(mut url: String) -> String {
            let sep = if url.contains('?') { '&' } else { '?' };
            let joined = format!("{url}{sep}chunked=true");
            url.push_str(concat!("{url}", "&"));
            client.put(&url).query(&[("chunked", "true")]);
            parsed.query().is_none()
        }
        fn with_query(mut url: String, pairs: Vec<String>) -> String {
            url.push(if url.contains('?') { '&' } else { '?' });
            url.push_str(&pairs.join("&"));
            url
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
            "'?'",
            "'&'",
            "'?'",
            r#""&""#,
            ".query(..)",
        ]
    );
}
