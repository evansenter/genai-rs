//! Server-Sent Events parsing for the Interactions API stream.
//!
//! Follows the SSE framing rules: an event is the set of field lines up to a
//! blank line, multiple `data:` lines join with `\n`, `:` lines are comments,
//! and one leading space after the colon is dropped.
//!
//! One deviation: a line that starts with `{` is read as data. The API sends
//! a mid-stream error as a bare JSON line with no `data:` prefix, then closes
//! the connection (live 2026-11-01, a recitation block). Under the SSE rules
//! that line is a field named `{"error"` and is dropped, and no SSE field
//! name can start with `{`.
//!
//! # Limits and cut-offs
//!
//! An event may grow to [`MAX_SSE_EVENT_BYTES`] before it ends; past that
//! the stream fails with [`GenaiError::MalformedResponse`] rather than buffering a body
//! that never sends a newline without bound.
//!
//! A body that ends early is [`GenaiError::StreamInterrupted`], whichever way
//! it ends: a transport error mid-body, or a close in the middle of an event
//! whose tail then fails to parse. (A close *between* events is the caller's
//! to judge, since only it knows whether a terminal event arrived.)

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

/// The most bytes one SSE event may take before it ends: 64 MiB.
///
/// Counts the event's data lines so far plus its unfinished line. The
/// largest real events, measured live 2026-11-04 on `DEFAULT_IMAGE_MODEL`
/// at `image_size: "4K"`, are one 16.0 MB `step.delta` of base64 JPEG and a
/// 15.7 MB `thought_signature` delta in the same stream; TTS audio deltas
/// are under 8 KB. 64 MiB leaves four times the largest of those, and still
/// stops a server that never sends a newline.
pub(crate) const MAX_SSE_EVENT_BYTES: usize = 64 * 1024 * 1024;

/// Parses an SSE byte stream into a stream of deserialized events.
///
/// Each dispatched event's joined `data` is parsed as one `T`. Events with
/// no data, and the `[DONE]` marker some endpoints send, are skipped. An
/// event still pending when the body ends is dispatched too, so a stream
/// whose last event lacks the terminating blank line (or newline) is not
/// truncated; if that unterminated tail does not parse, the body was cut
/// off mid-event and the error is [`GenaiError::StreamInterrupted`].
///
/// A transport error while reading the body is
/// [`GenaiError::StreamInterrupted`] too, and an event larger than
/// [`MAX_SSE_EVENT_BYTES`] is [`GenaiError::MalformedResponse`].
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
    parse_sse_stream_with_limit(byte_stream, ctx, request_id, MAX_SSE_EVENT_BYTES)
}

/// [`parse_sse_stream`] with the event size limit as a parameter, so tests
/// can exercise it without allocating 64 MiB.
pub(crate) fn parse_sse_stream_with_limit<'a, T>(
    byte_stream: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'a,
    ctx: &'a HttpContext,
    request_id: u64,
    max_event_bytes: usize,
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
                    buffer.extend_from_slice(&chunk.map_err(cut_off_by_transport)?);
                    false
                }
                None => {
                    if !buffer.is_empty() {
                        buffer.push(b'\n');
                    }
                    true
                }
            };
            // At the end of the body, anything still unterminated (a line
            // with no newline, or data with no blank line after it) is a
            // tail the server may have been cut off writing.
            let open_tail = finished && (!buffer.is_empty() || event.has_data());

            // Walk the complete lines by offset, then drop them in one drain,
            // rather than shifting the buffer down after every line.
            let mut start = 0;
            while let Some(offset) = memchr::memchr(b'\n', &buffer[start + scanned..]) {
                let end = start + scanned + offset;
                scanned = 0;
                let line = str::from_utf8(&buffer[start..end])
                    .map_err(GenaiError::from)
                    .map_err(|e| cut_off_mid_event(open_tail, e))?
                    .trim_end_matches('\r');
                start = end + 1;
                if line.is_empty() {
                    if let Some(parsed) = event
                        .dispatch(ctx, request_id)
                        .map_err(|e| cut_off_mid_event(open_tail, e))?
                    {
                        yield parsed;
                    }
                } else if line.starts_with('{') {
                    // A bare JSON line (see the module docs) is a whole
                    // event: end any event in progress first, so the two
                    // payloads can't be joined into one unparseable value.
                    if let Some(parsed) = event
                        .dispatch(ctx, request_id)
                        .map_err(|e| cut_off_mid_event(open_tail, e))?
                    {
                        yield parsed;
                    }
                    event.push_line(line);
                    if let Some(parsed) = event
                        .dispatch(ctx, request_id)
                        .map_err(|e| cut_off_mid_event(open_tail, e))?
                    {
                        yield parsed;
                    }
                } else {
                    event.push_line(line);
                }
            }
            buffer.drain(..start);
            scanned = buffer.len();

            if finished {
                if let Some(parsed) = event
                    .dispatch(ctx, request_id)
                    .map_err(|e| cut_off_mid_event(open_tail, e))?
                {
                    yield parsed;
                }
                break;
            }

            let pending = buffer.len() + event.data_len();
            if pending > max_event_bytes {
                Err(GenaiError::MalformedResponse(format!(
                    "an SSE event passed {max_event_bytes} bytes without ending \
                     ({pending} bytes buffered); the largest real event, a 4K \
                     image delta, is about 16 MB, so the server is not sending \
                     SSE framing"
                )))?;
            }
        }
    }
}

