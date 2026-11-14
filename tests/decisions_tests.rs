//! Live tests for the Decisions resource (`/v1beta/decisions`).
//!
//! ```bash
//! cargo nextest run --test decisions_tests --run-ignored all
//! ```

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented,
    reason = "a test reports a failure by panicking"
)]

mod common;

use std::collections::BTreeMap;

use common::{TINY_RED_PNG_BASE64, get_client};
use genai_rs::{
    Answer, Content, CreateDecisionRequest, CreateDecisionResponse, DEFAULT_DECISION_MODEL,
    GenaiError, NoulCriteria, Question,
};

/// Asserts `probabilities` is a distribution: each in `[0, 1]`, summing to 1.
fn assert_distribution(question: &str, probabilities: &BTreeMap<String, f64>) {
    assert!(!probabilities.is_empty(), "{question}: no probabilities");
    for (key, p) in probabilities {
        assert!((0.0..=1.0).contains(p), "{question}: {key} = {p}");
    }
    let total: f64 = probabilities.values().sum();
    assert!((total - 1.0).abs() < 1e-6, "{question}: sums to {total}");
}

/// The highest probability, and its key.
fn top(probabilities: &BTreeMap<String, f64>) -> (&str, f64) {
    probabilities
        .iter()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(key, p)| (key.as_str(), *p))
        .expect("a probability")
}

/// No field of the response or of an answer landed in `extra`: every field
/// the endpoint served is modeled.
fn assert_fully_read(response: &CreateDecisionResponse) {
    assert!(response.extra.is_empty(), "{:?}", response.extra);
    for (id, answer) in &response.answers {
        assert!(!answer.is_unknown(), "{id}: {answer:?}");
        assert_eq!(
            answer.extra(),
            Some(&Default::default()),
            "{id}: {answer:?}"
        );
    }
    let usage = response.usage.as_ref().expect("usage");
    assert!(usage.extra.is_empty(), "{:?}", usage.extra);
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_decision_answers_each_question_type() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };
    let request = CreateDecisionRequest::new(
        DEFAULT_DECISION_MODEL,
        "Review: the soup arrived cold, and the waiter ignored us for twenty minutes.",
    )
    .add_question(
        "negative",
        Question::noul_with_criteria(
            "Is the review negative?",
            NoulCriteria::new()
                .with_true_description("The reviewer disliked the visit")
                .with_false_description("The reviewer liked the visit"),
        ),
    )
    .add_question(
        "topic",
        Question::choice(
            "What is the review mainly about?",
            [
                ("food", "The food itself"),
                ("service", "The staff or the wait"),
                ("price", ""),
            ],
        ),
    )
    .add_question(
        "stars",
        Question::score(
            "How many stars would this reviewer give?",
            ["zero stars", "one star", "two stars", "three stars"],
        ),
    );

    let response = retry_request!([client, request] => {
        client.decisions().create(&request).await
    })
    .expect("decisions.create failed");
    println!("{response:#?}");
    assert_fully_read(&response);
    assert_eq!(response.answers.len(), 3, "{:?}", response.answers);
    assert!(response.block_reason().is_none());

    let Some(Answer::Noul {
        noul, confidence, ..
    }) = response.answer("negative")
    else {
        panic!("negative: {:?}", response.answer("negative"));
    };
    assert!(*noul > 0.5, "a plainly negative review read as {noul}");
    assert!((confidence - noul.max(1.0 - noul)).abs() < 1e-9);

    let Some(Answer::Choice {
        choice,
        confidence,
        probabilities,
        ..
    }) = response.answer("topic")
    else {
        panic!("topic: {:?}", response.answer("topic"));
    };
    assert_distribution("topic", probabilities);
    assert_eq!(
        probabilities.keys().collect::<Vec<_>>(),
        ["food", "price", "service"]
    );
    assert_eq!(top(probabilities), (choice.as_str(), *confidence));

    let Some(Answer::Score {
        score,
        confidence,
        legend,
        probabilities,
        ..
    }) = response.answer("stars")
    else {
        panic!("stars: {:?}", response.answer("stars"));
    };
    assert_distribution("stars", probabilities);
    assert_eq!(legend.get("0").map(String::as_str), Some("zero stars"));
    assert_eq!(legend.len(), 4, "{legend:?}");
    let expected: f64 = probabilities
        .iter()
        .map(|(level, p)| level.parse::<f64>().expect("a level index") * p)
        .sum();
    assert!((score - expected).abs() < 1e-6, "{score} vs {expected}");
    assert!((top(probabilities).1 - confidence).abs() < 1e-9);

    let usage = response.usage.as_ref().expect("usage");
    assert!(usage.total_input_tokens.is_some_and(|n| n > 0), "{usage:?}");
    assert_eq!(usage.total_output_tokens, Some(0), "{usage:?}");
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_decision_reads_content_input() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };
    let request = CreateDecisionRequest::new(
        DEFAULT_DECISION_MODEL,
        vec![
            Content::text("Here is a one-pixel image."),
            Content::image_data(TINY_RED_PNG_BASE64, "image/png"),
        ],
    )
    .add_question(
        "color",
        Question::choice(
            "What color is the pixel?",
            [("red", ""), ("green", ""), ("blue", "")],
        ),
    );

    let response = retry_request!([client, request] => {
        client.decisions().create(&request).await
    })
    .expect("decisions.create failed");
    println!("{response:#?}");
    assert_fully_read(&response);
    let Some(Answer::Choice { choice, .. }) = response.answer("color") else {
        panic!("color: {:?}", response.answer("color"));
    };
    assert_eq!(choice, "red");
    let usage = response.usage.as_ref().expect("usage");
    let modalities: Vec<&str> = usage
        .input_tokens_by_modality
        .iter()
        .flatten()
        .map(|m| m.modality.as_str())
        .collect();
    assert!(modalities.contains(&"image"), "{usage:?}");
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_decision_rejects_other_models_and_bad_questions() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };
    let wrong_model = CreateDecisionRequest::new(genai_rs::DEFAULT_MODEL, "x")
        .add_question("vowel", Question::noul("Is the letter x a vowel?"));
    match client.decisions().create(&wrong_model).await {
        Err(GenaiError::Api {
            status_code: 404,
            message,
            ..
        }) => assert!(message.contains("model not found"), "{message}"),
        other => panic!("expected a 404 for a non-decision model, got {other:?}"),
    }

    let one_option = CreateDecisionRequest::new(DEFAULT_DECISION_MODEL, "x")
        .add_question("pick", Question::choice("Pick one.", [("only", "")]));
    match client.decisions().create(&one_option).await {
        Err(GenaiError::Api {
            status_code: 400,
            message,
            ..
        }) => assert!(message.contains("between 2 and 255 options"), "{message}"),
        other => panic!("expected a 400 for a one-option choice, got {other:?}"),
    }

    let none = CreateDecisionRequest::new(DEFAULT_DECISION_MODEL, "x");
    match client.decisions().create(&none).await {
        Err(GenaiError::Api {
            status_code: 400,
            message,
            ..
        }) => assert!(message.contains("at least one question"), "{message}"),
        other => panic!("expected a 400 for no questions, got {other:?}"),
    }
}
