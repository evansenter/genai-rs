//! Offline wire tests for every resource endpoint, against the local stub in
//! `common/http_stub.rs`.
//!
//! Each endpoint is pinned on its HTTP method, path (percent-encoding
//! included), query, and JSON body, and a realistic response is parsed into
//! the typed result with Evergreen `Unknown` and `extra` preservation. Error
//! mapping and the wait helpers' success, failure and timeout paths are
//! covered too. Files API uploads (`upload_file*`) are left out: that API is
//! being redesigned.
//!
//! Known bugs live in `http_mock_resources/known_bugs.rs`, outside the
//! top-level files `ci_coverage.rs` scans for live-test ignore reasons.
//!
//! Tests whose replies carry unknown enum values are compiled out under
//! `strict-unknown`, which rejects those values by design.

mod common;

#[path = "http_mock_resources/known_bugs.rs"]
mod known_bugs;

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use common::http_stub::{Reply, Stub};
use futures_util::StreamExt;
use genai_rs::{
    Agent, AllowlistEntry, CreateCredentialRequest, CreateEnvironmentRequest,
    CreateFileSearchStoreRequest, CreateVoiceRequest, CredentialConfig, CredentialType,
    CredentialUpdate, DocumentState, EnvironmentFileUpload, EnvironmentSource, EnvironmentSpec,
    EnvironmentStatus, FileMetadata, FileSearchDocument, GenaiError, InjectionLocation,
    InteractionInput, InteractionRequest, ListVoicesParams, NetworkConfig, RevocationBehavior,
    StreamChunk, Tool, TriggerCreateParams, TriggerStatus, TriggerUpdate, VoiceAudio, VoicePitch,
    VoiceType, Webhook, WebhookEvent, WebhookState, WebhookUpdate,
};
#[cfg(not(feature = "strict-unknown"))]
use genai_rs::{CredentialStatus, EnvironmentFileType, InteractionStatus, TriggerExecutionStatus};
use serde_json::{Value, json};

/// The `Api-Revision` every Interactions-family request carries.
const API_REVISION: &str = "2026-05-20";

const STORE: &str = "fileSearchStores/abc";
const DOC: &str = "fileSearchStores/abc/documents/doc-1";

// =============================================================================
// Harness
// =============================================================================

/// A client call with its result discarded, so calls returning different
/// types fit in one table.
type Call<'a> = Pin<Box<dyn Future<Output = Result<(), GenaiError>> + 'a>>;

fn call<'a, T: 'a>(fut: impl Future<Output = Result<T, GenaiError>> + 'a) -> Call<'a> {
    Box::pin(async move { fut.await.map(drop) })
}

/// A call and the one request it must produce.
struct Wire<'a> {
    method: &'static str,
    target: String,
    /// The exact JSON body, or `None` for a request without one.
    body: Option<Value>,
    call: Call<'a>,
}

fn wire<'a, T: 'a>(
    method: &'static str,
    target: impl Into<String>,
    body: Option<Value>,
    fut: impl Future<Output = Result<T, GenaiError>> + 'a,
) -> Wire<'a> {
    Wire {
        method,
        target: target.into(),
        body,
        call: call(fut),
    }
}

/// A stub answering every request with a body every typed result in the
/// wire tables parses from (`FileMetadata` is the only one with required
/// fields).
async fn ok_stub() -> Stub {
    Stub::start(|_, _| Reply::json(200, json!({"name": "files/abc", "mimeType": "text/plain"})))
        .await
}

/// Runs each call in turn, asserting it sent exactly one request with the
/// expected method, target, body, and standard headers.
async fn assert_wire(stub: &Stub, rows: Vec<Wire<'_>>) {
    for Wire {
        method,
        target,
        body,
        call,
    } in rows
    {
        let label = format!("{method} {target}");
        let before = stub.requests().len();
        call.await.unwrap_or_else(|e| panic!("{label}: {e:?}"));
        let requests = stub.requests();
        assert_eq!(requests.len(), before + 1, "{label}: one request per call");
        let request = &requests[before];
        assert_eq!(request.method, method, "{label}");
        assert_eq!(request.target, target, "{label}");
        assert_eq!(
            request.header("x-goog-api-key"),
            Some("test-key"),
            "{label}"
        );
        assert_eq!(
            request.header("api-revision"),
            Some(API_REVISION),
            "{label}"
        );
        assert!(!request.target.contains("key="), "{label}: key in the URL");
        match body {
            Some(expected) => {
                assert_eq!(
                    request.header("content-type"),
                    Some("application/json"),
                    "{label}"
                );
                assert_eq!(request.json(), expected, "{label}");
            }
            None => assert!(
                request.body.is_empty(),
                "{label}: unexpected body {:?}",
                String::from_utf8_lossy(&request.body)
            ),
        }
    }
}

/// Whether a call hands back a parsed resource or nothing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Returns {
    Resource,
    Nothing,
}

/// One call per public endpoint method (the wait helpers and the
/// store upload aside), with valid IDs.
fn every_call(c: &genai_rs::Client) -> Vec<(&'static str, Returns, Call<'_>)> {
    use Returns::{Nothing, Resource};
    vec![
        (
            "create_webhook",
            Resource,
            call(async move {
                c.create_webhook(&Webhook::new("https://example.com/h", vec![]))
                    .await
            }),
        ),
        ("get_webhook", Resource, call(c.get_webhook("wh-1"))),
        ("list_webhooks", Resource, call(c.list_webhooks(None, None))),
        (
            "update_webhook",
            Resource,
            call(async move {
                c.update_webhook("wh-1", &WebhookUpdate::new().with_name("n"), None)
                    .await
            }),
        ),
        ("delete_webhook", Nothing, call(c.delete_webhook("wh-1"))),
        ("ping_webhook", Nothing, call(c.ping_webhook("wh-1"))),
        (
            "rotate_webhook_signing_secret",
            Resource,
            call(c.rotate_webhook_signing_secret("wh-1", None)),
        ),
        (
            "create_trigger",
            Resource,
            call(async move { c.create_trigger(&trigger_params()).await }),
        ),
        ("get_trigger", Resource, call(c.get_trigger("t-1"))),
        ("list_triggers", Resource, call(c.list_triggers(None, None))),
        (
            "update_trigger",
            Resource,
            call(async move {
                c.update_trigger("t-1", &TriggerUpdate::new().with_display_name("n"))
                    .await
            }),
        ),
        ("delete_trigger", Nothing, call(c.delete_trigger("t-1"))),
        ("run_trigger", Resource, call(c.run_trigger("t-1"))),
        (
            "list_trigger_executions",
            Resource,
            call(c.list_trigger_executions("t-1", None, None)),
        ),
        (
            "create_agent",
            Resource,
            call(async move { c.create_agent(&Agent::new("a")).await }),
        ),
        ("get_agent", Resource, call(c.get_agent("a"))),
        (
            "list_agents",
            Resource,
            call(c.list_agents(None, None, None)),
        ),
        ("delete_agent", Nothing, call(c.delete_agent("a"))),
        (
            "create_environment",
            Resource,
            call(async move {
                c.create_environment(&CreateEnvironmentRequest::from_environment("env-0"))
                    .await
            }),
        ),
        (
            "get_environment",
            Resource,
            call(c.get_environment("env-1")),
        ),
        (
            "list_environments",
            Resource,
            call(c.list_environments(None, None)),
        ),
        (
            "delete_environment",
            Nothing,
            call(c.delete_environment("env-1")),
        ),
        (
            "list_environment_files",
            Resource,
            call(c.list_environment_files("env-1", "", false, None, None)),
        ),
        (
            "upload_environment_file",
            Resource,
            call(c.upload_environment_file(
                "env-1",
                "a.txt",
                b"x".to_vec(),
                "text/plain",
                EnvironmentFileUpload::default(),
            )),
        ),
        (
            "create_credential",
            Resource,
            call(async move {
                c.create_credential(&CreateCredentialRequest::bearer_token("t"))
                    .await
            }),
        ),
        ("get_credential", Resource, call(c.get_credential("cred-1"))),
        (
            "list_credentials",
            Resource,
            call(c.list_credentials(None, None)),
        ),
        (
            "update_credential",
            Resource,
            call(async move {
                c.update_credential(
                    "cred-1",
                    &CredentialUpdate::new(CredentialType::BearerToken),
                    None,
                )
                .await
            }),
        ),
        (
            "delete_credential",
            Nothing,
            call(c.delete_credential("cred-1")),
        ),
        (
            "list_voices",
            Resource,
            call(async move { c.list_voices(&ListVoicesParams::new()).await }),
        ),
        ("get_voice", Resource, call(c.get_voice("achernar"))),
        (
            "create_voice",
            Resource,
            call(async move { c.create_voice(&CreateVoiceRequest::prompted("p")).await }),
        ),
        ("delete_voice", Nothing, call(c.delete_voice("voice_1"))),
        (
            "create_file_search_store",
            Resource,
            call(async move {
                c.create_file_search_store(&CreateFileSearchStoreRequest::new())
                    .await
            }),
        ),
        (
            "get_file_search_store",
            Resource,
            call(c.get_file_search_store(STORE)),
        ),
        (
            "list_file_search_stores",
            Resource,
            call(c.list_file_search_stores(None, None)),
        ),
        (
            "delete_file_search_store",
            Nothing,
            call(c.delete_file_search_store(STORE, true)),
        ),
        (
            "list_file_search_documents",
            Resource,
            call(c.list_file_search_documents(STORE, None, None)),
        ),
        (
            "get_file_search_document",
            Resource,
            call(c.get_file_search_document(DOC)),
        ),
        (
            "delete_file_search_document",
            Nothing,
            call(c.delete_file_search_document(DOC, true)),
        ),
        ("get_file", Resource, call(c.get_file("files/abc"))),
        ("list_files", Resource, call(c.list_files(None, None))),
        ("delete_file", Nothing, call(c.delete_file("files/abc"))),
        (
            "get_interaction",
            Resource,
            call(c.get_interaction("int-1")),
        ),
        (
            "get_interaction_with_input",
            Resource,
            call(c.get_interaction_with_input("int-1")),
        ),
        (
            "delete_interaction",
            Nothing,
            call(c.delete_interaction("int-1")),
        ),
        (
            "cancel_interaction",
            Resource,
            call(c.cancel_interaction("int-1")),
        ),
        // A stream's first item; a body without SSE frames is just empty.
        (
            "get_interaction_stream",
            Nothing,
            call(async move {
                match c.get_interaction_stream("int-1", None).next().await {
                    Some(item) => item.map(drop),
                    None => Ok(()),
                }
            }),
        ),
    ]
}

