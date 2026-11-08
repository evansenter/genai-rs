//! `interactions().stream` and `resume_stream`: following a background interaction and resuming
//! with `last_event_id`.
//!
//! Observed live 2026-09-24: streaming retrieval works only for background
//! interactions (a foreground one is rejected with 400), and only text deltas
//! streamed live carry an `event_id` (a long answer yields three or four).
//! Observed 2026-09-26: a resumed replay can tag fewer deltas than the live
//! stream did, so resumption is checked on the text, not the id list.
//! Observed 2026-11-04: thought deltas carry an `event_id` too (the first id
//! decoded to `0_thought_4:0`), so an id can come before any text.
//!
//! Every interaction a test starts is cancelled and deleted when it ends,
//! pass, fail or timeout ([`cleaning_up`]): a background story left running
//! keeps generating against the shared key (#501).
//!
//! ```bash
//! cargo nextest run --test streaming_resume_tests --run-ignored all
//! ```

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a test reports a failure by panicking"
)]

mod common;

use common::{consume_stream, get_client, stateful_builder, with_timeout};
use futures_util::{FutureExt, StreamExt};
use genai_rs::{Client, GenaiError, InteractionStatus};
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const STREAM_BUDGET: Duration = Duration::from_secs(300);

/// The interactions a test started, for [`cleaning_up`] to remove.
#[derive(Clone, Default)]
struct Started(Arc<Mutex<Vec<String>>>);

impl Started {
    fn push(&self, id: &str) {
        self.0
            .lock()
            .expect("id list poisoned")
            .push(id.to_string());
    }

    fn ids(&self) -> Vec<String> {
        self.0.lock().expect("id list poisoned").clone()
    }
}

/// Runs `body` under the stream budget, then cancels and deletes every
/// interaction it recorded in `Started`, including when it panics or runs
/// out of time.
async fn cleaning_up<F, Fut>(client: &Client, body: F)
where
    F: FnOnce(Started) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let started = Started::default();
    let outcome = AssertUnwindSafe(with_timeout(STREAM_BUDGET, body(started.clone())))
        .catch_unwind()
        .await;
    for id in started.ids() {
        // A finished interaction answers the cancel with an error, which
        // is fine: the delete is what matters.
        let _ = client.interactions().cancel(&id).await;
        match client.interactions().delete(&id).await {
            Ok(_)
            | Err(GenaiError::Api {
                status_code: 404, ..
            }) => {}
            Err(e) => eprintln!("cleanup failed for interaction {id}: {e:?}"),
        }
    }
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

/// Starts a background interaction long enough to still be generating text
/// when retrieval connects, recording it in `started` and returning its id.
/// Only deltas streamed live carry an `event_id`, so a short answer finished
/// before the connection yields one.
async fn start_background(client: &Client, started: &Started) -> String {
    let response = retry_request!([client] => {
        stateful_builder(&client)
            .with_text("Write a 2000-word story about a lighthouse keeper, in many short paragraphs.")
            .with_thinking_level(genai_rs::ThinkingLevel::Low)
            .with_background(true)
            .create()
            .await
    })
    .expect("background create failed");
    let id = response
        .id
        .expect("background interaction should have an id");
    started.push(&id);
    id
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_interactions_stream_follows_background_interaction() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let owned = client.clone();
    cleaning_up(&client, |started| async move {
        let client = owned;
        let id = start_background(&client, &started).await;
        let result = consume_stream(client.interactions().stream(&id)).await;

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
/// Needs an id with text on both sides of it, so there is a skipped prefix
/// and a replayed suffix to compare. The server decides which events carry
/// ids, so a run without one starts a new interaction (up to three) rather
/// than weakening the assertion.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_stream_resume_with_last_event_id() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let owned = client.clone();
    cleaning_up(&client, |started| async move {
        let client = owned;
        let mut seen = Vec::new();
        let (id, full, at) = loop {
            let id = start_background(&client, &started).await;
            let full = consume_stream(client.interactions().stream(&id)).await;
            let len = full.collected_text.len();
            let at = full
                .event_text_offsets
                .iter()
                .position(|&offset| offset > 0 && offset < len);
            seen.push((full.event_ids.clone(), full.event_text_offsets.clone(), len));
            if let Some(at) = at {
                break (id, full, at);
            }
            assert!(
                seen.len() < 3,
                "no retrieval stream carried an event_id with text on both sides \
                 (ids, text offsets, text length): {seen:?}"
            );
        };

        let resume_from = &full.event_ids[at];
        let skipped = full.event_text_offsets[at];
        let resumed = consume_stream(client.interactions().resume_stream(&id, resume_from)).await;

        // Which deltas carry an id differs between the live stream and the
        // replay (a replay can tag fewer), so the ids cannot be compared as
        // lists. The text can: it must be a non-empty tail of the full text
        // that leaves out everything up to the resume point.
        assert!(
            !resumed.collected_text.is_empty()
                && resumed.collected_text.len() <= full.collected_text.len() - skipped
                && full.collected_text.ends_with(&resumed.collected_text),
            "resuming from {resume_from} (after {skipped} bytes) should replay only the text \
             after it: got {} of {} bytes, tail match: {}",
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
async fn test_interactions_stream_rejects_foreground_interaction() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let owned = client.clone();
    cleaning_up(&client, |started| async move {
        let client = owned;
        let response = retry_request!([client] => {
            stateful_builder(&client)
                .with_text("Say hello.")
                .create()
                .await
        })
        .expect("create failed");
        let id = response.id.expect("stored interaction should have an id");
        started.push(&id);

        let mut stream = client.interactions().stream(&id);
        let first = stream.next().await;
        println!("streaming a foreground interaction: {first:?}");
        assert!(
            matches!(
                &first,
                Some(Err(GenaiError::Api {
                    status_code: 400,
                    message,
                    ..
                })) if message.contains("Streaming retrieval of interactions is not supported")
            ),
            "expected the API's 400 for streaming a foreground interaction, got: {first:?}"
        );
    })
    .await;
}
