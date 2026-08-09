//! Triggers resource (`/v1beta/triggers`) — server-side scheduled
//! interactions.
//!
//! A [`Trigger`] runs a stored interaction request on a cron
//! [`schedule`](Trigger::schedule) with **no client process running**: the
//! API creates a fresh interaction per firing, and past firings are
//! inspectable via
//! [`list_trigger_executions`](crate::client::Client::list_trigger_executions).
//!
//! Server-side constraint (verified live 2026-08-08): the trigger's
//! `interaction` must target a custom `agent` (an [`agents`](crate::agents)
//! resource ID) — plain `model` interactions are rejected ("Agent '' is
//! invalid or not found") and `store` is not allowed in the nested request.
//! Custom-agent creation is gated/allowlisted on standard API keys, so
//! trigger creation is too; the CRUD surface is modeled for accounts where
//! it is available.
//!
//! This is distinct from
//! [`antigravity::TriggerConfig`](crate::antigravity), which schedules
//! messages inside a *local* harness session.

use crate::request::InteractionRequest;
use chrono::{DateTime, Utc};
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Current status of a [`Trigger`].
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
///
/// # Wire Format
///
/// Serializes as lowercase strings: `"active"`, `"paused"`, `"error"`.
///
/// # Evergreen Pattern
///
/// Unknown values from the API deserialize into the `Unknown` variant,
/// preserving the original data for debugging and roundtrip serialization.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum TriggerStatus {
    /// The trigger fires on its schedule.
    Active,
    /// The trigger is paused and does not fire.
    Paused,
    /// The trigger is disabled after consecutive failures.
    Error,
    /// Unknown variant for forward compatibility (Evergreen pattern)
    Unknown {
        /// The unrecognized status type from the API
        status_type: String,
        /// The raw JSON value, preserved for debugging and roundtrip
        data: serde_json::Value,
    },
}

impl TriggerStatus {
    /// The wire string for this status — the single source both `Display`
    /// and `Serialize` render, so the two can never disagree.
    fn as_wire(&self) -> &str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Error => "error",
            Self::Unknown { status_type, .. } => status_type,
        }
    }

    /// Returns true if this is an unknown status.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the status type name if this is an unknown status.
    #[must_use]
    pub fn unknown_status_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { status_type, .. } => Some(status_type),
            _ => None,
        }
    }

    /// Returns the preserved data if this is an unknown status.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl fmt::Display for TriggerStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_wire())
    }
}

impl Serialize for TriggerStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_wire())
    }
}

impl<'de> Deserialize<'de> for TriggerStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value.as_str() {
            Some("active") => Ok(Self::Active),
            Some("paused") => Ok(Self::Paused),
            Some("error") => Ok(Self::Error),
            Some(other) => {
                tracing::warn!(
                    "Encountered unknown TriggerStatus '{other}' - using Unknown variant (Evergreen)"
                );
                Ok(Self::Unknown {
                    status_type: other.to_string(),
                    data: value.clone(),
                })
            }
            None => {
                tracing::warn!(
                    "TriggerStatus received non-string value: {value}. Preserving in Unknown variant."
                );
                Ok(Self::Unknown {
                    status_type: format!("<non-string: {value}>"),
                    data: value,
                })
            }
        }
    }
}

/// Status of a single [`TriggerExecution`].
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
///
/// # Wire Format
///
/// Serializes as snake_case strings: `"in_progress"`, `"completed"`,
/// `"failed"`, `"skipped"`, `"timed_out"`.
///
/// # Evergreen Pattern
///
/// Unknown values from the API deserialize into the `Unknown` variant,
/// preserving the original data for debugging and roundtrip serialization.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum TriggerExecutionStatus {
    /// The execution is still running.
    InProgress,
    /// The execution finished successfully.
    Completed,
    /// The execution failed.
    Failed,
    /// The execution was skipped (e.g. the prior one was still running).
    Skipped,
    /// The execution exceeded its timeout.
    TimedOut,
    /// Unknown variant for forward compatibility (Evergreen pattern)
    Unknown {
        /// The unrecognized status type from the API
        status_type: String,
        /// The raw JSON value, preserved for debugging and roundtrip
        data: serde_json::Value,
    },
}

