use serde_json::{Map, Value};

use crate::content::Content;

use super::{Step, StepDelta};

// =============================================================================
// Streaming step accumulation
// =============================================================================

/// Folds a streamed signature fragment into a step's signature.
///
/// `step.start` announces some signatures as `""` and delivers the value in
/// a delta (verified live on `processing_call`), so an empty existing value
/// is replaced; a non-empty one is extended, matching `thought_signature`.
fn merge_signature(existing: &mut Option<String>, fragment: Option<&str>) {
    let Some(fragment) = fragment.filter(|f| !f.is_empty()) else {
        return;
    };
    match existing {
        Some(sig) if !sig.is_empty() => sig.push_str(fragment),
        _ => *existing = Some(fragment.to_string()),
    }
}

/// Takes a delta's signature for a tool step, unless it is empty: a `""`
/// fragment (the placeholder `step.start` uses, see [`merge_signature`])
/// must not wipe the real value an earlier event carried, or replaying the
/// step fails with `400 ... missing signature`.
fn replace_signature(existing: &mut Option<String>, incoming: Option<&str>) {
    if let Some(signature) = incoming.filter(|s| !s.is_empty()) {
        *existing = Some(signature.to_string());
    }
}

/// Copies a delta's unmodeled fields onto what it builds, later values
/// winning, so the accumulated step round-trips them as the step a
/// non-streamed response carries would (#491).
fn absorb(target: &mut Map<String, Value>, extra: &Map<String, Value>) {
    target.extend(extra.iter().map(|(k, v)| (k.clone(), v.clone())));
}

/// The content blocks of a `user_input` / `model_output` step.
fn content_of(step: &mut Step) -> Option<&mut Vec<Content>> {
    match step {
        Step::UserInput { content, .. } | Step::ModelOutput { content, .. } => Some(content),
        _ => None,
    }
}

/// Accumulates `step.start` / `step.delta` / `step.stop` events into complete
/// [`Step`]s, so streaming consumers get a fully-populated `steps` array on
/// the final response even when the server's `interaction.completed` payload
/// omits it (as it did on every stream observed 2026-11-04).
#[derive(Debug, Default)]
pub(crate) struct StepAccumulator {
    steps: std::collections::BTreeMap<usize, AccumulatedStep>,
    /// Last cumulative interaction usage reported on a `step.stop` event.
    /// Used as a fallback when the terminal event carries no usage.
    last_cumulative_usage: Option<crate::response::UsageMetadata>,
}

#[derive(Debug)]
struct AccumulatedStep {
    step: Step,
    /// Raw buffer for `arguments_delta` fragments (function calls).
    args_buffer: String,
}

impl StepAccumulator {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Records a `step.start` event.
    pub(crate) fn start(&mut self, index: usize, step: Step) {
        self.steps.insert(
            index,
            AccumulatedStep {
                step,
                args_buffer: String::new(),
            },
        );
    }

    /// Applies a `step.delta` event to the step at `index`.
    pub(crate) fn apply_delta(&mut self, index: usize, delta: &StepDelta) {
        let entry = self.steps.entry(index).or_insert_with(|| AccumulatedStep {
            // Deltas without a preceding step.start most commonly belong to a
            // model_output step; start one so the content is not dropped.
            step: Step::model_output(Vec::new()),
            args_buffer: String::new(),
        });

        if Self::apply_content_delta(&mut entry.step, delta) {
            return;
        }
        if Self::apply_tool_delta(entry, delta) {
            // The delta matched the step it builds: keep its unmodeled
            // fields there too.
            if let (Some(target), Some(extra)) = (entry.step.extra_mut(), delta.extra()) {
                absorb(target, extra);
            }
            if let (Some(target), Some(extra)) =
                (entry.step.arguments_extra_mut(), delta.arguments_extra())
            {
                absorb(target, extra);
            }
        }
    }

