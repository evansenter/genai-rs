use super::*;
use std::time::Duration;

#[test]
fn a_file_upload_converts_into_a_document_upload_with_no_options() {
    let document: DocumentUpload = FileUpload::from_bytes(b"x".to_vec(), "text/plain").into();
    assert_eq!(document.options, DocumentOptions::default());
    assert_eq!(
        serde_json::to_value(&document.options).unwrap(),
        serde_json::json!({})
    );
}

#[test]
fn document_upload_setters_add_metadata_and_replace_chunking() {
    let document = DocumentUpload::new(FileUpload::from_path("notes.md"))
        .add_custom_metadata(CustomMetadata::string("team", "search"))
        .add_custom_metadata(CustomMetadata::numeric("year", 2026.0))
        .with_chunking_config(ChunkingConfig::white_space(500, 50))
        .with_chunking_config(ChunkingConfig::white_space(200, 20));
    assert_eq!(
        serde_json::to_value(&document.options).unwrap(),
        serde_json::json!({
            "customMetadata": [
                {"key": "team", "stringValue": "search"},
                {"key": "year", "numericValue": 2026.0}
            ],
            "chunkingConfig": {"whiteSpaceConfig": {"maxTokensPerChunk": 200, "maxOverlapTokens": 20}}
        })
    );
}

#[test]
fn document_import_converts_from_a_name_or_a_file() {
    let file: FileMetadata = serde_json::from_value(serde_json::json!({
        "name": "files/abc", "mimeType": "text/plain", "uri": "u"
    }))
    .unwrap();
    for import in [
        DocumentImport::from("files/abc"),
        DocumentImport::from(String::from("files/abc")),
        DocumentImport::from(&file),
        DocumentImport::new(&file.name),
    ] {
        assert_eq!(import.file_name, "files/abc");
        assert_eq!(import.options, DocumentOptions::default());
        assert_eq!(import.poll, PollOptions::new());
    }
}

#[test]
fn document_import_setters() {
    let poll = PollOptions::new().with_timeout(Duration::from_secs(5));
    let import = DocumentImport::new("files/abc")
        .add_custom_metadata(CustomMetadata::string_list("tags", ["a", "b"]))
        .with_chunking_config(ChunkingConfig::white_space(100, 10))
        .with_poll_options(poll);
    assert_eq!(import.poll, poll);
    assert_eq!(
        import.options.custom_metadata,
        [CustomMetadata::string_list("tags", ["a", "b"])]
    );
    assert_eq!(
        import.options.chunking_config,
        Some(ChunkingConfig::white_space(100, 10))
    );
}
