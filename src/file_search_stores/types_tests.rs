use super::*;

/// Exact create/get payload observed live 2026-08-16.
fn store_wire() -> serde_json::Value {
    serde_json::json!({
        "name": "fileSearchStores/genairsauditprobe-4kws71n2ybpr",
        "displayName": "genai-rs-audit-probe",
        "createTime": "2026-08-16T15:13:13.783782Z",
        "updateTime": "2026-08-16T15:13:13.783782Z",
        "embeddingModel": "models/gemini-embedding-001"
    })
}

#[test]
fn store_deserializes_camel_case_wire() {
    let store: FileSearchStore = serde_json::from_value(store_wire()).unwrap();

    assert_eq!(
        store.name,
        "fileSearchStores/genairsauditprobe-4kws71n2ybpr"
    );
    assert_eq!(store.display_name.as_deref(), Some("genai-rs-audit-probe"));
    assert_eq!(
        store.embedding_model.as_deref(),
        Some("models/gemini-embedding-001")
    );
    assert!(store.create_time.is_some());
    assert!(store.extra.is_empty());
}

#[test]
fn store_roundtrips_exactly() {
    let wire = store_wire();
    let store: FileSearchStore = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&store).unwrap(), wire);
}

#[test]
fn store_preserves_unknown_fields() {
    let mut wire = store_wire();
    wire["futureField"] = serde_json::json!({"nested": true});

    let store: FileSearchStore = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(
        store.extra.get("futureField"),
        Some(&serde_json::json!({"nested": true}))
    );
    assert_eq!(serde_json::to_value(&store).unwrap(), wire);
}

#[test]
fn list_response_uses_file_search_stores_envelope() {
    let wire = serde_json::json!({"fileSearchStores": [store_wire()]});
    let list: FileSearchStoreListResponse = serde_json::from_value(wire).unwrap();

    assert_eq!(list.stores.len(), 1);
    assert!(list.next_page_token.is_none());
}

#[test]
fn empty_list_response_deserializes() {
    // The API returns a bare `{}` for an empty store list.
    let list: FileSearchStoreListResponse = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(list.stores.is_empty());
}

#[test]
fn list_responses_drop_only_the_undeserializable_entry() {
    // `name` is required on both element types; a number in its place
    // costs that element, not the page.
    let stores: FileSearchStoreListResponse = serde_json::from_value(serde_json::json!({
        "fileSearchStores": [store_wire(), {"name": 7}],
        "nextPageToken": "p2"
    }))
    .unwrap();
    assert_eq!(stores.stores.len(), 1);
    assert_eq!(stores.next_page_token.as_deref(), Some("p2"));

    let docs: FileSearchDocumentListResponse = serde_json::from_value(serde_json::json!({
        "documents": [{"name": 7}, document_wire()]
    }))
    .unwrap();
    assert_eq!(docs.documents.len(), 1);
}

#[test]
fn list_responses_treat_an_explicit_null_list_as_empty() {
    let stores: FileSearchStoreListResponse =
        serde_json::from_value(serde_json::json!({"fileSearchStores": null})).unwrap();
    assert!(stores.stores.is_empty());
    let docs: FileSearchDocumentListResponse =
        serde_json::from_value(serde_json::json!({"documents": null})).unwrap();
    assert!(docs.documents.is_empty());
}

/// Exact document payload observed live 2026-08-16.
fn document_wire() -> serde_json::Value {
    serde_json::json!({
        "name": "fileSearchStores/probe-lhba715kr8z5/documents/doc1-25rp7vz1euwz",
        "displayName": "doc1",
        "updateTime": "2026-08-16T15:53:10.494695Z",
        "createTime": "2026-08-16T15:53:10.494695Z",
        "state": "STATE_PENDING",
        "sizeBytes": "27",
        "mimeType": "text/plain"
    })
}

