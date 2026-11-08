//! Property tests: a streamed step accumulates to the step a non-streamed
//! response carries.
//!
//! Each case draws steps and writes each one as a stream delivers it: a
//! `step.start` with the parts that grow left empty, those parts split over
//! `step.delta` events, then `step.stop`. The events go through
//! `dispatch_stream_event` as JSON, and the steps the completed response
//! carries must equal the drawn ones, compared as JSON. Every step with a
//! delta form is drawn, so a stream accumulator arm that drops or garbles
//! what its delta carries fails here: before this test, 7 of 9 disabled
//! arms left every offline test green (#504).
//!
//! A drawn step is one the stream can build: neighbouring content blocks
//! are of different kinds (a delta continues the block before it when that
//! is its kind), and media data is base64 in whole quanta, as every stream
//! observed has sent it (`docs/ENUM_WIRE_FORMATS.md`).

use super::content::{
    arb_annotation, arb_code_execution_language, arb_content, arb_google_maps_result_item,
    arb_google_search_result_item, arb_resolution, arb_url_context_result_item,
};
use super::steps::{arb_function_result_payload, arb_step_error};
use super::tools::{arb_retrieval_type, arb_search_type};
use super::*;
use crate::http::interactions::dispatch_stream_event;
use crate::steps::StepAccumulator;
use crate::wire_streaming::InteractionStreamEvent;
use serde_json::{Map, Value, json};

// =============================================================================
// Drawing a stream
// =============================================================================

/// A step, and the events that deliver it: an optional `step.start` and
/// the deltas after it (`step.stop` follows them).
#[derive(Clone, Debug)]
struct Streamed {
    expected: Step,
    start: Option<Step>,
    deltas: Vec<StepDelta>,
}

/// Choices drawn with a step and used in order: where to split a value
/// over deltas, and which event carries a field. They shrink toward 0, the
/// plainest stream: one delta, with everything on the first event.
#[derive(Clone, Debug)]
struct Picks {
    values: Vec<u8>,
    used: usize,
}

impl Picks {
    /// A choice in `0..n`; always 0 when `n` is 0 or 1.
    fn pick(&mut self, n: usize) -> usize {
        if n <= 1 || self.values.is_empty() {
            return 0;
        }
        let value = self.values[self.used % self.values.len()];
        self.used += 1;
        usize::from(value) % n
    }

    fn coin(&mut self) -> bool {
        self.pick(2) == 1
    }
}

fn arb_picks() -> impl Strategy<Value = Picks> {
    prop::collection::vec(any::<u8>(), 1..32).prop_map(|values| Picks { values, used: 0 })
}

/// Unmodeled fields: `zz_` keys, so none names a modeled field.
fn arb_extra() -> impl Strategy<Value = Map<String, Value>> {
    prop::collection::btree_map("zz_[a-z]{1,6}", ".{0,8}", 0..3).prop_map(|fields| {
        fields
            .into_iter()
            .map(|(k, v)| (k, Value::from(v)))
            .collect()
    })
}

/// Base64 text in whole quanta, the last one possibly padded.
fn arb_base64() -> impl Strategy<Value = String> {
    (
        prop::collection::vec("[A-Za-z0-9+/]{4}", 0..6),
        prop_oneof![Just(""), Just("QQ=="), Just("QUI=")],
    )
        .prop_map(|(quanta, last)| quanta.concat() + last)
}

/// `text` cut at up to two of the `at` offsets, into non-empty pieces (one
/// empty piece for empty text).
fn split_at(text: &str, at: &[usize], picks: &mut Picks) -> Vec<String> {
    let mut cuts: Vec<usize> = (0..picks.pick(3))
        .filter(|_| !at.is_empty())
        .map(|_| at[picks.pick(at.len())])
        .collect();
    cuts.sort_unstable();
    cuts.dedup();
    let mut pieces = Vec::new();
    let mut from = 0;
    for cut in cuts.into_iter().chain([text.len()]) {
        pieces.push(text[from..cut].to_string());
        from = cut;
    }
    pieces
}

/// `text` in up to three pieces, cut at char boundaries.
fn split_text(text: &str, picks: &mut Picks) -> Vec<String> {
    let at: Vec<usize> = text.char_indices().map(|(i, _)| i).skip(1).collect();
    split_at(text, &at, picks)
}

