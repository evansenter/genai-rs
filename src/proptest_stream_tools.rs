//! The stream forms of server-side tool calls and results, for
//! `src/proptest_stream.rs`: code execution, URL context, Google Search and
//! Maps, retrieval, File Search, MCP, processing, the generic `tool_call`
//! and, outside strict mode, an unknown step's signature.

use super::super::content::{
    arb_code_execution_language, arb_google_maps_result_item, arb_google_search_result_item,
    arb_url_context_result_item,
};
use super::super::steps::arb_function_result_payload;
use super::super::tools::{arb_retrieval_type, arb_search_type};
use super::*;

pub(super) fn arb_code_execution_call() -> impl Strategy<Value = Streamed> {
    (
        arb_identifier(),
        arb_code_execution_language(),
        arb_text(),
        proptest::option::of(arb_text()),
        arb_extra(),
        arb_extra(),
        arb_picks(),
    )
        .prop_map(
            |(id, language, code, signature, arguments_extra, extra, mut picks)| {
                let pieces = split_text(&code, &mut picks);
                let n = pieces.len();
                let sigs = replacing(signature.as_deref(), n, &mut picks);
                let mut args = spread(&arguments_extra, n + 1, &mut picks).into_iter();
                let mut extras = spread(&extra, n + 1, &mut picks).into_iter();
                let start = Step::CodeExecutionCall {
                    id: id.clone(),
                    language: language.clone(),
                    code: String::new(),
                    signature: announced(signature.as_deref(), &mut picks),
                    arguments_extra: args.next().unwrap_or_default(),
                    extra: extras.next().unwrap_or_default(),
                };
                let deltas = pieces
                    .into_iter()
                    .zip(sigs)
                    .map(|(code, signature)| StepDelta::CodeExecutionCall {
                        language: picks.coin().then(|| language.clone()),
                        code: Some(code),
                        signature,
                        arguments_extra: args.next().unwrap_or_default(),
                        extra: extras.next().unwrap_or_default(),
                    })
                    .collect();
                Streamed {
                    expected: Step::CodeExecutionCall {
                        id,
                        language,
                        code,
                        signature,
                        arguments_extra,
                        extra,
                    },
                    start: Some(start),
                    deltas,
                }
            },
        )
}

pub(super) fn arb_code_execution_result() -> impl Strategy<Value = Streamed> {
    (
        arb_identifier(),
        arb_text(),
        any::<bool>(),
        proptest::option::of(arb_text()),
        arb_extra(),
        arb_picks(),
    )
        .prop_map(|(call_id, result, is_error, signature, extra, mut picks)| {
            let pieces = split_text(&result, &mut picks);
            let n = pieces.len();
            let sigs = replacing(signature.as_deref(), n, &mut picks);
            let carrier = picks.pick(n);
            let mut extras = spread(&extra, n + 1, &mut picks).into_iter();
            let start = Step::CodeExecutionResult {
                call_id: call_id.clone(),
                result: CodeExecutionOutput::Text(String::new()),
                is_error: false,
                signature: announced(signature.as_deref(), &mut picks),
                extra: extras.next().unwrap_or_default(),
            };
            let deltas = pieces
                .into_iter()
                .zip(sigs)
                .enumerate()
                .map(|(i, (result, signature))| StepDelta::CodeExecutionResult {
                    result,
                    is_error: once(Some(&is_error), i, carrier),
                    signature,
                    extra: extras.next().unwrap_or_default(),
                })
                .collect();
            Streamed {
                expected: Step::CodeExecutionResult {
                    call_id,
                    result: CodeExecutionOutput::Text(result),
                    is_error,
                    signature,
                    extra,
                },
                start: Some(start),
                deltas,
            }
        })
}

/// What the three search-like calls share: the id, the items that stream,
/// the signature and both extras.
type CallParts = (
    String,
    Vec<String>,
    Option<String>,
    Map<String, Value>,
    Map<String, Value>,
);

fn arb_call_parts() -> impl Strategy<Value = CallParts> {
    (
        arb_identifier(),
        prop::collection::vec(arb_text(), 0..4),
        proptest::option::of(arb_text()),
        arb_extra(),
        arb_extra(),
    )
}

