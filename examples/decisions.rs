//! Decisions: calibrated answers to structured questions about one input.
//!
//! `client.decisions().create()` evaluates every question against the same
//! input and writes no text: a noul (yes/no) question comes back as the
//! probability of `true`, a choice question as a probability per option,
//! and a score question as a probability per rubric level with the expected
//! level. Each answer's `confidence` is the probability of its winning
//! outcome.
//!
//! Decisions run only on `DEFAULT_DECISION_MODEL`; any other model is a 404.
//! A choice takes 2 to 255 options and a score 2 to 255 levels.
//!
//! Run with: `cargo run --example decisions`

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "an example may stop on a missing key or a poisoned lock; the library may not"
)]

use genai_rs::{Answer, Client, CreateDecisionRequest, DEFAULT_DECISION_MODEL, Question};
use std::collections::BTreeMap;
use std::env;
use std::error::Error;

const REVIEWS: [&str; 2] = [
    "The pasta was perfect and our server checked on us just often enough.",
    "We waited forty minutes for cold soup, and nobody apologized.",
];

/// The option or level with the highest probability.
fn top(probabilities: &BTreeMap<String, f64>) -> Result<(&str, f64), Box<dyn Error>> {
    probabilities
        .iter()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(key, p)| (key.as_str(), *p))
        .ok_or_else(|| "an answer with no probabilities".into())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = Client::builder(api_key).build()?;

    let mut positive = Vec::new();
    for review in REVIEWS {
        let request = CreateDecisionRequest::new(DEFAULT_DECISION_MODEL, review)
            .add_question("positive", Question::noul("Is the review positive?"))
            .add_question(
                "topic",
                Question::choice(
                    "What does the review mostly talk about?",
                    [
                        ("food", "The dishes themselves"),
                        ("service", "The staff or the wait"),
                    ],
                ),
            )
            .add_question(
                "stars",
                Question::score(
                    "How many stars would this reviewer give?",
                    [
                        "one star",
                        "two stars",
                        "three stars",
                        "four stars",
                        "five stars",
                    ],
                ),
            );
        let response = client.decisions().create(&request).await?;

        println!("{review}");
        let Some(Answer::Noul {
            noul, confidence, ..
        }) = response.answer("positive")
        else {
            return Err(format!("no noul answer: {:?}", response.answer("positive")).into());
        };
        println!("  positive: P(true) = {noul:.3} (confidence {confidence:.3})");
        positive.push(*noul);

        let Some(Answer::Choice {
            choice,
            confidence,
            probabilities,
            ..
        }) = response.answer("topic")
        else {
            return Err(format!("no choice answer: {:?}", response.answer("topic")).into());
        };
        println!("  topic: {choice} (confidence {confidence:.3}) from {probabilities:.3?}");
        if top(probabilities)?.0 != choice {
            return Err(format!("{choice} is not the likeliest option: {probabilities:?}").into());
        }

        let Some(Answer::Score {
            score,
            legend,
            probabilities,
            ..
        }) = response.answer("stars")
        else {
            return Err(format!("no score answer: {:?}", response.answer("stars")).into());
        };
        let (level, p) = top(probabilities)?;
        let described = legend.get(level).map_or("?", String::as_str);
        println!("  stars: expected level {score:.2}, likeliest {described} ({p:.3})");

        if let Some(usage) = &response.usage {
            println!("  input tokens: {:?}", usage.total_input_tokens);
        }
    }

    // The praise reads as positive and the complaint does not.
    if positive[0] <= 0.5 || positive[1] >= 0.5 {
        return Err(format!("positive probabilities out of order: {positive:?}").into());
    }
    Ok(())
}
