//! Offline wire tests for every resource endpoint, against the local stub in
//! `common/http_stub.rs`.
//!
//! Each endpoint is pinned on its HTTP method, path (percent-encoding
//! included), query, and JSON body, and a realistic response is parsed into
//! the typed result with Evergreen `Unknown` and `extra` preservation. Error
//! mapping, the wait helpers' success, failure and timeout paths, and list
//! paging (`.pages()` / `.items()` across several stub pages) are covered
//! too.
//!
//! This file is the shared harness; the tests live one file per resource
//! (plus paging, ids and error mapping) in `tests/http_mock_resources/`.
//!
//! Tests whose replies carry unknown enum values are compiled out under
//! `genai_strict_unknown`, which rejects those values by design.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented,
    reason = "a test reports a failure by panicking"
)]

mod common;

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use common::http_stub::{Recorded, Reply, Stub, hold_clock};
use futures_util::{StreamExt, TryStreamExt};
use genai_rs::{
    Agent, AllowlistEntry, ChunkingConfig, CreateCredentialRequest, CreateEnvironmentRequest,
    CreateFileSearchStoreRequest, CreateTriggerRequest, CreateVoiceRequest, CredentialConfig,
    CredentialType, CredentialUpdate, CustomMetadata, DocumentImport, DocumentState,
    DocumentUpload, EnvironmentFileUpload, EnvironmentSource, EnvironmentSpec, EnvironmentStatus,
    FileMetadata, FileSearchDocument, FileUpload, GenaiError, InjectionLocation, InteractionInput,
    InteractionRequest, NetworkConfig, PollOptions, RevocationBehavior, StreamChunk, Tool, Trigger,
    TriggerExecution, TriggerStatus, TriggerUpdate, Voice, VoiceAudio, VoicePitch, VoiceType,
    Webhook, WebhookEvent, WebhookState, WebhookUpdate,
};
#[cfg(not(genai_strict_unknown))]
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
            "webhooks.create",
            Resource,
            call(async move {
                c.webhooks()
                    .create(&Webhook::new("https://example.com/h", vec![]))
                    .await
            }),
        ),
        ("webhooks.get", Resource, call(c.webhooks().get("wh-1"))),
        ("webhooks.list", Resource, call(c.webhooks().list().send())),
        (
            "webhooks.update",
            Resource,
            call(async move {
                c.webhooks()
                    .update("wh-1", &WebhookUpdate::new().with_name("n"))
                    .await
            }),
        ),
        (
            "webhooks.delete",
            Nothing,
            call(c.webhooks().delete("wh-1")),
        ),
        ("webhooks.ping", Nothing, call(c.webhooks().ping("wh-1"))),
        (
            "webhooks.rotate_signing_secret",
            Resource,
            call(c.webhooks().rotate_signing_secret("wh-1", None)),
        ),
        (
            "triggers.create",
            Resource,
            call(async move { c.triggers().create(&trigger_params()).await }),
        ),
        ("triggers.get", Resource, call(c.triggers().get("t-1"))),
        ("triggers.list", Resource, call(c.triggers().list().send())),
        (
            "triggers.update",
            Resource,
            call(async move {
                c.triggers()
                    .update("t-1", &TriggerUpdate::new().with_display_name("n"))
                    .await
            }),
        ),
        ("triggers.delete", Nothing, call(c.triggers().delete("t-1"))),
        ("triggers.run", Resource, call(c.triggers().run("t-1"))),
        (
            "triggers.list_executions",
            Resource,
            call(c.triggers().list_executions("t-1").send()),
        ),
        (
            "agents.create",
            Resource,
            call(async move { c.agents().create(&Agent::new("a", "base")).await }),
        ),
        ("agents.get", Resource, call(c.agents().get("a"))),
        ("agents.list", Resource, call(c.agents().list().send())),
        ("agents.delete", Nothing, call(c.agents().delete("a"))),
        (
            "environments.create",
            Resource,
            call(async move {
                c.environments()
                    .create(&CreateEnvironmentRequest::from_environment("env-0"))
                    .await
            }),
        ),
        (
            "environments.get",
            Resource,
            call(c.environments().get("env-1")),
        ),
        (
            "environments.list",
            Resource,
            call(c.environments().list().send()),
        ),
        (
            "environments.delete",
            Nothing,
            call(c.environments().delete("env-1")),
        ),
        (
            "environments.files.list",
            Resource,
            call(c.environments().files().list("env-1", "").send()),
        ),
        (
            "environments.files.upload",
            Resource,
            call(c.environments().files().upload(
                "env-1",
                "a.txt",
                EnvironmentFileUpload::new(b"x".to_vec(), "text/plain"),
            )),
        ),
        (
            "credentials.create",
            Resource,
            call(async move {
                c.credentials()
                    .create(&CreateCredentialRequest::bearer_token("t"))
                    .await
            }),
        ),
        (
            "credentials.get",
            Resource,
            call(c.credentials().get("cred-1")),
        ),
        (
            "credentials.list",
            Resource,
            call(c.credentials().list().send()),
        ),
        (
            "credentials.update",
            Resource,
            call(async move {
                c.credentials()
                    .update(
                        "cred-1",
                        &CredentialUpdate::new(CredentialType::BearerToken),
                    )
                    .await
            }),
        ),
        (
            "credentials.delete",
            Nothing,
            call(c.credentials().delete("cred-1")),
        ),
        ("voices.list", Resource, call(c.voices().list().send())),
        ("voices.get", Resource, call(c.voices().get("voice_1"))),
        (
            "voices.create",
            Resource,
            call(async move { c.voices().create(&CreateVoiceRequest::prompted("p")).await }),
        ),
        ("voices.delete", Nothing, call(c.voices().delete("voice_1"))),
        (
            "file_search_stores.create",
            Resource,
            call(async move {
                c.file_search_stores()
                    .create(&CreateFileSearchStoreRequest::new())
                    .await
            }),
        ),
        (
            "file_search_stores.get",
            Resource,
            call(c.file_search_stores().get(STORE)),
        ),
        (
            "file_search_stores.list",
            Resource,
            call(c.file_search_stores().list().send()),
        ),
        (
            "file_search_stores.delete",
            Nothing,
            call(c.file_search_stores().delete(STORE)),
        ),
        (
            "file_search_stores.force_delete",
            Nothing,
            call(c.file_search_stores().force_delete(STORE)),
        ),
        (
            "file_search_stores.documents.list",
            Resource,
            call(c.file_search_stores().documents().list(STORE).send()),
        ),
        (
            "file_search_stores.documents.get",
            Resource,
            call(c.file_search_stores().documents().get(DOC)),
        ),
        (
            "file_search_stores.documents.delete",
            Nothing,
            call(c.file_search_stores().documents().delete(DOC)),
        ),
        (
            "file_search_stores.documents.force_delete",
            Nothing,
            call(c.file_search_stores().documents().force_delete(DOC)),
        ),
        ("files.get", Resource, call(c.files().get("files/abc"))),
        ("files.list", Resource, call(c.files().list().send())),
        ("files.delete", Nothing, call(c.files().delete("files/abc"))),
        (
            "interactions.get",
            Resource,
            call(c.interactions().get("int-1")),
        ),
        (
            "interactions.get_with_input",
            Resource,
            call(c.interactions().get_with_input("int-1")),
        ),
        (
            "interactions.delete",
            Nothing,
            call(c.interactions().delete("int-1")),
        ),
        (
            "interactions.cancel",
            Resource,
            call(c.interactions().cancel("int-1")),
        ),
        // A stream's first item. A body without a terminal SSE event is a
        // cut-off stream, so it reads like a malformed resource.
        (
            "interactions.stream",
            Resource,
            call(async move {
                match c.interactions().stream("int-1").next().await {
                    Some(item) => item.map(drop),
                    None => Ok(()),
                }
            }),
        ),
    ]
}

