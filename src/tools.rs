// Shared types used by the Interactions API

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents a tool that can be used by the model (Interactions API format).
///
/// Tools in the Interactions API use a flat structure with the tool type and details
/// at the top level, rather than nested in arrays.
///
/// # Forward Compatibility (Evergreen Philosophy)
///
/// This enum is marked `#[non_exhaustive]`, which means:
/// - Match statements must include a wildcard arm (`_ => ...`)
/// - New variants may be added in minor version updates without breaking your code
///
/// When the API returns a tool type that this library doesn't recognize, it will be
/// captured as `Tool::Unknown` rather than causing a deserialization error.
/// This follows the [Evergreen spec](https://github.com/google-deepmind/evergreen-spec)
/// philosophy of graceful degradation.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum Tool {
    /// A custom function that the model can call
    Function {
        name: String,
        description: String,
        parameters: FunctionParameters,
    },
    /// Built-in Google Search tool
    ///
    /// Optionally configure `search_types` to enable web and/or image search.
    GoogleSearch {
        /// Types of search to perform (e.g., web search, image search).
        /// When `None`, the API defaults to web search only.
        search_types: Option<Vec<SearchType>>,
    },
    /// Built-in Google Maps tool for location-grounded responses
    GoogleMaps {
        /// Whether to enable the widget context token in the response
        enable_widget: Option<bool>,
        /// Latitude bias for location grounding
        latitude: Option<f64>,
        /// Longitude bias for location grounding
        longitude: Option<f64>,
    },
    /// Built-in code execution tool
    CodeExecution,
    /// Built-in URL context tool
    UrlContext,
    /// Built-in computer use tool for browser automation.
    ///
    /// **Security Warning**: This tool allows the model to interact with web browsers
    /// on your behalf. Only use with trusted models and carefully review excluded functions.
    ComputerUse {
        /// The environment being operated. Known values: `browser`, `mobile`, `desktop`.
        environment: String,
        /// List of predefined functions to exclude from model access
        /// (wire: `excluded_predefined_functions`).
        excluded_predefined_functions: Vec<String>,
        /// Whether to enable prompt injection detection for this request.
        enable_prompt_injection_detection: Option<bool>,
        /// Safety policies to disable. Known values include
        /// `financial_transactions`, `sensitive_data_modification`,
        /// `communication_tool`, `account_creation`, `data_modification`,
        /// `user_consent_management`, `legal_terms_and_agreements`.
        disabled_safety_policies: Vec<String>,
    },
    /// Model Context Protocol (MCP) server
    McpServer {
        name: String,
        url: String,
        /// Optional per-mode restrictions on which server tools the model may
        /// call (wire: `allowed_tools: [{mode, tools}]`).
        allowed_tools: Option<Vec<AllowedTools>>,
        /// Optional headers for authentication or configuration
        headers: Option<HashMap<String, String>>,
    },
    /// Built-in file search tool for semantic retrieval over document stores
    FileSearch {
        /// Names of file search stores to query (wire: `file_search_store_names`)
        store_names: Vec<String>,
        /// Number of semantic retrieval chunks to retrieve
        top_k: Option<i32>,
        /// Metadata filter for documents and chunks
        metadata_filter: Option<String>,
    },
    /// Built-in retrieval tool for grounding over external retrieval
    /// backends (Vertex AI Search, RAG stores, Exa.ai, Parallel.ai).
    ///
    /// Prefer constructing via [`RetrievalConfig`], which keeps
    /// `retrieval_types` in sync with the per-backend configs.
    Retrieval {
        /// The retrieval backends to enable.
        retrieval_types: Option<Vec<RetrievalType>>,
        /// Configuration for Vertex AI Search.
        vertex_ai_search_config: Option<VertexAiSearchConfig>,
        /// Configuration for Exa.ai search.
        exa_ai_search_config: Option<ExaAiSearchConfig>,
        /// Configuration for Parallel.ai search.
        parallel_ai_search_config: Option<ParallelAiSearchConfig>,
        /// Configuration for RAG Store retrieval.
        ///
        /// Boxed to keep the `Tool` enum small (this is the largest tool
        /// configuration).
        rag_store_config: Option<Box<RagStoreConfig>>,
    },
    /// Unknown tool type for forward compatibility.
    ///
    /// This variant captures tool types that the library doesn't recognize yet.
    /// This can happen when Google adds new built-in tools before this library
    /// is updated to support them.
    ///
    /// The `tool_type` field contains the unrecognized type string from the API,
    /// and `data` contains the full JSON object for inspection or debugging.
    Unknown {
        /// The unrecognized tool type name from the API
        tool_type: String,
        /// The full JSON data for this tool, preserved for debugging
        data: serde_json::Value,
    },
}

// Custom Serialize implementation for Tool.
// This handles the Unknown variant by merging tool_type into the data.
impl Serialize for Tool {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        match self {
            Self::Function {
                name,
                description,
                parameters,
            } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "function")?;
                map.serialize_entry("name", name)?;
                map.serialize_entry("description", description)?;
                map.serialize_entry("parameters", parameters)?;
                map.end()
            }
            Self::GoogleSearch { search_types } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "google_search")?;
                if let Some(types) = search_types
                    && !types.is_empty()
                {
                    map.serialize_entry("search_types", types)?;
                }
                map.end()
            }
            Self::GoogleMaps {
                enable_widget,
                latitude,
                longitude,
            } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "google_maps")?;
                if let Some(ew) = enable_widget {
                    map.serialize_entry("enable_widget", ew)?;
                }
                if let Some(lat) = latitude {
                    map.serialize_entry("latitude", lat)?;
                }
                if let Some(lng) = longitude {
                    map.serialize_entry("longitude", lng)?;
                }
                map.end()
            }
            Self::CodeExecution => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "code_execution")?;
                map.end()
            }
            Self::UrlContext => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "url_context")?;
                map.end()
            }
            Self::ComputerUse {
                environment,
                excluded_predefined_functions,
                enable_prompt_injection_detection,
                disabled_safety_policies,
            } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "computer_use")?;
                map.serialize_entry("environment", environment)?;
                if !excluded_predefined_functions.is_empty() {
                    map.serialize_entry(
                        "excluded_predefined_functions",
                        excluded_predefined_functions,
                    )?;
                }
                if let Some(detect) = enable_prompt_injection_detection {
                    map.serialize_entry("enable_prompt_injection_detection", detect)?;
                }
                if !disabled_safety_policies.is_empty() {
                    map.serialize_entry("disabled_safety_policies", disabled_safety_policies)?;
                }
                map.end()
            }
            Self::McpServer {
                name,
                url,
                allowed_tools,
                headers,
            } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "mcp_server")?;
                map.serialize_entry("name", name)?;
                map.serialize_entry("url", url)?;
                if let Some(tools) = allowed_tools
                    && !tools.is_empty()
                {
                    map.serialize_entry("allowed_tools", tools)?;
                }
                if let Some(hdrs) = headers
                    && !hdrs.is_empty()
                {
                    map.serialize_entry("headers", hdrs)?;
                }
                map.end()
            }
            Self::FileSearch {
                store_names,
                top_k,
                metadata_filter,
            } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "file_search")?;
                map.serialize_entry("file_search_store_names", store_names)?;
                if let Some(k) = top_k {
                    map.serialize_entry("top_k", k)?;
                }
                if let Some(filter) = metadata_filter {
                    map.serialize_entry("metadata_filter", filter)?;
                }
                map.end()
            }
            Self::Retrieval {
                retrieval_types,
                vertex_ai_search_config,
                exa_ai_search_config,
                parallel_ai_search_config,
                rag_store_config,
            } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "retrieval")?;
                if let Some(types) = retrieval_types
                    && !types.is_empty()
                {
                    map.serialize_entry("retrieval_types", types)?;
                }
                if let Some(config) = vertex_ai_search_config {
                    map.serialize_entry("vertex_ai_search_config", config)?;
                }
                if let Some(config) = exa_ai_search_config {
                    map.serialize_entry("exa_ai_search_config", config)?;
                }
                if let Some(config) = parallel_ai_search_config {
                    map.serialize_entry("parallel_ai_search_config", config)?;
                }
                if let Some(config) = rag_store_config {
                    map.serialize_entry("rag_store_config", config)?;
                }
                map.end()
            }
            Self::Unknown { tool_type, data } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", tool_type)?;
                // Flatten the data fields into the map if it's an object
                if let serde_json::Value::Object(obj) = data {
                    for (key, value) in obj {
                        if key != "type" {
                            map.serialize_entry(key, value)?;
                        }
                    }
                } else if !data.is_null() {
                    map.serialize_entry("data", data)?;
                }
                map.end()
            }
        }
    }
}

// Custom Deserialize implementation to handle unknown tool types gracefully.
impl<'de> Deserialize<'de> for Tool {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // First, deserialize into a raw JSON value
        let value = serde_json::Value::deserialize(deserializer)?;