impl TriggerExecutionStatus {
    /// The wire string for this status — the single source both `Display`
    /// and `Serialize` render, so the two can never disagree.
    fn as_wire(&self) -> &str {
        match self {
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::TimedOut => "timed_out",
            Self::Unknown { status_type, .. } => status_type,
        }
    }

    /// Returns true if this is an unknown status.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the status type name if this is an unknown status.
    #[must_use]
    pub fn unknown_status_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { status_type, .. } => Some(status_type),
            _ => None,
        }
    }

    /// Returns the preserved data if this is an unknown status.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl fmt::Display for TriggerExecutionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_wire())
    }
}

impl Serialize for TriggerExecutionStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_wire())
    }
}

impl<'de> Deserialize<'de> for TriggerExecutionStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value.as_str() {
            Some("in_progress") => Ok(Self::InProgress),
            Some("completed") => Ok(Self::Completed),
            Some("failed") => Ok(Self::Failed),
            Some("skipped") => Ok(Self::Skipped),
            Some("timed_out") => Ok(Self::TimedOut),
            Some(other) => {
                tracing::warn!(
                    "Encountered unknown TriggerExecutionStatus '{other}' - using Unknown variant (Evergreen)"
                );
                Ok(Self::Unknown {
                    status_type: other.to_string(),
                    data: value.clone(),
                })
            }
            None => {
                tracing::warn!(
                    "TriggerExecutionStatus received non-string value: {value}. Preserving in Unknown variant."
                );
                Ok(Self::Unknown {
                    status_type: format!("<non-string: {value}>"),
                    data: value,
                })
            }
        }
    }
}

/// A server-side scheduled trigger, as returned by `/v1beta/triggers`.
///
/// All fields are optional with a struct-level serde default (the Evergreen
/// preserve-don't-reject posture, mirroring [`Agent`](crate::agents::Agent)):
/// this resource shape is not yet fully live-verified — creation is
/// agent-gated — so a projection that elides fields must degrade per-field
/// rather than failing the whole list response.
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Trigger {
    /// Output only. The ID of the trigger.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Cron expression the trigger fires on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,
    /// IANA time zone the schedule is evaluated in (e.g. `"UTC"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,
    /// The interaction request created on each firing.
    ///
    /// A nested `input` this crate can't deserialize (explicit null, a
    /// stray scalar) degrades to empty text with a `warn!`, and any
    /// other undeserializable `interaction` (a non-object shape, a type
    /// mismatch on a modeled field) degrades to `None`, instead of
    /// failing the whole list response. An *absent* `input` (a projection
    /// that elides it — the common list shape) likewise reads as empty
    /// text, silently: don't treat `interaction.input` as evidence of the
    /// stored prompt. (Under default features a
    /// malformed steps *array* never reaches this path — the Evergreen
    /// `Step` deserializer absorbs unrecognized elements as
    /// `Step::Unknown` per-element; under `strict-unknown` it is rejected
    /// and degrades here like any other bad input.) Leniency is
    /// scoped to this response side; [`TriggerCreateParams`]'s send-side
    /// interaction stays strict, so a config-file typo is a clean parse
    /// error rather than a silently scheduled empty prompt.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_lenient_interaction"
    )]
    pub interaction: Option<InteractionRequest>,
    /// Human-readable display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// ID of the environment fired interactions run against.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<String>,
    /// Output only. The current status of the trigger.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<TriggerStatus>,
    /// Consecutive failures before the trigger is disabled.
    ///
    /// Tolerates the protobuf-JSON string form on deserialize (live-verified
    /// on the environments resource's int64s) so one string-encoded int
    /// can't fail the whole list response, and re-serializes in the same
    /// string form for roundtrip uniformity with [`Environment`]'s counts.
    /// (The send direction is [`TriggerCreateParams`], which emits plain
    /// numbers — protobuf-JSON accepts both on input.)
    ///
    /// [`Environment`]: crate::environments::Environment
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serde_util::serialize_string_i64",
        deserialize_with = "crate::serde_util::deserialize_string_i64"
    )]
    pub max_consecutive_failures: Option<i64>,
    /// Output only. Current count of consecutive failed executions.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serde_util::serialize_string_i64",
        deserialize_with = "crate::serde_util::deserialize_string_i64"
    )]
    pub consecutive_failure_count: Option<i64>,
    /// Per-execution timeout in seconds.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serde_util::serialize_string_i64",
        deserialize_with = "crate::serde_util::deserialize_string_i64"
    )]
    pub execution_timeout_seconds: Option<i64>,
    /// Output only. ID of the previous fired interaction, chained into the
    /// next firing's context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_interaction_id: Option<String>,
    /// Output only. When the trigger was created.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_lenient_timestamp"
    )]
    pub create_time: Option<DateTime<Utc>>,
    /// Output only. When the trigger was last updated.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_lenient_timestamp"
    )]
    pub update_time: Option<DateTime<Utc>>,
    /// Output only. When the trigger last fired.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_lenient_timestamp"
    )]
    pub last_run_time: Option<DateTime<Utc>>,
    /// Output only. When the trigger next fires.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_lenient_timestamp"
    )]
    pub next_run_time: Option<DateTime<Utc>>,
    /// Output only. When the trigger was last paused.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_lenient_timestamp"
    )]
    pub last_pause_time: Option<DateTime<Utc>>,
    /// Output only. When the trigger was last resumed.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_lenient_timestamp"
    )]
    pub last_resume_time: Option<DateTime<Utc>>,
}

