//! Environments and their files.

use super::*;

#[tokio::test]
async fn environment_endpoints_send_the_documented_requests() {
    let stub = ok_stub().await;
    let client = stub.client();
    let c = &client;
    assert_wire(
        &stub,
        vec![
            wire(
                "POST",
                "/v1beta/environments",
                Some(json!({
                    "sources": [
                        {"type": "inline", "target": "/etc/motd", "content": "hi"},
                        {"type": "repository", "source": "https://github.com/o/r", "target": "/src"}
                    ],
                    "network": {"allowlist": [{"domain": "pypi.org"}]}
                })),
                async move {
                    let request = CreateEnvironmentRequest::new()
                        .add_source(EnvironmentSource::inline("/etc/motd", "hi"))
                        .add_source(EnvironmentSource::repository(
                            "https://github.com/o/r",
                            "/src",
                        ))
                        .with_network(NetworkConfig::allowlist(vec![AllowlistEntry::new(
                            "pypi.org",
                        )]));
                    c.environments().create(&request).await
                },
            ),
            // Fork: the whole body is the source environment.
            wire(
                "POST",
                "/v1beta/environments",
                Some(json!({"from_environment": "env-1"})),
                async move {
                    c.environments()
                        .create(&CreateEnvironmentRequest::from_environment("env-1"))
                        .await
                },
            ),
            wire(
                "POST",
                "/v1beta/environments",
                Some(json!({"network": "disabled"})),
                async move {
                    c.environments()
                        .create(
                            &CreateEnvironmentRequest::new().with_network(NetworkConfig::Disabled),
                        )
                        .await
                },
            ),
            wire(
                "GET",
                "/v1beta/environments/env-1",
                None,
                c.environments().get("env-1"),
            ),
            wire(
                "GET",
                "/v1beta/environments",
                None,
                c.environments().list().send(),
            ),
            wire(
                "GET",
                "/v1beta/environments?page_size=50&page_token=p%2B2",
                None,
                c.environments()
                    .list()
                    .with_page_size(50)
                    .with_page_token("p+2")
                    .send(),
            ),
            wire(
                "DELETE",
                "/v1beta/environments/env-1",
                None,
                c.environments().delete("env-1"),
            ),
            wire(
                "GET",
                "/v1beta/environments/env-1/files/",
                None,
                c.environments().files().list("env-1", "").send(),
            ),
            wire(
                "GET",
                "/v1beta/environments/env-1/files/src/",
                None,
                c.environments().files().list("env-1", "src/").send(),
            ),
            // `with_recursive(false)` sends nothing, like the default.
            wire(
                "GET",
                "/v1beta/environments/env-1/files/src",
                None,
                c.environments()
                    .files()
                    .list("env-1", "src")
                    .with_recursive(false)
                    .send(),
            ),
            wire(
                "GET",
                "/v1beta/environments/env-1/files/src/my%20file.py?page_size=5&page_token=t&recursive=true",
                None,
                c.environments()
                    .files()
                    .list("env-1", "/src/my file.py")
                    .with_recursive(true)
                    .with_page_size(5)
                    .with_page_token("t")
                    .send(),
            ),
        ],
    )
    .await;
}

