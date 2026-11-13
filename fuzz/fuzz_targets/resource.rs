//! The resources read from the wire and every list page: no panics, and a
//! serialize/parse round trip that keeps every field, for each type the
//! input reads as. An `EnvironmentSpec` and an `Agent` also write back the
//! JSON they read.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| genai_rs::__fuzz::resource(data));
