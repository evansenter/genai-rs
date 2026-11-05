use super::common::{
    NO_BODY, api_request, path_segment, require_id, send_and_read, send_checked, with_query,
};
use super::context::HttpContext;
use super::error_helpers::{deserialize_with_context, not_an_event_stream};
use super::sse_parser::parse_sse_stream;
use crate::errors::GenaiError;
use crate::steps::StepAccumulator;
use crate::wire_streaming::InteractionStreamEvent;
use crate::{InteractionRequest, InteractionResponse, StreamChunk, StreamEvent};
use async_stream::try_stream;
use futures_util::{Stream, StreamExt};
use std::time::Duration;
use tracing::{debug, warn};

/// The longest a stream reads the body's tail after its terminal event: 5 s.
///
/// After `interaction.completed` the API sends `event: done` /
/// `data: [DONE]` and closes the connection about a second later, and the
/// stored interaction is readable only once it has: a `GET` sent right
/// after a close at `interaction.completed` answered `500 Internal error
/// encountered`, the same `GET` ten seconds later `200`, and one sent after
/// reading to the end `200` at once (live 2026-11-05, `DEFAULT_MODEL`). So a
/// stream reads the tail before it ends, and a caller that reads it to its
/// end can `get` the interaction or chain on it right away.
///
/// Five times the observed tail leaves room for a slow close, and bounds
/// the final poll on a connection left half-open after the terminal event,
/// which would otherwise hang it: the default client has no read timeout.
pub(crate) const STREAM_TAIL_LIMIT: Duration = Duration::from_secs(5);

fn interactions_url(ctx: &HttpContext) -> String {
    ctx.api_url("interactions")
}

fn interaction_url(ctx: &HttpContext, id: &str) -> String {
    ctx.api_url(&format!("interactions/{}", path_segment(id)))
}

/// Creates an interaction (`POST /v1beta/interactions`).
///
/// # Errors
///
/// Returns an error if the request fails, the status is not a success, or
/// the body does not parse as an [`InteractionResponse`].
pub async fn create_interaction(
    ctx: &HttpContext,
    mut request: InteractionRequest,
) -> Result<InteractionResponse, GenaiError> {
    // The transport decides: a `stream: true` body on this endpoint would
    // come back as SSE and fail to parse.
    request.stream = None;
    drop_transport_keys(&mut request);
    let text = send_and_read(
        ctx,
        reqwest::Method::POST,
        &interactions_url(ctx),
        Some(&request),
    )
    .await?;
    deserialize_with_context(&text, "InteractionResponse from create")
}

/// Removes a `stream` key from [`InteractionRequest::extra`], with a
/// `warn!`. Whether a request streams belongs to the transport, which sets
/// the modeled `stream` field to match the endpoint; `extra` is written after
/// the modeled fields and the API keeps the last of a repeated key, so a
/// copy there would override it (an SSE answer to `execute()`, which then
/// fails to parse).
fn drop_transport_keys(request: &mut InteractionRequest) {
    if request.extra.remove("stream").is_some() {
        warn!(
            "InteractionRequest::extra has a `stream` key; dropped it, since \
             execute() and execute_stream() set `stream` themselves"
        );
    }
}

