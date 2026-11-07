//! File search stores and their documents: upload, import and the waits.

use super::*;

#[tokio::test]
async fn file_search_store_endpoints_send_the_documented_requests() {
    let stub = ok_stub().await;
    let client = stub.client();
    let c = &client;
    assert_wire(
        &stub,
        vec![
            // camelCase body, unlike the Interactions-family resources.
            wire(
                "POST",
                "/v1beta/fileSearchStores",
                Some(json!({"displayName": "Docs", "embeddingModel": "models/text-embedding"})),
                async move {
                    let request = CreateFileSearchStoreRequest::new()
                        .with_display_name("Docs")
                        .with_embedding_model("models/text-embedding");
                    c.file_search_stores().create(&request).await
                },
            ),
            wire(
                "GET",
                "/v1beta/fileSearchStores/abc",
                None,
                c.file_search_stores().get(STORE),
            ),
            wire(
                "GET",
                "/v1beta/fileSearchStores",
                None,
                c.file_search_stores().list().send(),
            ),
            wire(
                "GET",
                "/v1beta/fileSearchStores?page_size=20&page_token=t",
                None,
                c.file_search_stores()
                    .list()
                    .with_page_size(20)
                    .with_page_token("t")
                    .send(),
            ),
            wire(
                "DELETE",
                "/v1beta/fileSearchStores/abc",
                None,
                c.file_search_stores().delete(STORE),
            ),
            wire(
                "DELETE",
                "/v1beta/fileSearchStores/abc?force=true",
                None,
                c.file_search_stores().force_delete(STORE),
            ),
            wire(
                "GET",
                "/v1beta/fileSearchStores/abc/documents?page_size=2",
                None,
                c.file_search_stores()
                    .documents()
                    .list(STORE)
                    .with_page_size(2)
                    .send(),
            ),
            wire(
                "GET",
                "/v1beta/fileSearchStores/abc/documents?page_token=d2",
                None,
                c.file_search_stores()
                    .documents()
                    .list(STORE)
                    .with_page_token("d2")
                    .send(),
            ),
            wire(
                "GET",
                "/v1beta/fileSearchStores/abc/documents/doc-1",
                None,
                c.file_search_stores().documents().get(DOC),
            ),
            wire(
                "DELETE",
                "/v1beta/fileSearchStores/abc/documents/doc-1",
                None,
                c.file_search_stores().documents().delete(DOC),
            ),
            wire(
                "DELETE",
                "/v1beta/fileSearchStores/abc/documents/doc-1?force=true",
                None,
                c.file_search_stores().documents().force_delete(DOC),
            ),
        ],
    )
    .await;
}

#[tokio::test]
async fn file_search_store_responses_are_camel_case_and_keep_extras() {
    let store = json!({
        "name": "fileSearchStores/abc",
        "displayName": "Docs",
        "createTime": "2026-08-16T15:13:13.783782Z",
        "updateTime": "2026-08-16T15:13:13.783782Z",
        "embeddingModel": "models/text-embedding",
        "activeDocumentsCount": "3",
        "archivedDocumentsCount": "1"
    });
    let stub = Stub::replying(vec![
        Reply::json(200, store.clone()),
        Reply::json(
            200,
            json!({"fileSearchStores": [store, {"name": "fileSearchStores/def"}], "nextPageToken": "p2"}),
        ),
    ])
    .await;
    let client = stub.client();

    let fetched = client.file_search_stores().get(STORE).await.unwrap();
    assert_eq!(fetched.name, "fileSearchStores/abc");
    assert_eq!(fetched.display_name.as_deref(), Some("Docs"));
    assert_eq!(
        fetched.embedding_model.as_deref(),
        Some("models/text-embedding")
    );
    assert!(fetched.create_time.is_some() && fetched.update_time.is_some());
    assert_eq!(fetched.active_documents_count, Some(3));
    assert_eq!(fetched.extra["archivedDocumentsCount"], "1");
    let back = serde_json::to_value(&fetched).unwrap();
    assert_eq!(back["displayName"], "Docs");
    assert_eq!(back["activeDocumentsCount"], "3");
    assert_eq!(back["archivedDocumentsCount"], "1");

    let list = client.file_search_stores().list().send().await.unwrap();
    let names: Vec<_> = list.stores.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["fileSearchStores/abc", "fileSearchStores/def"]);
    assert_eq!(list.next_page_token.as_deref(), Some("p2"));
}

