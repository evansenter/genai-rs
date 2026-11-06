use super::*;

use serde_json::json;

// =========================================================================
// StepAccumulator
// =========================================================================

#[test]
fn test_accumulator_text_stream() {
    let mut acc = StepAccumulator::new();
    acc.start(0, Step::model_output(vec![]));
    acc.apply_delta(
        0,
        &StepDelta::Text {
            text: "Hello ".into(),
            extra: Default::default(),
        },
    );
    acc.apply_delta(
        0,
        &StepDelta::Text {
            text: "world".into(),
            extra: Default::default(),
        },
    );
    acc.stop(0);
    let steps = acc.finish();
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].as_text(), Some("Hello world"));
}

/// The image data each delta carried, in block order.
fn image_blocks(step: &Step) -> Vec<&str> {
    step.content()
        .expect("a content step")
        .iter()
        .map(|block| match block {
            Content::Image { data, .. } => data.as_deref().unwrap_or_default(),
            other => panic!("Expected Image, got {other:?}"),
        })
        .collect()
}

/// Each streamed image arrives whole in one delta that names its mime_type
/// (live 2026-11-04, `gemini-3.1-flash-image`), so two of them in one step
/// are two blocks. Live, each image came in its own step; this same-step
/// shape is defensive. Merging by kind would make one corrupt base64 string
/// whenever `interaction.completed` omits `steps` (#491).
#[test]
fn test_accumulator_streamed_images_stay_separate_blocks() {
    let mut acc = StepAccumulator::new();
    acc.start(0, Step::model_output(vec![]));
    for data in ["/9j/AAAA", "/9j/BBBB"] {
        acc.apply_delta(
            0,
            &serde_json::from_value(
                json!({"type": "image", "mime_type": "image/jpeg", "data": data}),
            )
            .unwrap(),
        );
    }
    acc.stop(0);
    let steps = acc.finish();
    assert_eq!(image_blocks(&steps[0]), ["/9j/AAAA", "/9j/BBBB"]);
}

/// A delta carrying only `data` (no mime_type, no uri) has never been seen,
/// but would be a continuation of the image before it.
#[test]
fn test_accumulator_bare_image_fragment_continues_the_last_image() {
    let mut acc = StepAccumulator::new();
    acc.start(0, Step::model_output(vec![]));
    acc.apply_delta(
        0,
        &serde_json::from_value(json!({"type": "image", "mime_type": "image/png", "data": "AAAA"}))
            .unwrap(),
    );
    acc.apply_delta(
        0,
        &serde_json::from_value(json!({"type": "image", "data": "BBBB"})).unwrap(),
    );
    let steps = acc.finish();
    assert_eq!(image_blocks(&steps[0]), ["AAAABBBB"]);
}

/// A delta's unmodeled fields land on what it builds: the content block for
/// text and media, the step for a tool delta. The accumulated steps then
/// round-trip them like the steps of a non-streamed response (#491).
#[test]
fn test_accumulator_keeps_unmodeled_delta_fields() {
    let mut acc = StepAccumulator::new();
    acc.start(0, Step::model_output(vec![]));
    for delta in [
        json!({"type": "text", "text": "a", "lang": "en"}),
        json!({"type": "text", "text": "b"}),
        json!({"type": "image", "mime_type": "image/png", "data": "AA", "seed": 7}),
    ] {
        acc.apply_delta(0, &serde_json::from_value(delta).unwrap());
    }
    acc.start(
        1,
        serde_json::from_value(json!({
            "type": "url_context_call", "id": "c1",
            "arguments": {"urls": []}, "start_only": true
        }))
        .unwrap(),
    );
    acc.apply_delta(
        1,
        &serde_json::from_value(json!({
            "type": "url_context_call",
            "arguments": {"urls": ["https://example.com"], "max_pages": 3},
            "trace": "t1"
        }))
        .unwrap(),
    );
    let steps = acc.finish();
    assert_eq!(
        serde_json::to_value(&steps).unwrap(),
        json!([
            {"type": "model_output", "content": [
                {"type": "text", "text": "ab", "lang": "en"},
                {"type": "image", "mime_type": "image/png", "data": "AA", "seed": 7}
            ]},
            {"type": "url_context_call", "id": "c1",
             "arguments": {"urls": ["https://example.com"], "max_pages": 3},
             "start_only": true, "trace": "t1"}
        ])
    );
}

