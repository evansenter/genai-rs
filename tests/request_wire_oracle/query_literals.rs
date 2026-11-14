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
//! Every `.rs` file under `src/http/` is read, at any depth. Macro
//! arguments are read too; test code (a test-only file, or an item under a
//! test attribute or a `cfg` that needs `test`, as
//! `tests/common/source_scan.rs` reads them) and doc comments are left
//! out. The scan finds the usual ways to write a query, not every one: a
//! separator the code computes (`char::from(38)`) gets past it.

use std::path::Path;
use std::sync::LazyLock;

use syn::visit::{self, Visit};

use crate::live_verbs::source_scan::{self, is_test_gated};

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
        if !is_test_gated(&node.attrs) {
            visit::visit_item_mod(self, node);
        }
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if !is_test_gated(&node.attrs) {
            let outer = self.in_with_query;
            self.in_with_query = node.sig.ident == "with_query";
            visit::visit_item_fn(self, node);
            self.in_with_query = outer;
        }
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if !is_test_gated(&node.attrs) {
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

/// The library's source files under `dir`, at any depth
/// (`source_scan::rust_files`), parsed and keyed by their path relative to
/// `base` with `/` separators, without the test-only ones: a file a
/// test-gated `mod` declares, and every file below it
/// (`source_scan::test_modules`).
fn http_sources(base: &Path, dir: &Path) -> Vec<(String, syn::File)> {
    let parsed: Vec<(String, syn::File)> = source_scan::rust_files(dir)
        .iter()
        .map(|path| {
            let rel = path
                .strip_prefix(base)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            (rel, source_scan::parse_file(path))
        })
        .collect();
    let test_modules =
        source_scan::test_modules(parsed.iter().map(|(rel, file)| (rel.as_str(), file)));
    parsed
        .into_iter()
        .filter(|(rel, _)| !source_scan::is_test_only_path(rel, &test_modules))
        .collect()
}

/// [`query_keys_in`] for each of `files`, as `path: literal`.
fn query_keys_in_files(files: &[(String, syn::File)]) -> Vec<String> {
    files
        .iter()
        .flat_map(|(rel, file)| {
            query_keys_in(file)
                .into_iter()
                .map(move |literal| format!("{rel}: {literal}"))
        })
        .collect()
}

#[test]
fn query_keys_are_added_only_through_with_query() {
    let root = source_scan::root();
    let files = http_sources(&root, &root.join("src/http"));
    assert!(
        files.len() > 10,
        "read too few files: {:?}",
        files.iter().map(|(rel, _)| rel).collect::<Vec<_>>()
    );
    let found = query_keys_in_files(&files);
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
/// a variable and reqwest's own query builder; a test module's literal
/// (under `cfg(test)` or `cfg(all(test, unix))`), a doc comment, a path, a
/// parsed URL's query read, and `with_query`'s own separators are not.
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
        #[cfg(all(test, unix))]
        mod unix_tests {
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

/// A file nested below `src/http/` is read as one directly in it is, and
/// test code is left out wherever it is: a module under
/// `cfg(all(test, unix))` as one under `cfg(test)`, and a test-only file a
/// test-gated `mod` declares. One under `cfg(any(test, ..))` builds outside
/// tests, so it is read.
#[test]
fn every_library_file_under_src_http_is_read() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let http = base.path().join("http");
    let write = |rel: &str, text: &str| {
        let path = http.join(rel);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("create a directory");
        std::fs::write(path, text).expect("write a probe file");
    };
    let key = r#"pub fn url(u: &str) -> String { format!("{u}&chunked=true") }"#;
    write(
        "mod.rs",
        "mod files;\nmod flat;\nmod probe;\nmod probe_cfg;\n",
    );
    write("flat.rs", key);
    write("probe/nested.rs", key);
    write(
        "probe_cfg.rs",
        &format!(
            "#[cfg(all(test, unix))]\nmod tests {{ {key} }}\n\
             #[cfg(any(test, feature = \"x\"))]\nmod shipped {{ {key} }}\n"
        ),
    );
    write(
        "files.rs",
        "#[cfg(test)]\n#[path = \"files_tests.rs\"]\nmod tests;\n",
    );
    write("files_tests.rs", key);
    let files = http_sources(base.path(), &http);
    assert_eq!(
        files
            .iter()
            .map(|(rel, _)| rel.as_str())
            .collect::<Vec<_>>(),
        [
            "http/files.rs",
            "http/flat.rs",
            "http/mod.rs",
            "http/probe/nested.rs",
            "http/probe_cfg.rs",
        ]
    );
    assert_eq!(
        query_keys_in_files(&files),
        [
            r#"http/flat.rs: "{u}&chunked=true""#,
            r#"http/probe/nested.rs: "{u}&chunked=true""#,
            r#"http/probe_cfg.rs: "{u}&chunked=true""#,
        ]
    );
}
