//! Tool declarations: [`Tool`], function declarations, the built-in tools'
//! configs, and tool choice.

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
use serde_json::{Map, Value};
use std::collections::HashMap;

use crate::redact;
use crate::serde_util::is_none_or_empty;
use crate::wire_fields::{Fallback, ModeledMap, WireFields};

/// Represents a tool that can be used by the model (Interactions API format).
///
/// Tools in the Interactions API use a flat structure with the tool type and details
/// at the top level, rather than nested in arrays.
///
/// Build one with a config ([`GoogleSearchConfig`], [`McpServerConfig`],
/// [`FunctionDeclaration::into_tool`], ...) or a constructor
/// ([`Tool::code_execution`], [`Tool::url_context`]), which leave `extra`
/// empty.
///
/// # Forward Compatibility (Evergreen Philosophy)
///
/// This enum is marked `#[non_exhaustive]`, which means:
/// - Match statements must include a wildcard arm (`_ => ...`)
/// - New variants may be added in minor version updates without breaking your code
///
/// When the API returns a tool type that this library doesn't recognize, it will be
/// captured as `Tool::Unknown` rather than causing a deserialization error.
/// A known tool keeps the fields the crate does not model in its `extra`
/// map ([`Tool::extra`]), so a field the API adds to a tool a stored
/// interaction serves back survives the round trip.
/// This follows the [Evergreen spec](https://github.com/google-deepmind/evergreen-spec)
/// philosophy of graceful degradation.
///
/// `Debug` prints an MCP server's header names with every value as
/// `[REDACTED]`, as `LOUD_WIRE` does, and the retrieval configs redact
/// their API keys.
#[derive(Clone, PartialEq)]
#[non_exhaustive]
pub enum Tool {
    /// A custom function that the model can call.
    ///
    /// Build one with [`FunctionDeclaration::builder`] and
    /// [`into_tool`](FunctionDeclaration::into_tool). Only `name` is needed
    /// for the model to call it: the API accepts a function tool with no
    /// `description` or `parameters`, and a stored interaction read back
    /// serves it that way.
    Function {
        /// The function's name. Empty when the wire tool has none (the
        /// bindings make every field optional), and then not written.
        name: String,
        /// What the function does, for the model.
        description: Option<String>,
        /// The JSON Schema of the function's arguments.
        parameters: Option<FunctionParameters>,
        /// Unmodeled wire fields, kept for round trip ([`Tool::extra`]).
        extra: Map<String, Value>,
    },
    /// Built-in Google Search tool
    ///
    /// Optionally configure `search_types` to enable web and/or image search.
    GoogleSearch {
        /// Types of search to perform (e.g., web search, image search).
        /// When `None`, the API defaults to web search only. Not written
        /// when empty.
        search_types: Option<Vec<SearchType>>,
        /// Unmodeled wire fields, kept for round trip ([`Tool::extra`]).
        extra: Map<String, Value>,
    },
    /// Built-in Google Maps tool for location-grounded responses
    GoogleMaps {
        /// Whether to enable the widget context token in the response
        enable_widget: Option<bool>,
        /// Latitude bias for location grounding
        latitude: Option<f64>,
        /// Longitude bias for location grounding
        longitude: Option<f64>,
        /// Unmodeled wire fields, kept for round trip ([`Tool::extra`]).
        extra: Map<String, Value>,
    },
    /// Built-in code execution tool ([`Tool::code_execution`]).
    CodeExecution {
        /// Unmodeled wire fields, kept for round trip ([`Tool::extra`]).
        extra: Map<String, Value>,
    },
    /// Built-in URL context tool ([`Tool::url_context`]).
    UrlContext {
        /// Unmodeled wire fields, kept for round trip ([`Tool::extra`]).
        extra: Map<String, Value>,
    },
    /// Built-in computer use tool for browser automation.
    ///
    /// **Security Warning**: This tool allows the model to interact with web browsers
    /// on your behalf. Only use with trusted models and carefully review excluded functions.
    ComputerUse {
        /// The environment being operated. Known values: `browser`, `mobile`, `desktop`.
        /// Empty when the wire tool has none, and then not written.
        environment: String,
        /// List of predefined functions to exclude from model access
        /// (wire: `excluded_predefined_functions`; not written when empty).
        excluded_predefined_functions: Vec<String>,
        /// Whether to enable prompt injection detection for this request.
        enable_prompt_injection_detection: Option<bool>,
        /// Safety policies to disable. Known values include
        /// `financial_transactions`, `sensitive_data_modification`,
        /// `communication_tool`, `account_creation`, `data_modification`,
        /// `user_consent_management`, `legal_terms_and_agreements`. Not
        /// written when empty.
        disabled_safety_policies: Vec<String>,
        /// Unmodeled wire fields, kept for round trip ([`Tool::extra`]).
        extra: Map<String, Value>,
    },
    /// Model Context Protocol (MCP) server
    McpServer {
        /// The server's name, which prefixes its calls (`<name>:<tool>`).
        /// Empty when the wire tool has none, and then not written.
        name: String,
        /// The server's URL. Empty when the wire tool has none, and then not
        /// written.
        url: String,
        /// Optional per-mode restrictions on which server tools the model may
        /// call (wire: `allowed_tools: [{mode, tools}]`; not written when
        /// empty).
        allowed_tools: Option<Vec<AllowedTools>>,
        /// Optional headers for authentication or configuration (not
        /// written when empty)
        headers: Option<HashMap<String, String>>,
        /// Unmodeled wire fields, kept for round trip ([`Tool::extra`]).
        extra: Map<String, Value>,
    },
    /// Built-in file search tool for semantic retrieval over document stores
    FileSearch {
        /// Names of file search stores to query (wire:
        /// `file_search_store_names`; not written when empty)
        store_names: Vec<String>,
        /// Number of semantic retrieval chunks to retrieve
        top_k: Option<i32>,
        /// Metadata filter for documents and chunks
        metadata_filter: Option<String>,
        /// Unmodeled wire fields, kept for round trip ([`Tool::extra`]).
        extra: Map<String, Value>,
    },
    /// Built-in retrieval tool for grounding over external retrieval
    /// backends (Vertex AI Search, RAG stores, Exa.ai, Parallel.ai).
    ///
    /// Prefer constructing via [`RetrievalConfig`], which keeps
    /// `retrieval_types` in sync with the per-backend configs.
    ///
    /// Server-side constraint: the Gemini API
    /// rejects `type: "retrieval"` — "not supported ... on the Gemini API,
    /// it is allowed on the Gemini Enterprise Agent Platform" (Vertex).
    /// The Gemini API's supported tool types are `google_maps`,
    /// `mcp_server`, `function`, `google_search`, `file_search`,
    /// `computer_use`, `code_execution`, and `url_context`.
    Retrieval {
        /// The retrieval backends to enable (not written when empty).
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
        /// Unmodeled wire fields, kept for round trip ([`Tool::extra`]).
        extra: Map<String, Value>,
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
    Unknown {
        /// The unrecognized tool type name from the API
        tool_type: String,
        /// The full JSON data for this tool, preserved for debugging
        data: serde_json::Value,
    },
}

// Hand-written only for `McpServer`'s header values, which are secrets such
// as an `Authorization` token (D-067); every other field prints as the
// derive would. Each arm lists its fields, so a new one fails to compile here.
impl std::fmt::Debug for Tool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Function {
                name,
                description,
                parameters,
                extra,
            } => f
                .debug_struct("Function")
                .field("name", name)
                .field("description", description)
                .field("parameters", parameters)
                .field("extra", extra)
                .finish(),
            Self::GoogleSearch {
                search_types,
                extra,
            } => f
                .debug_struct("GoogleSearch")
                .field("search_types", search_types)
                .field("extra", extra)
                .finish(),
            Self::GoogleMaps {
                enable_widget,
                latitude,
                longitude,
                extra,
            } => f
                .debug_struct("GoogleMaps")
                .field("enable_widget", enable_widget)
                .field("latitude", latitude)
                .field("longitude", longitude)
                .field("extra", extra)
                .finish(),
            Self::CodeExecution { extra } => f
                .debug_struct("CodeExecution")
                .field("extra", extra)
                .finish(),
            Self::UrlContext { extra } => {
                f.debug_struct("UrlContext").field("extra", extra).finish()
            }
            Self::ComputerUse {
                environment,
                excluded_predefined_functions,
                enable_prompt_injection_detection,
                disabled_safety_policies,
                extra,
            } => f
                .debug_struct("ComputerUse")
                .field("environment", environment)
                .field(
                    "excluded_predefined_functions",
                    excluded_predefined_functions,
                )
                .field(
                    "enable_prompt_injection_detection",
                    enable_prompt_injection_detection,
                )
                .field("disabled_safety_policies", disabled_safety_policies)
                .field("extra", extra)
                .finish(),
            Self::McpServer {
                name,
                url,
                allowed_tools,
                headers,
                extra,
            } => f
                .debug_struct("McpServer")
                .field("name", name)
                .field("url", url)
                .field("allowed_tools", allowed_tools)
                .field("headers", &headers.as_ref().map(redact::values))
                .field("extra", extra)
                .finish(),
            Self::FileSearch {
                store_names,
                top_k,
                metadata_filter,
                extra,
            } => f
                .debug_struct("FileSearch")
                .field("store_names", store_names)
                .field("top_k", top_k)
                .field("metadata_filter", metadata_filter)
                .field("extra", extra)
                .finish(),
            Self::Retrieval {
                retrieval_types,
                vertex_ai_search_config,
                exa_ai_search_config,
                parallel_ai_search_config,
                rag_store_config,
                extra,
            } => f
                .debug_struct("Retrieval")
                .field("retrieval_types", retrieval_types)
                .field("vertex_ai_search_config", vertex_ai_search_config)
                .field("exa_ai_search_config", exa_ai_search_config)
                .field("parallel_ai_search_config", parallel_ai_search_config)
                .field("rag_store_config", rag_store_config)
                .field("extra", extra)
                .finish(),
            Self::Unknown { tool_type, data } => f
                .debug_struct("Unknown")
                .field("tool_type", tool_type)
                .field("data", data)
                .finish(),
        }
    }
}