/// The start fields and per-delta fields of a search-like call:
/// `(signature, arguments_extra, extra)` on the start, then each delta's
/// `(items, signature, arguments_extra, extra)`.
#[allow(clippy::type_complexity, reason = "a test's local plumbing")]
fn call_events(
    parts: &CallParts,
    picks: &mut Picks,
) -> (
    (Option<String>, Map<String, Value>, Map<String, Value>),
    Vec<(
        Vec<String>,
        Option<String>,
        Map<String, Value>,
        Map<String, Value>,
    )>,
) {
    let (_, items, signature, arguments_extra, extra) = parts;
    let chunks = split_items(items, picks);
    let n = chunks.len();
    let sigs = replacing(signature.as_deref(), n, picks);
    let mut args = spread(arguments_extra, n + 1, picks).into_iter();
    let mut extras = spread(extra, n + 1, picks).into_iter();
    let start = (
        announced(signature.as_deref(), picks),
        args.next().unwrap_or_default(),
        extras.next().unwrap_or_default(),
    );
    let deltas = chunks
        .into_iter()
        .zip(sigs)
        .map(|(items, signature)| {
            (
                items,
                signature,
                args.next().unwrap_or_default(),
                extras.next().unwrap_or_default(),
            )
        })
        .collect();
    (start, deltas)
}

pub(super) fn arb_url_context_call() -> impl Strategy<Value = Streamed> {
    (arb_call_parts(), arb_picks()).prop_map(|(parts, mut picks)| {
        let ((signature, arguments_extra, extra), events) = call_events(&parts, &mut picks);
        let (id, urls, expected_signature, expected_args, expected_extra) = parts;
        Streamed {
            expected: Step::UrlContextCall {
                id: id.clone(),
                urls,
                signature: expected_signature,
                arguments_extra: expected_args,
                extra: expected_extra,
            },
            start: Some(Step::UrlContextCall {
                id,
                urls: Vec::new(),
                signature,
                arguments_extra,
                extra,
            }),
            deltas: events
                .into_iter()
                .map(
                    |(urls, signature, arguments_extra, extra)| StepDelta::UrlContextCall {
                        urls,
                        signature,
                        arguments_extra,
                        extra,
                    },
                )
                .collect(),
        }
    })
}

pub(super) fn arb_google_search_call() -> impl Strategy<Value = Streamed> {
    (
        arb_call_parts(),
        proptest::option::of(arb_search_type()),
        arb_picks(),
    )
        .prop_map(|(parts, search_type, mut picks)| {
            let ((signature, arguments_extra, extra), events) = call_events(&parts, &mut picks);
            let (id, queries, expected_signature, expected_args, expected_extra) = parts;
            Streamed {
                expected: Step::GoogleSearchCall {
                    id: id.clone(),
                    queries,
                    search_type: search_type.clone(),
                    signature: expected_signature,
                    arguments_extra: expected_args,
                    extra: expected_extra,
                },
                // The search type has no delta field: step.start brings it.
                start: Some(Step::GoogleSearchCall {
                    id,
                    queries: Vec::new(),
                    search_type,
                    signature,
                    arguments_extra,
                    extra,
                }),
                deltas: events
                    .into_iter()
                    .map(|(queries, signature, arguments_extra, extra)| {
                        StepDelta::GoogleSearchCall {
                            queries,
                            signature,
                            arguments_extra,
                            extra,
                        }
                    })
                    .collect(),
            }
        })
}

pub(super) fn arb_google_maps_call() -> impl Strategy<Value = Streamed> {
    (arb_call_parts(), arb_picks()).prop_map(|(parts, mut picks)| {
        let ((signature, arguments_extra, extra), events) = call_events(&parts, &mut picks);
        let (id, queries, expected_signature, expected_args, expected_extra) = parts;
        Streamed {
            expected: Step::GoogleMapsCall {
                id: id.clone(),
                queries,
                signature: expected_signature,
                arguments_extra: expected_args,
                extra: expected_extra,
            },
            start: Some(Step::GoogleMapsCall {
                id,
                queries: Vec::new(),
                signature,
                arguments_extra,
                extra,
            }),
            deltas: events
                .into_iter()
                .map(
                    |(queries, signature, arguments_extra, extra)| StepDelta::GoogleMapsCall {
                        queries,
                        signature,
                        arguments_extra,
                        extra,
                    },
                )
                .collect(),
        }
    })
}

