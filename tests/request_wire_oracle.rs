//! The coverage oracle's request side, at the wire: every JSON body a
//! resource verb sends, captured by the local stub, has only keys its
//! google-genai binding class defines (`tests/api_surface/`).
//!
//! The in-crate oracle (`src/coverage_oracle/`) checks each request *type*
//! fully populated; this checks what each *verb* puts on the wire, wrapping
//! included (`{"file": {...}}` on an upload, the rotate body the crate builds
//! itself). Every verb that sends a body (one whose implementation reaches a
//! POST, PATCH or PUT request, found by `tests/live_verbs/`) must be called
//! here or listed in [`NO_JSON_BODY`] with its reason, and a body sent to an
//! endpoint `class_for` does not map fails the test, so a new verb with a
//! body fails here until it is added.
//!
//! The query string gets the same check: every key any verb sends is a query
//! parameter of its endpoint in the bindings (`bindings.json`'s `endpoints`)
//! or listed in [`QUERY_ALLOWED`], and every binding query parameter is sent
//! by some verb or listed in [`NOT_SENT`], each with its reason. The test
//! calls every verb of every resource handle, with every list builder
//! option set (both are checked), so a misspelled key, a key the API
//! dropped, and a parameter a new release adds all fail it, and so does a
//! request to a path no endpoint takes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test reports a failure by panicking"
)]

mod api_surface;
mod common;
mod live_verbs;

