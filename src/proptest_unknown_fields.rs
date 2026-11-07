//! Property: an unknown field survives a round trip wherever it appears (#491).
//!
//! Each value is serialized, an unknown key is added to every JSON object in
//! the result at every depth (the step itself, a call's nested `arguments`,
//! its content blocks, their annotations, result items, places and review
//! snippets), and the injected wire must come back out of
//! deserialize/re-serialize unchanged. This runs under the default features
//! and under `genai_strict_unknown`, which rejects unknown *types* but must keep
//! unknown *fields*.
//!
//! A whole `InteractionResponse` gets the same treatment, so the request
//! config a stored interaction serves back is covered too: every known
//! `Tool`, `ResponseFormat` and `TranscriptionMode` variant and the
//! `tool_choice` restriction (D-034). [`NOT_INJECTED_UNDER`] names the
//! subtrees left alone, each with its reason.
//!
//! The strategies are `prop_oneof!` lists, which nothing checks for
//! exhaustiveness, so `*_ordinal` below match every variant exhaustively and
//! a deterministic sample asserts each strategy reaches all of them.

use std::collections::BTreeSet;

use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;
use serde_json::{Value, json};

use super::content::{
    arb_annotation, arb_content, arb_google_maps_result_item, arb_google_search_result_item,
    arb_known_content, arb_url_context_result_item,
};
use super::request::arb_transcription_mode;
use super::response::arb_interaction_response;
use super::steps::{arb_known_step, arb_step, arb_step_delta, arb_step_error};
use super::tools::arb_tool;
use super::*;

/// The key added to every object. Not a field name any type models.
const INJECTED: &str = "x_future_field";

/// Keys of an `InteractionResponse` whose values the injection leaves alone.
/// Everything else in it keeps an unknown field.
const NOT_INJECTED_UNDER: &[(&str, &str)] = &[
    (
        "headers",
        "an MCP server tool's HTTP headers: a map from header name to string \
         value, so every key is data and a non-string value is malformed",
    ),
    (
        "vertex_ai_search_config",
        "the retrieval tool's configs are sent only: the Gemini API rejects \
         `type: retrieval`, so no stored interaction serves one back (D-034's \
         SENT_ONLY). The retrieval tool itself keeps its `extra`",
    ),
    (
        "exa_ai_search_config",
        "the retrieval tool's, as `vertex_ai_search_config`",
    ),
    (
        "parallel_ai_search_config",
        "the retrieval tool's, as `vertex_ai_search_config`",
    ),
    (
        "rag_store_config",
        "the retrieval tool's, as `vertex_ai_search_config`",
    ),
];

/// Adds [`INJECTED`] to every object in `value`, at every depth, except
/// below a key named in `skip`.
fn inject_except_under(value: &mut Value, skip: &[&str]) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if !skip.contains(&key.as_str()) {
                    inject_except_under(child, skip);
                }
            }
            map.insert(INJECTED.into(), json!({"nested": [1, "two", null]}));
        }
        Value::Array(items) => items
            .iter_mut()
            .for_each(|item| inject_except_under(item, skip)),
        _ => {}
    }
}

/// Serializes `value`, injects unknown keys everywhere, and asserts the
/// injected wire round-trips exactly.
fn assert_injected_round_trip<T>(value: &T) -> Result<(), TestCaseError>
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    assert_injected_round_trip_except(value, &[])
}

/// [`assert_injected_round_trip`], leaving the values under `skip` alone.
fn assert_injected_round_trip_except<T>(value: &T, skip: &[&str]) -> Result<(), TestCaseError>
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let mut wire = serde_json::to_value(value).expect("serialize");
    inject_except_under(&mut wire, skip);
    let parsed: T = serde_json::from_value(wire.clone())
        .map_err(|e| TestCaseError::fail(format!("injected wire rejected: {e}\n{wire}")))?;
    let again = serde_json::to_value(&parsed).expect("re-serialize");
    prop_assert_eq!(again, wire);
    Ok(())
}

