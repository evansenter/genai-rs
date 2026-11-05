//! Wire (de)serialization for [`Step`].
//!
//! Every known variant writes its modeled fields and then its `extra` map,
//! and reads with [`WireFields`], which leaves exactly the unread keys for
//! `extra` (#491). The five call steps that flatten a nested `arguments`
//! object treat its unread keys the same way, in `arguments_extra`.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::content::CodeExecutionLanguage;
use crate::wire_fields::{Fallback, ModeledMap, WireFields, merged_arguments};

use super::{FunctionResultPayload, Step};

/// `{"queries": [...]}` plus the extras, for the three query-shaped calls.
pub(super) fn queries_arguments(queries: &[String], extra: &Map<String, Value>) -> Value {
    Value::Object(merged_arguments(
        [("queries", Some(Value::from(queries)))],
        extra,
    ))
}

/// `{"urls": [...]}` plus the extras.
pub(super) fn urls_arguments(urls: &[String], extra: &Map<String, Value>) -> Value {
    Value::Object(merged_arguments([("urls", Some(Value::from(urls)))], extra))
}

/// `{"language": ..., "code": ...}` plus the extras. A `None` modeled value
/// (only a delta has one) is left out.
pub(super) fn code_arguments(
    language: Option<&CodeExecutionLanguage>,
    code: Option<&str>,
    extra: &Map<String, Value>,
) -> Value {
    Value::Object(merged_arguments(
        [
            ("language", language.map(|l| Value::from(l.to_string()))),
            ("code", code.map(Value::from)),
        ],
        extra,
    ))
}

/// Reads a `result` payload; absent or `null` is `Json(null)`.
pub(super) fn payload(f: &mut WireFields<'_>) -> Result<FunctionResultPayload, serde_json::Error> {
    Ok(f.optional("result")?
        .unwrap_or(FunctionResultPayload::Json(Value::Null)))
}

/// Reads a call step's nested `arguments` as `(modeled, arguments_extra)`.
/// Absent or `null` arguments read as the defaults.
pub(super) fn read_arguments<'a, T>(
    fields: &mut WireFields<'a>,
    read: impl FnOnce(&mut WireFields<'a>) -> Result<T, serde_json::Error>,
) -> Result<(T, Map<String, Value>), serde_json::Error>
where
    T: Default,
{
    match fields.object("arguments")? {
        Some(mut arguments) => Ok((read(&mut arguments)?, arguments.extra())),
        None => Ok((T::default(), Map::new())),
    }
}

