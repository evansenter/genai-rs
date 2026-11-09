//! Unit tests for the builder's input: history, content and text composition,
//! and the `ConversationBuilder`.

use super::super::tests::create_test_client;
use super::*;

// --- Step Array Input Tests ---

#[test]
fn test_interaction_builder_with_history() {
    use crate::Step;

    let client = create_test_client();
    let steps = vec![
        Step::user_text("What is 2+2?"),
        Step::model_text("2+2 equals 4."),
        Step::user_text("And what's that times 3?"),
    ];

    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_history(steps);

    assert_eq!(builder.history.len(), 3);
}

#[test]
fn test_interaction_builder_build_with_history() {
    use crate::Step;

    let client = create_test_client();
    let steps = vec![
        Step::user_text("Hello"),
        Step::model_text("Hi there!"),
        Step::user_text("How are you?"),
    ];

    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_history(steps);

    let result = builder.build();
    assert!(result.is_ok());

    let request = result.unwrap();
    assert_eq!(request.model.as_deref(), Some(crate::DEFAULT_MODEL));
    assert!(matches!(
        request.input,
        Some(crate::InteractionInput::Steps(_))
    ));
}

#[test]
fn test_interaction_builder_with_single_step() {
    use crate::Step;

    let client = create_test_client();
    let steps = vec![Step::user_text("Hello")];

    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_history(steps);

    let result = builder.build();
    assert!(result.is_ok());
}

// --- History + Current Message Composition Tests ---

#[test]
fn test_with_history_then_with_text_composes_correctly() {
    use crate::{InteractionInput, Step};

    let client = create_test_client();
    let history = vec![Step::user_text("Hello"), Step::model_text("Hi there!")];

    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_history(history)
        .with_text("How are you?");

    // Build should compose history + current_message
    let request = builder.build().expect("Build should succeed");

    // Verify the input is Steps with 3 items
    match request.input.as_ref().expect("input") {
        InteractionInput::Steps(steps) => {
            assert_eq!(steps.len(), 3, "Should have 3 steps");
            assert!(matches!(steps[0], Step::UserInput { .. }));
            assert!(matches!(steps[1], Step::ModelOutput { .. }));
            assert!(matches!(steps[2], Step::UserInput { .. }));
        }
        _ => panic!("Expected Steps input"),
    }
}

#[test]
fn test_with_text_then_with_history_composes_correctly() {
    use crate::{InteractionInput, Step};

    let client = create_test_client();
    let history = vec![Step::user_text("Hello"), Step::model_text("Hi there!")];

    // Order reversed - should produce same result
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("How are you?")
        .with_history(history);

    let request = builder.build().expect("Build should succeed");

    // Verify the input is Steps with 3 items (history + current)
    match request.input.as_ref().expect("input") {
        InteractionInput::Steps(steps) => {
            assert_eq!(steps.len(), 3, "Should have 3 steps");
            assert!(matches!(steps[0], Step::UserInput { .. }));
            assert!(matches!(steps[1], Step::ModelOutput { .. }));
            assert!(matches!(steps[2], Step::UserInput { .. })); // Current message appended
        }
        _ => panic!("Expected Steps input"),
    }
}

#[test]
fn test_history_and_text_order_independent() {
    use crate::Step;

    let client = create_test_client();
    let history = vec![Step::user_text("First"), Step::model_text("Response")];

    // Build in one order
    let req1 = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_history(history.clone())
        .with_text("Current")
        .build()
        .expect("Build should succeed");

    // Build in reverse order
    let req2 = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Current")
        .with_history(history)
        .build()
        .expect("Build should succeed");

    // Both should produce equivalent requests
    let json1 = serde_json::to_string(&req1.input).unwrap();
    let json2 = serde_json::to_string(&req2.input).unwrap();
    assert_eq!(json1, json2, "Order should not affect result");
}

#[test]
fn test_conversation_builder_then_with_text() {
    use crate::{InteractionInput, Step};

    let client = create_test_client();

    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .conversation()
        .user("What is 2+2?")
        .model("4")
        .done()
        .with_text("And times 3?");

    let request = builder.build().expect("Build should succeed");

    // Should have 3 steps: original 2 + appended current message
    match request.input.as_ref().expect("input") {
        InteractionInput::Steps(steps) => {
            assert_eq!(steps.len(), 3, "Should have 3 steps");
            assert!(matches!(steps[0], Step::UserInput { .. }));
            assert!(matches!(steps[1], Step::ModelOutput { .. }));
            assert!(matches!(steps[2], Step::UserInput { .. }));
        }
        _ => panic!("Expected Steps input"),
    }
}

