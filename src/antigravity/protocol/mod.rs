//! Proto-JSON wire types for the localharness WebSocket protocol.
//!
//! Everything after the stdio handshake travels as **proto-JSON** (the
//! protobuf JSON mapping) over a localhost WebSocket:
//!
//! - field names are `camelCase`;
//! - enums are `SCREAMING_SNAKE_CASE` strings;
//! - 64-bit integers are emitted as JSON *strings* by the harness (this
//!   module accepts both strings and numbers, and re-serializes as numbers —
//!   value-preserving, accepted by the harness's parser);
//! - a `oneof` sets at most one of its fields.
//!
//! Types follow the crate's Evergreen philosophy: unrecognized oneof
//! variants deserialize into `Unknown` variants, unrecognized enum strings
//! are preserved, and unrecognized fields on harness-emitted messages are
//! captured in `extra` maps so they roundtrip.
//!
//! Message and field shapes are verified against the descriptor set shipped
//! in the `google-antigravity` wheel pinned by
//! [`SUPPORTED_HARNESS_VERSION`](super::SUPPORTED_HARNESS_VERSION)
//! (`localharness.proto`, package `antigravity.localharness`). Fields the
//! harness has but this module does not model land in the `extra` maps.

use std::collections::BTreeMap;

mod config;
mod enums;
mod input;
mod output;
mod step_update;

pub use config::{
    AppendedSystemInstructions, CustomAgent, CustomSystemInstructions, FilesystemWorkspace,
    GeminiApiEndpoint, GeminiModelOptions, HarnessConfig, HarnessSideTools,
    InitializeConversationEvent, InstructionSection, McpHttpTransport, McpServerConfig,
    McpStdioTransport, ModelConfig, PermissionsConfig, SystemInstructionPart, SystemInstructions,
    Tool, ToolToggle, VertexEndpoint, Workspace,
};
pub use enums::{
    AgentBehavior, HookDecision, LifecycleHook, LineAction, Modality, ModelType, StepSource,
    StepState, StepTarget, StopReason, TrajectoryState,
};
pub use input::{
    CallHookResponse, EmptyResult, HookVerdict, InputEvent, Media, MultipleChoiceAnswer,
    QuestionsResponse, SlashCommand, ToolConfirmation, ToolResponse, UserInput, UserInputPart,
    UserQuestionAnswer, UserQuestionsResponse,
};
pub use output::{
    CallHookRequest, InitializeConversationResponse, ModalityTokenCount, OutputEvent,
    OutputPayload, PostToolArgs, PostTurnArgs, PreToolArgs, PreTurnArgs, SandboxStatus, ToolCall,
    TrajectoryStateUpdate, TrajectoryUsageEntry, UsageMetadata,
};
pub(crate) use output::{decode_genai_struct, hook_tool_name};
pub use step_update::{
    ActionCompaction, ActionCreateFile, ActionCustomTool, ActionEditFile, ActionError,
    ActionFindFile, ActionFinish, ActionGenerateImage, ActionInvokeSubagent, ActionListDirectory,
    ActionMcpTool, ActionReadUrlContent, ActionRunCommand, ActionSearchDirectory, ActionSearchWeb,
    ActionViewFile, DiffBlock, DiffLine, ListDirectoryEntry, MultipleChoice, StepUpdate,
    ToolConfirmationRequest, UserQuestion, UserQuestionsRequest,
};

// =============================================================================
// Protocol drift telemetry
// =============================================================================

/// Every unrecognized wire value seen this process, and how often.
static DRIFT: std::sync::Mutex<Option<BTreeMap<String, usize>>> = std::sync::Mutex::new(None);

/// Records an unrecognized wire value for [`drift_report`].
///
/// The Evergreen posture preserves what it does not recognize, which
/// prevents a crash but produces no *signal*: a `warn!` nobody reads is
/// the only trace, and behavior keyed on a renamed variant stops firing
/// silently. Accumulating them makes the degradation inspectable —
/// programmatically, not by grepping logs.
pub(crate) fn record_drift(enum_name: &str, value: &str) {
    if let Ok(mut guard) = DRIFT.lock() {
        *guard
            .get_or_insert_with(BTreeMap::new)
            .entry(format!("{enum_name}={value}"))
            .or_insert(0) += 1;
    }
}

/// Unrecognized wire values seen so far, as `"EnumName=WIRE_VALUE" -> count`.
///
/// Empty is the healthy state. A non-empty report means the harness sent
/// something this build does not model — which, for a value the crate
/// *matches on*, is the difference between working and silently doing
/// nothing (see `SUPPORTED_HARNESS_VERSION`). Process-wide and cumulative;
/// [`clear_drift_report`] resets it.
#[must_use]
pub fn drift_report() -> BTreeMap<String, usize> {
    DRIFT
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_default()
}

/// Clears [`drift_report`] (useful between test cases).
pub fn clear_drift_report() {
    if let Ok(mut guard) = DRIFT.lock() {
        *guard = None;
    }
}

// =============================================================================
// Flexible numeric deserialization (proto-JSON int64/uint64 arrive as strings)
// =============================================================================

pub(crate) mod flex_num {
    use serde::{Deserialize, Deserializer, de::Error};
    use serde_json::Value;

    fn value_to_i64<E: Error>(value: &Value) -> Result<i64, E> {
        match value {
            Value::Number(n) => n
                .as_i64()
                .ok_or_else(|| E::custom(format!("number {n} does not fit in i64"))),
            Value::String(s) => s
                .parse::<i64>()
                .map_err(|e| E::custom(format!("invalid i64 string {s:?}: {e}"))),
            other => Err(E::custom(format!("expected i64, got {other}"))),
        }
    }

    fn value_to_u64<E: Error>(value: &Value) -> Result<u64, E> {
        match value {
            Value::Number(n) => n
                .as_u64()
                .ok_or_else(|| E::custom(format!("number {n} does not fit in u64"))),
            Value::String(s) => s
                .parse::<u64>()
                .map_err(|e| E::custom(format!("invalid u64 string {s:?}: {e}"))),
            other => Err(E::custom(format!("expected u64, got {other}"))),
        }
    }

    pub fn opt_u64<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
        match Option::<Value>::deserialize(d)? {
            None | Some(Value::Null) => Ok(None),
            Some(v) => value_to_u64(&v).map(Some),
        }
    }

    pub fn opt_u32<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u32>, D::Error> {
        match Option::<Value>::deserialize(d)? {
            None | Some(Value::Null) => Ok(None),
            Some(v) => {
                let raw = value_to_u64(&v)?;
                u32::try_from(raw)
                    .map(Some)
                    .map_err(|_| Error::custom(format!("value {raw} does not fit in u32")))
            }
        }
    }

    pub fn opt_i32<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i32>, D::Error> {
        match Option::<Value>::deserialize(d)? {
            None | Some(Value::Null) => Ok(None),
            Some(v) => {
                let raw = value_to_i64(&v)?;
                i32::try_from(raw)
                    .map(Some)
                    .map_err(|_| Error::custom(format!("value {raw} does not fit in i32")))
            }
        }
    }
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
