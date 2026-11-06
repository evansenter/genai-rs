//! IDs and resource names: bare ids, full names and percent-encoding.

use super::*;

#[tokio::test]
async fn reserved_characters_in_ids_are_percent_encoded() {
    let stub = ok_stub().await;
    let client = stub.client();
    let c = &client;
    assert_wire(
        &stub,
        vec![
            wire(
                "GET",
                "/v1beta/webhooks/a%2Fb%3Fc%23d",
                None,
                c.webhooks().get("a/b?c#d"),
            ),
            // The colon verb stays outside the encoded ID.
            wire(
                "POST",
                "/v1beta/webhooks/wh%3A1:ping",
                Some(json!({})),
                c.webhooks().ping("wh:1"),
            ),
            wire(
                "POST",
                "/v1beta/webhooks/wh%3A1:rotateSigningSecret",
                Some(json!({})),
                c.webhooks().rotate_signing_secret("wh:1", None),
            ),
            wire(
                "DELETE",
                "/v1beta/webhooks/a%20b",
                None,
                c.webhooks().delete("a b"),
            ),
            // Dots inside an ID are not dot segments.
            wire(
                "GET",
                "/v1beta/webhooks/v1.2",
                None,
                c.webhooks().get("v1.2"),
            ),
            wire(
                "GET",
                "/v1beta/triggers/t%2F1",
                None,
                c.triggers().get("t/1"),
            ),
            wire(
                "POST",
                "/v1beta/triggers/t%2F1/executions",
                Some(json!({})),
                c.triggers().run("t/1"),
            ),
            wire(
                "GET",
                "/v1beta/triggers/t%3F1/executions?page_token=x%26y",
                None,
                c.triggers()
                    .list_executions("t?1")
                    .with_page_token("x&y")
                    .send(),
            ),
            wire(
                "GET",
                "/v1beta/agents/team%2Fagent",
                None,
                c.agents().get("team/agent"),
            ),
            // An already-encoded traversal is encoded again, not decoded.
            wire(
                "DELETE",
                "/v1beta/agents/%252e%252e%252f",
                None,
                c.agents().delete("%2e%2e%2f"),
            ),
            wire(
                "GET",
                "/v1beta/environments/e%231",
                None,
                c.environments().get("e#1"),
            ),
            wire(
                "GET",
                "/v1beta/environments/e%2F1/files/dir%20name/a%3Fb",
                None,
                c.environments().files().list("e/1", "dir name/a?b").send(),
            ),
            wire(
                "GET",
                "/v1beta/credentials/a%3Fb",
                None,
                c.credentials().get("a?b"),
            ),
            // A resource name stays one segment (the API answers it 400).
            wire(
                "PATCH",
                "/v1beta/credentials/credentials%2Fgh?update_mask=token%26x%3D1",
                Some(json!({"type": "bearer_token"})),
                async move {
                    let update = CredentialUpdate::new(CredentialType::BearerToken)
                        .with_update_mask("token&x=1");
                    c.credentials().update("credentials/gh", &update).await
                },
            ),
            wire(
                "GET",
                "/v1beta/voices/voice%2Fx",
                None,
                c.voices().get("voice/x"),
            ),
            wire(
                "GET",
                "/v1beta/interactions/int%2F1",
                None,
                c.interactions().get("int/1"),
            ),
            wire(
                "POST",
                "/v1beta/interactions/int%2F1/cancel",
                Some(json!({})),
                c.interactions().cancel("int/1"),
            ),
            wire(
                "DELETE",
                "/v1beta/interactions/int%3F1",
                None,
                c.interactions().delete("int?1"),
            ),
            wire(
                "GET",
                "/v1beta/interactions/int%2F1?alt=sse&stream=true&last_event_id=evt%2B1%26x",
                None,
                // Only the request matters here: the stub's body has no
                // terminal event, so the stream ends in an error.
                async move {
                    let _ = c
                        .interactions()
                        .resume_stream("int/1", "evt+1&x")
                        .collect::<Vec<_>>()
                        .await;
                    Ok::<_, GenaiError>(())
                },
            ),
            wire(
                "GET",
                "/v1beta/fileSearchStores/a%20b",
                None,
                c.file_search_stores().get("fileSearchStores/a b"),
            ),
            wire(
                "GET",
                "/v1beta/fileSearchStores/a%23b/documents",
                None,
                c.file_search_stores()
                    .documents()
                    .list("fileSearchStores/a#b")
                    .send(),
            ),
            wire(
                "GET",
                "/v1beta/fileSearchStores/a%20b/documents/c%3Fd",
                None,
                c.file_search_stores()
                    .documents()
                    .get("fileSearchStores/a b/documents/c?d"),
            ),
            wire(
                "GET",
                "/v1beta/files/a%20b",
                None,
                c.files().get("files/a b"),
            ),
            wire(
                "DELETE",
                "/v1beta/files/a%3Fb",
                None,
                c.files().delete("files/a?b"),
            ),
        ],
    )
    .await;
}

