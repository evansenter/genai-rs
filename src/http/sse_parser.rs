/// SSE (Server-Sent Events) parsing utilities
///
/// This module provides generic utilities for parsing SSE streams from the Gemini API.
/// SSE format consists of lines starting with "data: " followed by JSON payloads.
use super::context::HttpContext;
use super::error_helpers::format_json_parse_error;
use crate::errors::GenaiError;
use crate::wire::WireEvent;
use async_stream::try_stream;
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use serde::de::DeserializeOwned;
use std::str;
use tracing::debug;

/// Parses an SSE byte stream into a stream of deserialized objects.
///
/// This function handles the low-level SSE protocol parsing:
/// - Buffers incoming bytes
/// - Splits on newlines
/// - Extracts "data: " prefixed lines
/// - Deserializes JSON payloads
///
/// Wire inspectors installed on the client see every `data:` payload and
/// every `event:` line as [`WireEvent::SseFrame`] events.
///
/// # Type Parameters
///
/// * `T` - The type to deserialize each SSE data payload into
///
/// # Arguments
///
/// * `byte_stream` - An async stream of byte chunks from the HTTP response
/// * `ctx` - HTTP context carrying the wire inspectors
/// * `request_id` - Request ID for wire-event correlation
///
/// # Returns
///
/// A stream that yields deserialized objects of type `T` or errors
///
/// # Example
///
/// ```ignore
/// let byte_stream = response.bytes_stream();
/// let parsed_stream = parse_sse_stream::<MyResponseType>(byte_stream, ctx, request_id);
///
/// while let Some(result) = parsed_stream.next().await {
///     let response = result?;
///     // Process response...
/// }
/// ```
pub fn parse_sse_stream<'a, T>(
    byte_stream: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'a,
    ctx: &'a HttpContext,
    request_id: u64,
) -> impl Stream<Item = Result<T, GenaiError>> + Send + 'a
where
    T: DeserializeOwned + Send + 'a,
{
    try_stream! {
        futures_util::pin_mut!(byte_stream);
        let mut buffer = Vec::new();

        while let Some(chunk_result) = byte_stream.next().await {
            let chunk: Bytes = chunk_result?;
            buffer.extend_from_slice(&chunk);

            while let Some(newline_pos) = buffer.iter().position(|&b| b == b'\n') {
                let line_bytes = buffer.drain(..=newline_pos).collect::<Vec<u8>>();
                let line = str::from_utf8(&line_bytes)?.trim_end_matches(|c| c == '\n' || c == '\r');

                if line.starts_with("data:") {
                    let json_data = line
                        .strip_prefix("data:")
                        .expect("Line should start with 'data:' prefix after check");
                    let json_data = json_data.trim_start();

                    // Skip empty data lines and [DONE] markers (used by some SSE endpoints)
                    if !json_data.is_empty() && json_data != "[DONE]" {
                        debug!("SSE raw data: {}", json_data);

                        if ctx.has_inspectors() {
                            ctx.emit(WireEvent::SseFrame {
                                id: request_id,
                                event_type: None,
                                data: json_data.to_string(),
                            });
                        }

                        let parsed: T = serde_json::from_str(json_data).map_err(|e| {
                            let context_msg = format_json_parse_error(json_data, e);
                            GenaiError::Parse(context_msg)
                        })?;
                        yield parsed;
                    }
                } else if let Some(event_type) = line.strip_prefix("event:") {
                    // Surface event-type lines to wire inspectors. The
                    // Interactions API carries its event type inside the data
                    // payload, so these are informational only.
                    let event_type = event_type.trim_start();
                    if !event_type.is_empty() && ctx.has_inspectors() {
                        ctx.emit(WireEvent::SseFrame {
                            id: request_id,
                            event_type: Some(event_type.to_string()),
                            data: String::new(),
                        });
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::WireInspector;
    use futures_util::{pin_mut, stream};
    use serde::Deserialize;
    use std::sync::{Arc, Mutex};

    #[derive(Debug, Deserialize, PartialEq)]
    struct TestMessage {
        text: String,
    }

    /// Context with no inspectors installed (the default configuration).
    fn test_ctx() -> HttpContext {
        HttpContext::new(reqwest::Client::new(), "test-key".to_string(), vec![])
    }

    /// Test inspector that records every event it receives.
    struct Collector {
        events: Mutex<Vec<WireEvent>>,
    }

    impl WireInspector for Collector {
        fn on_event(&self, event: &WireEvent) {
            self.events.lock().unwrap().push(event.clone());
        }
    }

    #[tokio::test]
    async fn test_parse_sse_stream_emits_wire_frames() {
        let collector = Arc::new(Collector {
            events: Mutex::new(Vec::new()),
        });
        let ctx = HttpContext::new(
            reqwest::Client::new(),
            "test-key".to_string(),
            vec![collector.clone()],
        );

        let data = b"event: message\ndata: {\"text\":\"Hello\"}\n\ndata: {\"text\":\"World\"}\n\n"
            .to_vec();
        let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);

        let parsed_stream = parse_sse_stream::<TestMessage>(byte_stream, &ctx, 42);
        pin_mut!(parsed_stream);
        while let Some(result) = parsed_stream.next().await {
            result.unwrap();
        }

        let events = collector.events.lock().unwrap();
        assert_eq!(events.len(), 3, "expected event: line + 2 data frames");
        match &events[0] {
            WireEvent::SseFrame {
                id,
                event_type,
                data,
            } => {
                assert_eq!(*id, 42);
                assert_eq!(event_type.as_deref(), Some("message"));
                assert!(data.is_empty());
            }
            other => panic!("expected SseFrame, got {other:?}"),
        }
        match &events[1] {
            WireEvent::SseFrame {
                id,
                event_type,
                data,
            } => {
                assert_eq!(*id, 42);
                assert_eq!(*event_type, None);
                assert_eq!(data, r#"{"text":"Hello"}"#);
            }
            other => panic!("expected SseFrame, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_parse_sse_stream_single_message() {
        // Simulate SSE stream with a single message
        let data = b"data: {\"text\":\"Hello\"}\n\n".to_vec();
        let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);

        let ctx = test_ctx();
        let parsed_stream = parse_sse_stream::<TestMessage>(byte_stream, &ctx, 0);
        pin_mut!(parsed_stream);

        let result = parsed_stream.next().await;
        assert!(result.is_some());

        let message = result.unwrap().unwrap();
        assert_eq!(message.text, "Hello");
    }

    #[tokio::test]
    async fn test_parse_sse_stream_multiple_messages() {
        // Simulate SSE stream with multiple messages
        let data = b"data: {\"text\":\"First\"}\n\ndata: {\"text\":\"Second\"}\n\n".to_vec();
        let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);

        let ctx = test_ctx();
        let parsed_stream = parse_sse_stream::<TestMessage>(byte_stream, &ctx, 0);
        pin_mut!(parsed_stream);

        let first = parsed_stream.next().await.unwrap().unwrap();
        assert_eq!(first.text, "First");

        let second = parsed_stream.next().await.unwrap().unwrap();
        assert_eq!(second.text, "Second");
    }

    #[tokio::test]
    async fn test_parse_sse_stream_chunked_data() {
        // Simulate chunked SSE stream where data arrives in pieces
        let chunk1 = b"data: {\"te".to_vec();
        let chunk2 = b"xt\":\"Hello\"}\n\n".to_vec();
        let byte_stream = stream::iter(vec![Ok(Bytes::from(chunk1)), Ok(Bytes::from(chunk2))]);

        let ctx = test_ctx();
        let parsed_stream = parse_sse_stream::<TestMessage>(byte_stream, &ctx, 0);
        pin_mut!(parsed_stream);

        let message = parsed_stream.next().await.unwrap().unwrap();
        assert_eq!(message.text, "Hello");
    }

    #[tokio::test]
    async fn test_parse_sse_stream_ignores_non_data_lines() {
        // SSE streams can have comments and other lines we should ignore
        let data = b": comment\ndata: {\"text\":\"Hello\"}\n\nevent: test\n\n".to_vec();
        let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);

        let ctx = test_ctx();
        let parsed_stream = parse_sse_stream::<TestMessage>(byte_stream, &ctx, 0);
        pin_mut!(parsed_stream);

        let message = parsed_stream.next().await.unwrap().unwrap();
        assert_eq!(message.text, "Hello");

        // Should have no more messages
        assert!(parsed_stream.next().await.is_none());
    }

    #[tokio::test]
    async fn test_parse_sse_stream_empty_data_line() {
        // Empty "data: " lines should be skipped
        let data = b"data: \ndata: {\"text\":\"Hello\"}\n\n".to_vec();
        let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);

        let ctx = test_ctx();
        let parsed_stream = parse_sse_stream::<TestMessage>(byte_stream, &ctx, 0);
        pin_mut!(parsed_stream);

        let message = parsed_stream.next().await.unwrap().unwrap();
        assert_eq!(message.text, "Hello");
    }

    #[tokio::test]
    async fn test_parse_sse_stream_invalid_json() {
        // Invalid JSON should return an error
        let data = b"data: {invalid json}\n\n".to_vec();
        let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);

        let ctx = test_ctx();
        let parsed_stream = parse_sse_stream::<TestMessage>(byte_stream, &ctx, 0);
        pin_mut!(parsed_stream);

        let result = parsed_stream.next().await.unwrap();
        assert!(result.is_err());
    }

    // Stress tests for SSE parser

    #[tokio::test]
    async fn test_parse_sse_stream_large_message() {
        // Test with a very large message (>1MB)
        let large_text = "x".repeat(1_000_000);
        let data = format!("data: {{\"text\":\"{}\"}}\n\n", large_text);
        let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);

        let ctx = test_ctx();
        let parsed_stream = parse_sse_stream::<TestMessage>(byte_stream, &ctx, 0);
        pin_mut!(parsed_stream);

        let result = parsed_stream.next().await;
        assert!(result.is_some());
        let message = result.unwrap().unwrap();
        assert_eq!(message.text.len(), 1_000_000);
    }

    #[tokio::test]
    async fn test_parse_sse_stream_many_rapid_messages() {
        // Test with many messages in rapid succession
        let mut data = Vec::new();
        for i in 0..1000 {
            data.extend_from_slice(format!("data: {{\"text\":\"Message {}\"}}\n\n", i).as_bytes());
        }

        let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);
        let ctx = test_ctx();
        let parsed_stream = parse_sse_stream::<TestMessage>(byte_stream, &ctx, 0);
        pin_mut!(parsed_stream);

        let mut count = 0;
        while let Some(result) = parsed_stream.next().await {
            assert!(result.is_ok());
            let message = result.unwrap();
            assert_eq!(message.text, format!("Message {}", count));
            count += 1;
        }

        assert_eq!(count, 1000);
    }

    #[tokio::test]
    async fn test_parse_sse_stream_very_small_chunks() {
        // Test with extremely small chunks (1 byte at a time for part of the message)
        let full_message = b"data: {\"text\":\"Hello\"}\n\n";
        let chunks: Vec<Result<Bytes, reqwest::Error>> = full_message
            .iter()
            .map(|&byte| Ok(Bytes::from(vec![byte])))
            .collect();

        let byte_stream = stream::iter(chunks);
        let ctx = test_ctx();
        let parsed_stream = parse_sse_stream::<TestMessage>(byte_stream, &ctx, 0);
        pin_mut!(parsed_stream);

        let message = parsed_stream.next().await.unwrap().unwrap();
        assert_eq!(message.text, "Hello");
    }

    #[tokio::test]
    async fn test_parse_sse_stream_mixed_line_endings() {
        // Test with different line ending types (\n, \r\n)
        let data = b"data: {\"text\":\"First\"}\n\ndata: {\"text\":\"Second\"}\r\n\r\ndata: {\"text\":\"Third\"}\n\n".to_vec();
        let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);

        let ctx = test_ctx();
        let parsed_stream = parse_sse_stream::<TestMessage>(byte_stream, &ctx, 0);
        pin_mut!(parsed_stream);

        let first = parsed_stream.next().await.unwrap().unwrap();
        assert_eq!(first.text, "First");

        let second = parsed_stream.next().await.unwrap().unwrap();
        assert_eq!(second.text, "Second");

        let third = parsed_stream.next().await.unwrap().unwrap();
        assert_eq!(third.text, "Third");
    }

    #[tokio::test]
    async fn test_parse_sse_stream_with_unicode() {
        // Test with Unicode characters in SSE stream
        let data = b"data: {\"text\":\"Hello \\u4e16\\u754c \\ud83c\\udf0d\"}\n\n".to_vec();
        let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);

        let ctx = test_ctx();
        let parsed_stream = parse_sse_stream::<TestMessage>(byte_stream, &ctx, 0);
        pin_mut!(parsed_stream);

        let message = parsed_stream.next().await.unwrap().unwrap();
        // The JSON parser should decode \u sequences to actual Unicode characters
        assert_eq!(message.text, "Hello 世界 🌍");
    }
}
