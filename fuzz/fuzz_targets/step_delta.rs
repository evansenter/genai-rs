//! `StepDelta` deserialization: no panics, and a serialize/parse round trip that keeps every field.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| genai_rs::__fuzz::step_delta(data));