proptest! {
    #[test]
    fn step_keeps_unknown_fields_at_every_depth(step in arb_step()) {
        assert_injected_round_trip(&step)?;
    }

    #[test]
    fn step_delta_keeps_unknown_fields_at_every_depth(delta in arb_step_delta()) {
        assert_injected_round_trip(&delta)?;
    }

    #[test]
    fn content_keeps_unknown_fields_at_every_depth(content in arb_content()) {
        assert_injected_round_trip(&content)?;
    }

    #[test]
    fn annotation_keeps_unknown_fields_at_every_depth(annotation in arb_annotation()) {
        assert_injected_round_trip(&annotation)?;
    }

    /// The request config a stored interaction serves back keeps an unknown
    /// field on every known `Tool`, `ResponseFormat` and `TranscriptionMode`
    /// variant and beside `tool_choice`'s `allowed_tools`, as the steps do
    /// (C1 of the 2026-11-07 grading).
    #[test]
    fn interaction_response_keeps_unknown_fields_at_every_depth(
        response in arb_interaction_response(),
    ) {
        let skip: Vec<&str> = NOT_INJECTED_UNDER.iter().map(|(key, _)| *key).collect();
        assert_injected_round_trip_except(&response, &skip)?;
    }

    #[test]
    fn tool_keeps_unknown_fields_at_every_depth(tool in arb_tool()) {
        let skip: Vec<&str> = NOT_INJECTED_UNDER.iter().map(|(key, _)| *key).collect();
        assert_injected_round_trip_except(&tool, &skip)?;
    }

    #[test]
    fn transcription_mode_keeps_unknown_fields(mode in arb_transcription_mode()) {
        assert_injected_round_trip(&mode)?;
    }

    #[test]
    fn result_items_keep_unknown_fields(
        search in arb_google_search_result_item(),
        maps in arb_google_maps_result_item(),
        url in arb_url_context_result_item(),
        error in arb_step_error(),
    ) {
        assert_injected_round_trip(&search)?;
        assert_injected_round_trip(&maps)?;
        assert_injected_round_trip(&url)?;
        assert_injected_round_trip(&error)?;
    }
}

// =============================================================================
// Strategy coverage
// =============================================================================

/// Number of known `Step` variants; [`step_ordinal`] numbers them.
const STEP_VARIANTS: usize = 22;

/// Exhaustive by construction: a new variant does not compile until it gets
/// an ordinal here, and the next ordinal is `STEP_VARIANTS`.
fn step_ordinal(step: &Step) -> Option<usize> {
    Some(match step {
        Step::UserInput { .. } => 0,
        Step::ModelOutput { .. } => 1,
        Step::Thought { .. } => 2,
        Step::FunctionCall { .. } => 3,
        Step::FunctionResult { .. } => 4,
        Step::CodeExecutionCall { .. } => 5,
        Step::CodeExecutionResult { .. } => 6,
        Step::UrlContextCall { .. } => 7,
        Step::UrlContextResult { .. } => 8,
        Step::GoogleSearchCall { .. } => 9,
        Step::GoogleSearchResult { .. } => 10,
        Step::ToolCall { .. } => 11,
        Step::McpServerToolCall { .. } => 12,
        Step::McpServerToolResult { .. } => 13,
        Step::FileSearchCall { .. } => 14,
        Step::FileSearchResult { .. } => 15,
        Step::GoogleMapsCall { .. } => 16,
        Step::GoogleMapsResult { .. } => 17,
        Step::ProcessingCall { .. } => 18,
        Step::ProcessingResult { .. } => 19,
        Step::RetrievalCall { .. } => 20,
        Step::RetrievalResult { .. } => 21,
        Step::Unknown { .. } => return None,
    })
}

/// Number of known `StepDelta` variants; [`delta_ordinal`] numbers them.
const DELTA_VARIANTS: usize = 26;