/// A valid trigger create body: a custom agent, non-empty input, no `store`.
fn trigger_params() -> TriggerCreateParams {
    TriggerCreateParams::new("0 5 * * *", "UTC", nightly_interaction())
}

fn nightly_interaction() -> InteractionRequest {
    InteractionRequest {
        agent: Some("agents/nightly".to_string()),
        input: InteractionInput::Text("Summarize overnight alerts".to_string()),
        ..Default::default()
    }
}

fn not_found() -> Reply {
    Reply::json(
        404,
        json!({"error": {"code": 404, "message": "Requested entity was not found.", "status": "NOT_FOUND"}}),
    )
    .header("x-goog-request-id", "req-404")
}

fn document(state: Option<&str>) -> Reply {
    let mut body = json!({"name": DOC, "displayName": "notes.txt", "mimeType": "text/plain"});
    if let Some(state) = state {
        body["state"] = json!(state);
    }
    Reply::json(200, body)
}

fn file(state: &str) -> Value {
    json!({"name": "files/abc", "mimeType": "video/mp4", "uri": "u", "state": state})
}

// =============================================================================
// Webhooks
// =============================================================================

#[tokio::test]
async fn webhook_endpoints_send_the_documented_requests() {
    let stub = ok_stub().await;
    let client = stub.client();
    let c = &client;
    assert_wire(
        &stub,
        vec![
            wire(
                "POST",
                "/v1beta/webhooks",
                Some(json!({
                    "uri": "https://example.com/hook",
                    "subscribed_events": ["interaction.completed", "batch.failed"],
                    "name": "hook"
                })),
                async move {
                    c.create_webhook(
                        &Webhook::new(
                            "https://example.com/hook",
                            vec![
                                WebhookEvent::InteractionCompleted,
                                WebhookEvent::BatchFailed,
                            ],
                        )
                        .with_name("hook"),
                    )
                    .await
                },
            ),
            wire("GET", "/v1beta/webhooks/wh-1", None, c.get_webhook("wh-1")),
            wire("GET", "/v1beta/webhooks", None, c.list_webhooks(None, None)),
            wire(
                "GET",
                "/v1beta/webhooks?page_size=5&page_token=a%2Fb%3D",
                None,
                c.list_webhooks(Some(5), Some("a/b=")),
            ),
            wire(
                "PATCH",
                "/v1beta/webhooks/wh-1?update_mask=uri%2Cstate",
                Some(json!({"uri": "https://example.com/v2", "state": "disabled"})),
                async move {
                    let update = WebhookUpdate::new()
                        .with_uri("https://example.com/v2")
                        .with_state(WebhookState::Disabled);
                    c.update_webhook("wh-1", &update, Some("uri,state")).await
                },
            ),
            wire(
                "PATCH",
                "/v1beta/webhooks/wh-1",
                Some(json!({"subscribed_events": ["video.generated"]})),
                async move {
                    let update = WebhookUpdate::new()
                        .with_subscribed_events(vec![WebhookEvent::VideoGenerated]);
                    c.update_webhook("wh-1", &update, None).await
                },
            ),
            wire(
                "DELETE",
                "/v1beta/webhooks/wh-1",
                None,
                c.delete_webhook("wh-1"),
            ),
            wire(
                "POST",
                "/v1beta/webhooks/wh-1:ping",
                Some(json!({})),
                c.ping_webhook("wh-1"),
            ),
            wire(
                "POST",
                "/v1beta/webhooks/wh-1:rotateSigningSecret",
                Some(json!({})),
                c.rotate_webhook_signing_secret("wh-1", None),
            ),
            wire(
                "POST",
                "/v1beta/webhooks/wh-1:rotateSigningSecret",
                Some(json!({"revocation_behavior": "revoke_previous_secrets_immediately"})),
                c.rotate_webhook_signing_secret(
                    "wh-1",
                    Some(RevocationBehavior::RevokePreviousSecretsImmediately),
                ),
            ),
        ],
    )
    .await;
}

#[cfg(not(feature = "strict-unknown"))]
#[tokio::test]
async fn webhook_create_response_parses_the_secret_and_preserves_unknowns() {
    let wire = json!({
        "id": "wh-1",
        "name": "hook",
        "uri": "https://example.com/hook",
        "subscribed_events": ["interaction.completed", "interaction.paused"],
        "state": "enabled",
        "new_signing_secret": "whsec_full",
        "signing_secrets": [{"truncated_secret": "whsec_...ab", "expire_time": "2026-09-25T00:00:00Z"}],
        "create_time": "2026-09-24T01:00:00Z",
        "update_time": "2026-09-24T01:00:00Z",
        "delivery_stats": {"failed": 0}
    });
    let stub = Stub::replying(vec![Reply::json(200, wire.clone())]).await;

    let webhook = stub
        .client()
        .create_webhook(&Webhook::new("https://example.com/hook", vec![]))
        .await
        .unwrap();

    assert_eq!(webhook.id.as_deref(), Some("wh-1"));
    assert_eq!(webhook.state, Some(WebhookState::Enabled));
    assert_eq!(webhook.new_signing_secret.as_deref(), Some("whsec_full"));
    assert_eq!(
        webhook.subscribed_events[0],
        WebhookEvent::InteractionCompleted
    );
    assert_eq!(
        webhook.subscribed_events[1].unknown_event_type(),
        Some("interaction.paused")
    );
    let secrets = webhook.signing_secrets.as_deref().unwrap();
    assert_eq!(secrets[0].truncated_secret.as_deref(), Some("whsec_...ab"));
    assert!(secrets[0].expire_time.is_some());
    assert!(webhook.create_time.is_some() && webhook.update_time.is_some());
    assert_eq!(webhook.extra["delivery_stats"], json!({"failed": 0}));

    // Unknown events and unmodeled fields serialize back as received.
    let back = serde_json::to_value(&webhook).unwrap();
    assert_eq!(back["subscribed_events"], wire["subscribed_events"]);
    assert_eq!(back["delivery_stats"], wire["delivery_stats"]);
}

#[cfg(not(feature = "strict-unknown"))]
#[tokio::test]
async fn webhook_list_keeps_the_page_token_and_drops_only_undeserializable_entries() {
    let stub = Stub::replying(vec![Reply::json(
        200,
        json!({
            "webhooks": [
                {"id": "wh-1", "uri": "https://a.example", "subscribed_events": [], "state": "paused_by_admin"},
                {"id": "wh-2", "uri": 42},
                {"id": "wh-3", "uri": "https://c.example", "state": "disabled_due_to_failed_deliveries"}
            ],
            "next_page_token": "page-2"
        }),
    )])
    .await;

    let list = stub.client().list_webhooks(Some(3), None).await.unwrap();

    let ids: Vec<_> = list
        .webhooks
        .iter()
        .filter_map(|w| w.id.as_deref())
        .collect();
    assert_eq!(ids, ["wh-1", "wh-3"]);
    assert_eq!(
        list.webhooks[0]
            .state
            .as_ref()
            .unwrap()
            .unknown_state_type(),
        Some("paused_by_admin")
    );
    assert_eq!(
        list.webhooks[1].state,
        Some(WebhookState::DisabledDueToFailedDeliveries)
    );
    assert_eq!(list.next_page_token.as_deref(), Some("page-2"));
}

#[cfg(not(feature = "strict-unknown"))]
#[tokio::test]
async fn rotate_signing_secret_sends_unknown_behaviors_verbatim_and_returns_the_secret() {
    let stub = Stub::replying(vec![Reply::json(200, json!({"secret": "whsec_rotated"}))]).await;
    let behavior: RevocationBehavior =
        serde_json::from_value(json!("revoke_previous_secrets_after_week")).unwrap();
    assert!(behavior.is_unknown());

    let rotated = stub
        .client()
        .rotate_webhook_signing_secret("wh-1", Some(behavior))
        .await
        .unwrap();

    assert_eq!(rotated.secret.as_deref(), Some("whsec_rotated"));
    let [request] = stub.requests().try_into().unwrap();
    assert_eq!(
        request.json(),
        json!({"revocation_behavior": "revoke_previous_secrets_after_week"})
    );
}

// =============================================================================
// Triggers
// =============================================================================

