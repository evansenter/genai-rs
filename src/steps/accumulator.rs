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

/// Accumulates `step.start` / `step.delta` / `step.stop` events into complete
/// [`Step`]s, so streaming consumers get a fully-populated `steps` array on
/// the final response even when the server's `interaction.completed` payload
/// omits it.
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
            step: Step::ModelOutput {
                content: Vec::new(),
                error: None,
            },
            args_buffer: String::new(),
        });

        match delta {
            StepDelta::Text { text } => {
                if let Step::UserInput { content } | Step::ModelOutput { content, .. } =
                    &mut entry.step
                {
                    if let Some(Content::Text { text: Some(t), .. }) = content.last_mut() {
                        t.push_str(text);
                    } else {
                        content.push(Content::text(text.clone()));
                    }
                }
            }
            StepDelta::TextAnnotation { annotations } => {
                if let Step::UserInput { content } | Step::ModelOutput { content, .. } =
                    &mut entry.step
                    && let Some(Content::Text {
                        annotations: annots,
                        ..
                    }) = content.last_mut()
                {
                    annots
                        .get_or_insert_with(Vec::new)
                        .extend(annotations.iter().cloned());
                }
            }
            StepDelta::Image {
                data,
                uri,
                mime_type,
                resolution,
            } => {
                if let Step::UserInput { content } | Step::ModelOutput { content, .. } =
                    &mut entry.step
                {
                    // Images may stream in multiple chunks; append base64 data
                    // to the previous image block when present.
                    if let (
                        Some(Content::Image {
                            data: Some(existing),
                            ..
                        }),
                        Some(new_data),
                    ) = (content.last_mut(), data.as_ref())
                    {
                        existing.push_str(new_data);
                    } else {
                        content.push(Content::Image {
                            data: data.clone(),
                            uri: uri.clone(),
                            mime_type: mime_type.clone(),
                            resolution: resolution.clone(),
                        });
                    }
                }
            }
            StepDelta::Audio {
                data,
                uri,
                mime_type,
                rate,
                sample_rate,
                channels,
            } => {
                if let Step::UserInput { content } | Step::ModelOutput { content, .. } =
                    &mut entry.step
                {
                    // Audio may stream in multiple chunks; append base64 data to
                    // the previous audio block when present.
                    if let (
                        Some(Content::Audio {
                            data: Some(existing),
                            ..
                        }),
                        Some(new_data),
                    ) = (content.last_mut(), data.as_ref())
                    {
                        existing.push_str(new_data);
                    } else {
                        content.push(Content::Audio {
                            data: data.clone(),
                            uri: uri.clone(),
                            mime_type: mime_type.clone(),
                            sample_rate: sample_rate.or(*rate),
                            channels: *channels,
                        });
                    }
                }
            }
            StepDelta::Video {
                data,
                uri,
                mime_type,
                resolution,
            } => {
                if let Step::UserInput { content } | Step::ModelOutput { content, .. } =
                    &mut entry.step
                {
                    // Video may stream in multiple chunks; append base64 data
                    // to the previous video block when present.
                    if let (
                        Some(Content::Video {
                            data: Some(existing),
                            ..
                        }),
                        Some(new_data),
                    ) = (content.last_mut(), data.as_ref())
                    {
                        existing.push_str(new_data);
                    } else {
                        content.push(Content::Video {
                            data: data.clone(),
                            uri: uri.clone(),
                            mime_type: mime_type.clone(),
                            resolution: resolution.clone(),
                            processing: None,
                            name: None,
                        });
                    }
                }
            }
            StepDelta::Document {
                data,
                uri,
                mime_type,
            } => {
                if let Step::UserInput { content } | Step::ModelOutput { content, .. } =
                    &mut entry.step
                {
                    // Documents may stream in multiple chunks; append base64
                    // data to the previous document block when present.
                    if let (
                        Some(Content::Document {
                            data: Some(existing),
                            ..
                        }),
                        Some(new_data),
                    ) = (content.last_mut(), data.as_ref())
                    {
                        existing.push_str(new_data);
                    } else {
                        content.push(Content::Document {
                            data: data.clone(),
                            uri: uri.clone(),
                            mime_type: mime_type.clone(),
                        });
                    }
                }
            }
            StepDelta::ThoughtSummary { content } => {
                if let Step::Thought { summary, .. } = &mut entry.step
                    && let Some(c) = content
                {
                    // Consecutive text summaries merge; other content appends.
                    if let (
                        Some(Content::Text { text: Some(t), .. }),
                        Content::Text {
                            text: Some(new), ..
                        },
                    ) = (summary.last_mut(), c)
                    {
                        t.push_str(new);
                    } else {
                        summary.push(c.clone());
                    }
                }
            }
            StepDelta::ThoughtSignature { signature } => {
                if let Step::Thought { signature: sig, .. } = &mut entry.step
                    && let Some(fragment) = signature
                {
                    sig.get_or_insert_with(String::new).push_str(fragment);
                }
            }
            StepDelta::ArgumentsDelta { arguments } => {
                entry.args_buffer.push_str(arguments);
            }
            StepDelta::FunctionResult {
                call_id,
                name,
                result,
                is_error,
            } => {
                // Keep what step.start delivered: the delta carries no
                // signature, and may carry no call_id.
                let (start_call_id, signature) = match &entry.step {
                    Step::FunctionResult {
                        call_id, signature, ..
                    } => (Some(call_id.clone()), signature.clone()),
                    _ => (None, None),
                };
                entry.step = Step::FunctionResult {
                    call_id: call_id.clone().or(start_call_id).unwrap_or_default(),
                    name: name.clone(),
                    result: result.clone(),
                    is_error: *is_error,
                    signature,
                };
            }
            StepDelta::CodeExecutionCall {
                language,
                code,
                signature,
            } => {
                if let Step::CodeExecutionCall {
                    language: lang,
                    code: existing_code,
                    signature: sig,
                    ..
                } = &mut entry.step
                {
                    if let Some(l) = language {
                        *lang = l.clone();
                    }
                    if let Some(c) = code {
                        existing_code.push_str(c);
                    }
                    if signature.is_some() {
                        *sig = signature.clone();
                    }
                }
            }
            StepDelta::CodeExecutionResult {
                result,
                is_error,
                signature,
            } => {
                if let Step::CodeExecutionResult {
                    result: existing,
                    is_error: err,
                    signature: sig,
                    ..
                } = &mut entry.step
                {
                    existing.push_str(result);
                    if let Some(e) = is_error {
                        *err = *e;
                    }
                    if signature.is_some() {
                        *sig = signature.clone();
                    }
                }
            }
            StepDelta::UrlContextCall { urls, signature } => {
                if let Step::UrlContextCall {
                    urls: existing,
                    signature: sig,
                    ..
                } = &mut entry.step
                {
                    existing.extend(urls.iter().cloned());
                    if signature.is_some() {
                        *sig = signature.clone();
                    }
                }
            }
            StepDelta::UrlContextResult {
                result,
                is_error,
                signature,
            } => {
                if let Step::UrlContextResult {
                    result: existing,
                    is_error: err,
                    signature: sig,
                    ..
                } = &mut entry.step
                {
                    existing.extend(result.iter().cloned());
                    if is_error.is_some() {
                        *err = *is_error;
                    }
                    if signature.is_some() {
                        *sig = signature.clone();
                    }
                }
            }
            StepDelta::GoogleSearchCall { queries, signature } => {
                if let Step::GoogleSearchCall {
                    queries: existing,
                    signature: sig,
                    ..
                } = &mut entry.step
                {
                    existing.extend(queries.iter().cloned());
                    if signature.is_some() {
                        *sig = signature.clone();
                    }
                }
            }
            StepDelta::GoogleSearchResult {
                result,
                is_error,
                signature,
            } => {
                if let Step::GoogleSearchResult {
                    result: existing,
                    is_error: err,
                    signature: sig,
                    ..
                } = &mut entry.step
                {
                    existing.extend(result.iter().cloned());
                    if is_error.is_some() {
                        *err = *is_error;
                    }
                    if signature.is_some() {
                        *sig = signature.clone();
                    }
                }
            }
            StepDelta::McpServerToolCall {
                name,
                server_name,
                arguments,
            } => {
                if let Step::McpServerToolCall {
                    name: n,
                    server_name: sn,
                    arguments: args,
                    ..
                } = &mut entry.step
                {
                    *n = name.clone();
                    *sn = server_name.clone();
                    *args = arguments.clone();
                }
            }
            StepDelta::McpServerToolResult {
                name,
                server_name,
                result,
            } => {
                if let Step::McpServerToolResult {
                    name: n,
                    server_name: sn,
                    result: r,
                    ..
                } = &mut entry.step
                {
                    if name.is_some() {
                        *n = name.clone();
                    }
                    if server_name.is_some() {
                        *sn = server_name.clone();
                    }
                    *r = result.clone();
                }
            }
            StepDelta::FileSearchCall { signature } => {
                if let Step::FileSearchCall { signature: sig, .. } = &mut entry.step
                    && signature.is_some()
                {
                    *sig = signature.clone();
                }
            }
            StepDelta::FileSearchResult { result, signature } => {
                if let Step::FileSearchResult {
                    result: existing,
                    signature: sig,
                    ..
                } = &mut entry.step
                {
                    existing.extend(result.iter().cloned());
                    if signature.is_some() {
                        *sig = signature.clone();
                    }
                }
            }
            StepDelta::GoogleMapsCall { queries, signature } => {
                if let Step::GoogleMapsCall {
                    queries: existing,
                    signature: sig,
                    ..
                } = &mut entry.step
                {
                    existing.extend(queries.iter().cloned());
                    if signature.is_some() {
                        *sig = signature.clone();
                    }
                }
            }
            StepDelta::GoogleMapsResult { result, signature } => {
                if let Step::GoogleMapsResult {
                    result: existing,
                    signature: sig,
                    ..
                } = &mut entry.step
                {
                    existing.extend(result.iter().cloned());
                    if signature.is_some() {
                        *sig = signature.clone();
                    }
                }
            }
            StepDelta::ProcessingCall { signature } => {
                if let Step::ProcessingCall { signature: sig, .. } = &mut entry.step {
                    merge_signature(sig, signature.as_deref());
                }
            }
            StepDelta::ProcessingResult { signature } => {
                if let Step::ProcessingResult { signature: sig, .. } = &mut entry.step {
                    merge_signature(sig, signature.as_deref());
                }
            }
            StepDelta::RetrievalCall {
                queries,
                retrieval_type,
                signature,
            } => {
                if let Step::RetrievalCall {
                    queries: existing,
                    retrieval_type: rt,
                    signature: sig,
                    ..
                } = &mut entry.step
                {
                    existing.extend(queries.iter().cloned());
                    if retrieval_type.is_some() {
                        *rt = retrieval_type.clone();
                    }
                    merge_signature(sig, signature.as_deref());
                }
            }
            StepDelta::RetrievalResult {
                is_error,
                signature,
            } => {
                if let Step::RetrievalResult {
                    is_error: err,
                    signature: sig,
                    ..
                } = &mut entry.step
                {
                    if is_error.is_some() {
                        *err = *is_error;
                    }
                    merge_signature(sig, signature.as_deref());
                }
            }
            StepDelta::Unknown { delta_type, data } => {
                // A signature is the one field known to matter for replay, so
                // carry it onto a same-typed Unknown step rather than drop it.
                if let Step::Unknown {
                    step_type,
                    data: serde_json::Value::Object(step_obj),
                } = &mut entry.step
                    && step_type == delta_type
                    && let Some(fragment) = data.get("signature").and_then(|v| v.as_str())
                {
                    let mut sig = step_obj
                        .get("signature")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    merge_signature(&mut sig, Some(fragment));
                    if let Some(sig) = sig {
                        step_obj.insert("signature".into(), serde_json::Value::String(sig));
                    }
                } else {
                    tracing::debug!(
                        "Skipping unknown StepDelta type '{}' during accumulation",
                        delta_type
                    );
                }
            }
        }
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
            match serde_json::from_str::<serde_json::Value>(&entry.args_buffer) {
                Ok(parsed) => *arguments = parsed,
                Err(e) => {
                    tracing::warn!(
                        "Failed to parse accumulated arguments_delta buffer as JSON: {}. \
                         Preserving raw string.",
                        e
                    );
                    *arguments = serde_json::Value::String(std::mem::take(&mut entry.args_buffer));
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

#[cfg(test)]
#[path = "accumulator_tests.rs"]
mod tests;
