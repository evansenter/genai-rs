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
//! - [`step`], [`step_delta`], [`content`], and the types read from the
//!   wire beside them, [`interaction_response`], [`resource`] (the resource
//!   types and every list page) and, with the `antigravity` feature,
//!   `harness_output_event`: a value that deserializes reads back equal
//!   after one serialization, apart from the normalizations the crate makes
//!   on purpose (see `Normalize`), and a second write and read changes
//!   neither the wire form nor the value. A type that promises to write
//!   back what it read is also checked against its input (see `WireForm`,
//!   D-NEXT-J7), so a value lost on the first read is caught too.
//!
//! A failed check panics, as a test's does: that is how `cargo fuzz` and
//! `fuzzing_tests.rs` report it.
#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a fuzz entry point reports a failed check by panicking, as a test does"
)]

use crate::http::context::HttpContext;
use crate::http::interactions::interaction_events;
use crate::http::sse_parser::parse_sse_stream;
use crate::wire_streaming::InteractionStreamEvent;
use crate::{
    Agent, AgentListResponse, Annotation, CodeExecutionOutput, Content, CredentialListResponse,
    Environment, EnvironmentFileListResponse, EnvironmentListResponse, EnvironmentSpec,
    FileListResponse, FileSearchDocumentListResponse, FileSearchStoreListResponse,
    FunctionResultPayload, GenaiError, InteractionInput, InteractionRequest, InteractionResponse,
    Step, StepDelta, StreamEvent, Tool, Trigger, TriggerExecutionListResponse, TriggerListResponse,
    VoiceListResponse, Webhook, WebhookListResponse,
};
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
            Err(GenaiError::MalformedResponse(_)) => Err("parse"),
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
pub(crate) trait Normalize {
    /// Applies the differences the first write makes on purpose.
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
            Self::CodeExecutionResult { result, .. } => result.normalize(),
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

impl Normalize for CodeExecutionOutput {
    fn normalize(&mut self) {
        if let Self::Contents(contents) = self {
            contents.iter_mut().for_each(Normalize::normalize);
        }
    }
}

impl Normalize for Tool {
    fn normalize(&mut self) {
        if let Self::Unknown { tool_type, data } = self {
            unknown_read_back(tool_type, data);
        }
    }
}

impl<T: Normalize> Normalize for Vec<T> {
    fn normalize(&mut self) {
        self.iter_mut().for_each(Normalize::normalize);
    }
}

impl<T: Normalize> Normalize for Option<T> {
    fn normalize(&mut self) {
        if let Some(value) = self {
            value.normalize();
        }
    }
}

impl Normalize for InteractionInput {
    fn normalize(&mut self) {
        match self {
            Self::Content(contents) => contents.normalize(),
            Self::Steps(steps) => steps.normalize(),
            Self::Text(_) => {}
        }
    }
}

impl Normalize for InteractionResponse {
    fn normalize(&mut self) {
        self.input.normalize();
        self.steps.normalize();
        self.tools.normalize();
    }
}

impl Normalize for InteractionRequest {
    fn normalize(&mut self) {
        // A request writes `Content` input as one `user_input` step, which
        // reads back as `Steps` (`InteractionRequest::input`).
        if let Some(InteractionInput::Content(contents)) = &mut self.input {
            let step = Step::user_input(std::mem::take(contents));
            self.input = Some(InteractionInput::Steps(vec![step]));
        }
        self.input.normalize();
        self.tools.normalize();
    }
}

impl Normalize for Agent {
    fn normalize(&mut self) {
        self.tools.normalize();
    }
}

impl Normalize for Trigger {
    fn normalize(&mut self) {
        self.interaction.normalize();
    }
}

impl Normalize for AgentListResponse {
    fn normalize(&mut self) {
        self.agents.normalize();
    }
}

impl Normalize for TriggerListResponse {
    fn normalize(&mut self) {
        self.triggers.normalize();
    }
}

/// The types whose first write changes nothing on purpose.
macro_rules! no_normalization {
    ($($ty:ty),+ $(,)?) => {
        $(impl Normalize for $ty {
            fn normalize(&mut self) {}
        })+
    };
}

no_normalization!(
    Webhook,
    Environment,
    EnvironmentSpec,
    CredentialListResponse,
    FilePage,
    WebhookListResponse,
    VoiceListResponse,
    EnvironmentListResponse,
    TriggerExecutionListResponse,
    EnvironmentFileListResponse,
    FileSearchStoreListResponse,
    FileSearchDocumentListResponse,
);

/// What a type promises about the JSON it writes back, beyond a stable
/// round trip (D-NEXT-J7).
trait WireForm {
    /// Whether the type speaks proto3 JSON: [`wire::canonical`] then reads
    /// an int64 string as its number, and a field written at its default
    /// as absent, at the fields where the harness types do so.
    const PROTO3: bool = false;