#[test]
fn document_deserializes_with_string_size_bytes() {
    let doc: FileSearchDocument = serde_json::from_value(document_wire()).unwrap();

    // sizeBytes arrives as a JSON string, not a number.
    assert_eq!(doc.size_bytes, Some(27));
    assert_eq!(doc.state, Some(DocumentState::Pending));
    assert_eq!(doc.mime_type.as_deref(), Some("text/plain"));
}

#[test]
fn document_state_wire_values_use_state_prefix() {
    assert_eq!(
        serde_json::to_value(DocumentState::Pending).unwrap(),
        "STATE_PENDING"
    );
    assert_eq!(
        serde_json::to_value(DocumentState::Active).unwrap(),
        "STATE_ACTIVE"
    );
    assert_eq!(
        serde_json::to_value(DocumentState::Failed).unwrap(),
        "STATE_FAILED"
    );
}

#[cfg(not(genai_strict_unknown))]
#[test]
fn document_state_unknown_is_preserved() {
    let state: DocumentState = serde_json::from_value(serde_json::json!("STATE_QUARANTINED"))
        .expect("unknown states must not fail deserialization");

    assert!(state.is_unknown());
    assert_eq!(state.unknown_state_type(), Some("STATE_QUARANTINED"));
    assert!(!state.is_active());
    assert_eq!(
        serde_json::to_value(&state).unwrap(),
        "STATE_QUARANTINED",
        "unknown states must round-trip to their original wire value"
    );
}

#[test]
fn document_state_is_active_only_for_active() {
    assert!(DocumentState::Active.is_active());
    assert!(!DocumentState::Pending.is_active());
    assert!(!DocumentState::Failed.is_active());
}

#[test]
fn document_preserves_unknown_fields() {
    let mut wire = document_wire();
    wire["chunkCount"] = serde_json::json!(3);

    let doc: FileSearchDocument = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(doc.extra.get("chunkCount"), Some(&serde_json::json!(3)));
    assert_eq!(serde_json::to_value(&doc).unwrap(), wire);
}

#[test]
fn create_request_omits_display_name_when_unset() {
    let request = CreateFileSearchStoreRequest::default();
    assert_eq!(
        serde_json::to_value(&request).unwrap(),
        serde_json::json!({})
    );
}

#[test]
fn create_request_serializes_camel_case() {
    let request = CreateFileSearchStoreRequest {
        display_name: Some("my-docs".to_string()),
        embedding_model: Some("models/gemini-embedding-2".to_string()),
        extra: serde_json::Map::new(),
    };
    assert_eq!(
        serde_json::to_value(&request).unwrap(),
        serde_json::json!({"displayName": "my-docs", "embeddingModel": "models/gemini-embedding-2"})
    );
}

/// `extra` rides `#[serde(flatten)]`, so its keys must land beside
/// `displayName` rather than nested under an `extra` object.
#[test]
fn create_request_flattens_extra_beside_modeled_fields() {
    let mut extra = serde_json::Map::new();
    extra.insert(
        "customChunkingConfig".to_string(),
        serde_json::json!({"maxTokensPerChunk": 200}),
    );
    let request = CreateFileSearchStoreRequest {
        display_name: Some("my-docs".to_string()),
        embedding_model: None,
        extra,
    };

    assert_eq!(
        serde_json::to_value(&request).unwrap(),
        serde_json::json!({
            "displayName": "my-docs",
            "customChunkingConfig": {"maxTokensPerChunk": 200}
        })
    );
}

/// The builders have to agree with the struct-literal form the tests
/// around them use. They exist for downstream crates, which cannot use
/// that form at all on a `#[non_exhaustive]` type, so nothing outside
/// the crate would notice them drifting.
///
/// Doctests cover `add_extra`'s serialization, but doctests run only in
/// CI — not under `make test` / `make test-all` — so this is what the
/// repo's own default test commands exercise.
#[test]
fn builders_match_the_struct_literal_form() {
    let mut extra = serde_json::Map::new();
    extra.insert("someNewField".to_string(), serde_json::json!(7));
    let literal = CreateFileSearchStoreRequest {
        display_name: Some("my-docs".to_string()),
        embedding_model: Some("models/gemini-embedding-001".to_string()),
        extra,
    };

    let built = CreateFileSearchStoreRequest::new()
        .with_display_name("my-docs")
        .with_embedding_model("models/gemini-embedding-001")
        .add_extra("someNewField", 7);

    assert_eq!(built, literal);
    assert_eq!(
        serde_json::to_value(&built).unwrap(),
        serde_json::to_value(&literal).unwrap()
    );
}

