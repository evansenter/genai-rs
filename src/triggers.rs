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
    const fn as_wire(&self) -> Option<&'static str> {
        match self {
            Self::InProgress => Some("in_progress"),
            Self::Completed => Some("completed"),
            Self::Failed => Some("failed"),
            Self::Skipped => Some("skipped"),
            Self::TimedOut => Some("timed_out"),
            Self::Unknown { .. } => None,
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
        match self.as_wire() {
            Some(s) => write!(f, "{s}"),
            None => match self {
                Self::Unknown { status_type, .. } => write!(f, "{status_type}"),
                _ => unreachable!("as_wire covers all known variants"),
            },
        }
    }
}

impl Serialize for TriggerExecutionStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self.as_wire() {
            Some(s) => serializer.serialize_str(s),
            None => match self {
                Self::Unknown { status_type, .. } => serializer.serialize_str(status_type),
                _ => unreachable!("as_wire covers all known variants"),
            },
        }
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
///
/// (No `PartialEq`: the nested [`InteractionRequest`] doesn't implement it.)
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_time: Option<DateTime<Utc>>,
    /// Output only. When the trigger was last updated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_time: Option<DateTime<Utc>>,
    /// Output only. When the trigger last fired.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run_time: Option<DateTime<Utc>>,
    /// Output only. When the trigger next fires.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_run_time: Option<DateTime<Utc>>,
    /// Output only. When the trigger was last paused.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_pause_time: Option<DateTime<Utc>>,
    /// Output only. When the trigger was last resumed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_resume_time: Option<DateTime<Utc>>,
}

/// Request body for creating a [`Trigger`].
///
/// # Example
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
#[derive(Clone, Debug, Serialize, Deserialize)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_consecutive_failures: Option<i64>,
    /// Per-execution timeout in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_timeout_seconds: Option<i64>,
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
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TriggerUpdate {
    /// New display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// New status (`active` to resume, `paused` to pause).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<TriggerStatus>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_time: Option<DateTime<Utc>>,
    /// Output only. When the execution started.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<DateTime<Utc>>,
    /// Output only. When the execution finished.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<DateTime<Utc>>,
}

/// Response from listing triggers.
///
/// The API returns `{}` when no triggers exist (verified live 2026-08-08),
/// so both fields default.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TriggerListResponse {
    /// The triggers in this page.
    #[serde(default)]
    pub triggers: Vec<Trigger>,
    /// Token for fetching the next page, absent on the last page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

/// Response from listing a trigger's executions.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TriggerExecutionListResponse {
    /// The executions in this page.
    #[serde(default)]
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