        // Helper enum for deserializing known types
        // Note: variant names must match the serialized "type" field values exactly
        #[derive(Deserialize)]
        #[serde(tag = "type")]
        enum KnownTool {
            #[serde(rename = "function")]
            Function {
                name: String,
                description: String,
                parameters: FunctionParameters,
            },
            #[serde(rename = "google_search")]
            GoogleSearch {
                #[serde(default)]
                search_types: Option<Vec<SearchType>>,
            },
            #[serde(rename = "google_maps")]
            GoogleMaps {
                #[serde(default)]
                enable_widget: Option<bool>,
                #[serde(default)]
                latitude: Option<f64>,
                #[serde(default)]
                longitude: Option<f64>,
            },
            #[serde(rename = "code_execution")]
            CodeExecution,
            #[serde(rename = "url_context")]
            UrlContext,
            #[serde(rename = "computer_use")]
            ComputerUse {
                environment: String,
                // Spec wire format is snake_case; accept the legacy camelCase
                // alias for pre-revision payloads.
                #[serde(default, alias = "excludedPredefinedFunctions")]
                excluded_predefined_functions: Vec<String>,
                #[serde(default)]
                enable_prompt_injection_detection: Option<bool>,
                #[serde(default)]
                disabled_safety_policies: Vec<String>,
            },
            #[serde(rename = "mcp_server")]
            McpServer {
                name: String,
                url: String,
                #[serde(default)]
                allowed_tools: Option<Vec<AllowedTools>>,
                #[serde(default)]
                headers: Option<HashMap<String, String>>,
            },
            #[serde(rename = "file_search")]
            FileSearch {
                #[serde(rename = "file_search_store_names")]
                store_names: Vec<String>,
                #[serde(default)]
                top_k: Option<i32>,
                #[serde(default)]
                metadata_filter: Option<String>,
            },
            #[serde(rename = "retrieval")]
            Retrieval {
                #[serde(default)]
                retrieval_types: Option<Vec<RetrievalType>>,
                #[serde(default)]
                vertex_ai_search_config: Option<VertexAiSearchConfig>,
                #[serde(default)]
                exa_ai_search_config: Option<ExaAiSearchConfig>,
                #[serde(default)]
                parallel_ai_search_config: Option<ParallelAiSearchConfig>,
                #[serde(default)]
                rag_store_config: Option<Box<RagStoreConfig>>,
            },
        }

        // Try to deserialize as a known type
        match serde_json::from_value::<KnownTool>(value.clone()) {
            Ok(known) => Ok(match known {
                KnownTool::Function {
                    name,
                    description,
                    parameters,
                } => Tool::Function {
                    name,
                    description,
                    parameters,
                },
                KnownTool::GoogleSearch { search_types } => Tool::GoogleSearch { search_types },
                KnownTool::GoogleMaps {
                    enable_widget,
                    latitude,
                    longitude,
                } => Tool::GoogleMaps {
                    enable_widget,
                    latitude,
                    longitude,
                },
                KnownTool::CodeExecution => Tool::CodeExecution,
                KnownTool::UrlContext => Tool::UrlContext,
                KnownTool::ComputerUse {
                    environment,
                    excluded_predefined_functions,
                    enable_prompt_injection_detection,
                    disabled_safety_policies,
                } => Tool::ComputerUse {
                    environment,
                    excluded_predefined_functions,
                    enable_prompt_injection_detection,
                    disabled_safety_policies,
                },
                KnownTool::McpServer {
                    name,
                    url,
                    allowed_tools,
                    headers,
                } => Tool::McpServer {
                    name,
                    url,
                    allowed_tools,
                    headers,
                },
                KnownTool::FileSearch {
                    store_names,
                    top_k,
                    metadata_filter,
                } => Tool::FileSearch {
                    store_names,
                    top_k,
                    metadata_filter,
                },
                KnownTool::Retrieval {
                    retrieval_types,
                    vertex_ai_search_config,
                    exa_ai_search_config,
                    parallel_ai_search_config,
                    rag_store_config,
                } => Tool::Retrieval {
                    retrieval_types,
                    vertex_ai_search_config,
                    exa_ai_search_config,
                    parallel_ai_search_config,
                    rag_store_config,
                },
            }),
            Err(parse_error) => {
                // Unknown type - extract type name and preserve data
                let tool_type = value
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<missing type>")
                    .to_string();

                // Log the actual parse error for debugging - this helps distinguish
                // between truly unknown types and malformed known types
                tracing::warn!(
                    "Encountered unknown Tool type '{}'. \
                     Parse error: {}. \
                     This may indicate a new API feature or a malformed response. \
                     The tool will be preserved in the Unknown variant.",
                    tool_type,
                    parse_error
                );

                Ok(Tool::Unknown {
                    tool_type,
                    data: value,
                })
            }
        }
    }
}

impl Tool {
    /// Check if this is an unknown tool type.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the tool type name if this is an unknown tool type.
    ///
    /// Returns `None` for known tool types.
    #[must_use]
    pub fn unknown_tool_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { tool_type, .. } => Some(tool_type),
            _ => None,
        }
    }

    /// Returns the raw JSON data if this is an unknown tool type.
    ///
    /// Returns `None` for known tool types.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

/// Represents a function that can be called by the model.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct FunctionDeclaration {
    name: String,
    description: String,
    parameters: FunctionParameters,
}

/// Represents the parameters schema for a function.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct FunctionParameters {
    #[serde(rename = "type")]
    type_: String,
    properties: serde_json::Value,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    required: Vec<String>,
}

impl FunctionDeclaration {
    /// Creates a new FunctionDeclaration with the given fields.
    ///
    /// This is primarily intended for internal use by the macro system.
    /// For manual construction, prefer using `FunctionDeclaration::builder()`.
    #[doc(hidden)]
    pub fn new(name: String, description: String, parameters: FunctionParameters) -> Self {
        Self {
            name,
            description,
            parameters,
        }
    }

    /// Creates a builder for ergonomic FunctionDeclaration construction
    #[must_use]
    pub fn builder(name: impl Into<String>) -> FunctionDeclarationBuilder {
        FunctionDeclarationBuilder::new(name)
    }

    /// Returns the function name
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the function description
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Returns a reference to the function parameters
    #[must_use]
    pub fn parameters(&self) -> &FunctionParameters {
        &self.parameters
    }

    /// Converts this FunctionDeclaration into a Tool for API requests
    #[must_use]
    pub fn into_tool(self) -> Tool {
        Tool::Function {
            name: self.name,
            description: self.description,
            parameters: self.parameters,
        }
    }
}

impl FunctionParameters {
    /// Creates a new FunctionParameters with the given fields.
    ///
    /// This is primarily intended for internal use by the macro system.
    /// For manual construction, prefer using `FunctionDeclaration::builder()`.
    #[doc(hidden)]
    pub fn new(type_: String, properties: serde_json::Value, required: Vec<String>) -> Self {
        Self {
            type_,
            properties,
            required,
        }
    }

    /// Returns the parameter type (typically "object")
    #[must_use]
    pub fn type_(&self) -> &str {
        &self.type_
    }

    /// Returns the properties schema
    #[must_use]
    pub fn properties(&self) -> &serde_json::Value {
        &self.properties
    }

    /// Returns the list of required parameter names
    #[must_use]
    pub fn required(&self) -> &[String] {
        &self.required
    }
}

/// Builder for ergonomic FunctionDeclaration creation
#[derive(Debug)]
pub struct FunctionDeclarationBuilder {
    name: String,
    description: String,
    properties: serde_json::Value,
    required: Vec<String>,
}

impl FunctionDeclarationBuilder {
    /// Creates a new builder with the given function name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            properties: serde_json::Value::Object(serde_json::Map::new()),
            required: Vec::new(),
        }
    }

    /// Sets the function description
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Adds a parameter to the function schema
    pub fn parameter(mut self, name: &str, schema: serde_json::Value) -> Self {
        if let serde_json::Value::Object(ref mut map) = self.properties {
            map.insert(name.to_string(), schema);
        }
        self
    }

    /// Sets the list of required parameter names
    pub fn required(mut self, required: Vec<String>) -> Self {
        self.required = required;
        self
    }

    /// Builds the FunctionDeclaration
    ///
    /// # Validation
    ///
    /// This method performs validation and logs warnings for:
    /// - Empty or whitespace-only function names
    /// - Required parameters that don't exist in the properties schema
    ///
    /// These conditions may cause API errors but are allowed by the builder
    /// for backwards compatibility.
    pub fn build(self) -> FunctionDeclaration {
        // Validate function name
        if self.name.trim().is_empty() {
            tracing::warn!(
                "FunctionDeclaration built with empty or whitespace-only name. \
                This will likely be rejected by the API."
            );
        }

        // Validate required parameters exist in properties
        if let serde_json::Value::Object(ref props) = self.properties {
            for req in &self.required {
                if !props.contains_key(req) {
                    tracing::warn!(
                        "FunctionDeclaration '{}' requires parameter '{}' which is not defined in properties. \
                        This will likely cause API errors.",
                        self.name,
                        req
                    );
                }
            }
        }

        FunctionDeclaration {
            name: self.name,
            description: self.description,
            parameters: FunctionParameters {
                type_: "object".to_string(),
                properties: self.properties,
                required: self.required,
            },
        }
    }
}