/// `add_extra` adds one keyed entry, as the `add_*` convention says
/// (docs/BUILDER_API.md): entries added earlier stay, and a repeated key
/// replaces its value.
#[test]
fn add_extra_adds_an_entry_and_replaces_a_repeated_key() {
    let request = CreateFileSearchStoreRequest::new()
        .add_extra("first", 1)
        .add_extra("second", 2)
        .add_extra("first", 3);

    assert_eq!(
        serde_json::to_value(&request).unwrap(),
        serde_json::json!({"first": 3, "second": 2})
    );
}

#[test]
fn new_matches_default() {
    assert_eq!(
        CreateFileSearchStoreRequest::new(),
        CreateFileSearchStoreRequest::default()
    );
}

/// The other half of the flatten contract: an unmodeled key on the way
/// in has to survive into `extra` rather than being dropped, or a
/// round-trip through this type would silently discard it.
#[test]
fn create_request_captures_unknown_fields_into_extra() {
    let wire = serde_json::json!({
        "displayName": "my-docs",
        "somethingNewUpstream": "value"
    });
    let request: CreateFileSearchStoreRequest = serde_json::from_value(wire.clone()).unwrap();

    assert_eq!(request.display_name.as_deref(), Some("my-docs"));
    assert_eq!(
        request.extra.get("somethingNewUpstream"),
        Some(&serde_json::json!("value"))
    );
    assert_eq!(serde_json::to_value(&request).unwrap(), wire);
}

/// A store read while a document was indexing, as served live 2026-11-05:
/// the counts and the size are string int64s, all modeled.
#[test]
fn store_reads_its_document_counts_and_size() {
    let wire = serde_json::json!({
        "name": "fileSearchStores/genairswp2bprobe-2295uacesjnm",
        "displayName": "genai-rs-wp2b-probe",
        "createTime": "2026-11-05T08:51:35.455005Z",
        "updateTime": "2026-11-05T08:51:35.455005Z",
        "activeDocumentsCount": "5",
        "pendingDocumentsCount": "1",
        "failedDocumentsCount": "0",
        "sizeBytes": "233",
        "embeddingModel": "models/gemini-embedding-001"
    });
    let store: FileSearchStore = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(store.active_documents_count, Some(5));
    assert_eq!(store.pending_documents_count, Some(1));
    assert_eq!(store.failed_documents_count, Some(0));
    assert_eq!(store.size_bytes, Some(233));
    assert!(store.extra.is_empty(), "{:?}", store.extra);
    assert_eq!(serde_json::to_value(&store).unwrap(), wire);

    // A plain number reads the same; an unparseable count is dropped, not
    // the store.
    let store: FileSearchStore = serde_json::from_value(serde_json::json!({
        "name": "fileSearchStores/a", "activeDocumentsCount": 3, "sizeBytes": "big"
    }))
    .unwrap();
    assert_eq!(store.active_documents_count, Some(3));
    assert_eq!(store.size_bytes, None);
}