#[cfg(not(genai_strict_unknown))]
#[tokio::test]
async fn document_list_parses_states_sizes_and_unknown_states() {
    let stub = Stub::replying(vec![Reply::json(
        200,
        json!({
            "documents": [
                {"name": DOC, "displayName": "a.txt", "state": "STATE_ACTIVE", "sizeBytes": "27",
                 "mimeType": "text/plain", "customMetadata": [{"key": "k", "stringValue": "v"}]},
                {"name": "fileSearchStores/abc/documents/doc-2", "state": "STATE_ARCHIVED"}
            ],
            "nextPageToken": "n"
        }),
    )])
    .await;

    let list = stub
        .client()
        .file_search_stores()
        .documents()
        .list(STORE)
        .send()
        .await
        .unwrap();

    let active = &list.documents[0];
    assert_eq!(active.state, Some(DocumentState::Active));
    assert_eq!(active.size_bytes, Some(27));
    assert_eq!(active.mime_type.as_deref(), Some("text/plain"));
    assert_eq!(active.custom_metadata, [CustomMetadata::string("k", "v")]);
    let archived = list.documents[1].state.as_ref().unwrap();
    assert_eq!(archived.unknown_state_type(), Some("STATE_ARCHIVED"));
    assert_eq!(serde_json::to_value(archived).unwrap(), "STATE_ARCHIVED");
    assert_eq!(list.next_page_token.as_deref(), Some("n"));
}

/// The live list shape at `page_size=1` (2026-09-27): oldest first,
/// unpadded URL-safe base64 tokens, and no token on the last page.
#[tokio::test]
async fn file_search_store_list_items_follow_every_page_with_the_page_size() {
    let store = |name: &str| json!({"name": name, "displayName": name});
    let stub = Stub::replying(vec![
        Reply::json(
            200,
            json!({
                "fileSearchStores": [store("fileSearchStores/one-0pk6lx0oxgg5")],
                "nextPageToken": "cjEKD4IBDAi07uHVBhColM-EAQoeQhxnZW5haXJzcHJvYmVjODEtMHBrNmx4MG94Z2c1"
            }),
        ),
        Reply::json(
            200,
            json!({"fileSearchStores": [store("fileSearchStores/two-avyyk8egynb4")]}),
        ),
    ])
    .await;

    let stores: Vec<genai_rs::FileSearchStore> = stub
        .client()
        .file_search_stores()
        .list()
        .with_page_size(1)
        .items()
        .try_collect()
        .await
        .unwrap();

    let names: Vec<_> = stores.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "fileSearchStores/one-0pk6lx0oxgg5",
            "fileSearchStores/two-avyyk8egynb4"
        ]
    );
    assert_eq!(
        targets(&stub),
        [
            "/v1beta/fileSearchStores?page_size=1",
            "/v1beta/fileSearchStores?page_size=1&page_token=cjEKD4IBDAi07uHVBhColM-EAQoeQhxnZW5haXJzcHJvYmVjODEtMHBrNmx4MG94Z2c1",
        ]
    );
}

fn documents_page(ids: &[&str], next: Option<&str>) -> Reply {
    let documents: Vec<Value> = ids
        .iter()
        .map(|id| json!({"name": format!("{STORE}/documents/{id}"), "state": "STATE_ACTIVE"}))
        .collect();
    let mut body = json!({"documents": documents});
    if let Some(next) = next {
        body["nextPageToken"] = json!(next);
    }
    Reply::json(200, body)
}

fn document_ids(documents: &[FileSearchDocument]) -> Vec<&str> {
    documents
        .iter()
        .filter_map(|d| d.name.rsplit_once("/documents/").map(|(_, id)| id))
        .collect()
}

#[tokio::test]
async fn file_search_document_items_resend_the_store_and_page_size_on_every_page() {
    let stub = Stub::replying(vec![
        documents_page(
            &["d1"],
            Some("cigKD4IBDAjO7uHVBhCQ0MCZAwoVQhN1cGxvYWQtMDh2emdrdmdhcmhq"),
        ),
        // An empty page with a token is followed.
        documents_page(&[], Some("p+3")),
        documents_page(&["d2"], None),
    ])
    .await;

    let documents: Vec<FileSearchDocument> = stub
        .client()
        .file_search_stores()
        .documents()
        .list(STORE)
        .with_page_size(1)
        .items()
        .try_collect()
        .await
        .unwrap();

    assert_eq!(document_ids(&documents), ["d1", "d2"]);
    assert_eq!(
        targets(&stub),
        [
            "/v1beta/fileSearchStores/abc/documents?page_size=1",
            "/v1beta/fileSearchStores/abc/documents?page_size=1&page_token=cigKD4IBDAjO7uHVBhCQ0MCZAwoVQhN1cGxvYWQtMDh2emdrdmdhcmhq",
            "/v1beta/fileSearchStores/abc/documents?page_size=1&page_token=p%2B3",
        ]
    );
}

