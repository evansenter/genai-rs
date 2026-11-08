//! Stored interactions: get, delete and cancel, the response, and the
//! `stream` of a stored interaction.

use super::*;

#[tokio::test]
async fn interaction_endpoints_send_the_documented_requests() {
    let stub = ok_stub().await;
    let client = stub.client();
    let c = &client;
    assert_wire(
        &stub,
        vec![
            wire(
                "GET",
                "/v1beta/interactions/int-1",
                None,
                c.interactions().get("int-1"),
            ),
            wire(
                "GET",
                "/v1beta/interactions/int-1?include_input=true",
                None,
                c.interactions().get_with_input("int-1"),
            ),
            wire(
                "DELETE",
                "/v1beta/interactions/int-1",
                None,
                c.interactions().delete("int-1"),
            ),
            wire(
                "POST",
                "/v1beta/interactions/int-1/cancel",
                Some(json!({})),
                c.interactions().cancel("int-1"),
            ),
        ],
    )
    .await;
}

#[cfg(not(genai_strict_unknown))]
#[tokio::test]
async fn interaction_response_preserves_unknown_status_input_and_extras() {
    let stub = Stub::replying(vec![
        Reply::json(
            200,
            json!({
                "id": "int-1",
                "status": "paused_for_review",
                "model": "test-model",
                "input": "Hello",
                "steps": [{"type": "model_output", "content": [{"type": "text", "text": "Hi"}]}],
                "created": "2026-09-24T01:00:00Z",
                "region": "eu"
            }),
        ),
        Reply::json(200, json!({"id": "int-1", "status": "cancelled"})),
    ])
    .await;
    let client = stub.client();

    let response = client.interactions().get_with_input("int-1").await.unwrap();
    assert_eq!(
        response.status.unknown_status_type(),
        Some("paused_for_review")
    );
    assert!(matches!(&response.input, Some(InteractionInput::Text(t)) if t == "Hello"));
    assert_eq!(response.as_text(), Some("Hi"));
    assert!(response.created.is_some());
    assert_eq!(response.extra["region"], "eu");
    let back = serde_json::to_value(&response).unwrap();
    assert_eq!(back["status"], "paused_for_review");
    assert_eq!(back["region"], "eu");

    let cancelled = client.interactions().cancel("int-1").await.unwrap();
    assert_eq!(cancelled.status, InteractionStatus::Cancelled);
}

#[tokio::test]
async fn interactions_stream_without_resume_token_streams_the_lifecycle() {
    let stub = Stub::replying(vec![Reply::sse(&[
        "data: {\"event_type\":\"interaction.created\",\"interaction\":{\"id\":\"int-1\",\"status\":\"in_progress\"},\"event_id\":\"e1\"}\n\n",
        "data: {\"event_type\":\"step.start\",\"index\":0,\"step\":{\"type\":\"model_output\",\"content\":[]},\"event_id\":\"e2\"}\n\n",
        "data: {\"event_type\":\"step.delta\",\"index\":0,\"delta\":{\"type\":\"text\",\"text\":\"Hi\"},\"event_id\":\"e3\"}\n\n",
        "data: {\"event_type\":\"step.stop\",\"index\":0,\"event_id\":\"e4\"}\n\n",
        "data: {\"event_type\":\"interaction.completed\",\"interaction\":{\"id\":\"int-1\",\"status\":\"completed\"},\"event_id\":\"e5\"}\n\n",
    ])])
    .await;

    let events: Vec<_> = stub
        .client()
        .interactions()
        .stream("int-1")
        .map(Result::unwrap)
        .collect()
        .await;

    let ids: Vec<_> = events
        .iter()
        .filter_map(|e| e.event_id.as_deref())
        .collect();
    assert_eq!(ids, ["e1", "e2", "e3", "e4", "e5"]);
    assert!(matches!(events[0].chunk, StreamChunk::Created { .. }));
    match &events[4].chunk {
        StreamChunk::Completed {
            interaction: response,
            ..
        } => assert_eq!(response.as_text(), Some("Hi")),
        other => panic!("expected Completed, got {other:?}"),
    }

    let [request] = stub.requests().try_into().unwrap();
    assert_eq!(request.method, "GET");
    assert_eq!(
        request.target,
        "/v1beta/interactions/int-1?alt=sse&stream=true"
    );
    assert_eq!(request.header("x-goog-api-key"), Some("test-key"));
    assert_eq!(request.header("api-revision"), Some(API_REVISION));
}

#[tokio::test]
async fn interactions_stream_preserves_unknown_events() {
    let stub = Stub::replying(vec![Reply::sse(&[
        "data: {\"event_type\":\"interaction.paused\",\"reason\":\"maintenance\",\"event_id\":\"e9\"}\n\n",
        "data: {\"event_type\":\"interaction.completed\",\"interaction\":{\"id\":\"int-1\",\"status\":\"completed\"}}\n\n",
    ])])
    .await;

    let events: Vec<_> = stub
        .client()
        .interactions()
        .resume_stream("int-1", "e8")
        .collect()
        .await;

    let [Ok(event), Ok(done)] = events.as_slice() else {
        panic!("expected two events, got {events:?}");
    };
    assert!(done.is_terminal());
    assert_eq!(event.event_id.as_deref(), Some("e9"));
    assert_eq!(event.chunk.unknown_chunk_type(), Some("interaction.paused"));
    assert_eq!(event.chunk.unknown_data().unwrap()["reason"], "maintenance");
}

#[tokio::test]
async fn interactions_stream_http_error_is_the_only_item() {
    let stub = Stub::replying(vec![not_found()]).await;

    let events: Vec<_> = stub.client().interactions().stream("int-1").collect().await;

    assert!(
        matches!(
            events.as_slice(),
            [Err(GenaiError::Api { status_code: 404, message, .. })]
                if message == "NOT_FOUND: Requested entity was not found."
        ),
        "{events:?}"
    );
}

#[tokio::test]
async fn interactions_stream_is_lazy_owned_and_spawnable() {
    fn assert_send<T: Send>(_: &T) {}

    let stub = Stub::replying(vec![Reply::sse(&[
        "data: {\"event_type\":\"step.delta\",\"index\":0,\"delta\":{\"type\":\"text\",\"text\":\"b\"},\"event_id\":\"e2\"}\n\n",
        "data: {\"event_type\":\"interaction.completed\",\"interaction\":{\"id\":\"int-1\",\"status\":\"completed\"}}\n\n",
    ])])
    .await;

    // Built from a temporary client and ID, both gone before the first poll.
    let stream = {
        let id = String::from("int-1");
        let last = String::from("e1");
        stub.client().interactions().resume_stream(&id, &last)
    };
    assert_send(&stream);
    assert!(stub.requests().is_empty(), "nothing is sent before a poll");

    let events: Vec<_> = tokio::spawn(stream.collect::<Vec<_>>()).await.unwrap();
    let [Ok(event), Ok(_completed)] = events.as_slice() else {
        panic!("expected two events, got {events:?}");
    };
    assert_eq!(event.event_id.as_deref(), Some("e2"));
    let [request] = stub.requests().try_into().unwrap();
    assert_eq!(
        request.target,
        "/v1beta/interactions/int-1?alt=sse&stream=true&last_event_id=e1"
    );

    // A handle future holds only the client borrow, so it can be stored.
    let client = stub.client();
    let stored = client.interactions().get("int-1");
    assert_send(&stored);
    drop(stored);
}