    /// The JSON this value, read from `input`, writes back, when the type
    /// promises to write back what it read up to its documented
    /// normalizations; `None` for a type that normalizes more than this
    /// check can follow, whose round trip is checked on the value alone.
    fn faithful_wire(&self, input: &serde_json::Value) -> Option<serde_json::Value> {
        let _ = input;
        None
    }
}

/// The types that normalize more than [`WireForm::faithful_wire`] can follow:
/// a missing or non-string type tag becomes `"<missing type>"` on a tagged
/// union, and the resources read their int64s, timestamps and list elements
/// leniently, dropping what does not read (D-NEXT-J7).
macro_rules! normalizing {
    ($($ty:ty),+ $(,)?) => {
        $(impl WireForm for $ty {})+
    };
}

normalizing!(
    Step,
    StepDelta,
    Content,
    InteractionResponse,
    Trigger,
    Webhook,
    Environment,
    AgentListResponse,
    CredentialListResponse,
    FilePage,
    WebhookListResponse,
    VoiceListResponse,
    EnvironmentListResponse,
    TriggerListResponse,
    TriggerExecutionListResponse,
    EnvironmentFileListResponse,
    FileSearchStoreListResponse,
    FileSearchDocumentListResponse,
);

impl WireForm for EnvironmentSpec {
    fn faithful_wire(&self, input: &serde_json::Value) -> Option<serde_json::Value> {
        Some(wire::environment_spec(self, input))
    }
}

impl WireForm for Agent {
    fn faithful_wire(&self, input: &serde_json::Value) -> Option<serde_json::Value> {
        Some(wire::agent(self, input))
    }
}

/// `FileListResponse`, which has no `PartialEq`, compared field by field.
#[derive(Debug, serde::Deserialize, Serialize)]
#[serde(transparent)]
struct FilePage(FileListResponse);

impl PartialEq for FilePage {
    fn eq(&self, other: &Self) -> bool {
        let (this, that) = (&self.0, &other.0);
        this.files == that.files
            && this.next_page_token == that.next_page_token
            && this.extra == that.extra
    }
}

/// Deserializes `data` as a `T` and, if that works, checks the round trip:
/// what this crate writes is the input where the type promises that
/// ([`WireForm`]), it parses back to the value it was written from (after
/// [`Normalize`]), and writing and reading that again changes neither the
/// wire form nor the value.
fn round_trip<T>(data: &[u8])
where
    T: DeserializeOwned + Serialize + PartialEq + std::fmt::Debug + Normalize + WireForm,
{
    let Ok(mut first) = serde_json::from_slice::<T>(data) else {
        return;
    };
    let wire = serde_json::to_value(&first).expect("a deserialized value serializes");
    let input: serde_json::Value =
        serde_json::from_slice(data).expect("what deserialized as a type is JSON");
    if let Some(expected) = first.faithful_wire(&input) {
        assert_eq!(
            wire::canonical(&wire, T::PROTO3),
            wire::canonical(&expected, T::PROTO3),
            "the written JSON is not what was read: {input}\nwritten: {wire}"
        );
    }
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

/// `InteractionResponse` deserialization: no panics, and a stable round
/// trip.
pub fn interaction_response(data: &[u8]) {
    round_trip::<InteractionResponse>(data);
}

/// The resources read from the wire, each list page included: no panics,
/// and a stable round trip for every type the input reads as. An
/// `EnvironmentSpec` and an `Agent` also write back what they read.
pub fn resource(data: &[u8]) {
    round_trip::<Agent>(data);
    round_trip::<Trigger>(data);
    round_trip::<Webhook>(data);
    round_trip::<Environment>(data);
    round_trip::<EnvironmentSpec>(data);
    round_trip::<AgentListResponse>(data);
    round_trip::<CredentialListResponse>(data);
    round_trip::<FilePage>(data);
    round_trip::<WebhookListResponse>(data);
    round_trip::<VoiceListResponse>(data);
    round_trip::<EnvironmentListResponse>(data);
    round_trip::<TriggerListResponse>(data);
    round_trip::<TriggerExecutionListResponse>(data);
    round_trip::<EnvironmentFileListResponse>(data);
    round_trip::<FileSearchStoreListResponse>(data);
    round_trip::<FileSearchDocumentListResponse>(data);
}

#[cfg(feature = "antigravity")]
mod harness {
    use super::{Normalize, WireForm, round_trip, wire};
    use crate::antigravity::protocol::OutputEvent;

    impl Normalize for OutputEvent {
        fn normalize(&mut self) {}
    }

    impl WireForm for OutputEvent {
        const PROTO3: bool = true;

        fn faithful_wire(&self, input: &serde_json::Value) -> Option<serde_json::Value> {
            Some(wire::output_event(input))
        }
    }

    /// The Antigravity harness's `OutputEvent`, as the session reads every
    /// frame: no panics, a stable round trip, and the event written back as
    /// it was read.
    pub fn harness_output_event(data: &[u8]) {
        round_trip::<OutputEvent>(data);
    }
}

#[cfg(feature = "antigravity")]
pub use harness::harness_output_event;

#[path = "fuzzing_wire.rs"]
mod wire;

#[cfg(test)]
#[path = "fuzzing_tests.rs"]
mod tests;
