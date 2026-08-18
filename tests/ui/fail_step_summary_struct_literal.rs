//! The complement to `pass_step_summary_migration.rs`, and the half that
//! actually pins the attribute.
//!
//! `#[non_exhaustive]` only ever *removes* syntax from a downstream caller, so
//! any fixture written in the surviving subset — including the `pass_` one
//! next to this — compiles identically with the attribute present or absent.
//! Delete it from `StepSummary` and that file stays green.
//!
//! Functional update from `Default` is the form the migration note replaces,
//! and it is deliberately the only one here. A field-by-field literal is
//! rejected by the same `E0639`, but with the attribute gone it fails anew on
//! `E0063` for every field it omits — so a fixture carrying one would go red
//! on a *stderr mismatch*, leaving the signal riding on rustc's diagnostic
//! wording. This form compiles outright once the attribute goes, which
//! trybuild reports directly: a `fail_` fixture that builds is a failed test.

fn main() {
    let _update = genai_rs::StepSummary {
        tool_call_count: 1,
        ..Default::default()
    };
}