/// Base64 data in up to three pieces of whole quanta, each its own
/// encoding, as a stream sends them.
fn split_base64(data: &str, picks: &mut Picks) -> Vec<String> {
    let at: Vec<usize> = (4..data.len()).step_by(4).collect();
    split_at(data, &at, picks)
}

/// `items` in one to three chunks, in order; a chunk may be empty.
fn split_items<T: Clone>(items: &[T], picks: &mut Picks) -> Vec<Vec<T>> {
    let mut cuts: Vec<usize> = (0..picks.pick(3))
        .map(|_| picks.pick(items.len() + 1))
        .collect();
    cuts.sort_unstable();
    let mut chunks = Vec::new();
    let mut from = 0;
    for cut in cuts.into_iter().chain([items.len()]) {
        chunks.push(items[from..cut].to_vec());
        from = cut;
    }
    chunks
}

/// `extra`'s fields dealt over `slots` events.
fn spread(extra: &Map<String, Value>, slots: usize, picks: &mut Picks) -> Vec<Map<String, Value>> {
    let mut out = vec![Map::new(); slots.max(1)];
    for (key, value) in extra {
        out[picks.pick(slots)].insert(key.clone(), value.clone());
    }
    out
}

/// The signature a `step.start` announces: none, or the `""` placeholder
/// the API sends before the value arrives in a delta.
fn announced(signature: Option<&str>, picks: &mut Picks) -> Option<String> {
    match signature {
        None => None,
        Some("") => Some(String::new()),
        Some(_) => picks.coin().then(String::new),
    }
}

/// Each delta's signature for an arm that takes a delta's non-empty one:
/// the value on one delta, and `""` or nothing on the others.
fn replacing(signature: Option<&str>, deltas: usize, picks: &mut Picks) -> Vec<Option<String>> {
    let carrier = signature
        .filter(|s| !s.is_empty())
        .map(|_| picks.pick(deltas));
    (0..deltas)
        .map(|i| {
            if Some(i) == carrier {
                signature.map(String::from)
            } else {
                picks.coin().then(String::new)
            }
        })
        .collect()
}

/// The pieces of a signature an arm that extends it receives, one per
/// delta from the first; none for an absent or empty signature.
fn fragments(signature: Option<&str>, picks: &mut Picks) -> Vec<String> {
    match signature {
        Some(s) if !s.is_empty() => split_text(s, picks),
        _ => Vec::new(),
    }
}

/// Fragment `i` of `pieces`, or `""` or nothing past them.
fn fragment(pieces: &[String], i: usize, picks: &mut Picks) -> Option<String> {
    match pieces.get(i) {
        Some(piece) => Some(piece.clone()),
        None => picks.coin().then(String::new),
    }
}

/// `value` on the delta at `carrier`, nothing on the others: for a field a
/// later delta would overwrite.
fn once<T: Clone>(value: Option<&T>, i: usize, carrier: usize) -> Option<T> {
    if i == carrier { value.cloned() } else { None }
}

// =============================================================================
// Content blocks (user_input, model_output)
// =============================================================================

