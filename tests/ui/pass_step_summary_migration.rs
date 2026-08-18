//! `StepSummary` is `#[non_exhaustive]`, and this pins the migration the
//! CHANGELOG documents for it.
//!
//! The attribute is inert inside `genai-rs`, so no unit test can cover this:
//! both in-tree literals use `..Default::default()`, which is exactly the
//! idiom external crates *lose*. trybuild builds this as its own crate
//! against genai-rs, and this is the only out-of-crate site in the repo that
//! constructs `StepSummary` — making it the cheapest place to hold the
//! documented path.
//!
//! What it would catch: dropping `Default` from `StepSummary`, or making a
//! counter non-public. Both would compile fine in-crate and break every
//! downstream caller following the migration note.
//!
//! What it would *not* catch is the attribute itself. `#[non_exhaustive]`
//! only ever removes syntax from a downstream caller, so a fixture written in
//! the surviving subset compiles identically either way — delete the
//! attribute and this file stays green. That half is
//! `fail_step_summary_struct_literal.rs`.

fn main() {
    // The documented route: `default()` then assign. Struct-literal and
    // `..Default::default()` syntax are both unavailable here — that is the
    // point of the attribute, and why this file cannot use them.
    let mut summary = genai_rs::StepSummary::default();
    summary.tool_call_count = 1;
    summary.function_call_count = 2;
    assert_eq!(summary.tool_call_count, 1);
    assert_eq!(summary.function_call_count, 2);
}
