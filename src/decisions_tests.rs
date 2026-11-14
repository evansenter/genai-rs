//! Unit tests for the decisions request and response types.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::*;
use crate::DEFAULT_DECISION_MODEL;

fn to_json<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("serializes")
}

// =============================================================================
// Request
// =============================================================================

#[test]
fn a_noul_question_sends_its_instructions_and_no_criteria() {
    assert_eq!(
        to_json(&Question::noul("Is it positive?")),
        json!({"type": "noul", "instructions": "Is it positive?"})
    );
}

#[test]
fn a_noul_question_with_criteria_sends_the_descriptions_set() {
    let both = Question::noul_with_criteria(
        "Is it positive?",
        NoulCriteria::new()
            .with_true_description("liked it")
            .with_false_description("disliked it"),
    );
    assert_eq!(
        to_json(&both),
        json!({"type": "noul", "instructions": "Is it positive?", "criteria": {
            "true_description": "liked it", "false_description": "disliked it"}})
    );
    let one = Question::noul_with_criteria(
        "Is it positive?",
        NoulCriteria::new().with_false_description("disliked it"),
    );
    assert_eq!(
        to_json(&one)["criteria"],
        json!({"false_description": "disliked it"})
    );
    assert_eq!(to_json(&NoulCriteria::new()), json!({}));
}

#[test]
fn a_choice_question_sends_its_options_as_a_map() {
    let question = Question::choice(
        "What is the complaint about?",
        [("food", "The food itself"), ("price", "")],
    );
    assert_eq!(
        to_json(&question),
        json!({"type": "choice", "instructions": "What is the complaint about?",
               "criteria": {"food": "The food itself", "price": ""}})
    );
}

#[test]
fn a_score_question_sends_its_levels_in_order() {
    let question = Question::score("How many stars?", ["zero", "one", "two"]);
    assert_eq!(
        to_json(&question),
        json!({"type": "score", "instructions": "How many stars?",
               "criteria": ["zero", "one", "two"]})
    );
}

#[test]
fn an_unknown_question_is_sent_as_its_data() {
    let data = json!({"type": "rank", "instructions": "Order them", "items": ["a", "b"]});
    let question = Question::Unknown {
        question_type: "rank".into(),
        data: data.clone(),
    };
    assert_eq!(to_json(&question), data);
    assert!(question.is_unknown());
    assert_eq!(question.question_type(), "rank");
    assert_eq!(question.unknown_question_type(), Some("rank"));
    assert_eq!(question.unknown_data(), Some(&data));

    let known = Question::noul("q");
    assert!(!known.is_unknown());
    assert_eq!(known.unknown_question_type(), None);
    assert_eq!(known.unknown_data(), None);
}

#[test]
fn decision_input_converts_from_text_and_content() {
    assert_eq!(
        DecisionInput::from("hi"),
        DecisionInput::Text("hi".to_string())
    );
    assert_eq!(
        DecisionInput::from("hi".to_string()),
        DecisionInput::Text("hi".to_string())
    );
    assert_eq!(
        DecisionInput::from(Content::text("hi")),
        DecisionInput::Content(vec![Content::text("hi")])
    );
    let blocks = vec![Content::text("a"), Content::image_data("AAAA", "image/png")];
    assert_eq!(
        DecisionInput::from(blocks.clone()),
        DecisionInput::Content(blocks)
    );
    assert_eq!(to_json(&DecisionInput::from("hi")), json!("hi"));
    assert_eq!(
        to_json(&DecisionInput::from(Content::text("hi"))),
        json!([{"type": "text", "text": "hi"}])
    );
}

#[test]
fn a_request_sends_the_model_the_input_and_each_question_by_id() {
    let request = CreateDecisionRequest::new(DEFAULT_DECISION_MODEL, "The soup was cold.")
        .add_question("positive", Question::noul("Is the review positive?"))
        .add_question("stars", Question::score("Stars?", ["zero", "one"]));
    assert_eq!(
        to_json(&request),
        json!({
            "model": DEFAULT_DECISION_MODEL,
            "input": "The soup was cold.",
            "questions": {
                "positive": {"type": "noul", "instructions": "Is the review positive?"},
                "stars": {"type": "score", "instructions": "Stars?", "criteria": ["zero", "one"]},
            },
        })
    );
}