#[cfg(not(feature = "strict-unknown"))]
#[tokio::test]
async fn trigger_endpoints_send_the_documented_requests() {
    let stub = ok_stub().await;
    let client = stub.client();
    let c = &client;

    let interaction = nightly_interaction();
    let mut params = TriggerCreateParams::new("0 5 * * *", "Europe/Paris", interaction.clone())
        .with_display_name("nightly")
        .with_environment_id("env-1")
        .with_max_consecutive_failures(3)
        .with_execution_timeout_seconds(600);
    params.extra.insert("labels".into(), json!({"team": "ops"}));
    let mut unknown_update = TriggerUpdate::new()
        .with_status(serde_json::from_value::<TriggerStatus>(json!("archived")).unwrap());
    unknown_update
        .extra
        .insert("max_consecutive_failures".into(), json!("5"));

    assert_wire(
        &stub,
        vec![
            wire(
                "POST",
                "/v1beta/triggers",
                Some(json!({
                    "schedule": "0 5 * * *",
                    "time_zone": "Europe/Paris",
                    "interaction": serde_json::to_value(&interaction).unwrap(),
                    "display_name": "nightly",
                    "environment_id": "env-1",
                    "max_consecutive_failures": 3,
                    "execution_timeout_seconds": 600,
                    "labels": {"team": "ops"}
                })),
                async move { c.create_trigger(&params).await },
            ),
            wire("GET", "/v1beta/triggers/t-1", None, c.get_trigger("t-1")),
            wire(
                "GET",
                "/v1beta/triggers?page_size=10&page_token=p2",
                None,
                c.list_triggers(Some(10), Some("p2")),
            ),
            // No update_mask: the spec defines none for triggers.
            wire(
                "PATCH",
                "/v1beta/triggers/t-1",
                Some(json!({"display_name": "renamed", "status": "paused"})),
                async move {
                    let update = TriggerUpdate::new()
                        .with_display_name("renamed")
                        .with_status(TriggerStatus::Paused);
                    c.update_trigger("t-1", &update).await
                },
            ),
            wire(
                "PATCH",
                "/v1beta/triggers/t-1",
                Some(json!({"status": "archived", "max_consecutive_failures": "5"})),
                async move { c.update_trigger("t-1", &unknown_update).await },
            ),
            wire(
                "DELETE",
                "/v1beta/triggers/t-1",
                None,
                c.delete_trigger("t-1"),
            ),
            wire(
                "POST",
                "/v1beta/triggers/t-1/executions",
                Some(json!({})),
                c.run_trigger("t-1"),
            ),
            wire(
                "GET",
                "/v1beta/triggers/t-1/executions",
                None,
                c.list_trigger_executions("t-1", None, None),
            ),
            wire(
                "GET",
                "/v1beta/triggers/t-1/executions?page_size=2&page_token=next",
                None,
                c.list_trigger_executions("t-1", Some(2), Some("next")),
            ),
        ],
    )
    .await;
}

#[tokio::test]
async fn trigger_response_parses_string_counts_timestamp_aliases_and_sparse_interaction() {
    let stub = Stub::replying(vec![Reply::json(
        200,
        json!({
            "id": "t-1",
            "schedule": "0 5 * * *",
            "time_zone": "Europe/Paris",
            "display_name": "nightly",
            "status": "active",
            // A sparse projection: no `input`.
            "interaction": {"agent": "agents/nightly"},
            "max_consecutive_failures": "3",
            "consecutive_failure_count": "1",
            "execution_timeout_seconds": "600",
            "created": "2026-09-01T05:00:00Z",
            "updated": "2026-09-02T05:00:00Z",
            "next_run_time": "2026-09-25T03:00:00Z",
            "owner": "users/42"
        }),
    )])
    .await;

    let trigger = stub.client().get_trigger("t-1").await.unwrap();

    assert_eq!(trigger.status, Some(TriggerStatus::Active));
    assert_eq!(trigger.max_consecutive_failures, Some(3));
    assert_eq!(trigger.consecutive_failure_count, Some(1));
    assert_eq!(trigger.execution_timeout_seconds, Some(600));
    assert!(
        trigger.create_time.is_some() && trigger.update_time.is_some(),
        "the `created`/`updated` spellings alias the timestamps"
    );
    assert!(trigger.next_run_time.is_some());
    let interaction = trigger.interaction.as_ref().unwrap();
    assert_eq!(interaction.agent.as_deref(), Some("agents/nightly"));
    assert!(matches!(&interaction.input, InteractionInput::Text(t) if t.is_empty()));
    assert_eq!(trigger.extra["owner"], "users/42");
    // Counts go back out in the protobuf-JSON string form they came in.
    let back = serde_json::to_value(&trigger).unwrap();
    assert_eq!(back["max_consecutive_failures"], "3");
}

#[cfg(not(feature = "strict-unknown"))]
#[tokio::test]
async fn trigger_list_preserves_unknown_statuses() {
    let stub = Stub::replying(vec![
        Reply::json(
            200,
            json!({
                "triggers": [
                    {"id": "t-1", "status": "paused"},
                    {"id": "t-2", "status": "suspended_by_billing"}
                ],
                "next_page_token": "p2"
            }),
        ),
        // No triggers at all comes back as `{}`.
        Reply::json(200, json!({})),
    ])
    .await;
    let client = stub.client();

    let list = client.list_triggers(None, None).await.unwrap();
    assert_eq!(list.triggers[0].status, Some(TriggerStatus::Paused));
    let unknown = list.triggers[1].status.as_ref().unwrap();
    assert_eq!(unknown.unknown_status_type(), Some("suspended_by_billing"));
    assert_eq!(
        serde_json::to_value(unknown).unwrap(),
        "suspended_by_billing"
    );
    assert_eq!(list.next_page_token.as_deref(), Some("p2"));

    let empty = client.list_triggers(None, None).await.unwrap();
    assert!(empty.triggers.is_empty() && empty.next_page_token.is_none());
}

#[cfg(not(feature = "strict-unknown"))]
#[tokio::test]
async fn trigger_execution_responses_parse_under_both_list_keys() {
    let stub = Stub::replying(vec![
        Reply::json(
            200,
            json!({
                "id": "exec-1",
                "trigger_id": "t-1",
                "interaction_id": "int-9",
                "status": "queued",
                "scheduled_time": "2026-09-24T05:00:00Z",
                "attempt": 2
            }),
        ),
        Reply::json(
            200,
            json!({"trigger_executions": [{"id": "e1", "status": "completed"}], "next_page_token": "n"}),
        ),
        Reply::json(
            200,
            json!({"executions": [{"id": "e2", "status": "timed_out", "error": "deadline exceeded"}]}),
        ),
    ])
    .await;
    let client = stub.client();

    let run = client.run_trigger("t-1").await.unwrap();
    assert_eq!(run.interaction_id.as_deref(), Some("int-9"));
    assert_eq!(
        run.status.as_ref().unwrap().unknown_status_type(),
        Some("queued")
    );
    assert!(run.scheduled_time.is_some());
    assert_eq!(run.extra["attempt"], 2);

    let spec_key = client
        .list_trigger_executions("t-1", None, None)
        .await
        .unwrap();
    assert_eq!(
        spec_key.trigger_executions[0].status,
        Some(TriggerExecutionStatus::Completed)
    );
    assert_eq!(spec_key.next_page_token.as_deref(), Some("n"));

    let alias_key = client
        .list_trigger_executions("t-1", None, None)
        .await
        .unwrap();
    let execution = &alias_key.trigger_executions[0];
    assert_eq!(execution.status, Some(TriggerExecutionStatus::TimedOut));
    assert_eq!(execution.error.as_deref(), Some("deadline exceeded"));
}

// =============================================================================
// Agents
// =============================================================================

#[tokio::test]
async fn agent_endpoints_send_the_documented_requests() {
    let stub = ok_stub().await;
    let client = stub.client();
    let c = &client;
    assert_wire(
        &stub,
        vec![
            wire(
                "POST",
                "/v1beta/agents",
                Some(json!({
                    "id": "my-agent",
                    "base_agent": "agents/base",
                    "system_instruction": "Be brief.",
                    "description": "A test agent",
                    "tools": [{"type": "code_execution"}],
                    "base_environment": "env-1"
                })),
                async move {
                    let agent = Agent::new("my-agent")
                        .with_base_agent("agents/base")
                        .with_system_instruction("Be brief.")
                        .with_description("A test agent")
                        .add_tool(Tool::CodeExecution)
                        .with_base_environment("env-1");
                    c.create_agent(&agent).await
                },
            ),
            wire(
                "GET",
                "/v1beta/agents/my-agent",
                None,
                c.get_agent("my-agent"),
            ),
            wire(
                "GET",
                "/v1beta/agents",
                None,
                c.list_agents(None, None, None),
            ),
            wire(
                "GET",
                "/v1beta/agents?page_size=5&page_token=t&parent=projects%2Fp%201",
                None,
                c.list_agents(Some(5), Some("t"), Some("projects/p 1")),
            ),
            // With no paging, the filter takes the leading `?`.
            wire(
                "GET",
                "/v1beta/agents?parent=p",
                None,
                c.list_agents(None, None, Some("p")),
            ),
            wire(
                "DELETE",
                "/v1beta/agents/my-agent",
                None,
                c.delete_agent("my-agent"),
            ),
        ],
    )
    .await;
}