/// Deserializes `Trigger::interaction`, degrading a nested `input` that
/// [`InteractionInput`](crate::request::InteractionInput)'s deserializer
/// rejects (explicit null, a stray scalar) onto the empty default with a
/// `warn!` before parsing the interaction, and a non-object `interaction`
/// onto `None`. (Under default features a malformed steps *array* never
/// reaches the rejection path — the Evergreen `Step` deserializer absorbs
/// unrecognized elements as `Step::Unknown` per-element; `strict-unknown`
/// rejects it and it degrades here like any other bad input.)
///
/// The nested `input` is the one non-`Option` field in the trigger tree,
/// so without this a projection carrying `input: null` (or `input: 0`)
/// would propagate a hard error up through `Trigger` and fail the whole
/// list response — the same wholesale failure the lenient int64 and
/// timestamp helpers exist to avoid. Scoped here (not on
/// `InteractionRequest` itself) so the send side stays strict.
fn deserialize_lenient_interaction<'de, D>(
    deserializer: D,
) -> Result<Option<InteractionRequest>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        None | Some(serde_json::Value::Null) => Ok(None),
        // Catch-all like the serde_util helpers: a non-object interaction
        // (a stray scalar, an array) degrades to None with a warn! instead
        // of failing the whole list response.
        Some(other) if !other.is_object() => {
            tracing::warn!("Unexpected JSON type for trigger interaction, dropping: {other:?}");
            Ok(None)
        }
        Some(mut value) => {
            // Take `input` out and parse the rest — one parse each, no
            // clone (the catch-all arm above guarantees an object here).
            // The placeholder satisfies the now-required field; a sparse
            // projection's absent input thus deserializes to empty text
            // *on this path only* — the send side keeps input required.
            let obj = value
                .as_object_mut()
                .expect("non-object interactions are handled by the arm above");
            let raw_input = obj.remove("input");
            obj.insert(
                "input".to_string(),
                serde_json::Value::String(String::new()),
            );
            // Warn-and-drop like the arms above: a type mismatch on a
            // modeled field (numeric `model`, string `tools`) must not
            // zero the whole page either.
            let Ok(mut request) = serde_json::from_value::<InteractionRequest>(value)
                .map_err(|e| tracing::warn!("Undeserializable trigger interaction, dropping: {e}"))
            else {
                return Ok(None);
            };
            if let Some(raw) = raw_input {
                request.input = crate::request::input_from_value(raw).unwrap_or_else(|e| {
                    tracing::warn!(
                        "Undeserializable input in trigger interaction ({e}); \
                         degrading to empty text"
                    );
                    crate::request::InteractionInput::default()
                });
            }
            Ok(Some(request))
        }
    }
}