#[cfg(not(genai_strict_unknown))]
#[tokio::test]
async fn environment_response_parses_string_counts_and_preserves_unknowns() {
    let network =
        json!({"allowlist": [{"domain": "pypi.org", "credential": "cred-1"}], "mode": "strict"});
    let stub = Stub::replying(vec![Reply::json(
        200,
        json!({
            "id": "38aac1ae7f30fe9bd67afe42382ea041",
            "sources": [
                {"type": "inline", "target": "/etc/motd", "content": "hello"},
                {"type": "oci_image", "source": "gcr.io/x/y"}
            ],
            "network": network,
            "created": "2026-08-08T13:24:10.64798+00:00",
            "status": "hibernating",
            "file_count": "2",
            "size_bytes": "19",
            "region": "us-central1"
        }),
    )])
    .await;

    let env = stub.client().environments().get("env-1").await.unwrap();

    assert_eq!(env.id.as_deref(), Some("38aac1ae7f30fe9bd67afe42382ea041"));
    let status = env.status.as_ref().unwrap();
    assert_eq!(status.unknown_status_type(), Some("hibernating"));
    assert_eq!(env.file_count, Some(2));
    assert_eq!(env.size_bytes, Some(19));
    assert!(env.created.is_some());
    let sources = env.sources.as_deref().unwrap();
    assert_eq!(
        sources[1]
            .source_type
            .as_ref()
            .unwrap()
            .unknown_source_type(),
        Some("oci_image")
    );
    match env.network.as_ref().unwrap() {
        NetworkConfig::Allowlist { entries, extra } => {
            assert_eq!(entries[0].credential.as_deref(), Some("cred-1"));
            assert_eq!(extra["mode"], "strict");
        }
        other => panic!("expected an allowlist, got {other:?}"),
    }
    assert_eq!(env.extra["region"], "us-central1");

    let back = serde_json::to_value(&env).unwrap();
    assert_eq!(back["status"], "hibernating");
    assert_eq!(back["file_count"], "2");
    assert_eq!(back["network"], network);
    assert_eq!(back["sources"][1]["type"], "oci_image");
}

#[tokio::test]
async fn environment_list_parses_pages_and_the_empty_object() {
    let stub = Stub::replying(vec![
        Reply::json(
            200,
            json!({
                "environments": [{"id": "e1", "status": "active"}, {"id": "e2", "status": "expired"}],
                "next_page_token": "p2"
            }),
        ),
        Reply::json(200, json!({})),
    ])
    .await;
    let client = stub.client();

    let list = client.environments().list().send().await.unwrap();
    let statuses: Vec<_> = list
        .environments
        .iter()
        .map(|e| e.status.clone().unwrap())
        .collect();
    assert_eq!(
        statuses,
        [EnvironmentStatus::Active, EnvironmentStatus::Expired]
    );
    assert_eq!(list.next_page_token.as_deref(), Some("p2"));

    let empty = client.environments().list().send().await.unwrap();
    assert!(empty.environments.is_empty());
}

/// The live shape at `page_size=1` (2026-09-26): an empty page with a
/// token mid-list, and no token on the last page.
#[tokio::test]
async fn environment_list_items_follow_every_page_with_the_page_size() {
    let stub = Stub::replying(vec![
        Reply::json(
            200,
            json!({"environments": [{"id": "e1"}], "next_page_token": "cjUKD4IB"}),
        ),
        Reply::json(
            200,
            json!({"environments": [], "next_page_token": "chUKDoIB"}),
        ),
        Reply::json(200, json!({"environments": [{"id": "e2"}]})),
    ])
    .await;
    let client = stub.client();

    let environments: Vec<genai_rs::Environment> = client
        .environments()
        .list()
        .with_page_size(1)
        .items()
        .try_collect()
        .await
        .unwrap();

    let ids: Vec<_> = environments
        .iter()
        .filter_map(|e| e.id.as_deref())
        .collect();
    assert_eq!(ids, ["e1", "e2"]);
    assert_eq!(
        targets(&stub),
        [
            "/v1beta/environments?page_size=1",
            "/v1beta/environments?page_size=1&page_token=cjUKD4IB",
            "/v1beta/environments?page_size=1&page_token=chUKDoIB",
        ]
    );
}