#[test]
fn a_second_question_under_one_id_replaces_the_first() {
    let request = CreateDecisionRequest::new("m", "x")
        .add_question("q", Question::noul("first"))
        .add_question("q", Question::noul("second"));
    assert_eq!(
        request.questions,
        BTreeMap::from([("q".to_string(), Question::noul("second"))])
    );
}

// =============================================================================
// Response
// =============================================================================

/// A response the endpoint served for one question of each type (live
/// 2026-11-14, on `DEFAULT_DECISION_MODEL`).
fn served() -> Value {
    json!({
        "answers": {
            "stars": {"confidence": 0.805481183110952, "score": 0.21115011479210363,
                      "probabilities": {"3": 0.0031135167999383237, "1": 0.18100103578593058,
                                        "0": 0.805481183110952, "2": 0.01040426430317905},
                      "legend": {"2": "two stars", "3": "three stars", "1": "one star",
                                 "0": "zero stars"},
                      "type": "score"},
            "positive": {"confidence": 0.9702023518428351, "noul": 0.029797648157164923,
                         "type": "noul"},
            "topic": {"confidence": 0.6231926427595381, "choice": "service",
                      "probabilities": {"food": 0.36355010821604444,
                                        "service": 0.6231926427595381,
                                        "price": 0.013257249024417382},
                      "type": "choice"},
        },
        "usage": {"total_tokens": 249, "total_input_tokens": 249,
                  "input_tokens_by_modality": [{"modality": "text", "tokens": 249}],
                  "total_output_tokens": 0},
    })
}

#[test]
fn a_served_response_reads_every_answer_type_and_round_trips() {
    let response: CreateDecisionResponse = serde_json::from_value(served()).unwrap();
    assert!(response.extra.is_empty(), "{:?}", response.extra);
    assert_eq!(response.answers.len(), 3);

    let Some(Answer::Noul {
        noul,
        confidence,
        extra,
    }) = response.answer("positive")
    else {
        panic!("positive: {:?}", response.answer("positive"));
    };
    assert!((noul - 0.029_797_648_157_164_923).abs() < 1e-12);
    assert!((confidence - (1.0 - noul)).abs() < 1e-12);
    assert!(extra.is_empty());

    let Some(Answer::Choice {
        choice,
        probabilities,
        ..
    }) = response.answer("topic")
    else {
        panic!("topic: {:?}", response.answer("topic"));
    };
    assert_eq!(choice, "service");
    assert_eq!(probabilities.len(), 3);

    let Some(Answer::Score {
        score,
        legend,
        probabilities,
        ..
    }) = response.answer("stars")
    else {
        panic!("stars: {:?}", response.answer("stars"));
    };
    assert_eq!(legend["0"], "zero stars");
    let expected: f64 = probabilities
        .iter()
        .map(|(level, p)| level.parse::<f64>().unwrap() * p)
        .sum();
    assert!((score - expected).abs() < 1e-9, "{score} vs {expected}");

    let usage = response.usage.as_ref().expect("usage");
    assert_eq!(usage.total_input_tokens, Some(249));
    assert_eq!(usage.total_output_tokens, Some(0));
    assert!(response.prompt_feedback.is_none());
    assert_eq!(response.block_reason(), None);
    assert_eq!(response.answer("missing"), None);

    assert_eq!(to_json(&response), served());
}

#[test]
fn answer_accessors_name_the_type_and_the_confidence() {
    let response: CreateDecisionResponse = serde_json::from_value(served()).unwrap();
    for (id, answer_type) in [
        ("positive", "noul"),
        ("topic", "choice"),
        ("stars", "score"),
    ] {
        let answer = response.answer(id).unwrap();
        assert_eq!(answer.answer_type(), answer_type);
        assert!(
            answer
                .confidence()
                .is_some_and(|c| (0.0..=1.0).contains(&c))
        );
        assert_eq!(answer.extra(), Some(&Map::new()));
        assert!(!answer.is_unknown());
        assert_eq!(answer.unknown_answer_type(), None);
        assert_eq!(answer.unknown_data(), None);
    }
}

#[test]
fn a_field_a_known_answer_does_not_model_is_kept_in_extra() {
    let wire = json!({"type": "noul", "noul": 0.9, "confidence": 0.9, "rationale": "clear"});
    let mut answer: Answer = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(
        answer.extra().unwrap().get("rationale"),
        Some(&json!("clear"))
    );
    assert_eq!(to_json(&answer), wire);

    // A hand-written extra never overrides a modeled field or the type.
    let extra = answer.extra_mut().unwrap();
    extra.insert("noul".into(), json!(0.1));
    extra.insert("type".into(), json!("score"));
    let back = to_json(&answer);
    assert_eq!(back["noul"], json!(0.9));
    assert_eq!(back["type"], json!("noul"));
}

