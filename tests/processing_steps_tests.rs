//! Live tests for server-side `processing_call` / `processing_result` steps.
//!
//! Agentic video processing emits both steps before the model answers, each
//! carrying a large signature the API requires on stateless replay. When
//! streaming, `step.start` announces `signature: ""` and the value arrives in
//! `step.delta` — the case these tests pin.

mod common;

use common::{TINY_MP4_BASE64, consume_stream, extended_test_timeout, get_client, with_timeout};
use genai_rs::{Content, InteractionInput, Step, VideoProcessing};

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

    with_timeout(extended_test_timeout(), async {
        let response = client
            .interaction()
            .with_model(genai_rs::DEFAULT_MODEL)
            .with_input(InteractionInput::Content(agentic_video_turn()))
            .with_store_disabled()
            .create()
            .await
            .expect("agentic video request failed");
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

    with_timeout(extended_test_timeout(), async {
        let stream = client
            .interaction()
            .with_model(genai_rs::DEFAULT_MODEL)
            .with_input(InteractionInput::Content(agentic_video_turn()))
            .with_store_disabled()
            .create_stream();
        let result = consume_stream(stream).await;
        let first = result
            .final_response
            .expect("stream ended without a completed response");
        assert_processing_steps_signed(&first.steps);

        let mut history = vec![Step::user_input(agentic_video_turn())];
        history.extend(first.steps);

        let follow_up = client
            .interaction()
            .with_model(genai_rs::DEFAULT_MODEL)
            .with_history(history)
            .with_text("Is the video in color? Answer yes or no.")
            .with_store_disabled()
            .create()
            .await
            .expect("stateless replay of the streamed turn was rejected");
        assert!(
            follow_up.as_text().is_some_and(|t| !t.trim().is_empty()),
            "replay returned no text: {:?}",
            step_types(&follow_up.steps)
        );
    })
    .await;
}