#[tokio::test]
async fn file_search_document_pages_resume_from_with_page_token() {
    let stub = Stub::replying(vec![
        documents_page(&["d3"], Some("p3")),
        documents_page(&["d4"], None),
    ])
    .await;

    let pages: Vec<_> = stub
        .client()
        .file_search_stores()
        .documents()
        .list(STORE)
        .with_page_token("p2")
        .pages()
        .try_collect()
        .await
        .unwrap();

    assert_eq!(pages.len(), 2);
    assert_eq!(document_ids(&pages[0].documents), ["d3"]);
    assert_eq!(
        targets(&stub),
        [
            "/v1beta/fileSearchStores/abc/documents?page_token=p2",
            "/v1beta/fileSearchStores/abc/documents?page_token=p3"
        ]
    );
}

#[tokio::test]
async fn file_search_document_items_for_a_malformed_store_name_yield_only_the_error() {
    let stub = Stub::replying(vec![]).await;

    let results: Vec<_> = stub
        .client()
        .file_search_stores()
        .documents()
        .list("abc")
        .items()
        .collect()
        .await;

    assert_eq!(results.len(), 1, "{results:?}");
    assert!(
        matches!(&results[0], Err(GenaiError::InvalidInput(m)) if m.contains("fileSearchStores/<id>")),
        "{:?}",
        results[0]
    );
    assert!(stub.requests().is_empty());
}

/// The answer to a resumable upload's start request: the session to send
/// the file to.
pub(super) fn upload_session(url: &str) -> Reply {
    Reply::json(200, json!({})).header("x-goog-upload-url", url)
}

/// The API's answer to the finalizing request: an operation naming the
/// created document.
pub(super) fn upload_operation() -> Reply {
    Reply::json(
        200,
        json!({
            "name": "fileSearchStores/abc/upload/operations/op-1",
            "response": {"documentName": DOC, "mimeType": "text/plain", "sizeBytes": "12"}
        }),
    )
}

/// The three requests of a document upload, checked for the parts every
/// upload shares: a resumable start at the store's upload URL, the file sent
/// to the session, and the document read back.
fn assert_upload_requests(requests: Vec<Recorded>, size: usize) -> (Recorded, Recorded) {
    let [start, finish, read_back] = requests.try_into().unwrap();
    assert_eq!(start.method, "POST");
    assert_eq!(
        start.target,
        "/upload/v1beta/fileSearchStores/abc:uploadToFileSearchStore"
    );
    assert_eq!(start.header("x-goog-api-key"), Some("test-key"));
    assert_eq!(start.header("api-revision"), Some(API_REVISION));
    assert_eq!(start.header("x-goog-upload-protocol"), Some("resumable"));
    assert_eq!(start.header("x-goog-upload-command"), Some("start"));
    assert_eq!(
        start.header("x-goog-upload-header-content-length"),
        Some(size.to_string().as_str())
    );
    assert_eq!(finish.method, "POST");
    assert_eq!(finish.target, "/upload-session/doc");
    assert_eq!(
        finish.header("x-goog-upload-command"),
        Some("upload, finalize")
    );
    assert_eq!(finish.header("x-goog-upload-offset"), Some("0"));
    assert_eq!(
        finish.header("content-length"),
        Some(size.to_string().as_str())
    );
    assert_eq!(read_back.method, "GET");
    assert_eq!(
        read_back.target,
        "/v1beta/fileSearchStores/abc/documents/doc-1"
    );
    (start, finish)
}

