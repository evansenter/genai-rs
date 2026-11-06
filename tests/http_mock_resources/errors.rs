//! Error mapping, on every endpoint.

use super::file_search_stores::{upload_operation, upload_session};
use super::*;

#[tokio::test]
async fn not_found_envelope_maps_to_api_error_on_every_endpoint() {
    let stub = Stub::start(|_, _| not_found()).await;
    let client = stub.client();
    let calls = every_call(&client);
    let total = calls.len();

    for (name, _, call) in calls {
        let err = call.await.expect_err(name);
        match &err {
            GenaiError::Api {
                status_code: 404,
                message,
                request_id,
                retry_after: None,
            } => {
                assert_eq!(
                    message, "NOT_FOUND: Requested entity was not found.",
                    "{name}"
                );
                assert_eq!(request_id.as_deref(), Some("req-404"), "{name}");
            }
            other => panic!("{name}: expected a 404 Api error, got {other:?}"),
        }
        assert!(!err.is_retryable(), "{name}");
    }
    assert_eq!(stub.requests().len(), total, "one request per call");
}

#[tokio::test]
async fn non_json_bad_gateway_falls_back_to_a_body_preview_on_every_endpoint() {
    let page = format!(
        "<html><head><title>502 Bad Gateway</title></head><body>{}</body></html>",
        "upstream connect error ".repeat(20)
    );
    let preview = format!("{}...", &page[..200]);
    let stub = Stub::start(move |_, _| Reply::text(502, &page)).await;
    let client = stub.client();

    for (name, _, call) in every_call(&client) {
        let err = call.await.expect_err(name);
        assert!(
            matches!(&err, GenaiError::Api { status_code: 502, message, .. } if *message == preview),
            "{name}: {err:?}"
        );
        assert!(err.is_retryable(), "{name}");
    }
}

#[tokio::test]
async fn non_json_success_body_is_malformed_only_where_a_resource_is_returned() {
    let stub = Stub::start(|_, _| Reply::text(200, "<html>captive portal</html>")).await;
    let client = stub.client();

    for (name, returns, call) in every_call(&client) {
        let result = call.await;
        match returns {
            Returns::Resource => {
                let err = result.expect_err(name);
                assert!(
                    matches!(err, GenaiError::MalformedResponse(_)),
                    "{name}: {err:?}"
                );
                assert!(!err.is_retryable(), "{name}");
            }
            Returns::Nothing => {
                result.unwrap_or_else(|e| panic!("{name} must ignore the body: {e:?}"));
            }
        }
    }
}

#[tokio::test]
async fn unit_endpoints_accept_an_empty_no_content_reply() {
    let stub = Stub::start(|_, _| Reply::text(204, "")).await;
    let client = stub.client();

    let unit_calls: Vec<_> = every_call(&client)
        .into_iter()
        .filter(|(_, returns, _)| *returns == Returns::Nothing)
        .collect();
    assert!(unit_calls.len() >= 10, "every delete and ping");
    for (name, _, call) in unit_calls {
        call.await
            .unwrap_or_else(|e| panic!("{name} on 204: {e:?}"));
    }
}