/// Dispatches one parsed SSE event to `StreamChunk`s, updating the step
/// accumulator. Shared by the create and get streaming paths.
///
/// Returns `Some(chunk)` for events that should be surfaced to the consumer;
/// `None` for events that were dropped (with a warning).
pub(crate) fn dispatch_stream_event(
    event: InteractionStreamEvent,
    accumulator: &mut StepAccumulator,
) -> Option<StreamChunk> {
    match event.event_type.as_str() {
        "interaction.created" => {
            // Interaction accepted - provides early access to interaction ID
            if let Some(interaction) = event.interaction {
                Some(StreamChunk::Created { interaction })
            } else {
                warn!("interaction.created event missing interaction field - event dropped");
                None
            }
        }
        // No interaction_id on a `store: false` stream (live 2026-11-01),
        // so only the status is required.
        "interaction.status_update" => match event.status {
            Some(status) => Some(StreamChunk::StatusUpdate {
                interaction_id: event.interaction_id,
                status,
                usage: event.usage,
                continuation_token: event.continuation_token,
            }),
            None => {
                warn!("interaction.status_update missing status - event dropped");
                None
            }
        },
        "step.start" => match (event.index, event.step) {
            (Some(index), Some(step)) => {
                accumulator.start(index, step.clone());
                Some(StreamChunk::StepStart { index, step })
            }
            (index, step) => {
                warn!(
                    "step.start missing required fields: index={}, step={} - event dropped",
                    index.is_some(),
                    step.is_some()
                );
                None
            }
        },
        "step.delta" => match (event.index, event.delta) {
            (Some(index), Some(delta)) => {
                accumulator.apply_delta(index, &delta);
                let total_usage = event.metadata.and_then(|metadata| metadata.total_usage);
                // Like a step.stop's usage, a fallback for a terminal event
                // that omits usage.
                if let Some(usage) = &total_usage {
                    accumulator.record_cumulative_usage(usage.clone());
                }
                Some(StreamChunk::StepDelta {
                    index,
                    delta,
                    total_usage,
                })
            }
            (index, delta) => {
                warn!(
                    "step.delta missing required fields: index={}, delta={} - event dropped",
                    index.is_some(),
                    delta.is_some()
                );
                None
            }
        },
        "step.stop" => {
            if let Some(index) = event.index {
                accumulator.stop(index);
                // Retain the cumulative interaction usage as a fallback for
                // terminal events that omit usage entirely.
                if let Some(usage) = &event.usage {
                    accumulator.record_cumulative_usage(usage.clone());
                }
                Some(StreamChunk::StepStop {
                    index,
                    usage: event.usage,
                    step_usage: event.step_usage,
                })
            } else {
                warn!("step.stop event missing index field - event dropped");
                None
            }
        }
        "interaction.completed" => {
            if let Some(mut interaction) = event.interaction {
                // Total usage may arrive via event metadata instead of the
                // partial interaction payload; if both are absent, fall back
                // to the last cumulative usage seen on a step.stop event.
                // (Runs before the steps fill below, which takes the
                // accumulator.)
                if interaction.usage.is_none() {
                    interaction.usage = event
                        .metadata
                        .and_then(|metadata| metadata.total_usage)
                        .or_else(|| accumulator.take_cumulative_usage());
                }
                // The lifecycle payload may omit steps (streaming already
                // delivered them incrementally). Fill them in from the
                // accumulator so response.function_calls() / as_text() work.
                if interaction.steps.is_empty() && !accumulator.is_empty() {
                    interaction.steps = std::mem::take(accumulator).finish();
                }
                Some(StreamChunk::Completed(interaction))
            } else {
                warn!("interaction.completed event missing interaction field - event dropped");
                None
            }
        }
        "error" => {
            // Error occurred during streaming
            if let Some(error) = event.error {
                Some(StreamChunk::Error {
                    message: error.message,
                    code: error.code,
                })
            } else {
                // If no error object, treat as unknown error
                Some(StreamChunk::Error {
                    message: "Unknown streaming error".to_string(),
                    code: None,
                })
            }
        }
        other => {
            warn!(
                "Unknown SSE event type '{}' - preserving as StreamChunk::Unknown. \
                 This may indicate a new API feature.",
                other
            );
            Some(StreamChunk::Unknown {
                chunk_type: other.to_string(),
                data: event.raw,
            })
        }
    }
}

/// Creates an interaction and streams its events
/// (`POST /v1beta/interactions?alt=sse` with `stream: true`).
///
/// Yields a [`StreamEvent`] per surfaced SSE event (revision 2026-05-20
/// lifecycle: `Created`, `StatusUpdate`, `StepStart`, `StepDelta`,
/// `StepStop`, then `Completed` or `Error`).
pub fn create_interaction_stream<'a>(
    ctx: &'a HttpContext,
    mut request: InteractionRequest,
) -> impl Stream<Item = Result<StreamEvent, GenaiError>> + Send + 'a {
    request.stream = Some(true);
    drop_transport_keys(&mut request);
    let url = with_query(interactions_url(ctx), &[("alt", Some("sse"))]);

    let request_id = ctx.next_request_id();
    ctx.emit_request(
        request_id,
        "POST (stream)",
        &url,
        ctx.serialize_wire_body(&request).as_ref(),
    );
    let builder = api_request(ctx, reqwest::Method::POST, &url).json(&request);
    stream_events(ctx, request_id, builder)
}

