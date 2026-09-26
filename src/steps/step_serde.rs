use serde::{Deserialize, Serialize};

use crate::content::{
    CodeExecutionLanguage, Content, FileSearchResultItem, GoogleMapsResultItem,
    GoogleSearchResultItem, UrlContextResultItem,
};

use super::{FunctionResultPayload, Step, StepError};

impl Serialize for Step {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(None)?;
        match self {
            Self::UserInput { content } => {
                map.serialize_entry("type", "user_input")?;
                map.serialize_entry("content", content)?;
            }
            Self::ModelOutput { content, error } => {
                map.serialize_entry("type", "model_output")?;
                map.serialize_entry("content", content)?;
                if let Some(e) = error {
                    map.serialize_entry("error", e)?;
                }
            }
            Self::Thought { signature, summary } => {
                map.serialize_entry("type", "thought")?;
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
                if !summary.is_empty() {
                    map.serialize_entry("summary", summary)?;
                }
            }
            Self::FunctionCall {
                id,
                name,
                arguments,
                signature,
            } => {
                map.serialize_entry("type", "function_call")?;
                map.serialize_entry("id", id)?;
                map.serialize_entry("name", name)?;
                map.serialize_entry("arguments", arguments)?;
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::FunctionResult {
                call_id,
                name,
                result,
                is_error,
                signature,
            } => {
                map.serialize_entry("type", "function_result")?;
                map.serialize_entry("call_id", call_id)?;
                if let Some(n) = name {
                    map.serialize_entry("name", n)?;
                }
                map.serialize_entry("result", result)?;
                if let Some(e) = is_error {
                    map.serialize_entry("is_error", e)?;
                }
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::CodeExecutionCall {
                id,
                language,
                code,
                signature,
            } => {
                map.serialize_entry("type", "code_execution_call")?;
                map.serialize_entry("id", id)?;
                map.serialize_entry(
                    "arguments",
                    &serde_json::json!({ "language": language, "code": code }),
                )?;
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::CodeExecutionResult {
                call_id,
                result,
                is_error,
                signature,
            } => {
                map.serialize_entry("type", "code_execution_result")?;
                map.serialize_entry("call_id", call_id)?;
                map.serialize_entry("result", result)?;
                map.serialize_entry("is_error", is_error)?;
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::UrlContextCall {
                id,
                urls,
                signature,
            } => {
                map.serialize_entry("type", "url_context_call")?;
                map.serialize_entry("id", id)?;
                map.serialize_entry("arguments", &serde_json::json!({ "urls": urls }))?;
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::UrlContextResult {
                call_id,
                result,
                is_error,
                signature,
            } => {
                map.serialize_entry("type", "url_context_result")?;
                map.serialize_entry("call_id", call_id)?;
                map.serialize_entry("result", result)?;
                if let Some(e) = is_error {
                    map.serialize_entry("is_error", e)?;
                }
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::GoogleSearchCall {
                id,
                queries,
                search_type,
                signature,
            } => {
                map.serialize_entry("type", "google_search_call")?;
                map.serialize_entry("id", id)?;
                map.serialize_entry("arguments", &serde_json::json!({ "queries": queries }))?;
                if let Some(st) = search_type {
                    map.serialize_entry("search_type", st)?;
                }
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::GoogleSearchResult {
                call_id,
                result,
                is_error,
                signature,
            } => {
                map.serialize_entry("type", "google_search_result")?;
                map.serialize_entry("call_id", call_id)?;
                map.serialize_entry("result", result)?;
                if let Some(e) = is_error {
                    map.serialize_entry("is_error", e)?;
                }
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::ToolCall { id, signature } => {
                map.serialize_entry("type", "tool_call")?;
                map.serialize_entry("id", id)?;
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::McpServerToolCall {
                id,
                name,
                server_name,
                arguments,
            } => {
                map.serialize_entry("type", "mcp_server_tool_call")?;
                map.serialize_entry("id", id)?;
                map.serialize_entry("name", name)?;
                map.serialize_entry("server_name", server_name)?;
                map.serialize_entry("arguments", arguments)?;
            }
            Self::McpServerToolResult {
                call_id,
                name,
                server_name,
                result,
            } => {
                map.serialize_entry("type", "mcp_server_tool_result")?;
                map.serialize_entry("call_id", call_id)?;
                if let Some(n) = name {
                    map.serialize_entry("name", n)?;
                }
                if let Some(sn) = server_name {
                    map.serialize_entry("server_name", sn)?;
                }
                map.serialize_entry("result", result)?;
            }
            Self::FileSearchCall { id, signature } => {
                map.serialize_entry("type", "file_search_call")?;
                map.serialize_entry("id", id)?;
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::FileSearchResult {
                call_id,
                result,
                signature,
            } => {
                map.serialize_entry("type", "file_search_result")?;
                map.serialize_entry("call_id", call_id)?;
                if !result.is_empty() {
                    map.serialize_entry("result", result)?;
                }
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::GoogleMapsCall {
                id,
                queries,
                signature,
            } => {
                map.serialize_entry("type", "google_maps_call")?;
                map.serialize_entry("id", id)?;
                map.serialize_entry("arguments", &serde_json::json!({ "queries": queries }))?;
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::GoogleMapsResult {
                call_id,
                result,
                signature,
            } => {
                map.serialize_entry("type", "google_maps_result")?;
                map.serialize_entry("call_id", call_id)?;
                map.serialize_entry("result", result)?;
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::ProcessingCall { id, signature } => {
                map.serialize_entry("type", "processing_call")?;
                map.serialize_entry("id", id)?;
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::ProcessingResult { call_id, signature } => {
                map.serialize_entry("type", "processing_result")?;
                map.serialize_entry("call_id", call_id)?;
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::RetrievalCall {
                id,
                queries,
                retrieval_type,
                signature,
            } => {
                map.serialize_entry("type", "retrieval_call")?;
                map.serialize_entry("id", id)?;
                map.serialize_entry("arguments", &serde_json::json!({ "queries": queries }))?;
                if let Some(t) = retrieval_type {
                    map.serialize_entry("retrieval_type", t)?;
                }
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::RetrievalResult {
                call_id,
                is_error,
                signature,
            } => {
                map.serialize_entry("type", "retrieval_result")?;
                map.serialize_entry("call_id", call_id)?;
                if let Some(e) = is_error {
                    map.serialize_entry("is_error", e)?;
                }
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::Unknown { step_type, data } => {
                map.serialize_entry("type", step_type)?;
                match data {
                    serde_json::Value::Object(obj) => {
                        for (key, value) in obj {
                            if key != "type" {
                                map.serialize_entry(key, value)?;
                            }
                        }
                    }
                    other if !other.is_null() => {
                        map.serialize_entry("data", other)?;
                    }
                    _ => {}
                }
            }
        }
        map.end()
    }
}

/// Extracts a `Vec<String>` from `arguments.<key>` (used by call steps whose
/// arguments nest a string array).
pub(super) fn string_vec_from_arguments(
    arguments: Option<&serde_json::Value>,
    key: &str,
) -> Vec<String> {
    arguments
        .and_then(|args| args.get(key))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

impl<'de> Deserialize<'de> for Step {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[cfg(feature = "strict-unknown")]
        use serde::de::Error as _;

        let value = serde_json::Value::deserialize(deserializer)?;

        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "snake_case")]
        enum KnownStep {
            UserInput {
                #[serde(default)]
                content: Vec<Content>,
            },
            ModelOutput {
                #[serde(default)]
                content: Vec<Content>,
                #[serde(default)]
                error: Option<StepError>,
            },
            Thought {
                #[serde(default)]
                signature: Option<String>,
                #[serde(default)]
                summary: Vec<Content>,
            },
            FunctionCall {
                id: String,
                name: String,
                #[serde(default)]
                arguments: serde_json::Value,
                #[serde(default)]
                signature: Option<String>,
            },
            FunctionResult {
                call_id: String,
                #[serde(default)]
                name: Option<String>,
                #[serde(default)]
                result: Option<FunctionResultPayload>,
                #[serde(default)]
                is_error: Option<bool>,
                #[serde(default)]
                signature: Option<String>,
            },
            CodeExecutionCall {
                id: String,
                #[serde(default)]
                arguments: Option<serde_json::Value>,
                #[serde(default)]
                signature: Option<String>,
            },
            CodeExecutionResult {
                call_id: String,
                #[serde(default)]
                result: String,
                #[serde(default)]
                is_error: Option<bool>,
                #[serde(default)]
                signature: Option<String>,
            },
            UrlContextCall {
                id: String,
                #[serde(default)]
                arguments: Option<serde_json::Value>,
                #[serde(default)]
                signature: Option<String>,
            },
            UrlContextResult {
                call_id: String,
                #[serde(default)]
                result: Vec<UrlContextResultItem>,
                #[serde(default)]
                is_error: Option<bool>,
                #[serde(default)]
                signature: Option<String>,
            },
            GoogleSearchCall {
                id: String,
                #[serde(default)]
                arguments: Option<serde_json::Value>,
                #[serde(default)]
                search_type: Option<crate::tools::SearchType>,
                #[serde(default)]
                signature: Option<String>,
            },
            GoogleSearchResult {
                call_id: String,
                #[serde(default)]
                result: Vec<GoogleSearchResultItem>,
                #[serde(default)]
                is_error: Option<bool>,
                #[serde(default)]
                signature: Option<String>,
            },
            ToolCall {
                id: String,
                #[serde(default)]
                signature: Option<String>,
            },
            McpServerToolCall {
                id: String,
                name: String,
                server_name: String,
                #[serde(default)]
                arguments: serde_json::Value,
            },
            McpServerToolResult {
                call_id: String,
                #[serde(default)]
                name: Option<String>,
                #[serde(default)]
                server_name: Option<String>,
                #[serde(default)]
                result: Option<FunctionResultPayload>,
            },
            FileSearchCall {
                id: String,
                #[serde(default)]
                signature: Option<String>,
            },
            FileSearchResult {
                call_id: String,
                #[serde(default)]
                result: Vec<FileSearchResultItem>,
                #[serde(default)]
                signature: Option<String>,
            },
            GoogleMapsCall {
                id: String,
                #[serde(default)]
                arguments: Option<serde_json::Value>,
                #[serde(default)]
                signature: Option<String>,
            },
            GoogleMapsResult {
                call_id: String,
                #[serde(default)]
                result: Vec<GoogleMapsResultItem>,
                #[serde(default)]
                signature: Option<String>,
            },
            ProcessingCall {
                id: String,
                #[serde(default)]
                signature: Option<String>,
            },
            ProcessingResult {
                call_id: String,
                #[serde(default)]
                signature: Option<String>,
            },
            RetrievalCall {
                id: String,
                #[serde(default)]
                arguments: Option<serde_json::Value>,
                #[serde(default)]
                retrieval_type: Option<crate::tools::RetrievalType>,
                #[serde(default)]
                signature: Option<String>,
            },
            RetrievalResult {
                call_id: String,
                #[serde(default)]
                is_error: Option<bool>,
                #[serde(default)]
                signature: Option<String>,
            },
        }

        match serde_json::from_value::<KnownStep>(value.clone()) {
            Ok(known) => Ok(match known {
                KnownStep::UserInput { content } => Step::UserInput { content },
                KnownStep::ModelOutput { content, error } => Step::ModelOutput { content, error },
                KnownStep::Thought { signature, summary } => Step::Thought { signature, summary },
                KnownStep::FunctionCall {
                    id,
                    name,
                    arguments,
                    signature,
                } => Step::FunctionCall {
                    id,
                    name,
                    arguments,
                    signature,
                },
                KnownStep::FunctionResult {
                    call_id,
                    name,
                    result,
                    is_error,
                    signature,
                } => Step::FunctionResult {
                    call_id,
                    name,
                    result: result.unwrap_or(FunctionResultPayload::Json(serde_json::Value::Null)),
                    is_error,
                    signature,
                },
                KnownStep::CodeExecutionCall {
                    id,
                    arguments,
                    signature,
                } => {
                    let language = arguments
                        .as_ref()
                        .and_then(|a| a.get("language"))
                        .and_then(|l| {
                            serde_json::from_value::<CodeExecutionLanguage>(l.clone()).ok()
                        })
                        .unwrap_or_default();
                    let code = arguments
                        .as_ref()
                        .and_then(|a| a.get("code"))
                        .and_then(|c| c.as_str())
                        .unwrap_or_default()
                        .to_string();
                    Step::CodeExecutionCall {
                        id,
                        language,
                        code,
                        signature,
                    }
                }
                KnownStep::CodeExecutionResult {
                    call_id,
                    result,
                    is_error,
                    signature,
                } => Step::CodeExecutionResult {
                    call_id,
                    result,
                    is_error: is_error.unwrap_or(false),
                    signature,
                },
                KnownStep::UrlContextCall {
                    id,
                    arguments,
                    signature,
                } => Step::UrlContextCall {
                    id,
                    urls: string_vec_from_arguments(arguments.as_ref(), "urls"),
                    signature,
                },
                KnownStep::UrlContextResult {
                    call_id,
                    result,
                    is_error,
                    signature,
                } => Step::UrlContextResult {
                    call_id,
                    result,
                    is_error,
                    signature,
                },
                KnownStep::GoogleSearchCall {
                    id,
                    arguments,
                    search_type,
                    signature,
                } => Step::GoogleSearchCall {
                    id,
                    queries: string_vec_from_arguments(arguments.as_ref(), "queries"),
                    search_type,
                    signature,
                },
                KnownStep::GoogleSearchResult {
                    call_id,
                    result,
                    is_error,
                    signature,
                } => Step::GoogleSearchResult {
                    call_id,
                    result,
                    is_error,
                    signature,
                },
                KnownStep::ToolCall { id, signature } => Step::ToolCall { id, signature },
                KnownStep::McpServerToolCall {
                    id,
                    name,
                    server_name,
                    arguments,
                } => Step::McpServerToolCall {
                    id,
                    name,
                    server_name,
                    arguments,
                },
                KnownStep::McpServerToolResult {
                    call_id,
                    name,
                    server_name,
                    result,
                } => Step::McpServerToolResult {
                    call_id,
                    name,
                    server_name,
                    result: result.unwrap_or(FunctionResultPayload::Json(serde_json::Value::Null)),
                },
                KnownStep::FileSearchCall { id, signature } => {
                    Step::FileSearchCall { id, signature }
                }
                KnownStep::FileSearchResult {
                    call_id,
                    result,
                    signature,
                } => Step::FileSearchResult {
                    call_id,
                    result,
                    signature,
                },
                KnownStep::GoogleMapsCall {
                    id,
                    arguments,
                    signature,
                } => Step::GoogleMapsCall {
                    id,
                    queries: string_vec_from_arguments(arguments.as_ref(), "queries"),
                    signature,
                },
                KnownStep::GoogleMapsResult {
                    call_id,
                    result,
                    signature,
                } => Step::GoogleMapsResult {
                    call_id,
                    result,
                    signature,
                },
                KnownStep::ProcessingCall { id, signature } => {
                    Step::ProcessingCall { id, signature }
                }
                KnownStep::ProcessingResult { call_id, signature } => {
                    Step::ProcessingResult { call_id, signature }
                }
                KnownStep::RetrievalCall {
                    id,
                    arguments,
                    retrieval_type,
                    signature,
                } => Step::RetrievalCall {
                    id,
                    queries: string_vec_from_arguments(arguments.as_ref(), "queries"),
                    retrieval_type,
                    signature,
                },
                KnownStep::RetrievalResult {
                    call_id,
                    is_error,
                    signature,
                } => Step::RetrievalResult {
                    call_id,
                    is_error,
                    signature,
                },
            }),
            Err(parse_error) => {
                let step_type = value
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<missing type>")
                    .to_string();

                tracing::warn!(
                    "Encountered unknown Step type '{}'. Parse error: {}. \
                     This may indicate a new API feature or a malformed response. \
                     The step will be preserved in the Unknown variant.",
                    step_type,
                    parse_error
                );

                #[cfg(feature = "strict-unknown")]
                {
                    Err(D::Error::custom(format!(
                        "Unknown Step type '{}'. \
                         Strict mode is enabled via the 'strict-unknown' feature flag. \
                         Either update the library or disable strict mode.",
                        step_type
                    )))
                }

                #[cfg(not(feature = "strict-unknown"))]
                {
                    Ok(Step::Unknown {
                        step_type,
                        data: value,
                    })
                }
            }
        }
    }
}