/// Modes for function calling behavior.
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
/// New modes may be added in future versions.
///
/// # Forward Compatibility (Evergreen Philosophy)
///
/// When the API returns a mode value that this library doesn't recognize,
/// it will be captured as `FunctionCallingMode::Unknown` rather than
/// causing a deserialization error. This follows the
/// [Evergreen spec](https://github.com/google-deepmind/evergreen-spec)
/// philosophy of graceful degradation.
///
/// # Modes
///
/// - `Auto` (default): Model decides whether to call functions or respond naturally
/// - `Any`: Model must call a function; guarantees schema adherence for calls
/// - `None`: Prohibits function calling entirely
/// - `Validated` (Preview): Ensures either function calls OR natural language adhere to schema
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum FunctionCallingMode {
    /// Model decides whether to call functions or respond with natural language.
    Auto,
    /// Model must call a function; guarantees schema adherence for calls.
    Any,
    /// Function calling is disabled.
    None,
    /// Ensures either function calls OR natural language adhere to schema.
    ///
    /// This is a preview mode that provides schema adherence guarantees
    /// for both function call outputs and natural language responses.
    Validated,
    /// Unknown mode (for forward compatibility).
    ///
    /// This variant captures any unrecognized mode values from the API,
    /// allowing the library to handle new modes gracefully.
    ///
    /// The `mode_type` field contains the unrecognized mode string,
    /// and `data` contains the JSON value (typically the same string).
    Unknown {
        /// The unrecognized mode string from the API
        mode_type: String,
        /// The raw JSON value, preserved for debugging
        data: serde_json::Value,
    },
}

impl FunctionCallingMode {
    /// Check if this is an unknown mode.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the mode type name if this is an unknown mode.
    ///
    /// Returns `None` for known modes.
    #[must_use]
    pub fn unknown_mode_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { mode_type, .. } => Some(mode_type),
            _ => None,
        }
    }

    /// Returns the raw JSON data if this is an unknown mode.
    ///
    /// Returns `None` for known modes.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl Serialize for FunctionCallingMode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Wire format is lowercase per API revision 2026-05-20.
        match self {
            Self::Auto => serializer.serialize_str("auto"),
            Self::Any => serializer.serialize_str("any"),
            Self::None => serializer.serialize_str("none"),
            Self::Validated => serializer.serialize_str("validated"),
            Self::Unknown { mode_type, .. } => serializer.serialize_str(mode_type),
        }
    }
}

impl<'de> Deserialize<'de> for FunctionCallingMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;

        match value.as_str() {
            // Spec wire format is lowercase; accept legacy UPPERCASE too.
            Some("auto") | Some("AUTO") => Ok(Self::Auto),
            Some("any") | Some("ANY") => Ok(Self::Any),
            Some("none") | Some("NONE") => Ok(Self::None),
            Some("validated") | Some("VALIDATED") => Ok(Self::Validated),
            Some(other) => {
                tracing::warn!(
                    "Encountered unknown FunctionCallingMode '{}'. \
                     This may indicate a new API feature. \
                     The mode will be preserved in the Unknown variant.",
                    other
                );
                Ok(Self::Unknown {
                    mode_type: other.to_string(),
                    data: value,
                })
            }
            Option::None => {
                // Non-string value - preserve it in Unknown
                let mode_type = format!("<non-string: {}>", value);
                tracing::warn!(
                    "FunctionCallingMode received non-string value: {}. \
                     Preserving in Unknown variant.",
                    value
                );
                Ok(Self::Unknown {
                    mode_type,
                    data: value,
                })
            }
        }
    }
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

/// Types of search to perform with the Google Search tool.
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
///
/// # Wire Format
///
/// Values serialize as snake_case strings: `"web_search"`, `"image_search"`,
/// `"enterprise_web_search"`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SearchType {
    /// Web search
    WebSearch,
    /// Image search (only available for specific models like `gemini-3.1-flash-image-preview`)
    ImageSearch,
    /// Enterprise web search
    EnterpriseWebSearch,
    /// Unknown search type for forward compatibility
    Unknown {
        /// The unrecognized search type string from the API
        search_type: String,
        /// The raw JSON value, preserved for debugging
        data: serde_json::Value,
    },
}

impl SearchType {
    /// Check if this is an unknown search type.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the search type name if this is an unknown type.
    #[must_use]
    pub fn unknown_search_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { search_type, .. } => Some(search_type),
            _ => None,
        }
    }

    /// Returns the raw JSON data if this is an unknown search type.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl Serialize for SearchType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::WebSearch => serializer.serialize_str("web_search"),
            Self::ImageSearch => serializer.serialize_str("image_search"),
            Self::EnterpriseWebSearch => serializer.serialize_str("enterprise_web_search"),
            Self::Unknown { search_type, .. } => serializer.serialize_str(search_type),
        }
    }
}

impl<'de> Deserialize<'de> for SearchType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value.as_str() {
            Some("web_search") => Ok(Self::WebSearch),
            Some("image_search") => Ok(Self::ImageSearch),
            Some("enterprise_web_search") => Ok(Self::EnterpriseWebSearch),
            Some(other) => {
                tracing::warn!(
                    "Encountered unknown SearchType '{}'. \
                     Preserving in Unknown variant.",
                    other
                );
                Ok(Self::Unknown {
                    search_type: other.to_string(),
                    data: value,
                })
            }
            None => {
                let search_type = format!("<non-string: {}>", value);
                tracing::warn!(
                    "SearchType received non-string value: {}. \
                     Preserving in Unknown variant.",
                    value
                );
                Ok(Self::Unknown {
                    search_type,
                    data: value,
                })
            }
        }
    }
}

/// Retrieval backends for the built-in `retrieval` tool.
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
///
/// # Wire Format
///
/// Serializes as snake_case strings: `"vertex_ai_search"`, `"rag_store"`,
/// `"exa_ai_search"`, `"parallel_ai_search"`.
///
/// # Evergreen Pattern
///
/// Unknown values from the API deserialize into the `Unknown` variant,
/// preserving the original data for debugging and roundtrip serialization.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum RetrievalType {
    /// Vertex AI Search engines and datastores.
    VertexAiSearch,
    /// Vertex RAG Store corpora.
    RagStore,
    /// Exa.ai search.
    ExaAiSearch,
    /// Parallel.ai search.
    ParallelAiSearch,
    /// Unknown retrieval type for forward compatibility
    Unknown {
        /// The unrecognized retrieval type string from the API
        retrieval_type: String,
        /// The raw JSON value, preserved for debugging
        data: serde_json::Value,
    },
}

impl RetrievalType {
    /// Check if this is an unknown retrieval type.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the retrieval type name if this is an unknown type.
    #[must_use]
    pub fn unknown_retrieval_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { retrieval_type, .. } => Some(retrieval_type),
            _ => None,
        }
    }

    /// Returns the raw JSON data if this is an unknown retrieval type.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl Serialize for RetrievalType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::VertexAiSearch => serializer.serialize_str("vertex_ai_search"),
            Self::RagStore => serializer.serialize_str("rag_store"),
            Self::ExaAiSearch => serializer.serialize_str("exa_ai_search"),
            Self::ParallelAiSearch => serializer.serialize_str("parallel_ai_search"),
            Self::Unknown { retrieval_type, .. } => serializer.serialize_str(retrieval_type),
        }
    }
}

impl<'de> Deserialize<'de> for RetrievalType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value.as_str() {
            Some("vertex_ai_search") => Ok(Self::VertexAiSearch),
            Some("rag_store") => Ok(Self::RagStore),
            Some("exa_ai_search") => Ok(Self::ExaAiSearch),
            Some("parallel_ai_search") => Ok(Self::ParallelAiSearch),
            Some(other) => {
                tracing::warn!(
                    "Encountered unknown RetrievalType '{}'. \
                     Preserving in Unknown variant.",
                    other
                );
                Ok(Self::Unknown {
                    retrieval_type: other.to_string(),
                    data: value,
                })
            }
            None => {
                let retrieval_type = format!("<non-string: {}>", value);
                tracing::warn!(
                    "RetrievalType received non-string value: {}. \
                     Preserving in Unknown variant.",
                    value
                );
                Ok(Self::Unknown {
                    retrieval_type,
                    data: value,
                })
            }
        }
    }
}

/// Configuration for the Vertex AI Search retrieval backend.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VertexAiSearchConfig {
    /// The Vertex AI Search engine to use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
    /// The Vertex AI Search datastores to use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub datastores: Option<Vec<String>>,
}

impl VertexAiSearchConfig {
    /// Creates an empty Vertex AI Search config.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Vertex AI Search engine.
    #[must_use]
    pub fn with_engine(mut self, engine: impl Into<String>) -> Self {
        self.engine = Some(engine.into());
        self
    }

    /// Sets the Vertex AI Search datastores.
    #[must_use]
    pub fn with_datastores(mut self, datastores: Vec<String>) -> Self {
        self.datastores = Some(datastores);
        self
    }
}

/// Configuration for the Exa.ai search retrieval backend.
///
/// **Note**: `api_key` is your Exa.ai API key and is sent on the wire; treat
/// request logs accordingly.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExaAiSearchConfig {
    /// The Exa.ai API key (required by the API).
    pub api_key: String,
    /// Extra parameters passed through to the Exa.ai Search API.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_config: Option<serde_json::Value>,
}

impl ExaAiSearchConfig {
    /// Creates a config with the given Exa.ai API key.
    #[must_use]
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            custom_config: None,
        }
    }

    /// Sets extra parameters passed through to the Exa.ai Search API.
    #[must_use]
    pub fn with_custom_config(mut self, custom_config: serde_json::Value) -> Self {
        self.custom_config = Some(custom_config);
        self
    }
}