/// A content block of a kind that streams (text with its annotations, or
/// media), with the deltas that build it. The first delta names the
/// block's format and carries its first piece; the rest carry bare data,
/// which continues the block.
fn arb_streamed_block() -> impl Strategy<Value = (Content, Vec<StepDelta>)> {
    let media = || {
        (
            proptest::option::of(arb_base64()),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            arb_extra(),
            arb_picks(),
        )
    };
    prop_oneof![
        (
            arb_text(),
            proptest::option::of(prop::collection::vec(arb_annotation(), 0..3)),
            arb_extra(),
            arb_picks(),
        )
            .prop_map(|(text, annotations, extra, mut picks)| {
                let texts = split_text(&text, &mut picks);
                let notes = annotations
                    .as_ref()
                    .map(|a| split_items(a, &mut picks))
                    .unwrap_or_default();
                let mut extras = spread(&extra, texts.len() + notes.len(), &mut picks).into_iter();
                let mut deltas = Vec::new();
                for text in texts {
                    let extra = extras.next().unwrap_or_default();
                    deltas.push(StepDelta::Text { text, extra });
                }
                for annotations in notes {
                    let extra = extras.next().unwrap_or_default();
                    deltas.push(StepDelta::TextAnnotation { annotations, extra });
                }
                let block = Content::Text {
                    text: Some(text),
                    annotations,
                    extra,
                };
                (block, deltas)
            }),
        (media(), proptest::option::of(arb_resolution())).prop_map(
            |((data, uri, mime_type, extra, mut picks), resolution)| {
                let pieces = media_pieces(data.as_deref(), false, &mut picks);
                let mut extras = spread(&extra, pieces.len(), &mut picks).into_iter();
                let deltas = pieces
                    .into_iter()
                    .enumerate()
                    .map(|(i, piece)| StepDelta::Image {
                        data: piece,
                        uri: uri.clone().filter(|_| i == 0),
                        mime_type: mime_type.clone().filter(|_| i == 0),
                        resolution: resolution.clone().filter(|_| i == 0),
                        extra: extras.next().unwrap_or_default(),
                    })
                    .collect();
                let block = Content::Image {
                    data,
                    uri,
                    mime_type,
                    resolution,
                    extra,
                };
                (block, deltas)
            }
        ),
        (
            media(),
            proptest::option::of(any::<u32>()),
            proptest::option::of(1u32..8)
        )
            .prop_map(
                |((data, uri, mime_type, extra, mut picks), sample_rate, channels)| {
                    // A uri starts a block of its own, so audio with one
                    // arrives whole.
                    let pieces = media_pieces(data.as_deref(), uri.is_some(), &mut picks);
                    let mut extras = spread(&extra, pieces.len(), &mut picks).into_iter();
                    let mut deltas = Vec::new();
                    for (i, piece) in pieces.into_iter().enumerate() {
                        // A later piece may repeat the format, as a TTS
                        // stream's every delta does, and the sample rate
                        // may come under its legacy name.
                        let named = i == 0 || picks.coin();
                        let legacy = picks.coin();
                        let rate = sample_rate.filter(|_| named);
                        deltas.push(StepDelta::Audio {
                            data: piece,
                            uri: uri.clone().filter(|_| i == 0),
                            mime_type: mime_type.clone().filter(|_| named),
                            rate: rate.filter(|_| legacy),
                            sample_rate: rate.filter(|_| !legacy),
                            channels: channels.filter(|_| named),
                            extra: extras.next().unwrap_or_default(),
                        });
                    }
                    let block = Content::Audio {
                        data,
                        uri,
                        mime_type,
                        sample_rate,
                        channels,
                        extra,
                    };
                    (block, deltas)
                }
            ),
        (media(), proptest::option::of(arb_resolution())).prop_map(
            |((data, uri, mime_type, extra, mut picks), resolution)| {
                let pieces = media_pieces(data.as_deref(), false, &mut picks);
                let mut extras = spread(&extra, pieces.len(), &mut picks).into_iter();
                let deltas = pieces
                    .into_iter()
                    .enumerate()
                    .map(|(i, piece)| StepDelta::Video {
                        data: piece,
                        uri: uri.clone().filter(|_| i == 0),
                        mime_type: mime_type.clone().filter(|_| i == 0),
                        resolution: resolution.clone().filter(|_| i == 0),
                        extra: extras.next().unwrap_or_default(),
                    })
                    .collect();
                let block = Content::Video {
                    data,
                    uri,
                    mime_type,
                    resolution,
                    processing: None,
                    name: None,
                    extra,
                };
                (block, deltas)
            }
        ),
        media().prop_map(|(data, uri, mime_type, extra, mut picks)| {
            let pieces = media_pieces(data.as_deref(), false, &mut picks);
            let mut extras = spread(&extra, pieces.len(), &mut picks).into_iter();
            let deltas = pieces
                .into_iter()
                .enumerate()
                .map(|(i, piece)| StepDelta::Document {
                    data: piece,
                    uri: uri.clone().filter(|_| i == 0),
                    mime_type: mime_type.clone().filter(|_| i == 0),
                    extra: extras.next().unwrap_or_default(),
                })
                .collect();
            let block = Content::Document {
                data,
                uri,
                mime_type,
                extra,
            };
            (block, deltas)
        }),
    ]
}

/// Media data in the pieces its deltas carry: one piece when there is
/// none to split or it arrives `whole`.
fn media_pieces(data: Option<&str>, whole: bool, picks: &mut Picks) -> Vec<Option<String>> {
    match data {
        Some(data) if !whole => split_base64(data, picks).into_iter().map(Some).collect(),
        _ => vec![data.map(String::from)],
    }
}

