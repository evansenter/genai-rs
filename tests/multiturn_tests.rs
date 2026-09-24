//! Conversation mechanics: long chains, branching, explicit turn arrays,
//! system-instruction inheritance, and usage metadata.
//!
//! ```bash
//! cargo nextest run --test multiturn_tests --run-ignored all
//! ```

mod common;

use common::{
    assert_response_semantic, extended_test_timeout, get_client, get_inspecting_client,
    interaction_builder, stateful_builder, with_timeout,
};
use genai_rs::{FunctionCallingMode, FunctionDeclaration, InteractionStatus, Step};
use serde_json::json;

// =============================================================================
// Long Conversations
// =============================================================================

/// Ten facts over ten chained turns, then a recall question.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_very_long_conversation() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let facts = [
        "My name is Alice.",
        "I live in Seattle.",
        "I work as a software engineer.",
        "My favorite programming language is Rust.",
        "I have a dog named Max.",
        "My birthday is in March.",
        "I enjoy hiking on weekends.",
        "My favorite food is sushi.",
        "I drive a blue car.",
        "I went to Stanford for college.",
    ];

    with_timeout(extended_test_timeout(), async {
        let mut previous_id: Option<String> = None;
        for (i, fact) in facts.iter().enumerate() {
            let fact = (*fact).to_string();
            let prev = previous_id.clone();
            let response = retry_request!([client, fact, prev] => {
                let builder = stateful_builder(&client).with_text(&fact);
                match &prev {
                    Some(prev_id) => builder.with_previous_interaction(prev_id).create().await,
                    None => builder.create().await,
                }
            })
            .unwrap_or_else(|e| panic!("Turn {} failed: {e:?}", i + 1));
            previous_id = response.id;
        }

        let prev = previous_id.expect("chained turns should have ids");
        let final_response = retry_request!([client, prev] => {
            stateful_builder(&client)
                .with_previous_interaction(&prev)
                .with_text("What do you know about me? List everything you can remember.")
                .create()
                .await
        })
        .expect("Final turn failed");

        assert_eq!(final_response.status, InteractionStatus::Completed);
        let text = final_response.as_text().expect("Should have text response");
        assert_response_semantic(
            &client,
            &format!(
                "Over ten earlier turns the user said: {}. Now they asked what the model remembers.",
                facts.join(" ")
            ),
            text,
            "Does this response correctly recall at least seven of those ten facts?",
        )
        .await;
    })
    .await;
}

// =============================================================================
// Mixed Function/Text Turns
// =============================================================================

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_conversation_function_then_text() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let get_weather = FunctionDeclaration::builder("get_weather")
        .description("Get the current weather")
        .parameter("city", json!({"type": "string"}))
        .required(vec!["city".to_string()])
        .build();

    // Turn 1: forced call, so the rest of the conversation always happens.
    let response1 = retry_request!([client, get_weather] => {
        stateful_builder(&client)
            .with_text("What's the weather in Tokyo?")
            .add_function(get_weather.clone())
            .with_function_calling_mode(FunctionCallingMode::Any)
            .create()
            .await
    })
    .expect("Turn 1 failed");

    let calls = response1.function_calls();
    let call = calls
        .first()
        .expect("FunctionCallingMode::Any should force a call");

    // Turn 2: function result.
    let result = Step::function_result(
        "get_weather",
        call.id.to_string(),
        json!({"temperature": "25°C", "conditions": "sunny"}),
    );
    let prev_id = response1.id.clone().expect("id should exist");
    let response2 = retry_request!([client, prev_id, get_weather, result] => {
        stateful_builder(&client)
            .with_previous_interaction(&prev_id)
            .with_history(vec![result.clone()])
            .add_function(get_weather.clone())
            .create()
            .await
    })
    .expect("Turn 2 failed");
    assert!(response2.has_text(), "Turn 2 should answer from the result");

    // Turn 3: a plain follow-up that needs the turn-2 context.
    let prev_id = response2.id.clone().expect("id should exist");
    let response3 = retry_request!([client, prev_id, get_weather] => {
        stateful_builder(&client)
            .with_previous_interaction(&prev_id)
            .with_text("Should I bring a jacket?")
            .add_function(get_weather.clone())
            .create()
            .await
    })
    .expect("Turn 3 failed");

    let text = response3
        .as_text()
        .expect("Turn 3 should have text response");
    assert_response_semantic(
        &client,
        "Function returned 25°C sunny weather for Tokyo. User asked if they should bring a jacket.",
        text,
        "Does this response use the weather context (warm/sunny, 25°C) to advise about whether to bring a jacket?",
    )
    .await;
}

