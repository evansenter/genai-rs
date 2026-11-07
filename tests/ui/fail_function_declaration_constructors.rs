//! `FunctionDeclaration::new` and `FunctionParameters::new` are
//! crate-private. `#[tool]` reaches them through
//! `genai_rs::__private::function_declaration`, which has no semver
//! guarantee, and everyone else builds a declaration with
//! `FunctionDeclaration::builder`. They were `#[doc(hidden)] pub`, so a
//! consumer could call them and come to depend on the macro's plumbing.
//!
//! trybuild builds this as its own crate, which is where privacy applies.

use genai_rs::{FunctionDeclaration, FunctionParameters};

fn main() {
    let parameters = FunctionParameters::new("object", Default::default(), Vec::new());
    let _declaration = FunctionDeclaration::new("name", "description", parameters);
}
