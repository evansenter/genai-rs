//! Consuming a stream (or a paged listing) to its end in a live test.

use futures_util::StreamExt;
use genai_rs::{
    AutoFunctionStreamChunk, AutoFunctionStreamEvent, GenaiError, InteractionResponse, StreamChunk,
    StreamEvent,
};

// =============================================================================
// Streaming Utilities
// =============================================================================

/// What a fully consumed stream produced.
#[derive(Debug)]
pub struct StreamResult {
    /// Number of `StepDelta` chunks received.
    pub delta_count: usize,
    /// Text concatenated from text deltas.
    pub collected_text: String,
    /// The `Completed` response, if the stream sent one.
    pub final_response: Option<InteractionResponse>,
    /// A function-call step started or an arguments delta arrived.
    pub saw_function_call: bool,
    /// A thought step started, or a thought summary/signature delta arrived.
    pub saw_thought: bool,
    /// Every `event_id` seen, in order.
    pub event_ids: Vec<String>,
    /// For each entry of `event_ids`, the length of `collected_text` once
    /// that event was applied: the text a resume from that id should skip.
    pub event_text_offsets: Vec<usize>,
    /// Event types the crate didn't recognize (`StreamChunk::Unknown`), in
    /// order, without duplicates: API drift worth a look.
    pub unknown_event_types: Vec<String>,
}

impl StreamResult {
    /// True if the stream produced deltas or a final response.
    pub fn has_output(&self) -> bool {
        self.delta_count > 0 || self.final_response.is_some()
    }
}

/// Drains a stream, panicking on the first error: an `Err` item, or an
/// in-band SSE `error` event, which arrives as `Ok(StreamChunk::Error)`.
///
/// A stream that errors midway has usually already produced a delta, so a
/// helper that stopped quietly would let `has_output()` pass on a broken
/// stream. Tests that expect an error should iterate the stream themselves.
pub async fn consume_stream(
    mut stream: futures_util::stream::BoxStream<'_, Result<StreamEvent, GenaiError>>,
) -> StreamResult {
    let mut result = StreamResult {
        delta_count: 0,
        collected_text: String::new(),
        final_response: None,
        saw_function_call: false,
        saw_thought: false,
        event_ids: Vec::new(),
        event_text_offsets: Vec::new(),
        unknown_event_types: Vec::new(),
    };

    while let Some(item) = stream.next().await {
        let event = item.unwrap_or_else(|e| {
            panic!("stream failed after {} delta(s): {e:?}", result.delta_count)
        });
        let event_id = event.event_id;

        match event.chunk {
            StreamChunk::StepStart { step, .. } => match step {
                genai_rs::Step::FunctionCall { .. } => result.saw_function_call = true,
                genai_rs::Step::Thought { .. } => result.saw_thought = true,
                _ => {}
            },
            StreamChunk::StepDelta { delta, .. } => {
                result.delta_count += 1;
                if let Some(text) = delta.as_text() {
                    result.collected_text.push_str(text);
                }
                if delta.as_arguments_delta().is_some() {
                    result.saw_function_call = true;
                }
                if matches!(
                    delta,
                    genai_rs::StepDelta::ThoughtSignature { .. }
                        | genai_rs::StepDelta::ThoughtSummary { .. }
                ) {
                    result.saw_thought = true;
                }
            }
            StreamChunk::Completed {
                interaction: response,
                ..
            } => result.final_response = Some(*response),
            StreamChunk::Error { message, code, .. } => panic!(
                "stream sent an error event after {} delta(s): {message} (code {code:?})",
                result.delta_count
            ),
            StreamChunk::Unknown { chunk_type, .. } => {
                println!("stream sent an unrecognized event type: {chunk_type}");
                if !result.unknown_event_types.contains(&chunk_type) {
                    result.unknown_event_types.push(chunk_type);
                }
            }
            _ => {}
        }

        if let Some(eid) = event_id {
            result.event_ids.push(eid);
            result.event_text_offsets.push(result.collected_text.len());
        }
    }

    result
}