/// Request body for creating a [`Trigger`].
///
/// # Example
///
/// The struct literal below is the no-client form; with a [`Client`] in
/// scope, prefer `client.interaction()...build()` — the builder yields the
/// same [`InteractionRequest`] but stays source-compatible as fields are
/// added (struct literals break on every new public field; see the 0.9.0
/// CHANGELOG entry).
///
/// ```
/// use genai_rs::{InteractionInput, InteractionRequest, TriggerCreateParams};
///
/// let interaction = InteractionRequest {
///     agent: Some("my-custom-agent".to_string()),
///     input: InteractionInput::Text("Daily repo audit".to_string()),
///     ..Default::default()
/// };
/// let params = TriggerCreateParams::new("0 9 * * *", "UTC", interaction)
///     .with_display_name("daily-audit");
/// ```
///
/// [`Client`]: crate::Client
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TriggerCreateParams {
    /// Cron expression the trigger fires on.
    pub schedule: String,
    /// IANA time zone the schedule is evaluated in (e.g. `"UTC"`).
    pub time_zone: String,
    /// The interaction request created on each firing. Must target a
    /// custom `agent`; `store` is not allowed here (server-verified).
    pub interaction: InteractionRequest,
    /// Human-readable display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// ID of the environment fired interactions run against.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<String>,
    /// Consecutive failures before the trigger is disabled.
    ///
    /// Sends as a plain JSON number. The same logical field on the
    /// *response*-side [`Trigger`] re-serializes in the protobuf-JSON
    /// string form (for roundtrip fidelity to captured wire), so a
    /// read-modify-recreate flow changes the wire spelling — both forms
    /// are accepted on deserialize.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_consecutive_failures: Option<i64>,
    /// Per-execution timeout in seconds. Sends as a plain JSON number
    /// (see [`Self::max_consecutive_failures`] on the wire-form
    /// asymmetry with [`Trigger`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_timeout_seconds: Option<i64>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen) — lets an
    /// unmodeled create-request field be set without a crate release,
    /// which matters here because trigger creation is agent-gated and the
    /// body cannot be live-verified against the wire.
    ///
    /// The inherent cost: a *typo'd optional* key in a deserialized
    /// config (`dispaly_name`, ...) is silently absorbed here and
    /// forwarded to the server verbatim rather than rejected — the
    /// send-side strictness documented on
    /// [`Trigger::interaction`] covers the nested request and the
    /// required top-level fields, not optional key spellings.
    ///
    /// A key that collides with a modeled field **wins on serialize** via
    /// `serde_json::to_value` — the form the request path uses — so the
    /// escape hatch can also override a modeled field whose wire shape
    /// turns out wrong. (`to_string` on a flattened struct emits both keys
    /// rather than deduplicating; don't hand-serialize colliding params.)
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl TriggerCreateParams {
    /// Creates trigger parameters for `schedule` (cron) in `time_zone`.
    #[must_use]
    pub fn new(
        schedule: impl Into<String>,
        time_zone: impl Into<String>,
        interaction: InteractionRequest,
    ) -> Self {
        Self {
            schedule: schedule.into(),
            time_zone: time_zone.into(),
            interaction,
            display_name: None,
            environment_id: None,
            max_consecutive_failures: None,
            execution_timeout_seconds: None,
            extra: serde_json::Map::new(),
        }
    }

    /// Sets the display name.
    #[must_use]
    pub fn with_display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(name.into());
        self
    }

    /// Sets the environment ID fired interactions run against.
    #[must_use]
    pub fn with_environment_id(mut self, id: impl Into<String>) -> Self {
        self.environment_id = Some(id.into());
        self
    }

    /// Sets the consecutive-failure limit before the trigger is disabled.
    #[must_use]
    pub fn with_max_consecutive_failures(mut self, count: i64) -> Self {
        self.max_consecutive_failures = Some(count);
        self
    }

    /// Sets the per-execution timeout in seconds.
    #[must_use]
    pub fn with_execution_timeout_seconds(mut self, seconds: i64) -> Self {
        self.execution_timeout_seconds = Some(seconds);
        self
    }
}