#[test]
fn an_answer_of_an_unknown_type_is_kept_whole() {
    let wire = json!({"type": "rank", "order": ["b", "a"], "confidence": 0.7});
    let answer: Answer = serde_json::from_value(wire.clone()).unwrap();
    assert!(answer.is_unknown());
    assert_eq!(answer.answer_type(), "rank");
    assert_eq!(answer.unknown_answer_type(), Some("rank"));
    assert_eq!(answer.unknown_data(), Some(&wire));
    assert_eq!(answer.confidence(), None);
    assert_eq!(answer.extra(), None);
    assert_eq!(to_json(&answer), wire);

    let mut untyped: Answer = serde_json::from_value(json!({"noul": 0.5})).unwrap();
    assert_eq!(untyped.unknown_answer_type(), Some("<missing type>"));
    assert!(untyped.extra_mut().is_none());
}

#[test]
fn a_known_answer_with_a_field_of_the_wrong_type_is_kept_whole() {
    let wire = json!({"type": "choice", "choice": "a", "confidence": "high"});
    let answer: Answer = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(answer.unknown_answer_type(), Some("choice"));
    assert_eq!(answer.unknown_data(), Some(&wire));
}

/// Protobuf JSON leaves a zero value out, so an absent field is its zero
/// rather than a reason to drop the answer.
#[test]
fn an_absent_answer_field_reads_as_its_zero() {
    let answer: Answer =
        serde_json::from_value(json!({"type": "score", "confidence": 1.0})).unwrap();
    assert_eq!(
        answer,
        Answer::Score {
            score: 0.0,
            confidence: 1.0,
            legend: BTreeMap::new(),
            probabilities: BTreeMap::new(),
            extra: Map::new(),
        }
    );
}

#[test]
fn every_block_reason_reads_and_writes_its_wire_value() {
    for (wire, reason) in [
        ("safety", BlockReason::Safety),
        ("other", BlockReason::Other),
        ("blocklist", BlockReason::Blocklist),
        ("prohibited_content", BlockReason::ProhibitedContent),
        ("image_safety", BlockReason::ImageSafety),
    ] {
        assert_eq!(
            serde_json::from_value::<BlockReason>(json!(wire)).unwrap(),
            reason
        );
        assert_eq!(to_json(&reason), json!(wire));
    }
}

#[test]
fn a_blocked_input_has_feedback_and_no_answers() {
    let wire = json!({"prompt_feedback": {"block_reason": "prohibited_content",
                                          "block_reason_message": "Blocked."},
                      "usage": {"total_tokens": 10}});
    let response: CreateDecisionResponse = serde_json::from_value(wire.clone()).unwrap();
    assert!(response.answers.is_empty());
    assert_eq!(
        response.block_reason(),
        Some(&BlockReason::ProhibitedContent)
    );
    let feedback = response.prompt_feedback.as_ref().unwrap();
    assert_eq!(feedback.block_reason_message.as_deref(), Some("Blocked."));
    assert!(feedback.extra.is_empty());
    assert_eq!(to_json(&response), wire);
}

#[cfg(not(genai_strict_unknown))]
#[test]
fn an_unknown_block_reason_and_unknown_fields_are_kept() {
    let wire = json!({"prompt_feedback": {"block_reason": "spam", "score": 3},
                      "model_version": "decision-1-001"});
    let response: CreateDecisionResponse = serde_json::from_value(wire.clone()).unwrap();
    let reason = response.block_reason().unwrap();
    assert_eq!(reason.unknown_reason_type(), Some("spam"));
    assert_eq!(
        response
            .prompt_feedback
            .as_ref()
            .unwrap()
            .extra
            .get("score"),
        Some(&json!(3))
    );
    assert_eq!(
        response.extra.get("model_version"),
        Some(&json!("decision-1-001"))
    );
    assert_eq!(to_json(&response), wire);
}

#[test]
fn an_empty_response_reads_as_empty() {
    let response: CreateDecisionResponse = serde_json::from_value(json!({})).unwrap();
    assert_eq!(response, CreateDecisionResponse::default());
    assert_eq!(to_json(&response), json!({}));
}
