use super::*;
use serde_json::json;

#[test]
fn the_string_form_round_trips() {
    let output: CodeExecutionOutput = serde_json::from_value(json!("42\n")).unwrap();
    assert_eq!(output, CodeExecutionOutput::Text("42\n".into()));
    assert_eq!(output.as_text(), Some("42\n"));
    assert_eq!(output.as_contents(), None);
    assert_eq!(serde_json::to_value(&output).unwrap(), json!("42\n"));
}

#[test]
fn the_list_form_round_trips() {
    let wire = json!([
        {"type": "text", "text": "a"},
        {"type": "image", "data": "iVBORw0KGgo=", "mime_type": "image/png"},
    ]);
    let output: CodeExecutionOutput = serde_json::from_value(wire.clone()).unwrap();
    let contents = output.as_contents().expect("the list form");
    assert_eq!(contents.len(), 2);
    assert_eq!(contents[0].as_text(), Some("a"));
    assert_eq!(output.as_text(), None);
    assert_eq!(serde_json::to_value(&output).unwrap(), wire);
}

/// Strict mode fails the block instead, and with it the step.
#[test]
fn an_unknown_block_in_the_list_is_kept() {
    let wire = json!([{"type": "hologram", "frames": 3}]);
    let read = serde_json::from_value::<CodeExecutionOutput>(wire.clone());
    #[cfg(genai_strict_unknown)]
    assert!(read.is_err());
    #[cfg(not(genai_strict_unknown))]
    {
        let output = read.unwrap();
        assert!(output.as_contents().unwrap()[0].is_unknown());
        assert_eq!(serde_json::to_value(&output).unwrap(), wire);
    }
}

#[test]
fn other_shapes_are_errors() {
    for wire in [json!(42), json!({"stdout": "x"}), json!(true)] {
        assert!(
            serde_json::from_value::<CodeExecutionOutput>(wire.clone()).is_err(),
            "{wire} should not read as an output"
        );
    }
}

#[test]
fn text_reads_either_form() {
    assert_eq!(CodeExecutionOutput::from("x").text(), "x");
    assert!(matches!(
        CodeExecutionOutput::from("x").text(),
        Cow::Borrowed("x")
    ));
    assert_eq!(CodeExecutionOutput::from(Vec::new()).text(), "");
    assert_eq!(
        CodeExecutionOutput::from(vec![Content::text("only")]).text(),
        "only"
    );
    let joined = CodeExecutionOutput::from(vec![
        Content::text("a"),
        Content::image_data("iVBORw0KGgo=", "image/png"),
        Content::text("b"),
        Content::text("c"),
    ]);
    assert_eq!(joined.text(), "abc");
    assert_eq!(joined.to_string(), "abc");
}

#[test]
fn constructors_pick_the_form() {
    assert_eq!(
        CodeExecutionOutput::from("a".to_string()),
        CodeExecutionOutput::Text("a".into())
    );
    assert_eq!(
        CodeExecutionOutput::from("a"),
        CodeExecutionOutput::Text("a".into())
    );
    assert_eq!(
        CodeExecutionOutput::from(vec![Content::text("a")]),
        CodeExecutionOutput::Contents(vec![Content::text("a")])
    );
    assert_eq!(
        CodeExecutionOutput::default(),
        CodeExecutionOutput::Text(String::new())
    );
}

#[test]
fn push_str_extends_either_form() {
    let mut text = CodeExecutionOutput::default();
    text.push_str("4");
    text.push_str("2");
    assert_eq!(text, CodeExecutionOutput::Text("42".into()));

    let mut list = CodeExecutionOutput::from(vec![Content::text("a")]);
    list.push_str("b");
    assert_eq!(
        list,
        CodeExecutionOutput::Contents(vec![Content::text("a"), Content::text("b")])
    );
}