#[tokio::test]
async fn file_search_store_path_upload_starts_a_resumable_session_then_reads_the_document_back() {
    let stub = Stub::replying(vec![
        upload_session("{base}/upload-session/doc"),
        upload_operation(),
        document(Some("STATE_PENDING")),
    ])
    .await;
    let (_dir, path) = temp_file("notes.txt", b"hello search");

    let doc = stub
        .client()
        .file_search_stores()
        .upload(
            STORE,
            FileUpload::from_path(&path).with_display_name("My Doc"),
        )
        .await
        .unwrap();
    assert_eq!(doc.name, DOC);
    assert_eq!(doc.state, Some(DocumentState::Pending));

    let (start, finish) = assert_upload_requests(stub.requests(), 12);
    assert_eq!(
        start.json(),
        json!({"displayName": "My Doc", "mimeType": "text/plain"})
    );
    assert_eq!(
        start.header("x-goog-upload-header-content-type"),
        Some("text/plain")
    );
    // Without a display name the API would name the document after this
    // header (live 2026-11-05), so a path upload sends its file name.
    assert_eq!(start.header("x-goog-upload-file-name"), Some("notes.txt"));
    // The body is streamed from disk, so the length comes from the file.
    assert_eq!(finish.body, b"hello search");
}

#[tokio::test]
async fn file_search_store_path_upload_with_a_mime_type_and_no_display_name_sends_its_file_name() {
    let stub = Stub::replying(vec![
        upload_session("{base}/upload-session/doc"),
        upload_operation(),
        document(None),
    ])
    .await;
    let (_dir, path) = temp_file("report.data", b"# Report");

    stub.client()
        .file_search_stores()
        .upload(
            STORE,
            FileUpload::from_path(&path).with_mime_type("text/markdown"),
        )
        .await
        .unwrap();

    let (start, _) = assert_upload_requests(stub.requests(), 8);
    assert_eq!(start.json(), json!({"mimeType": "text/markdown"}));
    assert_eq!(
        start.header("x-goog-upload-header-content-type"),
        Some("text/markdown")
    );
    assert_eq!(start.header("x-goog-upload-file-name"), Some("report.data"));
}

/// An in-memory upload sends no file-name header: the API would take it as
/// the display name.
#[tokio::test]
async fn file_search_store_bytes_upload_sends_the_data_and_display_name_without_a_file_name() {
    let stub = Stub::replying(vec![
        upload_session("{base}/upload-session/doc"),
        upload_operation(),
        document(Some("STATE_PENDING")),
    ])
    .await;

    let doc = stub
        .client()
        .file_search_stores()
        .upload(
            STORE,
            FileUpload::from_bytes(b"in memory".to_vec(), "text/csv").with_display_name("Q4 memo"),
        )
        .await
        .unwrap();
    assert_eq!(doc.name, DOC);

    let (start, finish) = assert_upload_requests(stub.requests(), 9);
    assert_eq!(
        start.json(),
        json!({"displayName": "Q4 memo", "mimeType": "text/csv"})
    );
    assert_eq!(start.header("x-goog-upload-file-name"), None);
    assert_eq!(finish.body, b"in memory");
}

#[tokio::test]
async fn file_search_store_bytes_upload_without_a_display_name_sends_no_name_at_all() {
    let stub = Stub::replying(vec![
        upload_session("{base}/upload-session/doc"),
        upload_operation(),
        document(None),
    ])
    .await;

    stub.client()
        .file_search_stores()
        .upload(
            STORE,
            FileUpload::from_bytes(b"unnamed".to_vec(), "text/plain"),
        )
        .await
        .unwrap();

    let (start, finish) = assert_upload_requests(stub.requests(), 7);
    assert_eq!(start.json(), json!({"mimeType": "text/plain"}));
    assert_eq!(start.header("x-goog-upload-file-name"), None);
    assert_eq!(finish.body, b"unnamed");
}

/// Custom metadata and chunking ride the start request's JSON, camelCase.
#[tokio::test]
async fn file_search_store_upload_sends_custom_metadata_and_chunking() {
    let stub = Stub::replying(vec![
        upload_session("{base}/upload-session/doc"),
        upload_operation(),
        document(None),
    ])
    .await;

    stub.client()
        .file_search_stores()
        .upload(
            STORE,
            DocumentUpload::new(FileUpload::from_bytes(b"memo".to_vec(), "text/plain"))
                .add_custom_metadata(CustomMetadata::string("team", "search"))
                .add_custom_metadata(CustomMetadata::numeric("year", 2026.0))
                .add_custom_metadata(CustomMetadata::string_list("tags", ["a", "b"]))
                .with_chunking_config(ChunkingConfig::white_space(200, 20)),
        )
        .await
        .unwrap();

    let (start, _) = assert_upload_requests(stub.requests(), 4);
    assert_eq!(
        start.json(),
        json!({
            "mimeType": "text/plain",
            "customMetadata": [
                {"key": "team", "stringValue": "search"},
                {"key": "year", "numericValue": 2026.0},
                {"key": "tags", "stringListValue": {"values": ["a", "b"]}}
            ],
            "chunkingConfig": {"whiteSpaceConfig": {"maxTokensPerChunk": 200, "maxOverlapTokens": 20}}
        })
    );
}

