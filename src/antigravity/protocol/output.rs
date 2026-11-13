use super::{
    CallHookRequest, Modality, PolicyDecisionRequest, StepUpdate, StopReason, TrajectoryState,
    flex_num,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

// =============================================================================
// Harness -> client events (OutputEvent oneof)
// =============================================================================

/// `OutputEvent` — the harness-to-client message envelope.
///
/// Carries envelope metadata (`seq_num`, `timestamp_micros`, the token
/// usage) alongside the oneof payload.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OutputEvent {
    /// Monotonic sequence number.
    pub seq_num: Option<i64>,
    /// Event timestamp in microseconds since the Unix epoch.
    pub timestamp_micros: Option<i64>,
    /// Token usage: harness 0.1.5's flat `usageMetadata`, or the `total` of
    /// the `usageUpdate` that replaced it in 0.1.10.
    pub usage_metadata: Option<UsageMetadata>,
    /// The rest of a `usageUpdate` beside its total (which is
    /// [`usage_metadata`](Self::usage_metadata)): the usage of each
    /// trajectory. `Some` exactly when the event carried a `usageUpdate`
    /// object, and then the event serializes back as one. A `usageUpdate`
    /// of another JSON type carries no usage: it is kept in
    /// [`extra`](Self::extra), as it came.
    pub usage_update: Option<UsageUpdate>,
    /// The event payload (`None` if the oneof was empty).
    pub payload: Option<OutputPayload>,
    /// Envelope keys left over beside a recognized payload: a field a newer
    /// harness adds to the envelope, kept rather than dropped (Evergreen).
    /// With no recognized payload, the first leftover key is the payload
    /// instead, as [`OutputPayload::Unknown`].
    pub extra: Map<String, Value>,
}

/// The oneof payload of an [`OutputEvent`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum OutputPayload {
    /// Progress on one agent step. Boxed: `StepUpdate` is by far the
    /// largest message in the protocol.
    StepUpdate(Box<StepUpdate>),
    /// A trajectory changed lifecycle state.
    TrajectoryStateUpdate(TrajectoryStateUpdate),
    /// The model called a client-executed custom tool.
    ToolCall(ToolCall),
    /// Reply to the initial `InitializeConversationEvent`.
    InitializeConversationResponse(InitializeConversationResponse),
    /// The harness is invoking a client-side lifecycle hook.
    CallHookRequest(Box<CallHookRequest>),
    /// Session-end hooks completed. A `sessionEndResponse` that is not a
    /// bool is kept as [`Unknown`](Self::Unknown), as it came.
    SessionEndResponse(bool),
    /// The harness asks this client to judge a tool call against a dynamic
    /// policy rule or the auto policy.
    PolicyDecisionRequest(PolicyDecisionRequest),
    /// An event variant this crate does not recognize (Evergreen).
    Unknown {
        /// The unrecognized oneof field name.
        event_type: String,
        /// The raw JSON payload, preserved for roundtrip.
        data: Value,
    },
}

impl OutputPayload {
    /// Check if this is an unknown event.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the unrecognized oneof field name if this is an unknown event.
    #[must_use]
    pub fn unknown_event_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { event_type, .. } => Some(event_type),
            _ => None,
        }
    }

    /// Returns the preserved raw JSON if this is an unknown event.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }

    /// Whether this is the harness's reply to the session end: a
    /// `sessionEndResponse`, the proto's bool or a value of another type
    /// kept as `Unknown`.
    pub(crate) fn is_session_end_response(&self) -> bool {
        match self {
            Self::SessionEndResponse(_) => true,
            Self::Unknown { event_type, .. } => event_type == "sessionEndResponse",
            _ => false,
        }
    }
}

const OUTPUT_PAYLOAD_KEYS: &[&str] = &[
    "stepUpdate",
    "trajectoryStateUpdate",
    "toolCall",
    "initializeConversationResponse",
    "callHookRequest",
    "sessionEndResponse",
    "policyDecisionRequest",
];

