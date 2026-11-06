//! Credentials: every endpoint's request, the responses, list paging, and
//! secrets redacted from `LOUD_WIRE`.

use super::*;

#[tokio::test]
async fn credential_endpoints_send_the_documented_requests() {
    let stub = ok_stub().await;
    let client = stub.client();
    let c = &client;
    assert_wire(
        &stub,
        vec![
            wire(
                "POST",
                "/v1beta/credentials",
                Some(json!({
                    "id": "gh-token",
                    "type": "bearer_token",
                    "token": "s3cret",
                    "header_name": "Authorization",
                    "prefix": "Bearer"
                })),
                async move {
                    let request = CreateCredentialRequest::new(CredentialConfig::BearerToken {
                        token: "s3cret".into(),
                        header_name: Some("Authorization".into()),
                        prefix: Some("Bearer".into()),
                    })
                    .with_id("gh-token");
                    c.credentials().create(&request).await
                },
            ),
            wire(
                "POST",
                "/v1beta/credentials",
                Some(json!({
                    "type": "environment_variable",
                    "value": "v4lue",
                    "injection_location": ["header", "query"],
                    "trusted_domains": ["api.example"]
                })),
                async move {
                    let request = CreateCredentialRequest::environment_variable(
                        "v4lue",
                        vec![InjectionLocation::Header, InjectionLocation::Query],
                        vec!["api.example".into()],
                    );
                    c.credentials().create(&request).await
                },
            ),
            wire(
                "POST",
                "/v1beta/credentials",
                Some(json!({
                    "type": "oauth2",
                    "client_id": "cid",
                    "client_secret": "csec",
                    "refresh_token": "rtok",
                    "token_url": "https://oauth.example/token",
                    "scopes": ["repo"]
                })),
                async move {
                    let request = CreateCredentialRequest::new(CredentialConfig::OAuth2 {
                        client_id: "cid".into(),
                        client_secret: "csec".into(),
                        refresh_token: "rtok".into(),
                        token_url: "https://oauth.example/token".into(),
                        scopes: Some(vec!["repo".into()]),
                    });
                    c.credentials().create(&request).await
                },
            ),
            wire(
                "GET",
                "/v1beta/credentials/gh-token",
                None,
                c.credentials().get("gh-token"),
            ),
            wire(
                "GET",
                "/v1beta/credentials?page_size=10&page_token=n",
                None,
                c.credentials()
                    .list()
                    .with_page_size(10)
                    .with_page_token("n")
                    .send(),
            ),
            wire(
                "PATCH",
                "/v1beta/credentials/gh-token?update_mask=token%2Cprefix",
                Some(json!({"type": "bearer_token", "token": "rotated", "prefix": ""})),
                async move {
                    let mut update = CredentialUpdate::new(CredentialType::BearerToken)
                        .with_update_mask("token,prefix");
                    update.token = Some("rotated".into());
                    update.prefix = Some(String::new());
                    c.credentials().update("gh-token", &update).await
                },
            ),
            wire(
                "PATCH",
                "/v1beta/credentials/gh-token",
                Some(json!({"type": "environment_variable", "trusted_domains": ["api.example"]})),
                async move {
                    let mut update = CredentialUpdate::new(CredentialType::EnvironmentVariable);
                    update.trusted_domains = Some(vec!["api.example".into()]);
                    c.credentials().update("gh-token", &update).await
                },
            ),
            wire(
                "DELETE",
                "/v1beta/credentials/gh-token",
                None,
                c.credentials().delete("gh-token"),
            ),
        ],
    )
    .await;
}

#[cfg(not(genai_strict_unknown))]
#[tokio::test]
async fn credential_responses_parse_and_preserve_unknowns() {
    let stub = Stub::replying(vec![Reply::json(
        200,
        json!({
            "credentials": [
                {"id": "sweep-bearer-1", "status": "active", "type": "bearer_token",
                 "create_time": "2026-09-24T01:42:57.677356880Z", "update_time": "2026-09-24T01:42:57.677356880Z"},
                {"id": "deploy-key", "status": "rotating", "type": "ssh_key", "fingerprint": "SHA256:abc"}
            ],
            "next_page_token": "n"
        }),
    )])
    .await;

    let list = stub.client().credentials().list().send().await.unwrap();

    let bearer = &list.credentials[0];
    assert_eq!(bearer.credential_type, Some(CredentialType::BearerToken));
    assert_eq!(bearer.status, Some(CredentialStatus::Active));
    assert!(bearer.create_time.is_some() && bearer.update_time.is_some());
    let ssh = &list.credentials[1];
    assert_eq!(
        ssh.credential_type
            .as_ref()
            .unwrap()
            .unknown_credential_type(),
        Some("ssh_key")
    );
    assert_eq!(
        ssh.status.as_ref().unwrap().unknown_status_type(),
        Some("rotating")
    );
    assert_eq!(ssh.extra["fingerprint"], "SHA256:abc");
    assert_eq!(list.next_page_token.as_deref(), Some("n"));
}