#[tokio::test]
async fn file_search_store_upload_unresolved_operation_is_malformed_response() {
    let stub = Stub::replying(vec![
        upload_session("{base}/upload-session/doc"),
        Reply::json(
            200,
            json!({"name": "fileSearchStores/abc/upload/operations/op-7", "done": false}),
        ),
    ])
    .await;
    let (_dir, path) = temp_file("notes.txt", b"hello");

    let err = stub
        .client()
        .file_search_stores()
        .upload(STORE, FileUpload::from_path(&path))
        .await
        .unwrap_err();

    assert!(matches!(err, GenaiError::MalformedResponse(_)), "{err:?}");
    assert!(
        err.to_string().contains("operations/op-7"),
        "the operation name is the caller's only handle: {err}"
    );
    assert_eq!(stub.requests().len(), 2, "nothing to read back");
}

#[tokio::test]
async fn file_search_store_upload_without_a_session_url_is_malformed_response() {
    let stub = Stub::replying(vec![Reply::json(200, json!({}))]).await;

    let err = stub
        .client()
        .file_search_stores()
        .upload(STORE, FileUpload::from_bytes(b"x".to_vec(), "text/plain"))
        .await
        .unwrap_err();

    assert!(matches!(err, GenaiError::MalformedResponse(_)), "{err:?}");
    assert!(err.to_string().contains("x-goog-upload-url"), "{err}");
    assert_eq!(stub.requests().len(), 1);
}

#[tokio::test]
async fn file_search_store_upload_read_back_failure_names_the_document() {
    let unavailable = || {
        Reply::json(
            503,
            json!({"error": {"code": 503, "message": "Backend unavailable", "status": "UNAVAILABLE"}}),
        )
    };
    // (read-back reply, expected status or None for a non-API error, retryable)
    let cases: Vec<(Reply, Option<u16>, bool)> = vec![
        (unavailable(), Some(503), true),
        (not_found(), Some(404), false),
        (Reply::text(200, "<html>captive portal</html>"), None, false),
    ];
    let (_dir, path) = temp_file("notes.txt", b"hello");

    for (read_back, status, retryable) in cases {
        let stub = Stub::replying(vec![
            upload_session("{base}/upload-session/doc"),
            upload_operation(),
            read_back,
        ])
        .await;
        let err = stub
            .client()
            .file_search_stores()
            .upload(STORE, FileUpload::from_path(&path))
            .await
            .unwrap_err();

        match (&err, status) {
            (GenaiError::Api { status_code, .. }, Some(expected)) => {
                assert_eq!(*status_code, expected, "{err:?}");
            }
            (GenaiError::MalformedResponse(_), None) => {}
            _ => panic!("unexpected error shape for {status:?}: {err:?}"),
        }
        let message = err.to_string();
        assert!(
            message.contains(&format!("Upload succeeded and created '{DOC}'")),
            "{message}"
        );
        assert_eq!(err.is_retryable(), retryable, "{err:?}");
    }
}

/// The import answers with an unfinished operation and no document; `GET`
/// on the operation reports it done with the document's bare ID (the shapes
/// observed live 2026-11-05).
fn import_operation(done: Option<bool>, document_id: Option<&str>) -> Reply {
    let mut body = json!({
        "name": "fileSearchStores/abc/operations/op-1",
        "response": {"@type": "type.googleapis.com/google.ai.generativelanguage.v1main.ImportFileResponse"}
    });
    if let Some(done) = done {
        body["done"] = json!(done);
    }
    if let Some(id) = document_id {
        body["response"]["documentName"] = json!(id);
        body["response"]["parent"] = json!("abc");
    }
    Reply::json(200, body)
}

/// An import with every option set, polling fast so the test is quick.
fn full_import() -> DocumentImport {
    DocumentImport::new("files/f1")
        .add_custom_metadata(CustomMetadata::string("origin", "files-api"))
        .with_chunking_config(ChunkingConfig::white_space(100, 10))
        .with_poll_options(PollOptions::new().with_poll_interval(Duration::from_millis(1)))
}

