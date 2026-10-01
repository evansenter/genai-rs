// Shared types used by the Interactions API

mod builtin;
mod choice;
mod function;
mod retrieval;

pub use builtin::{
    ComputerUseConfig, FileSearchConfig, GoogleMapsConfig, GoogleSearchConfig, McpServerConfig,
    SearchType,
};
pub use choice::{AllowedTools, FunctionCallingMode, ToolChoice};
pub use function::{FunctionDeclaration, FunctionDeclarationBuilder, FunctionParameters};
pub use retrieval::{
    ExaAiSearchConfig, HybridSearchConfig, ParallelAiSearchConfig, RagFilter, RagRanking,
    RagResource, RagRetrievalConfig, RagStoreConfig, RankService, RetrievalConfig, RetrievalType,
    VertexAiSearchConfig,
};

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
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
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
        #[serde(skip_serializing_if = "crate::serde_util::is_none_or_empty")]
        search_types: Option<Vec<SearchType>>,
    },
    /// Built-in Google Maps tool for location-grounded responses
    GoogleMaps {
        /// Whether to enable the widget context token in the response
        #[serde(skip_serializing_if = "Option::is_none")]
        enable_widget: Option<bool>,
        /// Latitude bias for location grounding
        #[serde(skip_serializing_if = "Option::is_none")]
        latitude: Option<f64>,
        /// Longitude bias for location grounding
        #[serde(skip_serializing_if = "Option::is_none")]
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
        #[serde(skip_serializing_if = "Vec::is_empty")]
        excluded_predefined_functions: Vec<String>,
        /// Whether to enable prompt injection detection for this request.
        #[serde(skip_serializing_if = "Option::is_none")]
        enable_prompt_injection_detection: Option<bool>,
        /// Safety policies to disable. Known values include
        /// `financial_transactions`, `sensitive_data_modification`,
        /// `communication_tool`, `account_creation`, `data_modification`,
        /// `user_consent_management`, `legal_terms_and_agreements`.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        disabled_safety_policies: Vec<String>,
    },
    /// Model Context Protocol (MCP) server
    McpServer {
        name: String,
        url: String,
        /// Optional per-mode restrictions on which server tools the model may
        /// call (wire: `allowed_tools: [{mode, tools}]`).
        #[serde(skip_serializing_if = "crate::serde_util::is_none_or_empty")]
        allowed_tools: Option<Vec<AllowedTools>>,
        /// Optional headers for authentication or configuration
        #[serde(skip_serializing_if = "crate::serde_util::is_none_or_empty")]
        headers: Option<HashMap<String, String>>,
    },
    /// Built-in file search tool for semantic retrieval over document stores
    FileSearch {
        /// Names of file search stores to query (wire: `file_search_store_names`)
        #[serde(rename = "file_search_store_names")]
        store_names: Vec<String>,
        /// Number of semantic retrieval chunks to retrieve
        #[serde(skip_serializing_if = "Option::is_none")]
        top_k: Option<i32>,
        /// Metadata filter for documents and chunks
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata_filter: Option<String>,
    },
    /// Built-in retrieval tool for grounding over external retrieval
    /// backends (Vertex AI Search, RAG stores, Exa.ai, Parallel.ai).
    ///
    /// Prefer constructing via [`RetrievalConfig`], which keeps
    /// `retrieval_types` in sync with the per-backend configs.
    ///
    /// Server-side constraint (verified live 2026-07): the Gemini API
    /// rejects `type: "retrieval"` — "not supported ... on the Gemini API,
    /// it is allowed on the Gemini Enterprise Agent Platform" (Vertex).
    /// The Gemini API's supported tool types are `google_maps`,
    /// `mcp_server`, `function`, `google_search`, `file_search`,
    /// `computer_use`, `code_execution`, and `url_context`.
    Retrieval {
        /// The retrieval backends to enable.
        #[serde(skip_serializing_if = "crate::serde_util::is_none_or_empty")]
        retrieval_types: Option<Vec<RetrievalType>>,
        /// Configuration for Vertex AI Search.
        #[serde(skip_serializing_if = "Option::is_none")]
        vertex_ai_search_config: Option<VertexAiSearchConfig>,
        /// Configuration for Exa.ai search.
        #[serde(skip_serializing_if = "Option::is_none")]
        exa_ai_search_config: Option<ExaAiSearchConfig>,
        /// Configuration for Parallel.ai search.
        #[serde(skip_serializing_if = "Option::is_none")]
        parallel_ai_search_config: Option<ParallelAiSearchConfig>,
        /// Configuration for RAG Store retrieval.
        ///
        /// Boxed to keep the `Tool` enum small (this is the largest tool
        /// configuration).
        #[serde(skip_serializing_if = "Option::is_none")]
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
    // Serializes as `"type": tool_type` merged with the other entries of
    // `data` (see `serialize_unknown_merged`), so a hand-built unknown tool
    // still goes out with its type.
    #[serde(
        untagged,
        serialize_with = "crate::serde_util::serialize_unknown_merged"
    )]
    Unknown {
        /// The unrecognized tool type name from the API
        tool_type: String,
        /// The full JSON data for this tool, preserved for debugging
        data: serde_json::Value,
    },
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
                #[serde(default)]
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

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