#[tokio::test]
async fn empty_object_parses_as_an_empty_last_page_on_every_list_endpoint() {
    type Page<'a> = Pin<Box<dyn Future<Output = Result<(usize, Option<String>), GenaiError>> + 'a>>;
    // The API answers `{}` for an empty collection.
    let stub = Stub::start(|_, _| Reply::json(200, json!({}))).await;
    let client = stub.client();
    let c = &client;

    let pages: Vec<(&str, Page<'_>)> = vec![
        (
            "webhooks.list",
            Box::pin(async move {
                let l = c.webhooks().list().send().await?;
                Ok((l.webhooks.len(), l.next_page_token))
            }),
        ),
        (
            "webhooks.list items",
            Box::pin(async move {
                let items: Vec<_> = c.webhooks().list().items().try_collect().await?;
                Ok((items.len(), None))
            }),
        ),
        (
            "triggers.list",
            Box::pin(async move {
                let l = c.triggers().list().send().await?;
                Ok((l.triggers.len(), l.next_page_token))
            }),
        ),
        (
            "triggers.list items",
            Box::pin(async move {
                let items: Vec<_> = c.triggers().list().items().try_collect().await?;
                Ok((items.len(), None))
            }),
        ),
        (
            "triggers.list_executions",
            Box::pin(async move {
                let l = c.triggers().list_executions("t-1").send().await?;
                Ok((l.trigger_executions.len(), l.next_page_token))
            }),
        ),
        (
            "triggers.list_executions items",
            Box::pin(async move {
                let items: Vec<_> = c
                    .triggers()
                    .list_executions("t-1")
                    .items()
                    .try_collect()
                    .await?;
                Ok((items.len(), None))
            }),
        ),
        (
            "agents.list",
            Box::pin(async move {
                let l = c.agents().list().send().await?;
                Ok((l.agents.len(), l.next_page_token))
            }),
        ),
        (
            "agents.list items",
            Box::pin(async move {
                let items: Vec<_> = c.agents().list().items().try_collect().await?;
                Ok((items.len(), None))
            }),
        ),
        (
            "environments.list",
            Box::pin(async move {
                let l = c.environments().list().send().await?;
                Ok((l.environments.len(), l.next_page_token))
            }),
        ),
        (
            "environments.list items",
            Box::pin(async move {
                let items: Vec<_> = c.environments().list().items().try_collect().await?;
                Ok((items.len(), None))
            }),
        ),
        (
            "environments.files.list",
            Box::pin(async move {
                let l = c.environments().files().list("env-1", "").send().await?;
                Ok((l.files.len(), l.next_page_token))
            }),
        ),
        (
            "environments.files.list items",
            Box::pin(async move {
                let items: Vec<_> = c
                    .environments()
                    .files()
                    .list("env-1", "")
                    .items()
                    .try_collect()
                    .await?;
                Ok((items.len(), None))
            }),
        ),
        (
            "credentials.list",
            Box::pin(async move {
                let l = c.credentials().list().send().await?;
                Ok((l.credentials.len(), l.next_page_token))
            }),
        ),
        (
            "credentials.list items",
            Box::pin(async move {
                let items: Vec<_> = c.credentials().list().items().try_collect().await?;
                Ok((items.len(), None))
            }),
        ),
        (
            "voices.list",
            Box::pin(async move {
                let l = c.voices().list().send().await?;
                Ok((l.voices.len(), l.next_page_token))
            }),
        ),
        (
            "voices.list items",
            Box::pin(async move {
                let items: Vec<_> = c.voices().list().items().try_collect().await?;
                Ok((items.len(), None))
            }),
        ),
        (
            "file_search_stores.list",
            Box::pin(async move {
                let l = c.file_search_stores().list().send().await?;
                Ok((l.stores.len(), l.next_page_token))
            }),
        ),
        (
            "file_search_stores.list items",
            Box::pin(async move {
                let items: Vec<_> = c.file_search_stores().list().items().try_collect().await?;
                Ok((items.len(), None))
            }),
        ),
        (
            "file_search_stores.documents.list",
            Box::pin(async move {
                let l = c
                    .file_search_stores()
                    .documents()
                    .list(STORE)
                    .send()
                    .await?;
                Ok((l.documents.len(), l.next_page_token))
            }),
        ),
        (
            "file_search_stores.documents.list items",
            Box::pin(async move {
                let items: Vec<_> = c
                    .file_search_stores()
                    .documents()
                    .list(STORE)
                    .items()
                    .try_collect()
                    .await?;
                Ok((items.len(), None))
            }),
        ),
        (
            "files.list",
            Box::pin(async move {
                let l = c.files().list().send().await?;
                Ok((l.files.len(), l.next_page_token))
            }),
        ),
        (
            "files.list items",
            Box::pin(async move {
                let items: Vec<_> = c.files().list().items().try_collect().await?;
                Ok((items.len(), None))
            }),
        ),
    ];
    for (name, page) in pages {
        let (len, token) = page.await.unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!((len, token), (0, None), "{name}");
    }
}

/// On the paused clock (#501), held until the stub has the request: the
/// client's 100 ms total timeout then fires in virtual time against the
/// stub's 5 s delay, on any runner.
#[tokio::test(start_paused = true)]
async fn client_timeout_applies_to_resource_calls() {
    let (gate, release) = hold_clock();
    let stub = Stub::start(move |_, _| {
        release();
        Reply::json(200, json!({"id": "wh-1"})).delayed(Duration::from_secs(5))
    })
    .await;
    let client = genai_rs::Client::builder("test-key".to_string())
        .with_base_url(&stub.base_url)
        .with_timeout(Duration::from_millis(100))
        .build()
        .unwrap();

    let started = tokio::time::Instant::now();
    let err = client.webhooks().get("wh-1").await.unwrap_err();
    gate.await.unwrap();

    assert!(
        matches!(err, GenaiError::Http(ref e) if e.is_timeout()),
        "{err:?}"
    );
    assert!(err.is_retryable());
    assert_eq!(
        started.elapsed(),
        Duration::from_millis(100),
        "the reply was awaited past the timeout"
    );
}

#[tokio::test]
async fn base_url_prefix_applies_to_resource_and_upload_endpoints() {
    let stub = Stub::replying(vec![
        Reply::json(200, json!({})),
        Reply::json(200, json!({})).header("x-goog-upload-url", "{base}/session/1"),
        Reply::json(200, json!({"files": []})),
        upload_session("{base}/session/2"),
        upload_operation(),
        document(Some("STATE_PENDING")),
    ])
    .await;
    let client = genai_rs::Client::builder("test-key".to_string())
        .with_base_url(format!("{}/proxy/", stub.base_url))
        .build()
        .unwrap();
    let (_dir, path) = temp_file("notes.txt", b"hello");

    client.webhooks().list().send().await.unwrap();
    client
        .environments()
        .files()
        .upload(
            "env-1",
            "a.txt",
            EnvironmentFileUpload::new(b"hi".to_vec(), "text/plain"),
        )
        .await
        .unwrap();
    client
        .file_search_stores()
        .upload(STORE, FileUpload::from_path(&path))
        .await
        .unwrap();

    let targets: Vec<_> = stub.requests().into_iter().map(|r| r.target).collect();
    assert_eq!(
        targets,
        [
            "/proxy/v1beta/webhooks",
            "/proxy/upload/v1beta/environments/env-1/files/a.txt",
            "/session/1",
            "/proxy/upload/v1beta/fileSearchStores/abc:uploadToFileSearchStore",
            "/session/2",
            "/proxy/v1beta/fileSearchStores/abc/documents/doc-1",
        ]
    );
}