impl Serialize for OutputEvent {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{Error, SerializeMap};
        let mut len = 0;
        len += usize::from(self.seq_num.is_some());
        len += usize::from(self.timestamp_micros.is_some());
        len += usize::from(self.usage_metadata.is_some() || self.usage_update.is_some());
        len += usize::from(self.payload.is_some());
        len += self.extra.len();
        let mut map = serializer.serialize_map(Some(len))?;
        if let Some(seq_num) = self.seq_num {
            map.serialize_entry("seqNum", &seq_num)?;
        }
        if let Some(timestamp_micros) = self.timestamp_micros {
            map.serialize_entry("timestampMicros", &timestamp_micros)?;
        }
        if let Some(payload) = &self.payload {
            let (key, value) = match payload {
                OutputPayload::StepUpdate(v) => (
                    "stepUpdate",
                    serde_json::to_value(v).map_err(S::Error::custom)?,
                ),
                OutputPayload::TrajectoryStateUpdate(v) => (
                    "trajectoryStateUpdate",
                    serde_json::to_value(v).map_err(S::Error::custom)?,
                ),
                OutputPayload::ToolCall(v) => (
                    "toolCall",
                    serde_json::to_value(v).map_err(S::Error::custom)?,
                ),
                OutputPayload::InitializeConversationResponse(v) => (
                    "initializeConversationResponse",
                    serde_json::to_value(v).map_err(S::Error::custom)?,
                ),
                OutputPayload::CallHookRequest(v) => (
                    "callHookRequest",
                    serde_json::to_value(v).map_err(S::Error::custom)?,
                ),
                OutputPayload::SessionEndResponse(b) => ("sessionEndResponse", Value::Bool(*b)),
                OutputPayload::PolicyDecisionRequest(v) => (
                    "policyDecisionRequest",
                    serde_json::to_value(v).map_err(S::Error::custom)?,
                ),
                OutputPayload::Unknown { event_type, data } => (event_type.as_str(), data.clone()),
            };
            map.serialize_entry(key, &value)?;
        }
        match (&self.usage_update, &self.usage_metadata) {
            (Some(update), total) => {
                let mut update = serde_json::to_value(update).map_err(S::Error::custom)?;
                if let (Some(total), Value::Object(fields)) = (total, &mut update) {
                    fields.insert(
                        "total".to_string(),
                        serde_json::to_value(total).map_err(S::Error::custom)?,
                    );
                }
                map.serialize_entry("usageUpdate", &update)?;
            }
            (None, Some(usage_metadata)) => map.serialize_entry("usageMetadata", usage_metadata)?,
            (None, None) => {}
        }
        for (key, value) in &self.extra {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for OutputEvent {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let mut map = Map::deserialize(deserializer)?;

        fn take_i64<E: Error>(map: &mut Map<String, Value>, key: &str) -> Result<Option<i64>, E> {
            match map.remove(key) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::Number(n)) => n
                    .as_i64()
                    .map(Some)
                    .ok_or_else(|| E::custom(format!("{key}: number does not fit in i64"))),
                Some(Value::String(s)) => s
                    .parse::<i64>()
                    .map(Some)
                    .map_err(|e| E::custom(format!("{key}: invalid i64 string: {e}"))),
                Some(other) => Err(E::custom(format!("{key}: expected i64, got {other}"))),
            }
        }

