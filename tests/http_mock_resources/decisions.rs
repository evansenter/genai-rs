//! Decisions: the create request on the wire, the response it parses, and
//! the Interactions error envelope the endpoint answers with.

use super::*;

/// A response the endpoint served (live 2026-11-14, on
/// `DEFAULT_DECISION_MODEL`), one answer of each type.
fn served() -> Value {
    json!({
        "answers": {
            "stars": {"confidence": 0.805481183110952, "score": 0.21115011479210363,
                      "probabilities": {"0": 0.805481183110952, "1": 0.18100103578593058,
                                        "2": 0.01040426430317905, "3": 0.0031135167999383237},
                      "legend": {"0": "zero stars", "1": "one star", "2": "two stars",
                                 "3": "three stars"},
                      "type": "score"},
            "positive": {"confidence": 0.9702023518428351, "noul": 0.029797648157164923,
                         "type": "noul"},
            "topic": {"confidence": 0.6231926427595381, "choice": "service",
                      "probabilities": {"food": 0.36355010821604444,
                                        "service": 0.6231926427595381,
                                        "price": 0.013257249024417382},
                      "type": "choice"}
        },
        "usage": {"total_tokens": 249, "total_input_tokens": 249,
                  "input_tokens_by_modality": [{"modality": "text", "tokens": 249}],
                  "total_output_tokens": 0}
    })
}

#[tokio::test]
async fn decision_create_sends_the_documented_request() {
    let stub = ok_stub().await;
    let client = stub.client();
    let c = &client;
    assert_wire(
        &stub,
        vec![
            wire(
                "POST",
                "/v1beta/decisions",
                Some(json!({
                    "model": genai_rs::DEFAULT_DECISION_MODEL,
                    "input": "The soup was cold.",
                    "questions": {
                        "positive": {"type": "noul", "instructions": "Is it positive?",
                                     "criteria": {"true_description": "liked"}},
                        "stars": {"type": "score", "instructions": "Stars?",
                                  "criteria": ["zero", "one"]},
                        "topic": {"type": "choice", "instructions": "About?",
                                  "criteria": {"food": "The food", "service": ""}}
                    }
                })),
                async move {
                    let request = CreateDecisionRequest::new(
                        genai_rs::DEFAULT_DECISION_MODEL,
                        "The soup was cold.",
                    )
                    .add_question(
                        "positive",
                        Question::noul_with_criteria(
                            "Is it positive?",
                            NoulCriteria::new().with_true_description("liked"),
                        ),
                    )
                    .add_question(
                        "topic",
                        Question::choice("About?", [("food", "The food"), ("service", "")]),
                    )
                    .add_question("stars", Question::score("Stars?", ["zero", "one"]));
                    c.decisions().create(&request).await
                },
            ),
            wire(
                "POST",
                "/v1beta/decisions",
                Some(json!({
                    "model": format!("models/{}", genai_rs::DEFAULT_DECISION_MODEL),
                    "input": [{"type": "text", "text": "A pixel:"},
                              {"type": "image", "data": "AAAA", "mime_type": "image/png"}],
                    "questions": {"q": {"type": "noul", "instructions": "Is it red?"}}
                })),
                async move {
                    let request = CreateDecisionRequest::new(
                        format!("models/{}", genai_rs::DEFAULT_DECISION_MODEL),
                        vec![
                            genai_rs::Content::text("A pixel:"),
                            genai_rs::Content::image_data("AAAA", "image/png"),
                        ],
                    )
                    .add_question("q", Question::noul("Is it red?"));
                    c.decisions().create(&request).await
                },
            ),
        ],
    )
    .await;
}

#[tokio::test]
async fn decision_create_parses_every_answer_type() {
    let stub = Stub::replying(vec![Reply::json(200, served())]).await;
    let request = CreateDecisionRequest::new(genai_rs::DEFAULT_DECISION_MODEL, "x")
        .add_question("positive", Question::noul("Is it positive?"));

    let response = stub.client().decisions().create(&request).await.unwrap();

    assert!(matches!(
        response.answer("positive"),
        Some(Answer::Noul { noul, .. }) if *noul < 0.5
    ));
    assert!(matches!(
        response.answer("topic"),
        Some(Answer::Choice { choice, .. }) if choice == "service"
    ));
    assert!(matches!(
        response.answer("stars"),
        Some(Answer::Score { legend, .. }) if legend.len() == 4
    ));
    assert_eq!(
        response.usage.as_ref().and_then(|u| u.total_input_tokens),
        Some(249)
    );
    assert!(response.extra.is_empty());
    // Parsed from text, a float can land one ulp off the literal, so the
    // round trip is checked on the typed value.
    let again: genai_rs::CreateDecisionResponse =
        serde_json::from_value(serde_json::to_value(&response).unwrap()).unwrap();
    assert_eq!(again, response);
}

#[tokio::test]
async fn decision_errors_map_from_the_interactions_envelope() {
    let stub = Stub::replying(vec![Reply::json(
        400,
        json!({"error": {"message": "Request must contain at least one question.",
                         "code": "invalid_request"}}),
    )])
    .await;
    let request = CreateDecisionRequest::new(genai_rs::DEFAULT_DECISION_MODEL, "x");

    let err = stub
        .client()
        .decisions()
        .create(&request)
        .await
        .unwrap_err();

    assert!(
        matches!(
            &err,
            GenaiError::Api { status_code: 400, message, .. }
                if message == "invalid_request: Request must contain at least one question."
        ),
        "{err:?}"
    );
    assert!(!err.is_retryable());
}