/// Configuration for the Parallel.ai search retrieval backend.
///
/// **Note**: `api_key` is your Parallel.ai API key and is sent on the wire;
/// treat request logs accordingly.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParallelAiSearchConfig {
    /// The Parallel.ai API key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Extra parameters for Parallel.ai search.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_config: Option<serde_json::Value>,
}

impl ParallelAiSearchConfig {
    /// Creates an empty Parallel.ai search config.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Parallel.ai API key.
    #[must_use]
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Sets extra parameters for Parallel.ai search.
    #[must_use]
    pub fn with_custom_config(mut self, custom_config: serde_json::Value) -> Self {
        self.custom_config = Some(custom_config);
        self
    }
}

/// A RAG resource reference (corpus + optional file restriction).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RagResource {
    /// `RagCorpora` resource name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rag_corpus: Option<String>,
    /// RAG file IDs; the files must belong to `rag_corpus`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rag_file_ids: Option<Vec<String>>,
}

impl RagResource {
    /// Creates a RAG resource for the given corpus.
    #[must_use]
    pub fn new(rag_corpus: impl Into<String>) -> Self {
        Self {
            rag_corpus: Some(rag_corpus.into()),
            rag_file_ids: None,
        }
    }

    /// Restricts retrieval to the given file IDs within the corpus.
    #[must_use]
    pub fn with_rag_file_ids(mut self, rag_file_ids: Vec<String>) -> Self {
        self.rag_file_ids = Some(rag_file_ids);
        self
    }
}

/// Hybrid-search configuration for RAG retrieval.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HybridSearchConfig {
    /// Alpha value controlling the weight between dense and sparse vector
    /// search results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha: Option<f32>,
}

/// Filter configuration for RAG retrieval.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RagFilter {
    /// Only return contexts with vector distance smaller than the threshold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_distance_threshold: Option<f64>,
    /// Only return contexts with vector similarity larger than the threshold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_similarity_threshold: Option<f64>,
    /// String for metadata filtering.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_filter: Option<String>,
}

/// Ranking configuration for RAG retrieval (Rank Service).
///
/// # Wire Format
///
/// `{"ranking_config": "rank_service", "model_name": "..."}` — the
/// `ranking_config` discriminator is always `"rank_service"`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RagRanking {
    /// The ranking config discriminator (always `"rank_service"`).
    #[serde(default = "RagRanking::default_ranking_config")]
    pub ranking_config: String,
    /// The model name of the rank service.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub model_name: Option<String>,
}

impl RagRanking {
    fn default_ranking_config() -> String {
        "rank_service".to_string()
    }

    /// Creates a rank-service ranking config.
    #[must_use]
    pub fn rank_service() -> Self {
        Self::default()
    }

    /// Sets the rank service model name.
    #[must_use]
    pub fn with_model_name(mut self, model_name: impl Into<String>) -> Self {
        self.model_name = Some(model_name.into());
        self
    }
}

impl Default for RagRanking {
    fn default() -> Self {
        Self {
            ranking_config: Self::default_ranking_config(),
            model_name: None,
        }
    }
}

/// Context-retrieval configuration for the RAG store backend
/// (wire: `rag_retrieval_config`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RagRetrievalConfig {
    /// The number of contexts to retrieve.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i32>,
    /// Hybrid search configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hybrid_search: Option<HybridSearchConfig>,
    /// Filter configuration (wire: `filter`).
    #[serde(rename = "filter", skip_serializing_if = "Option::is_none")]
    pub filter: Option<RagFilter>,
    /// Rank Service configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ranking: Option<RagRanking>,
}

impl RagRetrievalConfig {
    /// Creates an empty RAG retrieval config.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the number of contexts to retrieve.
    #[must_use]
    pub fn with_top_k(mut self, top_k: i32) -> Self {
        self.top_k = Some(top_k);
        self
    }

    /// Sets the hybrid-search alpha (dense vs. sparse weighting).
    #[must_use]
    pub fn with_hybrid_search_alpha(mut self, alpha: f32) -> Self {
        self.hybrid_search = Some(HybridSearchConfig { alpha: Some(alpha) });
        self
    }

    /// Sets the filter configuration.
    #[must_use]
    pub fn with_filter(mut self, filter: RagFilter) -> Self {
        self.filter = Some(filter);
        self
    }

    /// Sets the ranking configuration.
    #[must_use]
    pub fn with_ranking(mut self, ranking: RagRanking) -> Self {
        self.ranking = Some(ranking);
        self
    }
}

/// Configuration for the RAG Store retrieval backend.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RagStoreConfig {
    /// The RAG sources to retrieve from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rag_resources: Option<Vec<RagResource>>,
    /// Number of top-k results to return from the selected corpora.
    ///
    /// Deprecated by the API in favor of
    /// `rag_retrieval_config.top_k`; still sent when set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity_top_k: Option<i32>,
    /// Only return results with vector distance smaller than the threshold.
    ///
    /// Deprecated by the API in favor of
    /// `rag_retrieval_config.filter.vector_distance_threshold`; still sent
    /// when set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_distance_threshold: Option<f64>,
    /// Context-retrieval configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rag_retrieval_config: Option<RagRetrievalConfig>,
}

impl RagStoreConfig {
    /// Creates a RAG store config over the given resources.
    #[must_use]
    pub fn new(rag_resources: Vec<RagResource>) -> Self {
        Self {
            rag_resources: Some(rag_resources),
            ..Default::default()
        }
    }

    /// Sets the context-retrieval configuration.
    #[must_use]
    pub fn with_rag_retrieval_config(mut self, config: RagRetrievalConfig) -> Self {
        self.rag_retrieval_config = Some(config);
        self
    }
}

// --- Tool Configuration Structs ---
//
// These provide ergonomic builders for constructing Tool variants with optional fields.
// Each implements `From<Config> for Tool` so they can be passed to `InteractionBuilder::add_tool()`.

/// Configuration for the Google Search built-in tool.
///
/// # Example
///
/// ```no_run
/// use genai_rs::{GoogleSearchConfig, SearchType};
///
/// // Default (web search only)
/// let config = GoogleSearchConfig::new();
///
/// // With image search enabled
/// let config = GoogleSearchConfig::new()
///     .with_search_types(vec![SearchType::WebSearch, SearchType::ImageSearch]);
/// ```
#[derive(Clone, Debug, Default)]
pub struct GoogleSearchConfig {
    search_types: Option<Vec<SearchType>>,
}

impl GoogleSearchConfig {
    /// Creates a new `GoogleSearchConfig` with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the search types to perform.
    #[must_use]
    pub fn with_search_types(mut self, search_types: Vec<SearchType>) -> Self {
        self.search_types = Some(search_types);
        self
    }
}

impl From<GoogleSearchConfig> for Tool {
    fn from(config: GoogleSearchConfig) -> Self {
        Tool::GoogleSearch {
            search_types: config.search_types,
        }
    }
}

/// Configuration for the Google Maps built-in tool.
///
/// # Example
///
/// ```no_run
/// use genai_rs::GoogleMapsConfig;
///
/// // Default
/// let config = GoogleMapsConfig::new();
///
/// // With widget enabled
/// let config = GoogleMapsConfig::new().with_widget();
/// ```
#[derive(Clone, Debug, Default)]
pub struct GoogleMapsConfig {
    enable_widget: Option<bool>,
    latitude: Option<f64>,
    longitude: Option<f64>,
}

impl GoogleMapsConfig {
    /// Creates a new `GoogleMapsConfig` with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enables the widget context token in the response.
    #[must_use]
    pub fn with_widget(mut self) -> Self {
        self.enable_widget = Some(true);
        self
    }

    /// Biases results toward the given coordinates.
    #[must_use]
    pub fn with_location(mut self, latitude: f64, longitude: f64) -> Self {
        self.latitude = Some(latitude);
        self.longitude = Some(longitude);
        self
    }
}

impl From<GoogleMapsConfig> for Tool {
    fn from(config: GoogleMapsConfig) -> Self {
        Tool::GoogleMaps {
            enable_widget: config.enable_widget,
            latitude: config.latitude,
            longitude: config.longitude,
        }
    }
}

/// Configuration for an MCP (Model Context Protocol) server tool.
///
/// # Example
///
/// ```no_run
/// use genai_rs::McpServerConfig;
/// use std::collections::HashMap;
///
/// let config = McpServerConfig::new("filesystem", "https://mcp.example.com/fs")
///     .with_allowed_tools(vec!["read_file".to_string(), "list_dir".to_string()])
///     .with_headers(HashMap::from([
///         ("Authorization".to_string(), "Bearer token".to_string()),
///     ]));
/// ```
#[derive(Clone, Debug)]
pub struct McpServerConfig {
    name: String,
    url: String,
    allowed_tools: Option<Vec<AllowedTools>>,
    headers: Option<HashMap<String, String>>,
}