/// The live shape at `page_size=1` (2026-09-27): URL-safe base64 tokens,
/// and no token on the last page.
#[tokio::test]
async fn credential_list_items_follow_every_page_with_the_page_size() {
    let stub = Stub::replying(vec![
        Reply::json(
            200,
            json!({
                "credentials": [{"id": "cred-1", "type": "bearer_token"}],
                "next_page_token": "cgoKCEIGY3JlZC0x"
            }),
        ),
        Reply::json(
            200,
            json!({"credentials": [{"id": "cred-2", "type": "environment_variable"}]}),
        ),
    ])
    .await;
    let client = stub.client();

    let credentials: Vec<genai_rs::Credential> = client
        .credentials()
        .list()
        .with_page_size(1)
        .items()
        .try_collect()
        .await
        .unwrap();

    let ids: Vec<_> = credentials.iter().filter_map(|c| c.id.as_deref()).collect();
    assert_eq!(ids, ["cred-1", "cred-2"]);
    assert_eq!(
        targets(&stub),
        [
            "/v1beta/credentials?page_size=1",
            "/v1beta/credentials?page_size=1&page_token=cgoKCEIGY3JlZC0x",
        ]
    );
}

/// Set in the child process `loud_wire_redacts_credential_secrets` spawns.
const LOUD_WIRE_CHILD_ENV: &str = "GENAI_RS_LOUD_WIRE_SECRETS_CHILD";

/// Every request body reaches the `LOUD_WIRE` printer, and a credential
/// create or update body carries its secret as `token`, `value`,
/// `client_secret` or `refresh_token`.
///
/// `LOUD_WIRE` is read when a client is built, so the test re-runs itself
/// as a child process with it set and inspects the child's stderr.
#[tokio::test]
async fn loud_wire_redacts_credential_secrets() {
    const SECRETS: [&str; 5] = [
        "tok-8f3a1c",
        "val-2d9e4b",
        "csec-5b2e9d",
        "rtok-7c4d0a",
        "val-upd-6e1f",
    ];

    if std::env::var_os(LOUD_WIRE_CHILD_ENV).is_some() {
        let stub = Stub::start(|_, _| Reply::json(200, json!({"id": "cred-1"}))).await;
        let client = stub.client();
        let requests = [
            CreateCredentialRequest::bearer_token(SECRETS[0]),
            CreateCredentialRequest::environment_variable(SECRETS[1], vec![], vec![]),
            CreateCredentialRequest::new(CredentialConfig::OAuth2 {
                client_id: "cid".into(),
                client_secret: SECRETS[2].into(),
                refresh_token: SECRETS[3].into(),
                token_url: "https://oauth.example/token".into(),
                scopes: None,
            }),
        ];
        for request in &requests {
            client.credentials().create(request).await.unwrap();
        }
        let update = CredentialUpdate {
            value: Some(SECRETS[4].into()),
            ..CredentialUpdate::new(CredentialType::EnvironmentVariable)
        };
        client
            .credentials()
            .update("cred-1", &update)
            .await
            .unwrap();
        return;
    }

    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            // The name libtest knows this test by: its module path in the
            // binary, then the function.
            "credentials::loud_wire_redacts_credential_secrets",
            "--exact",
            "--nocapture",
        ])
        .env(LOUD_WIRE_CHILD_ENV, "1")
        .env("LOUD_WIRE", "1")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "child failed:\n{stderr}");
    assert!(
        stderr.contains("/v1beta/credentials"),
        "the child printed its requests:\n{stderr}"
    );
    for secret in SECRETS {
        assert!(
            !stderr.contains(secret),
            "LOUD_WIRE printed the secret {secret:?}:\n{stderr}"
        );
    }
}