#[tokio::test]
async fn agent_responses_parse_tools_and_preserve_unknown_environments() {
    let agent = json!({
        "id": "my-agent",
        "base_agent": "agents/base",
        "tools": [{"type": "code_execution"}, {"type": "url_context"}],
        "base_environment": {"type": "gpu_sandbox", "accelerator": "l4"},
        "state": "ready"
    });
    let stub = Stub::replying(vec![
        Reply::json(200, agent.clone()),
        Reply::json(
            200,
            json!({"agents": [agent, {"id": "other", "base_environment": "env-9"}], "next_page_token": "p2"}),
        ),
    ])
    .await;
    let client = stub.client();

    let fetched = client.get_agent("my-agent").await.unwrap();
    assert!(matches!(
        fetched.tools.as_deref(),
        Some([Tool::CodeExecution, Tool::UrlContext])
    ));
    let env = fetched.base_environment.as_ref().unwrap();
    assert_eq!(env.unknown_environment_type(), Some("gpu_sandbox"));
    assert_eq!(fetched.extra["state"], "ready");
    let back = serde_json::to_value(&fetched).unwrap();
    assert_eq!(
        back["base_environment"],
        json!({"type": "gpu_sandbox", "accelerator": "l4"})
    );

    let list = client.list_agents(None, None, None).await.unwrap();
    assert_eq!(list.agents.len(), 2);
    assert_eq!(
        list.agents[1].base_environment,
        Some(EnvironmentSpec::Id("env-9".to_string()))
    );
    assert_eq!(list.next_page_token.as_deref(), Some("p2"));
}

// =============================================================================
// Environments and their files
// =============================================================================

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
                    c.create_environment(&request).await
                },
            ),
            // Fork: the whole body is the source environment.
            wire(
                "POST",
                "/v1beta/environments",
                Some(json!({"from_environment": "env-1"})),
                async move {
                    c.create_environment(&CreateEnvironmentRequest::from_environment("env-1"))
                        .await
                },
            ),
            wire(
                "POST",
                "/v1beta/environments",
                Some(json!({"network": "disabled"})),
                async move {
                    c.create_environment(
                        &CreateEnvironmentRequest::new().with_network(NetworkConfig::Disabled),
                    )
                    .await
                },
            ),
            wire(
                "GET",
                "/v1beta/environments/env-1",
                None,
                c.get_environment("env-1"),
            ),
            wire(
                "GET",
                "/v1beta/environments",
                None,
                c.list_environments(None, None),
            ),
            wire(
                "GET",
                "/v1beta/environments?page_size=50&page_token=p%2B2",
                None,
                c.list_environments(Some(50), Some("p+2")),
            ),
            wire(
                "DELETE",
                "/v1beta/environments/env-1",
                None,
                c.delete_environment("env-1"),
            ),
            wire(
                "GET",
                "/v1beta/environments/env-1/files/",
                None,
                c.list_environment_files("env-1", "", false, None, None),
            ),
            wire(
                "GET",
                "/v1beta/environments/env-1/files/src/",
                None,
                c.list_environment_files("env-1", "src/", false, None, None),
            ),
            wire(
                "GET",
                "/v1beta/environments/env-1/files/src/my%20file.py?page_size=5&page_token=t&recursive=true",
                None,
                c.list_environment_files("env-1", "/src/my file.py", true, Some(5), Some("t")),
            ),
        ],
    )
    .await;
}

#[cfg(not(feature = "strict-unknown"))]
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

    let env = stub.client().get_environment("env-1").await.unwrap();

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

    let list = client.list_environments(None, None).await.unwrap();
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

    let empty = client.list_environments(None, None).await.unwrap();
    assert!(empty.environments.is_empty());
}

#[cfg(not(feature = "strict-unknown"))]
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
        .list_environment_files("env-1", "", true, None, None)
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

#[tokio::test]
async fn upload_environment_file_starts_a_session_then_finalizes() {
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
        .upload_environment_file(
            "env-1",
            "data/in.csv",
            b"a,b\n1,2\n".to_vec(),
            "text/csv",
            EnvironmentFileUpload {
                overwrite: true,
                extract: true,
            },
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
async fn upload_environment_file_without_options_sends_no_query() {
    let stub = Stub::replying(vec![
        Reply::json(200, json!({})).header("x-goog-upload-url", "{base}/upload-session/2"),
        Reply::json(200, json!({"files": []})),
    ])
    .await;

    stub.client()
        .upload_environment_file(
            "env-1",
            "/notes/a b.txt",
            b"hi".to_vec(),
            "text/plain",
            EnvironmentFileUpload::default(),
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
async fn upload_environment_file_without_session_url_is_malformed_response() {
    let stub = Stub::replying(vec![Reply::json(200, json!({}))]).await;

    let err = stub
        .client()
        .upload_environment_file(
            "env-1",
            "a.txt",
            b"hi".to_vec(),
            "text/plain",
            EnvironmentFileUpload::default(),
        )
        .await
        .unwrap_err();

    assert!(matches!(err, GenaiError::MalformedResponse(_)), "{err:?}");
    assert!(err.to_string().contains("x-goog-upload-url"), "{err}");
    assert_eq!(stub.requests().len(), 1, "no bytes without a session");
}

#[tokio::test]
async fn upload_environment_file_start_rejection_stops_before_the_bytes() {
    let stub = Stub::replying(vec![Reply::json(
        409,
        json!({"error": {"message": "File already exists: a.txt", "code": "already_exists"}}),
    )])
    .await;

    let err = stub
        .client()
        .upload_environment_file(
            "env-1",
            "a.txt",
            b"hi".to_vec(),
            "text/plain",
            EnvironmentFileUpload::default(),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(&err, GenaiError::Api { status_code: 409, message, .. }
            if message == "already_exists: File already exists: a.txt"),
        "{err:?}"
    );
    assert_eq!(stub.requests().len(), 1);
}

// =============================================================================
// File search stores and documents
// =============================================================================

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
                        .with_extra("embeddingModel", "models/text-embedding");
                    c.create_file_search_store(&request).await
                },
            ),
            wire(
                "GET",
                "/v1beta/fileSearchStores/abc",
                None,
                c.get_file_search_store(STORE),
            ),
            wire(
                "GET",
                "/v1beta/fileSearchStores?page_size=20&page_token=t",
                None,
                c.list_file_search_stores(Some(20), Some("t")),
            ),
            wire(
                "DELETE",
                "/v1beta/fileSearchStores/abc",
                None,
                c.delete_file_search_store(STORE, false),
            ),
            wire(
                "DELETE",
                "/v1beta/fileSearchStores/abc?force=true",
                None,
                c.delete_file_search_store(STORE, true),
            ),
            wire(
                "GET",
                "/v1beta/fileSearchStores/abc/documents?page_size=2",
                None,
                c.list_file_search_documents(STORE, Some(2), None),
            ),
            wire(
                "GET",
                "/v1beta/fileSearchStores/abc/documents/doc-1",
                None,
                c.get_file_search_document(DOC),
            ),
            wire(
                "DELETE",
                "/v1beta/fileSearchStores/abc/documents/doc-1",
                None,
                c.delete_file_search_document(DOC, false),
            ),
            wire(
                "DELETE",
                "/v1beta/fileSearchStores/abc/documents/doc-1?force=true",
                None,
                c.delete_file_search_document(DOC, true),
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
        "activeDocumentsCount": "3"
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

    let fetched = client.get_file_search_store(STORE).await.unwrap();
    assert_eq!(fetched.name, "fileSearchStores/abc");
    assert_eq!(fetched.display_name.as_deref(), Some("Docs"));
    assert_eq!(
        fetched.embedding_model.as_deref(),
        Some("models/text-embedding")
    );
    assert!(fetched.create_time.is_some() && fetched.update_time.is_some());
    assert_eq!(fetched.extra["activeDocumentsCount"], "3");
    let back = serde_json::to_value(&fetched).unwrap();
    assert_eq!(back["displayName"], "Docs");
    assert_eq!(back["activeDocumentsCount"], "3");

    let list = client.list_file_search_stores(None, None).await.unwrap();
    let names: Vec<_> = list.stores.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["fileSearchStores/abc", "fileSearchStores/def"]);
    assert_eq!(list.next_page_token.as_deref(), Some("p2"));
}

#[cfg(not(feature = "strict-unknown"))]
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
        .list_file_search_documents(STORE, None, None)
        .await
        .unwrap();

    let active = &list.documents[0];
    assert_eq!(active.state, Some(DocumentState::Active));
    assert_eq!(active.size_bytes, Some(27));
    assert_eq!(active.mime_type.as_deref(), Some("text/plain"));
    assert_eq!(
        active.extra["customMetadata"],
        json!([{"key": "k", "stringValue": "v"}])
    );
    let archived = list.documents[1].state.as_ref().unwrap();
    assert_eq!(archived.unknown_state_type(), Some("STATE_ARCHIVED"));
    assert_eq!(serde_json::to_value(archived).unwrap(), "STATE_ARCHIVED");
    assert_eq!(list.next_page_token.as_deref(), Some("n"));
}

/// The API's upload answer: an operation naming the created document.
fn upload_operation() -> Reply {
    Reply::json(
        200,
        json!({
            "name": "fileSearchStores/abc/upload/operations/op-1",
            "response": {"documentName": DOC, "mimeType": "text/plain", "sizeBytes": "12"}
        }),
    )
}

fn temp_file(name: &str, contents: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    std::fs::write(&path, contents).unwrap();
    (dir, path)
}

