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

impl fmt::Display for TriggerStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Paused => write!(f, "paused"),
            Self::Error => write!(f, "error"),
            Self::Unknown { status_type, .. } => write!(f, "{status_type}"),
        }
    }
}

impl Serialize for TriggerStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Active => serializer.serialize_str("active"),
            Self::Paused => serializer.serialize_str("paused"),
            Self::Error => serializer.serialize_str("error"),
            Self::Unknown { status_type, .. } => serializer.serialize_str(status_type),
        }
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
            Some(other) => Ok(Self::Unknown {
                status_type: other.to_string(),
                data: value.clone(),
            }),
            None => Ok(Self::Unknown {
                status_type: String::new(),
                data: value,
            }),
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
            Some(other) => Ok(Self::Unknown {
                status_type: other.to_string(),
                data: value.clone(),
            }),
            None => Ok(Self::Unknown {
                status_type: String::new(),
                data: value,
            }),
        }
    }
}

/// A server-side scheduled trigger, as returned by `/v1beta/triggers`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Trigger {
    /// Output only. The ID of the trigger.
    pub id: String,
    /// Cron expression the trigger fires on.
    pub schedule: String,
    /// IANA time zone the schedule is evaluated in (e.g. `"UTC"`).
    pub time_zone: String,
    /// The interaction request created on each firing.
    pub interaction: InteractionRequest,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_consecutive_failures: Option<i64>,
    /// Output only. Current count of consecutive failed executions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consecutive_failure_count: Option<i64>,
    /// Per-execution timeout in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
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

/// Partial update for a [`Trigger`] — only the set fields change.
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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TriggerExecution {
    /// Output only. The ID of the execution.
    pub id: String,
    /// Output only. The ID of the trigger that fired.
    pub trigger_id: String,
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
        assert_eq!(trigger.status, Some(TriggerStatus::Active));
        assert!(trigger.next_run_time.is_some());
    }

    #[test]
    fn execution_status_wire_values() {
        for (status, wire) in [
            (TriggerExecutionStatus::InProgress, "in_progress"),
            (TriggerExecutionStatus::TimedOut, "timed_out"),
        ] {
            assert_eq!(serde_json::to_value(&status).unwrap(), wire);
        }
    }
}