/// A valid trigger create body: a custom agent, non-empty input, no `store`.
fn trigger_params() -> CreateTriggerRequest {
    CreateTriggerRequest::new("0 5 * * *", "UTC", nightly_interaction())
}

fn nightly_interaction() -> InteractionRequest {
    InteractionRequest {
        agent: Some("agents/nightly".to_string()),
        input: Some(InteractionInput::Text(
            "Summarize overnight alerts".to_string(),
        )),
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

fn targets(stub: &Stub) -> Vec<String> {
    stub.requests().into_iter().map(|r| r.target).collect()
}

fn temp_file(name: &str, contents: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    std::fs::write(&path, contents).unwrap();
    (dir, path)
}

// One file per resource (and paging, ids and error mapping) in
// tests/http_mock_resources/, sharing the harness above. A subdirectory, not
// tests/*.rs, so Cargo builds them into this one binary.
#[path = "http_mock_resources/agents.rs"]
mod agents;
#[path = "http_mock_resources/credentials.rs"]
mod credentials;
#[path = "http_mock_resources/environments.rs"]
mod environments;
#[path = "http_mock_resources/errors.rs"]
mod errors;
#[path = "http_mock_resources/file_search_stores.rs"]
mod file_search_stores;
#[path = "http_mock_resources/files.rs"]
mod files;
#[path = "http_mock_resources/ids.rs"]
mod ids;
#[path = "http_mock_resources/interactions.rs"]
mod interactions;
#[path = "http_mock_resources/paging.rs"]
mod paging;
#[path = "http_mock_resources/triggers.rs"]
mod triggers;
#[path = "http_mock_resources/voices.rs"]
mod voices;
#[path = "http_mock_resources/webhooks.rs"]
mod webhooks;