/// Live, the file listing's token is an offset, and a page token sent
/// without `recursive=true` lists something else (`{}`, 2026-09-26), so the
/// flag must ride on every page with the path and page size.
#[tokio::test]
async fn environment_file_items_resend_the_path_recursive_and_page_size_on_every_page() {
    let stub = Stub::replying(vec![
        Reply::json(
            200,
            json!({
                "files": [{"path": "probe", "type": "DIRECTORY"}, {"path": "probe/a.txt", "type": "FILE"}],
                "next_page_token": "2"
            }),
        ),
        Reply::json(200, json!({"files": [{"path": "probe/b c.txt", "type": "FILE"}]})),
    ])
    .await;
    let client = stub.client();

    let files: Vec<genai_rs::EnvironmentFile> = client
        .environments()
        .files()
        .list("env/1", "dir name")
        .with_recursive(true)
        .with_page_size(2)
        .items()
        .try_collect()
        .await
        .unwrap();

    let paths: Vec<_> = files.iter().filter_map(|f| f.path.as_deref()).collect();
    assert_eq!(paths, ["probe", "probe/a.txt", "probe/b c.txt"]);
    // The owned environment ID and path are encoded into every page's path.
    assert_eq!(
        targets(&stub),
        [
            "/v1beta/environments/env%2F1/files/dir%20name?page_size=2&recursive=true",
            "/v1beta/environments/env%2F1/files/dir%20name?page_size=2&page_token=2&recursive=true",
        ]
    );
}

#[cfg(not(genai_strict_unknown))]
#[tokio::test]
async fn environment_file_list_parses_uppercase_types_and_preserves_unknowns() {
    let stub = Stub::replying(vec![Reply::json(
        200,
        json!({
            "files": [
                {"name": ".agents", "path": ".agents", "type": "DIRECTORY", "created": "2026-09-23T14:47:11Z"},
                {"name": "hello.txt", "path": "sweep/hello.txt", "type": "FILE", "size_bytes": "17",
                 "mime_type": "text/plain; charset=utf-8", "modified": "2026-09-24T01:46:18Z", "checksum": "abc"},
                {"name": "latest", "path": "latest", "type": "SYMLINK"}
            ],
            "next_page_token": "p2"
        }),
    )])
    .await;

    let list = stub
        .client()
        .environments()
        .files()
        .list("env-1", "")
        .with_recursive(true)
        .send()
        .await
        .unwrap();

    assert_eq!(list.files.len(), 3);
    assert_eq!(
        list.files[0].file_type,
        Some(EnvironmentFileType::Directory)
    );
    let file = &list.files[1];
    assert_eq!(file.file_type, Some(EnvironmentFileType::File));
    assert_eq!(file.size_bytes, Some(17));
    assert_eq!(file.mime_type.as_deref(), Some("text/plain; charset=utf-8"));
    assert!(file.modified.is_some());
    assert_eq!(file.extra["checksum"], "abc");
    let link = list.files[2].file_type.as_ref().unwrap();
    assert_eq!(link.unknown_file_type(), Some("SYMLINK"));
    assert_eq!(serde_json::to_value(link).unwrap(), "SYMLINK");
    assert_eq!(list.next_page_token.as_deref(), Some("p2"));
}

/// A download is `GET` on the listing's URL with `alt=media`, and hands
/// back the body's bytes as served, with its content type.
#[tokio::test]
async fn environment_file_download_reads_the_raw_body() {
    let bytes: &[u8] = b"a,b\n1,2\n\xff";
    let file = Reply {
        status: 200,
        headers: vec![("content-type".into(), "text/csv; charset=utf-8".into())],
        delay: Duration::ZERO,
        parts: vec![(Duration::ZERO, bytes.to_vec())],
        content_length: true,
    };
    let archive = Reply {
        status: 200,
        headers: vec![("content-type".into(), "application/x-tar".into())],
        delay: Duration::ZERO,
        parts: vec![(Duration::ZERO, vec![0; 1024])],
        content_length: true,
    };
    let stub = Stub::replying(vec![file, archive, not_found()]).await;
    let client = stub.client();
    let files = client.environments().files();

    let downloaded = files.download("env/1", "data/in 1.csv").await.unwrap();
    assert_eq!(downloaded.data, bytes);
    assert_eq!(
        downloaded.mime_type.as_deref(),
        Some("text/csv; charset=utf-8")
    );
    assert!(!downloaded.is_tar_archive());
    assert!(
        format!("{downloaded:?}").contains("data_len: 9"),
        "{downloaded:?}"
    );

    let directory = files.download("env/1", "data").await.unwrap();
    assert!(directory.is_tar_archive());
    assert_eq!(directory.data.len(), 1024);

    let missing = files.download("env/1", "nope").await.unwrap_err();
    assert!(
        matches!(
            missing,
            GenaiError::Api {
                status_code: 404,
                ..
            }
        ),
        "{missing:?}"
    );

    let requests = stub.requests();
    assert_eq!(
        requests
            .iter()
            .map(|r| r.target.as_str())
            .collect::<Vec<_>>(),
        [
            "/v1beta/environments/env%2F1/files/data/in%201.csv?alt=media",
            "/v1beta/environments/env%2F1/files/data?alt=media",
            "/v1beta/environments/env%2F1/files/nope?alt=media",
        ]
    );
    for request in &requests {
        assert_eq!(request.method, "GET");
        assert_eq!(request.header("x-goog-api-key"), Some("test-key"));
        assert_eq!(request.header("api-revision"), Some(API_REVISION));
        assert!(request.body.is_empty());
    }

    // A dot segment never leaves the client.
    let err = files.download("env/1", "../etc").await.unwrap_err();
    assert!(matches!(err, GenaiError::InvalidInput(_)), "{err:?}");
    assert_eq!(stub.requests().len(), 3);
}

