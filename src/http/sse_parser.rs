//! Server-Sent Events parsing for the Interactions API stream.
//!
//! Follows the SSE framing rules: an event is the set of field lines up to a
//! blank line, multiple `data:` lines join with `\n`, `:` lines are comments,
//! and one leading space after the colon is dropped.

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

/// Parses an SSE byte stream into a stream of deserialized events.
///
/// Each dispatched event's joined `data` is parsed as one `T`. Events with
/// no data, and the `[DONE]` marker some endpoints send, are skipped. An
/// event still pending when the body ends is dispatched too, so a stream
/// whose last event lacks the terminating blank line (or newline) is not
/// truncated.
///
/// Wire inspectors see one [`WireEvent::SseFrame`] per dispatched event.
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
        let mut buffer: Vec<u8> = Vec::new();
        // Leading bytes of `buffer` already searched for a newline. Without
        // it, a multi-megabyte event (image output) arriving in 8 KB chunks
        // is rescanned from the start on every chunk: quadratic.
        let mut scanned = 0;
        let mut event = PendingEvent::default();

        loop {
            let finished = match byte_stream.next().await {
                Some(chunk) => {
                    buffer.extend_from_slice(&chunk?);
                    false
                }
                None => {
                    if !buffer.is_empty() {
                        buffer.push(b'\n');
                    }
                    true
                }
            };

            // Walk the complete lines by offset, then drop them in one drain,
            // rather than shifting the buffer down after every line.
            let mut start = 0;
            while let Some(offset) = memchr::memchr(b'\n', &buffer[start + scanned..]) {
                let end = start + scanned + offset;
                scanned = 0;
                let line = str::from_utf8(&buffer[start..end])?.trim_end_matches('\r');
                start = end + 1;
                if line.is_empty() {
                    if let Some(parsed) = event.dispatch(ctx, request_id)? {
                        yield parsed;
                    }
                } else {
                    event.push_line(line);
                }
            }
            buffer.drain(..start);
            scanned = buffer.len();

            if finished {
                if let Some(parsed) = event.dispatch(ctx, request_id)? {
                    yield parsed;
                }
                break;
            }
        }
    }
}

/// The fields of the event currently being read.
#[derive(Default)]
struct PendingEvent {
    event_type: Option<String>,
    data: Option<String>,
}