/// The modeled fields, then the extras that do not name one (#491).
impl Serialize for Tool {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Function {
                name,
                description,
                parameters,
                extra,
            } => {
                let mut map = ModeledMap::tagged(serializer, "function")?;
                map.field_unless(name.is_empty(), "name", name)?;
                map.opt("description", description.as_ref())?;
                map.opt("parameters", parameters.as_ref())?;
                map.end(extra)
            }
            Self::GoogleSearch {
                search_types,
                extra,
            } => {
                let mut map = ModeledMap::tagged(serializer, "google_search")?;
                map.field_unless(is_none_or_empty(search_types), "search_types", search_types)?;
                map.end(extra)
            }
            Self::GoogleMaps {
                enable_widget,
                latitude,
                longitude,
                extra,
            } => {
                let mut map = ModeledMap::tagged(serializer, "google_maps")?;
                map.opt("enable_widget", enable_widget.as_ref())?;
                map.opt("latitude", latitude.as_ref())?;
                map.opt("longitude", longitude.as_ref())?;
                map.end(extra)
            }
            Self::CodeExecution { extra } => {
                ModeledMap::tagged(serializer, "code_execution")?.end(extra)
            }
            Self::UrlContext { extra } => ModeledMap::tagged(serializer, "url_context")?.end(extra),
            Self::ComputerUse {
                environment,
                excluded_predefined_functions,
                enable_prompt_injection_detection,
                disabled_safety_policies,
                extra,
            } => {
                let mut map = ModeledMap::tagged(serializer, "computer_use")?;
                map.field_unless(environment.is_empty(), "environment", environment)?;
                map.field_unless(
                    excluded_predefined_functions.is_empty(),
                    "excluded_predefined_functions",
                    excluded_predefined_functions,
                )?;
                map.opt(
                    "enable_prompt_injection_detection",
                    enable_prompt_injection_detection.as_ref(),
                )?;
                map.field_unless(
                    disabled_safety_policies.is_empty(),
                    "disabled_safety_policies",
                    disabled_safety_policies,
                )?;
                map.end(extra)
            }
            Self::McpServer {
                name,
                url,
                allowed_tools,
                headers,
                extra,
            } => {
                let mut map = ModeledMap::tagged(serializer, "mcp_server")?;
                map.field_unless(name.is_empty(), "name", name)?;
                map.field_unless(url.is_empty(), "url", url)?;
                map.field_unless(
                    is_none_or_empty(allowed_tools),
                    "allowed_tools",
                    allowed_tools,
                )?;
                map.field_unless(is_none_or_empty(headers), "headers", headers)?;
                map.end(extra)
            }
            Self::FileSearch {
                store_names,
                top_k,
                metadata_filter,
                extra,
            } => {
                let mut map = ModeledMap::tagged(serializer, "file_search")?;
                map.field_unless(
                    store_names.is_empty(),
                    "file_search_store_names",
                    store_names,
                )?;
                map.opt("top_k", top_k.as_ref())?;
                map.opt("metadata_filter", metadata_filter.as_ref())?;
                map.end(extra)
            }
            Self::Retrieval {
                retrieval_types,
                vertex_ai_search_config,
                exa_ai_search_config,
                parallel_ai_search_config,
                rag_store_config,
                extra,
            } => {
                let mut map = ModeledMap::tagged(serializer, "retrieval")?;
                map.field_unless(
                    is_none_or_empty(retrieval_types),
                    "retrieval_types",
                    retrieval_types,
                )?;
                map.opt("vertex_ai_search_config", vertex_ai_search_config.as_ref())?;
                map.opt("exa_ai_search_config", exa_ai_search_config.as_ref())?;
                map.opt(
                    "parallel_ai_search_config",
                    parallel_ai_search_config.as_ref(),
                )?;
                map.opt("rag_store_config", rag_store_config.as_ref())?;
                map.end(extra)
            }
            Self::Unknown { tool_type, data } => {
                crate::serde_util::serialize_unknown_merged(tool_type, data, serializer)
            }
        }
    }
}

