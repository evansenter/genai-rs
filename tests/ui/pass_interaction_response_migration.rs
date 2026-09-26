//! `InteractionResponse` is `#[non_exhaustive]`, and this pins the migration
//! `docs/ENUM_WIRE_FORMATS.md` documents for it: `default()`, then assign.
//!
//! The attribute is inert inside `genai-rs`, so no unit test can cover this.
//! trybuild builds this as its own crate, which is where the attribute bites.
//!
//! What it would catch: dropping `Default` from `InteractionResponse`, or
//! making `id` non-public. Both compile fine in-crate and break every
//! downstream caller following the documented route.
//!
//! The `consumer-crate` workflow asserts the same thing from a fresh
//! dependency resolve, but only weekly and on PRs touching the macro or the
//! README. This file runs on every PR.

fn main() {
    let mut response = genai_rs::InteractionResponse::default();
    response.id = Some("x".into());
    assert_eq!(response.id.as_deref(), Some("x"));
}