        let seq_num = take_i64(&mut map, "seqNum")?;
        let timestamp_micros = take_i64(&mut map, "timestampMicros")?;
        // Harness 0.1.10 replaced the flat `usageMetadata` with
        // `usageUpdate: {agents: [...], total: UsageMetadata}`. Read the
        // aggregate from either spelling so one build drives both
        // revisions, and keep the rest of a `usageUpdate` (the
        // per-trajectory `agents`, and any key this crate does not model)
        // in `usage_update`, so it survives a round trip (D-034). Either
        // key must be consumed here or it falls through to the
        // leftover-key arm below and is misreported as an unknown oneof
        // variant.
        // Consume `usageUpdate` unconditionally, before choosing between
        // the two spellings: if a transitional harness ever sent *both*,
        // leaving it in `map` would hand it to the leftover-key arm below
        // and surface a bogus `Unknown` payload — the exact misreport this
        // branch exists to prevent, in the one shape a nested match
        // wouldn't cover. The flat spelling's usage wins then, as it
        // always has, and the update's own total is dropped.
        // A `usageUpdate` that is not an object (and so not the proto's
        // message) carries no usage, but is kept in `extra` and written back
        // as it came (D-001). It goes back into the map only after the
        // payload is chosen, so the leftover-key arm cannot claim it.
        let mut malformed_update = None;
        let (update_total, usage_update) = match map.remove("usageUpdate") {
            None | Some(Value::Null) => (None, None),
            Some(Value::Object(mut update)) => {
                let total = match update.remove("total") {
                    None | Some(Value::Null) => None,
                    Some(total) => Some(
                        serde_json::from_value::<UsageMetadata>(total).map_err(D::Error::custom)?,
                    ),
                };
                let rest = serde_json::from_value::<UsageUpdate>(Value::Object(update))
                    .map_err(D::Error::custom)?;
                (total, Some(rest))
            }
            Some(other) => {
                tracing::warn!(
                    "Unexpected JSON type for usageUpdate, keeping it in extra without usage: {other}"
                );
                malformed_update = Some(other);
                (None, None)
            }
        };
        let usage_metadata = match map.remove("usageMetadata") {
            Some(Value::Null) | None => update_total,
            Some(v) => Some(serde_json::from_value(v).map_err(D::Error::custom)?),
        };

        let mut payload = None;
        for key in OUTPUT_PAYLOAD_KEYS {
            if let Some(value) = map.remove(*key) {
                payload = Some(match *key {
                    "stepUpdate" => OutputPayload::StepUpdate(Box::new(
                        serde_json::from_value(value).map_err(D::Error::custom)?,
                    )),
                    "trajectoryStateUpdate" => OutputPayload::TrajectoryStateUpdate(
                        serde_json::from_value(value).map_err(D::Error::custom)?,
                    ),
                    "toolCall" => OutputPayload::ToolCall(
                        serde_json::from_value(value).map_err(D::Error::custom)?,
                    ),
                    "initializeConversationResponse" => {
                        OutputPayload::InitializeConversationResponse(
                            serde_json::from_value(value).map_err(D::Error::custom)?,
                        )
                    }
                    "callHookRequest" => OutputPayload::CallHookRequest(Box::new(
                        serde_json::from_value(value).map_err(D::Error::custom)?,
                    )),
                    "sessionEndResponse" => match value {
                        Value::Bool(ended) => OutputPayload::SessionEndResponse(ended),
                        // Not the proto's bool: kept as it came (D-001).
                        // `is_session_end_response` still reads it as the
                        // end of the session.
                        data => OutputPayload::Unknown {
                            event_type: (*key).to_string(),
                            data,
                        },
                    },
                    "policyDecisionRequest" => OutputPayload::PolicyDecisionRequest(
                        serde_json::from_value(value).map_err(D::Error::custom)?,
                    ),
                    // A key added to OUTPUT_PAYLOAD_KEYS without an arm
                    // here: preserve it rather than panic on harness input.
                    other => OutputPayload::Unknown {
                        event_type: other.to_string(),
                        data: value,
                    },
                });
                break;
            }
        }
        // With no recognized payload, a leftover field is an unrecognized
        // oneof variant: preserve it as the payload.
        if payload.is_none()
            && let Some(event_type) = map.keys().next().cloned()
            && let Some(data) = map.remove(&event_type)
        {
            tracing::warn!(
                "Unknown OutputEvent variant: '{}'. Preserving in Unknown variant.",
                event_type
            );
            payload = Some(OutputPayload::Unknown { event_type, data });
        }
        if let Some(update) = malformed_update {
            map.insert("usageUpdate".to_string(), update);
        }

        Ok(Self {
            seq_num,
            timestamp_micros,
            usage_metadata,
            usage_update,
            payload,
            extra: map,
        })
    }
}

