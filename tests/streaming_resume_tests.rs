//! Streaming resume tests
//!
//! Tests for stream resumption support via event_id tracking.
//!
//! **Note**: Per the [Interactions API spec](https://ai.google.dev/api/interactions-api#Resource:InteractionSseEvent),
//! `event_id` is **optional** on all SSE event types. The API may or may not include it.
//! These tests verify streaming works correctly regardless of event_id presence,
//! and test resume functionality when event_ids ARE available.
//!
//! # Running Tests
//!
//! ```bash
//! cargo test --test streaming_resume_tests -- --include-ignored --nocapture
//! ```

mod common;

use common::{
    consume_stream, get_client, interaction_builder, stateful_builder,
    validate_response_semantically,
};
use futures_util::StreamExt;
use genai_rs::{InteractionStatus, StreamChunk};

// =============================================================================
// Stream Event ID Tests
// =============================================================================

/// Test that streaming works and optionally includes event_id for position tracking.
///
/// Per API spec, event_id is optional. This test verifies streaming works
/// regardless of whether event_ids are present.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_stream_events_have_event_id() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let stream = interaction_builder(&client)
        .with_text("Write a haiku about Rust programming.")
        .create_stream();

    let result = consume_stream(stream).await;

    println!("\n--- Stream Stats ---");
    println!("Delta chunks received: {}", result.delta_count);
    println!("Event IDs collected: {}", result.event_ids.len());
    println!(
        "Last event_id: {:?}",
        result.last_event_id.as_deref().unwrap_or("(none)")
    );

    // Verify stream produced output (required)
    assert!(
        result.has_output(),
        "Stream should produce deltas or complete response"
    );

    // Log event_id presence (optional per API spec)
    if result.event_ids.is_empty() {
        println!("Note: API did not return event_ids (optional per spec)");
    } else {
        println!("✓ API returned {} event_ids", result.event_ids.len());
        if result.event_ids.len() > 2 {
            println!("Sample event_ids:");
            println!("  First: {}", result.event_ids[0]);
            println!("  Second: {}", result.event_ids[1]);
            println!("  Last: {}", result.event_ids.last().unwrap());
        }
    }
}

/// Test streaming a stored interaction works correctly.
///
/// Verifies stored interactions can be streamed and have an interaction ID.
/// Event_ids are optional per API spec.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_stored_interaction_stream_has_event_ids() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    // Create a streaming request with store enabled
    let stream = stateful_builder(&client)
        .with_text("What is 2 + 2? Answer in one word.")
        .create_stream();

    let result = consume_stream(stream).await;

    println!("\n--- Stored Stream Stats ---");
    println!("Delta chunks: {}", result.delta_count);
    println!("Event IDs: {}", result.event_ids.len());
    println!("Collected text: {}", result.collected_text);

    // Verify streaming worked (required)
    assert!(result.has_output(), "Should receive streaming output");

    // Log event_id presence (optional per API spec)
    if result.event_ids.is_empty() {
        println!("Note: API did not return event_ids (optional per spec)");
    } else {
        println!("✓ API returned {} event_ids", result.event_ids.len());
    }

    // Verify final response has interaction ID (needed for GET streaming)
    if let Some(ref response) = result.final_response {
        assert!(
            response.id.is_some(),
            "Stored interaction should have an ID"
        );
        println!("Interaction ID: {:?}", response.id);
    }
}