impl McpServerConfig {
    /// Creates a new `McpServerConfig` with the given name and URL.
    #[must_use]
    pub fn new(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            url: url.into(),
            allowed_tools: None,
            headers: None,
        }
    }

    /// Restricts the model to the given tool names (no explicit mode).
    ///
    /// For per-mode restrictions use
    /// [`with_allowed_tools_config`](Self::with_allowed_tools_config).
    #[must_use]
    pub fn with_allowed_tools(mut self, allowed_tools: Vec<String>) -> Self {
        self.allowed_tools = Some(vec![AllowedTools::new(allowed_tools)]);
        self
    }

    /// Sets the full `[{mode, tools}]` allowed-tools restriction list.
    #[must_use]
    pub fn with_allowed_tools_config(mut self, allowed_tools: Vec<AllowedTools>) -> Self {
        self.allowed_tools = Some(allowed_tools);
        self
    }

    /// Sets authentication/configuration headers.
    #[must_use]
    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers = Some(headers);
        self
    }
}

impl From<McpServerConfig> for Tool {
    fn from(config: McpServerConfig) -> Self {
        Tool::McpServer {
            name: config.name,
            url: config.url,
            allowed_tools: config.allowed_tools,
            headers: config.headers,
        }
    }
}

/// Configuration for the Computer Use built-in tool.
///
/// # Security Warning
///
/// Computer use allows the model to control a real browser. Use
/// [`ComputerUseConfig::excluding`] to restrict dangerous actions.
///
/// # Example
///
/// ```no_run
/// use genai_rs::ComputerUseConfig;
///
/// let config = ComputerUseConfig::new()
///     .excluding(vec!["submit_form".to_string(), "download_file".to_string()]);
/// ```
#[derive(Clone, Debug)]
pub struct ComputerUseConfig {
    environment: String,
    excluded_predefined_functions: Vec<String>,
    enable_prompt_injection_detection: Option<bool>,
    disabled_safety_policies: Vec<String>,
}

impl ComputerUseConfig {
    /// Creates a new `ComputerUseConfig` targeting the browser environment.
    #[must_use]
    pub fn new() -> Self {
        Self {
            environment: "browser".to_string(),
            excluded_predefined_functions: Vec::new(),
            enable_prompt_injection_detection: None,
            disabled_safety_policies: Vec::new(),
        }
    }

    /// Sets the operating environment. Known values: `browser`, `mobile`,
    /// `desktop`.
    #[must_use]
    pub fn with_environment(mut self, environment: impl Into<String>) -> Self {
        self.environment = environment.into();
        self
    }

    /// Excludes specific predefined browser functions from model access.
    #[must_use]
    pub fn excluding(mut self, functions: Vec<String>) -> Self {
        self.excluded_predefined_functions = functions;
        self
    }

    /// Enables (or disables) prompt injection detection.
    #[must_use]
    pub fn with_prompt_injection_detection(mut self, enabled: bool) -> Self {
        self.enable_prompt_injection_detection = Some(enabled);
        self
    }

    /// Disables the given safety policies.
    ///
    /// Known values include `financial_transactions`,
    /// `sensitive_data_modification`, `communication_tool`,
    /// `account_creation`, `data_modification`, `user_consent_management`,
    /// and `legal_terms_and_agreements`.
    #[must_use]
    pub fn disabling_safety_policies(mut self, policies: Vec<String>) -> Self {
        self.disabled_safety_policies = policies;
        self
    }
}

impl Default for ComputerUseConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl From<ComputerUseConfig> for Tool {
    fn from(config: ComputerUseConfig) -> Self {
        Tool::ComputerUse {
            environment: config.environment,
            excluded_predefined_functions: config.excluded_predefined_functions,
            enable_prompt_injection_detection: config.enable_prompt_injection_detection,
            disabled_safety_policies: config.disabled_safety_policies,
        }
    }
}

/// Configuration for the File Search built-in tool.
///
/// # Example
///
/// ```no_run
/// use genai_rs::FileSearchConfig;
///
/// let config = FileSearchConfig::new(vec!["my-store".to_string()])
///     .with_top_k(10)
///     .with_metadata_filter("category:technical");
/// ```
#[derive(Clone, Debug)]
pub struct FileSearchConfig {
    store_names: Vec<String>,
    top_k: Option<i32>,
    metadata_filter: Option<String>,
}

impl FileSearchConfig {
    /// Creates a new `FileSearchConfig` with the given store names.
    #[must_use]
    pub fn new(store_names: Vec<String>) -> Self {
        Self {
            store_names,
            top_k: None,
            metadata_filter: None,
        }
    }

    /// Sets the maximum number of semantic retrieval chunks to return.
    #[must_use]
    pub fn with_top_k(mut self, top_k: i32) -> Self {
        self.top_k = Some(top_k);
        self
    }

    /// Sets a metadata filter expression for document filtering.
    #[must_use]
    pub fn with_metadata_filter(mut self, filter: impl Into<String>) -> Self {
        self.metadata_filter = Some(filter.into());
        self
    }
}

impl From<FileSearchConfig> for Tool {
    fn from(config: FileSearchConfig) -> Self {
        Tool::FileSearch {
            store_names: config.store_names,
            top_k: config.top_k,
            metadata_filter: config.metadata_filter,
        }
    }
}

/// Configuration for the built-in Retrieval tool.
///
/// Each `with_*` backend method enables the corresponding
/// [`RetrievalType`] and attaches its config, keeping `retrieval_types`
/// consistent with the per-backend configuration.
///
/// # Example
///
/// ```no_run
/// use genai_rs::{RagResource, RagStoreConfig, RetrievalConfig, VertexAiSearchConfig};
///
/// // Vertex AI Search grounding
/// let config = RetrievalConfig::new().with_vertex_ai_search(
///     VertexAiSearchConfig::new().with_engine("projects/p/locations/global/engines/e"),
/// );
///
/// // RAG store grounding
/// let config = RetrievalConfig::new().with_rag_store(RagStoreConfig::new(vec![
///     RagResource::new("projects/p/locations/us/ragCorpora/c"),
/// ]));
/// ```
#[derive(Clone, Debug, Default)]
pub struct RetrievalConfig {
    retrieval_types: Vec<RetrievalType>,
    vertex_ai_search_config: Option<VertexAiSearchConfig>,
    exa_ai_search_config: Option<ExaAiSearchConfig>,
    parallel_ai_search_config: Option<ParallelAiSearchConfig>,
    rag_store_config: Option<Box<RagStoreConfig>>,
}

impl RetrievalConfig {
    /// Creates an empty retrieval config.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn enable(&mut self, retrieval_type: RetrievalType) {
        if !self.retrieval_types.contains(&retrieval_type) {
            self.retrieval_types.push(retrieval_type);
        }
    }

    /// Enables Vertex AI Search retrieval with the given config.
    #[must_use]
    pub fn with_vertex_ai_search(mut self, config: VertexAiSearchConfig) -> Self {
        self.enable(RetrievalType::VertexAiSearch);
        self.vertex_ai_search_config = Some(config);
        self
    }

    /// Enables Exa.ai search retrieval with the given config.
    #[must_use]
    pub fn with_exa_ai_search(mut self, config: ExaAiSearchConfig) -> Self {
        self.enable(RetrievalType::ExaAiSearch);
        self.exa_ai_search_config = Some(config);
        self
    }

    /// Enables Parallel.ai search retrieval with the given config.
    #[must_use]
    pub fn with_parallel_ai_search(mut self, config: ParallelAiSearchConfig) -> Self {
        self.enable(RetrievalType::ParallelAiSearch);
        self.parallel_ai_search_config = Some(config);
        self
    }

    /// Enables RAG Store retrieval with the given config.
    #[must_use]
    pub fn with_rag_store(mut self, config: RagStoreConfig) -> Self {
        self.enable(RetrievalType::RagStore);
        self.rag_store_config = Some(Box::new(config));
        self
    }

    /// Sets the enabled retrieval types explicitly (escape hatch).
    ///
    /// Replaces the types accumulated by the `with_*` backend methods.
    #[must_use]
    pub fn with_retrieval_types(mut self, retrieval_types: Vec<RetrievalType>) -> Self {
        self.retrieval_types = retrieval_types;
        self
    }
}