#[tokio::test]
async fn import_file_posts_the_file_polls_the_operation_and_reads_the_document_back() {
    let stub = Stub::replying(vec![
        import_operation(None, None),
        import_operation(Some(false), None),
        import_operation(Some(true), Some("doc-1")),
        document(Some("STATE_ACTIVE")),
    ])
    .await;

    let doc = stub
        .client()
        .file_search_stores()
        .import_file(STORE, full_import())
        .await
        .unwrap();
    assert_eq!(doc.name, DOC);

    let [import, poll_1, poll_2, read_back] = stub.requests().try_into().unwrap();
    assert_eq!(import.method, "POST");
    assert_eq!(import.target, "/v1beta/fileSearchStores/abc:importFile");
    assert_eq!(import.header("x-goog-api-key"), Some("test-key"));
    assert_eq!(
        import.json(),
        json!({
            "fileName": "files/f1",
            "customMetadata": [{"key": "origin", "stringValue": "files-api"}],
            "chunkingConfig": {"whiteSpaceConfig": {"maxTokensPerChunk": 100, "maxOverlapTokens": 10}}
        })
    );
    for poll in [poll_1, poll_2] {
        assert_eq!(poll.method, "GET");
        assert_eq!(poll.target, "/v1beta/fileSearchStores/abc/operations/op-1");
    }
    assert_eq!(
        read_back.target,
        "/v1beta/fileSearchStores/abc/documents/doc-1"
    );
}

/// A file's metadata imports by its name; an operation that already names
/// its document (in full) needs no poll.
#[tokio::test]
async fn import_file_takes_file_metadata_and_skips_the_poll_when_done() {
    let stub = Stub::replying(vec![
        import_operation(Some(true), Some(DOC)),
        document(None),
    ])
    .await;
    let file: FileMetadata = serde_json::from_value(file("ACTIVE")).unwrap();

    stub.client()
        .file_search_stores()
        .import_file(STORE, &file)
        .await
        .unwrap();

    let [import, read_back] = stub.requests().try_into().unwrap();
    assert_eq!(import.json(), json!({"fileName": "files/abc"}));
    assert_eq!(
        read_back.target,
        "/v1beta/fileSearchStores/abc/documents/doc-1"
    );
}

#[tokio::test]
async fn import_file_reports_a_failed_or_unnamed_operation() {
    let failed = Reply::json(
        200,
        json!({"name": "fileSearchStores/abc/operations/op-1", "done": true,
               "error": {"code": 5, "message": "File not found."}}),
    );
    let stub = Stub::replying(vec![import_operation(None, None), failed]).await;
    let err = stub
        .client()
        .file_search_stores()
        .import_file(STORE, full_import())
        .await
        .unwrap_err();
    assert!(matches!(err, GenaiError::ProcessingFailed(_)), "{err:?}");
    let message = err.to_string();
    assert!(message.contains("files/f1"), "{message}");
    assert!(message.contains("error 5: File not found."), "{message}");

    let stub = Stub::replying(vec![import_operation(Some(true), None)]).await;
    let err = stub
        .client()
        .file_search_stores()
        .import_file(STORE, "files/f1")
        .await
        .unwrap_err();
    assert!(matches!(err, GenaiError::MalformedResponse(_)), "{err:?}");
    assert!(err.to_string().contains("operations/op-1"), "{err}");
}

/// An unfinished operation is polled by its name, which must belong to the
/// store: anything else is not followed.
#[tokio::test]
async fn import_file_does_not_follow_an_operation_outside_the_store() {
    let stub = Stub::replying(vec![Reply::json(
        200,
        json!({"name": "fileSearchStores/other/operations/op-1"}),
    )])
    .await;

    let err = stub
        .client()
        .file_search_stores()
        .import_file(STORE, "files/f1")
        .await
        .unwrap_err();

    assert!(matches!(err, GenaiError::MalformedResponse(_)), "{err:?}");
    assert_eq!(stub.requests().len(), 1);
}