/// A streamed code execution call whose language the crate does not know
/// keeps its code, as the same call read as a step does. By default the
/// language reads as `CodeExecutionLanguage::Unknown`. Under
/// `genai_strict_unknown` the delta fails to parse, naming the field, like the
/// step; it used to become a `StepDelta::Unknown`, which the accumulator
/// left out of the step, so the code was lost without an error.
#[test]
fn test_accumulator_code_execution_delta_with_an_unknown_language() {
    let delta = json!({
        "type": "code_execution_call",
        "arguments": {"language": "cobol", "code": "DISPLAY 1"}
    });

    #[cfg(genai_strict_unknown)]
    {
        let err = serde_json::from_value::<StepDelta>(delta).unwrap_err();
        assert_eq!(
            err.to_string(),
            "StepDelta 'code_execution_call' has a field this crate cannot read: \
             field `arguments.language`: Unknown CodeExecutionLanguage 'cobol'. \
             Strict mode is enabled via the 'genai_strict_unknown' cfg flag. \
             Either update the library or disable strict mode."
        );
        let step = json!({
            "type": "code_execution_call", "id": "c1",
            "arguments": {"language": "cobol", "code": "DISPLAY 1"}
        });
        let step_err = serde_json::from_value::<Step>(step).unwrap_err();
        assert_eq!(
            step_err.to_string().replacen("Step ", "StepDelta ", 1),
            err.to_string(),
            "the step and the delta fail alike"
        );
    }

    #[cfg(not(genai_strict_unknown))]
    {
        let mut acc = StepAccumulator::new();
        acc.start(
            0,
            serde_json::from_value(json!({
                "type": "code_execution_call", "id": "c1", "arguments": {}
            }))
            .unwrap(),
        );
        acc.apply_delta(0, &serde_json::from_value(delta).unwrap());
        let steps = acc.finish();
        assert_eq!(
            serde_json::to_value(&steps).unwrap(),
            json!([{
                "type": "code_execution_call", "id": "c1",
                "arguments": {"language": "cobol", "code": "DISPLAY 1"}
            }])
        );
    }
}

#[test]
fn test_accumulator_image_uri_delta_pushes_new_block() {
    // Deltas without inline data (e.g. URI references) must not merge
    // into the previous block.
    let mut acc = StepAccumulator::new();
    acc.start(0, Step::model_output(vec![]));
    acc.apply_delta(
        0,
        &StepDelta::Image {
            data: Some("AAAA".into()),
            uri: None,
            mime_type: Some("image/png".into()),
            resolution: None,
            extra: Default::default(),
        },
    );
    acc.apply_delta(
        0,
        &StepDelta::Image {
            data: None,
            uri: Some("https://example.com/img.png".into()),
            mime_type: Some("image/png".into()),
            resolution: None,
            extra: Default::default(),
        },
    );
    acc.stop(0);
    let steps = acc.finish();
    match &steps[0] {
        Step::ModelOutput { content, .. } => assert_eq!(content.len(), 2),
        other => panic!("Expected ModelOutput, got {other:?}"),
    }
}

