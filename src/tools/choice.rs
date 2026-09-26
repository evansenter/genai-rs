use crate::wire_enum::wire_enum;
use serde::{Deserialize, Serialize};

wire_enum! {
    /// Modes for function calling behavior.
    ///
    /// # Modes
    ///
    /// - `Auto` (default): Model decides whether to call functions or respond naturally
    /// - `Any`: Model must call a function; guarantees schema adherence for calls
    /// - `None`: Prohibits function calling entirely
    /// - `Validated` (Preview): Ensures either function calls OR natural language adhere to schema
    pub enum FunctionCallingMode {
        /// Model decides whether to call functions or respond with natural language.
        Auto = "auto",
        /// Model must call a function; guarantees schema adherence for calls.
        Any = "any",
        /// Function calling is disabled.
        None = "none",
        /// Ensures either function calls OR natural language adhere to schema.
        ///
        /// This is a preview mode that provides schema adherence guarantees
        /// for both function call outputs and natural language responses.
        Validated = "validated",
    }
    unknown(mode_type, unknown_mode_type)
}

/// Restriction on which tools the model may call.
///
/// Used both as the object form of [`ToolChoice`]
/// (`{"allowed_tools": {"mode": ..., "tools": [...]}}`) and as the element
/// type of the MCP server tool's `allowed_tools` list.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AllowedTools {
    /// Function calling mode applied to the listed tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<FunctionCallingMode>,
    /// Names of the tools the model is allowed to call.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
}

impl AllowedTools {
    /// Creates a new tool restriction over the given tool names.
    #[must_use]
    pub fn new(tools: Vec<String>) -> Self {
        Self { mode: None, tools }
    }

    /// Sets the function calling mode for the listed tools.
    #[must_use]
    pub fn with_mode(mut self, mode: FunctionCallingMode) -> Self {
        self.mode = Some(mode);
        self
    }
}

/// The `generation_config.tool_choice` union.
///
/// Either a plain mode string (`"auto" | "any" | "none" | "validated"`) or an
/// object restricting the model to a named set of tools:
/// `{"allowed_tools": {"mode": ..., "tools": [...]}}`.
///
/// # Forward Compatibility
///
/// `#[non_exhaustive]`; unrecognized shapes deserialize into
/// [`ToolChoice::Unknown`] with the data preserved.
///
/// # Example
///
/// ```
/// use genai_rs::{FunctionCallingMode, ToolChoice};
///
/// // Plain mode
/// let choice = ToolChoice::Mode(FunctionCallingMode::Any);
/// assert_eq!(serde_json::to_string(&choice).unwrap(), "\"any\"");
///
/// // Restricted tool set
/// let choice = ToolChoice::allowed_tools(
///     Some(FunctionCallingMode::Any),
///     vec!["get_weather".to_string()],
/// );
/// ```
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ToolChoice {
    /// A plain function calling mode.
    Mode(FunctionCallingMode),
    /// Restriction to a named set of tools (wire: `{"allowed_tools": {...}}`).
    AllowedTools(AllowedTools),
    /// Unknown tool choice shape for forward compatibility.
    Unknown {
        /// A short description of the unrecognized shape (the string value or
        /// object key encountered).
        choice_type: String,
        /// The raw JSON value, preserved for debugging and roundtrip.
        data: serde_json::Value,
    },
}

impl ToolChoice {
    /// Creates a tool restriction choice from a mode and tool names.
    #[must_use]
    pub fn allowed_tools(mode: Option<FunctionCallingMode>, tools: Vec<String>) -> Self {
        Self::AllowedTools(AllowedTools { mode, tools })
    }

    /// Check if this is an unknown tool choice shape.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the choice type descriptor if this is an unknown tool choice.
    #[must_use]
    pub fn unknown_choice_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { choice_type, .. } => Some(choice_type),
            _ => None,
        }
    }

    /// Returns the raw JSON data if this is an unknown tool choice.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl From<FunctionCallingMode> for ToolChoice {
    fn from(mode: FunctionCallingMode) -> Self {
        Self::Mode(mode)
    }
}

impl Serialize for ToolChoice {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        match self {
            Self::Mode(mode) => mode.serialize(serializer),
            Self::AllowedTools(allowed) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("allowed_tools", allowed)?;
                map.end()
            }
            Self::Unknown { data, .. } => data.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ToolChoice {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match &value {
            serde_json::Value::String(_) => {
                // Delegates unknown strings to FunctionCallingMode::Unknown.
                let mode: FunctionCallingMode =
                    serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                Ok(Self::Mode(mode))
            }
            serde_json::Value::Object(obj) if obj.contains_key("allowed_tools") => {
                match serde_json::from_value::<AllowedTools>(obj["allowed_tools"].clone()) {
                    Ok(allowed) => Ok(Self::AllowedTools(allowed)),
                    Err(e) => {
                        tracing::warn!(
                            "Failed to parse tool_choice.allowed_tools: {}. \
                             Preserving in Unknown variant.",
                            e
                        );
                        Ok(Self::Unknown {
                            choice_type: "allowed_tools".to_string(),
                            data: value,
                        })
                    }
                }
            }
            _ => {
                tracing::warn!(
                    "Encountered unknown ToolChoice shape: {}. \
                     Preserving in Unknown variant.",
                    value
                );
                Ok(Self::Unknown {
                    choice_type: format!("<unrecognized: {}>", value),
                    data: value,
                })
            }
        }
    }
}

#[cfg(test)]
#[path = "choice_tests.rs"]
mod tests;
