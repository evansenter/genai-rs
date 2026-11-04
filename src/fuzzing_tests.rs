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
    for name in [
        "status_update",
        "step_start",
        "step_delta",
        "step_stop",
        "completed",
    ] {
        stream_event(format!("{{\"event_type\":\"{name}\",\"x\":1}}\n").as_bytes());
    }
}

/// `step`, `step_delta` and `content`, 2026-11-04, once the check compared
/// the first read with the second: an input with no string `type` (here a
/// bare number) reads as `Unknown` tagged `"<missing type>"`, and the write
/// puts that tag in its data. Intended (pinned in `tools_tests.rs`,
/// `annotation_tests.rs` and `serialize_tests.rs`), so [`Normalize`]
/// expects it, at the top level and nested. (Under `strict-unknown` these
/// inputs don't parse, and the targets skip them.)
#[cfg(not(feature = "strict-unknown"))]
#[test]
fn regression_typeless_input_is_tagged_missing_type() {
    step(b"0");
    step_delta(b"1");
    content(b"0");
    for json in [r#"0"#, r#"{"x":1}"#, r#"{"type":5,"x":1}"#, "null"] {
        normalized_round_trip::<Step>(json);
        normalized_round_trip::<StepDelta>(json);
        normalized_round_trip::<Content>(json);
    }
    normalized_round_trip::<Step>(r#"{"type":"model_output","content":[{"text":"no type"}]}"#);
    normalized_round_trip::<Content>(
        r#"{"type":"text","text":"a","annotations":[{"start_index":1}]}"#,
    );
    normalized_round_trip::<StepDelta>(
        r#"{"type":"text_annotation_delta","annotations":[{"end_index":2}]}"#,
    );
    // A real unknown type is already in its data: no change at all.
    let real = r#"{"type":"brand_new","x":[1]}"#;
    let first: Step = serde_json::from_str(real).unwrap();
    let mut normalized = first.clone();
    normalized.normalize();
    assert_eq!(first, normalized);
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

/// Asserts the first write of `json` does change it (so the input
/// exercises [`Normalize`]), then runs the full check.
fn normalized_round_trip<T>(json: &str)
where
    T: DeserializeOwned + Serialize + PartialEq + std::fmt::Debug + Normalize,
{
    let first: T = serde_json::from_str(json).unwrap();
    let second: T = serde_json::from_value(serde_json::to_value(&first).unwrap()).unwrap();
    assert_ne!(first, second, "{json} needs no normalization");
    round_trip::<T>(json.as_bytes());
}

#[test]
fn round_trip_allows_only_the_empty_annotation_normalization() {
    // An empty annotation list is not written, wherever the text sits.
    let text = r#"{"type":"text","text":"hi","annotations":[]}"#;
    normalized_round_trip::<Content>(text);
    for step_json in [
        format!(r#"{{"type":"model_output","content":[{text}]}}"#),
        format!(r#"{{"type":"user_input","content":[{text}]}}"#),
        format!(r#"{{"type":"thought","signature":"s","summary":[{text}]}}"#),
        format!(r#"{{"type":"function_result","call_id":"c","result":[{text}]}}"#),
        format!(r#"{{"type":"mcp_server_tool_result","call_id":"c","result":[{text}]}}"#),
    ] {
        normalized_round_trip::<Step>(&step_json);
    }
    for delta_json in [
        format!(r#"{{"type":"thought_summary","content":{text}}}"#),
        format!(r#"{{"type":"function_result","call_id":"c","result":[{text}]}}"#),
        format!(r#"{{"type":"mcp_server_tool_result","result":[{text}]}}"#),
    ] {
        normalized_round_trip::<StepDelta>(&delta_json);
    }
}

/// A type whose serializer drops a field: the bug the round-trip check is
/// for. It passes a fixed-point check (both later reads lack the field) and
/// must fail this one.
#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
struct DropsAField {
    kept: u8,
    #[serde(skip_serializing)]
    dropped: Option<u8>,
}

impl Normalize for DropsAField {
    fn normalize(&mut self) {}
}

#[test]
#[should_panic(expected = "a field was lost or changed on write")]
fn round_trip_catches_a_field_dropped_on_write() {
    round_trip::<DropsAField>(br#"{"kept":1,"dropped":2}"#);
}
