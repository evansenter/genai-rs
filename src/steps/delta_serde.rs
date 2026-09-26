use serde::{Deserialize, Serialize};

use crate::content::{
    Annotation, CodeExecutionLanguage, Content, FileSearchResultItem, GoogleMapsResultItem,
    GoogleSearchResultItem, Resolution, UrlContextResultItem,
};

use super::step_serde::string_vec_from_arguments;
use super::{FunctionResultPayload, StepDelta};

impl Serialize for StepDelta {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        /// Serialize an optional entry only when present.
        macro_rules! opt_entry {
            ($map:expr, $key:literal, $val:expr) => {
                if let Some(v) = $val {
                    $map.serialize_entry($key, v)?;
                }
            };
        }

        let mut map = serializer.serialize_map(None)?;
        match self {
            Self::Text { text } => {
                map.serialize_entry("type", "text")?;
                map.serialize_entry("text", text)?;
            }
            Self::Image {
                data,
                uri,
                mime_type,
                resolution,
            } => {
                map.serialize_entry("type", "image")?;
                opt_entry!(map, "data", data);
                opt_entry!(map, "uri", uri);
                opt_entry!(map, "mime_type", mime_type);
                opt_entry!(map, "resolution", resolution);
            }
            Self::Audio {
                data,
                uri,
                mime_type,
                rate,
                sample_rate,
                channels,
            } => {
                map.serialize_entry("type", "audio")?;
                opt_entry!(map, "data", data);
                opt_entry!(map, "uri", uri);
                opt_entry!(map, "mime_type", mime_type);
                opt_entry!(map, "rate", rate);
                opt_entry!(map, "sample_rate", sample_rate);
                opt_entry!(map, "channels", channels);
            }
            Self::Video {
                data,
                uri,
                mime_type,
                resolution,
            } => {
                map.serialize_entry("type", "video")?;
                opt_entry!(map, "data", data);
                opt_entry!(map, "uri", uri);
                opt_entry!(map, "mime_type", mime_type);
                opt_entry!(map, "resolution", resolution);
            }
            Self::Document {
                data,
                uri,
                mime_type,
            } => {
                map.serialize_entry("type", "document")?;
                opt_entry!(map, "data", data);
                opt_entry!(map, "uri", uri);
                opt_entry!(map, "mime_type", mime_type);
            }
            Self::ThoughtSummary { content } => {
                map.serialize_entry("type", "thought_summary")?;
                opt_entry!(map, "content", content);
            }
            Self::ThoughtSignature { signature } => {
                map.serialize_entry("type", "thought_signature")?;
                opt_entry!(map, "signature", signature);
            }
            Self::TextAnnotation { annotations } => {
                map.serialize_entry("type", "text_annotation_delta")?;
                map.serialize_entry("annotations", annotations)?;
            }
            Self::ArgumentsDelta { arguments } => {
                map.serialize_entry("type", "arguments_delta")?;
                map.serialize_entry("arguments", arguments)?;
            }
            Self::FunctionResult {
                call_id,
                name,
                result,
                is_error,
            } => {
                map.serialize_entry("type", "function_result")?;
                opt_entry!(map, "call_id", call_id);
                opt_entry!(map, "name", name);
                map.serialize_entry("result", result)?;
                opt_entry!(map, "is_error", is_error);
            }
            Self::CodeExecutionCall {
                language,
                code,
                signature,
            } => {
                map.serialize_entry("type", "code_execution_call")?;
                map.serialize_entry(
                    "arguments",
                    &serde_json::json!({ "language": language, "code": code }),
                )?;
                opt_entry!(map, "signature", signature);
            }
            Self::CodeExecutionResult {
                result,
                is_error,
                signature,
            } => {
                map.serialize_entry("type", "code_execution_result")?;
                map.serialize_entry("result", result)?;
                opt_entry!(map, "is_error", is_error);
                opt_entry!(map, "signature", signature);
            }
            Self::UrlContextCall { urls, signature } => {
                map.serialize_entry("type", "url_context_call")?;
                map.serialize_entry("arguments", &serde_json::json!({ "urls": urls }))?;
                opt_entry!(map, "signature", signature);
            }
            Self::UrlContextResult {
                result,
                is_error,
                signature,
            } => {
                map.serialize_entry("type", "url_context_result")?;
                map.serialize_entry("result", result)?;
                opt_entry!(map, "is_error", is_error);
                opt_entry!(map, "signature", signature);
            }
            Self::GoogleSearchCall { queries, signature } => {
                map.serialize_entry("type", "google_search_call")?;
                map.serialize_entry("arguments", &serde_json::json!({ "queries": queries }))?;
                opt_entry!(map, "signature", signature);
            }
            Self::GoogleSearchResult {
                result,
                is_error,
                signature,
            } => {
                map.serialize_entry("type", "google_search_result")?;
                map.serialize_entry("result", result)?;
                opt_entry!(map, "is_error", is_error);
                opt_entry!(map, "signature", signature);
            }
            Self::McpServerToolCall {
                name,
                server_name,
                arguments,
            } => {
                map.serialize_entry("type", "mcp_server_tool_call")?;
                map.serialize_entry("name", name)?;
                map.serialize_entry("server_name", server_name)?;
                map.serialize_entry("arguments", arguments)?;
            }
            Self::McpServerToolResult {
                name,
                server_name,
                result,
            } => {
                map.serialize_entry("type", "mcp_server_tool_result")?;
                opt_entry!(map, "name", name);
                opt_entry!(map, "server_name", server_name);
                map.serialize_entry("result", result)?;
            }
            Self::FileSearchCall { signature } => {
                map.serialize_entry("type", "file_search_call")?;
                opt_entry!(map, "signature", signature);
            }
            Self::FileSearchResult { result, signature } => {
                map.serialize_entry("type", "file_search_result")?;
                map.serialize_entry("result", result)?;
                opt_entry!(map, "signature", signature);
            }
            Self::GoogleMapsCall { queries, signature } => {
                map.serialize_entry("type", "google_maps_call")?;
                map.serialize_entry("arguments", &serde_json::json!({ "queries": queries }))?;
                opt_entry!(map, "signature", signature);
            }
            Self::GoogleMapsResult { result, signature } => {
                map.serialize_entry("type", "google_maps_result")?;
                map.serialize_entry("result", result)?;
                opt_entry!(map, "signature", signature);
            }
            Self::ProcessingCall { signature } => {
                map.serialize_entry("type", "processing_call")?;
                opt_entry!(map, "signature", signature);
            }
            Self::ProcessingResult { signature } => {
                map.serialize_entry("type", "processing_result")?;
                opt_entry!(map, "signature", signature);
            }
            Self::RetrievalCall {
                queries,
                retrieval_type,
                signature,
            } => {
                map.serialize_entry("type", "retrieval_call")?;
                map.serialize_entry("arguments", &serde_json::json!({ "queries": queries }))?;
                opt_entry!(map, "retrieval_type", retrieval_type);
                opt_entry!(map, "signature", signature);
            }
            Self::RetrievalResult {
                is_error,
                signature,
            } => {
                map.serialize_entry("type", "retrieval_result")?;
                opt_entry!(map, "is_error", is_error);
                opt_entry!(map, "signature", signature);
            }
            Self::Unknown { delta_type, data } => {
                map.serialize_entry("type", delta_type)?;
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

impl<'de> Deserialize<'de> for StepDelta {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;

        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "snake_case")]
        enum KnownDelta {
            Text {
                #[serde(default)]
                text: String,
            },
            Image {
                #[serde(default)]
                data: Option<String>,
                #[serde(default)]
                uri: Option<String>,
                #[serde(default)]
                mime_type: Option<String>,
                #[serde(default)]
                resolution: Option<Resolution>,
            },
            Audio {
                #[serde(default)]
                data: Option<String>,
                #[serde(default)]
                uri: Option<String>,
                #[serde(default)]
                mime_type: Option<String>,
                #[serde(default)]
                rate: Option<u32>,
                #[serde(default)]
                sample_rate: Option<u32>,
                #[serde(default)]
                channels: Option<u32>,
            },
            Video {
                #[serde(default)]
                data: Option<String>,
                #[serde(default)]
                uri: Option<String>,
                #[serde(default)]
                mime_type: Option<String>,
                #[serde(default)]
                resolution: Option<Resolution>,
            },
            Document {
                #[serde(default)]
                data: Option<String>,
                #[serde(default)]
                uri: Option<String>,
                #[serde(default)]
                mime_type: Option<String>,
            },
            ThoughtSummary {
                #[serde(default)]
                content: Option<Content>,
            },
            ThoughtSignature {
                #[serde(default)]
                signature: Option<String>,
            },
            #[serde(rename = "text_annotation_delta")]
            TextAnnotation {
                #[serde(default)]
                annotations: Vec<Annotation>,
            },
            ArgumentsDelta {
                #[serde(default)]
                arguments: String,
            },
            FunctionResult {
                #[serde(default)]
                call_id: Option<String>,
                #[serde(default)]
                name: Option<String>,
                #[serde(default)]
                result: Option<FunctionResultPayload>,
                #[serde(default)]
                is_error: Option<bool>,
            },
            CodeExecutionCall {
                #[serde(default)]
                arguments: Option<serde_json::Value>,
                #[serde(default)]
                signature: Option<String>,
            },
            CodeExecutionResult {
                #[serde(default)]
                result: String,
                #[serde(default)]
                is_error: Option<bool>,
                #[serde(default)]
                signature: Option<String>,
            },
            UrlContextCall {
                #[serde(default)]
                arguments: Option<serde_json::Value>,
                #[serde(default)]
                signature: Option<String>,
            },
            UrlContextResult {
                #[serde(default)]
                result: Vec<UrlContextResultItem>,
                #[serde(default)]
                is_error: Option<bool>,
                #[serde(default)]
                signature: Option<String>,
            },
            GoogleSearchCall {
                #[serde(default)]
                arguments: Option<serde_json::Value>,
                #[serde(default)]
                signature: Option<String>,
            },
            GoogleSearchResult {
                #[serde(default)]
                result: Vec<GoogleSearchResultItem>,
                #[serde(default)]
                is_error: Option<bool>,
                #[serde(default)]
                signature: Option<String>,
            },
            McpServerToolCall {
                name: String,
                server_name: String,
                #[serde(default)]
                arguments: serde_json::Value,
            },
            McpServerToolResult {
                #[serde(default)]
                name: Option<String>,
                #[serde(default)]
                server_name: Option<String>,
                #[serde(default)]
                result: Option<FunctionResultPayload>,
            },
            FileSearchCall {
                #[serde(default)]
                signature: Option<String>,
            },
            FileSearchResult {
                #[serde(default)]
                result: Vec<FileSearchResultItem>,
                #[serde(default)]
                signature: Option<String>,
            },
            GoogleMapsCall {
                #[serde(default)]
                arguments: Option<serde_json::Value>,
                #[serde(default)]
                signature: Option<String>,
            },
            GoogleMapsResult {
                #[serde(default)]
                result: Vec<GoogleMapsResultItem>,
                #[serde(default)]
                signature: Option<String>,
            },
            ProcessingCall {
                #[serde(default)]
                signature: Option<String>,
            },
            ProcessingResult {
                #[serde(default)]
                signature: Option<String>,
            },
            RetrievalCall {
                #[serde(default)]
                arguments: Option<serde_json::Value>,
                #[serde(default)]
                retrieval_type: Option<crate::tools::RetrievalType>,
                #[serde(default)]
                signature: Option<String>,
            },
            RetrievalResult {
                #[serde(default)]
                is_error: Option<bool>,
                #[serde(default)]
                signature: Option<String>,
            },
        }

        match serde_json::from_value::<KnownDelta>(value.clone()) {
            Ok(known) => Ok(match known {
                KnownDelta::Text { text } => StepDelta::Text { text },
                KnownDelta::Image {
                    data,
                    uri,
                    mime_type,
                    resolution,
                } => StepDelta::Image {
                    data,
                    uri,
                    mime_type,
                    resolution,
                },
                KnownDelta::Audio {
                    data,
                    uri,
                    mime_type,
                    rate,
                    sample_rate,
                    channels,
                } => StepDelta::Audio {
                    data,
                    uri,
                    mime_type,
                    rate,
                    sample_rate,
                    channels,
                },
                KnownDelta::Video {
                    data,
                    uri,
                    mime_type,
                    resolution,
                } => StepDelta::Video {
                    data,
                    uri,
                    mime_type,
                    resolution,
                },
                KnownDelta::Document {
                    data,
                    uri,
                    mime_type,
                } => StepDelta::Document {
                    data,
                    uri,
                    mime_type,
                },
                KnownDelta::ThoughtSummary { content } => StepDelta::ThoughtSummary { content },
                KnownDelta::ThoughtSignature { signature } => {
                    StepDelta::ThoughtSignature { signature }
                }
                KnownDelta::TextAnnotation { annotations } => {
                    StepDelta::TextAnnotation { annotations }
                }
                KnownDelta::ArgumentsDelta { arguments } => StepDelta::ArgumentsDelta { arguments },
                KnownDelta::FunctionResult {
                    call_id,
                    name,
                    result,
                    is_error,
                } => StepDelta::FunctionResult {
                    call_id,
                    name,
                    result: result.unwrap_or(FunctionResultPayload::Json(serde_json::Value::Null)),
                    is_error,
                },
                KnownDelta::CodeExecutionCall {
                    arguments,
                    signature,
                } => {
                    StepDelta::CodeExecutionCall {
                        language: arguments.as_ref().and_then(|a| a.get("language")).and_then(
                            |l| serde_json::from_value::<CodeExecutionLanguage>(l.clone()).ok(),
                        ),
                        code: arguments
                            .as_ref()
                            .and_then(|a| a.get("code"))
                            .and_then(|c| c.as_str())
                            .map(String::from),
                        signature,
                    }
                }
                KnownDelta::CodeExecutionResult {
                    result,
                    is_error,
                    signature,
                } => StepDelta::CodeExecutionResult {
                    result,
                    is_error,
                    signature,
                },
                KnownDelta::UrlContextCall {
                    arguments,
                    signature,
                } => StepDelta::UrlContextCall {
                    urls: string_vec_from_arguments(arguments.as_ref(), "urls"),
                    signature,
                },
                KnownDelta::UrlContextResult {
                    result,
                    is_error,
                    signature,
                } => StepDelta::UrlContextResult {
                    result,
                    is_error,
                    signature,
                },
                KnownDelta::GoogleSearchCall {
                    arguments,
                    signature,
                } => StepDelta::GoogleSearchCall {
                    queries: string_vec_from_arguments(arguments.as_ref(), "queries"),
                    signature,
                },
                KnownDelta::GoogleSearchResult {
                    result,
                    is_error,
                    signature,
                } => StepDelta::GoogleSearchResult {
                    result,
                    is_error,
                    signature,
                },
                KnownDelta::McpServerToolCall {
                    name,
                    server_name,
                    arguments,
                } => StepDelta::McpServerToolCall {
                    name,
                    server_name,
                    arguments,
                },
                KnownDelta::McpServerToolResult {
                    name,
                    server_name,
                    result,
                } => StepDelta::McpServerToolResult {
                    name,
                    server_name,
                    result: result.unwrap_or(FunctionResultPayload::Json(serde_json::Value::Null)),
                },
                KnownDelta::FileSearchCall { signature } => StepDelta::FileSearchCall { signature },
                KnownDelta::FileSearchResult { result, signature } => {
                    StepDelta::FileSearchResult { result, signature }
                }
                KnownDelta::GoogleMapsCall {
                    arguments,
                    signature,
                } => StepDelta::GoogleMapsCall {
                    queries: string_vec_from_arguments(arguments.as_ref(), "queries"),
                    signature,
                },
                KnownDelta::GoogleMapsResult { result, signature } => {
                    StepDelta::GoogleMapsResult { result, signature }
                }
                KnownDelta::ProcessingCall { signature } => StepDelta::ProcessingCall { signature },
                KnownDelta::ProcessingResult { signature } => {
                    StepDelta::ProcessingResult { signature }
                }
                KnownDelta::RetrievalCall {
                    arguments,
                    retrieval_type,
                    signature,
                } => StepDelta::RetrievalCall {
                    queries: string_vec_from_arguments(arguments.as_ref(), "queries"),
                    retrieval_type,
                    signature,
                },
                KnownDelta::RetrievalResult {
                    is_error,
                    signature,
                } => StepDelta::RetrievalResult {
                    is_error,
                    signature,
                },
            }),
            Err(parse_error) => {
                let delta_type = value
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<missing type>")
                    .to_string();

                tracing::warn!(
                    "Encountered unknown StepDelta type '{}'. Parse error: {}. \
                     The delta will be preserved in the Unknown variant.",
                    delta_type,
                    parse_error
                );

                Ok(StepDelta::Unknown {
                    delta_type,
                    data: value,
                })
            }
        }
    }
}