#[tokio::test]
async fn environment_file_upload_starts_a_session_then_finalizes() {
    let stub = Stub::replying(vec![
        Reply::json(200, json!({})).header("x-goog-upload-url", "{base}/upload-session/env-1"),
        Reply::json(
            200,
            json!({"files": [{"name": "in.csv", "path": "data/in.csv", "type": "FILE", "size_bytes": "8"}]}),
        ),
    ])
    .await;

    let written = stub
        .client()
        .environments()
        .files()
        .upload(
            "env-1",
            "data/in.csv",
            EnvironmentFileUpload::new(b"a,b\n1,2\n".to_vec(), "text/csv")
                .with_overwrite(true)
                .with_extract(true),
        )
        .await
        .unwrap();
    assert_eq!(written.files[0].path.as_deref(), Some("data/in.csv"));
    assert_eq!(written.files[0].size_bytes, Some(8));

    let [start, finish] = stub.requests().try_into().unwrap();
    assert_eq!(start.method, "PUT");
    assert_eq!(
        start.target,
        "/upload/v1beta/environments/env-1/files/data/in.csv?overwrite=true&extract=true"
    );
    assert_eq!(start.header("x-goog-api-key"), Some("test-key"));
    assert_eq!(start.header("api-revision"), Some(API_REVISION));
    assert_eq!(start.header("x-goog-upload-protocol"), Some("resumable"));
    assert_eq!(start.header("x-goog-upload-command"), Some("start"));
    assert_eq!(
        start.header("x-goog-upload-header-content-length"),
        Some("8")
    );
    assert_eq!(
        start.header("x-goog-upload-header-content-type"),
        Some("text/csv")
    );
    assert!(start.body.is_empty(), "the bytes go on the second leg");

    assert_eq!(finish.method, "POST");
    assert_eq!(finish.target, "/upload-session/env-1");
    assert_eq!(
        finish.header("x-goog-upload-command"),
        Some("upload, finalize")
    );
    assert_eq!(finish.header("x-goog-upload-offset"), Some("0"));
    assert_eq!(finish.body, b"a,b\n1,2\n");
}

#[tokio::test]
async fn environment_file_upload_without_options_sends_no_query() {
    let stub = Stub::replying(vec![
        Reply::json(200, json!({})).header("x-goog-upload-url", "{base}/upload-session/2"),
        Reply::json(200, json!({"files": []})),
    ])
    .await;

    stub.client()
        .environments()
        .files()
        .upload(
            "env-1",
            "/notes/a b.txt",
            EnvironmentFileUpload::new(b"hi".to_vec(), "text/plain"),
        )
        .await
        .unwrap();

    let requests = stub.requests();
    assert_eq!(
        requests[0].target,
        "/upload/v1beta/environments/env-1/files/notes/a%20b.txt"
    );
}