impl Serialize for Step {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = ModeledMap::tagged(serializer, self.step_type())?;
        let extra = match self {
            Self::UserInput { content, extra } => {
                map.field("content", content)?;
                extra
            }
            Self::ModelOutput {
                content,
                error,
                extra,
            } => {
                map.field("content", content)?;
                map.opt("error", error.as_ref())?;
                extra
            }
            Self::Thought {
                signature,
                summary,
                extra,
            } => {
                map.opt("signature", signature.as_ref())?;
                map.field_unless(summary.is_empty(), "summary", summary)?;
                extra
            }
            Self::FunctionCall {
                id,
                name,
                arguments,
                signature,
                extra,
            } => {
                map.field("id", id)?;
                map.field("name", name)?;
                map.field("arguments", arguments)?;
                map.opt("signature", signature.as_ref())?;
                extra
            }
            Self::FunctionResult {
                call_id,
                name,
                result,
                is_error,
                signature,
                extra,
            } => {
                map.field("call_id", call_id)?;
                map.opt("name", name.as_ref())?;
                map.field("result", result)?;
                map.opt("is_error", is_error.as_ref())?;
                map.opt("signature", signature.as_ref())?;
                extra
            }
            Self::CodeExecutionCall {
                id,
                language,
                code,
                signature,
                arguments_extra,
                extra,
            } => {
                map.field("id", id)?;
                map.field(
                    "arguments",
                    &code_arguments(Some(language), Some(code), arguments_extra),
                )?;
                map.opt("signature", signature.as_ref())?;
                extra
            }
            Self::CodeExecutionResult {
                call_id,
                result,
                is_error,
                signature,
                extra,
            } => {
                map.field("call_id", call_id)?;
                map.field("result", result)?;
                map.field("is_error", is_error)?;
                map.opt("signature", signature.as_ref())?;
                extra
            }
            Self::UrlContextCall {
                id,
                urls,
                signature,
                arguments_extra,
                extra,
            } => {
                map.field("id", id)?;
                map.field("arguments", &urls_arguments(urls, arguments_extra))?;
                map.opt("signature", signature.as_ref())?;
                extra
            }
            Self::UrlContextResult {
                call_id,
                result,
                is_error,
                signature,
                extra,
            } => {
                map.field("call_id", call_id)?;
                map.field("result", result)?;
                map.opt("is_error", is_error.as_ref())?;
                map.opt("signature", signature.as_ref())?;
                extra
            }
            Self::GoogleSearchCall {
                id,
                queries,
                search_type,
                signature,
                arguments_extra,
                extra,
            } => {
                map.field("id", id)?;
                map.field("arguments", &queries_arguments(queries, arguments_extra))?;
                map.opt("search_type", search_type.as_ref())?;
                map.opt("signature", signature.as_ref())?;
                extra
            }
            Self::GoogleSearchResult {
                call_id,
                result,
                is_error,
                signature,
                extra,
            } => {
                map.field("call_id", call_id)?;
                map.field("result", result)?;
                map.opt("is_error", is_error.as_ref())?;
                map.opt("signature", signature.as_ref())?;
                extra
            }
            Self::ToolCall {
                id,
                signature,
                extra,
            }
            | Self::FileSearchCall {
                id,
                signature,
                extra,
            }
            | Self::ProcessingCall {
                id,
                signature,
                extra,
            } => {
                map.field("id", id)?;
                map.opt("signature", signature.as_ref())?;
                extra
            }
            Self::McpServerToolCall {
                id,
                name,
                server_name,
                arguments,
                extra,
            } => {
                map.field("id", id)?;
                map.field("name", name)?;
                map.field("server_name", server_name)?;
                map.field("arguments", arguments)?;
                extra
            }
            Self::McpServerToolResult {
                call_id,
                name,
                server_name,
                result,
                extra,
            } => {
                map.field("call_id", call_id)?;
                map.opt("name", name.as_ref())?;
                map.opt("server_name", server_name.as_ref())?;
                map.field("result", result)?;
                extra
            }
            Self::FileSearchResult {
                call_id,
                signature,
                extra,
            } => {
                map.field("call_id", call_id)?;
                map.opt("signature", signature.as_ref())?;
                extra
            }
            Self::GoogleMapsCall {
                id,
                queries,
                signature,
                arguments_extra,
                extra,
            } => {
                map.field("id", id)?;
                map.field("arguments", &queries_arguments(queries, arguments_extra))?;
                map.opt("signature", signature.as_ref())?;
                extra
            }
            Self::GoogleMapsResult {
                call_id,
                result,
                signature,
                extra,
            } => {
                map.field("call_id", call_id)?;
                map.field("result", result)?;
                map.opt("signature", signature.as_ref())?;
                extra
            }
            Self::ProcessingResult {
                call_id,
                signature,
                extra,
            } => {
                map.field("call_id", call_id)?;
                map.opt("signature", signature.as_ref())?;
                extra
            }
            Self::RetrievalCall {
                id,
                queries,
                retrieval_type,
                signature,
                arguments_extra,
                extra,
            } => {
                map.field("id", id)?;
                map.field("arguments", &queries_arguments(queries, arguments_extra))?;
                map.opt("retrieval_type", retrieval_type.as_ref())?;
                map.opt("signature", signature.as_ref())?;
                extra
            }
            Self::RetrievalResult {
                call_id,
                is_error,
                signature,
                extra,
            } => {
                map.field("call_id", call_id)?;
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

/// An `Unknown` step's `data` as the entries to write after its `type`: an
/// object's own entries, or a non-object value under `"data"` (nothing for
/// `null`).
pub(super) fn unknown_fields(data: &Value) -> Map<String, Value> {
    match data {
        Value::Object(object) => object.clone(),
        Value::Null => Map::new(),
        other => Map::from_iter([("data".to_string(), other.clone())]),
    }
}

/// Reads a known step from its wire object. An error means the type is
/// unrecognized or a modeled field is malformed; the caller keeps the object
/// as [`Step::Unknown`].
fn read_known(object: &Map<String, Value>) -> Result<Step, Fallback> {
    let mut f = WireFields::tagged(object);
    let Some(step_type) = f.tag() else {
        return Err(Fallback::UnknownType);
    };
    let step = match step_type {
        "user_input" => Step::UserInput {
            content: f.or_default("content")?,
            extra: f.extra(),
        },
        "model_output" => Step::ModelOutput {
            content: f.or_default("content")?,
            error: f.optional("error")?,
            extra: f.extra(),
        },
        "thought" => Step::Thought {
            signature: f.optional("signature")?,
            summary: f.or_default("summary")?,
            extra: f.extra(),
        },
        "function_call" => Step::FunctionCall {
            id: f.required("id")?,
            name: f.required("name")?,
            arguments: f.or_default("arguments")?,
            signature: f.optional("signature")?,
            extra: f.extra(),
        },
        "function_result" => Step::FunctionResult {
            call_id: f.required("call_id")?,
            name: f.optional("name")?,
            result: payload(&mut f)?,
            is_error: f.optional("is_error")?,
            signature: f.optional("signature")?,
            extra: f.extra(),
        },
        "code_execution_call" => {
            let id = f.required("id")?;
            let ((language, code), arguments_extra) = read_arguments(&mut f, |a| {
                Ok((a.or_default("language")?, a.or_default("code")?))
            })?;
            Step::CodeExecutionCall {
                id,
                language,
                code,
                signature: f.optional("signature")?,
                arguments_extra,
                extra: f.extra(),
            }
        }
        "code_execution_result" => Step::CodeExecutionResult {
            call_id: f.required("call_id")?,
            result: f.or_default("result")?,
            is_error: f.or_default("is_error")?,
            signature: f.optional("signature")?,
            extra: f.extra(),
        },
        "url_context_call" => {
            let id = f.required("id")?;
            let (urls, arguments_extra) = read_arguments(&mut f, |a| a.or_default("urls"))?;
            Step::UrlContextCall {
                id,
                urls,
                signature: f.optional("signature")?,
                arguments_extra,
                extra: f.extra(),
            }
        }
        "url_context_result" => Step::UrlContextResult {
            call_id: f.required("call_id")?,
            result: f.or_default("result")?,
            is_error: f.optional("is_error")?,
            signature: f.optional("signature")?,
            extra: f.extra(),
        },
        "google_search_call" => {
            let id = f.required("id")?;
            let (queries, arguments_extra) = read_arguments(&mut f, |a| a.or_default("queries"))?;
            Step::GoogleSearchCall {
                id,
                queries,
                search_type: f.optional("search_type")?,
                signature: f.optional("signature")?,
                arguments_extra,
                extra: f.extra(),
            }
        }
        "google_search_result" => Step::GoogleSearchResult {
            call_id: f.required("call_id")?,
            result: f.or_default("result")?,
            is_error: f.optional("is_error")?,
            signature: f.optional("signature")?,
            extra: f.extra(),
        },
        "tool_call" => Step::ToolCall {
            id: f.required("id")?,
            signature: f.optional("signature")?,
            extra: f.extra(),
        },
        "mcp_server_tool_call" => Step::McpServerToolCall {
            id: f.required("id")?,
            name: f.required("name")?,
            server_name: f.required("server_name")?,
            arguments: f.or_default("arguments")?,
            extra: f.extra(),
        },
        "mcp_server_tool_result" => Step::McpServerToolResult {
            call_id: f.required("call_id")?,
            name: f.optional("name")?,
            server_name: f.optional("server_name")?,
            result: payload(&mut f)?,
            extra: f.extra(),
        },
        "file_search_call" => Step::FileSearchCall {
            id: f.required("id")?,
            signature: f.optional("signature")?,
            extra: f.extra(),
        },
        "file_search_result" => Step::FileSearchResult {
            call_id: f.required("call_id")?,
            signature: f.optional("signature")?,
            extra: f.extra(),
        },
        "google_maps_call" => {
            let id = f.required("id")?;
            let (queries, arguments_extra) = read_arguments(&mut f, |a| a.or_default("queries"))?;
            Step::GoogleMapsCall {
                id,
                queries,
                signature: f.optional("signature")?,
                arguments_extra,
                extra: f.extra(),
            }
        }
        "google_maps_result" => Step::GoogleMapsResult {
            call_id: f.required("call_id")?,
            result: f.or_default("result")?,
            signature: f.optional("signature")?,
            extra: f.extra(),
        },
        "processing_call" => Step::ProcessingCall {
            id: f.required("id")?,
            signature: f.optional("signature")?,
            extra: f.extra(),
        },
        "processing_result" => Step::ProcessingResult {
            call_id: f.required("call_id")?,
            signature: f.optional("signature")?,
            extra: f.extra(),
        },
        "retrieval_call" => {
            let id = f.required("id")?;
            let (queries, arguments_extra) = read_arguments(&mut f, |a| a.or_default("queries"))?;
            Step::RetrievalCall {
                id,
                queries,
                retrieval_type: f.optional("retrieval_type")?,
                signature: f.optional("signature")?,
                arguments_extra,
                extra: f.extra(),
            }
        }
        "retrieval_result" => Step::RetrievalResult {
            call_id: f.required("call_id")?,
            is_error: f.optional("is_error")?,
            signature: f.optional("signature")?,
            extra: f.extra(),
        },
        _ => return Err(Fallback::UnknownType),
    };
    Ok(step)
}

impl<'de> Deserialize<'de> for Step {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[cfg(feature = "strict-unknown")]
        use serde::de::Error as _;

        let value = Value::deserialize(deserializer)?;

        match Fallback::read(&value, read_known) {
            Ok(step) => Ok(step),
            Err(fallback) => {
                let step_type = value
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<missing type>")
                    .to_string();

                // An unknown type and a known type with a field the crate
                // cannot read both keep the whole object; the cause says
                // which.
                #[cfg(feature = "strict-unknown")]
                {
                    Err(D::Error::custom(
                        fallback.strict_message("Step", &step_type),
                    ))
                }

                #[cfg(not(feature = "strict-unknown"))]
                {
                    tracing::warn!(
                        "Keeping a step whole in Step::Unknown: {}. \
                         This may indicate a new API feature or a malformed response.",
                        fallback.describe("Step", &step_type)
                    );
                    Ok(Step::Unknown {
                        step_type,
                        data: value,
                    })
                }
            }
        }
    }
}