impl From<RetrievalConfig> for Tool {
    fn from(config: RetrievalConfig) -> Self {
        Tool::Retrieval {
            retrieval_types: if config.retrieval_types.is_empty() {
                None
            } else {
                Some(config.retrieval_types)
            },
            vertex_ai_search_config: config.vertex_ai_search_config,
            exa_ai_search_config: config.exa_ai_search_config,
            parallel_ai_search_config: config.parallel_ai_search_config,
            rag_store_config: config.rag_store_config,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn test_serialize_function_declaration() {
        let function = FunctionDeclaration::builder("get_weather")
            .description("Get the current weather in a given location")
            .parameter(
                "location",
                serde_json::json!({
                    "type": "string",
                    "description": "The city and state, e.g. San Francisco, CA"
                }),
            )
            .required(vec!["location".to_string()])
            .build();

        let json_string = serde_json::to_string(&function).expect("Serialization failed");
        let parsed: FunctionDeclaration =
            serde_json::from_str(&json_string).expect("Deserialization failed");

        assert_eq!(parsed.name(), "get_weather");
        assert_eq!(
            parsed.description(),
            "Get the current weather in a given location"
        );
    }

    #[test]
    fn test_function_calling_mode_serialization() {
        // Wire format is lowercase per API revision 2026-05-20
        let test_cases = [
            (FunctionCallingMode::Auto, "\"auto\""),
            (FunctionCallingMode::Any, "\"any\""),
            (FunctionCallingMode::None, "\"none\""),
            (FunctionCallingMode::Validated, "\"validated\""),
        ];

        for (mode, expected_json) in test_cases {
            let json = serde_json::to_string(&mode).expect("Serialization failed");
            assert_eq!(json, expected_json);

            let parsed: FunctionCallingMode =
                serde_json::from_str(&json).expect("Deserialization failed");
            assert_eq!(parsed, mode);
        }

        // Legacy UPPERCASE values are still accepted on deserialize
        for (raw, expected) in [
            ("\"AUTO\"", FunctionCallingMode::Auto),
            ("\"VALIDATED\"", FunctionCallingMode::Validated),
        ] {
            let parsed: FunctionCallingMode =
                serde_json::from_str(raw).expect("Deserialization failed");
            assert_eq!(parsed, expected);
        }
    }

    #[test]
    fn test_function_calling_mode_unknown_roundtrip() {
        // Test that unknown modes are preserved
        let json = "\"FUTURE_MODE\"";
        let parsed: FunctionCallingMode =
            serde_json::from_str(json).expect("Deserialization failed");

        assert!(parsed.is_unknown());
        assert_eq!(parsed.unknown_mode_type(), Some("FUTURE_MODE"));

        // Roundtrip should preserve the mode type
        let reserialized = serde_json::to_string(&parsed).expect("Serialization failed");
        assert_eq!(reserialized, json);
    }

    #[test]
    fn test_function_calling_mode_helper_methods() {
        // Known modes should not be unknown
        assert!(!FunctionCallingMode::Auto.is_unknown());
        assert!(!FunctionCallingMode::Any.is_unknown());
        assert!(!FunctionCallingMode::None.is_unknown());
        assert!(!FunctionCallingMode::Validated.is_unknown());

        assert!(FunctionCallingMode::Auto.unknown_mode_type().is_none());
        assert!(FunctionCallingMode::Auto.unknown_data().is_none());

        // Unknown mode should report its type
        let unknown = FunctionCallingMode::Unknown {
            mode_type: "NEW_MODE".to_string(),
            data: serde_json::json!("NEW_MODE"),
        };
        assert!(unknown.is_unknown());
        assert_eq!(unknown.unknown_mode_type(), Some("NEW_MODE"));
        assert!(unknown.unknown_data().is_some());
    }

    #[test]
    fn test_function_calling_mode_non_string_value() {
        // Test that non-string JSON values are handled gracefully
        let json = "123";
        let parsed: FunctionCallingMode =
            serde_json::from_str(json).expect("Deserialization should succeed");

        assert!(parsed.is_unknown());
        // The mode_type should indicate it was a non-string value
        assert!(parsed.unknown_mode_type().unwrap().contains("<non-string:"));
    }

    #[test]
    fn test_tool_google_search_roundtrip() {
        let tool = Tool::GoogleSearch { search_types: None };
        let json = serde_json::to_string(&tool).expect("Serialization failed");
        assert!(json.contains("\"type\":\"google_search\""));
        assert!(!json.contains("search_types"));

        let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");
        assert!(matches!(parsed, Tool::GoogleSearch { .. }));
    }

    #[test]
    fn test_tool_google_search_with_search_types_roundtrip() {
        let tool = Tool::GoogleSearch {
            search_types: Some(vec![SearchType::WebSearch, SearchType::ImageSearch]),
        };
        let json = serde_json::to_string(&tool).expect("Serialization failed");
        assert!(json.contains("\"search_types\""));
        assert!(json.contains("\"web_search\""));
        assert!(json.contains("\"image_search\""));

        let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");
        match parsed {
            Tool::GoogleSearch { search_types } => {
                let types = search_types.expect("Should have search_types");
                assert_eq!(types.len(), 2);
                assert_eq!(types[0], SearchType::WebSearch);
                assert_eq!(types[1], SearchType::ImageSearch);
            }
            other => panic!("Expected GoogleSearch variant, got {:?}", other),
        }
    }

    #[test]
    fn test_tool_google_maps_roundtrip() {
        let tool = Tool::GoogleMaps {
            enable_widget: None,
            latitude: None,
            longitude: None,
        };
        let json = serde_json::to_string(&tool).expect("Serialization failed");
        assert!(json.contains("\"type\":\"google_maps\""));
        assert!(!json.contains("enable_widget"));

        let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");
        match parsed {
            Tool::GoogleMaps { enable_widget, .. } => assert_eq!(enable_widget, None),
            other => panic!("Expected GoogleMaps variant, got {:?}", other),
        }
    }

    #[test]
    fn test_tool_google_maps_with_widget_roundtrip() {
        let tool = Tool::GoogleMaps {
            enable_widget: Some(true),
            latitude: Some(40.758),
            longitude: Some(-73.9855),
        };
        let json = serde_json::to_string(&tool).expect("Serialization failed");
        assert!(json.contains("\"enable_widget\":true"));
        assert!(json.contains("\"latitude\":40.758"));
        assert!(json.contains("\"longitude\":-73.9855"));

        let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");
        match parsed {
            Tool::GoogleMaps {
                enable_widget,
                latitude,
                longitude,
            } => {
                assert_eq!(enable_widget, Some(true));
                assert_eq!(latitude, Some(40.758));
                assert_eq!(longitude, Some(-73.9855));
            }
            other => panic!("Expected GoogleMaps variant, got {:?}", other),
        }
    }

    #[test]
    fn test_tool_function_roundtrip() {
        let tool = Tool::Function {
            name: "get_weather".to_string(),
            description: "Get weather".to_string(),
            parameters: FunctionParameters::new(
                "object".to_string(),
                serde_json::json!({}),
                vec![],
            ),
        };
        let json = serde_json::to_string(&tool).expect("Serialization failed");
        let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");

        match parsed {
            Tool::Function { name, .. } => assert_eq!(name, "get_weather"),
            other => panic!("Expected Function variant, got {:?}", other),
        }
    }

    #[test]
    fn test_tool_mcp_server_roundtrip() {
        let tool = Tool::McpServer {
            name: "my-server".to_string(),
            url: "https://mcp.example.com/api".to_string(),
            allowed_tools: None,
            headers: None,
        };
        let json = serde_json::to_string(&tool).expect("Serialization failed");
        assert!(json.contains("\"type\":\"mcp_server\""));
        assert!(json.contains("\"name\":\"my-server\""));
        assert!(json.contains("\"url\":\"https://mcp.example.com/api\""));
        assert!(!json.contains("allowed_tools"));
        assert!(!json.contains("headers"));

        let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");
        match parsed {
            Tool::McpServer {
                name,
                url,
                allowed_tools,
                headers,
            } => {
                assert_eq!(name, "my-server");
                assert_eq!(url, "https://mcp.example.com/api");
                assert_eq!(allowed_tools, None);
                assert_eq!(headers, None);
            }
            other => panic!("Expected McpServer variant, got {:?}", other),
        }
    }

    #[test]
    fn test_tool_mcp_server_with_optional_fields_roundtrip() {
        let tool = Tool::McpServer {
            name: "my-server".to_string(),
            url: "https://mcp.example.com/api".to_string(),
            allowed_tools: Some(vec![
                AllowedTools::new(vec!["read_file".to_string(), "list_dir".to_string()])
                    .with_mode(FunctionCallingMode::Auto),
            ]),
            headers: Some(HashMap::from([(
                "Authorization".to_string(),
                "Bearer token".to_string(),
            )])),
        };
        let json = serde_json::to_string(&tool).expect("Serialization failed");
        assert!(json.contains("\"allowed_tools\""));
        assert!(json.contains("\"headers\""));

        let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");
        match parsed {
            Tool::McpServer {
                allowed_tools,
                headers,
                ..
            } => {
                let tools = allowed_tools.expect("Should have allowed_tools");
                assert_eq!(tools.len(), 1);
                assert_eq!(tools[0].tools.len(), 2);
                assert_eq!(tools[0].mode, Some(FunctionCallingMode::Auto));
                let hdrs = headers.expect("Should have headers");
                assert_eq!(hdrs.get("Authorization").unwrap(), "Bearer token");
            }
            other => panic!("Expected McpServer variant, got {:?}", other),
        }
    }

    #[test]
    fn test_tool_unknown_deserialization() {
        // Simulate an unknown tool type from the API
        let json = r#"{"type": "future_tool", "some_field": "value", "number": 42}"#;
        let parsed: Tool = serde_json::from_str(json).expect("Deserialization failed");

        match parsed {
            Tool::Unknown { tool_type, data } => {
                assert_eq!(tool_type, "future_tool");
                assert_eq!(data.get("some_field").unwrap(), "value");
                assert_eq!(data.get("number").unwrap(), 42);
            }
            _ => panic!("Expected Unknown variant"),
        }
    }

    #[test]
    fn test_tool_unknown_roundtrip() {
        let tool = Tool::Unknown {
            tool_type: "new_tool".to_string(),
            data: serde_json::json!({"type": "new_tool", "config": {"enabled": true}}),
        };
        let json = serde_json::to_string(&tool).expect("Serialization failed");

        // Should contain the type and config, but not duplicate "type"
        assert!(json.contains("\"type\":\"new_tool\""));
        assert!(json.contains("\"config\""));

        let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");
        match parsed {
            Tool::Unknown { tool_type, .. } => assert_eq!(tool_type, "new_tool"),
            _ => panic!("Expected Unknown variant"),
        }
    }

    #[test]
    fn test_tool_unknown_helper_methods() {
        // Test Unknown variant
        let unknown_tool = Tool::Unknown {
            tool_type: "future_tool".to_string(),
            data: serde_json::json!({"type": "future_tool", "setting": 123}),
        };

        assert!(unknown_tool.is_unknown());
        assert_eq!(unknown_tool.unknown_tool_type(), Some("future_tool"));
        let data = unknown_tool.unknown_data().expect("Should have data");
        assert_eq!(data.get("setting").unwrap(), 123);
    }

    #[test]
    fn test_tool_computer_use_roundtrip() {
        let tool = Tool::ComputerUse {
            environment: "browser".to_string(),
            excluded_predefined_functions: vec!["submit_form".to_string(), "download".to_string()],
            enable_prompt_injection_detection: Some(true),
            disabled_safety_policies: vec!["data_modification".to_string()],
        };
        let json = serde_json::to_string(&tool).expect("Serialization failed");
        assert!(json.contains("\"type\":\"computer_use\""));
        assert!(json.contains("\"environment\":\"browser\""));
        // Spec wire format is snake_case (fixed from legacy camelCase)
        assert!(json.contains("\"excluded_predefined_functions\""));
        assert!(!json.contains("excludedPredefinedFunctions"));
        assert!(json.contains("\"enable_prompt_injection_detection\":true"));
        assert!(json.contains("\"disabled_safety_policies\""));

        let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");
        match parsed {
            Tool::ComputerUse {
                environment,
                excluded_predefined_functions,
                enable_prompt_injection_detection,
                disabled_safety_policies,
            } => {
                assert_eq!(environment, "browser");
                assert_eq!(excluded_predefined_functions.len(), 2);
                assert!(excluded_predefined_functions.contains(&"submit_form".to_string()));
                assert_eq!(enable_prompt_injection_detection, Some(true));
                assert_eq!(
                    disabled_safety_policies,
                    vec!["data_modification".to_string()]
                );
            }
            other => panic!("Expected ComputerUse variant, got {:?}", other),
        }
    }

    #[test]
    fn test_tool_computer_use_legacy_camel_case_accepted() {
        // Pre-revision payloads used camelCase; the alias keeps them parseable.
        let json = r#"{"type":"computer_use","environment":"browser","excludedPredefinedFunctions":["a"]}"#;
        let parsed: Tool = serde_json::from_str(json).expect("Deserialization failed");
        match parsed {
            Tool::ComputerUse {
                excluded_predefined_functions,
                ..
            } => assert_eq!(excluded_predefined_functions, vec!["a".to_string()]),
            other => panic!("Expected ComputerUse variant, got {:?}", other),
        }
    }

    #[test]
    fn test_tool_computer_use_empty_exclusions() {
        // Test that empty exclusions don't serialize the field
        let tool = Tool::ComputerUse {
            environment: "browser".to_string(),
            excluded_predefined_functions: vec![],
            enable_prompt_injection_detection: None,
            disabled_safety_policies: vec![],
        };
        let json = serde_json::to_string(&tool).expect("Serialization failed");
        assert!(json.contains("\"type\":\"computer_use\""));
        assert!(json.contains("\"environment\":\"browser\""));
        assert!(!json.contains("excluded_predefined_functions"));

        let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");
        match parsed {
            Tool::ComputerUse {
                excluded_predefined_functions,
                ..
            } => {
                assert!(excluded_predefined_functions.is_empty());
            }
            other => panic!("Expected ComputerUse variant, got {:?}", other),
        }
    }

    #[test]
    fn test_tool_known_types_helper_methods() {
        // Test known types return None for unknown helpers
        let google_search = Tool::GoogleSearch { search_types: None };
        assert!(!google_search.is_unknown());
        assert_eq!(google_search.unknown_tool_type(), None);
        assert_eq!(google_search.unknown_data(), None);

        let google_maps = Tool::GoogleMaps {
            enable_widget: None,
            latitude: None,
            longitude: None,
        };
        assert!(!google_maps.is_unknown());
        assert_eq!(google_maps.unknown_tool_type(), None);
        assert_eq!(google_maps.unknown_data(), None);

        let code_execution = Tool::CodeExecution;
        assert!(!code_execution.is_unknown());
        assert_eq!(code_execution.unknown_tool_type(), None);
        assert_eq!(code_execution.unknown_data(), None);

        let url_context = Tool::UrlContext;
        assert!(!url_context.is_unknown());
        assert_eq!(url_context.unknown_tool_type(), None);
        assert_eq!(url_context.unknown_data(), None);

        let computer_use = Tool::ComputerUse {
            environment: "browser".to_string(),
            excluded_predefined_functions: vec![],
            enable_prompt_injection_detection: None,
            disabled_safety_policies: vec![],
        };
        assert!(!computer_use.is_unknown());
        assert_eq!(computer_use.unknown_tool_type(), None);
        assert_eq!(computer_use.unknown_data(), None);

        let function = Tool::Function {
            name: "test".to_string(),
            description: "Test function".to_string(),
            parameters: FunctionParameters::new(
                "object".to_string(),
                serde_json::json!({}),
                vec![],
            ),
        };
        assert!(!function.is_unknown());
        assert_eq!(function.unknown_tool_type(), None);
        assert_eq!(function.unknown_data(), None);
    }

    #[test]
    fn test_tool_file_search_roundtrip() {
        let tool = Tool::FileSearch {
            store_names: vec!["store1".to_string(), "store2".to_string()],
            top_k: Some(5),
            metadata_filter: Some("category:technical".to_string()),
        };
        let json = serde_json::to_string(&tool).expect("Serialization failed");
        assert!(json.contains("\"type\":\"file_search\""));
        assert!(json.contains("\"file_search_store_names\"")); // Wire format uses full name
        assert!(json.contains("\"top_k\":5"));
        assert!(json.contains("\"metadata_filter\":\"category:technical\""));

        let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");
        match parsed {
            Tool::FileSearch {
                store_names,
                top_k,
                metadata_filter,
            } => {
                assert_eq!(store_names, vec!["store1", "store2"]);
                assert_eq!(top_k, Some(5));
                assert_eq!(metadata_filter, Some("category:technical".to_string()));
            }
            other => panic!("Expected FileSearch variant, got {:?}", other),
        }
    }

    #[test]
    fn test_tool_file_search_minimal() {
        // Test with only required field (store names)
        let tool = Tool::FileSearch {
            store_names: vec!["my-store".to_string()],
            top_k: None,
            metadata_filter: None,
        };
        let json = serde_json::to_string(&tool).expect("Serialization failed");
        assert!(json.contains("\"type\":\"file_search\""));
        assert!(json.contains("\"file_search_store_names\"")); // Wire format uses full name
        // Optional fields should not appear
        assert!(!json.contains("\"top_k\""));
        assert!(!json.contains("\"metadata_filter\""));

        let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");
        match parsed {
            Tool::FileSearch {
                store_names,
                top_k,
                metadata_filter,
            } => {
                assert_eq!(store_names, vec!["my-store"]);
                assert_eq!(top_k, None);
                assert_eq!(metadata_filter, None);
            }
            other => panic!("Expected FileSearch variant, got {:?}", other),
        }
    }

    #[test]
    fn test_tool_file_search_helper_methods() {
        let file_search = Tool::FileSearch {
            store_names: vec!["store".to_string()],
            top_k: None,
            metadata_filter: None,
        };
        assert!(!file_search.is_unknown());
        assert_eq!(file_search.unknown_tool_type(), None);
        assert_eq!(file_search.unknown_data(), None);
    }

    #[test]
    fn test_search_type_roundtrip() {
        let types = vec![SearchType::WebSearch, SearchType::ImageSearch];
        let json = serde_json::to_string(&types).expect("Serialization failed");
        assert_eq!(json, r#"["web_search","image_search"]"#);

        let parsed: Vec<SearchType> = serde_json::from_str(&json).expect("Deserialization failed");
        assert_eq!(parsed, types);
    }

    #[test]
    fn test_search_type_unknown_roundtrip() {
        let json = r#""future_search""#;
        let parsed: SearchType = serde_json::from_str(json).expect("Deserialization failed");
        assert!(parsed.is_unknown());
        assert_eq!(parsed.unknown_search_type(), Some("future_search"));
        assert_eq!(
            parsed.unknown_data(),
            Some(&serde_json::Value::String("future_search".to_string()))
        );

        let reserialized = serde_json::to_string(&parsed).expect("Serialization failed");
        assert_eq!(reserialized, json);
    }

    #[test]
    fn test_google_search_config_into_tool() {
        let tool: Tool = GoogleSearchConfig::new().into();
        assert!(matches!(tool, Tool::GoogleSearch { search_types: None }));

        let tool: Tool = GoogleSearchConfig::new()
            .with_search_types(vec![SearchType::ImageSearch])
            .into();
        match tool {
            Tool::GoogleSearch { search_types } => {
                let types = search_types.expect("Should have search_types");
                assert_eq!(types, vec![SearchType::ImageSearch]);
            }
            other => panic!("Expected GoogleSearch, got {:?}", other),
        }
    }

    #[test]
    fn test_google_maps_config_into_tool() {
        let tool: Tool = GoogleMapsConfig::new().into();
        assert!(matches!(
            tool,
            Tool::GoogleMaps {
                enable_widget: None,
                ..
            }
        ));

        let tool: Tool = GoogleMapsConfig::new().with_widget().into();
        assert!(matches!(
            tool,
            Tool::GoogleMaps {
                enable_widget: Some(true),
                ..
            }
        ));
    }

    #[test]
    fn test_mcp_server_config_into_tool() {
        let tool: Tool = McpServerConfig::new("server", "https://example.com").into();
        match tool {
            Tool::McpServer {
                name,
                url,
                allowed_tools,
                headers,
            } => {
                assert_eq!(name, "server");
                assert_eq!(url, "https://example.com");
                assert_eq!(allowed_tools, None);
                assert_eq!(headers, None);
            }
            other => panic!("Expected McpServer, got {:?}", other),
        }
    }

    #[test]
    fn test_computer_use_config_into_tool() {
        let tool: Tool = ComputerUseConfig::new().into();
        match tool {
            Tool::ComputerUse {
                environment,
                excluded_predefined_functions,
                ..
            } => {
                assert_eq!(environment, "browser");
                assert!(excluded_predefined_functions.is_empty());
            }
            other => panic!("Expected ComputerUse, got {:?}", other),
        }

        let tool: Tool = ComputerUseConfig::new()
            .excluding(vec!["download".to_string()])
            .into();
        match tool {
            Tool::ComputerUse {
                excluded_predefined_functions,
                ..
            } => {
                assert_eq!(excluded_predefined_functions, vec!["download"]);
            }
            other => panic!("Expected ComputerUse, got {:?}", other),
        }
    }

    #[test]
    fn test_file_search_config_into_tool() {
        let tool: Tool = FileSearchConfig::new(vec!["store".to_string()])
            .with_top_k(5)
            .with_metadata_filter("cat:tech")
            .into();
        match tool {
            Tool::FileSearch {
                store_names,
                top_k,
                metadata_filter,
            } => {
                assert_eq!(store_names, vec!["store"]);
                assert_eq!(top_k, Some(5));
                assert_eq!(metadata_filter, Some("cat:tech".to_string()));
            }
            other => panic!("Expected FileSearch, got {:?}", other),
        }
    }

    #[test]
    fn test_retrieval_type_wire_roundtrip() {
        for (retrieval_type, wire) in [
            (RetrievalType::VertexAiSearch, "\"vertex_ai_search\""),
            (RetrievalType::RagStore, "\"rag_store\""),
            (RetrievalType::ExaAiSearch, "\"exa_ai_search\""),
            (RetrievalType::ParallelAiSearch, "\"parallel_ai_search\""),
        ] {
            assert_eq!(serde_json::to_string(&retrieval_type).unwrap(), wire);
            let parsed: RetrievalType = serde_json::from_str(wire).unwrap();
            assert_eq!(parsed, retrieval_type);
        }
    }

    #[test]
    fn test_retrieval_type_unknown_roundtrip() {
        let unknown: RetrievalType = serde_json::from_str("\"bing_search\"").unwrap();
        assert!(unknown.is_unknown());
        assert_eq!(unknown.unknown_retrieval_type(), Some("bing_search"));
        assert!(unknown.unknown_data().is_some());
        assert_eq!(serde_json::to_string(&unknown).unwrap(), "\"bing_search\"");

        // Known types are not unknown
        assert!(!RetrievalType::RagStore.is_unknown());
        assert_eq!(RetrievalType::RagStore.unknown_retrieval_type(), None);
        assert_eq!(RetrievalType::RagStore.unknown_data(), None);
    }

    #[test]
    fn test_tool_retrieval_vertex_ai_search_wire_shape() {
        let tool: Tool = RetrievalConfig::new()
            .with_vertex_ai_search(
                VertexAiSearchConfig::new()
                    .with_engine("projects/p/locations/global/engines/e")
                    .with_datastores(vec!["ds-1".to_string()]),
            )
            .into();

        let value = serde_json::to_value(&tool).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "type": "retrieval",
                "retrieval_types": ["vertex_ai_search"],
                "vertex_ai_search_config": {
                    "engine": "projects/p/locations/global/engines/e",
                    "datastores": ["ds-1"]
                }
            })
        );
    }