/// A transport error while reading the body: the stream was cut off.
fn cut_off_by_transport(error: reqwest::Error) -> GenaiError {
    GenaiError::StreamInterrupted {
        message: format!("the connection failed mid-body: {error}"),
        source: Some(error),
    }
}

/// Reclassifies an error from the body's unterminated tail: the server
/// closed the connection partway through writing that event. Errors from
/// complete events pass through unchanged.
fn cut_off_mid_event(open_tail: bool, error: GenaiError) -> GenaiError {
    if !open_tail {
        return error;
    }
    GenaiError::StreamInterrupted {
        message: format!("the body ended in the middle of an event ({error})"),
        source: None,
    }
}

/// The fields of the event currently being read.
#[derive(Default)]
struct PendingEvent {
    event_type: Option<String>,
    data: Option<String>,
}

impl PendingEvent {
    fn has_data(&self) -> bool {
        self.data.is_some()
    }

    fn data_len(&self) -> usize {
        self.data.as_ref().map_or(0, String::len)
    }

    fn push_line(&mut self, line: &str) {
        if line.starts_with(':') {
            return;
        }
        if line.starts_with('{') {
            self.push_data(line);
            return;
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        match field {
            "data" => self.push_data(value),
            "event" => self.event_type = Some(value.to_string()),
            // `id` and `retry` carry nothing this client uses: the event id
            // is repeated inside the JSON payload.
            _ => {}
        }
    }

    fn push_data(&mut self, value: &str) {
        let data = self.data.get_or_insert_with(String::new);
        if !data.is_empty() {
            data.push('\n');
        }
        data.push_str(value);
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
            .map_err(|e| GenaiError::MalformedResponse(format_json_parse_error(data, e)))
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

    // Cut-offs: every early end is StreamInterrupted.

    /// Runs `chunks` (each `Ok` bytes or a transport error) through the
    /// parser with `limit` as the event size cap.
    async fn collect_chunks(
        chunks: Vec<Result<Bytes, reqwest::Error>>,
        limit: usize,
    ) -> Vec<Result<TestMessage, GenaiError>> {
        let ctx = test_ctx();
        parse_sse_stream_with_limit::<TestMessage>(stream::iter(chunks), &ctx, 0, limit)
            .collect()
            .await
    }

    /// A real `reqwest::Error`, built offline, standing in for a reset.
    fn transport_error() -> reqwest::Error {
        reqwest::Client::new()
            .get("not a url")
            .build()
            .expect_err("an unparsable URL fails to build")
    }

    #[tokio::test]
    async fn test_transport_error_mid_body_is_interrupted_with_its_source() {
        let results = collect_chunks(
            vec![
                Ok(Bytes::from_static(b"data: {\"text\":\"a\"}\n\ndata: {\"te")),
                Err(transport_error()),
            ],
            MAX_SSE_EVENT_BYTES,
        )
        .await;
        let [Ok(first), Err(err)] = results.as_slice() else {
            panic!("expected one event then the cut-off: {results:?}");
        };
        assert_eq!(first.text, "a");
        assert!(
            matches!(
                err,
                GenaiError::StreamInterrupted {
                    source: Some(_),
                    ..
                }
            ),
            "{err:?}"
        );
        assert!(err.is_retryable());
        assert!(std::error::Error::source(err).is_some(), "keeps the cause");
    }

    #[tokio::test]
    async fn test_body_ending_mid_event_is_interrupted() {
        // Cut inside the JSON, inside a multi-byte character, and before the
        // second line of a multi-line event.
        for body in [
            &b"data: {\"text\":\"a\"}\n\ndata: {\"text\":\"b"[..],
            &b"data: {\"text\":\"\xe6\x97"[..],
            &b"data: {\"text\":\ndata: \"jo"[..],
            &b"data: {\"text\":\n"[..],
            // The `\r` of a CRLF blank line, with the event's JSON cut short.
            &b"data: {\"te\r\n\r"[..],
        ] {
            let results =
                collect_chunks(vec![Ok(Bytes::from_static(body))], MAX_SSE_EVENT_BYTES).await;
            let last = results.last().expect("at least the error");
            assert!(
                matches!(last, Err(GenaiError::StreamInterrupted { source: None, message })
                    if message.contains("middle of an event")),
                "{:?}: {last:?}",
                String::from_utf8_lossy(body)
            );
        }
    }

    #[tokio::test]
    async fn test_complete_but_invalid_event_is_still_a_parse_error() {
        // The blank line arrived, so nothing was cut off: the event itself
        // is bad, which retrying won't fix.
        let results = collect_chunks(
            vec![Ok(Bytes::from_static(b"data: {\"text\":\"a\"\n\n"))],
            MAX_SSE_EVENT_BYTES,
        )
        .await;
        assert!(
            matches!(results.as_slice(), [Err(GenaiError::MalformedResponse(_))]),
            "{results:?}"
        );
        // A complete event in an earlier read stays a parse error even when
        // the body ends right after it.
        let results = collect_chunks(
            vec![
                Ok(Bytes::from_static(b"data: {bad}\n\n")),
                Ok(Bytes::from_static(b"data: {\"text\":\"b\"}")),
            ],
            MAX_SSE_EVENT_BYTES,
        )
        .await;
        assert!(
            matches!(results.first(), Some(Err(GenaiError::MalformedResponse(_)))),
            "{results:?}"
        );
    }

    // The event size cap.

    #[tokio::test]
    async fn test_line_without_newline_past_the_cap_is_an_error() {
        let results = collect_chunks(
            vec![
                Ok(Bytes::from_static(b"data: {\"text\":\"a\"}\n\n")),
                Ok(Bytes::from(vec![b'x'; 40])),
                Ok(Bytes::from(vec![b'x'; 40])),
                Ok(Bytes::from(vec![b'x'; 40])),
            ],
            64,
        )
        .await;
        let [Ok(first), Err(err)] = results.as_slice() else {
            panic!("expected the small event, then the cap error: {results:?}");
        };
        assert_eq!(first.text, "a");
        assert!(
            matches!(err, GenaiError::MalformedResponse(message) if message.contains("64 bytes")),
            "{err:?}"
        );
        assert!(
            !err.is_retryable(),
            "a server ignoring SSE framing won't change"
        );
    }

    #[tokio::test]
    async fn test_data_lines_without_a_blank_line_past_the_cap_are_an_error() {
        let lines = (0..10)
            .map(|_| Ok(Bytes::from_static(b"data: 0123456789\n")))
            .collect();
        let results = collect_chunks(lines, 64).await;
        assert!(
            matches!(results.as_slice(), [Err(GenaiError::MalformedResponse(message))]
                if message.contains("without ending")),
            "{results:?}"
        );
    }

    #[tokio::test]
    async fn test_event_at_the_cap_is_delivered() {
        // 64 bytes of event, arriving a byte at a time: never over the cap.
        let body = format!("data: {{\"text\":\"{}\"}}\n\n", "y".repeat(64 - 17));
        assert_eq!(body.len(), 66);
        let chunks = body
            .bytes()
            .map(|byte| Ok(Bytes::from(vec![byte])))
            .collect();
        let results = collect_chunks(chunks, 64).await;
        assert!(
            matches!(results.as_slice(), [Ok(m)] if m.text.len() == 47),
            "{results:?}"
        );
    }

    #[test]
    fn test_cap_leaves_room_for_the_largest_measured_event() {
        // The 4K image delta measured live 2026-11-04 (see the constant).
        const LARGEST_MEASURED: usize = 16_005_225;
        const { assert!(MAX_SSE_EVENT_BYTES >= 4 * LARGEST_MEASURED) };
    }

    #[tokio::test]
    async fn test_real_cap_passes_a_4k_image_sized_event_and_stops_a_runaway_line() {
        // A 16 MB event, as the live 4K image delta was, in 64 KB reads.
        let big = "z".repeat(16_005_225);
        let body = format!("data: {{\"text\":\"{big}\"}}\n\n").into_bytes();
        let chunks = body
            .chunks(64 * 1024)
            .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
            .collect();
        let results = collect_chunks(chunks, MAX_SSE_EVENT_BYTES).await;
        assert!(
            matches!(results.as_slice(), [Ok(m)] if m.text.len() == big.len()),
            "the largest real event must pass"
        );

        let runaway = Bytes::from(vec![b'x'; MAX_SSE_EVENT_BYTES + 1]);
        let results = collect_chunks(vec![Ok(runaway)], MAX_SSE_EVENT_BYTES).await;
        assert!(
            matches!(results.as_slice(), [Err(GenaiError::MalformedResponse(_))]),
            "{:?}",
            results.iter().map(Result::is_ok).collect::<Vec<_>>()
        );
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
        let data = format!("data: {{\"text\":\"{large_text}\"}}\n\n");
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
            data.extend_from_slice(format!("data: {{\"text\":\"Message {i}\"}}\n\n").as_bytes());
        }

        let byte_stream = stream::iter(vec![Ok(Bytes::from(data))]);
        let ctx = test_ctx();
        let parsed_stream = parse_sse_stream::<TestMessage>(byte_stream, &ctx, 0);
        pin_mut!(parsed_stream);

        let mut count = 0;
        while let Some(result) = parsed_stream.next().await {
            assert!(result.is_ok());
            let message = result.unwrap();
            assert_eq!(message.text, format!("Message {count}"));
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
