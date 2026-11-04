//! The fuzz entry points on hand-written inputs, plus every input a fuzz
//! run has failed on (kept as a regression test once fixed).

use super::*;

#[test]
fn sse_parser_accepts_the_live_lifecycle() {
    // Shapes from a live text stream (2026-11-04), trimmed.
    let body = concat!(
        "\u{3}", // read-size header: three bytes follow
        "\u{4}\u{0}\u{9}",
        "event: interaction.created\n",
        "data: {\"event_type\":\"interaction.created\",\"interaction\":{\"id\":\"i1\",\"status\":\"in_progress\"}}\n\n",
        "event: step.delta\r\n",
        "data: {\"event_type\":\"step.delta\",\"index\":0,\"delta\":{\"type\":\"text\",\"text\":\"Hi\"}}\r\n\r\n",
        "{\"error\":{\"message\":\"blocked\",\"code\":\"recitation\"}}\n",
        "event: done\ndata: [DONE]\n\n",
    );
    sse_parser(body.as_bytes());
}

#[test]
fn sse_parser_handles_degenerate_inputs() {
    for input in [
        &b""[..],
        b"\x00",
        b"\x07\xff\xff\xff\xff\xff\xff\xff",
        b"\x01\x00data: {\"a\":",
        b"\x00data: \xe6\x97\n\n",
        b"\x00\r\r\r\n\n\n",
        b"\x02\x00\x00:\n:\n{\n",
    ] {
        sse_parser(input);
    }
}

#[test]
fn split_reads_covers_the_body_in_order() {
    let (reads, body) = split_reads(b"\x02\x00\x01abcdefg");
    assert_eq!(body, b"abcdefg");
    let sizes: Vec<usize> = reads.iter().map(Bytes::len).collect();
    assert_eq!(sizes, [1, 2, 1, 2, 1]);
    assert_eq!(reads.concat(), body);
}

#[test]
fn stream_event_runs_a_full_lifecycle() {
    let body = concat!(
        "{\"event_type\":\"interaction.created\",\"interaction\":{\"id\":\"i1\",\"status\":\"in_progress\"}}\n",
        "{\"event_type\":\"step.start\",\"index\":0,\"step\":{\"type\":\"function_call\",\"id\":\"c1\",\"name\":\"f\",\"arguments\":{}}}\n",
        "{\"event_type\":\"step.delta\",\"index\":0,\"delta\":{\"type\":\"arguments_delta\",\"arguments\":\"{\\\"a\\\":1}\"}}\n",
        "{\"event_type\":\"step.stop\",\"index\":0}\n",
        "{\"event_type\":\"interaction.completed\",\"interaction\":{\"id\":\"i1\",\"status\":\"requires_action\"}}\n",
        "{\"event_type\":\"step.delta\",\"index\":7}\n",
    );
    stream_event(body.as_bytes());
    stream_event(b"{\"event_type\":\"step.delta\",\"index\":3,\"delta\":{\"type\":\"image\",\"data\":\"AA\"}}\n");
    stream_event(b"not sse at all");
}

// =============================================================================
// Inputs a fuzz run failed on
// =============================================================================

/// `stream_event`, 2026-11-04: a wire event named `created` (not
/// `interaction.created`) is an unknown chunk, but serialized it read back
/// as `StreamChunk::Created`. Unknown chunks named like a known
/// `chunk_type` now carry `"unknown": true`.
#[test]
fn regression_unknown_event_named_like_a_known_chunk_type() {
    stream_event(b"l\n{\"event_type\":\"created\"}\n{\0\0\0\0");
    for name in ["status_update", "step_start", "step_delta", "step_stop", "completed"] {
        stream_event(format!("{{\"event_type\":\"{name}\",\"x\":1}}\n").as_bytes());
    }
}

#[test]
fn deserializers_round_trip_known_shapes() {
    step(br#"{"type":"model_output","content":[{"type":"text","text":"hi"}]}"#);
    step(br#"{"type":"function_call","id":"c1","name":"f","arguments":{"x":[1,2.5,null]}}"#);
    step(br#"{"type":"brand_new_step","payload":{"k":1}}"#);
    step_delta(br#"{"type":"text","text":"hi"}"#);
    step_delta(br#"{"type":"arguments_delta","arguments":"{\"a\""}"#);
    step_delta(br#"{"type":"brand_new_delta"}"#);
    content(br#"{"type":"text","text":"hi","annotations":[]}"#);
    content(br#"{"type":"image","data":"AA","mime_type":"image/png"}"#);
    content(br#"{"type":"brand_new_content","x":true}"#);
    // Not even JSON: rejected without a panic.
    step(b"\xff");
    content(b"[");
}