/// Walks a listing's `.items()` stream until `found` matches, across every
/// page. Lists put new resources last or interleave concurrent runs on the
/// shared key, so a first-page or `take(n)` check starts failing once
/// earlier runs have leaked enough resources. Panics on a list error.
pub async fn listed<T>(
    mut items: futures_util::stream::BoxStream<'_, Result<T, GenaiError>>,
    found: impl Fn(&T) -> bool,
) -> bool {
    while let Some(item) = items.next().await {
        if found(&item.expect("list failed")) {
            return true;
        }
    }
    false
}

/// What a fully consumed auto-function stream produced.
#[derive(Debug)]
pub struct AutoFunctionStreamResult {
    /// Number of `Delta` chunks received.
    pub delta_count: usize,
    /// Text concatenated from text deltas.
    pub collected_text: String,
    /// Number of `ExecutingFunctions` chunks.
    pub executing_functions_count: usize,
    /// Names of every function executed, in order, without duplicates.
    pub executed_function_names: Vec<String>,
    /// Number of `FunctionResults` chunks.
    pub function_results_count: usize,
    /// The response from `Complete` or `MaxLoopsReached`.
    pub final_response: Option<InteractionResponse>,
    /// The stream ended with `MaxLoopsReached`.
    pub reached_max_loops: bool,
}

impl AutoFunctionStreamResult {
    /// True if the stream produced deltas or a final response.
    pub fn has_output(&self) -> bool {
        self.delta_count > 0 || self.final_response.is_some()
    }
}

/// Drains an auto-function stream, panicking on the first error (see
/// [`consume_stream`]) and on a failed function execution.
///
/// A declared function the loop can't resolve gets an error *result*, not a
/// stream error, and the model can often answer anyway. Without the second
/// check, every caller would pass on a stream whose functions all failed;
/// the non-streaming tests catch the same thing with
/// `all_executions_succeeded()`.
pub async fn consume_auto_function_stream(
    mut stream: futures_util::stream::BoxStream<'_, Result<AutoFunctionStreamEvent, GenaiError>>,
) -> AutoFunctionStreamResult {
    let mut result = AutoFunctionStreamResult {
        delta_count: 0,
        collected_text: String::new(),
        executing_functions_count: 0,
        executed_function_names: Vec::new(),
        function_results_count: 0,
        final_response: None,
        reached_max_loops: false,
    };

    fn record(names: &mut Vec<String>, name: &str) {
        if !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
    }

    while let Some(item) = stream.next().await {
        let event = item.unwrap_or_else(|e| {
            panic!(
                "auto-function stream failed after {} delta(s): {e:?}",
                result.delta_count
            )
        });
        match event.chunk {
            AutoFunctionStreamChunk::Delta { delta, .. } => {
                result.delta_count += 1;
                if let Some(text) = delta.as_text() {
                    result.collected_text.push_str(text);
                }
            }
            AutoFunctionStreamChunk::ExecutingFunctions { pending_calls, .. } => {
                result.executing_functions_count += 1;
                for call in &pending_calls {
                    record(&mut result.executed_function_names, &call.name);
                }
            }
            AutoFunctionStreamChunk::FunctionResults(results) => {
                result.function_results_count += 1;
                for r in &results {
                    assert!(
                        r.is_success(),
                        "function {} failed in the auto-function stream: {:?}",
                        r.name,
                        r.error_message()
                    );
                    record(&mut result.executed_function_names, &r.name);
                }
            }
            AutoFunctionStreamChunk::Complete(response) => result.final_response = Some(*response),
            AutoFunctionStreamChunk::MaxLoopsReached(response) => {
                result.reached_max_loops = true;
                result.final_response = Some(*response);
            }
            other => panic!("unexpected auto-function chunk: {other:?}"),
        }
    }

    result
}
