//! `#[tool]` must expand with no imports beyond the macro itself.
//!
//! Deliberately absent: `use genai_rs::CallableFunction;`, and any mention
//! of `async_trait` or `serde_json`. Before #402 the generated code named
//! `::async_trait` and `::serde_json`, which resolve in the *consumer's*
//! dependency graph, and called `.declaration()` in method position, which
//! needs the trait in scope. All three failed here.
//!
//! trybuild builds this as its own crate against genai-rs's dev-dependency
//! graph, so it catches the trait-import requirement directly. It cannot
//! catch a missing *dependency*, since the workspace provides both — the
//! `consumer-crate` workflow covers that half.

use genai_rs_macros::tool;

/// Adds two numbers.
///
/// # Arguments
/// * `a` - The first number
/// * `b` - The second number
#[tool]
async fn add(a: i64, b: i64) -> Result<i64, String> {
    Ok(a + b)
}

fn main() {
    // Both generated entry points, exercised without a trait import.
    let declaration = add_declaration();
    assert_eq!(declaration.name(), "add");
    let _callable = add_callable_factory();
}