/// Drops each block of the kind before it, so no two neighbours are of one
/// kind.
fn alternating(blocks: Vec<(Content, Vec<StepDelta>)>) -> Vec<(Content, Vec<StepDelta>)> {
    let mut kept: Vec<(Content, Vec<StepDelta>)> = Vec::new();
    for block in blocks {
        let repeats = kept.last().is_some_and(|(last, _)| {
            std::mem::discriminant(last) == std::mem::discriminant(&block.0)
        });
        if !repeats {
            kept.push(block);
        }
    }
    kept
}

/// `user_input` and `model_output`, whose deltas build content blocks. A
/// bare `model_output` may stream with no `step.start`, as the accumulator
/// allows.
fn arb_content_step() -> impl Strategy<Value = Streamed> {
    (
        prop::collection::vec(arb_streamed_block(), 0..4),
        proptest::option::of(arb_step_error()),
        arb_extra(),
        any::<bool>(),
        arb_picks(),
    )
        .prop_map(|(blocks, error, extra, user, mut picks)| {
            let (content, deltas): (Vec<Content>, Vec<Vec<StepDelta>>) =
                alternating(blocks).into_iter().unzip();
            let deltas: Vec<StepDelta> = deltas.into_iter().flatten().collect();
            if user {
                return Streamed {
                    expected: Step::UserInput {
                        content,
                        extra: extra.clone(),
                    },
                    start: Some(Step::UserInput {
                        content: Vec::new(),
                        extra,
                    }),
                    deltas,
                };
            }
            let bare = error.is_none() && extra.is_empty() && !deltas.is_empty();
            let start = (!bare || picks.coin()).then(|| Step::ModelOutput {
                content: Vec::new(),
                error: error.clone(),
                extra: extra.clone(),
            });
            Streamed {
                expected: Step::ModelOutput {
                    content,
                    error,
                    extra,
                },
                start,
                deltas,
            }
        })
}

// =============================================================================
// Thoughts and function calls
// =============================================================================

fn arb_thought() -> impl Strategy<Value = Streamed> {
    (
        prop::collection::vec(arb_content(), 0..3),
        proptest::option::of(arb_text()),
        arb_extra(),
        arb_picks(),
    )
        .prop_map(|(summary, signature, extra, mut picks)| {
            // Neighbouring text summaries merge, so none are drawn.
            let summary: Vec<Content> = {
                let mut kept: Vec<Content> = Vec::new();
                for block in summary {
                    let both_text = matches!(kept.last(), Some(Content::Text { .. }))
                        && matches!(block, Content::Text { .. });
                    if !both_text {
                        kept.push(block);
                    }
                }
                kept
            };
            let mut summaries: Vec<Option<Content>> = Vec::new();
            for block in &summary {
                // A text summary may arrive in pieces, which merge.
                if let Content::Text {
                    text: Some(text),
                    annotations,
                    extra,
                } = block
                {
                    let texts = split_text(text, &mut picks);
                    let extras = spread(extra, texts.len(), &mut picks);
                    for (i, (text, extra)) in texts.into_iter().zip(extras).enumerate() {
                        summaries.push(Some(Content::Text {
                            text: Some(text),
                            annotations: annotations.clone().filter(|_| i == 0),
                            extra,
                        }));
                    }
                } else {
                    summaries.push(Some(block.clone()));
                }
            }
            // A summary delta with no content changes nothing.
            if picks.coin() {
                let at = picks.pick(summaries.len() + 1);
                summaries.insert(at, None);
            }

            // The signature arrives in pieces, the first perhaps on the
            // step.start.
            let mut signatures = signature
                .as_deref()
                .map(|s| split_text(s, &mut picks))
                .unwrap_or_default();
            let first_piece =
                (!signatures.is_empty() && picks.coin()).then(|| signatures.remove(0));

            let mut kinds: Vec<bool> = Vec::new();
            let (mut s, mut g) = (summaries.len(), signatures.len());
            while s + g > 0 {
                let summary_next = g == 0 || (s > 0 && picks.coin());
                kinds.push(summary_next);
                if summary_next {
                    s -= 1;
                } else {
                    g -= 1;
                }
            }
            let mut extras = spread(&extra, kinds.len() + 1, &mut picks).into_iter();
            let start = Step::Thought {
                signature: first_piece,
                summary: Vec::new(),
                extra: extras.next().unwrap_or_default(),
            };
            let (mut summaries, mut signatures) = (summaries.into_iter(), signatures.into_iter());
            let deltas = kinds
                .into_iter()
                .map(|summary_next| {
                    let extra = extras.next().unwrap_or_default();
                    if summary_next {
                        StepDelta::ThoughtSummary {
                            content: summaries.next().flatten().map(Box::new),
                            extra,
                        }
                    } else {
                        StepDelta::ThoughtSignature {
                            signature: signatures.next(),
                            extra,
                        }
                    }
                })
                .collect();
            Streamed {
                expected: Step::Thought {
                    signature,
                    summary,
                    extra,
                },
                start: Some(start),
                deltas,
            }
        })
}