    /// Applies a delta that builds a content block (text, annotations,
    /// media). Returns `false` for every other delta type. The delta's
    /// unmodeled fields go on the block it extends or starts.
    fn apply_content_delta(step: &mut Step, delta: &StepDelta) -> bool {
        let Some(content) = content_of(step) else {
            return matches!(
                delta,
                StepDelta::Text { .. }
                    | StepDelta::TextAnnotation { .. }
                    | StepDelta::Image { .. }
                    | StepDelta::Audio { .. }
                    | StepDelta::Video { .. }
                    | StepDelta::Document { .. }
            );
        };
        match delta {
            StepDelta::Text { text, extra } => match content.last_mut() {
                Some(Content::Text {
                    text: Some(t),
                    extra: block_extra,
                    ..
                }) => {
                    t.push_str(text);
                    absorb(block_extra, extra);
                }
                _ => content.push(Content::Text {
                    text: Some(text.clone()),
                    annotations: None,
                    extra: extra.clone(),
                }),
            },
            StepDelta::TextAnnotation { annotations, extra } => {
                if let Some(Content::Text {
                    annotations: annots,
                    extra: block_extra,
                    ..
                }) = content.last_mut()
                {
                    annots
                        .get_or_insert_with(Vec::new)
                        .extend(annotations.iter().cloned());
                    absorb(block_extra, extra);
                }
            }
            StepDelta::Image {
                data,
                uri,
                mime_type,
                resolution,
                extra,
            } => {
                // Each image arrives whole in one delta that names its
                // mime_type (verified live 2026-11-04, `gemini-3.1-flash-image`,
                // streamed), so a delta with a mime_type or uri starts a new
                // block. Only a bare data fragment continues the last image.
                // Live, each image came in its own model_output step; two in
                // one step was never observed, but keying on the block kind
                // alone would merge them into one corrupt base64 string
                // (#491).
                let fragment = mime_type.is_none() && uri.is_none();
                match (content.last_mut(), data) {
                    (
                        Some(Content::Image {
                            data: Some(existing),
                            extra: block_extra,
                            ..
                        }),
                        Some(new_data),
                    ) if fragment => {
                        append_base64(existing, new_data);
                        absorb(block_extra, extra);
                    }
                    _ => content.push(Content::Image {
                        data: data.clone(),
                        uri: uri.clone(),
                        mime_type: mime_type.clone(),
                        resolution: resolution.clone(),
                        extra: extra.clone(),
                    }),
                }
            }
            // Audio streams in pieces of one recording: a streamed TTS
            // answer is hundreds of deltas in one step, each naming the same
            // mime_type, sample_rate and channels (live 2026-11-05,
            // `gemini-3.8-flash-tts`: 243 deltas of `audio/l16` at 24000 Hz,
            // mono). So a delta continues the last audio block unless it
            // names a uri or a format that differs from the block's; then it
            // starts a new one.
            StepDelta::Audio {
                data,
                uri,
                mime_type,
                rate,
                sample_rate,
                channels,
                extra,
            } => match (content.last_mut(), data) {
                (
                    Some(Content::Audio {
                        data: Some(existing),
                        uri: None,
                        mime_type: block_mime,
                        sample_rate: block_rate,
                        channels: block_channels,
                        extra: block_extra,
                    }),
                    Some(new_data),
                ) if uri.is_none()
                    && same_or_unnamed(mime_type.as_ref(), block_mime.as_ref())
                    && same_or_unnamed(sample_rate.or(*rate).as_ref(), block_rate.as_ref())
                    && same_or_unnamed(channels.as_ref(), block_channels.as_ref()) =>
                {
                    append_base64(existing, new_data);
                    absorb(block_extra, extra);
                }
                _ => content.push(Content::Audio {
                    data: data.clone(),
                    uri: uri.clone(),
                    mime_type: mime_type.clone(),
                    sample_rate: sample_rate.or(*rate),
                    channels: *channels,
                    extra: extra.clone(),
                }),
            },
            // No model streams video or documents to observe (2026-11-05),
            // so they follow the image rule: a delta that names a mime_type
            // or uri starts a block, and only a bare data fragment continues
            // the last one. Splitting a chunked file into blocks loses
            // nothing (their data concatenates back), where merging two
            // whole files would corrupt both.
            StepDelta::Video {
                data,
                uri,
                mime_type,
                resolution,
                extra,
            } => match (content.last_mut(), data) {
                (
                    Some(Content::Video {
                        data: Some(existing),
                        extra: block_extra,
                        ..
                    }),
                    Some(new_data),
                ) if mime_type.is_none() && uri.is_none() => {
                    append_base64(existing, new_data);
                    absorb(block_extra, extra);
                }
                _ => content.push(Content::Video {
                    data: data.clone(),
                    uri: uri.clone(),
                    mime_type: mime_type.clone(),
                    resolution: resolution.clone(),
                    processing: None,
                    name: None,
                    extra: extra.clone(),
                }),
            },
            StepDelta::Document {
                data,
                uri,
                mime_type,
                extra,
            } => match (content.last_mut(), data) {
                (
                    Some(Content::Document {
                        data: Some(existing),
                        extra: block_extra,
                        ..
                    }),
                    Some(new_data),
                ) if mime_type.is_none() && uri.is_none() => {
                    append_base64(existing, new_data);
                    absorb(block_extra, extra);
                }
                _ => content.push(Content::Document {
                    data: data.clone(),
                    uri: uri.clone(),
                    mime_type: mime_type.clone(),
                    extra: extra.clone(),
                }),
            },
            _ => return false,
        }
        true
    }