#[tokio::test]
async fn upload_to_file_search_store_sends_raw_bytes_then_reads_the_document_back() {
    let stub = Stub::replying(vec![upload_operation(), document(Some("STATE_PENDING"))]).await;
    let (_dir, path) = temp_file("notes.txt", b"hello search");

    let doc = stub
        .client()
        .upload_to_file_search_store(STORE, &path, Some("My Doc"))
        .await
        .unwrap();
    assert_eq!(doc.name, DOC);
    assert_eq!(doc.state, Some(DocumentState::Pending));

    let [upload, read_back] = stub.requests().try_into().unwrap();
    assert_eq!(upload.method, "POST");
    assert_eq!(
        upload.target,
        "/upload/v1beta/fileSearchStores/abc:uploadToFileSearchStore?display_name=My%20Doc"
    );
    assert_eq!(upload.header("x-goog-api-key"), Some("test-key"));
    assert_eq!(upload.header("x-goog-upload-protocol"), Some("raw"));
    assert_eq!(upload.header("x-goog-upload-file-name"), Some("notes.txt"));
    assert_eq!(upload.header("content-type"), Some("text/plain"));
    assert_eq!(upload.body, b"hello search");
    assert_eq!(read_back.method, "GET");
    assert_eq!(
        read_back.target,
        "/v1beta/fileSearchStores/abc/documents/doc-1"
    );
}

#[tokio::test]
async fn upload_to_file_search_store_with_mime_and_no_display_name_sends_no_query() {
    let stub = Stub::replying(vec![upload_operation(), document(None)]).await;
    let (_dir, path) = temp_file("report.data", b"# Report");

    stub.client()
        .upload_to_file_search_store_with_mime(STORE, &path, None, "text/markdown")
        .await
        .unwrap();

    let requests = stub.requests();
    assert_eq!(
        requests[0].target,
        "/upload/v1beta/fileSearchStores/abc:uploadToFileSearchStore"
    );
    assert_eq!(requests[0].header("content-type"), Some("text/markdown"));
    assert_eq!(
        requests[0].header("x-goog-upload-file-name"),
        Some("report.data")
    );
}

#[tokio::test]
async fn upload_to_file_search_store_unresolved_operation_is_malformed_response() {
    let stub = Stub::replying(vec![Reply::json(
        200,
        json!({"name": "fileSearchStores/abc/upload/operations/op-7", "done": false}),
    )])
    .await;
    let (_dir, path) = temp_file("notes.txt", b"hello");

    let err = stub
        .client()
        .upload_to_file_search_store(STORE, &path, None)
        .await
        .unwrap_err();

    assert!(matches!(err, GenaiError::MalformedResponse(_)), "{err:?}");
    assert!(
        err.to_string().contains("operations/op-7"),
        "the operation name is the caller's only handle: {err}"
    );
    assert_eq!(stub.requests().len(), 1, "nothing to read back");
}

#[tokio::test]
async fn upload_to_file_search_store_read_back_failure_names_the_document() {
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
        let stub = Stub::replying(vec![upload_operation(), read_back]).await;
        let err = stub
            .client()
            .upload_to_file_search_store(STORE, &path, None)
            .await
            .unwrap_err();

        match (&err, status) {
            (GenaiError::Api { status_code, .. }, Some(expected)) => {
                assert_eq!(*status_code, expected, "{err:?}");
            }
            (GenaiError::Internal(_), None) => {}
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

#[tokio::test]
async fn upload_to_file_search_store_rejects_bad_input_before_any_request() {
    let stub = Stub::replying(vec![]).await;
    let client = stub.client();
    let (dir, text) = temp_file("notes.txt", b"hello");
    let empty = dir.path().join("empty.txt");
    std::fs::write(&empty, b"").unwrap();
    let unknown_ext = dir.path().join("blob.zzz");
    std::fs::write(&unknown_ext, b"x").unwrap();
    let missing = dir.path().join("missing.txt");

    let cases: Vec<(&str, Call<'_>)> = vec![
        (
            "empty file",
            call(client.upload_to_file_search_store(STORE, &empty, None)),
        ),
        (
            "missing file",
            call(client.upload_to_file_search_store(STORE, &missing, None)),
        ),
        (
            "unknown extension",
            call(client.upload_to_file_search_store(STORE, &unknown_ext, None)),
        ),
        (
            "unheaderable mime type",
            call(client.upload_to_file_search_store_with_mime(
                STORE,
                &text,
                None,
                "text/plain\nX-Injected: 1",
            )),
        ),
        (
            "bare store id",
            call(client.upload_to_file_search_store("abc", &text, None)),
        ),
    ];
    for (label, call) in cases {
        let err = call.await.unwrap_err();
        assert!(
            matches!(err, GenaiError::InvalidInput(_)),
            "{label}: {err:?}"
        );
    }
    assert!(stub.requests().is_empty());
}

async fn wait_for_document(
    replies: Vec<Reply>,
    timeout: Duration,
) -> (Result<FileSearchDocument, GenaiError>, Vec<String>) {
    let stub = Stub::replying(replies).await;
    let result = stub
        .client()
        .wait_for_document_active(DOC, Some(timeout), Some(Duration::from_millis(5)))
        .await;
    let targets = stub.requests().into_iter().map(|r| r.target).collect();
    (result, targets)
}

#[tokio::test]
async fn wait_for_document_active_polls_until_active() {
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
async fn wait_for_document_active_failed_state_is_terminal() {
    let (result, targets) =
        wait_for_document(vec![document(Some("STATE_FAILED"))], Duration::from_secs(5)).await;

    let err = result.unwrap_err();
    assert!(matches!(err, GenaiError::Internal(_)), "{err:?}");
    assert!(!err.is_retryable(), "a failed document never recovers");
    assert!(err.to_string().contains(DOC), "{err}");
    assert_eq!(targets.len(), 1, "no polling past a terminal state");
}

#[tokio::test]
async fn wait_for_document_active_times_out_with_the_last_state() {
    let stub = Stub::start(|_, _| document(Some("STATE_PENDING"))).await;

    let err = stub
        .client()
        .wait_for_document_active(
            DOC,
            Some(Duration::from_millis(60)),
            Some(Duration::from_millis(10)),
        )
        .await
        .unwrap_err();

    assert!(matches!(err, GenaiError::Internal(_)), "{err:?}");
    let message = err.to_string();
    assert!(message.contains("Timeout"), "{message}");
    assert!(message.contains("STATE_PENDING"), "{message}");
    assert!(
        stub.requests().len() >= 2,
        "it polled before giving up: {}",
        stub.requests().len()
    );
}

#[cfg(not(feature = "strict-unknown"))]
#[tokio::test]
async fn wait_for_document_active_keeps_polling_through_unknown_and_missing_states() {
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
async fn wait_for_document_active_propagates_api_errors() {
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

// =============================================================================
// Voices
// =============================================================================

#[tokio::test]
async fn voice_endpoints_send_the_documented_requests() {
    let stub = ok_stub().await;
    let client = stub.client();
    let c = &client;
    assert_wire(
        &stub,
        vec![
            wire(
                "GET",
                "/v1beta/voices",
                None,
                async move { c.list_voices(&ListVoicesParams::new()).await },
            ),
            wire(
                "GET",
                "/v1beta/voices?page_size=5&page_token=t%2F1&search=warm%20voice&gender=female\
                 &language_code=en-US&type=prebuilt&pitch=high",
                None,
                async move {
                    let params = ListVoicesParams::new()
                        .with_page_size(5)
                        .with_page_token("t/1")
                        .with_search("warm voice")
                        .with_voice_type(VoiceType::Prebuilt)
                        .with_gender("female")
                        .with_language_code("en-US")
                        .with_pitch(VoicePitch::High);
                    c.list_voices(&params).await
                },
            ),
            wire(
                "GET",
                "/v1beta/voices?region_code=US&accent=General%20American&persona=Narrator\
                 &context=Content%20%26%20Media",
                None,
                async move {
                    let params = ListVoicesParams {
                        region_code: Some("US".into()),
                        accent: Some("General American".into()),
                        persona: Some("Narrator".into()),
                        context: Some("Content & Media".into()),
                        ..Default::default()
                    };
                    c.list_voices(&params).await
                },
            ),
            wire("GET", "/v1beta/voices/achernar", None, c.get_voice("achernar")),
            wire(
                "POST",
                "/v1beta/voices",
                Some(json!({
                    "voice": {"type": "prompted", "prompted": {"input": "A warm storyteller."}, "display_name": "teller"},
                    "store": true
                })),
                async move {
                    let request =
                        CreateVoiceRequest::prompted("A warm storyteller.").with_display_name("teller");
                    c.create_voice(&request).await
                },
            ),
            wire(
                "POST",
                "/v1beta/voices",
                Some(json!({
                    "voice": {"type": "replicated", "replicated": {
                        "source_audio": {"data": "c3Jj", "mime_type": "audio/wav"},
                        "consent_audio": {"data": "Y25z", "mime_type": "audio/wav"}
                    }},
                    "store": false
                })),
                async move {
                    let request = CreateVoiceRequest::replicated(
                        VoiceAudio::new("c3Jj", "audio/wav"),
                        VoiceAudio::new("Y25z", "audio/wav"),
                    )
                    .with_store(false);
                    c.create_voice(&request).await
                },
            ),
            wire(
                "DELETE",
                "/v1beta/voices/voice_abc",
                None,
                c.delete_voice("voice_abc"),
            ),
        ],
    )
    .await;
}

#[cfg(not(feature = "strict-unknown"))]
#[tokio::test]
async fn voice_list_parses_prebuilt_voices_and_preserves_unknowns() {
    let stub = Stub::replying(vec![Reply::json(
        200,
        json!({
            "voices": [
                {"id": "achernar", "type": "prebuilt", "display_name": "Achernar", "language_code": "en-US",
                 "region_code": "US", "accent": "General American", "persona": "Storyteller & Narrator",
                 "context": "Content & Media", "gender": "female", "pitch": "high",
                 "description": "Soft, calm, and soothing voice with a higher pitch."},
                {"id": "voice_x", "type": "cloned", "pitch": "very_high", "quality": "hd"}
            ],
            "next_page_token": "p2"
        }),
    )])
    .await;

    let list = stub
        .client()
        .list_voices(&ListVoicesParams::new())
        .await
        .unwrap();

    let prebuilt = &list.voices[0];
    assert_eq!(prebuilt.voice_type, Some(VoiceType::Prebuilt));
    assert_eq!(prebuilt.pitch, Some(VoicePitch::High));
    assert_eq!(prebuilt.persona.as_deref(), Some("Storyteller & Narrator"));
    assert!(prebuilt.extra.is_empty());
    let custom = &list.voices[1];
    assert_eq!(
        custom.voice_type.as_ref().unwrap().unknown_voice_type(),
        Some("cloned")
    );
    assert_eq!(
        custom.pitch.as_ref().unwrap().unknown_pitch_type(),
        Some("very_high")
    );
    assert_eq!(custom.extra["quality"], "hd");
    let back = serde_json::to_value(custom).unwrap();
    assert_eq!(back["type"], "cloned");
    assert_eq!(list.next_page_token.as_deref(), Some("p2"));
}

#[tokio::test]
async fn created_voice_parses_the_prompted_shape() {
    let stub = Stub::replying(vec![Reply::json(
        200,
        json!({
            "id": "voice_3d8wi4xx4yxg",
            "model": "models/tts-model",
            "type": "prompted",
            "expire_time": "2027-09-24T01:44:20.854771486Z",
            "display_name": "robot",
            "prompted": {"input": "A calm, low-pitched robot narrator."},
            "usage": {"total_tokens": 1334},
            "sample_audio": {"data": "UklGRg==", "mime_type": "audio/wav"}
        }),
    )])
    .await;

    let voice = stub
        .client()
        .create_voice(&CreateVoiceRequest::prompted(
            "A calm, low-pitched robot narrator.",
        ))
        .await
        .unwrap();

    assert_eq!(voice.id.as_deref(), Some("voice_3d8wi4xx4yxg"));
    assert_eq!(voice.voice_type, Some(VoiceType::Prompted));
    assert!(voice.expire_time.is_some());
    assert_eq!(voice.usage.unwrap().total_tokens, Some(1334));
    assert_eq!(voice.sample_audio.unwrap().data, "UklGRg==");
}

// =============================================================================
// Credentials
// =============================================================================

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
                    c.create_credential(&request).await
                },
            ),
            wire(
                "POST",
                "/v1beta/credentials",
                Some(json!({
                    "type": "environment_variable",
                    "value": "v4lue",
                    "injection_location": ["header", "query"]
                })),
                async move {
                    let request = CreateCredentialRequest::environment_variable(
                        "v4lue",
                        vec![InjectionLocation::Header, InjectionLocation::Query],
                    );
                    c.create_credential(&request).await
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
                    c.create_credential(&request).await
                },
            ),
            wire(
                "GET",
                "/v1beta/credentials/gh-token",
                None,
                c.get_credential("gh-token"),
            ),
            wire(
                "GET",
                "/v1beta/credentials?page_size=10&page_token=n",
                None,
                c.list_credentials(Some(10), Some("n")),
            ),
            wire(
                "PATCH",
                "/v1beta/credentials/gh-token?update_mask=token",
                Some(json!({"type": "bearer_token", "token": "rotated"})),
                async move {
                    let mut update = CredentialUpdate::new(CredentialType::BearerToken);
                    update.token = Some("rotated".into());
                    c.update_credential("gh-token", &update, Some("token"))
                        .await
                },
            ),
            wire(
                "PATCH",
                "/v1beta/credentials/gh-token",
                Some(json!({"type": "environment_variable", "trusted_domains": ["api.example"]})),
                async move {
                    let mut update = CredentialUpdate::new(CredentialType::EnvironmentVariable);
                    update.trusted_domains = Some(vec!["api.example".into()]);
                    c.update_credential("gh-token", &update, None).await
                },
            ),
            wire(
                "DELETE",
                "/v1beta/credentials/gh-token",
                None,
                c.delete_credential("gh-token"),
            ),
        ],
    )
    .await;
}

