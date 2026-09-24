//! Live tests for the Credentials resource (`/v1beta/credentials`) and the
//! environment references to it.
//!
//! ```bash
//! cargo test --test credentials_tests -- --include-ignored --nocapture
//! ```

mod common;

use common::{extended_test_timeout, get_client, with_timeout};
use futures_util::FutureExt;
use genai_rs::{
    AllowlistEntry, Client, CreateCredentialRequest, CredentialStatus, CredentialType,
    CredentialUpdate, EnvVar, GenaiError, InjectionLocation, NetworkConfig, RemoteEnvironment,
};
use std::panic::AssertUnwindSafe;

fn unique_id(label: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before epoch")
        .subsec_nanos();
    format!("genai-rs-test-{label}-{nanos}")
}

/// Creates the credential, runs `body` with its ID, and deletes it
/// afterwards, including when `body` panics.
async fn with_credential<F, Fut>(client: &Client, request: CreateCredentialRequest, body: F)
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let created = client
        .create_credential(&request)
        .await
        .expect("create_credential failed");
    let id = created.id.clone().expect("created credential has no id");

    let outcome = AssertUnwindSafe(body(id.clone())).catch_unwind().await;

    let already_gone = matches!(
        client.get_credential(&id).await,
        Err(GenaiError::Api {
            status_code: 404,
            ..
        })
    );
    if !already_gone && let Err(e) = client.delete_credential(&id).await {
        eprintln!("cleanup failed for credential {id}: {e:?}");
    }
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_bearer_credential_lifecycle() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };
    let id = unique_id("bearer");

    with_credential(
        &client,
        CreateCredentialRequest::bearer_token("dummy-token").with_id(&id),
        |id| {
            let client = client.clone();
            async move {
                let fetched = client.get_credential(&id).await.expect("get failed");
                assert_eq!(fetched.credential_type, Some(CredentialType::BearerToken));
                assert_eq!(fetched.status, Some(CredentialStatus::Active));

                let listed = client
                    .list_credentials(Some(50), None)
                    .await
                    .expect("list failed");
                assert!(
                    listed
                        .credentials
                        .iter()
                        .any(|c| c.id.as_deref() == Some(id.as_str())),
                    "created credential not listed"
                );

                let update = CredentialUpdate {
                    prefix: Some("Token".into()),
                    ..CredentialUpdate::new(CredentialType::BearerToken)
                };
                let updated = client
                    .update_credential(&id, &update, Some("prefix"))
                    .await
                    .expect("update failed");
                assert!(updated.update_time >= fetched.update_time);

                let mismatched = client
                    .update_credential(&id, &CredentialUpdate::new(CredentialType::OAuth2), None)
                    .await
                    .expect_err("type-mismatched update was accepted");
                assert!(matches!(
                    mismatched,
                    GenaiError::Api {
                        status_code: 400,
                        ..
                    }
                ));

                client.delete_credential(&id).await.expect("delete failed");
                assert!(matches!(
                    client.get_credential(&id).await,
                    Err(GenaiError::Api {
                        status_code: 404,
                        ..
                    })
                ));
            }
        },
    )
    .await;
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_duplicate_credential_id_conflicts() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };
    let id = unique_id("dup");

    with_credential(
        &client,
        CreateCredentialRequest::environment_variable("v", vec![InjectionLocation::Header])
            .with_id(&id),
        |id| {
            let client = client.clone();
            async move {
                let err = client
                    .create_credential(&CreateCredentialRequest::bearer_token("t").with_id(&id))
                    .await
                    .expect_err("duplicate id was accepted");
                assert!(
                    matches!(
                        err,
                        GenaiError::Api {
                            status_code: 409,
                            ..
                        }
                    ),
                    "unexpected error: {err:?}"
                );
            }
        },
    )
    .await;
}

/// The references are validated and accepted; their runtime effect is not
/// asserted because none was observed (2026-09-24).
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_environment_accepts_credential_references() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };
    let id = unique_id("envref");

    with_credential(
        &client,
        CreateCredentialRequest::environment_variable("s3cr3t", vec![InjectionLocation::Header])
            .with_id(&id),
        |id| {
            let client = client.clone();
            async move {
                with_timeout(extended_test_timeout(), async {
                    let environment = RemoteEnvironment::new()
                        .add_env_var("PLAIN_VAR", EnvVar::value("hello"))
                        .add_env_var("SECRET_VAR", EnvVar::credential(&id))
                        .with_network(NetworkConfig::allowlist(vec![
                            AllowlistEntry::new("example.com").with_credential(&id),
                        ]));

                    let response = client
                        .interaction()
                        .with_agent(genai_rs::DEFAULT_ANTIGRAVITY_AGENT)
                        .with_text("Reply with the single word OK.")
                        .with_environment(environment)
                        .with_background(true)
                        .with_store_enabled()
                        .create()
                        .await
                        .expect("environment with credential references was rejected");
                    let interaction_id = response.id.clone().expect("stored interaction id");

                    let _ = client.cancel_interaction(&interaction_id).await;
                    let _ = client.delete_interaction(&interaction_id).await;
                    if let Some(env_id) = response.environment_id.as_deref() {
                        let _ = client.delete_environment(env_id).await;
                    }
                })
                .await;
            }
        },
    )
    .await;
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_unknown_credential_reference_is_rejected() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let err = client
        .interaction()
        .with_agent(genai_rs::DEFAULT_ANTIGRAVITY_AGENT)
        .with_text("Reply OK.")
        .with_environment(
            RemoteEnvironment::new().add_env_var("X", EnvVar::credential("does-not-exist")),
        )
        .with_background(true)
        .with_store_enabled()
        .create()
        .await
        .expect_err("a missing credential reference was accepted");
    assert!(
        matches!(
            err,
            GenaiError::Api {
                status_code: 404,
                ..
            }
        ),
        "unexpected error: {err:?}"
    );
}