/// A document uploaded with all three kinds of custom metadata, as served
/// live 2026-11-05.
#[test]
fn document_reads_its_custom_metadata() {
    let wire = serde_json::json!({
        "name": "fileSearchStores/s/documents/wp2bmeta-8g7anhcuqvsh",
        "displayName": "wp2b-meta",
        "customMetadata": [
            {"key": "team", "stringValue": "rust"},
            {"key": "year", "numericValue": 2026},
            {"key": "tags", "stringListValue": {"values": ["a", "b"]}}
        ],
        "state": "STATE_ACTIVE",
        "sizeBytes": "54",
        "mimeType": "text/plain"
    });
    let doc: FileSearchDocument = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(
        doc.custom_metadata,
        [
            CustomMetadata::string("team", "rust"),
            CustomMetadata::numeric("year", 2026.0),
            CustomMetadata::string_list("tags", ["a", "b"]),
        ]
    );
    assert!(doc.extra.is_empty(), "{:?}", doc.extra);
    // The whole number comes back as `2026.0`, the same JSON number.
    let back = serde_json::to_value(&doc).unwrap();
    assert_eq!(back["customMetadata"][1]["numericValue"], 2026.0);
    assert_eq!(back["customMetadata"][2], wire["customMetadata"][2]);
}

#[test]
fn custom_metadata_keeps_an_unknown_value_kind_and_drops_only_a_malformed_entry() {
    let doc: FileSearchDocument = serde_json::from_value(serde_json::json!({
        "name": "fileSearchStores/s/documents/d",
        "customMetadata": [
            {"key": "flag", "boolValue": true},
            "not an object",
            {"key": "tags", "stringListValue": {"values": ["a"], "sorted": false}}
        ]
    }))
    .unwrap();
    assert_eq!(doc.custom_metadata.len(), 2);
    assert_eq!(doc.custom_metadata[0].extra["boolValue"], true);
    let tags = doc.custom_metadata[1].string_list_value.as_ref().unwrap();
    assert_eq!(tags.values, ["a"]);
    assert_eq!(tags.extra["sorted"], false);
}

#[test]
fn custom_metadata_constructors_set_one_value() {
    assert_eq!(
        serde_json::to_value(CustomMetadata::string("team", "search")).unwrap(),
        serde_json::json!({"key": "team", "stringValue": "search"})
    );
    assert_eq!(
        serde_json::to_value(CustomMetadata::numeric("year", 2026.5)).unwrap(),
        serde_json::json!({"key": "year", "numericValue": 2026.5})
    );
    assert_eq!(
        serde_json::to_value(CustomMetadata::string_list(
            "tags",
            vec![String::from("a"), String::from("b")]
        ))
        .unwrap(),
        serde_json::json!({"key": "tags", "stringListValue": {"values": ["a", "b"]}})
    );
}

#[test]
fn chunking_config_serializes_the_white_space_limits() {
    let chunking = ChunkingConfig::white_space(200, 20);
    assert_eq!(
        chunking.white_space_config,
        Some(WhiteSpaceConfig::new(200, 20))
    );
    assert_eq!(
        serde_json::to_value(&chunking).unwrap(),
        serde_json::json!({"whiteSpaceConfig": {"maxTokensPerChunk": 200, "maxOverlapTokens": 20}})
    );
    assert_eq!(
        serde_json::to_value(ChunkingConfig::default()).unwrap(),
        serde_json::json!({})
    );
    let mut custom = ChunkingConfig::default();
    custom
        .extra
        .insert("semanticConfig".into(), serde_json::json!({}));
    assert_eq!(
        serde_json::to_value(&custom).unwrap(),
        serde_json::json!({"semanticConfig": {}})
    );
}

/// A white-space config field the crate does not model survives a round
/// trip (D-001), as on `ChunkingConfig`.
#[test]
fn white_space_config_keeps_unmodeled_fields() {
    let wire = serde_json::json!({
        "maxTokensPerChunk": 200,
        "maxOverlapTokens": 20,
        "minTokensPerChunk": 50
    });
    let config: WhiteSpaceConfig = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(config.max_tokens_per_chunk, Some(200));
    assert_eq!(config.extra["minTokensPerChunk"], 50);
    assert_eq!(serde_json::to_value(&config).unwrap(), wire);
    assert!(WhiteSpaceConfig::new(200, 20).extra.is_empty());
}
