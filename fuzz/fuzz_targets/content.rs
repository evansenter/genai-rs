//! `Content` deserialization: no panics, and a serialize/parse fixed point.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| genai_rs::__fuzz::content(data));