/// Exhaustive by construction, like [`step_ordinal`].
fn delta_ordinal(delta: &StepDelta) -> Option<usize> {
    Some(match delta {
        StepDelta::Text { .. } => 0,
        StepDelta::Image { .. } => 1,
        StepDelta::Audio { .. } => 2,
        StepDelta::Video { .. } => 3,
        StepDelta::Document { .. } => 4,
        StepDelta::ThoughtSummary { .. } => 5,
        StepDelta::ThoughtSignature { .. } => 6,
        StepDelta::TextAnnotation { .. } => 7,
        StepDelta::ArgumentsDelta { .. } => 8,
        StepDelta::FunctionResult { .. } => 9,
        StepDelta::CodeExecutionCall { .. } => 10,
        StepDelta::CodeExecutionResult { .. } => 11,
        StepDelta::UrlContextCall { .. } => 12,
        StepDelta::UrlContextResult { .. } => 13,
        StepDelta::GoogleSearchCall { .. } => 14,
        StepDelta::GoogleSearchResult { .. } => 15,
        StepDelta::McpServerToolCall { .. } => 16,
        StepDelta::McpServerToolResult { .. } => 17,
        StepDelta::FileSearchCall { .. } => 18,
        StepDelta::FileSearchResult { .. } => 19,
        StepDelta::GoogleMapsCall { .. } => 20,
        StepDelta::GoogleMapsResult { .. } => 21,
        StepDelta::ProcessingCall { .. } => 22,
        StepDelta::ProcessingResult { .. } => 23,
        StepDelta::RetrievalCall { .. } => 24,
        StepDelta::RetrievalResult { .. } => 25,
        StepDelta::Unknown { .. } => return None,
    })
}

/// Number of known `Content` variants; [`content_ordinal`] numbers them.
const CONTENT_VARIANTS: usize = 5;

fn content_ordinal(content: &Content) -> Option<usize> {
    Some(match content {
        Content::Text { .. } => 0,
        Content::Image { .. } => 1,
        Content::Audio { .. } => 2,
        Content::Video { .. } => 3,
        Content::Document { .. } => 4,
        Content::Unknown { .. } => return None,
    })
}

/// Number of known `Annotation` variants; [`annotation_ordinal`] numbers
/// them.
const ANNOTATION_VARIANTS: usize = 5;

fn annotation_ordinal(annotation: &Annotation) -> Option<usize> {
    Some(match annotation {
        Annotation::UrlCitation { .. } => 0,
        Annotation::FileCitation { .. } => 1,
        Annotation::PlaceCitation { .. } => 2,
        Annotation::SpeechMetadata { .. } => 3,
        Annotation::WordInfo { .. } => 4,
        Annotation::Unknown { .. } => return None,
    })
}

/// Number of known `Tool` variants; [`tool_ordinal`] numbers them.
const TOOL_VARIANTS: usize = 9;

fn tool_ordinal(tool: &Tool) -> Option<usize> {
    Some(match tool {
        Tool::Function { .. } => 0,
        Tool::GoogleSearch { .. } => 1,
        Tool::GoogleMaps { .. } => 2,
        Tool::CodeExecution { .. } => 3,
        Tool::UrlContext { .. } => 4,
        Tool::ComputerUse { .. } => 5,
        Tool::McpServer { .. } => 6,
        Tool::FileSearch { .. } => 7,
        Tool::Retrieval { .. } => 8,
        Tool::Unknown { .. } => return None,
    })
}

/// Number of known `ResponseFormat` variants; [`format_ordinal`] numbers
/// them.
const FORMAT_VARIANTS: usize = 4;

fn format_ordinal(format: &ResponseFormat) -> Option<usize> {
    Some(match format {
        ResponseFormat::Text { .. } => 0,
        ResponseFormat::Audio { .. } => 1,
        ResponseFormat::Image { .. } => 2,
        ResponseFormat::Video { .. } => 3,
        ResponseFormat::Unknown { .. } => return None,
    })
}

/// Number of known `TranscriptionMode` variants; [`mode_ordinal`] numbers
/// them.
const MODE_VARIANTS: usize = 2;

