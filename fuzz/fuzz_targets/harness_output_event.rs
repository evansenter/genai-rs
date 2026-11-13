//! The Antigravity harness's `OutputEvent` (the `antigravity` feature): no
//! panics, a serialize/parse round trip that keeps every field, and the
//! event written back as it was read.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| genai_rs::__fuzz::harness_output_event(data));