/// Sends a streaming request and turns its SSE body into [`StreamEvent`]s,
/// accumulating steps so the final `Completed` response is whole.
///
/// The terminal event (`Completed` or `Error`) is yielded as soon as it
/// arrives. The next poll reads the rest of the body to its end, discarding
/// every frame and swallowing every error, and then the stream ends: a late
/// error (a reset while the server closes, a stray frame) can't follow a
/// finished interaction and undo it, in particular a finished auto-function
/// round, and once the stream has ended the server has stored the
/// interaction, so a `get` or a chained request finds it (see
/// [`STREAM_TAIL_LIMIT`], which bounds that read).
///
/// A body that ends before a terminal event yields
/// [`GenaiError::StreamInterrupted`] last, so a cut-off stream can't pass for
/// a finished one; the parser gives a transport failure mid-body the same
/// error. The exception is a stream whose last event is one this crate
/// doesn't recognize: that may be a new terminal event, so it ends cleanly,
/// with a `warn!` (Evergreen).
fn stream_events(
    ctx: &HttpContext,
    request_id: u64,
    builder: reqwest::RequestBuilder,
) -> impl Stream<Item = Result<StreamEvent, GenaiError>> + Send + '_ {
    try_stream! {
        let response = send_checked(ctx, request_id, builder).await?;
        // The API answers `text/event-stream` (live 2026-11-04). Another
        // declared type is a body that was never a stream (a captive
        // portal's page, a plain JSON interaction), so its end is not a
        // cut-off. A missing header (a proxy may strip it) is given the
        // benefit of the doubt; a breaking change, so the CHANGELOG says how
        // to migrate.
        if let Some(content_type) = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .filter(|value| !is_event_stream(value))
            .map(str::to_owned)
        {
            let body = response.text().await?;
            ctx.emit_response_body(request_id, &body);
            Err(not_an_event_stream(&content_type, &body))?;
            return;
        }
        // Fused: the parser may already have read the end of the body when
        // the tail is drained below.
        let body = response.bytes_stream().fuse();
        futures_util::pin_mut!(body);
        {
            let parsed = parse_sse_stream::<InteractionStreamEvent>(body.as_mut(), ctx, request_id);
            let events = interaction_events(parsed);
            futures_util::pin_mut!(events);
            while let Some(event) = events.next().await {
                yield event?;
            }
        }
        // The events ended without an error: at a terminal event, or at the
        // end of the body after an unrecognized one.
        drain_tail(body).await;
    }
}

/// Reads what is left of a body after its terminal event, up to its end or
/// [`STREAM_TAIL_LIMIT`], discarding the bytes and swallowing errors: the
/// interaction is finished, and the server stores it only once the stream
/// is done. Raw bytes, not SSE events, so a frame that doesn't parse can't
/// stop the read early.
async fn drain_tail(body: impl Stream<Item = Result<bytes::Bytes, reqwest::Error>>) {
    futures_util::pin_mut!(body);
    let read = tokio::time::timeout(STREAM_TAIL_LIMIT, async {
        let mut len = 0;
        while let Some(chunk) = body.next().await {
            match chunk {
                Ok(chunk) => len += chunk.len(),
                Err(error) => {
                    debug!("the body failed after the terminal event (ignored): {error}");
                    break;
                }
            }
        }
        len
    })
    .await;
    match read {
        Ok(len) => debug!("read the stream's {len}-byte tail after its terminal event"),
        Err(_) => warn!(
            "the stream's body did not end within {STREAM_TAIL_LIMIT:?} of its terminal \
             event; closing the connection. The stored interaction may not be readable \
             for a moment"
        ),
    }
}

/// Whether a `Content-Type` value is `text/event-stream`, ignoring case and
/// parameters (`; charset=utf-8`).
fn is_event_stream(content_type: &str) -> bool {
    content_type
        .split(';')
        .next()
        .is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case("text/event-stream"))
}