/// `InitializeConversationResponse`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InitializeConversationResponse {
    /// The conversation id (persist to resume this session later).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cascade_id: Option<String>,
    /// Restored steps when resuming a saved conversation.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub history: Vec<StepUpdate>,
    /// Token usage accumulated by a resumed conversation so far.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cumulative_usage: Option<UsageMetadata>,
    /// Per-trajectory breakdown of `cumulative_usage`.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub trajectory_usage: Vec<TrajectoryUsageEntry>,
    /// Whether the OS command sandbox can be enforced (new in harness
    /// 0.1.18). Only meaningful when `run_command` sandboxing was
    /// requested, which this crate does not do yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_status: Option<SandboxStatus>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `TrajectoryUsageEntry` — token usage attributed to one trajectory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrajectoryUsageEntry {
    /// The trajectory the usage belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trajectory_id: Option<String>,
    /// Its usage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageMetadata>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `UsageUpdate` beside its `total` — how harness 0.1.10 and later report
/// token usage. The total is [`OutputEvent::usage_metadata`]; this holds
/// the rest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UsageUpdate {
    /// The usage of each trajectory: the conversation's own and each
    /// subagent's.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub agents: Vec<TrajectoryUsageEntry>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `SandboxStatus` — whether the harness can enforce the OS command
/// sandbox.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SandboxStatus {
    /// Whether the sandbox actually enforces isolation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available: Option<bool>,
    /// Why not, when `available` is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `ModalityTokenCount` — a token count for one content modality.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModalityTokenCount {
    /// The modality counted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modality: Option<Modality>,
    /// Tokens of that modality.
    #[serde(
        default,
        deserialize_with = "flex_num::opt_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub token_count: Option<u64>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `UsageMetadata` — token accounting for a step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UsageMetadata {
    /// Prompt tokens.
    #[serde(
        default,
        deserialize_with = "flex_num::opt_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub prompt_token_count: Option<u64>,
    /// Cached-content tokens.
    #[serde(
        default,
        deserialize_with = "flex_num::opt_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub cached_content_token_count: Option<u64>,
    /// Response candidate tokens.
    #[serde(
        default,
        deserialize_with = "flex_num::opt_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub candidates_token_count: Option<u64>,
    /// Thinking tokens.
    #[serde(
        default,
        deserialize_with = "flex_num::opt_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub thoughts_token_count: Option<u64>,
    /// Total tokens.
    #[serde(
        default,
        deserialize_with = "flex_num::opt_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub total_token_count: Option<u64>,
    /// The service tier the backend served the request on (a raw string:
    /// the value set differs between the Gemini API and Vertex AI).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    /// Prompt tokens by modality (new in harness 0.1.18).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub prompt_tokens_details: Vec<ModalityTokenCount>,
    /// Cached tokens by modality (new in harness 0.1.18).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub cache_tokens_details: Vec<ModalityTokenCount>,
    /// Response tokens by modality (new in harness 0.1.18).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub candidates_tokens_details: Vec<ModalityTokenCount>,
    /// Tool-use prompt tokens by modality (new in harness 0.1.18).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tool_use_prompt_tokens_details: Vec<ModalityTokenCount>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `TrajectoryStateUpdate`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrajectoryStateUpdate {
    /// The trajectory that changed state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trajectory_id: Option<String>,
    /// The new state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<TrajectoryState>,
    /// Error message (e.g. why the turn was cancelled).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The trajectory that spawned this one — set for subagent
    /// trajectories, absent for the root conversation (new in harness
    /// 0.1.18).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_trajectory_id: Option<String>,
    /// Subagent nesting depth (`0`/absent for the root conversation).
    #[serde(
        default,
        deserialize_with = "flex_num::opt_i32",
        skip_serializing_if = "Option::is_none"
    )]
    pub depth: Option<i32>,
    /// Why the trajectory stopped, when it was not a normal completion
    /// (new in harness 0.1.18).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `ToolCall` — the model invoking a client-executed custom tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    /// Correlation id; echo it in the [`ToolResponse`](super::ToolResponse).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The tool name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Arguments, serialized as a JSON string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments_json: Option<String>,
    /// Arguments as a structured value (protobuf `Struct`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
    /// The trajectory that made the call — lets a subagent's custom tool
    /// calls be told apart from the parent's (new in harness 0.1.18).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trajectory_id: Option<String>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
