//! Failing tests for known bugs, ignored until fixed.
//!
//! They live in a subdirectory so `ci_coverage.rs`, which requires every
//! ignore reason in a top-level test file to be the live-test one, does not
//! mistake them for unrun live tests. Run them with
//! `cargo nextest run --test http_mock_resources --run-ignored only`.

use super::common::http_stub::{Reply, Stub};
use genai_rs::{CreateCredentialRequest, CredentialConfig, EnvironmentFileUpload, GenaiError};
use serde_json::json;

/// `http/file_search_stores.rs` validates the MIME header value up front for
/// exactly this reason; `http/environment_files.rs` passes it straight to
/// `RequestBuilder::header`, which defers the failure to `send()` as a
/// builder error, i.e. `GenaiError::Http`, which `is_retryable()` calls
/// transient. A retry loop would spin on input that can never succeed.
#[tokio::test]
#[ignore = "known bug: upload_environment_file reports an unheaderable MIME type as a retryable GenaiError::Http instead of InvalidInput"]
async fn upload_environment_file_rejects_an_unheaderable_mime_type_as_invalid_input() {
    let stub = Stub::replying(vec![]).await;

    let err = stub
        .client()
        .upload_environment_file(
            "env-1",
            "a.txt",
            b"x".to_vec(),
            "text/plain\nX-Injected: 1",
            EnvironmentFileUpload::default(),
        )
        .await
        .unwrap_err();

    assert!(matches!(err, GenaiError::InvalidInput(_)), "{err:?}");
    assert!(!err.is_retryable(), "{err:?}");
    assert!(stub.requests().is_empty());
}

/// Set in the child process `loud_wire_redacts_credential_secrets` spawns.
const CHILD_ENV: &str = "GENAI_RS_LOUD_WIRE_SECRETS_CHILD";

/// `send_and_read` hands every request body to the wire inspectors, and the
/// `LOUD_WIRE` printer redacts only `api_key`, `new_signing_secret` and
/// `secret` (`REDACT_FIELDS` in `src/wire.rs`). A credential create or
/// update body carries its secret as `token`, `value`, `client_secret` or
/// `refresh_token`, so `LOUD_WIRE=1` prints it to stderr in full.
///
/// `LOUD_WIRE` is read when a client is built, so the test re-runs itself
/// as a child process with it set and inspects the child's stderr.
#[tokio::test]
#[ignore = "known bug: LOUD_WIRE prints credential secrets (token, value, client_secret, refresh_token) unredacted"]
async fn loud_wire_redacts_credential_secrets() {
    const SECRETS: [&str; 4] = ["tok-8f3a1c", "val-2d9e4b", "csec-5b2e9d", "rtok-7c4d0a"];

    if std::env::var_os(CHILD_ENV).is_some() {
        let stub = Stub::start(|_, _| Reply::json(200, json!({"id": "cred-1"}))).await;
        let client = stub.client();
        let requests = [
            CreateCredentialRequest::bearer_token(SECRETS[0]),
            CreateCredentialRequest::environment_variable(SECRETS[1], vec![]),
            CreateCredentialRequest::new(CredentialConfig::OAuth2 {
                client_id: "cid".into(),
                client_secret: SECRETS[2].into(),
                refresh_token: SECRETS[3].into(),
                token_url: "https://oauth.example/token".into(),
                scopes: None,
            }),
        ];
        for request in &requests {
            client.create_credential(request).await.unwrap();
        }
        return;
    }

    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "known_bugs::loud_wire_redacts_credential_secrets",
            "--exact",
            "--include-ignored",
            "--nocapture",
        ])
        .env(CHILD_ENV, "1")
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