    #[test]
    fn test_tool_retrieval_rag_store_wire_shape() {
        let tool: Tool = RetrievalConfig::new()
            .with_rag_store(
                RagStoreConfig::new(vec![
                    RagResource::new("projects/p/locations/us/ragCorpora/c")
                        .with_rag_file_ids(vec!["f1".to_string()]),
                ])
                .with_rag_retrieval_config(
                    RagRetrievalConfig::new()
                        .with_top_k(8)
                        .with_hybrid_search_alpha(0.5)
                        .with_filter(RagFilter {
                            vector_distance_threshold: Some(0.7),
                            vector_similarity_threshold: None,
                            metadata_filter: Some("category = \"tech\"".to_string()),
                        })
                        .with_ranking(RagRanking::rank_service().with_model_name("ranker-v2")),
                ),
            )
            .into();

        let value = serde_json::to_value(&tool).unwrap();
        assert_eq!(value["type"], "retrieval");
        assert_eq!(value["retrieval_types"], serde_json::json!(["rag_store"]));
        let rag = &value["rag_store_config"];
        assert_eq!(
            rag["rag_resources"][0]["rag_corpus"],
            "projects/p/locations/us/ragCorpora/c"
        );
        assert_eq!(rag["rag_resources"][0]["rag_file_ids"][0], "f1");
        let retrieval = &rag["rag_retrieval_config"];
        assert_eq!(retrieval["top_k"], 8);
        assert_eq!(retrieval["hybrid_search"]["alpha"], 0.5);
        // Wire field is `filter` (Rust field `filter`, spec alias `filter_`)
        assert_eq!(retrieval["filter"]["vector_distance_threshold"], 0.7);
        assert_eq!(
            retrieval["filter"]["metadata_filter"],
            "category = \"tech\""
        );
        assert_eq!(retrieval["ranking"]["ranking_config"], "rank_service");
        assert_eq!(retrieval["ranking"]["model_name"], "ranker-v2");
    }

