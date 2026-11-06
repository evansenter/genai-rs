//! `#[tool]` must expand to documented items: everything it generates is
//! public, and a crate that denies `missing_docs` used to fail on the
//! callable struct, its `new`, and the declaration and factory functions.

#![deny(missing_docs)]

use genai_rs_macros::tool;

/// Returns the length of `text`.
#[tool]
pub fn text_length(text: String) -> usize {
    text.len()
}

fn main() {
    let declaration = text_length_declaration();
    assert_eq!(declaration.name(), "text_length");
    let _callable = TextLengthCallable::new();
    let _boxed = text_length_callable_factory();
}
