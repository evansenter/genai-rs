//! Accessors for the wire fields a known step or delta does not model.
//!
//! The (de)serializers keep them (#491); see `crate::wire_fields`.

use serde_json::{Map, Value};

use super::{Step, StepDelta};

/// Matches every known variant of `$enum` binding `$field`, else `$default`.
macro_rules! known_variants {
    (Step, $value:expr, $field:ident => $body:expr, else $default:expr) => {
        match $value {
            Step::UserInput { $field, .. }
            | Step::ModelOutput { $field, .. }
            | Step::Thought { $field, .. }
            | Step::FunctionCall { $field, .. }
            | Step::FunctionResult { $field, .. }
            | Step::CodeExecutionCall { $field, .. }
            | Step::CodeExecutionResult { $field, .. }
            | Step::UrlContextCall { $field, .. }
            | Step::UrlContextResult { $field, .. }
            | Step::GoogleSearchCall { $field, .. }
            | Step::GoogleSearchResult { $field, .. }
            | Step::ToolCall { $field, .. }
            | Step::McpServerToolCall { $field, .. }
            | Step::McpServerToolResult { $field, .. }
            | Step::FileSearchCall { $field, .. }
            | Step::FileSearchResult { $field, .. }
            | Step::GoogleMapsCall { $field, .. }
            | Step::GoogleMapsResult { $field, .. }
            | Step::ProcessingCall { $field, .. }
            | Step::ProcessingResult { $field, .. }
            | Step::RetrievalCall { $field, .. }
            | Step::RetrievalResult { $field, .. } => $body,
            Step::Unknown { .. } => $default,
        }
    };
    (StepDelta, $value:expr, $field:ident => $body:expr, else $default:expr) => {
        match $value {
            StepDelta::Text { $field, .. }
            | StepDelta::Image { $field, .. }
            | StepDelta::Audio { $field, .. }
            | StepDelta::Video { $field, .. }
            | StepDelta::Document { $field, .. }
            | StepDelta::ThoughtSummary { $field, .. }
            | StepDelta::ThoughtSignature { $field, .. }
            | StepDelta::TextAnnotation { $field, .. }
            | StepDelta::ArgumentsDelta { $field, .. }
            | StepDelta::FunctionResult { $field, .. }
            | StepDelta::CodeExecutionCall { $field, .. }
            | StepDelta::CodeExecutionResult { $field, .. }
            | StepDelta::UrlContextCall { $field, .. }
            | StepDelta::UrlContextResult { $field, .. }
            | StepDelta::GoogleSearchCall { $field, .. }
            | StepDelta::GoogleSearchResult { $field, .. }
            | StepDelta::McpServerToolCall { $field, .. }
            | StepDelta::McpServerToolResult { $field, .. }
            | StepDelta::FileSearchCall { $field, .. }
            | StepDelta::FileSearchResult { $field, .. }
            | StepDelta::GoogleMapsCall { $field, .. }
            | StepDelta::GoogleMapsResult { $field, .. }
            | StepDelta::ProcessingCall { $field, .. }
            | StepDelta::ProcessingResult { $field, .. }
            | StepDelta::RetrievalCall { $field, .. }
            | StepDelta::RetrievalResult { $field, .. } => $body,
            StepDelta::Unknown { .. } => $default,
        }
    };
}

/// Matches the five call variants of `$enum` that flatten a nested
/// `arguments` object, binding `arguments_extra`, else `$default`.
macro_rules! reshaped_calls {
    ($enum:ident, $value:expr, $field:ident => $body:expr, else $default:expr) => {
        match $value {
            $enum::CodeExecutionCall { $field, .. }
            | $enum::UrlContextCall { $field, .. }
            | $enum::GoogleSearchCall { $field, .. }
            | $enum::GoogleMapsCall { $field, .. }
            | $enum::RetrievalCall { $field, .. } => $body,
            _ => $default,
        }
    };
}

impl Step {
    /// The fields of this step that the crate does not model.
    ///
    /// Deserializing a known step keeps every key it does not read here, and
    /// serializing writes them back after the modeled fields. A field the
    /// API adds therefore survives a round trip through stateless history
    /// before this crate models it, as `signature` would have (#491). An
    /// entry named `type` or after one of the variant's modeled fields is
    /// never written; the modeled value wins.
    ///
    /// `None` for [`Step::Unknown`], whose `data` already holds the whole
    /// object.
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Step;
    ///
    /// let wire = serde_json::json!({
    ///     "type": "function_call", "id": "c1", "name": "f",
    ///     "arguments": {}, "future_field": 1
    /// });
    /// let step: Step = serde_json::from_value(wire.clone()).unwrap();
    /// assert_eq!(step.extra().unwrap()["future_field"], 1);
    /// assert_eq!(serde_json::to_value(&step).unwrap(), wire);
    /// ```
    #[must_use]
    pub fn extra(&self) -> Option<&Map<String, Value>> {
        known_variants!(Step, self, extra => Some(extra), else None)
    }

    /// Mutable access to [`extra`](Self::extra).
    pub fn extra_mut(&mut self) -> Option<&mut Map<String, Value>> {
        known_variants!(Step, self, extra => Some(extra), else None)
    }

    /// The keys of the wire `arguments` object that the crate does not
    /// model, on the five call steps that flatten it: `code_execution_call`
    /// (`language`, `code`), `url_context_call` (`urls`),
    /// `google_search_call`, `google_maps_call` and `retrieval_call`
    /// (`queries`).
    ///
    /// Serializing merges them back into `arguments` after the modeled
    /// keys, which win a collision. `None` for every other step.
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Step;
    ///
    /// let wire = serde_json::json!({
    ///     "type": "url_context_call", "id": "c1",
    ///     "arguments": {"urls": ["https://example.com"], "max_pages": 3}
    /// });
    /// let step: Step = serde_json::from_value(wire.clone()).unwrap();
    /// assert_eq!(step.arguments_extra().unwrap()["max_pages"], 3);
    /// assert_eq!(serde_json::to_value(&step).unwrap(), wire);
    /// ```
    #[must_use]
    pub fn arguments_extra(&self) -> Option<&Map<String, Value>> {
        reshaped_calls!(Step, self, arguments_extra => Some(arguments_extra), else None)
    }

    /// Mutable access to [`arguments_extra`](Self::arguments_extra).
    pub fn arguments_extra_mut(&mut self) -> Option<&mut Map<String, Value>> {
        reshaped_calls!(Step, self, arguments_extra => Some(arguments_extra), else None)
    }
}

impl StepDelta {
    /// The fields of this delta that the crate does not model; see
    /// [`Step::extra`]. The stream accumulator merges them into the step or
    /// content block the delta builds. `None` for [`StepDelta::Unknown`].
    #[must_use]
    pub fn extra(&self) -> Option<&Map<String, Value>> {
        known_variants!(StepDelta, self, extra => Some(extra), else None)
    }

    /// The unmodeled keys of a call delta's `arguments` object; see
    /// [`Step::arguments_extra`].
    #[must_use]
    pub fn arguments_extra(&self) -> Option<&Map<String, Value>> {
        reshaped_calls!(StepDelta, self, arguments_extra => Some(arguments_extra), else None)
    }
}