#[cfg(not(feature = "strict-unknown"))]
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

    let list = stub.client().list_credentials(None, None).await.unwrap();

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
        let update = CredentialUpdate {
            value: Some(SECRETS[4].into()),
            ..CredentialUpdate::new(CredentialType::EnvironmentVariable)
        };
        client
            .update_credential("cred-1", &update, None)
            .await
            .unwrap();
        return;
    }

    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "loud_wire_redacts_credential_secrets",
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

// =============================================================================
// Interactions: get, delete, cancel, stream
// =============================================================================

#[tokio::test]
async fn interaction_endpoints_send_the_documented_requests() {
    let stub = ok_stub().await;
    let client = stub.client();
    let c = &client;
    assert_wire(
        &stub,
        vec![
            wire(
                "GET",
                "/v1beta/interactions/int-1",
                None,
                c.get_interaction("int-1"),
            ),
            wire(
                "GET",
                "/v1beta/interactions/int-1?include_input=true",
                None,
                c.get_interaction_with_input("int-1"),
            ),
            wire(
                "DELETE",
                "/v1beta/interactions/int-1",
                None,
                c.delete_interaction("int-1"),
            ),
            wire(
                "POST",
                "/v1beta/interactions/int-1/cancel",
                Some(json!({})),
                c.cancel_interaction("int-1"),
            ),
        ],
    )
    .await;
}

#[cfg(not(feature = "strict-unknown"))]
#[tokio::test]
async fn interaction_response_preserves_unknown_status_input_and_extras() {
    let stub = Stub::replying(vec![
        Reply::json(
            200,
            json!({
                "id": "int-1",
                "status": "paused_for_review",
                "model": "test-model",
                "input": "Hello",
                "steps": [{"type": "model_output", "content": [{"type": "text", "text": "Hi"}]}],
                "created": "2026-09-24T01:00:00Z",
                "region": "eu"
            }),
        ),
        Reply::json(200, json!({"id": "int-1", "status": "cancelled"})),
    ])
    .await;
    let client = stub.client();

    let response = client.get_interaction_with_input("int-1").await.unwrap();
    assert_eq!(
        response.status.unknown_status_type(),
        Some("paused_for_review")
    );
    assert!(matches!(&response.input, Some(InteractionInput::Text(t)) if t == "Hello"));
    assert_eq!(response.as_text(), Some("Hi"));
    assert!(response.created.is_some());
    assert_eq!(response.extra["region"], "eu");
    let back = serde_json::to_value(&response).unwrap();
    assert_eq!(back["status"], "paused_for_review");
    assert_eq!(back["region"], "eu");

    let cancelled = client.cancel_interaction("int-1").await.unwrap();
    assert_eq!(cancelled.status, InteractionStatus::Cancelled);
}

#[tokio::test]
async fn get_interaction_stream_without_resume_token_streams_the_lifecycle() {
    let stub = Stub::replying(vec![Reply::sse(&[
        "data: {\"event_type\":\"interaction.created\",\"interaction\":{\"id\":\"int-1\",\"status\":\"in_progress\"},\"event_id\":\"e1\"}\n\n",
        "data: {\"event_type\":\"step.start\",\"index\":0,\"step\":{\"type\":\"model_output\",\"content\":[]},\"event_id\":\"e2\"}\n\n",
        "data: {\"event_type\":\"step.delta\",\"index\":0,\"delta\":{\"type\":\"text\",\"text\":\"Hi\"},\"event_id\":\"e3\"}\n\n",
        "data: {\"event_type\":\"step.stop\",\"index\":0,\"event_id\":\"e4\"}\n\n",
        "data: {\"event_type\":\"interaction.completed\",\"interaction\":{\"id\":\"int-1\",\"status\":\"completed\"},\"event_id\":\"e5\"}\n\n",
    ])])
    .await;

    let events: Vec<_> = stub
        .client()
        .get_interaction_stream("int-1", None)
        .map(Result::unwrap)
        .collect()
        .await;

    let ids: Vec<_> = events
        .iter()
        .filter_map(|e| e.event_id.as_deref())
        .collect();
    assert_eq!(ids, ["e1", "e2", "e3", "e4", "e5"]);
    assert!(matches!(events[0].chunk, StreamChunk::Created { .. }));
    match &events[4].chunk {
        StreamChunk::Completed(response) => assert_eq!(response.as_text(), Some("Hi")),
        other => panic!("expected Completed, got {other:?}"),
    }

    let [request] = stub.requests().try_into().unwrap();
    assert_eq!(request.method, "GET");
    assert_eq!(
        request.target,
        "/v1beta/interactions/int-1?alt=sse&stream=true"
    );
    assert_eq!(request.header("x-goog-api-key"), Some("test-key"));
    assert_eq!(request.header("api-revision"), Some(API_REVISION));
}