pub(super) fn arb_retrieval_call() -> impl Strategy<Value = Streamed> {
    (
        arb_call_parts(),
        proptest::option::of(arb_retrieval_type()),
        arb_picks(),
    )
        .prop_map(|(parts, retrieval_type, mut picks)| {
            let (id, queries, signature, arguments_extra, extra) = parts;
            // The signature extends piece by piece here, rather than being
            // replaced.
            let pieces = fragments(signature.as_deref(), &mut picks);
            let mut chunks = split_items(&queries, &mut picks);
            chunks.resize(chunks.len().max(pieces.len()), Vec::new());
            let n = chunks.len();
            // The retrieval type comes on the start or on one delta.
            let typed_start = picks.coin();
            let carrier = if typed_start { n } else { picks.pick(n) };
            let mut args = spread(&arguments_extra, n + 1, &mut picks).into_iter();
            let mut extras = spread(&extra, n + 1, &mut picks).into_iter();
            let start = Step::RetrievalCall {
                id: id.clone(),
                queries: Vec::new(),
                retrieval_type: retrieval_type.clone().filter(|_| typed_start),
                signature: announced(signature.as_deref(), &mut picks),
                arguments_extra: args.next().unwrap_or_default(),
                extra: extras.next().unwrap_or_default(),
            };
            let deltas = chunks
                .into_iter()
                .enumerate()
                .map(|(i, queries)| StepDelta::RetrievalCall {
                    queries,
                    retrieval_type: once(retrieval_type.as_ref(), i, carrier),
                    signature: fragment(&pieces, i, &mut picks),
                    arguments_extra: args.next().unwrap_or_default(),
                    extra: extras.next().unwrap_or_default(),
                })
                .collect();
            Streamed {
                expected: Step::RetrievalCall {
                    id,
                    queries,
                    retrieval_type,
                    signature,
                    arguments_extra,
                    extra,
                },
                start: Some(start),
                deltas,
            }
        })
}

/// What the results that stream a list share: the call id, the
/// `is_error` flag where the step has one, the signature and the extra.
fn arb_result_parts()
-> impl Strategy<Value = (String, Option<bool>, Option<String>, Map<String, Value>)> {
    (
        arb_identifier(),
        proptest::option::of(any::<bool>()),
        proptest::option::of(arb_text()),
        arb_extra(),
    )
}

/// Per-delta `(items, is_error, signature, extra)` for a result whose list
/// streams in chunks, and the start's `(signature, extra)`.
#[allow(clippy::type_complexity, reason = "a test's local plumbing")]
fn result_events<T: Clone>(
    items: &[T],
    is_error: Option<bool>,
    signature: Option<&str>,
    extra: &Map<String, Value>,
    picks: &mut Picks,
) -> (
    (Option<String>, Map<String, Value>),
    Vec<(Vec<T>, Option<bool>, Option<String>, Map<String, Value>)>,
) {
    let chunks = split_items(items, picks);
    let n = chunks.len();
    let sigs = replacing(signature, n, picks);
    let carrier = picks.pick(n);
    let mut extras = spread(extra, n + 1, picks).into_iter();
    let start = (
        announced(signature, picks),
        extras.next().unwrap_or_default(),
    );
    let deltas = chunks
        .into_iter()
        .zip(sigs)
        .enumerate()
        .map(|(i, (items, signature))| {
            (
                items,
                once(is_error.as_ref(), i, carrier),
                signature,
                extras.next().unwrap_or_default(),
            )
        })
        .collect();
    (start, deltas)
}