/// Update payload for a [`Trigger`] — unset fields are omitted from the
/// PATCH body.
///
/// Unlike [`Client::update_webhook`](crate::client::Client::update_webhook),
/// the SDK spec exposes **no `update_mask` parameter** for trigger updates
/// (google-genai 2.17.0: `triggers.update(id, display_name, status)` only),
/// so field omission is the only scoping mechanism available. The sibling
/// webhooks PATCH was observed live (2026-07) to apply exactly the fields
/// present in the body, but trigger updates are not live-verifiable while
/// creation is agent-gated — treat the partial-update semantics as
/// unconfirmed until then.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct TriggerUpdate {
    /// New display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// New status (`active` to resume, `paused` to pause).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<TriggerStatus>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen) — lets an
    /// unmodeled update field be sent without a crate release. Empty maps
    /// add nothing to the body, keeping the empty-update-is-`{}` contract.
    /// Colliding keys win on serialize, as on
    /// [`TriggerCreateParams::extra`].
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl TriggerUpdate {
    /// Creates an empty update.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the display name.
    #[must_use]
    pub fn with_display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(name.into());
        self
    }

    /// Sets the status.
    ///
    /// [`Active`](TriggerStatus::Active) resumes the trigger and
    /// [`Paused`](TriggerStatus::Paused) pauses it — the two values a
    /// caller meaningfully sends. [`Error`](TriggerStatus::Error) is
    /// output-only (the server sets it after consecutive failures); the
    /// open enum accepts it here per the Evergreen posture, but sending it
    /// is untested against the live API.
    ///
    /// In a read-modify-write flow, don't echo back a status whose
    /// [`is_unknown()`](TriggerStatus::is_unknown) type came from a
    /// *non-string* wire value: its wire form is the crate's
    /// `<non-string: ...>` debug marker, not the original value. (An
    /// unknown *string* status round-trips faithfully.)
    #[must_use]
    pub fn with_status(mut self, status: TriggerStatus) -> Self {
        self.status = Some(status);
        self
    }
}

/// A single firing of a [`Trigger`].
///
/// All fields optional with a struct-level serde default; see [`Trigger`]
/// for the rationale.
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct TriggerExecution {
    /// Output only. The ID of the execution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Output only. The ID of the trigger that fired.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_id: Option<String>,
    /// Output only. The interaction created by this firing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interaction_id: Option<String>,
    /// Output only. The environment the firing ran against.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<String>,
    /// Output only. Status of this execution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<TriggerExecutionStatus>,
    /// Output only. Error message when the execution failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Output only. When the firing was scheduled for.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_lenient_timestamp"
    )]
    pub scheduled_time: Option<DateTime<Utc>>,
    /// Output only. When the execution started.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_lenient_timestamp"
    )]
    pub start_time: Option<DateTime<Utc>>,
    /// Output only. When the execution finished.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_lenient_timestamp"
    )]
    pub end_time: Option<DateTime<Utc>>,
}

/// Response from listing triggers.
///
/// The API returns `{}` when no triggers exist (verified live 2026-08-08),
/// so both fields default.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TriggerListResponse {
    /// The triggers in this page. An explicit null degrades to empty.
    #[serde(deserialize_with = "crate::serde_util::deserialize_null_as_empty_vec")]
    pub triggers: Vec<Trigger>,
    /// Token for fetching the next page, absent on the last page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

