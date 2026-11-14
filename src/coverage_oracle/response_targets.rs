//! The response types the response oracle checks ([`super::response`],
//! [`super::response_minimal`]), each with the binding class and spec schema
//! it is read from, and the probes that deserialize a body as that type and
//! serialize it again.

use std::fmt::Debug;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

pub(super) type Probe = fn(Value) -> Result<(String, Value), String>;

/// Deserializes, then returns the `{:?}` rendering and the value serialized
/// again.
pub(super) fn probe<T: DeserializeOwned + Serialize + Debug>(
    value: Value,
) -> Result<(String, Value), String> {
    let parsed: T = serde_json::from_value(value).map_err(|e| e.to_string())?;
    let back = serde_json::to_value(&parsed).map_err(|e| e.to_string())?;
    Ok((format!("{parsed:?}"), back))
}

/// A stream event the way the streaming path reads it: parsed as the wire
/// event, then dispatched to the public [`StreamEvent`]. Serialized again,
/// a `StreamEvent` uses the crate's own `{chunk_type, data, extra}`
/// envelope, which is mapped back to the wire shape for the comparison:
/// the chunk's `extra` (the event's keys it does not read) goes back beside
/// the payload, and what it kept of a `metadata` or `error` object back
/// into that object, so a key the chunk kept is not reported dropped.
pub(super) fn probe_stream_event(value: Value) -> Result<(String, Value), String> {
    use crate::http::interactions::dispatch_stream_event;
    use crate::steps::StepAccumulator;
    use crate::wire_streaming::{InteractionStreamEvent, StreamEvent};

    let event: InteractionStreamEvent = serde_json::from_value(value).map_err(|e| e.to_string())?;
    let event_id = event.event_id.clone();
    let chunk =
        dispatch_stream_event(event, &mut StepAccumulator::new()).ok_or("the event was dropped")?;
    let public = StreamEvent::new(chunk, event_id);
    let envelope = serde_json::to_value(&public).map_err(|e| e.to_string())?;
    let mut data = envelope.get("data").cloned().unwrap_or(Value::Null);
    let (event_type, wrap) = match envelope["chunk_type"].as_str().unwrap_or_default() {
        "created" => ("interaction.created", Some("interaction")),
        "completed" => ("interaction.completed", Some("interaction")),
        "status_update" => ("interaction.status_update", None),
        "step_start" => ("step.start", None),
        "step_delta" => ("step.delta", None),
        "step_stop" => ("step.stop", None),
        "error" => ("error", Some("error")),
        other => return Err(format!("unexpected chunk type {other:?}")),
    };
    if let Some(key) = wrap {
        data = json!({ key: data });
    }
    // A step.delta's usage rides in the event's `metadata`.
    if event_type == "step.delta"
        && let Some(usage) = data.as_object_mut().and_then(|d| d.remove("total_usage"))
    {
        data["metadata"] = json!({ "total_usage": usage });
    }
    let mut wire = data.as_object().cloned().unwrap_or_default();
    if let Some(Value::Object(extra)) = envelope.get("extra") {
        for (key, kept) in extra {
            match (wire.get_mut(key), kept) {
                (Some(Value::Object(read)), Value::Object(rest)) => {
                    read.extend(rest.iter().map(|(k, v)| (k.clone(), v.clone())));
                }
                // The chunk read this key whole; its value is the one to keep.
                (Some(_), _) => {}
                (None, _) => {
                    wire.insert(key.clone(), kept.clone());
                }
            }
        }
    }
    wire.insert("event_type".into(), json!(event_type));
    if let Some(id) = envelope.get("event_id") {
        wire.insert("event_id".into(), id.clone());
    }
    Ok((format!("{public:?}"), Value::Object(wire)))
}

/// A response type and the class it is read from, in the bindings and the
/// spec.
pub(super) struct Target {
    pub(super) name: &'static str,
    pub(super) binding: Option<&'static str>,
    pub(super) spec: Option<&'static str>,
    pub(super) probe: Probe,
}

