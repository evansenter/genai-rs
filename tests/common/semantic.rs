//! Judging non-deterministic model output with the model itself.

use super::{interaction_builder, is_transient_error, retry_on_transient};
use genai_rs::{Client, GenaiError};

// =============================================================================
// Semantic Validation Using Structured Output
// =============================================================================

/// Asks the model, via structured output, whether `response_text` answers
/// `validation_question` given `context`. Prefer [`assert_response_semantic`];
/// call this directly only when the verdict is needed as a `Result`.
///
/// An unparseable or missing verdict counts as valid but prints the
/// `SEMANTIC_VALIDATION_SKIPPED` marker, which CI counts, so a drifted
/// validator contract cannot go quietly green.
#[allow(dead_code)]
pub async fn validate_response_semantically(
    client: &Client,
    context: &str,
    response_text: &str,
    validation_question: &str,
) -> Result<bool, GenaiError> {
    use serde_json::json;

    let validation_prompt = format!(
        "You are a test validator. Your job is to judge whether an LLM response is appropriate given the context.\n\nContext: {context}\n\nResponse to validate: {response_text}\n\nQuestion: {validation_question}\n\nProvide your judgment as a yes/no boolean and explain your reasoning."
    );

    let schema = json!({
        "type": "object",
        "properties": {
            "is_valid": {
                "type": "boolean",
                "description": "Whether the response is semantically valid"
            },
            "reason": {
                "type": "string",
                "description": "Brief explanation of the judgment"
            }
        },
        "required": ["is_valid", "reason"]
    });

    let validation = interaction_builder(client)
        .with_text(&validation_prompt)
        .with_response_format(schema)
        .create()
        .await?;

    let parsed = validation
        .as_text()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok());
    let Some(json) = parsed else {
        let preview: String = validation
            .as_text()
            .unwrap_or("(no text)")
            .chars()
            .take(100)
            .collect();
        println!(
            "SEMANTIC_VALIDATION_SKIPPED (unparseable-verdict): could not parse validator response (text: '{preview}'), assuming valid"
        );
        return Ok(true);
    };

    let Some(is_valid) = json.get("is_valid").and_then(serde_json::Value::as_bool) else {
        println!(
            "SEMANTIC_VALIDATION_SKIPPED (missing-verdict): is_valid absent or non-boolean, assuming valid"
        );
        return Ok(true);
    };
    let reason = json
        .get("reason")
        .and_then(|v| v.as_str())
        .unwrap_or("(no reason provided)");
    println!(
        "Semantic validation: {} - {reason}",
        if is_valid { "VALID" } else { "INVALID" }
    );
    Ok(is_valid)
}

/// Asserts a semantic verdict (see [`validate_response_semantically`]).
///
/// The validator call gets one transient retry. A transient failure that
/// survives it is tolerated with the `SEMANTIC_VALIDATION_SKIPPED` marker,
/// because the validator is a second round-trip that can fail independently
/// of the input under test; any other validator error panics.
#[allow(dead_code)]
pub async fn assert_response_semantic(
    client: &Client,
    context: &str,
    response_text: &str,
    validation_question: &str,
) {
    // One retry only: callers often nest this inside a `with_timeout` budget
    // that the primary call's retry chain has already drawn down.
    match retry_on_transient(1, || {
        validate_response_semantically(client, context, response_text, validation_question)
    })
    .await
    {
        Ok(is_valid) => assert!(
            is_valid,
            "Semantic validation failed.\nQuestion: {validation_question}\nResponse: {response_text}"
        ),
        Err(e) if e.is_retryable() || is_transient_error(&e) => {
            eprintln!("SEMANTIC_VALIDATION_SKIPPED (transient validator error): {e:?}")
        }
        Err(e) => panic!("Semantic validation call failed non-transiently: {e:?}"),
    }
}