// =============================================================================
// Conversation Branching
// =============================================================================

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_conversation_branch() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let response1 = retry_request!([client] => {
        stateful_builder(&client)
            .with_text("My favorite color is red.")
            .create()
            .await
    })
    .expect("Turn 1 failed");

    let prev_id = response1.id.clone().expect("id should exist");
    let response2 = retry_request!([client, prev_id] => {
        stateful_builder(&client)
            .with_previous_interaction(&prev_id)
            .with_text("My favorite number is 7.")
            .create()
            .await
    })
    .expect("Turn 2 failed");

    let prev_id = response2.id.clone().expect("id should exist");
    let response3 = retry_request!([client, prev_id] => {
        stateful_builder(&client)
            .with_previous_interaction(&prev_id)
            .with_text("My favorite animal is a cat.")
            .create()
            .await
    })
    .expect("Turn 3 failed");

    // Branch from turn 2: the cat fact was said on a sibling branch only.
    let prev_id = response2.id.clone().expect("id should exist");
    let branch_response = retry_request!([client, prev_id] => {
        stateful_builder(&client)
            .with_previous_interaction(&prev_id)
            .with_text("List every favorite of mine that you know so far.")
            .create()
            .await
    })
    .expect("Branch failed");

    let text = branch_response
        .as_text()
        .expect("Should have text response");
    assert_response_semantic(
        &client,
        "The user said their favorite color is red (turn 1) and favorite number is 7 (turn 2). \
         This question branches from turn 2, so the model has never been told any favorite animal.",
        text,
        "Does this response mention red or 7, and NOT claim that the user's favorite animal is a cat?",
    )
    .await;

    // The original line of the conversation still has turn 3.
    let prev_id = response3.id.clone().expect("id should exist");
    let continue_response = retry_request!([client, prev_id] => {
        stateful_builder(&client)
            .with_previous_interaction(&prev_id)
            .with_text("And what's my favorite animal?")
            .create()
            .await
    })
    .expect("Continue failed");

    let continue_text = continue_response.as_text().expect("Should have text");
    assert_response_semantic(
        &client,
        "User said their favorite animal is a cat in turn 3, then asked what their favorite animal is",
        continue_text,
        "Does this response correctly identify cat as the user's favorite animal?",
    )
    .await;
}

// =============================================================================
// System Instruction Inheritance
// =============================================================================

/// `system_instruction` is not inherited through `previous_interaction_id`:
/// the server records it only on the turn that sent it, so every turn that
/// needs it must resend it.
///
/// This pins the server's record rather than the model's behaviour on an
/// un-resent turn, because that behaviour is not a clean signal: the model's
/// replayed thoughts from turn 1 can restate the instruction, so it often
/// still follows it (4 of 6 runs for a conditional rule, verified live
/// 2026-09-24). `InteractionResponse` does not expose `system_instruction`,
/// so the stored record is read from the raw GET body.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_system_instruction_not_inherited() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };
    let (inspecting, bodies) = get_inspecting_client().expect("key checked above");
    let stored_instruction = |id: String| {
        let (inspecting, bodies) = (&inspecting, &bodies);
        async move {
            inspecting
                .get_interaction(&id)
                .await
                .expect("get_interaction");
            bodies.take().get("system_instruction").cloned()
        }
    };

    const RULE: &str = "If the user asks for the capital of any country, reply with only \
                        the single word BANANA and nothing else. Otherwise answer normally.";

    let turn1 = retry_request!([client] => {
        stateful_builder(&client)
            .with_system_instruction(RULE)
            .with_text("Hi, how are you today?")
            .create()
            .await
    })
    .expect("Turn 1 failed");
    let turn1_id = turn1.id.expect("turn 1 id");

    let turn2 = retry_request!([client, turn1_id] => {
        stateful_builder(&client)
            .with_previous_interaction(&turn1_id)
            .with_text("What is the capital of France?")
            .create()
            .await
    })
    .expect("Turn 2 failed");
    let turn2_id = turn2.id.expect("turn 2 id");

    assert_eq!(
        stored_instruction(turn1_id).await,
        Some(json!(RULE)),
        "turn 1 should record the instruction it sent"
    );
    assert_eq!(
        stored_instruction(turn2_id.clone()).await,
        None,
        "turn 2 sent no instruction and should not inherit turn 1's"
    );

    // Resending it is what makes it apply.
    let turn3 = retry_request!([client, turn2_id] => {
        stateful_builder(&client)
            .with_previous_interaction(&turn2_id)
            .with_system_instruction(RULE)
            .with_text("What is the capital of Germany?")
            .create()
            .await
    })
    .expect("Turn 3 failed");
    let text = turn3.as_text().expect("Turn 3 should have text");
    assert_response_semantic(
        &client,
        "The system instruction said: answer any capital-city question with only the word BANANA. \
         The user asked for the capital of Germany.",
        text,
        "Is this response just the word BANANA rather than naming Berlin?",
    )
    .await;
}

