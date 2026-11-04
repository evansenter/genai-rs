//! The SSE parser: no panics, events independent of read boundaries, and a
//! canonical re-encoding that parses back to the same events. The first
//! byte sets how many read sizes follow; see `genai_rs::__fuzz::sse_parser`.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| genai_rs::__fuzz::sse_parser(data));