/// Turns parsed SSE events into [`StreamEvent`]s: dispatch, accumulation,
/// the stop at the first terminal event, and the cut-off check. Split from
/// [`stream_events`] so it runs without HTTP (unit tests, fuzzing).
///
/// Nothing is parsed after the terminal event: [`stream_events`] reads the
/// rest of the body as raw bytes.
pub(crate) fn interaction_events(
    parsed: impl Stream<Item = Result<InteractionStreamEvent, GenaiError>> + Send,
) -> impl Stream<Item = Result<StreamEvent, GenaiError>> + Send {
    try_stream! {
        futures_util::pin_mut!(parsed);
        let mut accumulator = StepAccumulator::new();
        let mut last_was_unknown = false;
        while let Some(result) = parsed.next().await {
            let event = result?;
            debug!(
                "SSE event received: event_type={:?}, index={:?}, event_id={:?}",
                event.event_type, event.index, event.event_id
            );

            let event_id = event.event_id.clone();
            if let Some(chunk) = dispatch_stream_event(event, &mut accumulator) {
                let terminal = chunk.is_terminal();
                last_was_unknown = chunk.is_unknown();
                yield StreamEvent::new(chunk, event_id);
                if terminal {
                    // What follows is never parsed, so a late error can't
                    // follow the terminal event.
                    return;
                }
            }
        }
        if last_was_unknown {
            tracing::warn!(
                "the stream ended on an unrecognized event, with no interaction.completed; \
                 treating it as a new terminal event"
            );
        } else {
            Err(GenaiError::StreamInterrupted {
                message: "the body ended before interaction.completed or an error event \
                          (the server closed the connection)"
                    .to_string(),
                source: None,
            })?;
        }
    }
}

/// Retrieves an interaction (`GET /v1beta/interactions/{id}`).
///
/// `include_input` sets the `include_input=true` query parameter.
pub async fn get_interaction(
    ctx: &HttpContext,
    interaction_id: &str,
    include_input: bool,
) -> Result<InteractionResponse, GenaiError> {
    require_id(interaction_id, "interaction")?;
    let url = with_query(
        interaction_url(ctx, interaction_id),
        &[("include_input", include_input.then_some("true"))],
    );
    let text = send_and_read(ctx, reqwest::Method::GET, &url, NO_BODY).await?;
    deserialize_with_context(&text, "InteractionResponse from get")
}

/// The URL for streaming an existing interaction. Without `stream=true` the
/// server ignores `alt=sse` and answers with the plain JSON interaction.
fn interaction_stream_url(
    ctx: &HttpContext,
    interaction_id: &str,
    last_event_id: Option<&str>,
) -> String {
    with_query(
        interaction_url(ctx, interaction_id),
        &[
            ("alt", Some("sse")),
            ("stream", Some("true")),
            ("last_event_id", last_event_id),
        ],
    )
}

/// Streams an existing interaction's events
/// (`GET /v1beta/interactions/{id}?alt=sse&stream=true`), optionally resuming
/// after `last_event_id`.
///
/// An invalid `interaction_id` is yielded as the stream's only item.
pub fn get_interaction_stream<'a>(
    ctx: &'a HttpContext,
    interaction_id: &'a str,
    last_event_id: Option<&'a str>,
) -> impl Stream<Item = Result<StreamEvent, GenaiError>> + Send + 'a {
    try_stream! {
        require_id(interaction_id, "interaction")?;
        let url = interaction_stream_url(ctx, interaction_id, last_event_id);

        let request_id = ctx.next_request_id();
        let method = match last_event_id {
            Some(id) => format!("GET (stream, resuming from {id})"),
            None => "GET (stream)".to_string(),
        };
        ctx.emit_request(request_id, &method, &url, None);

        let builder = api_request(ctx, reqwest::Method::GET, &url);
        let events = stream_events(ctx, request_id, builder);
        futures_util::pin_mut!(events);
        while let Some(event) = events.next().await {
            yield event?;
        }
    }
}

/// Deletes an interaction (`DELETE /v1beta/interactions/{id}`).
pub async fn delete_interaction(ctx: &HttpContext, interaction_id: &str) -> Result<(), GenaiError> {
    require_id(interaction_id, "interaction")?;
    send_and_read(
        ctx,
        reqwest::Method::DELETE,
        &interaction_url(ctx, interaction_id),
        NO_BODY,
    )
    .await?;
    Ok(())
}

