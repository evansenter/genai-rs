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
//! The strategies are `prop_oneof!` lists, which nothing checks for
//! exhaustiveness, so `*_ordinal` below match every variant exhaustively and
//! a deterministic sample asserts each strategy reaches all of them.

use std::collections::BTreeSet;

use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;
use serde_json::{Value, json};

use super::*;

/// The key added to every object. Not a field name any type models.
const INJECTED: &str = "x_future_field";

/// Adds [`INJECTED`] to every object in `value`, at every depth.
fn inject_everywhere(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.values_mut().for_each(inject_everywhere);
            map.insert(INJECTED.into(), json!({"nested": [1, "two", null]}));
        }
        Value::Array(items) => items.iter_mut().for_each(inject_everywhere),
        _ => {}
    }
}

/// Serializes `value`, injects unknown keys everywhere, and asserts the
/// injected wire round-trips exactly.
fn assert_injected_round_trip<T>(value: &T) -> Result<(), TestCaseError>
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let mut wire = serde_json::to_value(value).expect("serialize");
    inject_everywhere(&mut wire);
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