    /// Applies a thought, function or tool delta. Returns `true` when the
    /// delta matched the step at its index, so its `extra` belongs there.
    fn apply_tool_delta(entry: &mut AccumulatedStep, delta: &StepDelta) -> bool {
        match (delta, &mut entry.step) {
            (
                StepDelta::ThoughtSummary {
                    content: Some(c), ..
                },
                Step::Thought { summary, .. },
            ) => {
                // Consecutive text summaries merge, keeping each chunk's
                // unmodeled fields on the block; other content appends.
                let c: &Content = c;
                if let (
                    Some(Content::Text {
                        text: Some(t),
                        extra: block_extra,
                        ..
                    }),
                    Content::Text {
                        text: Some(new),
                        extra: new_extra,
                        ..
                    },
                ) = (summary.last_mut(), c)
                {
                    t.push_str(new);
                    absorb(block_extra, new_extra);
                } else {
                    summary.push(c.clone());
                }
            }
            (StepDelta::ThoughtSummary { content: None, .. }, Step::Thought { .. }) => {}
            (
                StepDelta::ThoughtSignature { signature, .. },
                Step::Thought { signature: sig, .. },
            ) => {
                if let Some(fragment) = signature {
                    sig.get_or_insert_with(String::new).push_str(fragment);
                }
            }
            (StepDelta::ArgumentsDelta { arguments, .. }, _) => {
                entry.args_buffer.push_str(arguments);
                return matches!(entry.step, Step::FunctionCall { .. });
            }
            (
                StepDelta::FunctionResult {
                    call_id,
                    name,
                    result,
                    is_error,
                    ..
                },
                step,
            ) => {
                // Keep what step.start delivered: the delta carries no
                // signature, and may carry no call_id.
                let (start_call_id, signature, extra) = match step {
                    Step::FunctionResult {
                        call_id,
                        signature,
                        extra,
                        ..
                    } => (
                        Some(std::mem::take(call_id)),
                        signature.take(),
                        std::mem::take(extra),
                    ),
                    _ => (None, None, Map::new()),
                };
                *step = Step::FunctionResult {
                    call_id: call_id.clone().or(start_call_id).unwrap_or_default(),
                    name: name.clone(),
                    result: result.clone(),
                    is_error: *is_error,
                    signature,
                    extra,
                };
            }
            (
                StepDelta::CodeExecutionCall {
                    language,
                    code,
                    signature,
                    ..
                },
                Step::CodeExecutionCall {
                    language: lang,
                    code: existing_code,
                    signature: sig,
                    ..
                },
            ) => {
                if let Some(l) = language {
                    *lang = l.clone();
                }
                if let Some(c) = code {
                    existing_code.push_str(c);
                }
                replace_signature(sig, signature.as_deref());
            }
            (
                StepDelta::CodeExecutionResult {
                    result,
                    is_error,
                    signature,
                    ..
                },
                Step::CodeExecutionResult {
                    result: existing,
                    is_error: err,
                    signature: sig,
                    ..
                },
            ) => {
                existing.push_str(result);
                if let Some(e) = is_error {
                    *err = *e;
                }
                replace_signature(sig, signature.as_deref());
            }
            (
                StepDelta::UrlContextCall {
                    urls, signature, ..
                },
                Step::UrlContextCall {
                    urls: existing,
                    signature: sig,
                    ..
                },
            )
            | (
                StepDelta::GoogleSearchCall {
                    queries: urls,
                    signature,
                    ..
                },
                Step::GoogleSearchCall {
                    queries: existing,
                    signature: sig,
                    ..
                },
            )
            | (
                StepDelta::GoogleMapsCall {
                    queries: urls,
                    signature,
                    ..
                },
                Step::GoogleMapsCall {
                    queries: existing,
                    signature: sig,
                    ..
                },
            ) => {
                existing.extend(urls.iter().cloned());
                replace_signature(sig, signature.as_deref());
            }
            (
                StepDelta::UrlContextResult {
                    result,
                    is_error,
                    signature,
                    ..
                },
                Step::UrlContextResult {
                    result: existing,
                    is_error: err,
                    signature: sig,
                    ..
                },
            ) => {
                existing.extend(result.iter().cloned());
                if is_error.is_some() {
                    *err = *is_error;
                }
                replace_signature(sig, signature.as_deref());
            }
            (
                StepDelta::GoogleSearchResult {
                    result,
                    is_error,
                    signature,
                    ..
                },
                Step::GoogleSearchResult {
                    result: existing,
                    is_error: err,
                    signature: sig,
                    ..
                },
            ) => {
                existing.extend(result.iter().cloned());
                if is_error.is_some() {
                    *err = *is_error;
                }
                replace_signature(sig, signature.as_deref());
            }
            (
                StepDelta::McpServerToolCall {
                    name,
                    server_name,
                    arguments,
                    ..
                },
                Step::McpServerToolCall {
                    name: n,
                    server_name: sn,
                    arguments: args,
                    ..
                },
            ) => {
                n.clone_from(name);
                sn.clone_from(server_name);
                args.clone_from(arguments);
            }
            (
                StepDelta::McpServerToolResult {
                    name,
                    server_name,
                    result,
                    ..
                },
                Step::McpServerToolResult {
                    name: n,
                    server_name: sn,
                    result: r,
                    ..
                },
            ) => {
                if name.is_some() {
                    n.clone_from(name);
                }
                if server_name.is_some() {
                    sn.clone_from(server_name);
                }
                r.clone_from(result);
            }
            (
                StepDelta::FileSearchCall { signature, .. },
                Step::FileSearchCall { signature: sig, .. },
            ) => replace_signature(sig, signature.as_deref()),
            (
                StepDelta::FileSearchResult {
                    result, signature, ..
                },
                Step::FileSearchResult {
                    signature: sig,
                    extra,
                    ..
                },
            ) => {
                // The step has no `result` field; keep what the stream sent.
                if !result.is_empty()
                    && let serde_json::Value::Array(existing) = extra
                        .entry("result")
                        .or_insert_with(|| serde_json::Value::Array(Vec::new()))
                {
                    existing.extend(result.iter().cloned());
                }
                replace_signature(sig, signature.as_deref());
            }
            (
                StepDelta::GoogleMapsResult {
                    result, signature, ..
                },
                Step::GoogleMapsResult {
                    result: existing,
                    signature: sig,
                    ..
                },
            ) => {
                existing.extend(result.iter().cloned());
                replace_signature(sig, signature.as_deref());
            }
            (
                StepDelta::ProcessingCall { signature, .. },
                Step::ProcessingCall { signature: sig, .. },
            )
            | (
                StepDelta::ProcessingResult { signature, .. },
                Step::ProcessingResult { signature: sig, .. },
            ) => merge_signature(sig, signature.as_deref()),
            (
                StepDelta::RetrievalCall {
                    queries,
                    retrieval_type,
                    signature,
                    ..
                },
                Step::RetrievalCall {
                    queries: existing,
                    retrieval_type: rt,
                    signature: sig,
                    ..
                },
            ) => {
                existing.extend(queries.iter().cloned());
                if retrieval_type.is_some() {
                    rt.clone_from(retrieval_type);
                }
                merge_signature(sig, signature.as_deref());
            }
            (
                StepDelta::RetrievalResult {
                    is_error,
                    signature,
                    ..
                },
                Step::RetrievalResult {
                    is_error: err,
                    signature: sig,
                    ..
                },
            ) => {
                if is_error.is_some() {
                    *err = *is_error;
                }
                merge_signature(sig, signature.as_deref());
            }
            (
                StepDelta::Unknown { delta_type, data },
                Step::Unknown {
                    step_type,
                    data: Value::Object(step_obj),
                },
            ) if step_type == delta_type
                && data.get("signature").and_then(Value::as_str).is_some() =>
            {
                // A signature is the one field known to matter for replay,
                // so carry it onto a same-typed Unknown step rather than
                // drop it.
                let fragment = data.get("signature").and_then(Value::as_str);
                let mut sig = step_obj
                    .get("signature")
                    .and_then(Value::as_str)
                    .map(String::from);
                merge_signature(&mut sig, fragment);
                if let Some(sig) = sig {
                    step_obj.insert("signature".into(), Value::String(sig));
                }
            }
            (delta, step) => {
                tracing::debug!(
                    "Skipping '{}' delta for a '{}' step during accumulation",
                    delta.delta_type(),
                    step.step_type()
                );
                return false;
            }
        }
        true
    }

