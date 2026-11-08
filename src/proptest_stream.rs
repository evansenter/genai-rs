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

use super::content::{arb_annotation, arb_content, arb_resolution};
use super::steps::{arb_function_result_payload, arb_step_error};
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

// The server-side tools' stream forms, beside this file.
#[path = "proptest_stream_tools.rs"]
mod server_tools;
#[cfg(not(genai_strict_unknown))]
use server_tools::arb_unknown_step;
use server_tools::{
    arb_code_execution_call, arb_code_execution_result, arb_file_search_call,
    arb_file_search_result, arb_google_maps_call, arb_google_maps_result, arb_google_search_call,
    arb_google_search_result, arb_mcp_server_tool_call, arb_mcp_server_tool_result,
    arb_retrieval_call, arb_signature_step, arb_tool_call, arb_url_context_call,
    arb_url_context_result,
};

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