/// Reads a known tool; an error keeps it as [`Tool::Unknown`]. Every field
/// a tool carries is optional in the bindings: a served tool that leaves
/// one out is still that tool.
fn read_known(object: &Map<String, Value>) -> Result<Tool, Fallback> {
    let mut f = WireFields::tagged(object);
    let Some(tool_type) = f.tag() else {
        return Err(Fallback::UnknownType);
    };
    let tool = match tool_type {
        "function" => Tool::Function {
            name: f.or_default("name")?,
            description: f.optional("description")?,
            parameters: f.optional("parameters")?,
            extra: f.extra(),
        },
        "google_search" => Tool::GoogleSearch {
            search_types: f.optional("search_types")?,
            extra: f.extra(),
        },
        "google_maps" => Tool::GoogleMaps {
            enable_widget: f.optional("enable_widget")?,
            latitude: f.optional("latitude")?,
            longitude: f.optional("longitude")?,
            extra: f.extra(),
        },
        "code_execution" => Tool::CodeExecution { extra: f.extra() },
        "url_context" => Tool::UrlContext { extra: f.extra() },
        "computer_use" => Tool::ComputerUse {
            environment: f.or_default("environment")?,
            excluded_predefined_functions: f.or_default("excluded_predefined_functions")?,
            enable_prompt_injection_detection: f.optional("enable_prompt_injection_detection")?,
            disabled_safety_policies: f.or_default("disabled_safety_policies")?,
            extra: f.extra(),
        },
        "mcp_server" => Tool::McpServer {
            name: f.or_default("name")?,
            url: f.or_default("url")?,
            allowed_tools: f.optional("allowed_tools")?,
            headers: f.optional("headers")?,
            extra: f.extra(),
        },
        "file_search" => Tool::FileSearch {
            store_names: f.or_default("file_search_store_names")?,
            top_k: f.optional("top_k")?,
            metadata_filter: f.optional("metadata_filter")?,
            extra: f.extra(),
        },
        "retrieval" => Tool::Retrieval {
            retrieval_types: f.optional("retrieval_types")?,
            vertex_ai_search_config: f.optional("vertex_ai_search_config")?,
            exa_ai_search_config: f.optional("exa_ai_search_config")?,
            parallel_ai_search_config: f.optional("parallel_ai_search_config")?,
            rag_store_config: f.optional("rag_store_config")?,
            extra: f.extra(),
        },
        _ => return Err(Fallback::UnknownType),
    };
    Ok(tool)
}

