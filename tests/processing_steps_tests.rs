//! Live tests for server-side `processing_call` / `processing_result` steps.
//!
//! Agentic video processing emits both steps before the model answers, each
//! carrying a large signature the API requires on stateless replay. When
//! streaming, `step.start` announces `signature: ""` and the value arrives in
//! `step.delta` — the case these tests pin.
//!
//! Agentic processing normally answers in 5-15 s, but since 2026-10-01 it
//! often fails server-side: on 2026-10-01 2 of 6 raw-HTTP requests hung past
//! 150 s, and on 2026-10-06 3 of 5 returned `500 Internal error encountered`
//! (as a stream `error` event when streamed) while `static` processing of the
//! same video succeeded 5 of 5. So each request gets [`ATTEMPT_TIMEOUT`] and
//! [`ATTEMPTS`] tries; a timeout, a 5xx or a server-sent stream error is
//! retried. If no attempt answers, the test prints
//! `LIVE_TOOL_EVIDENCE_SKIPPED` (server availability, not a crate defect) for
//! CI to count, as the MCP test does for its third-party server. Any other
//! error, a stream that ends without completing, and an answer that fails an
//! assertion still fail the test.

mod common;

use common::{TINY_MP4_BASE64, get_client, with_timeout};
use futures_util::StreamExt;
use futures_util::stream::BoxStream;
use genai_rs::{
    Content, GenaiError, InteractionInput, InteractionResponse, Step, StreamChunk, StreamEvent,
    VideoProcessing,
};
use std::future::Future;
use std::time::Duration;

/// A streamed turn once took 47 s to complete.
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(45);
const ATTEMPTS: u32 = 5;
/// Every attempt of both requests in the streamed test.
const TEST_BUDGET: Duration = Duration::from_secs(45 * 5 * 2 + 10);

/// A failure the server reported or caused: a 5xx, or an `error` event in
/// the stream.
fn is_server_failure(err: &GenaiError) -> bool {
    matches!(err, GenaiError::Api { status_code, .. } if *status_code >= 500)
        || matches!(err, GenaiError::Stream { .. })
}

/// Runs `attempt` up to [`ATTEMPTS`] times, retrying one that outlives
/// [`ATTEMPT_TIMEOUT`] or fails server-side. Returns `None`, after printing
/// the skip marker, when no attempt answered; any other result is returned
/// as is.
async fn agentic_attempts<F, Fut, T>(mut attempt: F) -> Option<Result<T, GenaiError>>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, GenaiError>>,
{
    for n in 1..=ATTEMPTS {
        match tokio::time::timeout(ATTEMPT_TIMEOUT, attempt()).await {
            Ok(Err(err)) if is_server_failure(&err) => {
                println!("agentic video request attempt {n}/{ATTEMPTS} failed server-side: {err}");
            }
            Ok(result) => return Some(result),
            Err(_) => println!(
                "agentic video request attempt {n}/{ATTEMPTS} did not answer within \
                 {ATTEMPT_TIMEOUT:?}"
            ),
        }
    }
    println!(
        "LIVE_TOOL_EVIDENCE_SKIPPED: agentic video processing did not answer on any of \
         {ATTEMPTS} attempts"
    );
    None
}

/// The stream's completed response, with a server `error` event surfaced as
/// [`GenaiError::Stream`]. A stream that ends with neither is malformed.
async fn completed_response(
    mut stream: BoxStream<'_, Result<StreamEvent, GenaiError>>,
) -> Result<InteractionResponse, GenaiError> {
    let mut completed = None;
    while let Some(event) = stream.next().await {
        match event?.chunk {
            StreamChunk::Completed(response) => completed = Some(response),
            StreamChunk::Error { message, code } => {
                return Err(GenaiError::Stream { message, code });
            }
            _ => {}
        }
    }
    completed.ok_or_else(|| {
        GenaiError::MalformedResponse("stream ended without a completed response".into())
    })
}

fn agentic_video_turn() -> Vec<Content> {
    vec![
        Content::text("What is in this video? Answer in five words."),
        Content::video_data(TINY_MP4_BASE64, "video/mp4").with_processing(VideoProcessing::Agentic),
    ]
}

fn assert_processing_steps_signed(steps: &[Step]) {
    for kind in ["processing_call", "processing_result"] {
        let step = steps
            .iter()
            .find(|s| s.step_type() == kind)
            .unwrap_or_else(|| panic!("no {kind} step in {:?}", step_types(steps)));
        assert!(!step.is_unknown(), "{kind} fell through to Unknown");
        assert!(
            step.signature().is_some_and(|s| !s.is_empty()),
            "{kind} step has no signature"
        );
    }
}

fn step_types(steps: &[Step]) -> Vec<&str> {
    steps.iter().map(Step::step_type).collect()
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_agentic_video_emits_signed_processing_steps() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    with_timeout(TEST_BUDGET, async {
        let Some(result) = agentic_attempts(|| {
            client
                .interaction()
                .with_model(genai_rs::DEFAULT_MODEL)
                .with_input(InteractionInput::Content(agentic_video_turn()))
                .with_store_disabled()
                .create()
        })
        .await
        else {
            return;
        };
        let response = result.expect("agentic video request failed");
        assert_processing_steps_signed(&response.steps);
        // The model may process more than once (three calls observed).
        let summary = response.step_summary();
        assert!(summary.processing_call_count >= 1);
        assert_eq!(
            summary.processing_call_count,
            summary.processing_result_count
        );
    })
    .await;
}

/// Regression: a streamed turn's processing signatures must survive
/// accumulation, or replaying it fails with
/// `400 Processing call step is missing signature`.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_streamed_agentic_video_turn_replays_statelessly() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    with_timeout(TEST_BUDGET, async {
        let Some(result) = agentic_attempts(|| {
            completed_response(
                client
                    .interaction()
                    .with_model(genai_rs::DEFAULT_MODEL)
                    .with_input(InteractionInput::Content(agentic_video_turn()))
                    .with_store_disabled()
                    .create_stream(),
            )
        })
        .await
        else {
            return;
        };
        let first = result.expect("streamed agentic video turn failed");
        assert_processing_steps_signed(&first.steps);

        let mut history = vec![Step::user_input(agentic_video_turn())];
        history.extend(first.steps);

        let Some(follow_up) = agentic_attempts(|| {
            client
                .interaction()
                .with_model(genai_rs::DEFAULT_MODEL)
                .with_history(history.clone())
                .with_text("Is the video in color? Answer yes or no.")
                .with_store_disabled()
                .create()
        })
        .await
        else {
            return;
        };
        let follow_up = follow_up.expect("stateless replay of the streamed turn was rejected");
        assert!(
            follow_up.as_text().is_some_and(|t| !t.trim().is_empty()),
            "replay returned no text: {:?}",
            step_types(&follow_up.steps)
        );
    })
    .await;
}
