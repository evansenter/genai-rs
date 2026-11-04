//! Entry points for the `cargo fuzz` targets in `fuzz/`, one per target.
//!
//! Compiled only under `cfg(fuzzing)` (which `cargo fuzz` sets) and in this
//! crate's own tests, where `fuzzing_tests.rs` replays the inputs that once
//! failed. **Not a public API.**
//!
//! Each entry point must not panic on any input, and checks a round trip
//! where the input is valid:
//!
//! - [`sse_parser`]: the events don't depend on where the body is split into
//!   reads, and re-encoding them as plain `data:` events parses back to the
//!   same values.
//! - [`stream_event`]: a body of `InteractionStreamEvent`s runs through the
//!   whole stream pipeline (dispatch, step accumulation, the terminal stop),
//!   and every `StreamEvent` it yields serializes and parses back unchanged.
//! - [`step`], [`step_delta`], [`content`]: a value that deserializes
//!   reaches a fixed point after one serialization: it may be normalized
//!   once, but writing and reading it again changes nothing.

use crate::http::context::HttpContext;
use crate::http::interactions::interaction_events;
use crate::http::sse_parser::parse_sse_stream;
use crate::wire_streaming::InteractionStreamEvent;
use crate::{Content, GenaiError, Step, StepDelta, StreamEvent};
use bytes::Bytes;
use futures_util::{FutureExt, Stream, StreamExt, stream};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::sync::OnceLock;

/// One context for every run: building a reqwest client per input would
/// dominate the time spent. Nothing in it is used to send.
fn ctx() -> &'static HttpContext {
    static CTX: OnceLock<HttpContext> = OnceLock::new();
    CTX.get_or_init(|| HttpContext::new(reqwest::Client::new(), String::new(), Vec::new()))
}

/// Collects a stream that never waits (every source here is in memory).
fn collect<S: Stream>(stream: S) -> Vec<S::Item> {
    stream
        .collect()
        .now_or_never()
        .expect("an in-memory stream never waits")
}

/// Splits `data` into a read-size header and a body: each header byte is
/// one read's length (plus one), cycled until the body is used up.
fn split_reads(data: &[u8]) -> (Vec<Bytes>, &[u8]) {
    let header_len = data.first().map_or(0, |first| usize::from(first % 8));
    let header_len = header_len.min(data.len().saturating_sub(1));
    let (header, body) = data
        .get(1..=header_len)
        .map_or((&[][..], data), |header| (header, &data[header_len + 1..]));
    let mut reads = Vec::new();
    let mut rest = body;
    let mut sizes = header.iter().cycle();
    while !rest.is_empty() {
        let size = sizes
            .next()
            .map_or(rest.len(), |size| usize::from(*size) + 1)
            .min(rest.len());
        let (read, tail) = rest.split_at(size);
        reads.push(Bytes::copy_from_slice(read));
        rest = tail;
    }
    (reads, body)
}

fn parse_reads<T>(reads: Vec<Bytes>) -> Vec<Result<T, GenaiError>>
where
    T: DeserializeOwned + Send + 'static,
{
    let chunks = stream::iter(reads.into_iter().map(Ok::<_, reqwest::Error>));
    collect(parse_sse_stream::<T>(chunks, ctx(), 0))
}

/// What a parse produced, comparable across runs: each event's value, or
/// the kind of error.
fn outcomes(results: &[Result<serde_json::Value, GenaiError>]) -> Vec<Result<&serde_json::Value, &'static str>> {
    results
        .iter()
        .map(|result| match result {
            Ok(value) => Ok(value),
            Err(GenaiError::Parse(_)) => Err("parse"),
            Err(GenaiError::Utf8(_)) => Err("utf8"),
            Err(GenaiError::StreamInterrupted { .. }) => Err("interrupted"),
            Err(other) => panic!("the SSE parser returned an unexpected error: {other:?}"),
        })
        .collect()
}

/// The SSE parser: no panics, events independent of read boundaries, and a
/// canonical re-encoding that parses back to the same events.
pub fn sse_parser(data: &[u8]) {
    let (reads, body) = split_reads(data);
    let split = parse_reads::<serde_json::Value>(reads);
    let whole = parse_reads::<serde_json::Value>(vec![Bytes::copy_from_slice(body)]);
    assert_eq!(
        outcomes(&split),
        outcomes(&whole),
        "read boundaries changed the events"
    );

    let values: Vec<&serde_json::Value> = whole.iter().filter_map(|r| r.as_ref().ok()).collect();
    let mut canonical = Vec::new();
    for value in &values {
        canonical.extend_from_slice(b"data: ");
        canonical.extend_from_slice(&serde_json::to_vec(value).expect("a Value serializes"));
        canonical.extend_from_slice(b"\n\n");
    }
    let again = parse_reads::<serde_json::Value>(vec![Bytes::from(canonical)]);
    let again: Vec<&serde_json::Value> = again
        .iter()
        .map(|r| r.as_ref().expect("a canonical event parses"))
        .collect();
    assert_eq!(values, again, "the canonical re-encoding parsed differently");
}

/// The stream pipeline on an SSE body of `InteractionStreamEvent`s: no
/// panics, and every `StreamEvent` it yields round-trips through serde.
pub fn stream_event(data: &[u8]) {
    let chunks = stream::iter([Ok::<_, reqwest::Error>(Bytes::copy_from_slice(data))]);
    let parsed = parse_sse_stream::<InteractionStreamEvent>(chunks, ctx(), 0);
    for event in collect(interaction_events(parsed)).into_iter().flatten() {
        let value = serde_json::to_value(&event).expect("a StreamEvent serializes");
        let back: StreamEvent = serde_json::from_value(value.clone())
            .unwrap_or_else(|e| panic!("a serialized StreamEvent must parse: {e}\n{value}"));
        let again = serde_json::to_value(&back).expect("a StreamEvent serializes");
        assert_eq!(value, again, "StreamEvent changed on a round trip");
    }
}

/// Deserializes `data` as a `T` and, if that works, checks the round trip.
///
/// The first serialization may normalize (an empty annotation list is
/// omitted, for one), so the property is a fixed point: what this crate
/// writes parses back, and writing that again changes neither the wire form
/// nor the value.
fn round_trip<T>(data: &[u8])
where
    T: DeserializeOwned + Serialize + PartialEq + std::fmt::Debug,
{
    let Ok(first) = serde_json::from_slice::<T>(data) else {
        return;
    };
    let wire = serde_json::to_value(&first).expect("a deserialized value serializes");
    let second: T = serde_json::from_value(wire.clone())
        .unwrap_or_else(|e| panic!("a serialized value must parse: {e}\n{wire}"));
    let wire_again = serde_json::to_value(&second).expect("a deserialized value serializes");
    assert_eq!(wire, wire_again, "the wire form changed on a round trip");
    let third: T = serde_json::from_value(wire_again)
        .unwrap_or_else(|e| panic!("a serialized value must parse: {e}\n{wire}"));
    assert_eq!(second, third, "the value changed on a round trip: {wire}");
}

/// `Step` deserialization: no panics, and a stable round trip.
pub fn step(data: &[u8]) {
    round_trip::<Step>(data);
}

/// `StepDelta` deserialization: no panics, and a stable round trip.
pub fn step_delta(data: &[u8]) {
    round_trip::<StepDelta>(data);
}

/// `Content` deserialization: no panics, and a stable round trip.
pub fn content(data: &[u8]) {
    round_trip::<Content>(data);
}

#[cfg(test)]
#[path = "fuzzing_tests.rs"]
mod tests;