#[test]
fn test_with_text_only_produces_text_input() {
    use crate::InteractionInput;

    let client = create_test_client();

    let request = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Hello!")
        .build()
        .expect("Build should succeed");

    // Should produce Text input when no history
    assert!(
        matches!(request.input, Some(InteractionInput::Text(_))),
        "Expected Text input, got {:?}",
        request.input
    );
}

#[test]
fn test_with_history_only_produces_steps_input() {
    use crate::{InteractionInput, Step};

    let client = create_test_client();
    let history = vec![Step::user_text("Hello"), Step::model_text("Hi!")];

    let request = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_history(history)
        .build()
        .expect("Build should succeed");

    // Should produce Steps input
    match request.input.as_ref().expect("input") {
        InteractionInput::Steps(steps) => {
            assert_eq!(steps.len(), 2);
        }
        _ => panic!("Expected Steps input"),
    }
}

#[test]
fn test_chained_preserves_history_and_current_message() {
    use crate::Step;

    let client = create_test_client();
    let history = vec![Step::user_text("Hello"), Step::model_text("Hi!")];

    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_history(history)
        .with_text("Current message")
        .with_previous_interaction("prev-123");

    // Fields should be preserved when chaining methods
    assert_eq!(builder.history.len(), 2);
    assert_eq!(builder.current_message.as_deref(), Some("Current message"));
    assert_eq!(builder.previous_interaction_id.as_deref(), Some("prev-123"));
}

#[test]
fn test_store_disabled_preserves_history_and_current_message() {
    use crate::Step;

    let client = create_test_client();
    let history = vec![Step::user_text("Hello"), Step::model_text("Hi!")];

    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_history(history)
        .with_text("Current message")
        .with_store_disabled();

    // Fields should be preserved when setting store disabled
    assert_eq!(builder.history.len(), 2);
    assert_eq!(builder.current_message.as_deref(), Some("Current message"));
    assert_eq!(builder.store, Some(false));
}

#[test]
fn test_with_content_composes_after_history() {
    use crate::{Content, Step};

    let client = create_test_client();
    let history = vec![Step::user_text("Hello"), Step::model_text("Hi!")];
    let content = vec![Content::image_data("aGk=", "image/png")];

    let request = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_content(content.clone())
        .with_history(history.clone())
        .build()
        .expect("content after history is a conversation (#454)");

    let mut expected = history;
    expected.push(Step::user_input(content));
    assert_eq!(request.input, Some(InteractionInput::Steps(expected)));
}

#[test]
fn test_with_text_content_and_history_compose() {
    use crate::{Content, Step};

    let client = create_test_client();
    let history = vec![Step::user_text("Hello"), Step::model_text("Hi!")];
    let image = Content::image_data("aGk=", "image/png");

    let request = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_history(history.clone())
        .with_text("What is this?")
        .with_content(vec![image.clone()])
        .build()
        .unwrap();

    // The text leads this turn's content, as without history.
    let mut expected = history;
    expected.push(Step::user_input(vec![
        Content::text("What is this?"),
        image,
    ]));
    assert_eq!(request.input, Some(InteractionInput::Steps(expected)));
    let wire = serde_json::to_value(&request).unwrap();
    assert_eq!(wire["input"][2]["type"], "user_input");
    assert_eq!(wire["input"][2]["content"][0]["text"], "What is this?");
    assert_eq!(wire["input"][2]["content"][1]["type"], "image");
}

#[test]
fn test_interaction_builder_build_empty_content() {
    let client = create_test_client();
    let err = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_content(vec![])
        .build()
        .unwrap_err();
    assert!(
        matches!(&err, crate::GenaiError::InvalidInput(message) if message.contains("with_content()")),
        "{err:?}"
    );

    // Text fills an empty content list: it is the turn's one block.
    let request = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_content(vec![])
        .with_text("x")
        .build()
        .expect("with_text() gives the content a block");
    assert_eq!(
        request.input,
        Some(InteractionInput::Content(vec![crate::Content::text("x")]))
    );
}