#[tokio::test]
async fn empty_and_dot_segment_ids_are_rejected_before_any_request() {
    let stub = Stub::replying(vec![]).await;
    let client = stub.client();
    let c = &client;
    let plain = |data: &[u8]| EnvironmentFileUpload::new(data.to_vec(), "text/plain");
    let poll_once = PollOptions::new().with_timeout(Duration::ZERO);

    let cases: Vec<(&str, Call<'_>)> = vec![
        ("webhooks.get empty", call(c.webhooks().get(""))),
        ("webhooks.get ..", call(c.webhooks().get(".."))),
        (
            "webhooks.update",
            call(async move { c.webhooks().update("", &WebhookUpdate::new()).await }),
        ),
        ("webhooks.delete", call(c.webhooks().delete(""))),
        ("webhooks.ping", call(c.webhooks().ping("."))),
        (
            "webhooks.rotate_signing_secret",
            call(c.webhooks().rotate_signing_secret("", None)),
        ),
        ("triggers.get", call(c.triggers().get(""))),
        (
            "triggers.update",
            call(async move { c.triggers().update("%2E%2E", &TriggerUpdate::new()).await }),
        ),
        ("triggers.delete", call(c.triggers().delete(""))),
        ("triggers.run", call(c.triggers().run("."))),
        (
            "triggers.list_executions",
            call(c.triggers().list_executions("").send()),
        ),
        (
            "triggers.list_executions items",
            call(
                c.triggers()
                    .list_executions("..")
                    .items()
                    .try_collect::<Vec<_>>(),
            ),
        ),
        ("agents.get", call(c.agents().get(""))),
        ("agents.delete", call(c.agents().delete("%2e%2e"))),
        ("environments.get", call(c.environments().get(""))),
        ("environments.delete", call(c.environments().delete(".."))),
        (
            "environments.files.list id",
            call(c.environments().files().list("", "").send()),
        ),
        (
            "environments.files.list ..",
            call(c.environments().files().list("env-1", "a/../b").send()),
        ),
        (
            "environments.files.list .",
            call(c.environments().files().list("env-1", "./a").send()),
        ),
        (
            "environments.files.list items",
            call(
                c.environments()
                    .files()
                    .list("env-1", "../a")
                    .with_recursive(true)
                    .items()
                    .try_collect::<Vec<_>>(),
            ),
        ),
        (
            "environments.files.upload id",
            call(c.environments().files().upload("", "a.txt", plain(b"x"))),
        ),
        (
            "environments.files.upload ..",
            call(
                c.environments()
                    .files()
                    .upload("env-1", "../a.txt", plain(b"x")),
            ),
        ),
        (
            "environments.files.upload empty data",
            call(
                c.environments()
                    .files()
                    .upload("env-1", "a.txt", plain(b"")),
            ),
        ),
        ("credentials.get", call(c.credentials().get(""))),
        (
            "credentials.update",
            call(async move {
                c.credentials()
                    .update("", &CredentialUpdate::new(CredentialType::BearerToken))
                    .await
            }),
        ),
        ("credentials.delete", call(c.credentials().delete("."))),
        ("voices.get", call(c.voices().get(""))),
        ("voices.delete", call(c.voices().delete(".."))),
        ("interactions.get", call(c.interactions().get(""))),
        (
            "interactions.get_with_input",
            call(c.interactions().get_with_input("..")),
        ),
        ("interactions.delete", call(c.interactions().delete(""))),
        ("interactions.cancel", call(c.interactions().cancel(""))),
        (
            "interactions.stream",
            call(async move {
                let items: Vec<_> = c.interactions().stream("").collect().await;
                assert_eq!(items.len(), 1, "the error is the stream's only item");
                items.into_iter().next().unwrap().map(drop)
            }),
        ),
        ("files.get dot", call(c.files().get("files/.."))),
        ("files.delete empty", call(c.files().delete("files/"))),
        (
            "files.wait_until_active",
            call(c.files().wait_until_active("files/", poll_once)),
        ),
        (
            "file_search_stores.get",
            call(c.file_search_stores().get("fileSearchStores/")),
        ),
        (
            "file_search_stores.delete",
            call(c.file_search_stores().delete("fileSearchStores/..")),
        ),
        (
            "file_search_stores.force_delete",
            call(c.file_search_stores().force_delete("fileSearchStores/..")),
        ),
        (
            "file_search_stores.documents.get",
            call(
                c.file_search_stores()
                    .documents()
                    .get("fileSearchStores/abc/documents/"),
            ),
        ),
        (
            "file_search_stores.documents.delete",
            call(
                c.file_search_stores()
                    .documents()
                    .delete("fileSearchStores//documents/doc-1"),
            ),
        ),
        (
            "file_search_stores.documents.force_delete",
            call(
                c.file_search_stores()
                    .documents()
                    .force_delete("fileSearchStores//documents/doc-1"),
            ),
        ),
        (
            "file_search_stores.documents.wait_until_active",
            call(
                c.file_search_stores()
                    .documents()
                    .wait_until_active("fileSearchStores/abc/documents/..", poll_once),
            ),
        ),
    ];
    for (label, call) in cases {
        let err = call.await.unwrap_err();
        assert!(
            matches!(err, GenaiError::InvalidInput(_)),
            "{label}: {err:?}"
        );
        assert!(stub.requests().is_empty(), "{label} sent a request");
    }
}