fn arb_function_call() -> impl Strategy<Value = Streamed> {
    (
        arb_identifier(),
        arb_identifier(),
        arb_json_value(),
        proptest::option::of(arb_text()),
        arb_extra(),
        arb_picks(),
    )
        .prop_map(|(id, name, arguments, signature, extra, mut picks)| {
            let pieces = split_text(&arguments.to_string(), &mut picks);
            let mut extras = spread(&extra, pieces.len() + 1, &mut picks).into_iter();
            let start = Step::FunctionCall {
                id: id.clone(),
                name: name.clone(),
                arguments: json!({}),
                signature: signature.clone(),
                extra: extras.next().unwrap_or_default(),
            };
            let deltas = pieces
                .into_iter()
                .map(|arguments| StepDelta::ArgumentsDelta {
                    arguments,
                    extra: extras.next().unwrap_or_default(),
                })
                .collect();
            Streamed {
                expected: Step::FunctionCall {
                    id,
                    name,
                    arguments,
                    signature,
                    extra,
                },
                start: Some(start),
                deltas,
            }
        })
}

fn arb_function_result() -> impl Strategy<Value = Streamed> {
    (
        arb_identifier(),
        proptest::option::of(arb_identifier()),
        arb_function_result_payload(),
        proptest::option::of(any::<bool>()),
        proptest::option::of(arb_text()),
        arb_extra(),
        arb_picks(),
    )
        .prop_map(
            |(call_id, name, result, is_error, signature, extra, mut picks)| {
                let mut extras = spread(&extra, 2, &mut picks).into_iter();
                let start = Step::FunctionResult {
                    call_id: call_id.clone(),
                    name: None,
                    result: FunctionResultPayload::Text(String::new()),
                    is_error: None,
                    signature: signature.clone(),
                    extra: extras.next().unwrap_or_default(),
                };
                // The delta may leave out the call id step.start gave.
                let delta = StepDelta::FunctionResult {
                    call_id: picks.coin().then(|| call_id.clone()),
                    name: name.clone(),
                    result: result.clone(),
                    is_error,
                    extra: extras.next().unwrap_or_default(),
                };
                Streamed {
                    expected: Step::FunctionResult {
                        call_id,
                        name,
                        result,
                        is_error,
                        signature,
                        extra,
                    },
                    start: Some(start),
                    deltas: vec![delta],
                }
            },
        )
}

// =============================================================================
// Server-side tool calls and results
// =============================================================================