#[test]
fn test_with_content_and_text_merge() {
    use crate::{Content, InteractionInput};

    let client = create_test_client();
    let image_content = Content::Image {
        data: Some("dGVzdA==".to_string()),
        uri: None,
        mime_type: Some("image/png".to_string()),
        resolution: None,
        extra: Default::default(),
    };

    // with_text() then with_content() - text should be prepended to content
    let request = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Describe this image")
        .with_content(vec![image_content.clone()])
        .build()
        .expect("Should succeed");

    match request.input.as_ref().expect("input") {
        InteractionInput::Content(items) => {
            assert_eq!(items.len(), 2, "Should have 2 items (text + image)");
            // Text should be first (prepended)
            assert!(
                matches!(&items[0], Content::Text { text: Some(t), .. } if t == "Describe this image"),
                "First item should be the text"
            );
            assert!(
                matches!(&items[1], Content::Image { .. }),
                "Second item should be the image"
            );
        }
        _ => panic!("Expected Content input"),
    }

    // with_content() then with_text() - should also work (order-independent)
    let request2 = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_content(vec![image_content])
        .with_text("Describe this image")
        .build()
        .expect("Should succeed");

    match request2.input.as_ref().expect("input") {
        InteractionInput::Content(items) => {
            assert_eq!(items.len(), 2, "Should have 2 items (text + image)");
            // Text should still be first (prepended at build time)
            assert!(
                matches!(&items[0], Content::Text { text: Some(t), .. } if t == "Describe this image"),
                "First item should be the text"
            );
        }
        _ => panic!("Expected Content input"),
    }

    // Verify order-independence: both orders produce identical serialized output
    let json1 = serde_json::to_string(&request.input).unwrap();
    let json2 = serde_json::to_string(&request2.input).unwrap();
    assert_eq!(json1, json2, "Builder order should not affect result");
}

#[test]
fn test_with_content_alone_works() {
    use crate::{Content, InteractionInput};

    let client = create_test_client();
    let content = vec![Content::Text {
        text: Some("test".to_string()),
        annotations: None,
        extra: Default::default(),
    }];

    let result = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_content(content)
        .build();

    assert!(result.is_ok());
    let request = result.unwrap();
    assert!(matches!(request.input, Some(InteractionInput::Content(_))));
}

#[test]
fn test_conversation_builder_fluent_api() {
    use crate::Step;

    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .conversation()
        .user("What is 2+2?")
        .model("2+2 equals 4.")
        .user("And what's that times 3?")
        .done();

    // Verify the history has correct length and step types
    assert_eq!(builder.history.len(), 3);
    assert!(matches!(builder.history[0], Step::UserInput { .. }));
    assert!(matches!(builder.history[1], Step::ModelOutput { .. }));
    assert!(matches!(builder.history[2], Step::UserInput { .. }));
}

#[test]
fn test_conversation_builder_debug_shows_its_steps_and_not_the_key() {
    let client = crate::Client::builder("secret-key-4417").build().unwrap();
    let conversation = client
        .interaction()
        .with_model("test-model")
        .conversation()
        .user("Hello there");
    let debug = format!("{conversation:?}");
    assert!(debug.starts_with("ConversationBuilder {"), "{debug}");
    assert!(debug.contains("Hello there"), "{debug}");
    assert!(debug.contains("test-model"), "{debug}");
    assert!(!debug.contains("secret-key-4417"), "{debug}");
}

#[test]
fn test_conversation_builder_with_parts_content() {
    use crate::{Content, Step, TurnContent};

    let client = create_test_client();
    let parts = vec![Content::Text {
        text: Some("What is in this image?".to_string()),
        annotations: None,
        extra: Default::default(),
    }];

    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .conversation()
        .user(TurnContent::Parts(parts))
        .done();

    // Verify the history has 1 user_input step wrapping the parts
    assert_eq!(builder.history.len(), 1);
    assert!(matches!(builder.history[0], Step::UserInput { .. }));
    let content = builder.history[0]
        .content()
        .expect("user_input step should expose content");
    assert_eq!(content.len(), 1);
    assert_eq!(content[0].as_text(), Some("What is in this image?"));
}

#[test]
fn test_conversation_builder_with_turn_method() {
    use crate::Role;

    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .conversation()
        .turn(Role::User, "Hello")
        .turn(Role::Model, "Hi!")
        .done();

    assert_eq!(builder.history.len(), 2);
}

#[test]
fn test_conversation_builder_empty() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .conversation()
        .done();

    // Empty conversation results in empty history
    assert!(builder.history.is_empty());
}

#[test]
fn test_conversation_builder_preserves_model() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .conversation()
        .user("Hello")
        .done();

    // Model should be preserved through conversation builder
    assert_eq!(builder.model.as_deref(), Some(crate::DEFAULT_MODEL));
}

#[test]
fn test_conversation_builder_preserves_system_instruction() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_system_instruction("Be helpful")
        .conversation()
        .user("Hello")
        .done();

    // System instruction should be preserved through conversation builder
    assert!(builder.system_instruction.is_some());
}