#[tokio::test]
async fn malformed_resource_names_are_rejected_before_any_request() {
    let stub = Stub::replying(vec![]).await;
    let client = stub.client();
    let c = &client;

    let cases: Vec<(&str, &str, Call<'_>)> = vec![
        ("files.get", "bare id", call(c.files().get("abc"))),
        (
            "files.get",
            "extra segment",
            call(c.files().get("files/a/b")),
        ),
        (
            "files.delete",
            "wrong prefix",
            call(c.files().delete("fileSearchStores/abc")),
        ),
        (
            "files.wait_until_active",
            "bare id",
            call(c.files().wait_until_active("abc", PollOptions::new())),
        ),
        (
            "file_search_stores.get",
            "bare id",
            call(c.file_search_stores().get("abc")),
        ),
        (
            "file_search_stores.get",
            "extra segment",
            call(c.file_search_stores().get("fileSearchStores/abc/documents")),
        ),
        (
            "file_search_stores.documents.list",
            "bare id",
            call(c.file_search_stores().documents().list("abc").send()),
        ),
        (
            "file_search_stores.documents.get",
            "store name only",
            call(c.file_search_stores().documents().get(STORE)),
        ),
        (
            "file_search_stores.documents.get",
            "no store prefix",
            call(c.file_search_stores().documents().get("documents/doc-1")),
        ),
        (
            "file_search_stores.documents.get",
            "nested store",
            call(
                c.file_search_stores()
                    .documents()
                    .get("fileSearchStores/a/b/documents/doc-1"),
            ),
        ),
        (
            "file_search_stores.documents.delete",
            "nested document",
            call(
                c.file_search_stores()
                    .documents()
                    .delete("fileSearchStores/abc/documents/d/e"),
            ),
        ),
        (
            "file_search_stores.documents.wait_until_active",
            "store name only",
            call(
                c.file_search_stores()
                    .documents()
                    .wait_until_active(STORE, PollOptions::new()),
            ),
        ),
    ];
    for (method, shape, call) in cases {
        let err = call.await.unwrap_err();
        assert!(
            matches!(err, GenaiError::InvalidInput(_)),
            "{method} ({shape}): {err:?}"
        );
    }
    assert!(stub.requests().is_empty());
}