/// Test that get_interaction_stream can stream a completed interaction.
///
/// Verifies the resume-by-ID flow works. Event_ids are optional per API spec.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_get_interaction_stream() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    // First, create a stored interaction (non-streaming)
    let response = retry_request!([client] => {
        stateful_builder(&client)
            .with_text("Count from 1 to 5.")
            .create()
            .await
    })
    .expect("Initial request failed");

    println!("Created interaction: {:?}", response.id);
    assert_eq!(response.status, InteractionStatus::Completed);

    let interaction_id = response.id.as_ref().expect("Should have ID");

    // Now stream the completed interaction using get_interaction_stream
    let mut stream = client.get_interaction_stream(interaction_id, None, false);

    let mut delta_count = 0;
    let mut collected_text = String::new();
    let mut event_ids: Vec<String> = Vec::new();
    let mut final_response = None;

    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => {
                // Track event_id (optional per API spec)
                if let Some(ref eid) = event.event_id {
                    event_ids.push(eid.clone());
                }

                match event.chunk {
                    StreamChunk::Delta(delta) => {
                        delta_count += 1;
                        if let Some(text) = delta.as_text() {
                            collected_text.push_str(text);
                            print!("{}", text);
                        }
                    }
                    StreamChunk::Complete(resp) => {
                        println!("\n[GET stream complete: {:?}]", resp.id);
                        final_response = Some(resp);
                    }
                    _ => {}
                }
            }
            Err(e) => {
                eprintln!("Stream error: {:?}", e);
                break;
            }
        }
    }

    println!("\n--- GET Stream Stats ---");
    println!("Delta chunks: {}", delta_count);
    println!("Event IDs: {}", event_ids.len());
    println!("Collected text: {}", collected_text);

    // Note: get_interaction_stream on completed interactions may not replay content
    // depending on API behavior. We verify the API call works without error.
    if delta_count == 0 && final_response.is_none() {
        println!(
            "Note: GET stream returned no content (API may not replay completed interactions)"
        );
    } else {
        println!("✓ GET stream produced output");
    }

    // Log event_id presence (optional per API spec)
    if event_ids.is_empty() {
        println!("Note: API did not return event_ids (optional per spec)");
    } else {
        println!("✓ API returned {} event_ids", event_ids.len());
    }

    // Verify final response if received
    if let Some(resp) = final_response {
        assert_eq!(resp.status, InteractionStatus::Completed);
    }
}

/// Test that StreamEvent wrapper preserves all chunk types.
/// Verifies the wrapper doesn't lose information during streaming.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_stream_event_wrapper_preserves_chunks() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let mut stream = interaction_builder(&client)
        .with_text("Say 'hello' and nothing else.")
        .create_stream();

    let mut saw_delta = false;
    let mut saw_complete = false;
    let mut event_count = 0;

    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => {
                event_count += 1;

                // Verify event structure
                match &event.chunk {
                    StreamChunk::Delta(_) => {
                        saw_delta = true;
                        // Delta events should have event_id from API
                        // (though it may be None for some events)
                    }
                    StreamChunk::Complete(response) => {
                        saw_complete = true;
                        // Complete should have the full response
                        assert!(response.status == InteractionStatus::Completed);
                    }
                    _ => {
                        // Unknown variants - that's fine (future compatibility)
                    }
                }
            }
            Err(e) => {
                eprintln!("Error: {:?}", e);
                break;
            }
        }
    }

    println!("Events received: {}", event_count);
    println!("Saw delta: {}", saw_delta);
    println!("Saw complete: {}", saw_complete);

    // A successful stream should have at least deltas or complete
    assert!(
        saw_delta || saw_complete,
        "Stream should produce Delta or Complete events"
    );
}

// =============================================================================
// Multi-turn Stream Resume Tests
// =============================================================================