// =============================================================================
// Usage Metadata
// =============================================================================

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_usage_metadata_returned() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let response = stateful_builder(&client)
        .with_text("What is the capital of France? Answer briefly.")
        .create()
        .await
        .expect("Interaction failed");

    assert_eq!(response.status, InteractionStatus::Completed);
    let usage = response.usage.expect("usage should be reported");
    let input = usage.total_input_tokens.expect("input tokens");
    let output = usage.total_output_tokens.expect("output tokens");
    let total = usage.total_tokens.expect("total tokens");
    assert!(
        input > 0 && output > 0,
        "token counts should be positive: {usage:?}"
    );
    assert!(
        total >= input + output,
        "total should cover input and output: {usage:?}"
    );
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_usage_longer_response() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let output_tokens = |response: genai_rs::InteractionResponse| {
        response
            .usage
            .and_then(|u| u.total_output_tokens)
            .expect("output tokens should be reported")
    };

    let short = output_tokens(
        stateful_builder(&client)
            .with_text("Say 'hello'")
            .create()
            .await
            .expect("Short interaction failed"),
    );
    let long = output_tokens(
        stateful_builder(&client)
            .with_text("Write a 100-word paragraph about space exploration.")
            .create()
            .await
            .expect("Long interaction failed"),
    );

    assert!(
        long > short,
        "a longer response should use more output tokens: {long} vs {short}"
    );
}

// =============================================================================
// Explicit Step Arrays
// =============================================================================

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_explicit_turns_basic() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let turns = vec![
        Step::user_text("What is 2+2?"),
        Step::model_text("2+2 equals 4."),
        Step::user_text("And what's that times 3?"),
    ];

    let response = interaction_builder(&client)
        .with_history(turns)
        .create()
        .await
        .expect("Request failed");

    assert_eq!(response.status, InteractionStatus::Completed);
    let text = response.as_text().expect("Should have text response");
    // A computed value, so a substring check is deterministic.
    assert!(
        text.contains("12"),
        "Response should contain 12. Got: {text}"
    );
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_conversation_builder_fluent_api() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let response = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .conversation()
        .user("My name is Alice and I love hiking.")
        .model("Nice to meet you, Alice! Hiking is a wonderful outdoor activity.")
        .user("What's my name and what do I enjoy doing?")
        .done()
        .create()
        .await
        .expect("Request failed");

    assert_eq!(response.status, InteractionStatus::Completed);
    let text = response.as_text().expect("Should have text response");
    assert_response_semantic(
        &client,
        "User said their name is Alice and they love hiking, then asked what their name is and what they enjoy",
        text,
        "Does this response correctly recall that the user's name is Alice AND that they enjoy hiking?",
    )
    .await;
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_explicit_turns_context_preservation() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let turns = vec![
        Step::user_text("I'm planning a trip to Tokyo next month."),
        Step::model_text(
            "Tokyo is an amazing destination! It offers incredible food, \
             ancient temples, modern technology, and beautiful cherry blossoms. \
             What aspects of the city are you most excited to explore?",
        ),
        Step::user_text("I love trying local cuisine."),
        Step::model_text(
            "Great choice! Tokyo has some of the best food in the world. \
             Try ramen in a local shop, fresh sushi at Tsukiji Outer Market, \
             and don't miss Japanese convenience store food - it's surprisingly excellent!",
        ),
        Step::user_text("Where am I going and what do I enjoy?"),
    ];

    let response = interaction_builder(&client)
        .with_history(turns)
        .create()
        .await
        .expect("Request failed");

    assert_eq!(response.status, InteractionStatus::Completed);
    let text = response.as_text().expect("Should have text response");
    assert_response_semantic(
        &client,
        "User discussed planning a trip to Tokyo and mentioned loving local cuisine. Then asked where they're going and what they enjoy.",
        text,
        "Does this response correctly recall that the user is going to Tokyo/Japan AND that they enjoy food/cuisine?",
    )
    .await;
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_explicit_turns_single_user_message() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let turns = vec![Step::user_text(
        "What is the capital of France? Reply in one word.",
    )];

    let response = interaction_builder(&client)
        .with_history(turns)
        .create()
        .await
        .expect("Request failed");

    assert_eq!(response.status, InteractionStatus::Completed);
    let text = response.as_text().expect("Should have text response");
    assert_response_semantic(
        &client,
        "Asked for the capital of France in one word",
        text,
        "Does this response identify Paris as the capital of France?",
    )
    .await;
}