/// On the paused clock, with each request holding it while in flight
/// (#501): the import, then polls every 10 ms (`PollOptions`' floor, D-047)
/// until the 100 ms deadline.
#[tokio::test(start_paused = true)]
async fn import_file_times_out_on_an_operation_that_never_finishes() {
    let stub = Stub::start(|_, _| import_operation(Some(false), None)).await;

    let err = stub
        .paused_clock_client()
        .file_search_stores()
        .import_file(
            STORE,
            DocumentImport::new("files/f1").with_poll_options(
                PollOptions::new()
                    .with_poll_interval(Duration::from_millis(10))
                    .with_timeout(Duration::from_millis(100)),
            ),
        )
        .await
        .unwrap_err();

    assert!(matches!(err, GenaiError::PollTimeout { .. }), "{err:?}");
    let message = err.to_string();
    assert!(message.contains("Timeout"), "{message}");
    // The import goes on, and a second one makes a second document: the
    // error hands over the operation and the document, and does not say to
    // try again.
    assert!(
        message.contains("operation 'fileSearchStores/abc/operations/op-1'"),
        "{message}"
    );
    assert!(
        message.contains("document 'fileSearchStores/abc/documents/op-1'"),
        "{message}"
    );
    assert!(message.contains("second document"), "{message}");
    assert!(!message.contains("try again"), "{message}");
    // Virtual time: the import and polls at 0, 10, ..., 90 ms; the one at
    // 100 would start at the deadline.
    assert!(message.contains("waited 90ms"), "{message}");
    assert_eq!(stub.requests().len(), 11, "{message}");
}

/// A non-retryable error from the operation poll keeps its status and
/// names the operation and the document the import creates.
#[tokio::test]
async fn import_file_poll_failure_names_the_operation_and_document() {
    let stub = Stub::replying(vec![
        import_operation(None, None),
        Reply::json(
            403,
            json!({"error": {"code": 403, "message": "Permission denied", "status": "PERMISSION_DENIED"}}),
        ),
    ])
    .await;

    let err = stub
        .client()
        .file_search_stores()
        .import_file(STORE, full_import())
        .await
        .unwrap_err();

    let GenaiError::Api { status_code, .. } = &err else {
        panic!("{err:?}")
    };
    assert_eq!(*status_code, 403);
    let message = err.to_string();
    for expected in [
        "operation 'fileSearchStores/abc/operations/op-1'",
        "document 'fileSearchStores/abc/documents/op-1'",
        "Permission denied",
    ] {
        assert!(message.contains(expected), "{expected}: {message}");
    }
    assert!(!err.is_retryable(), "{err:?}");
}
#[tokio::test]
async fn file_search_store_upload_rejects_bad_input_before_any_request() {
    let stub = Stub::replying(vec![]).await;
    let client = stub.client();
    let stores = client.file_search_stores();
    let (dir, text) = temp_file("notes.txt", b"hello");
    let empty = dir.path().join("empty.txt");
    std::fs::write(&empty, b"").unwrap();
    let unknown_ext = dir.path().join("blob.zzz");
    std::fs::write(&unknown_ext, b"x").unwrap();
    let missing = dir.path().join("missing.txt");

    let cases: Vec<(&str, Call<'_>)> = vec![
        (
            "empty file",
            call(stores.upload(STORE, FileUpload::from_path(&empty))),
        ),
        (
            "missing file",
            call(stores.upload(STORE, FileUpload::from_path(&missing))),
        ),
        (
            "unknown extension",
            call(stores.upload(STORE, FileUpload::from_path(&unknown_ext))),
        ),
        (
            "unheaderable mime type",
            call(stores.upload(
                STORE,
                FileUpload::from_path(&text).with_mime_type("text/plain\nX-Injected: 1"),
            )),
        ),
        (
            "empty bytes",
            call(stores.upload(STORE, FileUpload::from_bytes(Vec::new(), "text/plain"))),
        ),
        (
            "unheaderable mime type for bytes",
            call(stores.upload(
                STORE,
                FileUpload::from_bytes(b"x".to_vec(), "text/plain\r\nX-Injected: 1"),
            )),
        ),
        (
            "bare store id",
            call(stores.upload("abc", FileUpload::from_path(&text))),
        ),
        (
            "bare store id for bytes",
            call(stores.upload("abc", FileUpload::from_bytes(b"x".to_vec(), "text/plain"))),
        ),
        (
            "bare store id for an import",
            call(stores.import_file("abc", "files/f1")),
        ),
        // The API answers both with a 500 (live 2026-11-05).
        (
            "chunk overlap equal to the chunk",
            call(
                stores.upload(
                    STORE,
                    DocumentUpload::new(FileUpload::from_path(&text))
                        .with_chunking_config(ChunkingConfig::white_space(100, 100)),
                ),
            ),
        ),
        (
            "chunk overlap larger than the chunk, for an import",
            call(
                stores.import_file(
                    STORE,
                    DocumentImport::new("files/f1")
                        .with_chunking_config(ChunkingConfig::white_space(100, 150)),
                ),
            ),
        ),
    ];
    for (label, call) in cases {
        let err = call.await.unwrap_err();
        assert!(
            matches!(err, GenaiError::InvalidInput(_)),
            "{label}: {err:?}"
        );
        if label.starts_with("chunk overlap") {
            assert!(err.to_string().contains("max_overlap_tokens"), "{err}");
        }
    }
    assert!(stub.requests().is_empty());
}

