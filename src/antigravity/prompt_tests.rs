use super::*;
use serde_json::json;

#[test]
fn test_a_string_is_a_one_part_text_prompt() {
    let owned = String::from("hello");
    for prompt in [
        Prompt::from("hello"),
        Prompt::from(owned.clone()),
        Prompt::from(&owned),
        Prompt::from(Cow::Borrowed("hello")),
        Prompt::from(Box::<str>::from("hello")),
    ] {
        assert_eq!(
            serde_json::to_value(prompt.to_wire()).unwrap(),
            json!({"parts": [{"text": "hello"}]})
        );
    }
}

#[test]
fn test_prompt_parts_keep_their_order_on_the_wire() {
    let prompt = Prompt::new()
        .add_slash_command(SlashCommand::Plan)
        .add_text("describe it")
        .add_media(Media::new(b"abc".to_vec(), " Image/PNG ").with_description("a chart"));
    assert_eq!(prompt.len(), 3);
    assert_eq!(
        serde_json::to_value(prompt.to_wire()).unwrap(),
        json!({"parts": [
            {"slashCommand": {"name": "plan"}},
            {"text": "describe it"},
            {"media": {"mimeType": "image/png", "description": "a chart", "data": "YWJj"}},
        ]})
    );
}

#[test]
fn test_media_from_file_reads_bytes_and_infers_the_mime_type() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chart.png");
    std::fs::write(&path, b"png-bytes").unwrap();
    let media = Media::from_file(&path).unwrap();
    assert_eq!(media.data(), b"png-bytes");
    assert_eq!(media.mime_type(), "image/png");
    assert_eq!(media.description(), None);

    let unknown = dir.path().join("blob.zzz");
    std::fs::write(&unknown, b"x").unwrap();
    assert!(matches!(
        Media::from_file(&unknown),
        Err(AntigravityError::Config(_))
    ));
    let missing = dir.path().join("missing.png");
    let Err(AntigravityError::Io { message, source }) = Media::from_file(&missing) else {
        panic!("an unreadable file is an I/O error");
    };
    assert!(
        message.contains(&missing.display().to_string()),
        "the error names the path: {message}"
    );
    assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn test_an_empty_prompt_is_refused() {
    assert!(matches!(
        user_input(Prompt::new()),
        Err(AntigravityError::Config(_))
    ));
    assert!(user_input("").is_ok(), "an empty string is still one part");
}

#[test]
fn test_media_debug_omits_the_bytes() {
    let rendered = format!("{:?}", Media::new(vec![7; 1000], "image/png"));
    assert!(rendered.contains("<1000 bytes>"), "{rendered}");
}