#[test]
fn test_accumulator_function_call_arguments_delta() {
    let mut acc = StepAccumulator::new();
    acc.start(
        0,
        Step::FunctionCall {
            id: "c1".into(),
            name: "get_weather".into(),
            arguments: serde_json::Value::Null,
            signature: None,
            extra: Default::default(),
        },
    );
    acc.apply_delta(
        0,
        &StepDelta::ArgumentsDelta {
            arguments: "{\"city\": ".into(),
            extra: Default::default(),
        },
    );
    acc.apply_delta(
        0,
        &StepDelta::ArgumentsDelta {
            arguments: "\"Tokyo\"}".into(),
            extra: Default::default(),
        },
    );
    acc.stop(0);
    let steps = acc.finish();
    match &steps[0] {
        Step::FunctionCall { arguments, .. } => assert_eq!(arguments["city"], "Tokyo"),
        other => panic!("Expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn test_accumulator_thought_summary_and_signature() {
    let mut acc = StepAccumulator::new();
    acc.start(
        0,
        Step::Thought {
            signature: None,
            summary: vec![],
            extra: Default::default(),
        },
    );
    acc.apply_delta(
        0,
        &StepDelta::ThoughtSummary {
            content: Some(Box::new(Content::text("Consider "))),
            extra: Default::default(),
        },
    );
    acc.apply_delta(
        0,
        &StepDelta::ThoughtSummary {
            content: Some(Box::new(Content::text("the problem"))),
            extra: Default::default(),
        },
    );
    acc.apply_delta(
        0,
        &StepDelta::ThoughtSignature {
            signature: Some("sig-xyz".into()),
            extra: Default::default(),
        },
    );
    let steps = acc.finish();
    match &steps[0] {
        Step::Thought {
            signature, summary, ..
        } => {
            assert_eq!(signature.as_deref(), Some("sig-xyz"));
            assert_eq!(summary[0].as_text(), Some("Consider the problem"));
        }
        other => panic!("Expected Thought, got {other:?}"),
    }
}

/// Merging consecutive summary text chunks keeps every chunk's unmodeled
/// content fields on the merged block, as the text and media merges do, so
/// the thought replays like a non-streamed one (#491).
#[test]
fn test_accumulator_thought_summary_merge_keeps_each_chunks_extra() {
    let mut acc = StepAccumulator::new();
    acc.start(
        0,
        serde_json::from_value(json!({"type": "thought", "signature": "sig"})).unwrap(),
    );
    for delta in [
        json!({"type": "thought_summary", "content": {"type": "text", "text": "Consider ", "first": 1}}),
        json!({"type": "thought_summary", "content": {"type": "text", "text": "this", "later": 2}}),
    ] {
        acc.apply_delta(0, &serde_json::from_value(delta).unwrap());
    }
    let steps = acc.finish();
    assert_eq!(
        serde_json::to_value(&steps[0]).unwrap(),
        json!({"type": "thought", "signature": "sig", "summary": [
            {"type": "text", "text": "Consider this", "first": 1, "later": 2}
        ]})
    );
}

#[test]
fn test_accumulator_delta_without_start_creates_model_output() {
    let mut acc = StepAccumulator::new();
    acc.apply_delta(
        0,
        &StepDelta::Text {
            text: "orphan".into(),
            extra: Default::default(),
        },
    );
    let steps = acc.finish();
    assert_eq!(steps[0].as_text(), Some("orphan"));
}

#[test]
fn test_accumulator_orders_steps_by_index() {
    let mut acc = StepAccumulator::new();
    acc.start(2, Step::model_text("second"));
    acc.start(1, Step::thought("sig"));
    let steps = acc.finish();
    assert!(matches!(steps[0], Step::Thought { .. }));
    assert!(matches!(steps[1], Step::ModelOutput { .. }));
}

// =========================================================================
// processing_* / retrieval_* steps (google-genai 2.21+ / 2.24+ bindings)
// =========================================================================

/// The streamed `processing_call` announces `signature: ""` on
/// `step.start` and sends the value in `step.delta` (live 2026-09-24).
/// Dropping it made stateless replay fail with
/// `400 Processing call step is missing signature`.
#[test]
fn accumulator_moves_streamed_processing_signature_onto_the_step() {
    let mut acc = StepAccumulator::new();
    acc.start(
        0,
        serde_json::from_value(json!({"type": "processing_call", "id": "c1", "signature": ""}))
            .unwrap(),
    );
    acc.apply_delta(
        0,
        &serde_json::from_value(json!({"type": "processing_call", "signature": "SIG"})).unwrap(),
    );
    acc.stop(0);
    acc.start(
        1,
        serde_json::from_value(
            json!({"type": "processing_result", "call_id": "c1", "signature": ""}),
        )
        .unwrap(),
    );
    acc.apply_delta(
        1,
        &serde_json::from_value(json!({"type": "processing_result", "signature": "RES"})).unwrap(),
    );
    let steps = acc.finish();
    assert_eq!(steps[0].signature(), Some("SIG"));
    assert_eq!(steps[1].signature(), Some("RES"));
}

#[test]
fn merge_signature_replaces_empty_and_extends_fragments() {
    let mut sig = Some(String::new());
    merge_signature(&mut sig, Some("ab"));
    assert_eq!(sig.as_deref(), Some("ab"));
    merge_signature(&mut sig, Some("cd"));
    assert_eq!(sig.as_deref(), Some("abcd"));
    merge_signature(&mut sig, Some(""));
    merge_signature(&mut sig, None);
    assert_eq!(sig.as_deref(), Some("abcd"));
    let mut none = None;
    merge_signature(&mut none, Some("x"));
    assert_eq!(none.as_deref(), Some("x"));
}

/// A step type the crate does not model yet must not lose a streamed
/// signature either — the same failure, one API release later.
#[test]
fn accumulator_merges_unknown_delta_signature_into_unknown_step() {
    let mut acc = StepAccumulator::new();
    // Built directly: `genai_strict_unknown` rejects unknown steps on deserialize.
    acc.start(
        0,
        Step::Unknown {
            step_type: "future_call".into(),
            data: json!({"type": "future_call", "id": "f1", "signature": ""}),
        },
    );
    acc.apply_delta(
        0,
        &serde_json::from_value(json!({"type": "future_call", "signature": "FUT"})).unwrap(),
    );
    // A differently-typed unknown delta is not merged.
    acc.apply_delta(
        0,
        &serde_json::from_value(json!({"type": "other_delta", "signature": "NO"})).unwrap(),
    );
    let steps = acc.finish();
    assert_eq!(
        steps[0].unknown_data().unwrap(),
        &json!({"type": "future_call", "id": "f1", "signature": "FUT"})
    );
    assert_eq!(
        serde_json::to_value(&steps[0]).unwrap(),
        json!({"type": "future_call", "id": "f1", "signature": "FUT"})
    );
}

/// `FunctionResultDelta.call_id` is gone from the 2.25 bindings.
#[test]
fn function_result_delta_without_call_id_stays_typed() {
    let delta: StepDelta =
        serde_json::from_value(json!({"type": "function_result", "result": "ok"})).unwrap();
    assert!(matches!(
        delta,
        StepDelta::FunctionResult { call_id: None, .. }
    ));

    let mut acc = StepAccumulator::new();
    acc.start(
        0,
        serde_json::from_value(
            json!({"type": "function_result", "call_id": "c9", "signature": "s"}),
        )
        .unwrap(),
    );
    acc.apply_delta(0, &delta);
    match &acc.finish()[0] {
        Step::FunctionResult {
            call_id, signature, ..
        } => {
            assert_eq!(call_id, "c9");
            assert_eq!(signature.as_deref(), Some("s"));
        }
        other => panic!("expected FunctionResult, got {other:?}"),
    }
}

/// An empty signature in a later delta must not wipe the real one an
/// earlier event carried: replay would then fail with `400 ... missing
/// signature`.
#[test]
fn test_accumulator_empty_delta_signature_keeps_the_real_one() {
    let mut acc = StepAccumulator::new();
    acc.start(
        0,
        serde_json::from_value(json!({
            "type": "google_search_call",
            "id": "c1",
            "arguments": {"queries": []},
            "signature": "real-sig"
        }))
        .unwrap(),
    );
    acc.apply_delta(
        0,
        &StepDelta::GoogleSearchCall {
            queries: vec!["rust".into()],
            signature: Some(String::new()),
            arguments_extra: Default::default(),
            extra: Default::default(),
        },
    );
    acc.stop(0);
    let steps = acc.finish();
    assert_eq!(steps[0].signature(), Some("real-sig"));
}

#[test]
fn test_accumulator_keeps_file_search_delta_results_in_the_step_extra() {
    // The step has no `result` field; the spec gives the delta one.
    let mut acc = StepAccumulator::new();
    acc.start(
        0,
        serde_json::from_value(json!({"type": "file_search_result", "call_id": "c1"})).unwrap(),
    );
    for (i, item) in [json!({"text": "a"}), json!({"text": "b"})]
        .into_iter()
        .enumerate()
    {
        acc.apply_delta(
            0,
            &StepDelta::FileSearchResult {
                result: vec![item],
                signature: (i == 1).then(|| "c2ln".to_string()),
                extra: Default::default(),
            },
        );
    }
    acc.stop(0);
    let steps = acc.finish();
    assert_eq!(
        serde_json::to_value(&steps[0]).unwrap(),
        json!({
            "type": "file_search_result",
            "call_id": "c1",
            "signature": "c2ln",
            "result": [{"text": "a"}, {"text": "b"}]
        })
    );
}

#[test]
fn test_accumulator_file_search_delta_without_results_adds_nothing() {
    // What a live stream sends (2026-11-05): only the signature.
    let mut acc = StepAccumulator::new();
    acc.start(
        0,
        serde_json::from_value(json!({"type": "file_search_result", "call_id": "c1"})).unwrap(),
    );
    let delta: StepDelta =
        serde_json::from_value(json!({"type": "file_search_result", "signature": "c2ln"})).unwrap();
    assert_eq!(
        serde_json::to_value(&delta).unwrap(),
        json!({"type": "file_search_result", "signature": "c2ln"})
    );
    acc.apply_delta(0, &delta);
    acc.stop(0);
    assert_eq!(
        serde_json::to_value(&acc.finish()[0]).unwrap(),
        json!({"type": "file_search_result", "call_id": "c1", "signature": "c2ln"})
    );
}

/// Feeds `deltas` (wire JSON) into one `model_output` step and returns its
/// content blocks, serialized.
fn stream_blocks(deltas: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let mut acc = StepAccumulator::new();
    acc.start(0, Step::model_output(vec![]));
    for delta in deltas {
        acc.apply_delta(0, &serde_json::from_value(delta.clone()).unwrap());
    }
    acc.stop(0);
    let steps = acc.finish();
    steps[0]
        .content()
        .unwrap()
        .iter()
        .map(|c| serde_json::to_value(c).unwrap())
        .collect()
}

#[test]
fn test_accumulator_streamed_tts_audio_is_one_block() {
    // The live shape (2026-11-05, gemini-3.8-flash-tts): every chunk names
    // the same format.
    let chunk = |data: &str| {
        json!({"type": "audio", "mime_type": "audio/l16", "data": data,
               "channels": 1, "sample_rate": 24000})
    };
    let blocks = stream_blocks(&[chunk("AAAA"), chunk("BBBB"), chunk("CCCC")]);
    assert_eq!(
        blocks,
        [
            json!({"type": "audio", "mime_type": "audio/l16", "data": "AAAABBBBCCCC",
                "channels": 1, "sample_rate": 24000})
        ]
    );
}

#[test]
fn test_accumulator_audio_in_another_format_starts_a_block() {
    let blocks = stream_blocks(&[
        json!({"type": "audio", "mime_type": "audio/l16", "data": "AAAA"}),
        json!({"type": "audio", "data": "BBBB"}),
        json!({"type": "audio", "mime_type": "audio/wav", "data": "CCCC"}),
        json!({"type": "audio", "mime_type": "audio/wav", "data": "DDDD", "sample_rate": 16000}),
    ]);
    let data: Vec<&str> = blocks.iter().map(|b| b["data"].as_str().unwrap()).collect();
    // A delta naming no format continues; a new mime type or rate does not.
    assert_eq!(data, ["AAAABBBB", "CCCC", "DDDD"]);
}

#[test]
fn test_accumulator_whole_videos_and_documents_stay_separate() {
    let blocks = stream_blocks(&[
        json!({"type": "video", "mime_type": "video/mp4", "data": "AAAA"}),
        json!({"type": "video", "data": "BBBB"}),
        json!({"type": "video", "mime_type": "video/mp4", "data": "CCCC"}),
        json!({"type": "document", "mime_type": "application/pdf", "data": "DDDD"}),
        json!({"type": "document", "mime_type": "application/pdf", "data": "EEEE"}),
    ]);
    let data: Vec<&str> = blocks.iter().map(|b| b["data"].as_str().unwrap()).collect();
    assert_eq!(data, ["AAAABBBB", "CCCC", "DDDD", "EEEE"]);
}

#[test]
fn test_accumulator_joins_padded_base64_as_bytes() {
    use base64::Engine;
    let engine = base64::engine::general_purpose::STANDARD;
    // "a" and "bc" encode with padding; their text concatenation is invalid.
    let blocks = stream_blocks(&[
        json!({"type": "audio", "mime_type": "audio/l16", "data": engine.encode("a")}),
        json!({"type": "audio", "mime_type": "audio/l16", "data": engine.encode("bc")}),
    ]);
    let joined = engine.decode(blocks[0]["data"].as_str().unwrap()).unwrap();
    assert_eq!(joined, b"abc");
}

#[test]
fn test_accumulator_joins_unpadded_partial_base64_as_bytes() {
    use base64::Engine;
    let unpadded = base64::engine::general_purpose::STANDARD_NO_PAD;
    let standard = base64::engine::general_purpose::STANDARD;
    // "a" and "bc" encode unpadded to 2 and 3 characters, not whole quanta:
    // as text, "YQ" + "YmM" + "ZGVm" would decode to garbage.
    let chunks = [
        unpadded.encode("a"),
        unpadded.encode("bc"),
        unpadded.encode("def"),
    ];
    assert_eq!(chunks, ["YQ", "YmM", "ZGVm"]);
    let blocks = stream_blocks(&[
        json!({"type": "audio", "mime_type": "audio/l16", "data": chunks[0]}),
        json!({"type": "audio", "data": chunks[1]}),
        json!({"type": "audio", "data": chunks[2]}),
    ]);
    assert_eq!(blocks.len(), 1);
    let joined = blocks[0]["data"].as_str().unwrap();
    assert_eq!(standard.decode(joined).unwrap(), b"abcdef");

    // A partial chunk after whole quanta re-encodes only the end: the whole
    // quanta before it are kept as they came.
    let blocks = stream_blocks(&[
        json!({"type": "audio", "mime_type": "audio/l16", "data": "ZGVm"}),
        json!({"type": "audio", "data": "YQ"}),
        json!({"type": "audio", "data": "YmM"}),
    ]);
    let joined = blocks[0]["data"].as_str().unwrap();
    assert!(joined.starts_with("ZGVm"), "{joined}");
    assert_eq!(standard.decode(joined).unwrap(), b"defabc");
}

#[test]
fn test_accumulator_concatenates_undecodable_base64_as_text() {
    // Not base64 at all: kept as text rather than lost.
    let blocks = stream_blocks(&[
        json!({"type": "audio", "mime_type": "audio/l16", "data": "not base64!"}),
        json!({"type": "audio", "data": "??"}),
    ]);
    assert_eq!(blocks[0]["data"], "not base64!??");
}