use api_surface::Surface;
use common::http_stub::{Recorded, Reply, Stub};
use futures_util::StreamExt;
use genai_rs::{
    Agent, AllowlistEntry, AntigravityConfig, ChunkingConfig, CreateCredentialRequest,
    CreateEnvironmentRequest, CreateFileSearchStoreRequest, CreateTriggerRequest,
    CreateVoiceRequest, CredentialConfig, CredentialType, CredentialUpdate, CustomMetadata,
    DeepResearchConfig, DocumentImport, DocumentUpload, EnvVar, EnvironmentFileUpload,
    EnvironmentSource, FileUpload, GenerationConfig, HarmCategory, InjectionLocation,
    InteractionInput, InteractionRequest, NetworkConfig, PollOptions, RemoteEnvironment,
    ResponseFormat, ResponseFormatSpec, RevocationBehavior, SafetySetting, SafetyThreshold,
    SigningSecret, SpeechConfig, ThinkingLevel, Tool, TriggerStatus, TriggerUpdate, VoiceAudio,
    VoicePitch, VoiceType, Webhook, WebhookConfig, WebhookEvent, WebhookState, WebhookUpdate,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

/// Findings the bindings get wrong for the live API, each with its evidence.
const ALLOWED: &[(&str, &str)] = &[
    (
        "credentials.OAuth2Config.id",
        "Required in the bindings, optional live: a create without one gets a \
         server-assigned UUID (live 2026-11-04).",
    ),
    (
        "legacy.ImportFileConfig.fileName",
        "google-genai's `import_file` takes `file_name` as its own argument and \
         sends it in the body beside the config's fields; the API requires it \
         there (live 2026-11-05).",
    ),
];

/// Verbs that make a POST, PATCH or PUT request with no JSON body, each with
/// its reason.
const NO_JSON_BODY: &[(&str, &str)] = &[(
    "EnvironmentFiles::upload",
    "A resumable upload: an empty PUT starts the session, then a POST \
         carries the file's bytes.",
)];

/// Stands for an endpoint whose binding method takes no request body. The
/// crate sends `{}` there, and nothing else may be sent.
const NO_CLASS: &str = "<no request class: the body must be {}>";

/// The binding class a captured body is sent as, by method and path.
fn class_for(request: &Recorded) -> Option<(&'static str, Value)> {
    let path = request.target.split('?').next().unwrap_or_default();
    let body = request.json();
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let class = match (request.method.as_str(), segments.as_slice()) {
        ("POST", ["v1beta", "interactions", _, "cancel"]) => NO_CLASS,
        ("POST", ["v1beta", "triggers", _, "executions"]) => NO_CLASS,
        ("POST", ["v1beta", "webhooks", id]) if id.ends_with(":ping") => {
            "webhooks.PingWebhookRequest"
        }
        ("POST", ["v1beta", "interactions"]) if body.get("agent").is_some() => {
            "interactions.CreateAgentInteraction"
        }
        ("POST", ["v1beta", "interactions"]) => "interactions.CreateModelInteraction",
        ("POST", ["v1beta", "webhooks"]) => "webhooks.WebhookInput",
        ("PATCH", ["v1beta", "webhooks", _]) => "webhooks.WebhookUpdate",
        ("POST", ["v1beta", "webhooks", id]) if id.ends_with(":rotateSigningSecret") => {
            "webhooks.RotateSigningSecretRequest"
        }
        ("POST", ["v1beta", "triggers"]) => "triggers.TriggerCreateParams",
        ("PATCH", ["v1beta", "triggers", _]) => "triggers.TriggerUpdate",
        ("POST", ["v1beta", "agents"]) => "agents.Agent",
        ("POST", ["v1beta", "environments"]) => "environments.CreateEnvironmentRequest",
        ("POST", ["v1beta", "credentials"]) => "credentials.CredentialCreateParams",
        ("PATCH", ["v1beta", "credentials", _]) => "credentials.CredentialUpdate",
        ("POST", ["v1beta", "voices"]) => "voices.CreateVoiceRequest",
        ("POST", ["v1beta", "fileSearchStores"]) => "legacy.CreateFileSearchStoreConfig",
        ("POST", ["v1beta", "fileSearchStores", id]) if id.ends_with(":importFile") => {
            "legacy.ImportFileConfig"
        }
        // The resumable upload's start request carries the document's settings.
        ("POST", ["upload", "v1beta", "fileSearchStores", id])
            if id.ends_with(":uploadToFileSearchStore") =>
        {
            "legacy.UploadToFileSearchStoreConfig"
        }
        // The resumable upload's start request wraps the file metadata.
        ("POST", ["upload", "v1beta", "files"]) => {
            return Some(("legacy.File", body["file"].clone()));
        }
        _ => return None,
    };
    Some((class, body))
}

/// Answers each request with a body its typed result parses from.
async fn stub() -> Stub {
    Stub::start(|request, _| {
        let path = request.target.split('?').next().unwrap_or_default();
        if path == "/upload/v1beta/files" {
            Reply::json(200, json!({})).header("x-goog-upload-url", "{base}/upload-session/f")
        } else if path.ends_with(":uploadToFileSearchStore") {
            Reply::json(200, json!({})).header("x-goog-upload-url", "{base}/upload-session/doc")
        } else if path == "/upload-session/doc" || path.ends_with(":importFile") {
            Reply::json(
                200,
                json!({"name": "fileSearchStores/s/operations/o", "done": true,
                       "response": {"documentName": "fileSearchStores/s/documents/d"}}),
            )
        } else if path.starts_with("/upload-session/") {
            Reply::json(
                200,
                json!({"file": {"name": "files/f", "mimeType": "text/plain", "uri": "u"}}),
            )
        } else if path.starts_with("/v1beta/interactions") {
            Reply::json(200, json!({"id": "i", "status": "completed", "steps": []}))
        } else {
            Reply::json(200, json!({"name": "n", "id": "x", "uri": "u"}))
        }
    })
    .await
}

fn interaction(agent: bool) -> InteractionRequest {
    InteractionRequest {
        model: (!agent).then(|| "test-model".to_string()),
        agent: agent.then(|| "test-agent".to_string()),
        agent_config: agent.then(|| {
            DeepResearchConfig::new()
                .with_thinking_summaries(genai_rs::ThinkingSummaries::Auto)
                .into()
        }),
        input: Some(InteractionInput::Content(vec![genai_rs::Content::text(
            "hi",
        )])),
        previous_interaction_id: Some("prev".into()),
        tools: Some(vec![
            Tool::code_execution(),
            Tool::GoogleSearch {
                search_types: None,
                extra: Default::default(),
            },
            Tool::url_context(),
        ]),
        response_modalities: Some(vec!["text".into()]),
        response_format: Some(ResponseFormatSpec::Single(ResponseFormat::Text {
            mime_type: Some("application/json".into()),
            schema: Some(json!({"type": "object"})),
            extra: Default::default(),
        })),
        generation_config: (!agent).then(|| GenerationConfig {
            max_output_tokens: Some(100),
            thinking_level: Some(ThinkingLevel::Low),
            seed: Some(7),
            stop_sequences: Some(vec!["STOP".into()]),
            speech_config: Some(vec![SpeechConfig {
                voice: Some("Kore".into()),
                language: Some("en-US".into()),
                speaker: None,
                ..Default::default()
            }]),
            ..Default::default()
        }),
        stream: None,
        background: Some(agent),
        store: Some(true),
        system_instruction: Some("be brief".into()),
        service_tier: None,
        webhook_config: Some(
            WebhookConfig::new()
                .with_uris(vec!["https://example.com/hook".into()])
                .with_user_metadata(json!({"k": "v"})),
        ),
        environment: agent.then(|| {
            RemoteEnvironment::new()
                .with_environment_id("e")
                .add_source(EnvironmentSource::inline("/a.txt", "x"))
                .add_env_var("K", EnvVar::value("v"))
                .into()
        }),
        safety_settings: Some(vec![SafetySetting::new(
            HarmCategory::Harassment,
            SafetyThreshold::BlockOnlyHigh,
        )]),
        labels: Some([("team".to_string(), "x".to_string())].into()),
        continuation_token: None,
        extra: serde_json::Map::new(),
    }
}

#[tokio::test]
async fn every_request_body_a_verb_sends_fits_its_binding_class() {
    let stub = stub().await;
    let client = stub.client();
    let network = || {
        NetworkConfig::allowlist(vec![
            AllowlistEntry::new("example.com").with_credential("credentials/c"),
        ])
    };

    client
        .execute(interaction(false))
        .await
        .expect("model interaction");
    client
        .execute(interaction(true))
        .await
        .expect("agent interaction");
    // A webhook read back from the API carries the output-only fields, which
    // the create body (the bindings' WebhookInput) does not take.
    let mut webhook = Webhook::new(
        "https://example.com/hook",
        vec![WebhookEvent::InteractionCompleted],
    )
    .with_name("hook");
    webhook.id = Some("wh-1".into());
    webhook.state = Some(WebhookState::Enabled);
    webhook.signing_secrets = Some(vec![SigningSecret::default()]);
    webhook.new_signing_secret = Some("secret".into());
    webhook.create_time = Some(chrono::DateTime::UNIX_EPOCH);
    webhook.update_time = Some(chrono::DateTime::UNIX_EPOCH);
    client
        .webhooks()
        .create(&webhook)
        .await
        .expect("webhooks.create");
    client
        .webhooks()
        .update(
            "wh-1",
            &WebhookUpdate::new()
                .with_name("hook")
                .with_uri("https://example.com/hook")
                .with_subscribed_events(vec![WebhookEvent::InteractionFailed])
                .with_state(WebhookState::Disabled)
                .with_update_mask("name"),
        )
        .await
        .expect("webhooks.update");
    client
        .webhooks()
        .rotate_signing_secret(
            "wh-1",
            Some(RevocationBehavior::RevokePreviousSecretsAfterH24),
        )
        .await
        .expect("webhooks.rotate_signing_secret");
    client
        .triggers()
        .create(
            &CreateTriggerRequest::new("0 5 1 1 *", "UTC", interaction(true))
                .with_display_name("daily")
                .with_environment_id("environments/e")
                .with_max_consecutive_failures(3)
                .with_execution_timeout_seconds(600),
        )
        .await
        .expect("triggers.create");
    client
        .triggers()
        .update(
            "t-1",
            &TriggerUpdate::new()
                .with_display_name("daily")
                .with_status(TriggerStatus::Paused),
        )
        .await
        .expect("triggers.update");
    client
        .agents()
        .create(
            &Agent::new("my-agent", "base")
                .with_system_instruction("be brief")
                .with_description("d")
                .add_tool(Tool::code_execution())
                .with_agent_config(
                    AntigravityConfig::new()
                        .with_model("m")
                        .with_max_total_tokens(1000),
                )
                .with_base_environment(RemoteEnvironment::new().with_network(network())),
        )
        .await
        .expect("agents.create");
    client
        .environments()
        .create(
            &CreateEnvironmentRequest::new()
                .add_source(EnvironmentSource::gcs("gs://b/d", "/data").with_encoding("raw"))
                .with_network(network()),
        )
        .await
        .expect("environments.create");
    client
        .credentials()
        .create(
            &CreateCredentialRequest::environment_variable(
                "v",
                vec![InjectionLocation::Header],
                vec!["example.com".into()],
            )
            .with_id("cred"),
        )
        .await
        .expect("credentials.create");
    client
        .credentials()
        .create(&CreateCredentialRequest::new(CredentialConfig::OAuth2 {
            client_id: "c".into(),
            client_secret: "s".into(),
            refresh_token: "r".into(),
            token_url: "https://example.com/token".into(),
            scopes: Some(vec!["read".into()]),
        }))
        .await
        .expect("credentials.create (oauth2)");
    let mut update = CredentialUpdate::new(CredentialType::BearerToken);
    update.token = Some("t".into());
    update.header_name = Some("X-Token".into());
    update.prefix = Some("Bearer".into());
    client
        .credentials()
        .update("cred", &update.with_update_mask("token"))
        .await
        .expect("credentials.update");
    client
        .voices()
        .create(
            &CreateVoiceRequest::prompted("a calm narrator")
                .with_display_name("narrator")
                .with_base_voice("Kore")
                .with_description("a calm narrator")
                .with_language_code("en-US")
                .with_region_code("US")
                .with_accent("General American")
                .with_persona("Storyteller & Narrator")
                .with_context("Content & Media")
                .with_gender("male")
                .with_pitch(VoicePitch::Low)
                .with_store(true),
        )
        .await
        .expect("voices.create");
    client
        .voices()
        .create(&CreateVoiceRequest::replicated(
            VoiceAudio::new("AAAA", "audio/wav"),
            VoiceAudio::new("AAAA", "audio/wav"),
        ))
        .await
        .expect("voices.create (replicated)");
    client
        .file_search_stores()
        .create(
            &CreateFileSearchStoreRequest::new()
                .with_display_name("docs")
                .with_embedding_model("models/gemini-embedding-2"),
        )
        .await
        .expect("file_search_stores.create");
    client
        .file_search_stores()
        .upload(
            "fileSearchStores/s",
            DocumentUpload::new(
                FileUpload::from_bytes(b"hi".to_vec(), "text/plain").with_display_name("d"),
            )
            .add_custom_metadata(CustomMetadata::string("team", "search"))
            .add_custom_metadata(CustomMetadata::numeric("year", 2026.0))
            .add_custom_metadata(CustomMetadata::string_list("tags", ["a", "b"]))
            .with_chunking_config(ChunkingConfig::white_space(200, 20)),
        )
        .await
        .expect("file_search_stores.upload");
    client
        .file_search_stores()
        .import_file(
            "fileSearchStores/s",
            DocumentImport::new("files/f")
                .add_custom_metadata(CustomMetadata::string("team", "search"))
                .with_chunking_config(ChunkingConfig::white_space(200, 20)),
        )
        .await
        .expect("file_search_stores.import_file");
    client
        .files()
        .upload(FileUpload::from_bytes(b"hi".to_vec(), "text/plain").with_display_name("f"))
        .await
        .expect("files.upload");
    client.webhooks().ping("wh-1").await.expect("webhooks.ping");
    client.triggers().run("t-1").await.expect("triggers.run");
    client
        .interactions()
        .cancel("i-1")
        .await
        .expect("interactions.cancel");

    let surface = Surface::bindings();
    let mut problems = Vec::new();
    let mut used = Vec::new();
    let mut bodies = 0;
    for request in stub.requests() {
        let is_json = request
            .header("content-type")
            .is_some_and(|t| t.starts_with("application/json"));
        if request.body.is_empty() || !is_json {
            continue;
        }
        let Some((class, body)) = class_for(&request) else {
            problems.push(format!(
                "{} {}: a JSON body with no binding class mapped in class_for()",
                request.method, request.target
            ));
            continue;
        };
        bodies += 1;
        if class == NO_CLASS {
            if body != json!({}) {
                problems.push(format!(
                    "{} {}: the bindings send no body here, but the crate sent {body}",
                    request.method, request.target
                ));
            }
            continue;
        }
        for finding in surface.check(&surface.ty(class), &body) {
            match ALLOWED.iter().find(|(site, _)| *site == finding.site) {
                Some((site, _)) => used.push(*site),
                None => {
                    problems.push(format!("{} {}: {finding}", request.method, request.target));
                }
            }
        }
    }
    assert_eq!(bodies, 21, "one body per call above");
    for (site, _) in ALLOWED {
        if !used.contains(site) {
            problems.push(format!(
                "stale ALLOWED entry, which matches nothing: {site}"
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "request bodies that do not fit google-genai {}:\n  {}",
        surface.version,
        problems.join("\n  ")
    );
}

/// Every verb that sends a body is called by the test above, or listed in
/// [`NO_JSON_BODY`]: a new verb with a body fails here until its body is
/// checked.
#[test]
fn every_verb_with_a_body_is_called_above() {
    let handles = live_verbs::handles();
    let with_body = live_verbs::body_verbs(&handles);
    assert!(
        with_body.len() > 15,
        "found only {} verbs with a body: {with_body:?}",
        with_body.len()
    );
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file!());
    let source = std::fs::read_to_string(&path).expect("read this file");
    let file = syn::parse_file(&source).expect("parse this file");
    let called = live_verbs::calls_from(
        &file,
        "every_request_body_a_verb_sends_fits_its_binding_class",
        &handles,
    );

    let key = |(handle, verb): &(String, String)| format!("{handle}::{verb}");
    let exempt: BTreeSet<&str> = NO_JSON_BODY.iter().map(|(verb, _)| *verb).collect();
    let mut problems = Vec::new();
    for verb in &with_body {
        let k = key(verb);
        match (called.contains(verb), exempt.contains(k.as_str())) {
            (false, false) => problems.push(format!(
                "{k} sends a body, but the wire test does not call it: call it \
                 there, or list it in NO_JSON_BODY with the reason"
            )),
            (true, true) => problems.push(format!("NO_JSON_BODY: {k} is called; delete the entry")),
            _ => {}
        }
    }
    for k in exempt {
        if !with_body.iter().any(|verb| key(verb) == k) {
            problems.push(format!(
                "NO_JSON_BODY: {k} is not a verb with a body; delete the entry"
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The discovery follows a verb through the functions it calls by name, and
/// leaves out a verb whose requests carry no body.
#[test]
fn verbs_with_a_body_are_found_through_helpers() {
    let file: syn::File = syn::parse_quote! {
        #[must_use = "a resource handle does nothing until you call one of its methods"]
        pub struct Things<'a> { client: &'a Client }
        impl Client {
            pub fn things(&self) -> Things<'_> { todo!() }
        }
        impl<'a> Things<'a> {
            pub async fn create(self, t: &Thing) -> Result<Thing, GenaiError> {
                crate::http::things::create_thing(&self.client.http, t).await
            }
            pub async fn get(self, id: &str) -> Result<Thing, GenaiError> {
                crate::http::things::get_thing(&self.client.http, id).await
            }
            pub async fn upload(self, data: Vec<u8>) -> Result<Thing, GenaiError> {
                start_upload(&self.client.http, data).await
            }
        }
        pub async fn create_thing(ctx: &HttpContext, t: &Thing) -> Result<Thing, GenaiError> {
            send_and_read(ctx, reqwest::Method::POST, &url(ctx), Some(t)).await
        }
        pub async fn get_thing(ctx: &HttpContext, id: &str) -> Result<Thing, GenaiError> {
            send_and_read(ctx, reqwest::Method::GET, &url(ctx), NO_BODY).await
        }
        async fn start_upload(ctx: &HttpContext, data: Vec<u8>) -> Result<Thing, GenaiError> {
            finish_upload(ctx.http_client.post(&url(ctx)).body(data)).await
        }
        #[cfg(test)]
        mod tests {
            fn get_thing() { let _ = reqwest::Method::PUT; }
        }
    };
    let files = [file];
    let handles = live_verbs::handles_in(&files);
    let found: BTreeSet<String> = live_verbs::body_verbs_in(&files, &handles)
        .into_iter()
        .map(|(handle, verb)| format!("{handle}::{verb}"))
        .collect();
    assert_eq!(
        found,
        BTreeSet::from(["Things::create".to_string(), "Things::upload".to_string()])
    );
}

/// The discovery follows method calls too: a verb that sends its body
/// through a method on `self`, or on a value it builds, is found. A method
/// on `self` is the `impl` type's own, so another type's method of the same
/// name that posts does not make a GET verb a verb with a body.
#[test]
fn verbs_with_a_body_are_found_through_methods() {
    let file: syn::File = syn::parse_quote! {
        #[must_use = "a resource handle does nothing until you call one of its methods"]
        pub struct Things<'a> { client: &'a Client }
        impl Client {
            pub fn things(&self) -> Things<'_> { todo!() }
        }
        impl<'a> Things<'a> {
            pub async fn rename(self, id: &str, name: &str) -> Result<Thing, GenaiError> {
                self.patch_thing(id, name).await
            }
            pub async fn archive(self, id: &str) -> Result<Thing, GenaiError> {
                let request = ArchiveRequest::new(id);
                request.submit(&self.client.http).await
            }
            pub async fn get(self, id: &str) -> Result<Thing, GenaiError> {
                self.fetch(id).await
            }
            async fn patch_thing(&self, id: &str, name: &str) -> Result<Thing, GenaiError> {
                send_and_read(&self.client.http, reqwest::Method::PATCH, &url(id), Some(name)).await
            }
            async fn fetch(&self, id: &str) -> Result<Thing, GenaiError> {
                send_and_read(&self.client.http, reqwest::Method::GET, &url(id), NO_BODY).await
            }
        }
        impl ArchiveRequest {
            pub(crate) async fn submit(&self, ctx: &HttpContext) -> Result<Thing, GenaiError> {
                send_and_read(ctx, reqwest::Method::POST, &url(&self.id), Some(self)).await
            }
        }
        impl Uploader {
            async fn fetch(&self, id: &str) -> Result<Thing, GenaiError> {
                send_and_read(&self.ctx, reqwest::Method::PUT, &url(id), Some(id)).await
            }
        }
    };
    let files = [file];
    let handles = live_verbs::handles_in(&files);
    let found: BTreeSet<String> = live_verbs::body_verbs_in(&files, &handles)
        .into_iter()
        .map(|(handle, verb)| format!("{handle}::{verb}"))
        .collect();
    assert_eq!(
        found,
        BTreeSet::from(["Things::archive".to_string(), "Things::rename".to_string()])
    );
}

// =============================================================================
// Query parameters
// =============================================================================

/// `alt` is a Google API system parameter, accepted on every method and
/// listed in no method's request class.
const ALT: &str = "`alt` is a Google API system parameter, which every method takes and no \
     request class lists: `alt=sse` asks for server-sent events (the live stream, resume and \
     create-stream tests read them) and `alt=media` for a file's bytes \
     (`EnvironmentFiles::download`, live in environment_files_tests).";

/// Query keys the crate sends that its endpoint's binding request does not
/// list, each with its reason, by site (`METHOD /path?key`).
const QUERY_ALLOWED: &[(&str, &str)] = &[
    ("GET /{}/environments/{}/files/{}?alt", ALT),
    ("GET /{}/interactions/{}?alt", ALT),
    ("POST /{}/interactions?alt", ALT),
];

/// Binding query parameters no verb sends, each with its reason, by site.
const NOT_SENT: &[(&str, &str)] = &[
    (
        "GET /{}/triggers?filter",
        "Not modeled: the API parses the filter, but every expression tried is a \
         `400 Invalid filter expression` and the syntax is undocumented (live \
         2026-11-05, docs/INTERACTIONS_API_GAP.md \"Resource residue\").",
    ),
    (
        "GET /{}/files/{}:download?alt",
        "No verb calls files.download: only generated files download, and an \
         uploaded one is a `400 Only GENERATED files can be downloaded` (live \
         2026-11-05; tests/ci_coverage.rs, ENDPOINTS_WITHOUT_A_VERB).",
    ),
    (
        "GET /{}/{clean_id}?alt",
        "No verb calls file_search_stores.download_media: it needs a `media_id` \
         from a `file_citation`, which the API had not served by 2026-11-05 \
         (tests/ci_coverage.rs, ENDPOINTS_WITHOUT_A_VERB).",
    ),
];

/// The bindings' endpoints (`METHOD /path`, path parameters as `{}`) and
/// each one's query parameters.
fn binding_endpoints() -> BTreeMap<String, BTreeSet<String>> {
    let path = api_surface::fixtures_dir().join("bindings.json");
    let text = std::fs::read_to_string(&path).expect("read bindings.json");
    let bindings: Value = serde_json::from_str(&text).expect("bindings.json is JSON");
    let endpoints = bindings["endpoints"]
        .as_object()
        .expect("bindings.json endpoints is an object");
    endpoints
        .iter()
        .map(|(key, endpoint)| {
            let query = endpoint["query"]
                .as_array()
                .unwrap_or_else(|| panic!("{key}: no query list"))
                .iter()
                .map(|name| name.as_str().expect("a query name").to_string())
                .collect();
            (key.clone(), query)
        })
        .collect()
}

/// The resource names a named path parameter takes, each with its source.
///
/// The dump spells a legacy resource name out from its class's collection
/// path; one it cannot place keeps its own name (`{operationName}`), and
/// stands for a full resource name. Matched against any path, such a
/// template would take every GET, so a request to no endpoint would never
/// be a finding: it matches only the names listed here, and a named
/// parameter not listed here matches nothing.
const FULL_NAMES: &[(&str, &[&str], &str)] = &[
    (
        "{clean_id}",
        &["fileSearchStores/{}/media/{}"],
        "file_search_stores.download_media's media ID: google-genai rejects one \
         without `/media/`, and names the route `GET /{name=fileSearchStores/*/media/*}`.",
    ),
    (
        "{operationName}",
        &[
            "fileSearchStores/{}/operations/{}",
            "fileSearchStores/{}/upload/operations/{}",
        ],
        "The operations the API serves: an import's and a store upload's \
         (docs/ENUM_WIRE_FORMATS.md, \"File Search\").",
    ),
];

/// How a template segment matches a request path's segments, with the
/// number of literal segments it matched; `None` when it does not.
///
/// `{}` is one segment, also inside one (`{}:importFile`); a named
/// parameter is one of the resource names [`FULL_NAMES`] gives it;
/// anything else is literal.
fn match_segments(template: &[&str], path: &[&str]) -> Option<usize> {
    let Some((head, rest)) = template.split_first() else {
        return path.is_empty().then_some(0);
    };
    if head.starts_with('{') && head.ends_with('}') && *head != "{}" {
        let (_, shapes, _) = FULL_NAMES.iter().find(|(name, ..)| name == head)?;
        return shapes
            .iter()
            .filter_map(|shape| {
                let spelled: Vec<&str> = shape.split('/').chain(rest.iter().copied()).collect();
                match_segments(&spelled, path)
            })
            .max();
    }
    let (segment, path_rest) = path.split_first()?;
    let literal = match head.split_once("{}") {
        Some((prefix, suffix)) => {
            let fits = segment.len() > prefix.len() + suffix.len()
                && segment.starts_with(prefix)
                && segment.ends_with(suffix);
            if !fits {
                return None;
            }
            usize::from(!(prefix.is_empty() && suffix.is_empty()))
        }
        None if head == segment => 1,
        None => return None,
    };
    match_segments(rest, path_rest).map(|n| n + literal)
}

/// The endpoints a request may have gone to: the templates for its method
/// that match its path with the most literal segments. Two that match it
/// equally well both come back, and a key either takes is its own.
fn endpoints_for<'a>(
    endpoints: impl IntoIterator<Item = &'a str>,
    method: &str,
    path: &str,
) -> Vec<&'a str> {
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let mut matches: Vec<(usize, &str)> = Vec::new();
    for endpoint in endpoints {
        let Some((m, template)) = endpoint.split_once(' ') else {
            continue;
        };
        if m != method {
            continue;
        }
        let template: Vec<&str> = template.trim_start_matches('/').split('/').collect();
        if let Some(score) = match_segments(&template, &segments) {
            matches.push((score, endpoint));
        }
    }
    let top = matches.iter().map(|(score, _)| *score).max();
    matches
        .into_iter()
        .filter(|(score, _)| Some(*score) == top)
        .map(|(_, endpoint)| endpoint)
        .collect()
}

/// Everything wrong with the query keys of `requests` (`(method, target)`)
/// against `endpoints`: a key its endpoint does not take, a parameter no
/// request sent, a request to no endpoint, and an entry in `allowed` or
/// `not_sent` that matches nothing.
fn query_findings(
    endpoints: &BTreeMap<String, BTreeSet<String>>,
    requests: &[(String, String)],
    allowed: &[(&str, &str)],
    not_sent: &[(&str, &str)],
) -> Vec<String> {
    let mut problems = Vec::new();
    let mut sent: BTreeSet<String> = BTreeSet::new();
    let mut used: BTreeSet<&str> = BTreeSet::new();
    for (method, target) in requests {
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        let candidates = endpoints_for(endpoints.keys().map(String::as_str), method, path);
        if candidates.is_empty() {
            problems.push(format!("{method} {target}: no endpoint in the bindings"));
            continue;
        }
        for pair in query.split('&').filter(|pair| !pair.is_empty()) {
            let key = pair.split('=').next().unwrap_or_default();
            let sites: Vec<String> = candidates.iter().map(|e| format!("{e}?{key}")).collect();
            let takes: Vec<&String> = candidates
                .iter()
                .zip(&sites)
                .filter(|(endpoint, _)| endpoints[**endpoint].contains(key))
                .map(|(_, site)| site)
                .collect();
            if !takes.is_empty() {
                sent.extend(takes.into_iter().cloned());
            } else if let Some((entry, _)) =
                allowed.iter().find(|(s, _)| sites.contains(&s.to_string()))
            {
                used.insert(entry);
            } else {
                problems.push(format!(
                    "{method} {target}: `{key}` is not a query parameter of {} \
                     (misspelled, or list it in QUERY_ALLOWED with the reason)",
                    candidates.join(" or ")
                ));
            }
        }
    }
    for (endpoint, params) in endpoints {
        for param in params {
            let site = format!("{endpoint}?{param}");
            let reasoned = not_sent.iter().find(|(s, _)| *s == site);
            match (sent.contains(&site), reasoned) {
                (false, None) => problems.push(format!(
                    "{site}: no verb sends this binding query parameter (send it, or \
                     list it in NOT_SENT with the reason)"
                )),
                (true, Some((entry, _))) => {
                    used.insert(entry);
                    problems.push(format!("NOT_SENT: {site} is sent; delete the entry"));
                }
                (false, Some((entry, _))) => {
                    used.insert(entry);
                }
                (true, None) => {}
            }
        }
    }
    for (site, _) in allowed.iter().chain(not_sent) {
        if !used.contains(site) {
            problems.push(format!("stale entry, which matches nothing: {site}"));
        }
    }
    problems
}

/// An active uploaded file, as `files.get` serves it.
fn active_file() -> Value {
    json!({"name": "files/f", "mimeType": "text/plain", "uri": "u", "state": "ACTIVE"})
}

/// Answers each verb's requests with a body its typed result parses from:
/// uploads get a session URL (`/upload-session/...`, not an endpoint), an
/// import an unfinished operation, `alt=sse` a stream.
async fn query_stub() -> Stub {
    Stub::start(|request, _| {
        let (path, query) = request
            .target
            .split_once('?')
            .unwrap_or((&request.target, ""));
        let session = |name: &str| {
            let url = format!("{{base}}/upload-session/{name}");
            Reply::json(200, json!({})).header("x-goog-upload-url", &url)
        };
        let operation = |done: bool| {
            let mut body = json!({"name": "fileSearchStores/s/operations/o", "done": done});
            if done {
                body["response"] = json!({"documentName": "fileSearchStores/s/documents/d"});
            }
            Reply::json(200, body)
        };
        if path == "/upload/v1beta/files" {
            session("file")
        } else if path.ends_with(":uploadToFileSearchStore") {
            session("doc")
        } else if path.starts_with("/upload/v1beta/environments/") {
            session("env")
        } else if path == "/upload-session/file" {
            Reply::json(200, json!({"file": active_file()}))
        } else if path == "/upload-session/env" {
            Reply::json(200, json!({"files": []}))
        } else if path == "/upload-session/doc" || path.ends_with("/operations/o") {
            operation(true)
        } else if path.ends_with(":importFile") {
            // Unfinished, so the import polls the operation.
            operation(false)
        } else if query.contains("alt=sse") {
            Reply::sse(&[
                concat!(
                    r#"data: {"event_type":"interaction.created","event_id":"e1","#,
                    r#""interaction":{"id":"i","status":"in_progress"}}"#,
                    "\n\n"
                ),
                concat!(
                    r#"data: {"event_type":"interaction.completed","event_id":"e2","#,
                    r#""interaction":{"id":"i","status":"completed"}}"#,
                    "\n\n"
                ),
            ])
        } else if query.contains("alt=media") {
            Reply::text(200, "x")
        } else if request.method == "DELETE" {
            Reply::json(200, json!({}))
        } else if path.starts_with("/v1beta/interactions") {
            Reply::json(200, json!({"id": "i", "status": "completed", "steps": []}))
        } else if path.starts_with("/v1beta/files/") {
            Reply::json(200, active_file())
        } else if path.starts_with("/v1beta/fileSearchStores/s/documents/") {
            Reply::json(
                200,
                json!({"name": "fileSearchStores/s/documents/d", "state": "STATE_ACTIVE"}),
            )
        } else {
            Reply::json(200, json!({"name": "n", "id": "x", "uri": "u"}))
        }
    })
    .await
}

/// A short poll, so the waits and the import's operation poll finish fast.
fn quick_poll() -> PollOptions {
    PollOptions::new()
        .with_poll_interval(Duration::from_millis(10))
        .with_timeout(Duration::from_secs(10))
}

/// Calls every verb of every resource handle, with every option it has set,
/// plus the interaction create and create-stream requests (not a handle's),
/// and checks every query key against the bindings.
#[tokio::test]
async fn every_query_key_a_verb_sends_is_a_binding_query_parameter() {
    let stub = query_stub().await;
    let client = stub.client();

    client.execute(interaction(false)).await.expect("execute");
    let events: Vec<_> = client.execute_stream(interaction(false)).collect().await;
    assert!(
        events.iter().all(Result::is_ok),
        "execute_stream: {events:?}"
    );

    client
        .interactions()
        .get("i")
        .await
        .expect("interactions.get");
    client
        .interactions()
        .get_with_input("i")
        .await
        .expect("interactions.get_with_input");
    let events: Vec<_> = client.interactions().stream("i").collect().await;
    assert!(
        events.iter().all(Result::is_ok),
        "interactions.stream: {events:?}"
    );
    let events: Vec<_> = client
        .interactions()
        .resume_stream("i", "e1")
        .collect()
        .await;
    assert!(
        events.iter().all(Result::is_ok),
        "interactions.resume_stream: {events:?}"
    );
    client
        .interactions()
        .cancel("i")
        .await
        .expect("interactions.cancel");
    client
        .interactions()
        .delete("i")
        .await
        .expect("interactions.delete");

    let webhook = Webhook::new(
        "https://example.com/hook",
        vec![WebhookEvent::InteractionCompleted],
    );
    client
        .webhooks()
        .create(&webhook)
        .await
        .expect("webhooks.create");
    client.webhooks().get("w").await.expect("webhooks.get");
    client
        .webhooks()
        .list()
        .with_page_size(5)
        .with_page_token("t")
        .send()
        .await
        .expect("webhooks.list");
    client
        .webhooks()
        .update(
            "w",
            &WebhookUpdate::new()
                .with_name("hook")
                .with_update_mask("name"),
        )
        .await
        .expect("webhooks.update");
    client.webhooks().ping("w").await.expect("webhooks.ping");
    client
        .webhooks()
        .rotate_signing_secret("w", None)
        .await
        .expect("webhooks.rotate_signing_secret");
    client
        .webhooks()
        .delete("w")
        .await
        .expect("webhooks.delete");

    client
        .triggers()
        .create(&CreateTriggerRequest::new(
            "0 5 1 1 *",
            "UTC",
            interaction(true),
        ))
        .await
        .expect("triggers.create");
    client.triggers().get("t").await.expect("triggers.get");
    client
        .triggers()
        .list()
        .with_page_size(5)
        .with_page_token("t")
        .send()
        .await
        .expect("triggers.list");
    client
        .triggers()
        .update(
            "t",
            &TriggerUpdate::new().with_status(TriggerStatus::Paused),
        )
        .await
        .expect("triggers.update");
    client.triggers().run("t").await.expect("triggers.run");
    client
        .triggers()
        .list_executions("t")
        .with_page_size(5)
        .with_page_token("t")
        .send()
        .await
        .expect("triggers.list_executions");
    client
        .triggers()
        .delete("t")
        .await
        .expect("triggers.delete");

    client
        .agents()
        .create(&Agent::new("my-agent", "base"))
        .await
        .expect("agents.create");
    client.agents().get("a").await.expect("agents.get");
    client
        .agents()
        .list()
        .with_page_size(5)
        .with_page_token("t")
        .with_parent("p")
        .send()
        .await
        .expect("agents.list");
    client.agents().delete("a").await.expect("agents.delete");

    client
        .environments()
        .create(&CreateEnvironmentRequest::new())
        .await
        .expect("environments.create");
    client
        .environments()
        .get("e")
        .await
        .expect("environments.get");
    client
        .environments()
        .list()
        .with_page_size(5)
        .with_page_token("t")
        .send()
        .await
        .expect("environments.list");
    // One-segment paths: a nested one spans segments a `{}` does not.
    client
        .environments()
        .files()
        .list("e", "data")
        .with_recursive(true)
        .with_page_size(5)
        .with_page_token("t")
        .send()
        .await
        .expect("environments.files.list");
    client
        .environments()
        .files()
        .download("e", "a.txt")
        .await
        .expect("environments.files.download");
    client
        .environments()
        .files()
        .upload(
            "e",
            "a.txt",
            EnvironmentFileUpload::new(b"x".to_vec(), "text/plain")
                .with_overwrite(true)
                .with_extract(true),
        )
        .await
        .expect("environments.files.upload");
    client
        .environments()
        .delete("e")
        .await
        .expect("environments.delete");

    client
        .credentials()
        .create(&CreateCredentialRequest::environment_variable(
            "v",
            vec![InjectionLocation::Header],
            vec!["example.com".into()],
        ))
        .await
        .expect("credentials.create");
    client
        .credentials()
        .get("c")
        .await
        .expect("credentials.get");
    client
        .credentials()
        .list()
        .with_page_size(5)
        .with_page_token("t")
        .send()
        .await
        .expect("credentials.list");
    let mut update = CredentialUpdate::new(CredentialType::BearerToken);
    update.token = Some("t".into());
    client
        .credentials()
        .update("c", &update.with_update_mask("token"))
        .await
        .expect("credentials.update");
    client
        .credentials()
        .delete("c")
        .await
        .expect("credentials.delete");

    client
        .voices()
        .create(&CreateVoiceRequest::prompted("a calm narrator"))
        .await
        .expect("voices.create");
    client.voices().get("v").await.expect("voices.get");
    client
        .voices()
        .list()
        .with_page_size(5)
        .with_page_token("t")
        .with_search("calm")
        .with_voice_type(VoiceType::Prompted)
        .with_gender("female")
        .with_language_code("en-US")
        .with_region_code("US")
        .with_accent("General American")
        .with_persona("Storyteller & Narrator")
        .with_context("Content & Media")
        .with_pitch(VoicePitch::Low)
        .send()
        .await
        .expect("voices.list");
    client.voices().delete("v").await.expect("voices.delete");

    client
        .files()
        .upload(FileUpload::from_bytes(b"hi".to_vec(), "text/plain"))
        .await
        .expect("files.upload");
    client.files().get("files/f").await.expect("files.get");
    client
        .files()
        .list()
        .with_page_size(5)
        .with_page_token("t")
        .send()
        .await
        .expect("files.list");
    client
        .files()
        .wait_until_active("files/f", quick_poll())
        .await
        .expect("files.wait_until_active");
    client
        .files()
        .delete("files/f")
        .await
        .expect("files.delete");

    client
        .file_search_stores()
        .create(&CreateFileSearchStoreRequest::new())
        .await
        .expect("file_search_stores.create");
    client
        .file_search_stores()
        .get("fileSearchStores/s")
        .await
        .expect("file_search_stores.get");
    client
        .file_search_stores()
        .list()
        .with_page_size(5)
        .with_page_token("t")
        .send()
        .await
        .expect("file_search_stores.list");
    client
        .file_search_stores()
        .upload(
            "fileSearchStores/s",
            FileUpload::from_bytes(b"hi".to_vec(), "text/plain"),
        )
        .await
        .expect("file_search_stores.upload");
    client
        .file_search_stores()
        .import_file(
            "fileSearchStores/s",
            DocumentImport::new("files/f").with_poll_options(quick_poll()),
        )
        .await
        .expect("file_search_stores.import_file");
    client
        .file_search_stores()
        .documents()
        .list("fileSearchStores/s")
        .with_page_size(5)
        .with_page_token("t")
        .send()
        .await
        .expect("documents.list");
    client
        .file_search_stores()
        .documents()
        .get("fileSearchStores/s/documents/d")
        .await
        .expect("documents.get");
    client
        .file_search_stores()
        .documents()
        .wait_until_active("fileSearchStores/s/documents/d", quick_poll())
        .await
        .expect("documents.wait_until_active");
    client
        .file_search_stores()
        .documents()
        .delete("fileSearchStores/s/documents/d")
        .await
        .expect("documents.delete");
    client
        .file_search_stores()
        .documents()
        .force_delete("fileSearchStores/s/documents/d")
        .await
        .expect("documents.force_delete");
    client
        .file_search_stores()
        .delete("fileSearchStores/s")
        .await
        .expect("file_search_stores.delete");
    client
        .file_search_stores()
        .force_delete("fileSearchStores/s")
        .await
        .expect("file_search_stores.force_delete");

    let requests: Vec<(String, String)> = stub
        .requests()
        .into_iter()
        // The session URL an upload's start request returned: not an endpoint.
        .filter(|r| !r.target.starts_with("/upload-session/"))
        .map(|r| (r.method, r.target))
        .collect();
    assert!(
        requests
            .iter()
            .any(|(_, target)| target == "/v1beta/fileSearchStores/s/operations/o"),
        "the import did not poll its operation"
    );
    let endpoints = binding_endpoints();
    let problems = query_findings(&endpoints, &requests, QUERY_ALLOWED, NOT_SENT);
    assert!(
        problems.is_empty(),
        "query strings that do not fit google-genai's endpoints:\n  {}",
        problems.join("\n  ")
    );
}

/// A list builder a handle verb returns: `(handle, verb)`.
type ListVerb = (String, String);

/// Every option of every list builder a handle verb returns, by the verb:
/// the paging pair each `paging::list_builder!` defines, and every public
/// method of the builder's own `impl` blocks that returns the builder.
fn list_builder_options(
    files: &[syn::File],
    handles: &live_verbs::Handles,
) -> BTreeMap<ListVerb, BTreeSet<String>> {
    fn flatten(items: &[syn::Item], out: &mut Vec<syn::Item>) {
        for item in items {
            if let syn::Item::Mod(m) = item
                && let Some((_, inner)) = &m.content
            {
                flatten(inner, out);
            }
            out.push(item.clone());
        }
    }
    fn last_ident(ty: &syn::Type) -> Option<String> {
        match ty {
            syn::Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
            _ => None,
        }
    }
    let mut items = Vec::new();
    for file in files {
        flatten(&file.items, &mut items);
    }
    // The builders: the struct each `list_builder!` invocation defines.
    let mut options: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for item in &items {
        let syn::Item::Macro(m) = item else { continue };
        if !m
            .mac
            .path
            .segments
            .last()
            .is_some_and(|s| s.ident == "list_builder")
        {
            continue;
        }
        // The ident after the top-level `struct`; a doc attribute is one
        // token tree, so a "struct" in its text is not read.
        let struct_name = |input: syn::parse::ParseStream| {
            let mut name = None;
            while !input.is_empty() {
                if name.is_none() && input.peek(syn::Token![struct]) {
                    input.parse::<syn::Token![struct]>()?;
                    name = Some(input.parse::<syn::Ident>()?);
                    continue;
                }
                input.step(|cursor| {
                    let (_, next) = cursor.token_tree().expect("not at the end");
                    Ok(((), next))
                })?;
            }
            name.ok_or_else(|| input.error("no struct"))
        };
        let name = syn::parse::Parser::parse2(struct_name, m.mac.tokens.clone())
            .expect("a list_builder! invocation defines a struct")
            .to_string();
        options.insert(
            name,
            BTreeSet::from(["with_page_size".into(), "with_page_token".into()]),
        );
    }
    let mut verbs: BTreeMap<ListVerb, String> = BTreeMap::new();
    for item in &items {
        let syn::Item::Impl(imp) = item else { continue };
        let Some(owner) = last_ident(&imp.self_ty).filter(|_| imp.trait_.is_none()) else {
            continue;
        };
        for item in &imp.items {
            let syn::ImplItem::Fn(f) = item else { continue };
            if f.sig.receiver().is_none() || !matches!(f.vis, syn::Visibility::Public(_)) {
                continue;
            }
            let syn::ReturnType::Type(_, ty) = &f.sig.output else {
                continue;
            };
            let Some(ret) = last_ident(ty) else { continue };
            let name = f.sig.ident.to_string();
            if let Some(own) = options.get_mut(&owner) {
                if ret == "Self" || ret == owner {
                    own.insert(name);
                }
            } else if handles.verbs.contains_key(&owner) && options.contains_key(&ret) {
                verbs.insert((owner.clone(), name), ret);
            }
        }
    }
    for builder in options.keys() {
        assert!(
            verbs.values().any(|b| b == builder),
            "no handle verb returns the list builder {builder}"
        );
    }
    verbs
        .into_iter()
        .map(|(verb, builder)| (verb, options[&builder].clone()))
        .collect()
}

/// The options the test function `test` sets on each list builder: every
/// method call on a chain that starts at a `Client` accessor
/// (`client.triggers().list().with_page_size(5)`), reaches a handle through
/// accessors, and that handle's list verb, so an option counts only on the
/// builder of the verb it follows.
fn options_set(
    file: &syn::File,
    test: &str,
    handles: &live_verbs::Handles,
    builders: &BTreeMap<ListVerb, BTreeSet<String>>,
) -> BTreeSet<(ListVerb, String)> {
    use syn::visit::Visit;
    struct Chains(Vec<Vec<String>>);
    impl<'ast> Visit<'ast> for Chains {
        fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
            let mut names = vec![call.method.to_string()];
            let mut receiver = &*call.receiver;
            loop {
                match receiver {
                    syn::Expr::MethodCall(inner) => {
                        names.push(inner.method.to_string());
                        receiver = &inner.receiver;
                    }
                    syn::Expr::Await(inner) => receiver = &inner.base,
                    _ => break,
                }
            }
            names.reverse();
            self.0.push(names);
            syn::visit::visit_expr_method_call(self, call);
        }
    }
    let function = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Fn(f) if f.sig.ident == test => Some(f),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no function {test}"));
    let mut chains = Chains(Vec::new());
    chains.visit_item_fn(function);

    let mut set = BTreeSet::new();
    for names in chains.0 {
        let mut ty = "Client".to_string();
        let mut builder: Option<ListVerb> = None;
        for name in names {
            match &builder {
                Some(verb) if builders[verb].contains(&name) => {
                    set.insert((verb.clone(), name));
                }
                Some(_) => break,
                None => {
                    let key = (ty.clone(), name);
                    if let Some(next) = handles.accessors.get(&key) {
                        ty.clone_from(next);
                    } else if builders.contains_key(&key) {
                        builder = Some(key);
                    } else {
                        break;
                    }
                }
            }
        }
    }
    set
}

/// Every verb of every resource handle is called by the query test above,
/// and every option of every list builder is set there, so every query
/// string the crate builds is checked. (A request type's own options, such
/// as `EnvironmentFileUpload`'s, are set there by hand.)
#[test]
fn every_verb_is_called_by_the_query_test() {
    const TEST: &str = "every_query_key_a_verb_sends_is_a_binding_query_parameter";
    let handles = live_verbs::handles();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file!());
    let source = std::fs::read_to_string(&path).expect("read this file");
    let file = syn::parse_file(&source).expect("parse this file");
    let called = live_verbs::calls_from(&file, TEST, &handles);
    let missing: Vec<String> = handles
        .verbs
        .iter()
        .flat_map(|(handle, verbs)| verbs.iter().map(move |verb| (handle, verb)))
        .filter(|(handle, verb)| !called.contains(&((*handle).clone(), (*verb).clone())))
        .map(|(handle, verb)| format!("{handle}::{verb}"))
        .collect();
    assert!(
        handles.verbs.len() >= 11,
        "parsed too few handles: {:?}",
        handles.verbs.keys()
    );
    assert!(
        missing.is_empty(),
        "verbs the query test does not call (call each, with every option set):\n  {}",
        missing.join("\n  ")
    );

    let builders = list_builder_options(&live_verbs::src_files(), &handles);
    assert!(
        builders.len() >= 11,
        "found too few list builders: {:?}",
        builders.keys()
    );
    let set = options_set(&file, TEST, &handles, &builders);
    let unset: Vec<String> = builders
        .iter()
        .flat_map(|(verb, options)| options.iter().map(move |option| (verb, option)))
        .filter(|(verb, option)| !set.contains(&((*verb).clone(), (*option).clone())))
        .map(|((handle, verb), option)| format!("{handle}::{verb}(..).{option}(..)"))
        .collect();
    assert!(
        unset.is_empty(),
        "list builder options the query test does not set (set each on its verb's \
         builder there):\n  {}",
        unset.join("\n  ")
    );
}

/// The builder discovery reads `list_builder!` and the builder's own
/// `impl`, and an option counts only on the builder of the verb it follows.
#[test]
fn list_builder_options_are_found_and_checked_per_verb() {
    let src: syn::File = syn::parse_quote! {
        #[must_use = "a resource handle does nothing until you call one of its methods"]
        pub struct Things<'a> { client: &'a Client }
        impl Client {
            pub fn things(&self) -> Things<'_> { todo!() }
        }
        impl<'a> Things<'a> {
            pub fn list(self) -> ListThings<'a> { todo!() }
            pub fn list_parts(self, id: &str) -> ListParts<'a> { todo!() }
            pub async fn get(self, id: &str) -> Result<Thing, GenaiError> { todo!() }
        }
        paging::list_builder! {
            /// Things: a struct Decoy in the docs is not the builder.
            pub struct ListThings<'a> {}
            page: ThingPage => Thing;
            label: "things";
        }
        paging::list_builder! {
            /// Parts.
            pub struct ListParts<'a> { id: String }
            page: PartPage => Part;
            label: "parts";
        }
        impl ListThings<'_> {
            pub fn with_filter(mut self, filter: impl Into<String>) -> Self { self }
            pub fn filter(&self) -> Option<&str> { None }
            fn with_hidden(self) -> Self { self }
        }
    };
    let files = [src];
    let handles = live_verbs::handles_in(&files);
    let builders = list_builder_options(&files, &handles);
    let things = ("Things".to_string(), "list".to_string());
    let parts = ("Things".to_string(), "list_parts".to_string());
    assert_eq!(
        builders[&things],
        BTreeSet::from([
            "with_filter".into(),
            "with_page_size".into(),
            "with_page_token".into()
        ])
    );
    assert_eq!(
        builders[&parts],
        BTreeSet::from(["with_page_size".into(), "with_page_token".into()])
    );

    let test: syn::File = syn::parse_quote! {
        async fn the_test() {
            client.things().list().with_page_size(5).send().await.expect("list");
            client.things().list_parts("p").with_filter("x").with_page_token("t").send().await;
            other.list().with_filter("x");
        }
    };
    let set = options_set(&test, "the_test", &handles, &builders);
    assert_eq!(
        set,
        BTreeSet::from([(things, "with_page_size".to_string())]),
        "with_filter is not an option of ListParts, so the chain stops there"
    );
}