/// Test actual stream resume functionality with last_event_id.
/// This verifies that passing a last_event_id resumes from that position.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_stream_resume_with_last_event_id() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    // Create a stored interaction that generates multiple events
    let response = retry_request!([client] => {
        stateful_builder(&client)
            .with_text("Count slowly from 1 to 10, one number per line.")
            .create()
            .await
    })
    .expect("Initial request failed");

    println!("Created interaction: {:?}", response.id);
    assert_eq!(response.status, InteractionStatus::Completed);

    let interaction_id = response.id.as_ref().expect("Should have ID");

    // First, stream the entire interaction to capture all event_ids
    let mut full_stream = client.get_interaction_stream(interaction_id, None, false);

    let mut all_event_ids: Vec<String> = Vec::new();
    let mut full_text = String::new();

    while let Some(result) = full_stream.next().await {
        if let Ok(event) = result {
            if let Some(ref eid) = event.event_id {
                all_event_ids.push(eid.clone());
            }
            if let StreamChunk::Delta(delta) = event.chunk
                && let Some(text) = delta.as_text()
            {
                full_text.push_str(text);
            }
        }
    }

    println!("\n--- Full Stream ---");
    println!("Total event_ids: {}", all_event_ids.len());
    println!("Full text length: {} chars", full_text.len());

    // Skip test if not enough events to test resume
    if all_event_ids.len() < 3 {
        println!("Not enough events to test resume (need at least 3), skipping");
        return;
    }

    // Pick an event_id from the middle to resume from
    let resume_from_index = all_event_ids.len() / 2;
    let resume_event_id = &all_event_ids[resume_from_index];

    println!(
        "\n--- Resuming from event {} of {} ---",
        resume_from_index + 1,
        all_event_ids.len()
    );
    println!("Resume event_id: {}", resume_event_id);

    // Now stream with last_event_id to resume
    let mut resumed_stream =
        client.get_interaction_stream(interaction_id, Some(resume_event_id), false);

    let mut resumed_event_ids: Vec<String> = Vec::new();
    let mut resumed_text = String::new();

    while let Some(result) = resumed_stream.next().await {
        if let Ok(event) = result {
            if let Some(ref eid) = event.event_id {
                resumed_event_ids.push(eid.clone());
            }
            if let StreamChunk::Delta(delta) = event.chunk
                && let Some(text) = delta.as_text()
            {
                resumed_text.push_str(text);
            }
        }
    }

    println!("\n--- Resumed Stream ---");
    println!("Resumed event_ids: {}", resumed_event_ids.len());
    println!("Resumed text length: {} chars", resumed_text.len());

    // Verify resumed stream has fewer events (started partway through)
    // Note: The resumed stream should have events AFTER the resume_event_id
    assert!(
        resumed_event_ids.len() < all_event_ids.len(),
        "Resumed stream should have fewer events. Full: {}, Resumed: {}",
        all_event_ids.len(),
        resumed_event_ids.len()
    );

    // Verify the resumed event_ids are a suffix of the full event_ids
    // (they should be the events after the resume point)
    let expected_count = all_event_ids.len() - resume_from_index - 1;
    println!(
        "Expected ~{} events after resume point, got {}",
        expected_count,
        resumed_event_ids.len()
    );

    // The first resumed event_id should NOT be the resume_event_id
    // (it should be the one after)
    if !resumed_event_ids.is_empty() {
        assert_ne!(
            &resumed_event_ids[0], resume_event_id,
            "Resumed stream should not include the resume_event_id itself"
        );
    }

    println!("\n✓ Stream resume with last_event_id works correctly");
}

/// Test that multi-turn streaming works and preserves context.
///
/// Event_ids are optional per API spec.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_multiturn_stream_event_ids() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    // Turn 1: Establish context
    let response1 = retry_request!([client] => {
        stateful_builder(&client)
            .with_text("My name is Alice. Please remember this.")
            .create()
            .await
    })
    .expect("Turn 1 failed");

    println!("Turn 1 completed: {:?}", response1.id);
    assert_eq!(response1.status, InteractionStatus::Completed);

    // Turn 2: Stream a follow-up question
    let stream = stateful_builder(&client)
        .with_previous_interaction(response1.id.as_ref().expect("id should exist"))
        .with_text("What is my name?")
        .create_stream();

    let result = consume_stream(stream).await;

    println!("\n--- Turn 2 Stream Stats ---");
    println!("Delta chunks: {}", result.delta_count);
    println!("Event IDs: {}", result.event_ids.len());
    println!("Last event_id: {:?}", result.last_event_id);
    println!("Response text: {}", result.collected_text);

    // Verify streaming worked (required)
    assert!(result.has_output(), "Turn 2 stream should produce output");

    // Log event_id presence (optional per API spec)
    if result.event_ids.is_empty() {
        println!("Note: API did not return event_ids (optional per spec)");
    } else {
        println!("✓ API returned {} event_ids", result.event_ids.len());
    }

    // Verify context was preserved - use semantic validation
    let is_valid = validate_response_semantically(
        &client,
        "Turn 1 established 'My name is Alice'. Turn 2 asked 'What is my name?'",
        &result.collected_text,
        "Does this response identify the name as Alice?",
    )
    .await
    .expect("Semantic validation failed");
    assert!(
        is_valid,
        "Response should recall the name from Turn 1. Got: {}",
        result.collected_text
    );
}
