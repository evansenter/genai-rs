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
//! - [`step`], [`step_delta`], [`content`]: a value that deserializes reads
//!   back equal after one serialization, apart from two normalizations the
//!   crate makes on purpose (see `Normalize`), and a second write and read
//!   changes neither the wire form nor the value.

use crate::http::context::HttpContext;
use crate::http::interactions::interaction_events;
use crate::http::sse_parser::parse_sse_stream;
use crate::wire_streaming::InteractionStreamEvent;
use crate::{Annotation, Content, FunctionResultPayload, GenaiError, Step, StepDelta, StreamEvent};
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
fn outcomes(
    results: &[Result<serde_json::Value, GenaiError>],
) -> Vec<Result<&serde_json::Value, &'static str>> {
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
    assert_eq!(
        values, again,
        "the canonical re-encoding parsed differently"
    );
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

/// The differences a round trip makes on purpose. Every other field must
/// survive the first write unchanged.
///
/// - `Content::Text` leaves an empty annotation list unwritten, so
///   `Some(vec![])` reads back as `None`.
/// - An `Unknown` is written as its type tag merged with its data (see
///   [`unknown_read_back`]). Read from the wire, its data already holds that
///   tag, so nothing changes; an input with no string `type` is tagged
///   `"<missing type>"`, and that tag is in its data from then on.
trait Normalize {
    fn normalize(&mut self);
}

/// The data an `Unknown` reads back with: the documented wire form of
/// `serialize_unknown_merged` and the `Step`/`StepDelta` serializers, which
/// write `type` and then the data's other entries (non-object data under
/// `data`, null data not at all).
fn unknown_read_back(type_name: &str, data: &mut serde_json::Value) {
    let mut merged = serde_json::Map::new();
    merged.insert("type".to_string(), type_name.into());
    match std::mem::take(data) {
        serde_json::Value::Object(entries) => {
            merged.extend(entries.into_iter().filter(|(key, _)| key != "type"));
        }
        serde_json::Value::Null => {}
        other => {
            merged.insert("data".to_string(), other);
        }
    }
    *data = serde_json::Value::Object(merged);
}

impl Normalize for Annotation {
    fn normalize(&mut self) {
        if let Self::Unknown {
            annotation_type,
            data,
        } = self
        {
            unknown_read_back(annotation_type, data);
        }
    }
}

impl Normalize for Content {
    fn normalize(&mut self) {
        match self {
            Self::Text { annotations, .. } => {
                if annotations.as_ref().is_some_and(Vec::is_empty) {
                    *annotations = None;
                }
                annotations
                    .iter_mut()
                    .flatten()
                    .for_each(Normalize::normalize);
            }
            Self::Unknown { content_type, data } => unknown_read_back(content_type, data),
            _ => {}
        }
    }
}

impl Normalize for FunctionResultPayload {
    fn normalize(&mut self) {
        if let Self::Contents(contents) = self {
            contents.iter_mut().for_each(Normalize::normalize);
        }
    }
}

impl Normalize for Step {
    fn normalize(&mut self) {
        match self {
            Self::UserInput { content, .. }
            | Self::ModelOutput { content, .. }
            | Self::Thought {
                summary: content, ..
            } => content.iter_mut().for_each(Normalize::normalize),
            Self::FunctionResult { result, .. } | Self::McpServerToolResult { result, .. } => {
                result.normalize();
            }
            Self::Unknown { step_type, data } => unknown_read_back(step_type, data),
            _ => {}
        }
    }
}

impl Normalize for StepDelta {
    fn normalize(&mut self) {
        match self {
            Self::ThoughtSummary {
                content: Some(content),
                ..
            } => content.normalize(),
            Self::FunctionResult { result, .. } | Self::McpServerToolResult { result, .. } => {
                result.normalize();
            }
            Self::TextAnnotation { annotations, .. } => {
                annotations.iter_mut().for_each(Normalize::normalize);
            }
            Self::Unknown { delta_type, data } => unknown_read_back(delta_type, data),
            _ => {}
        }
    }
}

/// Deserializes `data` as a `T` and, if that works, checks the round trip:
/// what this crate writes parses back to the value it was written from
/// (after [`Normalize`]), and writing and reading that again changes
/// neither the wire form nor the value.
fn round_trip<T>(data: &[u8])
where
    T: DeserializeOwned + Serialize + PartialEq + std::fmt::Debug + Normalize,
{
    let Ok(mut first) = serde_json::from_slice::<T>(data) else {
        return;
    };
    let wire = serde_json::to_value(&first).expect("a deserialized value serializes");
    let second: T = serde_json::from_value(wire.clone())
        .unwrap_or_else(|e| panic!("a serialized value must parse: {e}\n{wire}"));
    first.normalize();
    assert_eq!(
        first, second,
        "a field was lost or changed on write: {wire}"
    );
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
