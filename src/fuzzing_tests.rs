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
/// expects it, at the top level and nested. (Under `genai_strict_unknown` these
/// inputs don't parse, and the targets skip them.)
#[cfg(not(genai_strict_unknown))]
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
    // 2026-11-13, when the property tests first compared a step's first
    // write: a code execution result's content blocks, which `Normalize`
    // did not look inside.
    normalized_round_trip::<Step>(
        r#"{"type":"code_execution_result","call_id":"a","result":[{"x":1}]}"#,
    );
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

/// `content`, 2026-11-05 (PR #521's smoke run, whose branch predated the
/// fix in 31ee193): a non-string `resolution` read into
/// `Resolution::Unknown` with a `<non-string: 333333>` type and was written
/// back as that descriptor string, so it read back with string data. A
/// non-string unknown `wire_enum!` value now writes the original JSON.
#[test]
fn regression_non_string_enum_value_round_trips() {
    // The input exactly as the run saved it.
    content(br#"{"type": "image", "datat": "image/png", "resolution": 333333}"#);
    // The other non-string shapes, at the same field.
    for value in ["0", "true", "null", "[1]", r#"{"x":1}"#] {
        content(format!(r#"{{"type":"image","resolution":{value}}}"#).as_bytes());
    }
}

/// `step`, 2026-11-10 (PR #548's fuzz run): a code execution call's
/// non-string `language` read into `CodeExecutionLanguage::Unknown` and was
/// written back as its `<non-string: 55555555>` descriptor, because the
/// step's nested `arguments` were built from the language's `Display`
/// rather than its `Serialize`. The delta writes the same arguments.
#[test]
fn regression_non_string_code_language_round_trips() {
    // The input exactly as the run saved it.
    step(br#"{"type": "code_execution_call", "id": "x1", "arguments": {"code": "1)", "language": 55555555}}"#);
    for value in ["0", "true", "null", "[1]", r#"{"x":1}"#] {
        step(
            format!(
                r#"{{"type":"code_execution_call","id":"x1","arguments":{{"code":"c","language":{value}}}}}"#
            )
            .as_bytes(),
        );
        step_delta(
            format!(r#"{{"type":"code_execution_call","arguments":{{"language":{value}}}}}"#)
                .as_bytes(),
        );
    }
}

/// Every committed seed in `fuzz/seeds/<target>/`, through its target's
/// entry point. The fuzz workflow runs only on schedule and on pull
/// requests that touch what the targets build; this runs in the required
/// Test Suite, so a change that breaks a seed fails here first. A seed
/// directory with no entry point here, or a target with no seeds, fails
/// too.
#[test]
fn committed_seeds_replay_through_their_targets() {
    /// A fuzz target's entry point.
    type EntryPoint = fn(&[u8]);
    let targets: Vec<(&str, EntryPoint)> = vec![
        ("content", content),
        // Its entry point is behind the feature its target enables.
        #[cfg(feature = "antigravity")]
        ("harness_output_event", harness_output_event),
        ("interaction_response", interaction_response),
        ("resource", resource),
        ("sse_parser", sse_parser),
        ("step", step),
        ("step_delta", step_delta),
        ("stream_event", stream_event),
    ];
    let seeds = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fuzz/seeds");
    let dirs: Vec<String> = listing(&seeds)
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        // Without the feature its target needs, it has no entry point here.
        .filter(|dir| cfg!(feature = "antigravity") || dir != "harness_output_event")
        .collect();
    let names: Vec<&str> = targets.iter().map(|(name, _)| *name).collect();
    assert_eq!(
        dirs, names,
        "fuzz/seeds/ must hold one directory per entry point listed here"
    );

    for (name, run) in targets {
        let files = listing(&seeds.join(name));
        assert!(!files.is_empty(), "fuzz/seeds/{name}/ has no seeds");
        for file in files {
            let input = std::fs::read(&file)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()));
            // The entry point's own panic is printed above this one.
            if std::panic::catch_unwind(|| run(&input)).is_err() {
                panic!("the {name} target failed on the seed {}", file.display());
            }
        }
    }
}

/// The entries of `dir`, sorted, but for hidden ones such as the
/// `.DS_Store` macOS's Finder leaves: git ignores them, so they are no seed.
fn listing(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|entry| entry.expect("dir entry").path())
        .filter(|path| !path.file_name().unwrap().to_string_lossy().starts_with('.'))
        .collect();
    paths.sort();
    paths
}

#[test]
fn seed_listing_skips_hidden_files() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["b", ".DS_Store", "a"] {
        std::fs::write(dir.path().join(name), b"{}").unwrap();
    }
    std::fs::create_dir(dir.path().join("content")).unwrap();
    let names: Vec<_> = listing(dir.path())
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["a", "b", "content"]);
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
    T: DeserializeOwned + Serialize + PartialEq + std::fmt::Debug + Normalize + WireForm,
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

impl WireForm for DropsAField {}

#[test]
#[should_panic(expected = "a field was lost or changed on write")]
fn round_trip_catches_a_field_dropped_on_write() {
    round_trip::<DropsAField>(br#"{"kept":1,"dropped":2}"#);
}

// =============================================================================
// The wire-read targets and the faithfulness check (D-NEXT-J7)
// =============================================================================

/// A type whose deserializer rewrites what it read (an unknown discriminant
/// read as a known one), and whose round trip is then stable: what
/// `EnvironmentSpec` did to a non-string `type`. Only the comparison with
/// the input catches it.
#[derive(Debug, PartialEq, Serialize)]
struct RewritesOnRead {
    #[serde(rename = "type")]
    kind: String,
}

impl<'de> serde::Deserialize<'de> for RewritesOnRead {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let kind = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("remote");
        Ok(Self { kind: kind.into() })
    }
}

impl Normalize for RewritesOnRead {
    fn normalize(&mut self) {}
}

impl WireForm for RewritesOnRead {
    fn faithful_wire(&self, input: &serde_json::Value) -> Option<serde_json::Value> {
        Some(input.clone())
    }
}

#[test]
#[should_panic(expected = "the written JSON is not what was read")]
fn round_trip_catches_a_value_rewritten_on_read() {
    round_trip::<RewritesOnRead>(br#"{"type":5}"#);
}

#[test]
fn round_trip_accepts_a_faithful_write() {
    round_trip::<RewritesOnRead>(br#"{"type":"remote"}"#);
}

#[test]
fn canonical_form_merges_only_what_the_crate_treats_as_one() {
    use serde_json::json;
    let same = |a: serde_json::Value, b: serde_json::Value, int64_strings: bool| {
        wire::canonical(&a, int64_strings) == wire::canonical(&b, int64_strings)
    };
    // Null, empty and absent members.
    assert!(same(
        json!({"a": null, "b": [], "c": {"d": null}, "e": 1}),
        json!({"e": 1}),
        false
    ));
    // A whole float and its integer; the f32 fields at f32 precision, and
    // beyond its range absent.
    assert!(same(json!({"x": 1.0}), json!({"x": 1}), false));
    assert!(same(
        json!({"temperature": 0.1}),
        json!({"temperature": f64::from(0.1_f32)}),
        false
    ));
    assert!(same(json!({"alpha": 1e39, "k": 1}), json!({"k": 1}), false));
    // proto3 JSON's int64 strings and default scalars, only where asked for.
    assert!(same(json!({"seqNum": "5"}), json!({"seqNum": 5}), true));
    assert!(!same(json!({"seqNum": "5"}), json!({"seqNum": 5}), false));
    assert!(same(
        json!({"id": "", "n": 0, "b": false, "k": "0"}),
        json!({}),
        true
    ));
    assert!(!same(json!({"id": ""}), json!({}), false));
    assert!(!same(json!({"n": [0, false, ""]}), json!({"n": []}), true));
    // Not the same: a changed value, an added key, a null in a list, an empty
    // string, and a number beside a string elsewhere.
    assert!(!same(json!({"type": 5}), json!({"type": "remote"}), false));
    assert!(!same(
        json!({"mode": "disabled"}),
        json!({"mode": "disabled", "domain": ""}),
        false
    ));
    assert!(!same(json!([null]), json!([]), false));
    assert!(!same(json!({"x": 0.5}), json!({"x": 0.25}), false));
    assert!(!same(json!({"text": "5"}), json!({"text": 5}), false));
}

/// The round-trip findings of 2026-11-10, each as the input that showed it,
/// through the target that reaches it.
#[test]
fn regression_round_trip_findings() {
    // A non-string environment `type` read as "remote" (D-001).
    for spec in [
        r#"{"type":5,"environment_id":"e"}"#,
        r#"{"type":["remote"]}"#,
    ] {
        resource(spec.as_bytes());
        resource(format!(r#"{{"id":"a","base_environment":{spec}}}"#).as_bytes());
    }
    // A domain-less allowlist entry written back with `"domain":""`.
    resource(br#"{"type":"remote","network":{"allowlist":[{"mode":"disabled","zz":1}]}}"#);
    resource(br#"{"type":"remote","network":{"allowlist":[{"mode":"disabled"}],"zz":1}}"#);
    // The forms an environment normalizes, which the check expects.
    resource(br#"{"environment_id":"e","env":[{"A":{"value":"x"}},{"B":{"credential":"c"}}]}"#);
    resource(br#"{"type":null,"network":{"allowlist":[{"mode":"disabled"}]}}"#);
    resource(br#"{"tools":[{"x":1},{"type":"brand_new"}],"base_environment":"env-1"}"#);
    // The speakers form's keys, and an f32 beyond its range.
    interaction_response(
        br#"{"status":"completed","generation_config":{"speech_config":{"speakers":[{"voice":"Kore"}],"x":1},"temperature":1e39}}"#,
    );
    // A single input object.
    interaction_response(br#"{"input":{"type":"text","text":"hi"}}"#);
}

/// The harness findings of 2026-11-10: a non-object `usageUpdate` dropped,
/// and a non-bool `sessionEndResponse` read as `false`.
#[cfg(feature = "antigravity")]
#[test]
fn regression_harness_event_round_trips() {
    for input in [
        r#"{"seqNum":1,"usageUpdate":[1,2]}"#,
        r#"{"seqNum":"1","usageUpdate":7,"stepUpdate":{"trajectoryId":"t"}}"#,
        r#"{"sessionEndResponse":{"hooksRun":2}}"#,
        r#"{"sessionEndResponse":"true"}"#,
        r#"{"usageMetadata":{"totalTokenCount":1},"usageUpdate":{"total":{"totalTokenCount":2}}}"#,
    ] {
        harness_output_event(input.as_bytes());
    }
}

/// `harness_output_event`, 2026-11-13 (its first run): a `toolResponse`
/// with no `id` was written back with `"id": ""`. The same message in
/// proto3, which has no presence for a scalar at its default, so the
/// faithfulness check reads the harness's JSON as proto3 does.
#[cfg(feature = "antigravity")]
#[test]
fn regression_harness_default_scalar_written_back() {
    // The input exactly as the run saved it.
    harness_output_event(
        br#"{"stepUpdate":{"cascadeId":"c","customTool":{"toolCall":{"arguments":{"fields":[{"name":"city","value":{"stringValue":"Zurich"}}]},"raugmentsJson":"{\"city\":\"Zurich\"}","id":"call_58372","name":"antigravity_test_weather"},"toolResponse":{"ie":"antigravity_test_weather","responseJson":"{\"result\":\"ok\"}"}},"source":"SOURCE_MODEL","state":"STATE_DONE","stepIndex":1,"target":"TARGET_ENVIRMNMENT","text":"Weather check","textDelta":"","thinking":"","thinkingDelta":"","trajectoryId":"t"},"seqNum":"9"}"#,
    );
}

/// The normalizers apply what each type documents and nothing more: an
/// environment read as remote from a `type` other than a missing or null
/// one keeps that `type`, so a reader that rewrote it is still caught.
#[test]
fn normalizers_apply_only_the_documented_changes() {
    use crate::{NetworkConfig, RemoteEnvironment};
    use serde_json::json;
    let remote = EnvironmentSpec::Remote(RemoteEnvironment::new());
    for (input, expected) in [
        (json!({}), json!({"type": "remote"})),
        (json!({"type": null}), json!({"type": "remote"})),
        (json!({"type": 5}), json!({"type": 5})),
        (json!({"type": "remote"}), json!({"type": "remote"})),
    ] {
        assert_eq!(wire::environment_spec(&remote, &input), expected, "{input}");
    }
    let env_list = json!({"type": "remote", "env": [{"A": {"value": "1"}}, {"A": {"value": "2"}}]});
    assert_eq!(
        wire::environment_spec(&remote, &env_list),
        json!({"type": "remote", "env": {"A": {"value": "2"}}})
    );
    let entries = json!({"allowlist": [{"domain": ""}, {"domain": "x"}, {"mode": "m"}]});
    assert_eq!(
        wire::network_config(&NetworkConfig::allowlist(vec![]), &entries),
        json!({"allowlist": [{}, {"domain": "x"}, {"mode": "m"}]})
    );
    // An unknown spec and an id are written as they were read.
    let unknown: EnvironmentSpec = serde_json::from_value(json!({"type": 5})).unwrap();
    assert_eq!(
        wire::environment_spec(&unknown, &json!({"type": 5})),
        json!({"type": 5})
    );
}