    /// Finalizes the step at `index` (called on `step.stop`).
    ///
    /// Parses any buffered `arguments_delta` fragments into the function
    /// call's `arguments`.
    pub(crate) fn stop(&mut self, index: usize) {
        if let Some(entry) = self.steps.get_mut(&index) {
            Self::finalize_entry(entry);
        }
    }

    /// Records the cumulative interaction usage reported on a `step.stop`
    /// event, so it can serve as a fallback if the terminal event omits usage.
    pub(crate) fn record_cumulative_usage(&mut self, usage: crate::response::UsageMetadata) {
        self.last_cumulative_usage = Some(usage);
    }

    /// Takes the last recorded cumulative usage, if any.
    pub(crate) fn take_cumulative_usage(&mut self) -> Option<crate::response::UsageMetadata> {
        self.last_cumulative_usage.take()
    }

    fn finalize_entry(entry: &mut AccumulatedStep) {
        if entry.args_buffer.is_empty() {
            return;
        }
        if let Step::FunctionCall { arguments, .. } = &mut entry.step {
            match serde_json::from_str::<Value>(&entry.args_buffer) {
                Ok(parsed) => *arguments = parsed,
                Err(e) => {
                    tracing::warn!(
                        "Failed to parse accumulated arguments_delta buffer as JSON: {}. \
                         Preserving raw string.",
                        e
                    );
                    *arguments = Value::String(std::mem::take(&mut entry.args_buffer));
                }
            }
        }
        entry.args_buffer.clear();
    }

