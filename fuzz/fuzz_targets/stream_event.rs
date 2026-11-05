//! An SSE body of `InteractionStreamEvent`s through the whole stream
//! pipeline (dispatch, step accumulation, the stop at the terminal event):
//! no panics, and every `StreamEvent` round-trips through serde.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| genai_rs::__fuzz::stream_event(data));