/// Response from listing a trigger's executions.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TriggerExecutionListResponse {
    /// The executions in this page. An explicit null degrades to empty.
    #[serde(deserialize_with = "crate::serde_util::deserialize_null_as_empty_vec")]
    pub trigger_executions: Vec<TriggerExecution>,
    /// Token for fetching the next page, absent on the last page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::InteractionInput;

    fn probe_interaction() -> InteractionRequest {
        InteractionRequest {
            agent: Some("my-agent".to_string()),
            input: InteractionInput::Text("Say OK".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn create_params_serialize_minimal() {
        let params = TriggerCreateParams::new("0 5 1 1 *", "UTC", probe_interaction());
        let json = serde_json::to_value(&params).unwrap();
        assert_eq!(json["schedule"], "0 5 1 1 *");
        assert_eq!(json["time_zone"], "UTC");
        assert_eq!(json["interaction"]["agent"], "my-agent");
        assert!(json.get("display_name").is_none());
    }

    #[test]
    fn trigger_update_serializes_partial() {
        // The send direction for TriggerStatus: pause/resume rides on this
        // exact wire value.
        let update = TriggerUpdate::new().with_status(TriggerStatus::Paused);
        assert_eq!(
            serde_json::to_value(&update).unwrap(),
            serde_json::json!({"status": "paused"})
        );

        // An empty update must serialize to an empty object. There is no
        // update_mask on this endpoint (see the TriggerUpdate docs), so
        // omitting unset fields from the body is the only partial-update
        // mechanism the wire offers — this pins that we never send nulls.
        assert_eq!(
            serde_json::to_value(TriggerUpdate::new()).unwrap(),
            serde_json::json!({})
        );

        let named = TriggerUpdate::new()
            .with_display_name("renamed")
            .with_status(TriggerStatus::Active);
        let json = serde_json::to_value(&named).unwrap();
        assert_eq!(json["display_name"], "renamed");
        assert_eq!(json["status"], "active");
    }

    #[test]
    fn list_envelopes_deserialize_under_spec_keys() {
        // Pins the envelope keys the crate is betting on for the resource
        // ENUM_WIRE_FORMATS.md marks as unverified: `triggers` and (the one
        // that diverges from its path segment) `trigger_executions`. If the
        // live wire turns out to use `executions`, the fix lands as a
        // visible diff here rather than as a list that quietly reads zero.
        let list: TriggerListResponse =
            serde_json::from_value(serde_json::json!({"triggers": [{"id": "t1"}]})).unwrap();
        assert_eq!(list.triggers.len(), 1);

        let executions: TriggerExecutionListResponse = serde_json::from_value(serde_json::json!({
            "trigger_executions": [{"id": "e1", "status": "completed"}]
        }))
        .unwrap();
        assert_eq!(executions.trigger_executions.len(), 1);
        assert_eq!(
            executions.trigger_executions[0].status,
            Some(TriggerExecutionStatus::Completed)
        );
    }

    #[test]
    fn create_params_and_update_pass_through_unmodeled_fields() {
        // Trigger bodies can't be live-verified while creation is
        // agent-gated, so the Evergreen extra map is the release valve for
        // fields the crate doesn't model yet (same shape as
        // CreateEnvironmentRequest::extra next door).
        let mut params = TriggerCreateParams::new("0 9 * * *", "UTC", probe_interaction());
        params
            .extra
            .insert("future_field".into(), serde_json::json!("x"));
        let json = serde_json::to_value(&params).unwrap();
        assert_eq!(json["future_field"], "x");

        // A colliding key wins on serialize (the flattened map is emitted
        // last) — pinned so the precedence reads as a decision rather than
        // an artifact of field-declaration order.
        params
            .extra
            .insert("schedule".into(), serde_json::json!("*/5 * * * *"));
        let json = serde_json::to_value(&params).unwrap();
        assert_eq!(json["schedule"], "*/5 * * * *");

        let mut update = TriggerUpdate::new();
        update.extra.insert("other".into(), serde_json::json!(1));
        let json = serde_json::to_value(&update).unwrap();
        assert_eq!(json, serde_json::json!({"other": 1}));
        // An empty map keeps the empty-update-is-{} contract.
        assert_eq!(
            serde_json::to_value(TriggerUpdate::new()).unwrap(),
            serde_json::json!({})
        );

        // The deserialize direction: an unmodeled key on the way in lands
        // in `extra` — the documented absorption behavior a config-file
        // typo relies on, and the direction where a flatten regression
        // (flatten buffers every sibling through serde's Content, and the
        // nested interaction deserializers are custom) would be silent.
        let params: TriggerCreateParams = serde_json::from_value(serde_json::json!({
            "schedule": "0 9 * * *",
            "time_zone": "UTC",
            "interaction": {"agent": "my-agent", "input": "hi"},
            "future_field": "x"
        }))
        .unwrap();
        assert_eq!(params.extra["future_field"], "x");
        let update: TriggerUpdate =
            serde_json::from_value(serde_json::json!({"other": 1})).unwrap();
        assert_eq!(update.extra["other"], 1);
    }

    #[test]
    fn trigger_int64s_tolerate_string_wire_form() {
        // The environments resource live-verified that this API family
        // serializes int64s as protobuf-JSON strings; a trigger doing the
        // same must degrade per-field, not fail the whole list response.
        let json = serde_json::json!({
            "id": "trig-1",
            "max_consecutive_failures": "3",
            "consecutive_failure_count": "1",
            "execution_timeout_seconds": 600
        });
        let trigger: Trigger = serde_json::from_value(json).unwrap();
        assert_eq!(trigger.max_consecutive_failures, Some(3));
        assert_eq!(trigger.consecutive_failure_count, Some(1));
        assert_eq!(trigger.execution_timeout_seconds, Some(600));

        // Re-serialization is uniform with Environment's counts: the
        // protobuf-JSON string form, whichever form arrived.
        let back = serde_json::to_value(&trigger).unwrap();
        assert_eq!(back["max_consecutive_failures"], serde_json::json!("3"));
        assert_eq!(back["execution_timeout_seconds"], serde_json::json!("600"));
    }

    #[test]
    fn trigger_timestamps_degrade_per_field() {
        // Same posture as the int64s on this wire-unverified resource: a
        // timestamp arriving in an unexpected encoding (epoch number,
        // proto-style object, garbage string) drops that field to None
        // instead of failing the whole list response.
        let json = serde_json::json!({
            "id": "trig-1",
            "create_time": "2026-08-08T12:30:00Z",
            "update_time": "not-a-time",
            "last_run_time": 1754656200,
            "next_run_time": {"seconds": 1754656200}
        });
        let trigger: Trigger = serde_json::from_value(json).unwrap();
        assert_eq!(trigger.id.as_deref(), Some("trig-1"));
        assert!(trigger.create_time.is_some());
        assert_eq!(trigger.update_time, None);
        assert_eq!(trigger.last_run_time, None);
        assert_eq!(trigger.next_run_time, None);

        let json = serde_json::json!({
            "id": "exec-1",
            "scheduled_time": "2026-08-08T12:30:00Z",
            "end_time": "garbage"
        });
        let execution: TriggerExecution = serde_json::from_value(json).unwrap();
        assert!(execution.scheduled_time.is_some());
        assert_eq!(execution.end_time, None);
    }

    #[test]
    fn create_params_serialize_all_fields() {
        let params = TriggerCreateParams::new("0 9 * * *", "UTC", probe_interaction())
            .with_display_name("daily-audit")
            .with_environment_id("env-123")
            .with_max_consecutive_failures(3)
            .with_execution_timeout_seconds(600);
        let json = serde_json::to_value(&params).unwrap();
        assert_eq!(json["display_name"], "daily-audit");
        assert_eq!(json["environment_id"], "env-123");
        assert_eq!(json["max_consecutive_failures"], 3);
        assert_eq!(json["execution_timeout_seconds"], 600);
    }

    #[test]
    fn empty_list_response_deserializes() {
        // GET /v1beta/triggers returns `{}` when nothing exists.
        let list: TriggerListResponse = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(list.triggers.is_empty());

        // An explicit null list key — the one empty shape the struct-level
        // serde default does not reach — degrades to empty rather than
        // zeroing the page with an error.
        let list: TriggerListResponse =
            serde_json::from_value(serde_json::json!({"triggers": null})).unwrap();
        assert!(list.triggers.is_empty());
        let executions: TriggerExecutionListResponse =
            serde_json::from_value(serde_json::json!({"trigger_executions": null})).unwrap();
        assert!(executions.trigger_executions.is_empty());
    }

    #[test]
    fn unknown_statuses_roundtrip() {
        let status: TriggerStatus = serde_json::from_value(serde_json::json!("snoozing")).unwrap();
        assert!(status.is_unknown());
        assert_eq!(serde_json::to_value(&status).unwrap(), "snoozing");

        let exec: TriggerExecutionStatus =
            serde_json::from_value(serde_json::json!("requeued")).unwrap();
        assert!(exec.is_unknown());
        assert_eq!(serde_json::to_value(&exec).unwrap(), "requeued");
    }

    #[test]
    fn trigger_deserializes_sdk_shape() {
        let json = serde_json::json!({
            "id": "trig-1",
            "schedule": "0 9 * * *",
            "time_zone": "UTC",
            "interaction": {"agent": "my-agent", "input": "audit"},
            "status": "active",
            "next_run_time": "2026-08-09T09:00:00Z"
        });
        let trigger: Trigger = serde_json::from_value(json).unwrap();
        assert_eq!(trigger.id.as_deref(), Some("trig-1"));
        assert_eq!(trigger.status, Some(TriggerStatus::Active));
        assert!(trigger.next_run_time.is_some());
    }

    #[test]
    fn sparse_trigger_projection_degrades_per_field() {
        // A list projection that elides the nested interaction (or any
        // other field) must still deserialize — Evergreen posture.
        let trigger: Trigger = serde_json::from_value(serde_json::json!({"id": "t"})).unwrap();
        assert_eq!(trigger.id.as_deref(), Some("t"));
        assert!(trigger.interaction.is_none());

        let execution: TriggerExecution =
            serde_json::from_value(serde_json::json!({"status": "completed"})).unwrap();
        assert!(execution.id.is_none());
        assert_eq!(execution.status, Some(TriggerExecutionStatus::Completed));

        // Present-but-partial interaction (identity fields without input)
        // must also degrade rather than fail the trigger.
        let trigger: Trigger = serde_json::from_value(serde_json::json!({
            "id": "t2",
            "interaction": {"agent": "my-agent"}
        }))
        .unwrap();
        let interaction = trigger.interaction.expect("interaction present");
        assert_eq!(interaction.agent.as_deref(), Some("my-agent"));
        // An absent input reads as empty text on this path (documented on
        // the field): indistinguishable from a genuinely empty prompt.
        assert_eq!(
            interaction.input,
            crate::request::InteractionInput::Text(String::new())
        );

        // An interaction carrying an undeserializable `input` — explicit
        // null (serde defaults only cover the key-absent case) or a stray
        // scalar — degrades to empty text too, instead of failing the
        // whole list response. (A malformed steps *array* is deliberately
        // not in this list: under default features the Evergreen Step
        // deserializer absorbs unrecognized elements as Unknown steps, so
        // only scalar shapes are rejectable in every feature mode.)
        for bad_input in [serde_json::Value::Null, serde_json::json!(0)] {
            let trigger: Trigger = serde_json::from_value(serde_json::json!({
                "id": "t3",
                "interaction": {"agent": "my-agent", "input": bad_input}
            }))
            .unwrap();
            let interaction = trigger.interaction.expect("interaction present");
            assert_eq!(
                interaction.input,
                crate::request::InteractionInput::Text(String::new())
            );
        }

        // A non-object `interaction` (stray scalar, array) or one with a
        // type mismatch on a modeled field degrades to None wholesale —
        // the catch-all arms, uniform with the serde_util helpers.
        for bad_interaction in [
            serde_json::json!(0),
            serde_json::json!([5]),
            serde_json::json!({"model": 5}),
        ] {
            let trigger: Trigger = serde_json::from_value(serde_json::json!({
                "id": "t4",
                "interaction": bad_interaction
            }))
            .unwrap();
            assert_eq!(trigger.id.as_deref(), Some("t4"));
            assert!(trigger.interaction.is_none());
        }

        // The leniency is scoped to the response side: the same malformed
        // input in a send-side TriggerCreateParams (e.g. loaded from a
        // config file) is a clean parse error, not a silently scheduled
        // empty prompt.
        let result: Result<TriggerCreateParams, _> = serde_json::from_value(serde_json::json!({
            "schedule": "0 9 * * *",
            "time_zone": "UTC",
            "interaction": {"agent": "my-agent", "input": 0}
        }));
        assert!(result.is_err(), "send-side input must stay strict");
        // Absent (or typo'd, e.g. "inputs") is equally a clean parse
        // error on the send side — `input` is a required field there, so
        // a config mistake cannot silently schedule an empty prompt.
        let result: Result<TriggerCreateParams, _> = serde_json::from_value(serde_json::json!({
            "schedule": "0 9 * * *",
            "time_zone": "UTC",
            "interaction": {"agent": "my-agent", "inputs": "typo"}
        }));
        assert!(result.is_err(), "send-side absent input must stay strict");
    }

    #[test]
    fn execution_status_wire_values() {
        for (status, wire) in [
            (TriggerExecutionStatus::InProgress, "in_progress"),
            (TriggerExecutionStatus::TimedOut, "timed_out"),
        ] {
            assert_eq!(serde_json::to_value(&status).unwrap(), wire);
            // Display is public API and must agree with the wire value.
            assert_eq!(status.to_string(), wire);
        }
        for (status, wire) in [
            (TriggerStatus::Active, "active"),
            (TriggerStatus::Paused, "paused"),
            (TriggerStatus::Error, "error"),
        ] {
            assert_eq!(serde_json::to_value(&status).unwrap(), wire);
            assert_eq!(status.to_string(), wire);
        }
    }
}