/// An unknown type, or a known one with a field the crate cannot read, is
/// kept whole as [`Tool::Unknown`], in strict mode too.
impl<'de> Deserialize<'de> for Tool {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match Fallback::read(&value, read_known) {
            Ok(tool) => Ok(tool),
            Err(fallback) => {
                let tool_type = value
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("<missing type>")
                    .to_string();
                tracing::warn!(
                    "Keeping a tool whole in Tool::Unknown: {}. This may indicate a new \
                     API feature or a malformed response.",
                    fallback.describe("Tool", &tool_type)
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
    /// The built-in code execution tool, with no unmodeled fields.
    #[must_use]
    pub fn code_execution() -> Self {
        Self::CodeExecution { extra: Map::new() }
    }

    /// The built-in URL context tool, with no unmodeled fields.
    #[must_use]
    pub fn url_context() -> Self {
        Self::UrlContext { extra: Map::new() }
    }

    /// The fields of this tool that the crate does not model, kept so they
    /// round-trip (#491), as on [`Content::extra`](crate::Content::extra).
    /// `None` for [`Tool::Unknown`], whose `data` holds everything.
    #[must_use]
    pub fn extra(&self) -> Option<&Map<String, Value>> {
        match self {
            Self::Function { extra, .. }
            | Self::GoogleSearch { extra, .. }
            | Self::GoogleMaps { extra, .. }
            | Self::CodeExecution { extra }
            | Self::UrlContext { extra }
            | Self::ComputerUse { extra, .. }
            | Self::McpServer { extra, .. }
            | Self::FileSearch { extra, .. }
            | Self::Retrieval { extra, .. } => Some(extra),
            Self::Unknown { .. } => None,
        }
    }

    /// Mutable access to [`extra`](Self::extra). An entry named after a
    /// modeled field, or `type`, is never written.
    pub fn extra_mut(&mut self) -> Option<&mut Map<String, Value>> {
        match self {
            Self::Function { extra, .. }
            | Self::GoogleSearch { extra, .. }
            | Self::GoogleMaps { extra, .. }
            | Self::CodeExecution { extra }
            | Self::UrlContext { extra }
            | Self::ComputerUse { extra, .. }
            | Self::McpServer { extra, .. }
            | Self::FileSearch { extra, .. }
            | Self::Retrieval { extra, .. } => Some(extra),
            Self::Unknown { .. } => None,
        }
    }

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