/// A request maps to the endpoint whose template matches it most literally:
/// a store, its document, an operation by its full name, a custom verb; to
/// every template that matches it as well as that.
#[test]
fn a_request_maps_to_its_most_literal_endpoint() {
    let endpoints = [
        "GET /{}/fileSearchStores/{}",
        "GET /{}/fileSearchStores/{}/documents/{}",
        "GET /{}/{operationName}",
        "POST /{}/fileSearchStores/{}:importFile",
        "POST /{}/fileSearchStores",
    ];
    let find = |method, path| endpoints_for(endpoints, method, path);
    assert_eq!(
        find("GET", "/v1beta/fileSearchStores/s"),
        ["GET /{}/fileSearchStores/{}"]
    );
    assert_eq!(
        find("GET", "/v1beta/fileSearchStores/s/documents/d"),
        ["GET /{}/fileSearchStores/{}/documents/{}"]
    );
    assert_eq!(
        find("GET", "/v1beta/fileSearchStores/s/operations/o"),
        ["GET /{}/{operationName}"]
    );
    assert_eq!(
        find("POST", "/v1beta/fileSearchStores/s:importFile"),
        ["POST /{}/fileSearchStores/{}:importFile"]
    );
    assert!(find("POST", "/v1beta/fileSearchStores/s:other").is_empty());
    assert!(find("DELETE", "/v1beta/fileSearchStores/s").is_empty());
    // A full resource name matches only the names FULL_NAMES gives it, so a
    // path no endpoint takes maps to nothing.
    assert!(find("GET", "/v1beta/bogus/x").is_empty());
    assert!(find("GET", "/v1beta/fileSearchStores/s/operations").is_empty());
    assert!(find("GET", "/v1beta/fileSearchStores/s/documents/d/extra").is_empty());
    assert!(endpoints_for(["GET /{}/{unlisted}"], "GET", "/v1beta/a/b").is_empty());
    assert_eq!(
        endpoints_for(
            ["GET /{}/{clean_id}", "GET /{}/fileSearchStores/{}/media/{}"],
            "GET",
            "/v1beta/fileSearchStores/s/media/m"
        ),
        ["GET /{}/{clean_id}", "GET /{}/fileSearchStores/{}/media/{}"]
    );
}