fn mode_ordinal(mode: &TranscriptionMode) -> Option<usize> {
    Some(match mode {
        TranscriptionMode::Smart { .. } => 0,
        TranscriptionMode::Verbatim { .. } => 1,
        TranscriptionMode::Unknown { .. } => return None,
    })
}

/// Number of known `ToolChoice` variants; [`choice_ordinal`] numbers them.
const CHOICE_VARIANTS: usize = 2;

fn choice_ordinal(choice: &ToolChoice) -> Option<usize> {
    Some(match choice {
        ToolChoice::Mode(_) => 0,
        ToolChoice::AllowedTools { .. } => 1,
        ToolChoice::Unknown { .. } => return None,
    })
}

/// The request-side variants a response carries, as `(union, ordinal)`:
/// 0 for `Tool`, 1 for `ResponseFormat`, 2 for `TranscriptionMode` and 3 for
/// `ToolChoice`.
fn request_side_ordinals(response: &InteractionResponse) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    for tool in response.tools.iter().flatten() {
        found.extend(tool_ordinal(tool).map(|n| (0, n)));
    }
    let formats = match &response.response_format {
        Some(ResponseFormatSpec::Single(format)) => std::slice::from_ref(format),
        Some(ResponseFormatSpec::List(formats)) => formats.as_slice(),
        _ => &[],
    };
    for format in formats {
        found.extend(format_ordinal(format).map(|n| (1, n)));
    }
    if let Some(config) = &response.generation_config {
        let mode = config
            .transcription_config
            .as_ref()
            .and_then(|transcription| transcription.mode.as_ref());
        found.extend(mode.and_then(mode_ordinal).map(|n| (2, n)));
        let choice = config.tool_choice.as_ref();
        found.extend(choice.and_then(choice_ordinal).map(|n| (3, n)));
    }
    found
}

/// The known-variant ordinals a strategy produces over a fixed sample.
fn ordinals_reached<S: Strategy>(
    strategy: S,
    ordinal: impl Fn(&S::Value) -> Option<usize>,
) -> BTreeSet<usize> {
    let mut runner = TestRunner::deterministic();
    (0..4000)
        .filter_map(|_| ordinal(&strategy.new_tree(&mut runner).expect("sample").current()))
        .collect()
}

#[test]
fn strategies_reach_every_known_variant() {
    let all = |n: usize| (0..n).collect::<BTreeSet<_>>();
    assert_eq!(
        ordinals_reached(arb_known_step(), step_ordinal),
        all(STEP_VARIANTS),
        "arb_known_step misses a Step variant"
    );
    assert_eq!(
        ordinals_reached(arb_step_delta(), delta_ordinal),
        all(DELTA_VARIANTS),
        "arb_step_delta misses a StepDelta variant"
    );
    assert_eq!(
        ordinals_reached(arb_known_content(), content_ordinal),
        all(CONTENT_VARIANTS),
        "arb_known_content misses a Content variant"
    );
    assert_eq!(
        ordinals_reached(arb_annotation(), annotation_ordinal),
        all(ANNOTATION_VARIANTS),
        "arb_annotation misses an Annotation variant"
    );
}

/// The response property above reaches every request-side variant it
/// claims to cover.
#[test]
fn the_response_strategy_reaches_every_request_side_variant() {
    let mut runner = TestRunner::deterministic();
    let strategy = arb_interaction_response();
    let mut reached = BTreeSet::new();
    for _ in 0..2000 {
        let response = strategy.new_tree(&mut runner).expect("sample").current();
        reached.extend(request_side_ordinals(&response));
    }
    for (union, name, count) in [
        (0, "Tool", TOOL_VARIANTS),
        (1, "ResponseFormat", FORMAT_VARIANTS),
        (2, "TranscriptionMode", MODE_VARIANTS),
        (3, "ToolChoice", CHOICE_VARIANTS),
    ] {
        let got: BTreeSet<usize> = reached
            .iter()
            .filter(|(u, _)| *u == union)
            .map(|(_, n)| *n)
            .collect();
        assert_eq!(
            got,
            (0..count).collect(),
            "arb_interaction_response misses a {name} variant"
        );
    }
}