#[tokio::test]
async fn get_interaction_stream_preserves_unknown_events() {
    let stub = Stub::replying(vec![Reply::sse(&[
        "data: {\"event_type\":\"interaction.paused\",\"reason\":\"maintenance\",\"event_id\":\"e9\"}\n\n",
    ])])
    .await;

    let events: Vec<_> = stub
        .client()
        .get_interaction_stream("int-1", Some("e8"))
        .collect()
        .await;

    let [Ok(event)] = events.as_slice() else {
        panic!("expected one event, got {events:?}");
    };
    assert_eq!(event.event_id.as_deref(), Some("e9"));
    assert_eq!(event.chunk.unknown_chunk_type(), Some("interaction.paused"));
    assert_eq!(event.chunk.unknown_data().unwrap()["reason"], "maintenance");
}

#[tokio::test]
async fn get_interaction_stream_http_error_is_the_only_item() {
    let stub = Stub::replying(vec![not_found()]).await;

    let events: Vec<_> = stub
        .client()
        .get_interaction_stream("int-1", None)
        .collect()
        .await;

    assert!(
        matches!(
            events.as_slice(),
            [Err(GenaiError::Api { status_code: 404, message, .. })]
                if message == "NOT_FOUND: Requested entity was not found."
        ),
        "{events:?}"
    );
}

// =============================================================================
// Files API: metadata, list, delete, wait
// =============================================================================

#[tokio::test]
async fn files_endpoints_send_the_documented_requests() {
    let stub = ok_stub().await;
    let client = stub.client();
    let c = &client;
    assert_wire(
        &stub,
        vec![
            wire("GET", "/v1beta/files/abc", None, c.get_file("files/abc")),
            wire("GET", "/v1beta/files", None, c.list_files(None, None)),
            // The Files API spells its paging parameters in camelCase.
            wire(
                "GET",
                "/v1beta/files?pageSize=10&pageToken=a%2Bb",
                None,
                c.list_files(Some(10), Some("a+b")),
            ),
            wire(
                "DELETE",
                "/v1beta/files/abc",
                None,
                c.delete_file("files/abc"),
            ),
        ],
    )
    .await;
}

#[cfg(not(feature = "strict-unknown"))]
#[tokio::test]
async fn list_files_parses_metadata_and_preserves_unknown_states() {
    let stub = Stub::replying(vec![Reply::json(
        200,
        json!({
            "files": [
                {"name": "files/abc", "displayName": "clip.mp4", "mimeType": "video/mp4", "sizeBytes": "1048576",
                 "createTime": "2026-09-24T01:00:00Z", "expirationTime": "2026-09-26T01:00:00Z",
                 "sha256Hash": "ZmFrZQ==", "uri": "https://generativelanguage.googleapis.com/v1beta/files/abc",
                 "state": "ACTIVE", "videoMetadata": {"videoDuration": "12s"}},
                {"name": "files/def", "mimeType": "text/plain", "state": "ARCHIVED"}
            ],
            "nextPageToken": "p2"
        }),
    )])
    .await;

    let list = stub.client().list_files(None, None).await.unwrap();

    let clip = &list.files[0];
    assert!(clip.is_active());
    assert_eq!(clip.display_name.as_deref(), Some("clip.mp4"));
    assert_eq!(clip.size_bytes_as_u64(), Some(1_048_576));
    assert!(clip.create_time.is_some() && clip.expiration_time.is_some());
    assert_eq!(
        clip.video_metadata
            .as_ref()
            .unwrap()
            .video_duration
            .as_deref(),
        Some("12s")
    );
    let archived = list.files[1].state.as_ref().unwrap();
    assert_eq!(archived.unknown_state_type(), Some("ARCHIVED"));
    assert!(list.files[1].uri.is_empty(), "uri defaults when absent");
    assert_eq!(list.next_page_token.as_deref(), Some("p2"));
}

async fn wait_for_file(stub: &Stub, timeout: Duration) -> Result<FileMetadata, GenaiError> {
    let metadata: FileMetadata = serde_json::from_value(file("PROCESSING")).unwrap();
    stub.client()
        .wait_for_file_ready(&metadata, Duration::from_millis(10), timeout)
        .await
}

#[tokio::test]
async fn wait_for_file_ready_times_out_with_the_last_state() {
    let stub = Stub::start(|_, _| Reply::json(200, file("PROCESSING"))).await;

    let err = wait_for_file(&stub, Duration::from_millis(60))
        .await
        .unwrap_err();

    assert!(matches!(err, GenaiError::Internal(_)), "{err:?}");
    let message = err.to_string();
    assert!(message.contains("Timeout"), "{message}");
    assert!(message.contains("Processing"), "{message}");
    assert!(stub.requests().len() >= 2, "it polled before giving up");
}

#[cfg(not(feature = "strict-unknown"))]
#[tokio::test]
async fn wait_for_file_ready_keeps_polling_through_unknown_states() {
    let stub = Stub::replying(vec![
        Reply::json(200, file("TRANSCODING")),
        Reply::json(200, file("ACTIVE")),
    ])
    .await;

    let ready = wait_for_file(&stub, Duration::from_secs(5)).await.unwrap();

    assert!(ready.is_active());
    assert_eq!(stub.requests().len(), 2);
}

#[tokio::test]
async fn wait_for_file_ready_propagates_api_errors() {
    let stub = Stub::replying(vec![Reply::json(200, file("PROCESSING")), not_found()]).await;

    let err = wait_for_file(&stub, Duration::from_secs(5))
        .await
        .unwrap_err();

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
    assert_eq!(stub.requests().len(), 2);
}

