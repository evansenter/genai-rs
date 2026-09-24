use super::common::{
    NO_BODY, api_request, path_segment, require_id, send_and_read, send_checked, with_query,
};
use super::context::HttpContext;
use super::error_helpers::deserialize_with_context;
use super::sse_parser::parse_sse_stream;
use crate::errors::GenaiError;
use crate::steps::StepAccumulator;
use crate::{
    InteractionRequest, InteractionResponse, InteractionStreamEvent, StreamChunk, StreamEvent,
};
use async_stream::try_stream;
use futures_util::{Stream, StreamExt};
use tracing::{debug, warn};

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
    let text = send_and_read(
        ctx,
        reqwest::Method::POST,
        &interactions_url(ctx),
        Some(&request),
    )
    .await?;
    deserialize_with_context(&text, "InteractionResponse from create")
}

/// Dispatches one parsed SSE event to `StreamChunk`s, updating the step
/// accumulator. Shared by the create and get streaming paths.
///
/// Returns `Some(chunk)` for events that should be surfaced to the consumer;
/// `None` for events that were dropped (with a warning).
fn dispatch_stream_event(
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
        "interaction.status_update" => match (event.interaction_id, event.status) {
            (Some(interaction_id), Some(status)) => Some(StreamChunk::StatusUpdate {
                interaction_id,
                status,
            }),
            (has_id, has_status) => {
                warn!(
                    "interaction.status_update missing required fields: interaction_id={}, status={} - event dropped",
                    has_id.is_some(),
                    has_status.is_some()
                );
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
                Some(StreamChunk::StepDelta { index, delta })
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
            debug!(
                "Unknown SSE event type '{}' - preserving as StreamChunk::Unknown",
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
fn stream_events(
    ctx: &HttpContext,
    request_id: u64,
    builder: reqwest::RequestBuilder,
) -> impl Stream<Item = Result<StreamEvent, GenaiError>> + Send + '_ {
    try_stream! {
        let response = send_checked(ctx, request_id, builder).await?;
        let parsed_stream =
            parse_sse_stream::<InteractionStreamEvent>(response.bytes_stream(), ctx, request_id);
        futures_util::pin_mut!(parsed_stream);

        let mut accumulator = StepAccumulator::new();
        while let Some(result) = parsed_stream.next().await {
            let event = result?;
            debug!(
                "SSE event received: event_type={:?}, index={:?}, event_id={:?}",
                event.event_type, event.index, event.event_id
            );

            let event_id = event.event_id.clone();
            if let Some(chunk) = dispatch_stream_event(event, &mut accumulator) {
                yield StreamEvent::new(chunk, event_id);
            }
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
            input: InteractionInput::Text("Hello".to_string()),
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
            StreamChunk::StepDelta { index: 0, delta } => {
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
}