    /// Consumes the accumulator and returns the ordered steps.
    pub(crate) fn finish(mut self) -> Vec<Step> {
        for entry in self.steps.values_mut() {
            Self::finalize_entry(entry);
        }
        self.steps.into_values().map(|e| e.step).collect()
    }
}

/// Whether a delta's format field matches its block's: equal, or not named
/// by the delta.
fn same_or_unnamed<T: PartialEq>(delta: Option<&T>, block: Option<&T>) -> bool {
    delta.is_none_or(|d| block == Some(d))
}

/// Appends one base64 chunk to another. Text concatenation is exact when the
/// first chunk carries no padding, which every streamed chunk so far has
/// met (live 2026-11-05: 243 TTS chunks, each a multiple of 4 characters
/// with no `=`). A padded first chunk is decoded, joined as bytes and
/// encoded again, so the result stays valid base64; data that does not
/// decode is concatenated as text, as before.
fn append_base64(existing: &mut String, chunk: &str) {
    use base64::Engine;
    if !existing.ends_with('=') {
        existing.push_str(chunk);
        return;
    }
    let engine = base64::engine::general_purpose::STANDARD;
    match (engine.decode(existing.as_bytes()), engine.decode(chunk)) {
        (Ok(mut bytes), Ok(more)) => {
            bytes.extend_from_slice(&more);
            *existing = engine.encode(bytes);
        }
        _ => existing.push_str(chunk),
    }
}

#[cfg(test)]
#[path = "accumulator_tests.rs"]
mod tests;