// =============================================================================
// IDs and resource names
// =============================================================================

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
                c.get_webhook("a/b?c#d"),
            ),
            // The colon verb stays outside the encoded ID.
            wire(
                "POST",
                "/v1beta/webhooks/wh%3A1:ping",
                Some(json!({})),
                c.ping_webhook("wh:1"),
            ),
            wire(
                "POST",
                "/v1beta/webhooks/wh%3A1:rotateSigningSecret",
                Some(json!({})),
                c.rotate_webhook_signing_secret("wh:1", None),
            ),
            wire(
                "DELETE",
                "/v1beta/webhooks/a%20b",
                None,
                c.delete_webhook("a b"),
            ),
            // Dots inside an ID are not dot segments.
            wire("GET", "/v1beta/webhooks/v1.2", None, c.get_webhook("v1.2")),
            wire("GET", "/v1beta/triggers/t%2F1", None, c.get_trigger("t/1")),
            wire(
                "POST",
                "/v1beta/triggers/t%2F1/executions",
                Some(json!({})),
                c.run_trigger("t/1"),
            ),
            wire(
                "GET",
                "/v1beta/triggers/t%3F1/executions?page_token=x%26y",
                None,
                c.list_trigger_executions("t?1", None, Some("x&y")),
            ),
            wire(
                "GET",
                "/v1beta/agents/team%2Fagent",
                None,
                c.get_agent("team/agent"),
            ),
            // An already-encoded traversal is encoded again, not decoded.
            wire(
                "DELETE",
                "/v1beta/agents/%252e%252e%252f",
                None,
                c.delete_agent("%2e%2e%2f"),
            ),
            wire(
                "GET",
                "/v1beta/environments/e%231",
                None,
                c.get_environment("e#1"),
            ),
            wire(
                "GET",
                "/v1beta/environments/e%2F1/files/dir%20name/a%3Fb",
                None,
                c.list_environment_files("e/1", "dir name/a?b", false, None, None),
            ),
            wire(
                "GET",
                "/v1beta/credentials/a%3Fb",
                None,
                c.get_credential("a?b"),
            ),
            wire(
                "GET",
                "/v1beta/voices/voice%2Fx",
                None,
                c.get_voice("voice/x"),
            ),
            wire(
                "GET",
                "/v1beta/interactions/int%2F1",
                None,
                c.get_interaction("int/1"),
            ),
            wire(
                "POST",
                "/v1beta/interactions/int%2F1/cancel",
                Some(json!({})),
                c.cancel_interaction("int/1"),
            ),
            wire(
                "DELETE",
                "/v1beta/interactions/int%3F1",
                None,
                c.delete_interaction("int?1"),
            ),
            wire(
                "GET",
                "/v1beta/interactions/int%2F1?alt=sse&stream=true&last_event_id=evt%2B1%26x",
                None,
                async move {
                    c.get_interaction_stream("int/1", Some("evt+1&x"))
                        .collect::<Vec<_>>()
                        .await
                        .into_iter()
                        .collect::<Result<Vec<_>, _>>()
                },
            ),
            wire(
                "GET",
                "/v1beta/fileSearchStores/a%20b",
                None,
                c.get_file_search_store("fileSearchStores/a b"),
            ),
            wire(
                "GET",
                "/v1beta/fileSearchStores/a%23b/documents",
                None,
                c.list_file_search_documents("fileSearchStores/a#b", None, None),
            ),
            wire(
                "GET",
                "/v1beta/fileSearchStores/a%20b/documents/c%3Fd",
                None,
                c.get_file_search_document("fileSearchStores/a b/documents/c?d"),
            ),
            wire("GET", "/v1beta/files/a%20b", None, c.get_file("files/a b")),
            wire(
                "DELETE",
                "/v1beta/files/a%3Fb",
                None,
                c.delete_file("files/a?b"),
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
    let data = || b"x".to_vec();
    let plain = EnvironmentFileUpload::default();
    let unnamed_file: FileMetadata =
        serde_json::from_value(json!({"name": "abc", "mimeType": "text/plain"})).unwrap();
    let unnamed_file = &unnamed_file;
    let second = Duration::from_secs(1);

    let cases: Vec<(&str, Call<'_>)> = vec![
        ("get_webhook empty", call(c.get_webhook(""))),
        ("get_webhook ..", call(c.get_webhook(".."))),
        (
            "update_webhook",
            call(async move { c.update_webhook("", &WebhookUpdate::new(), None).await }),
        ),
        ("delete_webhook", call(c.delete_webhook(""))),
        ("ping_webhook", call(c.ping_webhook("."))),
        (
            "rotate_webhook_signing_secret",
            call(c.rotate_webhook_signing_secret("", None)),
        ),
        ("get_trigger", call(c.get_trigger(""))),
        (
            "update_trigger",
            call(async move { c.update_trigger("%2E%2E", &TriggerUpdate::new()).await }),
        ),
        ("delete_trigger", call(c.delete_trigger(""))),
        ("run_trigger", call(c.run_trigger("."))),
        (
            "list_trigger_executions",
            call(c.list_trigger_executions("", None, None)),
        ),
        ("get_agent", call(c.get_agent(""))),
        ("delete_agent", call(c.delete_agent("%2e%2e"))),
        ("get_environment", call(c.get_environment(""))),
        ("delete_environment", call(c.delete_environment(".."))),
        (
            "list_environment_files id",
            call(c.list_environment_files("", "", false, None, None)),
        ),
        (
            "list_environment_files ..",
            call(c.list_environment_files("env-1", "a/../b", false, None, None)),
        ),
        (
            "list_environment_files .",
            call(c.list_environment_files("env-1", "./a", false, None, None)),
        ),
        (
            "upload_environment_file id",
            call(c.upload_environment_file("", "a.txt", data(), "text/plain", plain)),
        ),
        (
            "upload_environment_file ..",
            call(c.upload_environment_file("env-1", "../a.txt", data(), "text/plain", plain)),
        ),
        (
            "upload_environment_file empty data",
            call(c.upload_environment_file("env-1", "a.txt", Vec::new(), "text/plain", plain)),
        ),
        ("get_credential", call(c.get_credential(""))),
        (
            "update_credential",
            call(async move {
                c.update_credential(
                    "",
                    &CredentialUpdate::new(CredentialType::BearerToken),
                    None,
                )
                .await
            }),
        ),
        ("delete_credential", call(c.delete_credential("."))),
        ("get_voice", call(c.get_voice(""))),
        ("delete_voice", call(c.delete_voice(".."))),
        ("get_interaction", call(c.get_interaction(""))),
        (
            "get_interaction_with_input",
            call(c.get_interaction_with_input("..")),
        ),
        ("delete_interaction", call(c.delete_interaction(""))),
        ("cancel_interaction", call(c.cancel_interaction(""))),
        (
            "get_interaction_stream",
            call(async move {
                let items: Vec<_> = c.get_interaction_stream("", None).collect().await;
                assert_eq!(items.len(), 1, "the error is the stream's only item");
                items.into_iter().next().unwrap().map(drop)
            }),
        ),
        ("get_file dot", call(c.get_file("files/.."))),
        ("delete_file empty", call(c.delete_file("files/"))),
        (
            "wait_for_file_ready",
            call(c.wait_for_file_ready(unnamed_file, second, second)),
        ),
        (
            "get_file_search_store",
            call(c.get_file_search_store("fileSearchStores/")),
        ),
        (
            "delete_file_search_store",
            call(c.delete_file_search_store("fileSearchStores/..", true)),
        ),
        (
            "get_file_search_document",
            call(c.get_file_search_document("fileSearchStores/abc/documents/")),
        ),
        (
            "delete_file_search_document",
            call(c.delete_file_search_document("fileSearchStores//documents/doc-1", true)),
        ),
        (
            "wait_for_document_active",
            call(c.wait_for_document_active("fileSearchStores/abc/documents/..", None, None)),
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
        ("get_file", "bare id", call(c.get_file("abc"))),
        ("get_file", "extra segment", call(c.get_file("files/a/b"))),
        (
            "delete_file",
            "wrong prefix",
            call(c.delete_file("fileSearchStores/abc")),
        ),
        (
            "get_file_search_store",
            "bare id",
            call(c.get_file_search_store("abc")),
        ),
        (
            "get_file_search_store",
            "extra segment",
            call(c.get_file_search_store("fileSearchStores/abc/documents")),
        ),
        (
            "list_file_search_documents",
            "bare id",
            call(c.list_file_search_documents("abc", None, None)),
        ),
        (
            "get_file_search_document",
            "store name only",
            call(c.get_file_search_document(STORE)),
        ),
        (
            "get_file_search_document",
            "no store prefix",
            call(c.get_file_search_document("documents/doc-1")),
        ),
        (
            "get_file_search_document",
            "nested store",
            call(c.get_file_search_document("fileSearchStores/a/b/documents/doc-1")),
        ),
        (
            "delete_file_search_document",
            "nested document",
            call(c.delete_file_search_document("fileSearchStores/abc/documents/d/e", false)),
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

// =============================================================================
// Error mapping, on every endpoint
// =============================================================================

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
    assert!(unit_calls.len() >= 10, "every delete, ping, and the stream");
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
            "list_webhooks",
            Box::pin(async move {
                let l = c.list_webhooks(None, None).await?;
                Ok((l.webhooks.len(), l.next_page_token))
            }),
        ),
        (
            "list_triggers",
            Box::pin(async move {
                let l = c.list_triggers(None, None).await?;
                Ok((l.triggers.len(), l.next_page_token))
            }),
        ),
        (
            "list_trigger_executions",
            Box::pin(async move {
                let l = c.list_trigger_executions("t-1", None, None).await?;
                Ok((l.trigger_executions.len(), l.next_page_token))
            }),
        ),
        (
            "list_agents",
            Box::pin(async move {
                let l = c.list_agents(None, None, None).await?;
                Ok((l.agents.len(), l.next_page_token))
            }),
        ),
        (
            "list_environments",
            Box::pin(async move {
                let l = c.list_environments(None, None).await?;
                Ok((l.environments.len(), l.next_page_token))
            }),
        ),
        (
            "list_environment_files",
            Box::pin(async move {
                let l = c
                    .list_environment_files("env-1", "", false, None, None)
                    .await?;
                Ok((l.files.len(), l.next_page_token))
            }),
        ),
        (
            "list_credentials",
            Box::pin(async move {
                let l = c.list_credentials(None, None).await?;
                Ok((l.credentials.len(), l.next_page_token))
            }),
        ),
        (
            "list_voices",
            Box::pin(async move {
                let l = c.list_voices(&ListVoicesParams::new()).await?;
                Ok((l.voices.len(), l.next_page_token))
            }),
        ),
        (
            "list_file_search_stores",
            Box::pin(async move {
                let l = c.list_file_search_stores(None, None).await?;
                Ok((l.stores.len(), l.next_page_token))
            }),
        ),
        (
            "list_file_search_documents",
            Box::pin(async move {
                let l = c.list_file_search_documents(STORE, None, None).await?;
                Ok((l.documents.len(), l.next_page_token))
            }),
        ),
        (
            "list_files",
            Box::pin(async move {
                let l = c.list_files(None, None).await?;
                Ok((l.files.len(), l.next_page_token))
            }),
        ),
    ];
    for (name, page) in pages {
        let (len, token) = page.await.unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!((len, token), (0, None), "{name}");
    }
}

#[tokio::test]
async fn client_timeout_applies_to_resource_calls() {
    let stub =
        Stub::start(|_, _| Reply::json(200, json!({"id": "wh-1"})).delayed(Duration::from_secs(5)))
            .await;
    let client = genai_rs::Client::builder("test-key".to_string())
        .with_base_url(&stub.base_url)
        .with_timeout(Duration::from_millis(100))
        .build()
        .unwrap();

    let started = std::time::Instant::now();
    let err = client.get_webhook("wh-1").await.unwrap_err();

    assert!(
        matches!(err, GenaiError::Http(ref e) if e.is_timeout()),
        "{err:?}"
    );
    assert!(err.is_retryable());
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "the reply was never awaited in full: {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn base_url_prefix_applies_to_resource_and_upload_endpoints() {
    let stub = Stub::replying(vec![
        Reply::json(200, json!({})),
        Reply::json(200, json!({})).header("x-goog-upload-url", "{base}/session/1"),
        Reply::json(200, json!({"files": []})),
        upload_operation(),
        document(Some("STATE_PENDING")),
    ])
    .await;
    let client = genai_rs::Client::builder("test-key".to_string())
        .with_base_url(format!("{}/proxy/", stub.base_url))
        .build()
        .unwrap();
    let (_dir, path) = temp_file("notes.txt", b"hello");

    client.list_webhooks(None, None).await.unwrap();
    client
        .upload_environment_file(
            "env-1",
            "a.txt",
            b"hi".to_vec(),
            "text/plain",
            EnvironmentFileUpload::default(),
        )
        .await
        .unwrap();
    client
        .upload_to_file_search_store(STORE, &path, None)
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
            "/proxy/v1beta/fileSearchStores/abc/documents/doc-1",
        ]
    );
}