pub(super) fn arb_url_context_result() -> impl Strategy<Value = Streamed> {
    (
        arb_result_parts(),
        prop::collection::vec(arb_url_context_result_item(), 0..4),
        arb_picks(),
    )
        .prop_map(
            |((call_id, is_error, signature, extra), result, mut picks)| {
                let ((start_signature, start_extra), events) =
                    result_events(&result, is_error, signature.as_deref(), &extra, &mut picks);
                Streamed {
                    expected: Step::UrlContextResult {
                        call_id: call_id.clone(),
                        result,
                        is_error,
                        signature,
                        extra,
                    },
                    start: Some(Step::UrlContextResult {
                        call_id,
                        result: Vec::new(),
                        is_error: None,
                        signature: start_signature,
                        extra: start_extra,
                    }),
                    deltas: events
                        .into_iter()
                        .map(
                            |(result, is_error, signature, extra)| StepDelta::UrlContextResult {
                                result,
                                is_error,
                                signature,
                                extra,
                            },
                        )
                        .collect(),
                }
            },
        )
}

pub(super) fn arb_google_search_result() -> impl Strategy<Value = Streamed> {
    (
        arb_result_parts(),
        prop::collection::vec(arb_google_search_result_item(), 0..4),
        arb_picks(),
    )
        .prop_map(
            |((call_id, is_error, signature, extra), result, mut picks)| {
                let ((start_signature, start_extra), events) =
                    result_events(&result, is_error, signature.as_deref(), &extra, &mut picks);
                Streamed {
                    expected: Step::GoogleSearchResult {
                        call_id: call_id.clone(),
                        result,
                        is_error,
                        signature,
                        extra,
                    },
                    start: Some(Step::GoogleSearchResult {
                        call_id,
                        result: Vec::new(),
                        is_error: None,
                        signature: start_signature,
                        extra: start_extra,
                    }),
                    deltas: events
                        .into_iter()
                        .map(
                            |(result, is_error, signature, extra)| StepDelta::GoogleSearchResult {
                                result,
                                is_error,
                                signature,
                                extra,
                            },
                        )
                        .collect(),
                }
            },
        )
}

pub(super) fn arb_google_maps_result() -> impl Strategy<Value = Streamed> {
    (
        arb_result_parts(),
        prop::collection::vec(arb_google_maps_result_item(), 0..3),
        arb_picks(),
    )
        .prop_map(|((call_id, _, signature, extra), result, mut picks)| {
            let ((start_signature, start_extra), events) =
                result_events(&result, None, signature.as_deref(), &extra, &mut picks);
            Streamed {
                expected: Step::GoogleMapsResult {
                    call_id: call_id.clone(),
                    result,
                    signature,
                    extra,
                },
                start: Some(Step::GoogleMapsResult {
                    call_id,
                    result: Vec::new(),
                    signature: start_signature,
                    extra: start_extra,
                }),
                deltas: events
                    .into_iter()
                    .map(
                        |(result, _, signature, extra)| StepDelta::GoogleMapsResult {
                            result,
                            signature,
                            extra,
                        },
                    )
                    .collect(),
            }
        })
}

/// A File Search result keeps the results a stream sends in its `extra`,
/// under `result`: the step has no such field.
pub(super) fn arb_file_search_result() -> impl Strategy<Value = Streamed> {
    (
        arb_result_parts(),
        prop::collection::vec(arb_unknown_object(), 0..3),
        arb_picks(),
    )
        .prop_map(|((call_id, _, signature, extra), result, mut picks)| {
            let ((start_signature, start_extra), events) =
                result_events(&result, None, signature.as_deref(), &extra, &mut picks);
            let mut expected_extra = extra;
            if !result.is_empty() {
                expected_extra.insert("result".into(), Value::Array(result));
            }
            Streamed {
                expected: Step::FileSearchResult {
                    call_id: call_id.clone(),
                    signature,
                    extra: expected_extra,
                },
                start: Some(Step::FileSearchResult {
                    call_id,
                    signature: start_signature,
                    extra: start_extra,
                }),
                deltas: events
                    .into_iter()
                    .map(
                        |(result, _, signature, extra)| StepDelta::FileSearchResult {
                            result,
                            signature,
                            extra,
                        },
                    )
                    .collect(),
            }
        })
}

