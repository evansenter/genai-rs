use super::record_drift;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// =============================================================================
// String-valued wire enums (Evergreen: unknown values are preserved)
// =============================================================================

/// Generates a proto-JSON string enum with an `Unknown` variant that
/// preserves unrecognized wire values, plus the crate-standard helper
/// methods (`is_unknown`, `unknown_<context>_type`, `unknown_data`).
macro_rules! wire_string_enum {
    (
        $(#[$meta:meta])*
        $name:ident, $ctx:ident, $unknown_type_fn:ident {
            $( $(#[$vmeta:meta])* $variant:ident => $wire:literal $(| $alias:literal)* ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq)]
        #[non_exhaustive]
        pub enum $name {
            $( $(#[$vmeta])* $variant, )+
            /// A wire value this crate does not recognize (Evergreen).
            Unknown {
                /// The unrecognized enum string from the harness.
                $ctx: String,
                /// The raw JSON value, preserved for roundtrip.
                data: Value,
            },
        }

        impl $name {
            /// Returns the proto-JSON wire string for this value.
            #[must_use]
            pub fn as_wire_str(&self) -> &str {
                match self {
                    $( Self::$variant => $wire, )+
                    Self::Unknown { $ctx, .. } => $ctx,
                }
            }

            /// Every wire spelling this enum recognizes, canonical and
            /// alias alike.
            ///
            /// Exists for the protocol-drift guard in
            /// `tests/antigravity_harness.rs`, which diffs these against
            /// the enum values in the installed harness wheel's protobuf
            /// descriptor. A value the harness gained (or renamed) is
            /// otherwise invisible: it deserializes to `Unknown` and any
            /// behavior keyed on the old variant silently stops firing.
            #[must_use]
            pub const fn all_wire_values() -> &'static [&'static str] {
                &[$( $wire $(, $alias)* ),+]
            }

            /// Check if this is an unknown value.
            #[must_use]
            pub const fn is_unknown(&self) -> bool {
                matches!(self, Self::Unknown { .. })
            }

            /// Returns the unrecognized wire string if this is an unknown value.
            #[must_use]
            pub fn $unknown_type_fn(&self) -> Option<&str> {
                match self {
                    Self::Unknown { $ctx, .. } => Some($ctx),
                    _ => None,
                }
            }

            /// Returns the preserved raw JSON if this is an unknown value.
            #[must_use]
            pub fn unknown_data(&self) -> Option<&Value> {
                match self {
                    Self::Unknown { data, .. } => Some(data),
                    _ => None,
                }
            }
        }

        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_wire_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = Value::deserialize(deserializer)?;
                if let Value::String(s) = &value {
                    match s.as_str() {
                        // Aliases accept spellings from other harness
                        // revisions; `as_wire_str` always emits the
                        // canonical (current-harness) form.
                        $( $wire $(| $alias)* => return Ok(Self::$variant), )+
                        _ => {}
                    }
                }
                let $ctx = match &value {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                tracing::warn!(
                    concat!("Unknown ", stringify!($name), " wire value: '{}'. \
                     Preserving in Unknown variant."),
                    $ctx
                );
                // Also accumulate it: a warn is only seen by whoever is
                // reading logs at the time, and this is the signal that
                // distinguishes "preserved harmlessly" from "the bridge
                // stopped recognizing something it acts on".
                record_drift(stringify!($name), &$ctx);
                Ok(Self::Unknown { $ctx, data: value })
            }
        }
    };
}

wire_string_enum!(
    /// `StepUpdate.State` — lifecycle state of one agent step.
    StepState, state_type, unknown_state_type {
        /// `STATE_UNSPECIFIED`.
        Unspecified => "STATE_UNSPECIFIED",
        /// The step is executing.
        Active => "STATE_ACTIVE",
        /// The step completed successfully.
        Done => "STATE_DONE",
        /// The step is blocked on client input (confirmation or questions).
        WaitingForUser => "STATE_WAITING_FOR_USER",
        /// The step failed.
        Error => "STATE_ERROR",
    }
);

wire_string_enum!(
    /// `StepUpdate.Source` — who produced a step.
    StepSource, source_type, unknown_source_type {
        /// `SOURCE_UNSPECIFIED`.
        Unspecified => "SOURCE_UNSPECIFIED",
        /// Emitted by the platform (e.g. system errors).
        System => "SOURCE_SYSTEM",
        /// Emitted on behalf of the user.
        User => "SOURCE_USER",
        /// Emitted by the model.
        Model => "SOURCE_MODEL",
    }
);

wire_string_enum!(
    /// `StepUpdate.Target` — who a step is directed at.
    StepTarget, target_type, unknown_target_type {
        /// `TARGET_UNSPECIFIED`.
        Unspecified => "TARGET_UNSPECIFIED",
        /// Directed at the user (e.g. final response text).
        User => "TARGET_USER",
        /// Directed at the model.
        Model => "TARGET_MODEL",
        /// Directed at the environment (e.g. tool executions).
        Environment => "TARGET_ENVIRONMENT",
    }
);

wire_string_enum!(
    /// `TrajectoryStateUpdate.State` — lifecycle state of one trajectory.
    TrajectoryState, state_type, unknown_state_type {
        /// `STATE_UNSPECIFIED`.
        Unspecified => "STATE_UNSPECIFIED",
        /// The trajectory is processing a turn.
        Running => "STATE_RUNNING",
        /// The trajectory finished the turn and is awaiting input — the
        /// signal that ends a turn.
        ///
        /// Harness 0.1.10 renamed this from `STATE_IDLE` to
        /// `STATE_FULLY_IDLE`; the old spelling is accepted as an alias
        /// so one build drives either harness revision. Without the
        /// alias the value degrades to `Unknown` and, because only
        /// `Idle` ends a turn, every turn silently runs to its timeout.
        Idle => "STATE_FULLY_IDLE" | "STATE_IDLE",
        /// The trajectory is blocked on subordinate tasks (e.g. running
        /// subagents) and will return to `Running`. Explicitly **not**
        /// terminal — new in harness 0.1.10.
        WaitingForTasks => "STATE_WAITING_FOR_TASKS",
        /// The turn was cancelled (halt request or pre-turn hook denial).
        Cancelled => "STATE_CANCELLED",
    }
);

wire_string_enum!(
    /// `TrajectoryStateUpdate.StopReason` — why a trajectory stopped, when
    /// it was not a normal completion (new in harness 0.1.18).
    ///
    /// Arrives alongside a terminal trajectory state. The budget reasons
    /// only fire when a harness-side budget is configured; quota
    /// exhaustion can arrive on any turn.
    StopReason, reason_type, unknown_reason_type {
        /// `STOP_REASON_UNSPECIFIED` — normal completion.
        Unspecified => "STOP_REASON_UNSPECIFIED",
        /// The session exceeded its model-call budget.
        MaxModelCallsExceeded => "STOP_REASON_MAX_MODEL_CALLS_EXCEEDED",
        /// The session exceeded its tool-call budget.
        MaxToolCallsExceeded => "STOP_REASON_MAX_TOOL_CALLS_EXCEEDED",
        /// The session exceeded its input-token budget.
        MaxInputTokensExceeded => "STOP_REASON_MAX_INPUT_TOKENS_EXCEEDED",
        /// The session exceeded its output-token budget.
        MaxOutputTokensExceeded => "STOP_REASON_MAX_OUTPUT_TOKENS_EXCEEDED",
        /// The session exceeded its total-token budget.
        MaxTotalTokensExceeded => "STOP_REASON_MAX_TOTAL_TOKENS_EXCEEDED",
        /// The model backend's quota was exhausted.
        QuotaExhausted => "STOP_REASON_QUOTA_EXHAUSTED",
    }
);

wire_string_enum!(
    /// `Modality` — the content modality a token count applies to (new in
    /// harness 0.1.18, on the per-modality usage breakdowns).
    ///
    /// Note the bare wire spellings (`TEXT`, not `MODALITY_TEXT`): only the
    /// unspecified value carries the prefix.
    Modality, modality_type, unknown_modality_type {
        /// `MODALITY_UNSPECIFIED`.
        Unspecified => "MODALITY_UNSPECIFIED",
        /// Text.
        Text => "TEXT",
        /// Images.
        Image => "IMAGE",
        /// Video.
        Video => "VIDEO",
        /// Audio.
        Audio => "AUDIO",
        /// Documents (e.g. PDF).
        Document => "DOCUMENT",
    }
);

wire_string_enum!(
    /// `AgentBehavior` — how the harness frames the agent's task in its
    /// system prompt (new in harness 0.1.18). Client → harness only.
    AgentBehavior, behavior_type, unknown_behavior_type {
        /// `AGENT_BEHAVIOR_UNSPECIFIED` — the harness default, which on
        /// 0.1.18 is identical to `Autonomous` (verified by diffing the
        /// system prompt it sends the model).
        Unspecified => "AGENT_BEHAVIOR_UNSPECIFIED",
        /// "The user will not respond to questions" — work unattended.
        Autonomous => "AGENT_BEHAVIOR_AUTONOMOUS",
        /// Work with a human in the loop: clarifying questions, artifacts.
        Interactive => "AGENT_BEHAVIOR_INTERACTIVE",
        /// A pruned prompt for small-context models.
        Minimal => "AGENT_BEHAVIOR_MINIMAL",
    }
);

wire_string_enum!(
    /// `ModelType` — the roles a configured model can serve.
    ModelType, model_type, unknown_model_type {
        /// `MODEL_TYPE_UNSPECIFIED`.
        Unspecified => "MODEL_TYPE_UNSPECIFIED",
        /// Text generation model.
        Text => "MODEL_TYPE_TEXT",
        /// Image generation model.
        Image => "MODEL_TYPE_IMAGE",
    }
);

wire_string_enum!(
    /// `LifecycleHook` — hook points the harness can call back into.
    LifecycleHook, hook_type, unknown_hook_type {
        /// `LIFECYCLE_HOOK_UNSPECIFIED`.
        Unspecified => "LIFECYCLE_HOOK_UNSPECIFIED",
        /// Fired when the session starts.
        OnSessionStart => "LIFECYCLE_HOOK_ON_SESSION_START",
        /// Fired when the session ends.
        OnSessionEnd => "LIFECYCLE_HOOK_ON_SESSION_END",
        /// Fired before each user turn.
        PreTurn => "LIFECYCLE_HOOK_PRE_TURN",
        /// Fired after each user turn.
        PostTurn => "LIFECYCLE_HOOK_POST_TURN",
        /// Fired before each tool call (may deny it).
        PreTool => "LIFECYCLE_HOOK_PRE_TOOL",
        /// Fired after each successful tool call.
        PostTool => "LIFECYCLE_HOOK_POST_TOOL",
        /// Fired when a tool call errors.
        OnToolError => "LIFECYCLE_HOOK_ON_TOOL_ERROR",
        /// Fired after history compaction (new in harness 0.1.18).
        OnCompaction => "LIFECYCLE_HOOK_ON_COMPACTION",
        /// Fired when a turn is about to stop; the hook may ask the agent
        /// to continue instead (new in harness 0.1.18).
        Stop => "LIFECYCLE_HOOK_STOP",
    }
);

wire_string_enum!(
    /// `PreToolResult.Decision` / `PreTurnResult.Decision` — hook verdicts.
    HookDecision, decision_type, unknown_decision_type {
        /// `DECISION_UNSPECIFIED`.
        Unspecified => "DECISION_UNSPECIFIED",
        /// Allow the operation.
        Allow => "ALLOW",
        /// Deny the operation.
        Deny => "DENY",
    }
);

wire_string_enum!(
    /// `ActionEditFile.DiffLine.LineAction` — per-line diff operation.
    LineAction, action_type, unknown_action_type {
        /// `LINE_ACTION_UNSPECIFIED`.
        Unspecified => "LINE_ACTION_UNSPECIFIED",
        /// The line was inserted.
        Insert => "LINE_ACTION_INSERT",
        /// The line was deleted.
        Delete => "LINE_ACTION_DELETE",
        /// The line is unchanged context.
        None => "LINE_ACTION_NONE",
    }
);
