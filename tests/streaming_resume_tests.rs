//! `get_interaction_stream`: following a background interaction and resuming
//! with `last_event_id`.
//!
//! Observed live 2026-09-24: streaming retrieval works only for background
//! interactions (a foreground one is rejected with 400), and only text deltas
//! streamed live carry an `event_id` (a long answer yields three or four).
//! Observed 2026-09-26: a resumed replay can tag fewer deltas than the live
//! stream did, so resumption is checked on the text, not the id list.
//!
//! ```bash
//! cargo nextest run --test streaming_resume_tests --run-ignored all
//! ```

mod common;

use common::{consume_stream, get_client, stateful_builder, with_timeout};
use futures_util::StreamExt;
use genai_rs::{Client, GenaiError, InteractionStatus};
use std::time::Duration;

const STREAM_BUDGET: Duration = Duration::from_secs(300);

/// Starts a background interaction long enough to still be generating text
/// when retrieval connects, returning its id. Only deltas streamed live carry
/// an `event_id`, so a short answer finished before the connection yields one.
async fn start_background(client: &Client) -> String {
    let response = retry_request!([client] => {
        stateful_builder(&client)
            .with_text("Write a 2000-word story about a lighthouse keeper, in many short paragraphs.")
            .with_thinking_level(genai_rs::ThinkingLevel::Low)
            .with_background(true)
            .create()
            .await
    })
    .expect("background create failed");
    response
        .id
        .expect("background interaction should have an id")
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_get_interaction_stream_follows_background_interaction() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    with_timeout(STREAM_BUDGET, async {
        let id = start_background(&client).await;
        let result = consume_stream(client.get_interaction_stream(&id, None)).await;

        assert!(
            !result.collected_text.is_empty(),
            "retrieval stream produced no text"
        );
        assert!(
            !result.event_ids.is_empty(),
            "retrieval stream carried no event_id to resume from"
        );
        let done = result.final_response.expect("no Completed event");
        assert_eq!(done.status, InteractionStatus::Completed);
    })
    .await;
}

/// Resuming from an event replays only what came after it.
///
/// Needs a stream with at least two ids to have a suffix to compare. The
/// server decides how many it sends, so a run that saw only one starts a new
/// interaction (up to three) rather than weakening the assertion.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_stream_resume_with_last_event_id() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    with_timeout(STREAM_BUDGET, async {
        let mut seen = Vec::new();
        let (id, full) = loop {
            let id = start_background(&client).await;
            let full = consume_stream(client.get_interaction_stream(&id, None)).await;
            seen.push(full.event_ids.len());
            if full.event_ids.len() >= 2 {
                break (id, full);
            }
            assert!(
                seen.len() < 3,
                "no retrieval stream carried two event_ids (counts: {seen:?})"
            );
        };

        let resume_from = &full.event_ids[0];
        let resumed = consume_stream(client.get_interaction_stream(&id, Some(resume_from))).await;

        // Which deltas carry an id differs between the live stream and the
        // replay (a replay can tag fewer), so the ids cannot be compared as
        // lists. The text can: it must be a proper, non-empty tail.
        assert!(
            !resumed.collected_text.is_empty()
                && resumed.collected_text.len() < full.collected_text.len()
                && full.collected_text.ends_with(&resumed.collected_text),
            "resuming from {resume_from} should replay only the text after it: \
             got {} of {} bytes, tail match: {}",
            resumed.collected_text.len(),
            full.collected_text.len(),
            full.collected_text.ends_with(&resumed.collected_text)
        );
        assert!(
            !resumed.event_ids.contains(resume_from),
            "the resume point itself was replayed: {:?}",
            resumed.event_ids
        );
        let done = resumed
            .final_response
            .expect("resumed stream should still end in Completed");
        assert_eq!(done.status, InteractionStatus::Completed);
    })
    .await;
}

/// A foreground interaction cannot be retrieved as a stream; the API's 400
/// must surface rather than read as an empty stream.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_get_interaction_stream_rejects_foreground_interaction() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let response = retry_request!([client] => {
        stateful_builder(&client)
            .with_text("Say hello.")
            .create()
            .await
    })
    .expect("create failed");
    let id = response.id.expect("stored interaction should have an id");

    let mut stream = client.get_interaction_stream(&id, None);
    let first = stream.next().await;
    assert!(
        matches!(
            first,
            Some(Err(GenaiError::Api {
                status_code: 400,
                ..
            }))
        ),
        "expected the API's 400 for streaming a foreground interaction, got: {first:?}"
    );
}