    #[test]
    fn test_tool_retrieval_exa_and_parallel_wire_shape() {
        let tool: Tool = RetrievalConfig::new()
            .with_exa_ai_search(
                ExaAiSearchConfig::new("exa-key")
                    .with_custom_config(serde_json::json!({"num_results": 5})),
            )
            .with_parallel_ai_search(ParallelAiSearchConfig::new().with_api_key("par-key"))
            .into();

        let value = serde_json::to_value(&tool).unwrap();
        assert_eq!(
            value["retrieval_types"],
            serde_json::json!(["exa_ai_search", "parallel_ai_search"])
        );
        assert_eq!(value["exa_ai_search_config"]["api_key"], "exa-key");
        assert_eq!(
            value["exa_ai_search_config"]["custom_config"]["num_results"],
            5
        );
        assert_eq!(value["parallel_ai_search_config"]["api_key"], "par-key");
    }

    #[test]
    fn test_tool_retrieval_roundtrip() {
        let tool: Tool = RetrievalConfig::new()
            .with_rag_store(RagStoreConfig::new(vec![RagResource::new("corpora/c")]))
            .with_vertex_ai_search(VertexAiSearchConfig::new().with_engine("engines/e"))
            .into();

        let json = serde_json::to_string(&tool).expect("Serialization failed");
        let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");
        match parsed {
            Tool::Retrieval {
                retrieval_types,
                vertex_ai_search_config,
                rag_store_config,
                exa_ai_search_config,
                parallel_ai_search_config,
            } => {
                assert_eq!(
                    retrieval_types,
                    Some(vec![RetrievalType::RagStore, RetrievalType::VertexAiSearch])
                );
                assert_eq!(
                    vertex_ai_search_config.unwrap().engine.as_deref(),
                    Some("engines/e")
                );
                assert!(rag_store_config.is_some());
                assert_eq!(exa_ai_search_config, None);
                assert_eq!(parallel_ai_search_config, None);
            }
            other => panic!("Expected Retrieval variant, got {:?}", other),
        }
    }

    #[test]
    fn test_tool_retrieval_minimal_serializes_type_only() {
        let tool = Tool::Retrieval {
            retrieval_types: None,
            vertex_ai_search_config: None,
            exa_ai_search_config: None,
            parallel_ai_search_config: None,
            rag_store_config: None,
        };
        let json = serde_json::to_string(&tool).expect("Serialization failed");
        assert_eq!(json, r#"{"type":"retrieval"}"#);

        let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");
        assert!(matches!(parsed, Tool::Retrieval { .. }));
        assert!(!parsed.is_unknown());
    }

    #[test]
    fn test_retrieval_config_unknown_types_escape_hatch() {
        let tool: Tool = RetrievalConfig::new()
            .with_retrieval_types(vec![RetrievalType::Unknown {
                retrieval_type: "future_backend".to_string(),
                data: serde_json::json!("future_backend"),
            }])
            .into();
        let value = serde_json::to_value(&tool).unwrap();
        assert_eq!(value["retrieval_types"][0], "future_backend");
    }
}