fn arb_code_execution_call() -> impl Strategy<Value = Streamed> {
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

fn arb_code_execution_result() -> impl Strategy<Value = Streamed> {
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

fn arb_url_context_call() -> impl Strategy<Value = Streamed> {
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

fn arb_google_search_call() -> impl Strategy<Value = Streamed> {
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

fn arb_google_maps_call() -> impl Strategy<Value = Streamed> {
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

fn arb_retrieval_call() -> impl Strategy<Value = Streamed> {
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

fn arb_url_context_result() -> impl Strategy<Value = Streamed> {
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

fn arb_google_search_result() -> impl Strategy<Value = Streamed> {
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

fn arb_google_maps_result() -> impl Strategy<Value = Streamed> {
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
fn arb_file_search_result() -> impl Strategy<Value = Streamed> {
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

fn arb_file_search_call() -> impl Strategy<Value = Streamed> {
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

fn arb_mcp_server_tool_call() -> impl Strategy<Value = Streamed> {
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

fn arb_mcp_server_tool_result() -> impl Strategy<Value = Streamed> {
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
fn arb_signature_step() -> impl Strategy<Value = Streamed> {
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
fn arb_tool_call() -> impl Strategy<Value = Streamed> {
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
fn arb_unknown_step() -> impl Strategy<Value = Streamed> {
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

/// Every step the stream builds, each with the events that deliver it.
fn arb_streamed_step() -> BoxedStrategy<Streamed> {
    let known = prop_oneof![
        arb_content_step(),
        arb_thought(),
        arb_function_call(),
        arb_function_result(),
        arb_code_execution_call(),
        arb_code_execution_result(),
        arb_url_context_call(),
        arb_url_context_result(),
        arb_google_search_call(),
        arb_google_search_result(),
        arb_google_maps_call(),
        arb_google_maps_result(),
        arb_mcp_server_tool_call(),
        arb_mcp_server_tool_result(),
        arb_file_search_call(),
        arb_file_search_result(),
        arb_retrieval_call(),
        arb_signature_step(),
        arb_tool_call(),
    ];
    #[cfg(not(genai_strict_unknown))]
    let known = prop_oneof![9 => known, 1 => arb_unknown_step()];
    known.boxed()
}

// =============================================================================
// The property
// =============================================================================

/// Sends `steps` through `dispatch_stream_event` as one stream's events,
/// in order, and returns the steps on its `interaction.completed`, which
/// carries none of its own (as no stream observed has).
fn accumulate(steps: &[Streamed]) -> Result<Vec<Step>, TestCaseError> {
    let mut events = Vec::new();
    for (index, step) in steps.iter().enumerate() {
        if let Some(start) = &step.start {
            events.push(json!({"event_type": "step.start", "index": index, "step": start}));
        }
        for delta in &step.deltas {
            events.push(json!({"event_type": "step.delta", "index": index, "delta": delta}));
        }
        events.push(json!({"event_type": "step.stop", "index": index}));
    }
    events.push(json!({
        "event_type": "interaction.completed",
        "interaction": {"id": "streamed", "status": "completed"},
    }));

    let mut accumulator = StepAccumulator::new();
    let mut completed = None;
    for event in events {
        let parsed: InteractionStreamEvent = serde_json::from_value(event.clone())
            .map_err(|e| TestCaseError::fail(format!("{event} does not parse: {e}")))?;
        if let Some(StreamChunk::Completed(response)) =
            dispatch_stream_event(parsed, &mut accumulator)
        {
            completed = Some(response);
        }
    }
    let response =
        completed.ok_or_else(|| TestCaseError::fail("the stream produced no Completed chunk"))?;
    Ok(response.steps)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// The steps a stream delivers equal the steps a non-streamed response
    /// carries, compared as the JSON they replay as.
    #[test]
    fn streamed_steps_accumulate_to_the_steps_they_stream(
        steps in prop::collection::vec(arb_streamed_step(), 1..4),
    ) {
        let accumulated = accumulate(&steps)?;
        let expected: Vec<&Step> = steps.iter().map(|s| &s.expected).collect();
        prop_assert_eq!(
            serde_json::to_value(&accumulated).expect("steps serialize"),
            serde_json::to_value(&expected).expect("steps serialize"),
            "deltas: {:#?}",
            steps.iter().map(|s| &s.deltas).collect::<Vec<_>>()
        );
    }
}

/// The generator draws every step type the roundtrip generator does, so
/// the property reaches every arm: `prop_oneof!` is not
/// exhaustiveness-checked, and a step type left out would go untested.
#[test]
fn the_stream_generator_draws_every_known_step_type() {
    use super::steps::arb_known_step;
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;

    fn types<T>(
        strategy: impl Strategy<Value = T>,
        step: impl Fn(&T) -> &Step,
    ) -> std::collections::BTreeSet<String> {
        let mut runner = TestRunner::deterministic();
        (0..4000)
            .map(|_| {
                let value = strategy
                    .new_tree(&mut runner)
                    .expect("a value tree")
                    .current();
                step(&value).step_type().to_string()
            })
            .collect()
    }
    let known = types(arb_known_step(), |step| step);
    let streamed = types(arb_streamed_step(), |streamed| &streamed.expected);
    assert!(
        known.len() > 20,
        "the roundtrip generator drew only {known:?}"
    );
    let missing: Vec<&String> = known.difference(&streamed).collect();
    assert!(
        missing.is_empty(),
        "the stream generator never drew {missing:?}"
    );
}