/// Cancels a background interaction
/// (`POST /v1beta/interactions/{id}/cancel`).
pub async fn cancel_interaction(
    ctx: &HttpContext,
    interaction_id: &str,
) -> Result<InteractionResponse, GenaiError> {
    require_id(interaction_id, "interaction")?;
    let url = format!("{}/cancel", interaction_url(ctx, interaction_id));
    let text = send_and_read(
        ctx,
        reqwest::Method::POST,
        &url,
        Some(&serde_json::json!({})),
    )
    .await?;
    deserialize_with_context(&text, "InteractionResponse from cancel")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InteractionInput, InteractionStatus, Step, StepDelta};

    #[test]
    fn test_interaction_urls() {
        let ctx = HttpContext::new(reqwest::Client::new(), "k".to_string(), vec![]);
        assert_eq!(
            interactions_url(&ctx),
            "https://generativelanguage.googleapis.com/v1beta/interactions"
        );
        // A path-metacharacter ID is encoded, not interpolated raw.
        assert_eq!(
            interaction_url(&ctx, "a/b?c"),
            "https://generativelanguage.googleapis.com/v1beta/interactions/a%2Fb%3Fc"
        );
    }

    #[test]
    fn test_interaction_stream_url_requests_sse() {
        let ctx = HttpContext::new(reqwest::Client::new(), "k".to_string(), vec![]);
        assert_eq!(
            interaction_stream_url(&ctx, "int-1", None),
            "https://generativelanguage.googleapis.com/v1beta/interactions/int-1?alt=sse&stream=true"
        );
        // The resume token is percent-encoded.
        assert_eq!(
            interaction_stream_url(&ctx, "int-1", Some("evt+1&x")),
            "https://generativelanguage.googleapis.com/v1beta/interactions/int-1?alt=sse&stream=true&last_event_id=evt%2B1%26x"
        );
    }

    #[test]
    fn test_create_interaction_request_serialization() {
        // Verify request serialization works correctly
        let request = InteractionRequest {
            model: Some("test-model".to_string()),
            agent: None,
            agent_config: None,
            input: Some(InteractionInput::Text("Hello".to_string())),
            previous_interaction_id: None,
            tools: None,
            response_modalities: None,
            response_format: None,
            generation_config: None,
            stream: None,
            background: None,
            store: None,
            system_instruction: None,
            service_tier: None,
            webhook_config: None,
            environment: None,
            safety_settings: None,
            labels: None,
            continuation_token: None,
            extra: serde_json::Map::new(),
        };

        let json = serde_json::to_string(&request).expect("Serialization should work");
        assert!(json.contains("test-model"));
        assert!(json.contains("Hello"));
    }

    #[test]
    fn test_interaction_response_deserialization() {
        // Verify we can deserialize a typical revision 2026-05-20 response
        let response_json = r#"{
            "id": "test_interaction_123",
            "model": "test-model",
            "steps": [{"type": "model_output", "content": [{"type": "text", "text": "Hi there!"}]}],
            "status": "completed"
        }"#;

        let response: InteractionResponse =
            serde_json::from_str(response_json).expect("Deserialization should work");

        assert_eq!(response.id.as_deref(), Some("test_interaction_123"));
        assert_eq!(response.status, InteractionStatus::Completed);
        assert_eq!(response.steps.len(), 1);
        assert_eq!(response.as_text(), Some("Hi there!"));
    }

    #[test]
    fn test_cancelled_interaction_response_deserialization() {
        // Verify we can deserialize a cancelled interaction response
        let response_json = r#"{
            "id": "cancelled_interaction_123",
            "model": "test-model",
            "steps": [],
            "status": "cancelled"
        }"#;

        let response: InteractionResponse =
            serde_json::from_str(response_json).expect("Deserialization should work");

        assert_eq!(response.id.as_deref(), Some("cancelled_interaction_123"));
        assert_eq!(response.status, InteractionStatus::Cancelled);
        assert!(response.steps.is_empty());
    }

    // =========================================================================
    // SSE dispatch tests (event shapes per revision 2026-05-20)
    // =========================================================================

    fn parse_event(json: &str) -> InteractionStreamEvent {
        serde_json::from_str(json).expect("event should parse")
    }

    #[test]
    fn test_dispatch_full_text_lifecycle() {
        let mut acc = StepAccumulator::new();

        let created = dispatch_stream_event(
            parse_event(
                r#"{"event_type":"interaction.created","interaction":{"id":"i1","status":"in_progress"},"event_id":"e0"}"#,
            ),
            &mut acc,
        )
        .unwrap();
        assert!(matches!(created, StreamChunk::Created { .. }));

        let start = dispatch_stream_event(
            parse_event(
                r#"{"event_type":"step.start","index":0,"step":{"type":"model_output","content":[]},"event_id":"e1"}"#,
            ),
            &mut acc,
        )
        .unwrap();
        assert!(matches!(start, StreamChunk::StepStart { index: 0, .. }));

        let delta = dispatch_stream_event(
            parse_event(
                r#"{"event_type":"step.delta","index":0,"delta":{"type":"text","text":"Hello"},"event_id":"e2"}"#,
            ),
            &mut acc,
        )
        .unwrap();
        match &delta {
            StreamChunk::StepDelta {
                index: 0, delta, ..
            } => {
                assert_eq!(delta.as_text(), Some("Hello"));
            }
            other => panic!("Expected StepDelta, got {other:?}"),
        }

        let stop = dispatch_stream_event(
            parse_event(
                r#"{"event_type":"step.stop","index":0,"step_usage":{"total_output_tokens":5},"event_id":"e3"}"#,
            ),
            &mut acc,
        )
        .unwrap();
        match &stop {
            StreamChunk::StepStop { step_usage, .. } => {
                assert_eq!(step_usage.as_ref().unwrap().total_output_tokens, Some(5));
            }
            other => panic!("Expected StepStop, got {other:?}"),
        }

        let completed = dispatch_stream_event(
            parse_event(
                r#"{"event_type":"interaction.completed","interaction":{"id":"i1","status":"completed"},"metadata":{"total_usage":{"total_tokens":12}},"event_id":"e4"}"#,
            ),
            &mut acc,
        )
        .unwrap();
        match completed {
            StreamChunk::Completed(response) => {
                // Steps were filled from the accumulator.
                assert_eq!(response.as_text(), Some("Hello"));
                // Usage was taken from event metadata.
                assert_eq!(response.total_tokens(), Some(12));
            }
            other => panic!("Expected Completed, got {other:?}"),
        }
    }

    #[test]
    fn test_dispatch_function_call_arguments_delta() {
        let mut acc = StepAccumulator::new();

        dispatch_stream_event(
            parse_event(
                r#"{"event_type":"step.start","index":0,"step":{"type":"function_call","id":"c1","name":"get_weather","arguments":{}}}"#,
            ),
            &mut acc,
        );
        let delta = dispatch_stream_event(
            parse_event(
                r#"{"event_type":"step.delta","index":0,"delta":{"type":"arguments_delta","arguments":"{\"city\":\"Tokyo\"}"}}"#,
            ),
            &mut acc,
        )
        .unwrap();
        assert!(matches!(
            delta,
            StreamChunk::StepDelta {
                delta: StepDelta::ArgumentsDelta { .. },
                ..
            }
        ));
        dispatch_stream_event(
            parse_event(r#"{"event_type":"step.stop","index":0}"#),
            &mut acc,
        );

        let completed = dispatch_stream_event(
            parse_event(
                r#"{"event_type":"interaction.completed","interaction":{"id":"i1","status":"requires_action"}}"#,
            ),
            &mut acc,
        )
        .unwrap();
        match completed {
            StreamChunk::Completed(response) => {
                let calls = response.function_calls();
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].name, "get_weather");
                assert_eq!(calls[0].args["city"], "Tokyo");
            }
            other => panic!("Expected Completed, got {other:?}"),
        }
    }

    #[test]
    fn test_dispatch_status_update_and_error() {
        let mut acc = StepAccumulator::new();

        let update = dispatch_stream_event(
            parse_event(
                r#"{"event_type":"interaction.status_update","interaction_id":"i1","status":"budget_exceeded"}"#,
            ),
            &mut acc,
        )
        .unwrap();
        assert!(matches!(
            update,
            StreamChunk::StatusUpdate {
                status: InteractionStatus::BudgetExceeded,
                ..
            }
        ));

        let error = dispatch_stream_event(
            parse_event(r#"{"event_type":"error","error":{"message":"boom","code":"internal"}}"#),
            &mut acc,
        )
        .unwrap();
        assert!(matches!(error, StreamChunk::Error { message, .. } if message == "boom"));
    }

    #[test]
    fn test_dispatch_unknown_event_preserved() {
        let mut acc = StepAccumulator::new();
        let chunk = dispatch_stream_event(
            parse_event(r#"{"event_type":"interaction.paused","reason":"maintenance"}"#),
            &mut acc,
        )
        .unwrap();
        assert!(chunk.is_unknown());
        assert_eq!(chunk.unknown_chunk_type(), Some("interaction.paused"));
        assert_eq!(chunk.unknown_data().unwrap()["reason"], "maintenance");
    }

    #[test]
    fn test_dispatch_completed_falls_back_to_step_stop_usage() {
        let mut acc = StepAccumulator::new();

        dispatch_stream_event(
            parse_event(
                r#"{"event_type":"step.start","index":0,"step":{"type":"model_output","content":[]}}"#,
            ),
            &mut acc,
        );
        dispatch_stream_event(
            parse_event(
                r#"{"event_type":"step.stop","index":0,"usage":{"total_tokens":42},"step_usage":{"total_output_tokens":5}}"#,
            ),
            &mut acc,
        );

        // Terminal event with neither interaction.usage nor metadata.total_usage.
        let completed = dispatch_stream_event(
            parse_event(
                r#"{"event_type":"interaction.completed","interaction":{"id":"i1","status":"completed"}}"#,
            ),
            &mut acc,
        )
        .unwrap();
        match completed {
            StreamChunk::Completed(response) => {
                assert_eq!(response.total_tokens(), Some(42));
            }
            other => panic!("Expected Completed, got {other:?}"),
        }
    }

    #[test]
    fn test_dispatch_step_delta_keeps_its_total_usage() {
        // The bindings' StepDelta.metadata.total_usage (not seen live as of
        // 2026-11-05). It reaches the chunk, and a completed event without
        // usage falls back to it.
        let mut acc = StepAccumulator::new();
        dispatch_stream_event(
            parse_event(r#"{"event_type":"step.start","index":0,"step":{"type":"model_output"}}"#),
            &mut acc,
        );
        let delta = dispatch_stream_event(
            parse_event(
                r#"{"event_type":"step.delta","index":0,"delta":{"type":"text","text":"Hi"},"metadata":{"total_usage":{"total_tokens":21}}}"#,
            ),
            &mut acc,
        )
        .unwrap();
        match &delta {
            StreamChunk::StepDelta { total_usage, .. } => {
                assert_eq!(total_usage.as_ref().and_then(|u| u.total_tokens), Some(21));
            }
            other => panic!("Expected StepDelta, got {other:?}"),
        }
        let completed = dispatch_stream_event(
            parse_event(
                r#"{"event_type":"interaction.completed","interaction":{"id":"i1","status":"completed"}}"#,
            ),
            &mut acc,
        )
        .unwrap();
        match completed {
            StreamChunk::Completed(response) => {
                assert_eq!(response.usage.and_then(|u| u.total_tokens), Some(21));
            }
            other => panic!("Expected Completed, got {other:?}"),
        }
    }

    #[test]
    fn test_dispatch_completed_metadata_usage_wins_over_step_stop() {
        let mut acc = StepAccumulator::new();

        dispatch_stream_event(
            parse_event(r#"{"event_type":"step.stop","index":0,"usage":{"total_tokens":42}}"#),
            &mut acc,
        );

        let completed = dispatch_stream_event(
            parse_event(
                r#"{"event_type":"interaction.completed","interaction":{"id":"i1","status":"completed"},"metadata":{"total_usage":{"total_tokens":99}}}"#,
            ),
            &mut acc,
        )
        .unwrap();
        match completed {
            StreamChunk::Completed(response) => {
                assert_eq!(response.total_tokens(), Some(99));
            }
            other => panic!("Expected Completed, got {other:?}"),
        }
    }

    #[test]
    fn test_dispatch_completed_prefers_server_steps() {
        let mut acc = StepAccumulator::new();
        acc.start(0, Step::model_text("accumulated"));

        let completed = dispatch_stream_event(
            parse_event(
                r#"{"event_type":"interaction.completed","interaction":{
                    "id":"i1","status":"completed",
                    "steps":[{"type":"model_output","content":[{"type":"text","text":"authoritative"}]}]
                }}"#,
            ),
            &mut acc,
        )
        .unwrap();
        match completed {
            StreamChunk::Completed(response) => {
                assert_eq!(response.as_text(), Some("authoritative"));
            }
            other => panic!("Expected Completed, got {other:?}"),
        }
    }

    // =========================================================================
    // Terminal events and cut-offs
    // =========================================================================

    fn events_from(
        items: Vec<Result<&str, GenaiError>>,
    ) -> Vec<Result<InteractionStreamEvent, GenaiError>> {
        items
            .into_iter()
            .map(|item| item.map(parse_event))
            .collect()
    }

    async fn run(
        items: Vec<Result<InteractionStreamEvent, GenaiError>>,
    ) -> (Vec<Result<StreamEvent, GenaiError>>, usize) {
        let pulled = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = pulled.clone();
        let source = futures_util::stream::iter(items).inspect(move |_| {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        let out = interaction_events(source).collect().await;
        (out, pulled.load(std::sync::atomic::Ordering::SeqCst))
    }

    const CREATED: &str =
        r#"{"event_type":"interaction.created","interaction":{"id":"i1","status":"in_progress"}}"#;
    const COMPLETED: &str =
        r#"{"event_type":"interaction.completed","interaction":{"id":"i1","status":"completed"}}"#;

    #[tokio::test]
    async fn test_nothing_is_read_after_completed() {
        let late = GenaiError::StreamInterrupted {
            message: "reset while closing".to_string(),
            source: None,
        };
        let (events, pulled) = run(events_from(vec![
            Ok(CREATED),
            Ok(COMPLETED),
            Err(late),
            Ok(r#"{"event_type":"step.delta","index":0,"delta":{"type":"text","text":"late"}}"#),
        ]))
        .await;
        assert!(
            matches!(events.as_slice(), [Ok(_), Ok(last)] if last.chunk.is_terminal()),
            "{events:?}"
        );
        assert_eq!(pulled, 2, "the event after the terminal one is never read");
    }

    #[tokio::test]
    async fn test_nothing_is_read_after_an_error_event() {
        let (events, pulled) = run(events_from(vec![
            Ok(r#"{"event_type":"error","error":{"message":"blocked","code":"recitation"}}"#),
            Ok(COMPLETED),
        ]))
        .await;
        assert!(
            matches!(
                events.as_slice(),
                [Ok(StreamEvent {
                    chunk: StreamChunk::Error { .. },
                    ..
                })]
            ),
            "{events:?}"
        );
        assert_eq!(pulled, 1);
    }

    #[tokio::test]
    async fn test_end_before_a_terminal_event_is_interrupted() {
        let (events, _) = run(events_from(vec![Ok(CREATED)])).await;
        let [Ok(_), Err(err)] = events.as_slice() else {
            panic!("expected the created event, then the cut-off: {events:?}");
        };
        assert!(
            matches!(err, GenaiError::StreamInterrupted { source: None, .. }),
            "{err:?}"
        );
        assert!(err.is_retryable());

        // An empty body is cut off too.
        let (events, _) = run(Vec::new()).await;
        assert!(
            matches!(
                events.as_slice(),
                [Err(GenaiError::StreamInterrupted { .. })]
            ),
            "{events:?}"
        );
    }

    /// A body still open after the terminal event is given up at the limit;
    /// the integration tests (`tests/reliability_tests.rs`) cover the tail
    /// that ends, and one that fails.
    #[tokio::test(start_paused = true)]
    async fn test_drain_tail_gives_up_at_the_limit() {
        let start = tokio::time::Instant::now();
        drain_tail(futures_util::stream::pending::<
            Result<bytes::Bytes, reqwest::Error>,
        >())
        .await;
        assert_eq!(start.elapsed(), STREAM_TAIL_LIMIT);
    }

    #[test]
    fn test_drop_transport_keys_removes_only_stream() {
        let mut request = InteractionRequest {
            stream: Some(true),
            ..Default::default()
        };
        request
            .extra
            .insert("stream".into(), serde_json::json!(false));
        request
            .extra
            .insert("brand_new".into(), serde_json::json!(1));
        drop_transport_keys(&mut request);
        assert_eq!(request.stream, Some(true));
        assert!(!request.extra.contains_key("stream"));
        assert!(request.extra.contains_key("brand_new"));
    }

    #[test]
    fn test_is_event_stream() {
        for yes in [
            "text/event-stream",
            "TEXT/Event-Stream",
            "text/event-stream; charset=utf-8",
            " text/event-stream ;charset=UTF-8",
        ] {
            assert!(is_event_stream(yes), "{yes}");
        }
        for no in ["application/json", "text/html", "text/event-streamx", ""] {
            assert!(!is_event_stream(no), "{no}");
        }
    }
}