pub(super) fn arb_file_search_call() -> impl Strategy<Value = Streamed> {
    (
        arb_identifier(),
        proptest::option::of(arb_text()),
        arb_extra(),
        arb_picks(),
    )
        .prop_map(|(id, signature, extra, mut picks)| {
            let n = 1 + picks.pick(2);
            let sigs = replacing(signature.as_deref(), n, &mut picks);
            let mut extras = spread(&extra, n + 1, &mut picks).into_iter();
            let start = Step::FileSearchCall {
                id: id.clone(),
                signature: announced(signature.as_deref(), &mut picks),
                extra: extras.next().unwrap_or_default(),
            };
            let deltas = sigs
                .into_iter()
                .map(|signature| StepDelta::FileSearchCall {
                    signature,
                    extra: extras.next().unwrap_or_default(),
                })
                .collect();
            Streamed {
                expected: Step::FileSearchCall {
                    id,
                    signature,
                    extra,
                },
                start: Some(start),
                deltas,
            }
        })
}

pub(super) fn arb_mcp_server_tool_call() -> impl Strategy<Value = Streamed> {
    (
        arb_identifier(),
        arb_identifier(),
        arb_identifier(),
        arb_json_value(),
        arb_extra(),
        arb_picks(),
    )
        .prop_map(|(id, name, server_name, arguments, extra, mut picks)| {
            // Each delta carries the whole call; the last one stands.
            let n = 1 + picks.pick(2);
            let mut extras = spread(&extra, n + 1, &mut picks).into_iter();
            let start = Step::McpServerToolCall {
                id: id.clone(),
                name: String::new(),
                server_name: String::new(),
                arguments: json!({}),
                extra: extras.next().unwrap_or_default(),
            };
            let deltas = (0..n)
                .map(|_| StepDelta::McpServerToolCall {
                    name: name.clone(),
                    server_name: server_name.clone(),
                    arguments: arguments.clone(),
                    extra: extras.next().unwrap_or_default(),
                })
                .collect();
            Streamed {
                expected: Step::McpServerToolCall {
                    id,
                    name,
                    server_name,
                    arguments,
                    extra,
                },
                start: Some(start),
                deltas,
            }
        })
}

pub(super) fn arb_mcp_server_tool_result() -> impl Strategy<Value = Streamed> {
    (
        arb_identifier(),
        proptest::option::of(arb_identifier()),
        proptest::option::of(arb_identifier()),
        arb_function_result_payload(),
        arb_extra(),
        arb_picks(),
    )
        .prop_map(|(call_id, name, server_name, result, extra, mut picks)| {
            // Each name comes on the start or on the delta; a delta without
            // one keeps the start's.
            let (name_first, server_first) = (picks.coin(), picks.coin());
            let mut extras = spread(&extra, 2, &mut picks).into_iter();
            let start = Step::McpServerToolResult {
                call_id: call_id.clone(),
                name: name.clone().filter(|_| name_first),
                server_name: server_name.clone().filter(|_| server_first),
                result: FunctionResultPayload::Text(String::new()),
                extra: extras.next().unwrap_or_default(),
            };
            let delta = StepDelta::McpServerToolResult {
                name: name.clone().filter(|_| !name_first),
                server_name: server_name.clone().filter(|_| !server_first),
                result: result.clone(),
                extra: extras.next().unwrap_or_default(),
            };
            Streamed {
                expected: Step::McpServerToolResult {
                    call_id,
                    name,
                    server_name,
                    result,
                    extra,
                },
                start: Some(start),
                deltas: vec![delta],
            }
        })
}

