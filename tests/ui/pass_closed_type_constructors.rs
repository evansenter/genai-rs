//! Constructors that exist *because* their type is `#[non_exhaustive]`.
//!
//! Closing a struct takes struct-literal syntax away from downstream crates.
//! For `ModalityTokens` that left serde as the only way to build one, so
//! `ModalityTokens::new()` was added; for the read-write resources,
//! `docs/ENUM_WIRE_FORMATS.md` argues closing them costs nothing *because*
//! constructors like `Agent::new()` cover the sending side.
//!
//! Neither claim can be checked in-crate, where the attribute is inert.
//! trybuild builds this as its own crate, so removing or narrowing either
//! constructor fails here, on every PR, rather than on the next weekly
//! `consumer-crate` run.

fn main() {
    let modality = genai_rs::ModalityTokens::new("text", 1);
    assert_eq!(modality.tokens, 1);

    let agent = genai_rs::Agent::new("agents/x");
    assert_eq!(agent.id.as_deref(), Some("agents/x"));
}