impl PendingEvent {
    fn push_line(&mut self, line: &str) {
        if line.starts_with(':') {
            return;
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        match field {
            "data" => {
                let data = self.data.get_or_insert_with(String::new);
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(value);
            }
            "event" => self.event_type = Some(value.to_string()),
            // `id` and `retry` carry nothing this client uses: the event id
            // is repeated inside the JSON payload.
            _ => {}
        }
    }

    /// Ends the current event and parses its data, if it has any.
    fn dispatch<T: DeserializeOwned>(
        &mut self,
        ctx: &HttpContext,
        request_id: u64,
    ) -> Result<Option<T>, GenaiError> {
        let Self { event_type, data } = std::mem::take(self);
        let Some(data) = data else {
            return Ok(None);
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            return Ok(None);
        }

        debug!("SSE raw data: {}", data);
        if ctx.has_inspectors() {
            ctx.emit(WireEvent::SseFrame {
                id: request_id,
                event_type,
                data: data.to_string(),
            });
        }

        serde_json::from_str(data)
            .map(Some)
            .map_err(|e| GenaiError::Parse(format_json_parse_error(data, e)))
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
        assert_eq!(events.len(), 2, "one frame per dispatched event");
        match &events[0] {
            WireEvent::SseFrame {
                id,
                event_type,
                data,
            } => {
                assert_eq!(*id, 42);
                assert_eq!(event_type.as_deref(), Some("message"));
                assert_eq!(data, r#"{"text":"Hello"}"#);
            }
            other => panic!("expected SseFrame, got {other:?}"),
        }
        match &events[1] {
            WireEvent::SseFrame {
                event_type, data, ..
            } => {
                assert_eq!(*event_type, None);
                assert_eq!(data, r#"{"text":"World"}"#);
            }
            other => panic!("expected SseFrame, got {other:?}"),
        }
    }

    async fn collect(data: &'static [u8]) -> Vec<Result<TestMessage, GenaiError>> {
        let ctx = test_ctx();
        let byte_stream = stream::iter(vec![Ok(Bytes::from_static(data))]);
        parse_sse_stream::<TestMessage>(byte_stream, &ctx, 0)
            .collect()
            .await
    }

    fn texts(results: Vec<Result<TestMessage, GenaiError>>) -> Vec<String> {
        results.into_iter().map(|r| r.unwrap().text).collect()
    }

    #[tokio::test]
    async fn test_final_event_without_trailing_newline_is_dispatched() {
        assert_eq!(
            texts(collect(b"data: {\"text\":\"a\"}\n\ndata: {\"text\":\"b\"}").await),
            ["a", "b"]
        );
        // Terminated line but no blank line after it.
        assert_eq!(texts(collect(b"data: {\"text\":\"c\"}\n").await), ["c"]);
    }

    #[tokio::test]
    async fn test_multi_line_data_joins_into_one_event() {
        let results = collect(b"data: {\"text\":\ndata: \"joined\"}\n\n").await;
        assert_eq!(texts(results), ["joined"]);
    }

    #[tokio::test]
    async fn test_crlf_blank_line_ends_an_event() {
        let results = collect(
            b"event: step.delta\r\ndata: {\"text\":\"a\"}\r\n\r\ndata: {\"text\":\"b\"}\r\n\r\n",
        )
        .await;
        assert_eq!(texts(results), ["a", "b"]);
    }

    #[tokio::test]
    async fn test_done_marker_comments_and_dataless_events_are_skipped() {
        let results = collect(
            b": keep-alive\n\nevent: ping\n\nid: 7\ndata: {\"text\":\"a\"}\n\ndata: [DONE]\n\n",
        )
        .await;
        assert_eq!(texts(results), ["a"]);
    }

    #[tokio::test]
    async fn test_data_without_space_after_colon() {
        assert_eq!(texts(collect(b"data:{\"text\":\"a\"}\n\n").await), ["a"]);
    }

    #[tokio::test]
    async fn test_invalid_utf8_is_an_error() {
        let results = collect(b"data: \xff\n\n").await;
        assert!(matches!(results.as_slice(), [Err(GenaiError::Utf8(_))]));
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

    /// A body exercising every framing rule, with an event large enough to
    /// span many chunks and multi-byte UTF-8 for cuts to land inside.
    fn framing_corpus() -> (Vec<u8>, Vec<String>) {
        let big = "é".repeat(20_000);
        let body = format!(
            ": comment\n\
             event: message\ndata: {{\"text\":\"a\"}}\n\n\
             data: {{\"text\":\r\ndata: \"b\"}}\r\n\r\n\
             data:{{\"text\":\"{big}\"}}\n\n\
             data: [DONE]\n\n\
             data: {{\"text\":\"日本\"}}"
        );
        let expected = vec!["a".to_string(), "b".to_string(), big, "日本".to_string()];
        (body.into_bytes(), expected)
    }

    proptest::proptest! {
        /// Where the chunk boundaries fall (including empty chunks) must not
        /// change which events come out.
        #[test]
        fn chunk_boundaries_do_not_change_events(
            cuts in proptest::collection::vec(proptest::num::usize::ANY, 0..64)
        ) {
            let (body, expected) = framing_corpus();
            let mut cuts: Vec<usize> = cuts.into_iter().map(|c| c % (body.len() + 1)).collect();
            cuts.sort_unstable();
            let mut chunks: Vec<Result<Bytes, reqwest::Error>> = Vec::new();
            let mut from = 0;
            for cut in cuts.into_iter().chain([body.len()]) {
                chunks.push(Ok(Bytes::copy_from_slice(&body[from..cut])));
                from = cut;
            }

            let ctx = test_ctx();
            let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let results = runtime.block_on(
                parse_sse_stream::<TestMessage>(stream::iter(chunks), &ctx, 0).collect(),
            );
            proptest::prop_assert_eq!(texts(results), expected);
        }
    }
}
