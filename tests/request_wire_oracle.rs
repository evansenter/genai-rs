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

mod api_surface;
mod common;
mod live_verbs;

use api_surface::Surface;
use common::http_stub::{Recorded, Reply, Stub};
use genai_rs::{
    Agent, AllowlistEntry, CreateCredentialRequest, CreateEnvironmentRequest,
    CreateFileSearchStoreRequest, CreateVoiceRequest, CredentialConfig, CredentialType,
    CredentialUpdate, DeepResearchConfig, EnvVar, EnvironmentSource, FileUpload, GenerationConfig,
    HarmCategory, InjectionLocation, InteractionInput, InteractionRequest, NetworkConfig,
    RemoteEnvironment, ResponseFormat, ResponseFormatSpec, RevocationBehavior, SafetySetting,
    SafetyThreshold, SpeechConfig, ThinkingLevel, Tool, TriggerCreateParams, TriggerStatus,
    TriggerUpdate, VoiceAudio, Webhook, WebhookConfig, WebhookEvent, WebhookState, WebhookUpdate,
};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// Findings the bindings get wrong for the live API, each with its evidence.
const ALLOWED: &[(&str, &str)] = &[(
    "credentials.OAuth2Config.id",
    "Required in the bindings, optional live: a create without one gets a \
     server-assigned UUID (live 2026-11-04).",
)];

/// Verbs that make a POST, PATCH or PUT request with no JSON body, each with
/// its reason.
const NO_JSON_BODY: &[(&str, &str)] = &[
    (
        "EnvironmentFiles::upload",
        "A resumable upload: an empty PUT starts the session, then a POST \
         carries the file's bytes.",
    ),
    (
        "FileSearchStores::upload",
        "A raw-protocol upload: the file's bytes, with the display name as a \
         query parameter and the MIME type as the Content-Type.",
    ),
];

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
        input: InteractionInput::Content(vec![genai_rs::Content::text("hi")]),
        previous_interaction_id: Some("prev".into()),
        tools: Some(vec![
            Tool::CodeExecution,
            Tool::GoogleSearch { search_types: None },
            Tool::UrlContext,
        ]),
        response_modalities: Some(vec!["text".into()]),
        response_format: Some(ResponseFormatSpec::Single(ResponseFormat::Text {
            mime_type: Some("application/json".into()),
            schema: Some(json!({"type": "object"})),
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
    client
        .webhooks()
        .create(
            &Webhook::new(
                "https://example.com/hook",
                vec![WebhookEvent::InteractionCompleted],
            )
            .with_name("hook"),
        )
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
            &TriggerCreateParams::new("0 5 1 1 *", "UTC", interaction(true))
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
                .add_tool(Tool::CodeExecution)
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
        .create(&CreateFileSearchStoreRequest::new().with_display_name("docs"))
        .await
        .expect("file_search_stores.create");
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
    assert_eq!(bodies, 19, "one body per call above");
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
