//! Live tests for the Credentials resource (`/v1beta/credentials`) and the
//! environment references to it.
//!
//! ```bash
//! cargo test --test credentials_tests -- --include-ignored --nocapture
//! ```

mod common;

use common::{get_client, poll_until_done};
use futures_util::FutureExt;
use genai_rs::{
    AllowlistEntry, Client, CreateCredentialRequest, CredentialConfig, CredentialStatus,
    CredentialType, CredentialUpdate, EnvVar, EnvironmentSource, GenaiError, InjectionLocation,
    InteractionStatus, NetworkConfig, RemoteEnvironment,
};
use std::panic::AssertUnwindSafe;
use std::time::Duration;

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

/// The script the agent is asked to run, mounted into the environment.
/// Everything the test asserts comes from its output as the sandbox reports
/// it (`code_execution_result`), never from the model's reply: a model that
/// ignores the instruction can make this test fail, but not pass.
const CHECK_SCRIPT: &str = concat!(
    "cat genai_rs_test/fixture.txt; echo\n",
    "echo \"PLAIN=[$GENAI_RS_TEST_PLAIN]\"\n",
    "echo \"CREDENTIAL=[$GENAI_RS_TEST_CREDENTIAL]\"\n",
    "echo '--- egress'\n",
    "curl -s -m 20 \"https://httpbin.org/anything?q=$GENAI_RS_TEST_CREDENTIAL\" \\\n",
    "  -H \"X-Test: $GENAI_RS_TEST_CREDENTIAL\"\n",
);

/// Says what the run is, because it is exactly what a model should be wary
/// of doing unasked: read credentials and send them to a third-party host.
const CHECK_PROMPT: &str = "This is an automated integration test for the genai-rs client \
    library. It checks that this environment's variables and its credential injection \
    work. Every value involved is a throwaway fixture created for this run and deleted \
    afterwards. Run `bash genai_rs_test/check.sh` once and reply with its raw output only.";

/// Environment variables and credentials take effect at runtime (first
/// verified live 2026-09-24):
///
/// - a plain `env` value is visible in the sandbox;
/// - an `environment_variable` credential is *not*: the sandbox sees a
///   placeholder, and the egress proxy substitutes the secret into requests
///   to its `trusted_domains`, at its `injection_location`s;
/// - a credential on an allowlist entry is injected into requests to that
///   domain.
///
/// Uses httpbin.org as the echo server, so an httpbin outage fails it.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_credentials_reach_sandbox_and_egress() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };
    let file_nonce = unique_id("file");
    let plain = unique_id("plain");
    let secret = unique_id("secret");
    let token = unique_id("token");

    let bearer = CreateCredentialRequest::bearer_token(&token);
    let secret_request = CreateCredentialRequest::new(CredentialConfig::EnvironmentVariable {
        value: secret.clone(),
        injection_location: vec![InjectionLocation::Header, InjectionLocation::Query],
        trusted_domains: Some(vec!["httpbin.org".into()]),
    });

    with_credential(&client, bearer, |bearer_id| {
        let client = client.clone();
        async move {
            with_credential(&client, secret_request, |secret_id| {
                let client = client.clone();
                async move {
                    let environment = RemoteEnvironment::new()
                        .add_source(EnvironmentSource::inline(
                            "genai_rs_test/fixture.txt",
                            &file_nonce,
                        ))
                        .add_source(EnvironmentSource::inline(
                            "genai_rs_test/check.sh",
                            CHECK_SCRIPT,
                        ))
                        .add_env_var("GENAI_RS_TEST_PLAIN", EnvVar::value(&plain))
                        .add_env_var("GENAI_RS_TEST_CREDENTIAL", EnvVar::credential(&secret_id))
                        .with_network(NetworkConfig::allowlist(vec![
                            AllowlistEntry::new("httpbin.org").with_credential(&bearer_id),
                        ]));
                    let created = client
                        .interaction()
                        .with_agent(genai_rs::DEFAULT_ANTIGRAVITY_AGENT)
                        .with_text(CHECK_PROMPT)
                        .with_environment(environment)
                        .with_background(true)
                        .with_store_enabled()
                        .create()
                        .await
                        .expect("interaction with credential references was rejected");
                    let id = created.id.clone().expect("stored interaction id");

                    let outcome =
                        AssertUnwindSafe(poll_until_done(&client, &id, Duration::from_secs(300)))
                            .catch_unwind()
                            .await;
                    let _ = client.delete_interaction(&id).await;
                    if let Some(env_id) = created.environment_id.as_deref() {
                        let _ = client.delete_environment(env_id).await;
                    }
                    let done = outcome.unwrap_or_else(|panic| std::panic::resume_unwind(panic));

                    assert_eq!(done.status, InteractionStatus::Completed, "{done:?}");
                    let output: String = done
                        .code_execution_results()
                        .iter()
                        .map(|r| r.result)
                        .collect::<Vec<_>>()
                        .join("\n");
                    let step_types: Vec<_> = done.steps.iter().map(|s| s.step_type()).collect();
                    assert!(
                        output.contains(&file_nonce),
                        "the script did not run inside the environment (fixture file \
                         not read); sandbox output: {output:?}; steps: {step_types:?}; reply: {:?}",
                        done.as_text()
                    );
                    let (sandbox, egress) = output
                        .split_once("--- egress")
                        .unwrap_or_else(|| panic!("no egress section in {output:?}"));

                    assert!(
                        sandbox.contains(&format!("PLAIN=[{plain}]")),
                        "plain env var not visible: {sandbox:?}"
                    );
                    assert!(
                        !sandbox.contains("CREDENTIAL=[]"),
                        "credential env var not set at all: {sandbox:?}"
                    );
                    assert!(
                        !sandbox.contains(&secret),
                        "the credential's secret is readable inside the sandbox"
                    );
                    assert!(
                        egress.matches(secret.as_str()).count() >= 2,
                        "secret not substituted into both the query and the header: {egress:?}"
                    );
                    assert!(
                        egress.contains(&format!("Bearer {token}")),
                        "allowlist credential not injected: {egress:?}"
                    );
                }
            })
            .await;
        }
    })
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