pub(super) fn targets() -> Vec<Target> {
    use crate::agents::{Agent, AgentListResponse};
    use crate::credentials::{Credential, CredentialListResponse};
    use crate::decisions::CreateDecisionResponse;
    use crate::environments::{
        Environment, EnvironmentFile, EnvironmentFileListResponse, EnvironmentListResponse,
    };
    use crate::file_search_stores::{
        FileSearchDocument, FileSearchDocumentListResponse, FileSearchStore,
        FileSearchStoreListResponse,
    };
    use crate::files::{FileListResponse, FileMetadata};
    use crate::response::InteractionResponse;
    use crate::triggers::{
        Trigger, TriggerExecution, TriggerExecutionListResponse, TriggerListResponse,
    };
    use crate::voices::{Voice, VoiceListResponse};
    use crate::webhooks::{RotateSigningSecretResponse, Webhook, WebhookListResponse};

    let t = |name, binding, spec, probe| Target {
        name,
        binding,
        spec,
        probe,
    };
    vec![
        t(
            "InteractionResponse",
            Some("interactions.Interaction"),
            Some("Interaction"),
            probe::<InteractionResponse>,
        ),
        t(
            "StreamEvent",
            Some("interactions.InteractionSSEEvent"),
            Some("InteractionSseEvent"),
            probe_stream_event,
        ),
        t(
            "Webhook",
            Some("webhooks.Webhook"),
            Some("Webhook"),
            probe::<Webhook>,
        ),
        t(
            "WebhookListResponse",
            Some("webhooks.WebhookListResponse"),
            Some("ListWebhooksResponse"),
            probe::<WebhookListResponse>,
        ),
        t(
            "RotateSigningSecretResponse",
            Some("webhooks.WebhookRotateSigningSecretResponse"),
            Some("RotateSigningSecretResponse"),
            probe::<RotateSigningSecretResponse>,
        ),
        t(
            "Trigger",
            Some("triggers.Trigger"),
            Some("Trigger"),
            probe::<Trigger>,
        ),
        t(
            "TriggerListResponse",
            Some("triggers.ListTriggersResponse"),
            Some("ListTriggersResponse"),
            probe::<TriggerListResponse>,
        ),
        t(
            "TriggerExecution",
            Some("triggers.TriggerExecution"),
            Some("TriggerExecution"),
            probe::<TriggerExecution>,
        ),
        t(
            "TriggerExecutionListResponse",
            Some("triggers.ListTriggerExecutionsResponse"),
            Some("ListTriggerExecutionsResponse"),
            probe::<TriggerExecutionListResponse>,
        ),
        t("Agent", Some("agents.Agent"), Some("Agent"), probe::<Agent>),
        t(
            "AgentListResponse",
            Some("agents.AgentListResponse"),
            Some("ListAgentsResponse"),
            probe::<AgentListResponse>,
        ),
        t(
            "Environment",
            Some("environments.Environment"),
            Some("Environment"),
            probe::<Environment>,
        ),
        t(
            "EnvironmentListResponse",
            Some("environments.ListEnvironmentsResponse"),
            Some("ListEnvironmentsResponse"),
            probe::<EnvironmentListResponse>,
        ),
        t(
            "EnvironmentFile",
            Some("environments.EnvironmentFile"),
            Some("EnvironmentFile"),
            probe::<EnvironmentFile>,
        ),
        t(
            "EnvironmentFileListResponse",
            Some("environments.GetEnvironmentFilesResponse"),
            Some("GetEnvironmentFilesResponse"),
            probe::<EnvironmentFileListResponse>,
        ),
        // What `EnvironmentFiles::upload` reads. The spec has its own
        // schema for the upload's response; the bindings parse it as
        // `GetEnvironmentFilesResponse`, the target above.
        t(
            "EnvironmentFileListResponse (upload)",
            None,
            Some("UploadEnvironmentFileResponse"),
            probe::<EnvironmentFileListResponse>,
        ),
        t(
            "Credential",
            Some("credentials.Credential"),
            Some("Credential"),
            probe::<Credential>,
        ),
        t(
            "CredentialListResponse",
            Some("credentials.CredentialListResponse"),
            Some("ListCredentialsResponse"),
            probe::<CredentialListResponse>,
        ),
        t(
            "Voice",
            Some("voices.VoiceOutput"),
            Some("Voice"),
            probe::<Voice>,
        ),
        t(
            "VoiceListResponse",
            Some("voices.ListVoicesResponse"),
            Some("ListVoicesResponse"),
            probe::<VoiceListResponse>,
        ),
        // The published specs do not list the decisions resource yet.
        t(
            "CreateDecisionResponse",
            Some("decisions.CreateDecisionResponse"),
            None,
            probe::<CreateDecisionResponse>,
        ),
        t(
            "FileMetadata",
            Some("legacy.File"),
            None,
            probe::<FileMetadata>,
        ),
        t(
            "FileListResponse",
            Some("legacy.ListFilesResponse"),
            None,
            probe::<FileListResponse>,
        ),
        t(
            "FileSearchStore",
            Some("legacy.FileSearchStore"),
            None,
            probe::<FileSearchStore>,
        ),
        t(
            "FileSearchStoreListResponse",
            Some("legacy.ListFileSearchStoresResponse"),
            None,
            probe::<FileSearchStoreListResponse>,
        ),
        t(
            "FileSearchDocument",
            Some("legacy.Document"),
            None,
            probe::<FileSearchDocument>,
        ),
        t(
            "FileSearchDocumentListResponse",
            Some("legacy.ListDocumentsResponse"),
            None,
            probe::<FileSearchDocumentListResponse>,
        ),
    ]
}

/// The binding class of every target, for the completeness check in
/// [`super`].
pub(super) fn target_classes() -> Vec<&'static str> {
    targets().iter().filter_map(|t| t.binding).collect()
}