/// `processing_call`, `processing_result` and `retrieval_result`: a
/// signature that extends piece by piece (`retrieval_result` also carries
/// `is_error`).
pub(super) fn arb_signature_step() -> impl Strategy<Value = Streamed> {
    (
        arb_identifier(),
        proptest::option::of(arb_text()),
        proptest::option::of(any::<bool>()),
        arb_extra(),
        0..3usize,
        arb_picks(),
    )
        .prop_map(|(id, signature, is_error, extra, kind, mut picks)| {
            let pieces = fragments(signature.as_deref(), &mut picks);
            let n = pieces.len().max(1);
            let carrier = picks.pick(n);
            let mut extras = spread(&extra, n + 1, &mut picks).into_iter();
            let start_signature = announced(signature.as_deref(), &mut picks);
            let start_extra = extras.next().unwrap_or_default();
            let sigs: Vec<Option<String>> =
                (0..n).map(|i| fragment(&pieces, i, &mut picks)).collect();
            let mut extras: Vec<_> = extras.collect();
            extras.resize(n, Map::new());
            match kind {
                0 => Streamed {
                    expected: Step::ProcessingCall {
                        id: id.clone(),
                        signature,
                        extra,
                    },
                    start: Some(Step::ProcessingCall {
                        id,
                        signature: start_signature,
                        extra: start_extra,
                    }),
                    deltas: sigs
                        .into_iter()
                        .zip(extras)
                        .map(|(signature, extra)| StepDelta::ProcessingCall { signature, extra })
                        .collect(),
                },
                1 => Streamed {
                    expected: Step::ProcessingResult {
                        call_id: id.clone(),
                        signature,
                        extra,
                    },
                    start: Some(Step::ProcessingResult {
                        call_id: id,
                        signature: start_signature,
                        extra: start_extra,
                    }),
                    deltas: sigs
                        .into_iter()
                        .zip(extras)
                        .map(|(signature, extra)| StepDelta::ProcessingResult { signature, extra })
                        .collect(),
                },
                _ => Streamed {
                    expected: Step::RetrievalResult {
                        call_id: id.clone(),
                        is_error,
                        signature,
                        extra,
                    },
                    start: Some(Step::RetrievalResult {
                        call_id: id,
                        is_error: None,
                        signature: start_signature,
                        extra: start_extra,
                    }),
                    deltas: sigs
                        .into_iter()
                        .zip(extras)
                        .enumerate()
                        .map(|(i, (signature, extra))| StepDelta::RetrievalResult {
                            is_error: once(is_error.as_ref(), i, carrier),
                            signature,
                            extra,
                        })
                        .collect(),
                },
            }
        })
}

/// A step with no delta form arrives whole in its `step.start`.
pub(super) fn arb_tool_call() -> impl Strategy<Value = Streamed> {
    (
        arb_identifier(),
        proptest::option::of(arb_text()),
        arb_extra(),
    )
        .prop_map(|(id, signature, extra)| {
            let step = Step::ToolCall {
                id,
                signature,
                extra,
            };
            Streamed {
                expected: step.clone(),
                start: Some(step),
                deltas: Vec::new(),
            }
        })
}

/// An unknown step whose signature streams in same-typed unknown deltas:
/// the one field the accumulator carries onto it (strict mode rejects
/// unknown steps, so it draws none).
#[cfg(not(genai_strict_unknown))]
pub(super) fn arb_unknown_step() -> impl Strategy<Value = Streamed> {
    (
        arb_unknown_type(),
        arb_unknown_object(),
        ".{1,40}",
        arb_picks(),
    )
        .prop_map(|(step_type, data, signature, mut picks)| {
            let mut fields = data.as_object().cloned().unwrap_or_default();
            fields.retain(|key, _| key != "type" && key != "signature");
            fields.insert("type".into(), Value::from(step_type.clone()));
            let mut start = fields.clone();
            if picks.coin() {
                start.insert("signature".into(), Value::from(""));
            }
            let mut expected = fields;
            expected.insert("signature".into(), Value::from(signature.clone()));
            let unknown = |fields: Map<String, Value>| {
                serde_json::from_value::<Step>(Value::Object(fields)).expect("an unknown step")
            };
            let deltas = split_text(&signature, &mut picks)
                .into_iter()
                .map(|piece| {
                    serde_json::from_value::<StepDelta>(
                        json!({"type": step_type, "signature": piece}),
                    )
                    .expect("an unknown delta")
                })
                .collect();
            Streamed {
                expected: unknown(expected),
                start: Some(unknown(start)),
                deltas,
            }
        })
}