async fn wait_for_document(
    replies: Vec<Reply>,
    timeout: Duration,
) -> (Result<FileSearchDocument, GenaiError>, Vec<String>) {
    let stub = Stub::replying(replies).await;
    let poll = PollOptions::new()
        .with_timeout(timeout)
        .with_poll_interval(Duration::from_millis(5));
    let result = stub
        .client()
        .file_search_stores()
        .documents()
        .wait_until_active(DOC, poll)
        .await;
    let targets = stub.requests().into_iter().map(|r| r.target).collect();
    (result, targets)
}

#[tokio::test]
async fn document_wait_until_active_polls_until_active() {
    let (result, targets) = wait_for_document(
        vec![
            document(Some("STATE_PENDING")),
            document(Some("STATE_ACTIVE")),
        ],
        Duration::from_secs(5),
    )
    .await;

    assert_eq!(result.unwrap().state, Some(DocumentState::Active));
    assert_eq!(
        targets,
        [
            "/v1beta/fileSearchStores/abc/documents/doc-1",
            "/v1beta/fileSearchStores/abc/documents/doc-1"
        ]
    );
}

#[tokio::test]
async fn document_wait_until_active_failed_state_is_terminal() {
    let (result, targets) =
        wait_for_document(vec![document(Some("STATE_FAILED"))], Duration::from_secs(5)).await;

    let err = result.unwrap_err();
    assert!(matches!(err, GenaiError::ProcessingFailed(_)), "{err:?}");
    assert!(!err.is_retryable(), "a failed document never recovers");
    assert!(err.to_string().contains(DOC), "{err}");
    assert_eq!(targets.len(), 1, "no polling past a terminal state");
}

/// On the paused clock, with each request holding it while in flight: polls
/// at 0, 10, ..., 50 ms, and the one at 60 would start at the deadline.
#[tokio::test(start_paused = true)]
async fn document_wait_until_active_times_out_with_the_last_state() {
    let stub = Stub::start(|_, _| document(Some("STATE_PENDING"))).await;
    let poll = PollOptions::new()
        .with_timeout(Duration::from_millis(60))
        .with_poll_interval(Duration::from_millis(10));

    let err = stub
        .paused_clock_client()
        .file_search_stores()
        .documents()
        .wait_until_active(DOC, poll)
        .await
        .unwrap_err();

    assert!(matches!(err, GenaiError::PollTimeout { .. }), "{err:?}");
    let message = err.to_string();
    assert!(message.contains("Timeout"), "{message}");
    assert!(message.contains("waited 50ms"), "{message}");
    assert!(message.contains("STATE_PENDING"), "{message}");
    assert_eq!(stub.requests().len(), 6, "{message}");
}

#[cfg(not(genai_strict_unknown))]
#[tokio::test]
async fn document_wait_until_active_keeps_polling_through_unknown_and_missing_states() {
    let (result, targets) = wait_for_document(
        vec![
            document(Some("STATE_REINDEXING")),
            document(None),
            document(Some("STATE_ACTIVE")),
        ],
        Duration::from_secs(5),
    )
    .await;

    assert_eq!(result.unwrap().state, Some(DocumentState::Active));
    assert_eq!(targets.len(), 3);
}

#[tokio::test]
async fn document_wait_until_active_propagates_api_errors() {
    let (result, targets) = wait_for_document(
        vec![document(Some("STATE_PENDING")), not_found()],
        Duration::from_secs(5),
    )
    .await;

    let err = result.unwrap_err();
    assert!(
        matches!(
            err,
            GenaiError::Api {
                status_code: 404,
                ..
            }
        ),
        "{err:?}"
    );
    assert_eq!(targets.len(), 2);
}
