//! Wire (de)serialization for [`StepDelta`], on the same `extra` /
//! `arguments_extra` scheme as `Step` (see `step_serde`).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::wire_fields::{Fallback, ModeledMap, WireFields};

use super::StepDelta;
use super::step_serde::{
    code_arguments, payload, queries_arguments, read_arguments, unknown_fields, urls_arguments,
};

impl StepDelta {
    /// The wire `type` tag for this delta.
    pub(crate) fn delta_type(&self) -> &str {
        match self {
            Self::Text { .. } => "text",
            Self::Image { .. } => "image",
            Self::Audio { .. } => "audio",
            Self::Video { .. } => "video",
            Self::Document { .. } => "document",
            Self::ThoughtSummary { .. } => "thought_summary",
            Self::ThoughtSignature { .. } => "thought_signature",
            Self::TextAnnotation { .. } => "text_annotation_delta",
            Self::ArgumentsDelta { .. } => "arguments_delta",
            Self::FunctionResult { .. } => "function_result",
            Self::CodeExecutionCall { .. } => "code_execution_call",
            Self::CodeExecutionResult { .. } => "code_execution_result",
            Self::UrlContextCall { .. } => "url_context_call",
            Self::UrlContextResult { .. } => "url_context_result",
            Self::GoogleSearchCall { .. } => "google_search_call",
            Self::GoogleSearchResult { .. } => "google_search_result",
            Self::McpServerToolCall { .. } => "mcp_server_tool_call",
            Self::McpServerToolResult { .. } => "mcp_server_tool_result",
            Self::FileSearchCall { .. } => "file_search_call",
            Self::FileSearchResult { .. } => "file_search_result",
            Self::GoogleMapsCall { .. } => "google_maps_call",
            Self::GoogleMapsResult { .. } => "google_maps_result",
            Self::ProcessingCall { .. } => "processing_call",
            Self::ProcessingResult { .. } => "processing_result",
            Self::RetrievalCall { .. } => "retrieval_call",
            Self::RetrievalResult { .. } => "retrieval_result",
            Self::Unknown { delta_type, .. } => delta_type,
        }
    }
}