/// Every named path parameter in the bindings has its resource names in
/// [`FULL_NAMES`], and every entry there names one: a new one would
/// otherwise take no request at all.
#[test]
fn every_named_path_parameter_has_its_resource_names() {
    let named: BTreeSet<String> = binding_endpoints()
        .keys()
        .flat_map(|endpoint| {
            endpoint
                .split(['/', ' '])
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .filter(|segment| segment.starts_with('{') && segment.ends_with('}') && segment != "{}")
        .collect();
    let listed: BTreeSet<String> = FULL_NAMES
        .iter()
        .map(|(name, ..)| (*name).to_string())
        .collect();
    assert_eq!(
        named, listed,
        "named path parameters in bindings.json (left) and FULL_NAMES (right) differ: \
         give a new one its resource names, and delete an entry the bindings dropped"
    );
}

/// The check fails on a misspelled key and on a parameter nothing sends,
/// and on reasoned entries that no longer match anything.
#[test]
fn a_misspelled_or_unsent_query_key_is_a_finding() {
    let endpoints: BTreeMap<String, BTreeSet<String>> = [(
        "GET /{}/things".to_string(),
        BTreeSet::from(["page_size".to_string(), "page_token".to_string()]),
    )]
    .into();
    let request = |target: &str| vec![("GET".to_string(), target.to_string())];

    let clean = query_findings(
        &endpoints,
        &request("/v1beta/things?page_size=5&page_token=t"),
        &[],
        &[],
    );
    assert!(clean.is_empty(), "{clean:?}");

    let misspelled = query_findings(
        &endpoints,
        &request("/v1beta/things?page_sise=5&page_token=t"),
        &[],
        &[],
    );
    assert_eq!(misspelled.len(), 2, "{misspelled:?}");
    assert!(misspelled[0].contains("`page_sise` is not a query parameter"));
    assert!(misspelled[1].starts_with("GET /{}/things?page_size: no verb sends"));

    let reasoned = query_findings(
        &endpoints,
        &request("/v1beta/things?alt=sse&page_token=t"),
        &[("GET /{}/things?alt", "a system parameter")],
        &[("GET /{}/things?page_size", "not modeled")],
    );
    assert!(reasoned.is_empty(), "{reasoned:?}");

    let stale = query_findings(
        &endpoints,
        &request("/v1beta/things?page_size=5&page_token=t"),
        &[("GET /{}/things?alt", "a system parameter")],
        &[("GET /{}/things?page_size", "not modeled")],
    );
    assert_eq!(
        stale,
        [
            "NOT_SENT: GET /{}/things?page_size is sent; delete the entry",
            "stale entry, which matches nothing: GET /{}/things?alt",
        ]
    );

    let nowhere = query_findings(&endpoints, &request("/v1beta/other"), &[], &[]);
    assert!(
        nowhere[0].contains("no endpoint in the bindings"),
        "{nowhere:?}"
    );

    // A template that takes a full resource name does not swallow a GET
    // to no endpoint.
    let mut with_operations = endpoints.clone();
    with_operations.insert("GET /{}/{operationName}".to_string(), BTreeSet::new());
    let nowhere = query_findings(
        &with_operations,
        &[
            (
                "GET".to_string(),
                "/v1beta/things?page_size=5&page_token=t".to_string(),
            ),
            ("GET".to_string(), "/v1beta/bogus/x".to_string()),
        ],
        &[],
        &[],
    );
    assert_eq!(
        nowhere,
        ["GET /v1beta/bogus/x: no endpoint in the bindings"]
    );
}