#[tokio::test]
async fn environment_file_upload_without_session_url_is_malformed_response() {
    let stub = Stub::replying(vec![Reply::json(200, json!({}))]).await;

    let err = stub
        .client()
        .environments()
        .files()
        .upload(
            "env-1",
            "a.txt",
            EnvironmentFileUpload::new(b"hi".to_vec(), "text/plain"),
        )
        .await
        .unwrap_err();

    assert!(matches!(err, GenaiError::MalformedResponse(_)), "{err:?}");
    assert!(err.to_string().contains("x-goog-upload-url"), "{err}");
    assert_eq!(stub.requests().len(), 1, "no bytes without a session");
}

/// A missing environment is rejected at the start (live 2026-09-26).
#[tokio::test]
async fn environment_file_upload_start_rejection_stops_before_the_bytes() {
    let stub = Stub::replying(vec![Reply::json(
        404,
        json!({"error": {"message": "Requested entity was not found.", "code": "not_found"}}),
    )])
    .await;

    let err = stub
        .client()
        .environments()
        .files()
        .upload(
            "env-1",
            "a.txt",
            EnvironmentFileUpload::new(b"hi".to_vec(), "text/plain"),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(&err, GenaiError::Api { status_code: 404, message, .. }
            if message == "not_found: Requested entity was not found."),
        "{err:?}"
    );
    assert_eq!(stub.requests().len(), 1);
}

/// An existing file without `overwrite` is rejected only when the bytes are
/// finalized (live 2026-09-26).
#[tokio::test]
async fn environment_file_upload_conflict_surfaces_from_the_finalizing_request() {
    let stub = Stub::replying(vec![
        Reply::json(200, json!({})).header("x-goog-upload-url", "{base}/upload-session/3"),
        Reply::json(
            409,
            json!({"error": {"message": "Requested entity already exists", "code": "aborted"}}),
        ),
    ])
    .await;

    let err = stub
        .client()
        .environments()
        .files()
        .upload(
            "env-1",
            "a.txt",
            EnvironmentFileUpload::new(b"hi".to_vec(), "text/plain"),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(&err, GenaiError::Api { status_code: 409, message, .. }
            if message == "aborted: Requested entity already exists"),
        "{err:?}"
    );
    let [start, finish] = stub.requests().try_into().unwrap();
    assert_eq!(
        start.target,
        "/upload/v1beta/environments/env-1/files/a.txt"
    );
    assert_eq!(finish.target, "/upload-session/3");
    assert_eq!(finish.body, b"hi");
}

/// `RequestBuilder::header` would defer an unheaderable value to `send()`
/// as `GenaiError::Http`, which `is_retryable()` calls transient, so a retry
/// loop would spin on input that can never succeed.
#[tokio::test]
async fn environment_file_upload_rejects_an_unheaderable_mime_type_as_invalid_input() {
    let stub = Stub::replying(vec![]).await;

    let err = stub
        .client()
        .environments()
        .files()
        .upload(
            "env-1",
            "a.txt",
            EnvironmentFileUpload::new(b"x".to_vec(), "text/plain\nX-Injected: 1"),
        )
        .await
        .unwrap_err();

    assert!(matches!(err, GenaiError::InvalidInput(_)), "{err:?}");
    assert!(!err.is_retryable(), "{err:?}");
    assert!(stub.requests().is_empty());
}

/// The Files API upload validates its MIME type the same way.
#[tokio::test]
async fn files_upload_rejects_an_unheaderable_mime_type_as_invalid_input() {
    let stub = Stub::replying(vec![]).await;

    let err = stub
        .client()
        .files()
        .upload(FileUpload::from_bytes(
            b"x".to_vec(),
            "text/plain\nX-Injected: 1",
        ))
        .await
        .unwrap_err();

    assert!(matches!(err, GenaiError::InvalidInput(_)), "{err:?}");
    assert!(stub.requests().is_empty());
}