impl Serialize for StepDelta {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = ModeledMap::tagged(serializer, self.delta_type())?;
        let extra = match self {
            Self::Text { text, extra } => {
                map.field("text", text)?;
                extra
            }
            Self::Image {
                data,
                uri,
                mime_type,
                resolution,
                extra,
            }
            | Self::Video {
                data,
                uri,
                mime_type,
                resolution,
                extra,
            } => {
                map.opt("data", data.as_ref())?;
                map.opt("uri", uri.as_ref())?;
                map.opt("mime_type", mime_type.as_ref())?;
                map.opt("resolution", resolution.as_ref())?;
                extra
            }
            Self::Audio {
                data,
                uri,
                mime_type,
                rate,
                sample_rate,
                channels,
                extra,
            } => {
                map.opt("data", data.as_ref())?;
                map.opt("uri", uri.as_ref())?;
                map.opt("mime_type", mime_type.as_ref())?;
                map.opt("rate", rate.as_ref())?;
                map.opt("sample_rate", sample_rate.as_ref())?;
                map.opt("channels", channels.as_ref())?;
                extra
            }
            Self::Document {
                data,
                uri,
                mime_type,
                extra,
            } => {
                map.opt("data", data.as_ref())?;
                map.opt("uri", uri.as_ref())?;
                map.opt("mime_type", mime_type.as_ref())?;
                extra
            }
            Self::ThoughtSummary { content, extra } => {
                map.opt("content", content.as_ref())?;
                extra
            }
            Self::ThoughtSignature { signature, extra }
            | Self::FileSearchCall { signature, extra }
            | Self::ProcessingCall { signature, extra }
            | Self::ProcessingResult { signature, extra } => {
                map.opt("signature", signature.as_ref())?;
                extra
            }
            Self::TextAnnotation { annotations, extra } => {
                map.field("annotations", annotations)?;
                extra
            }
            Self::ArgumentsDelta { arguments, extra } => {
                map.field("arguments", arguments)?;
                extra
            }
            Self::FunctionResult {
                call_id,
                name,
                result,
                is_error,
                extra,
            } => {
                map.opt("call_id", call_id.as_ref())?;
                map.opt("name", name.as_ref())?;
                map.field("result", result)?;
                map.opt("is_error", is_error.as_ref())?;
                extra
            }
            Self::CodeExecutionCall {
                language,
                code,
                signature,
                arguments_extra,
                extra,
            } => {
                map.field(
                    "arguments",
                    &code_arguments(language.as_ref(), code.as_deref(), arguments_extra),
                )?;
                map.opt("signature", signature.as_ref())?;
                extra
            }
            Self::CodeExecutionResult {
                result,
                is_error,
                signature,
                extra,
            } => {
                map.field("result", result)?;
                map.opt("is_error", is_error.as_ref())?;
                map.opt("signature", signature.as_ref())?;
                extra
            }
            Self::UrlContextCall {
                urls,
                signature,
                arguments_extra,
                extra,
            } => {
                map.field("arguments", &urls_arguments(urls, arguments_extra))?;
                map.opt("signature", signature.as_ref())?;
                extra
            }
            Self::UrlContextResult {
                result,
                is_error,
                signature,
                extra,
            } => {
                map.field("result", result)?;
                map.opt("is_error", is_error.as_ref())?;
                map.opt("signature", signature.as_ref())?;
                extra
            }
            Self::GoogleSearchCall {
                queries,
                signature,
                arguments_extra,
                extra,
            }
            | Self::GoogleMapsCall {
                queries,
                signature,
                arguments_extra,
                extra,
            } => {
                map.field("arguments", &queries_arguments(queries, arguments_extra))?;
                map.opt("signature", signature.as_ref())?;
                extra
            }
            Self::GoogleSearchResult {
                result,
                is_error,
                signature,
                extra,
            } => {
                map.field("result", result)?;
                map.opt("is_error", is_error.as_ref())?;
                map.opt("signature", signature.as_ref())?;
                extra
            }
            Self::McpServerToolCall {
                name,
                server_name,
                arguments,
                extra,
            } => {
                map.field("name", name)?;
                map.field("server_name", server_name)?;
                map.field("arguments", arguments)?;
                extra
            }
            Self::McpServerToolResult {
                name,
                server_name,
                result,
                extra,
            } => {
                map.opt("name", name.as_ref())?;
                map.opt("server_name", server_name.as_ref())?;
                map.field("result", result)?;
                extra
            }
            Self::FileSearchResult {
                result,
                signature,
                extra,
            } => {
                map.field("result", result)?;
                map.opt("signature", signature.as_ref())?;
                extra
            }
            Self::GoogleMapsResult {
                result,
                signature,
                extra,
            } => {
                map.field("result", result)?;
                map.opt("signature", signature.as_ref())?;
                extra
            }
            Self::RetrievalCall {
                queries,
                retrieval_type,
                signature,
                arguments_extra,
                extra,
            } => {
                map.field("arguments", &queries_arguments(queries, arguments_extra))?;
                map.opt("retrieval_type", retrieval_type.as_ref())?;
                map.opt("signature", signature.as_ref())?;
                extra
            }
            Self::RetrievalResult {
                is_error,
                signature,
                extra,
            } => {
                map.opt("is_error", is_error.as_ref())?;
                map.opt("signature", signature.as_ref())?;
                extra
            }
            Self::Unknown { data, .. } => {
                return map.end(&unknown_fields(data));
            }
        };
        map.end(extra)
    }
}

