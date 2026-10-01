//! Live tests for server-side `processing_call` / `processing_result` steps.
//!
//! Agentic video processing emits both steps before the model answers, each
//! carrying a large signature the API requires on stateless replay. When
//! streaming, `step.start` announces `signature: ""` and the value arrives in
//! `step.delta` — the case these tests pin.
//!
//! Agentic processing normally answers in 5-15 s, but since 2026-10-01 some
//! requests never answer: 2 of 6 raw-HTTP requests hung past 150 s, and a
//! replay of the turn can hang too. So each request gets [`ATTEMPT_TIMEOUT`]
//! and [`ATTEMPTS`] tries. If none answers, the test prints
//! `LIVE_TOOL_EVIDENCE_SKIPPED` (server availability, not a crate defect)
//! for CI to count, as the MCP test does for its third-party server. An
//! answer that fails an assertion still fails the test.

mod common;

use common::{TINY_MP4_BASE64, consume_stream, get_client, with_timeout};
use genai_rs::{Content, InteractionInput, Step, VideoProcessing};
use std::future::Future;
use std::time::Duration;

/// A streamed turn once took 47 s to complete.
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(45);
const ATTEMPTS: u32 = 3;
/// Every attempt of both requests in the streamed test.
const TEST_BUDGET: Duration = Duration::from_secs(45 * 3 * 2 + 10);

/// Runs `attempt` up to [`ATTEMPTS`] times, abandoning any that outlives
/// [`ATTEMPT_TIMEOUT`]. Returns `None`, after printing the skip marker, when
/// no attempt answered.
async fn agentic_attempts<F, Fut, T>(mut attempt: F) -> Option<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = T>,
{
    for n in 1..=ATTEMPTS {
        match tokio::time::timeout(ATTEMPT_TIMEOUT, attempt()).await {
            Ok(value) => return Some(value),
            Err(_) => println!(
                "agentic video request attempt {n}/{ATTEMPTS} did not answer within \
                 {ATTEMPT_TIMEOUT:?}"
            ),
        }
    }
    println!(
        "LIVE_TOOL_EVIDENCE_SKIPPED: agentic video processing did not answer within \
         {ATTEMPT_TIMEOUT:?} on any of {ATTEMPTS} attempts"
    );
    None
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
            consume_stream(
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
        let first = result
            .final_response
            .expect("stream ended without a completed response");
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