/// Reads a known delta from its wire object; an error keeps it as
/// [`StepDelta::Unknown`].
fn read_known(object: &Map<String, Value>) -> Result<StepDelta, Fallback> {
    let mut f = WireFields::tagged(object);
    let Some(delta_type) = f.tag() else {
        return Err(Fallback::UnknownType);
    };
    let delta = match delta_type {
        "text" => StepDelta::Text {
            text: f.or_default("text")?,
            extra: f.extra(),
        },
        "image" => StepDelta::Image {
            data: f.optional("data")?,
            uri: f.optional("uri")?,
            mime_type: f.optional("mime_type")?,
            resolution: f.optional("resolution")?,
            extra: f.extra(),
        },
        "audio" => StepDelta::Audio {
            data: f.optional("data")?,
            uri: f.optional("uri")?,
            mime_type: f.optional("mime_type")?,
            rate: f.optional("rate")?,
            sample_rate: f.optional("sample_rate")?,
            channels: f.optional("channels")?,
            extra: f.extra(),
        },
        "video" => StepDelta::Video {
            data: f.optional("data")?,
            uri: f.optional("uri")?,
            mime_type: f.optional("mime_type")?,
            resolution: f.optional("resolution")?,
            extra: f.extra(),
        },
        "document" => StepDelta::Document {
            data: f.optional("data")?,
            uri: f.optional("uri")?,
            mime_type: f.optional("mime_type")?,
            extra: f.extra(),
        },
        "thought_summary" => StepDelta::ThoughtSummary {
            content: f.optional("content")?,
            extra: f.extra(),
        },
        "thought_signature" => StepDelta::ThoughtSignature {
            signature: f.optional("signature")?,
            extra: f.extra(),
        },
        "text_annotation_delta" => StepDelta::TextAnnotation {
            annotations: f.or_default("annotations")?,
            extra: f.extra(),
        },
        "arguments_delta" => StepDelta::ArgumentsDelta {
            arguments: f.or_default("arguments")?,
            extra: f.extra(),
        },
        "function_result" => StepDelta::FunctionResult {
            call_id: f.optional("call_id")?,
            name: f.optional("name")?,
            result: payload(&mut f)?,
            is_error: f.optional("is_error")?,
            extra: f.extra(),
        },
        "code_execution_call" => {
            let ((language, code), arguments_extra) = read_arguments(&mut f, |a| {
                Ok((a.optional("language")?, a.optional("code")?))
            })?;
            StepDelta::CodeExecutionCall {
                language,
                code,
                signature: f.optional("signature")?,
                arguments_extra,
                extra: f.extra(),
            }
        }
        "code_execution_result" => StepDelta::CodeExecutionResult {
            result: f.or_default("result")?,
            is_error: f.optional("is_error")?,
            signature: f.optional("signature")?,
            extra: f.extra(),
        },
        "url_context_call" => {
            let (urls, arguments_extra) = read_arguments(&mut f, |a| a.or_default("urls"))?;
            StepDelta::UrlContextCall {
                urls,
                signature: f.optional("signature")?,
                arguments_extra,
                extra: f.extra(),
            }
        }
        "url_context_result" => StepDelta::UrlContextResult {
            result: f.or_default("result")?,
            is_error: f.optional("is_error")?,
            signature: f.optional("signature")?,
            extra: f.extra(),
        },
        "google_search_call" => {
            let (queries, arguments_extra) = read_arguments(&mut f, |a| a.or_default("queries"))?;
            StepDelta::GoogleSearchCall {
                queries,
                signature: f.optional("signature")?,
                arguments_extra,
                extra: f.extra(),
            }
        }
        "google_search_result" => StepDelta::GoogleSearchResult {
            result: f.or_default("result")?,
            is_error: f.optional("is_error")?,
            signature: f.optional("signature")?,
            extra: f.extra(),
        },
        "mcp_server_tool_call" => StepDelta::McpServerToolCall {
            name: f.required("name")?,
            server_name: f.required("server_name")?,
            arguments: f.or_default("arguments")?,
            extra: f.extra(),
        },
        "mcp_server_tool_result" => StepDelta::McpServerToolResult {
            name: f.optional("name")?,
            server_name: f.optional("server_name")?,
            result: payload(&mut f)?,
            extra: f.extra(),
        },
        "file_search_call" => StepDelta::FileSearchCall {
            signature: f.optional("signature")?,
            extra: f.extra(),
        },
        "file_search_result" => StepDelta::FileSearchResult {
            result: f.or_default("result")?,
            signature: f.optional("signature")?,
            extra: f.extra(),
        },
        "google_maps_call" => {
            let (queries, arguments_extra) = read_arguments(&mut f, |a| a.or_default("queries"))?;
            StepDelta::GoogleMapsCall {
                queries,
                signature: f.optional("signature")?,
                arguments_extra,
                extra: f.extra(),
            }
        }
        "google_maps_result" => StepDelta::GoogleMapsResult {
            result: f.or_default("result")?,
            signature: f.optional("signature")?,
            extra: f.extra(),
        },
        "processing_call" => StepDelta::ProcessingCall {
            signature: f.optional("signature")?,
            extra: f.extra(),
        },
        "processing_result" => StepDelta::ProcessingResult {
            signature: f.optional("signature")?,
            extra: f.extra(),
        },
        "retrieval_call" => {
            let (queries, arguments_extra) = read_arguments(&mut f, |a| a.or_default("queries"))?;
            StepDelta::RetrievalCall {
                queries,
                retrieval_type: f.optional("retrieval_type")?,
                signature: f.optional("signature")?,
                arguments_extra,
                extra: f.extra(),
            }
        }
        "retrieval_result" => StepDelta::RetrievalResult {
            is_error: f.optional("is_error")?,
            signature: f.optional("signature")?,
            extra: f.extra(),
        },
        _ => return Err(Fallback::UnknownType),
    };
    Ok(delta)
}

impl<'de> Deserialize<'de> for StepDelta {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;

        match Fallback::read(&value, read_known) {
            Ok(delta) => Ok(delta),
            Err(fallback) => {
                let delta_type = value
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<missing type>")
                    .to_string();

                tracing::warn!(
                    "Keeping a delta whole in StepDelta::Unknown: {}.",
                    fallback.describe("StepDelta", &delta_type)
                );

                Ok(StepDelta::Unknown {
                    delta_type,
                    data: value,
                })
            }
        }
    }
}
