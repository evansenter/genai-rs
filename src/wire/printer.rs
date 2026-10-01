use super::{RAW_BODY_LIMIT, WireEvent, WireInspector, truncate_long_fields, truncate_utf8};

// =============================================================================
// Color abstraction (feature-gated)
// =============================================================================

#[cfg(feature = "wire-color")]
mod paint {
    use colored::Colorize;

    pub fn bold(s: &str) -> String {
        s.bold().to_string()
    }
    pub fn dimmed(s: &str) -> String {
        s.dimmed().to_string()
    }
    pub fn green(s: &str) -> String {
        s.green().to_string()
    }
    pub fn red(s: &str) -> String {
        s.red().to_string()
    }
    pub fn green_bold(s: &str) -> String {
        s.green().bold().to_string()
    }
    pub fn yellow_bold(s: &str) -> String {
        s.yellow().bold().to_string()
    }
    pub fn magenta_bold(s: &str) -> String {
        s.magenta().bold().to_string()
    }
    pub fn cyan_bold(s: &str) -> String {
        s.cyan().bold().to_string()
    }
    pub fn red_bold(s: &str) -> String {
        s.red().bold().to_string()
    }
    pub fn blue_bold(s: &str) -> String {
        s.blue().bold().to_string()
    }

    /// Colorize JSON for terminal output, or `None` if colorization fails.
    pub fn json(value: &serde_json::Value) -> Option<String> {
        colored_json::to_colored_json_auto(value).ok()
    }
}

#[cfg(not(feature = "wire-color"))]
mod paint {
    pub fn bold(s: &str) -> String {
        s.to_string()
    }
    pub fn dimmed(s: &str) -> String {
        s.to_string()
    }
    pub fn green(s: &str) -> String {
        s.to_string()
    }
    pub fn red(s: &str) -> String {
        s.to_string()
    }
    pub fn green_bold(s: &str) -> String {
        s.to_string()
    }
    pub fn yellow_bold(s: &str) -> String {
        s.to_string()
    }
    pub fn magenta_bold(s: &str) -> String {
        s.to_string()
    }
    pub fn cyan_bold(s: &str) -> String {
        s.to_string()
    }
    pub fn red_bold(s: &str) -> String {
        s.to_string()
    }
    pub fn blue_bold(s: &str) -> String {
        s.to_string()
    }

    /// Without the `wire-color` feature there is no colorizer; callers fall
    /// back to plain pretty-printed JSON.
    pub fn json(_value: &serde_json::Value) -> Option<String> {
        None
    }
}

// =============================================================================
// WireFilter
// =============================================================================

/// Which events [`LoudWirePrinter`] should print, and how loudly.
///
/// Parsed from the `LOUD_WIRE` environment variable. The firehose is the
/// right default for a single request, and the wrong one the moment a
/// harness session is involved: a few turns produce thousands of
/// pretty-printed lines, and finding the one message that matters means
/// grepping raw JSON out of the scrollback.
///
/// # Syntax
///
/// `LOUD_WIRE` takes a comma-separated list. `1`, `true`, or any empty
/// value means "everything, pretty-printed" — the historical behavior.
///
/// | Selector | Keeps |
/// |----------|-------|
/// | `request` | HTTP requests |
/// | `response` | HTTP responses (status, body, error bodies) |
/// | `sse` | SSE frames |
/// | `ws` | Every WebSocket message |
/// | `harness` | Harness spawn and stderr lines |
/// | `upload` | File-upload start/complete |
/// | *anything else* | A WebSocket payload whose top-level key matches (e.g. `stepUpdate`, `toolCall`), or whose *nested* key one level in matches (e.g. `mcpTool`, `runCommand` — the step actions that all live under `stepUpdate`) |
/// | `summary` | Modifier: one line per event instead of full bodies |
///
/// A value that matches no category and no payload key selects **nothing**,
/// so `LOUD_WIRE=0`, `false`, `off` and `verbose` all print silence. Note
/// that this is a behavior change: the gate used to be "is the variable set
/// at all", so those values previously produced the full firehose. If
/// output has gone missing, an unrecognized selector is the first thing to
/// check.
///
/// # Examples
///
/// ```bash
/// LOUD_WIRE=1                      # everything (unchanged)
/// LOUD_WIRE=summary                # everything, one line each
/// LOUD_WIRE=stepUpdate             # only stepUpdate WS payloads
/// LOUD_WIRE=toolCall,summary       # tool calls, one line each
/// LOUD_WIRE=request,response       # HTTP only, no WebSocket noise
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WireFilter {
    /// Lowercased selectors. Empty means "keep everything".
    selectors: Vec<String>,
    /// One line per event instead of pretty-printed bodies.
    summary: bool,
}

impl WireFilter {
    /// A filter that keeps every event, pretty-printed.
    #[must_use]
    pub const fn all() -> Self {
        Self {
            selectors: Vec::new(),
            summary: false,
        }
    }

    /// Parses a `LOUD_WIRE` value. See the type docs for the syntax.
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        let mut selectors = Vec::new();
        let mut summary = false;
        let mut keep_all = false;
        for token in raw.split(',') {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            match token.to_ascii_lowercase().as_str() {
                // The historical "on" values select everything.
                "1" | "true" | "yes" | "on" | "all" => keep_all = true,
                "summary" => summary = true,
                other => selectors.push(other.to_string()),
            }
        }
        // Deliberately after the loop rather than an early return: `summary`
        // is a modifier, so `1,summary` and `summary,1` must mean the same
        // thing. Returning on the "on" arm would honor only the latter.
        if keep_all {
            selectors.clear();
        }
        Self { selectors, summary }
    }

    /// True when bodies should be collapsed to one line per event.
    #[must_use]
    pub const fn is_summary(&self) -> bool {
        self.summary
    }

    /// The category name an event belongs to, for selector matching.
    const fn category(event: &WireEvent) -> &'static str {
        match event {
            WireEvent::Request { .. } => "request",
            WireEvent::ResponseStatus { .. }
            | WireEvent::ResponseBody { .. }
            | WireEvent::ErrorBody { .. } => "response",
            WireEvent::SseFrame { .. } => "sse",
            WireEvent::UploadStart { .. } | WireEvent::UploadComplete { .. } => "upload",
            WireEvent::HarnessSpawn { .. } | WireEvent::HarnessStderr { .. } => "harness",
            WireEvent::WsSend { .. } | WireEvent::WsReceive { .. } => "ws",
        }
    }

    /// Whether this event should be printed.
    #[must_use]
    pub fn allows(&self, event: &WireEvent) -> bool {
        if self.selectors.is_empty() {
            return true;
        }
        let category = Self::category(event);
        if self.selectors.iter().any(|s| s == category) {
            return true;
        }
        // Otherwise a selector may name a WebSocket payload's oneof key,
        // which is the granularity that actually matters when reading a
        // harness session (`stepUpdate` vs `toolCall` vs `userInput`).
        // Received frames get the extra nested-action granularity below;
        // sends are matched on their arm alone, for the same reason
        // `ws_payload_keys` only qualifies receives. Selection and summary
        // rendering must agree about what a line is *about*, and an
        // `InputEvent` arm has no action to descend into.
        let (payload, harness_receive) = match event {
            WireEvent::WsSend { payload, .. } => (payload, false),
            WireEvent::WsReceive { payload, .. } => (payload, true),
            _ => return false,
        };
        payload.as_object().is_some_and(|map| {
            map.iter()
                // Gated the same way `payload_keys_inner` gates it, so a
                // key that renders on a line can also select it. Latent
                // either way today — an `InputEvent` serializes as a lone
                // oneof key and carries no envelope — but the two sides
                // drifting is the failure this pairing exists to prevent.
                .filter(|(k, _)| !(harness_receive && is_envelope_key(k)))
                .any(|(key, value)| {
                    if self.selectors.contains(&key.to_ascii_lowercase()) {
                        return true;
                    }
                    if !harness_receive {
                        return false;
                    }
                    // ...and one level deeper, because the granularity a
                    // reader usually wants is *which action* a step carried,
                    // and every builtin action (`mcpTool`, `runCommand`,
                    // `viewFile`, …) hides under the single `stepUpdate` key.
                    // Without this, `LOUD_WIRE=mcpTool` silently matches
                    // nothing — which is exactly what it did until an example
                    // advertised it and printed silence.
                    //
                    // Restricted to *object-valued* nested keys, which is
                    // what an action is — matching a scalar like `text`
                    // would print a line labelled with some unrelated
                    // action rather than the key that matched. And only one
                    // level: deeper would match leaf field names across
                    // unrelated messages, since selectors are not scoped by
                    // message type.
                    nested_action_keys(value)
                        .iter()
                        .any(|nested| self.selectors.contains(&nested.to_ascii_lowercase()))
                })
        })
    }
}

/// The object-valued keys one level inside a payload value — the step
/// *actions* (`mcpTool`, `runCommand`, `viewFile`, …), as opposed to a
/// step's scalar fields (`text`, `stepIndex`, `state`).
///
/// Shared by selection and summary rendering so the two cannot disagree
/// about what a line is "about": a nested selector matches exactly the
/// keys the label can name.
fn nested_action_keys(value: &serde_json::Value) -> Vec<&str> {
    value.as_object().map_or_else(Vec::new, |inner| {
        inner
            .iter()
            .filter(|(_, v)| v.is_object())
            .map(|(k, _)| k.as_str())
            .collect()
    })
}

/// Envelope bookkeeping that rides alongside a harness message rather
/// than being one, and is therefore useless as a selector: matching on
/// `seqNum` would keep the whole stream, and `usageUpdate` would keep
/// every message that happens to carry usage.
///
/// Deliberately the same set the protocol deserializer strips before it
/// picks the oneof arm (`OutputEvent::deserialize` removes `seqNum`,
/// `timestampMicros` and the usage keys) — selection, summary rendering
/// and deserialization should agree on what a message *is*. Excluded from
/// both selector matching and summary rendering for that reason.
fn is_envelope_key(key: &str) -> bool {
    matches!(
        key,
        "seqNum" | "timestampMicros" | "usageMetadata" | "usageUpdate"
    )
}

// =============================================================================
// LoudWirePrinter
// =============================================================================

/// Pretty-prints wire events to stderr.
///
/// This is the inspector installed automatically when the `LOUD_WIRE`
/// environment variable is set at `Client` construction time. Output format:
///
/// - Green `>>>` for outgoing requests, red `<<<` for incoming responses
/// - Timestamps and request ids (`[REQ#N]` / `[RES#N]`) for correlation
/// - Request ids use alternating colors (even/odd) for visual distinction:
///   `[REQ#N]` green (even) / yellow (odd); `[RES#N]` magenta (even) /
///   cyan (odd)
/// - SSE frames labelled in blue
/// - Pretty-printed (and, with the `wire-color` feature, colored) JSON
/// - Base64-heavy `data`/`signature` fields truncated to keep output readable
/// - Secret fields (e.g. third-party retrieval `api_key`s) fully redacted
#[derive(Debug, Clone, Default)]
pub struct LoudWirePrinter {
    filter: WireFilter,
}

impl LoudWirePrinter {
    /// Creates a printer that prints every event, pretty-printed.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            filter: WireFilter::all(),
        }
    }

    /// Creates a printer restricted by `filter` (see [`WireFilter`]).
    #[must_use]
    pub const fn with_filter(filter: WireFilter) -> Self {
        Self { filter }
    }

    /// One-line rendering, for `LOUD_WIRE=summary`. Enough to see the
    /// shape and order of a session without the bodies that make a
    /// harness run unreadable.
    fn print_summary(&self, event: &WireEvent) {
        let (id, label, detail) = match event {
            WireEvent::Request {
                id, method, url, ..
            } => (*id, "REQ", format!("{method} {url}")),
            WireEvent::ResponseStatus { id, status } => (*id, "RES", format!("status {status}")),
            WireEvent::ResponseBody { id, body } => (*id, "RES", Self::payload_keys(body)),
            WireEvent::ErrorBody { id, status, body } => {
                (*id, "ERR", format!("status {status}, {} bytes", body.len()))
            }
            WireEvent::SseFrame { id, event_type, .. } => {
                (*id, "SSE", event_type.clone().unwrap_or_else(|| "-".into()))
            }
            WireEvent::UploadStart {
                id,
                file_name,
                size_bytes,
                ..
            } => (*id, "UP", format!("{file_name} ({size_bytes} bytes)")),
            WireEvent::UploadComplete { id, uri } => (*id, "UP", format!("done {uri}")),
            WireEvent::HarnessSpawn { id, path, pid } => {
                (*id, "HARNESS", format!("{path} (pid {pid:?})"))
            }
            WireEvent::HarnessStderr { id, line } => (*id, "STDERR", line.clone()),
            WireEvent::WsSend { id, payload } => (*id, "WS>", Self::payload_keys(payload)),
            WireEvent::WsReceive { id, payload } => (*id, "WS<", Self::ws_payload_keys(payload)),
        };
        eprintln!(
            "{} {} [#{id}] {label:<7} {detail}",
            paint::bold("[LOUD_WIRE]"),
            Self::timestamp()
        );
    }

    /// The oneof key(s) of a WebSocket payload — the part that says what
    /// the message *is*.
    ///
    /// The harness does send messages whose only non-envelope content is
    /// usage (a bare `seqNum` + `usageUpdate` deserializes with no payload
    /// at all), so an empty key list is a real message and not a bug.
    /// Label it rather than printing a blank detail column, which in the
    /// one format meant for skimming would read as a rendering fault. The
    /// label stays neutral because this also renders HTTP response bodies,
    /// where there is no envelope to speak of — and those stay unqualified
    /// for the same reason (see `ws_payload_keys`).
    fn payload_keys(payload: &serde_json::Value) -> String {
        Self::payload_keys_inner(payload, false)
    }

    /// `payload_keys` for harness WebSocket messages the crate *receives*,
    /// qualifying a step with the action it carried.
    ///
    /// Receive-only on purpose. `InputEvent` arms have no actions, so the
    /// argument that excludes HTTP bodies applies to sends too — and
    /// `questionResponse` carries an object-valued `response` field, which
    /// would both render as `questionResponse/response` and be selected by
    /// `LOUD_WIRE=response`, the category selector for HTTP responses.
    fn ws_payload_keys(payload: &serde_json::Value) -> String {
        Self::payload_keys_inner(payload, true)
    }

    fn payload_keys_inner(payload: &serde_json::Value, harness_receive: bool) -> String {
        payload.as_object().map_or_else(
            || "(non-object)".to_string(),
            |m| {
                let keys: Vec<String> = m
                    .iter()
                    // Envelope stripping is a harness-wire notion. On a
                    // Gemini HTTP response `usageMetadata` is a real
                    // top-level field, and stripping it there could render
                    // a body as `(no payload keys)` instead of naming what
                    // came back.
                    .filter(|(k, _)| !(harness_receive && is_envelope_key(k)))
                    .map(|(key, value)| {
                        // Qualify a harness message with the action it
                        // carried: a bare "stepUpdate" says almost nothing,
                        // while "stepUpdate/mcpTool" says which action ran
                        // — and names exactly what a nested selector can
                        // match, so asking for `mcpTool` cannot produce a
                        // line labelled with a different key.
                        //
                        // WebSocket only. HTTP response bodies share this
                        // renderer and have no actions, so qualifying them
                        // would invent structure that isn't there
                        // (`interaction/outputs`).
                        if !harness_receive {
                            return key.clone();
                        }
                        let actions = nested_action_keys(value);
                        if actions.is_empty() {
                            key.clone()
                        } else {
                            // Every action, not just the first: a step
                            // carries one in practice, but naming only the
                            // lowest-sorting of several would reintroduce
                            // the label/selector mismatch in a rarer shape.
                            format!("{key}/{}", actions.join("+"))
                        }
                    })
                    .collect();
                if keys.is_empty() {
                    "(no payload keys)".to_string()
                } else {
                    keys.join(", ")
                }
            },
        )
    }

    /// Format the current timestamp for log output (ISO 8601 UTC).
    fn timestamp() -> String {
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
    }

    /// Log prefix with timestamp and request ID (for outgoing requests).
    /// Colors alternate: green (even) / yellow (odd) for visual distinction.
    fn request_prefix(request_id: u64) -> String {
        let ts = paint::dimmed(&Self::timestamp());
        let req_label = format!("[REQ#{request_id}]");
        let colored_label = if request_id.is_multiple_of(2) {
            paint::green_bold(&req_label)
        } else {
            paint::yellow_bold(&req_label)
        };
        format!("{} {} {}", paint::bold("[LOUD_WIRE]"), ts, colored_label)
    }

    /// Log prefix with timestamp and response ID (for incoming responses).
    /// Colors alternate: magenta (even) / cyan (odd) for visual distinction.
    fn response_prefix(request_id: u64) -> String {
        let ts = paint::dimmed(&Self::timestamp());
        let res_label = format!("[RES#{request_id}]");
        let colored_label = if request_id.is_multiple_of(2) {
            paint::magenta_bold(&res_label)
        } else {
            paint::cyan_bold(&res_label)
        };
        format!("{} {} {}", paint::bold("[LOUD_WIRE]"), ts, colored_label)
    }

    /// Pretty-print a JSON value line-by-line under the given prefix,
    /// truncating base64-heavy fields.
    fn print_json(prefix: &str, value: &serde_json::Value) {
        let mut value = value.clone();
        truncate_long_fields(&mut value);
        if let Some(colored) = paint::json(&value) {
            for line in colored.lines() {
                eprintln!("{prefix} {line}");
            }
        } else if let Ok(pretty) = serde_json::to_string_pretty(&value) {
            for line in pretty.lines() {
                eprintln!("{prefix} {line}");
            }
        }
    }

    fn print_request(id: u64, method: &str, url: &str, body: Option<&serde_json::Value>) {
        let prefix = Self::request_prefix(id);
        let direction = paint::green_bold(">>>");

        eprintln!("{prefix} {direction} {method} {url}");

        if let Some(body) = body {
            eprintln!("{prefix} {}:", paint::green("Body"));
            Self::print_json(&prefix, body);
        }
    }

    fn print_response_status(id: u64, status: u16) {
        let prefix = Self::response_prefix(id);
        let direction = paint::red_bold("<<<");
        let status_text = if status < 300 {
            paint::green(&format!("{status} OK"))
        } else {
            paint::red(&format!("{status} ERROR"))
        };

        eprintln!("{prefix} {direction} {status_text}");
    }

    fn print_response_body(id: u64, body: &serde_json::Value) {
        let prefix = Self::response_prefix(id);

        // Non-JSON bodies are carried as a top-level string: print raw
        // (truncated for safety) instead of as a JSON-quoted string.
        if let serde_json::Value::String(raw) = body {
            let truncated = truncate_utf8(raw, RAW_BODY_LIMIT);
            eprintln!("{prefix} {}: {truncated}", paint::red("Response"));
            return;
        }

        eprintln!("{prefix} {}:", paint::red("Response"));
        Self::print_json(&prefix, body);
    }

    fn print_error_body(id: u64, status: u16, body: &str) {
        let prefix = Self::response_prefix(id);
        let label = paint::red_bold(&format!("Error ({status})"));

        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(body) {
            eprintln!("{prefix} {label}:");
            Self::print_json(&prefix, &parsed);
        } else {
            let truncated = truncate_utf8(body, RAW_BODY_LIMIT);
            eprintln!("{prefix} {label}: {truncated}");
        }
    }

    fn print_sse_frame(id: u64, event_type: Option<&str>, data: &str) {
        let prefix = Self::response_prefix(id);
        let label = paint::blue_bold("SSE");

        if let Some(event_type) = event_type {
            eprintln!("{prefix} {label} event: {event_type}");
        }

        if data.is_empty() {
            return;
        }

        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
            eprintln!("{prefix} {label}:");
            Self::print_json(&prefix, &parsed);
        } else {
            eprintln!("{prefix} {label}: {data}");
        }
    }

    fn print_upload_start(id: u64, file_name: &str, mime_type: &str, size_bytes: u64) {
        let prefix = Self::request_prefix(id);
        let direction = paint::green_bold(">>>");
        let size_mb = size_bytes as f64 / 1_048_576.0;

        eprintln!(
            "{prefix} {direction} {} \"{file_name}\" ({mime_type}, {size_mb:.2} MB)",
            paint::green_bold("UPLOAD")
        );
    }

    fn print_upload_complete(id: u64, uri: &str) {
        let prefix = Self::response_prefix(id);
        let direction = paint::red_bold("<<<");

        eprintln!(
            "{prefix} {direction} {} {uri}",
            paint::green_bold("UPLOADED")
        );
    }

    fn print_harness_spawn(id: u64, path: &str, pid: Option<u32>) {
        let prefix = Self::request_prefix(id);
        let direction = paint::green_bold(">>>");
        let pid_text = pid.map_or_else(|| "?".to_string(), |p| p.to_string());

        eprintln!(
            "{prefix} {direction} {} {path} (pid {pid_text})",
            paint::green_bold("HARNESS")
        );
    }

    fn print_ws_send(id: u64, payload: &serde_json::Value) {
        let prefix = Self::request_prefix(id);
        let direction = paint::green_bold(">>>");

        eprintln!("{prefix} {direction} {}:", paint::green("WS Send"));
        Self::print_json(&prefix, payload);
    }

    fn print_ws_receive(id: u64, payload: &serde_json::Value) {
        let prefix = Self::response_prefix(id);
        let direction = paint::red_bold("<<<");

        eprintln!("{prefix} {direction} {}:", paint::red("WS Receive"));
        Self::print_json(&prefix, payload);
    }

    fn print_harness_stderr(id: u64, line: &str) {
        let prefix = Self::response_prefix(id);
        let label = paint::blue_bold("STDERR");
        let truncated = truncate_utf8(line, RAW_BODY_LIMIT);

        eprintln!("{prefix} {label}: {truncated}");
    }
}

impl WireInspector for LoudWirePrinter {
    fn on_event(&self, event: &WireEvent) {
        if !self.filter.allows(event) {
            return;
        }
        if self.filter.is_summary() {
            self.print_summary(event);
            return;
        }
        match event {
            WireEvent::Request {
                id,
                method,
                url,
                body,
            } => Self::print_request(*id, method, url, body.as_ref()),
            WireEvent::ResponseStatus { id, status } => Self::print_response_status(*id, *status),
            WireEvent::ResponseBody { id, body } => Self::print_response_body(*id, body),
            WireEvent::ErrorBody { id, status, body } => {
                Self::print_error_body(*id, *status, body);
            }
            WireEvent::SseFrame {
                id,
                event_type,
                data,
            } => Self::print_sse_frame(*id, event_type.as_deref(), data),
            WireEvent::UploadStart {
                id,
                file_name,
                mime_type,
                size_bytes,
            } => Self::print_upload_start(*id, file_name, mime_type, *size_bytes),
            WireEvent::UploadComplete { id, uri } => Self::print_upload_complete(*id, uri),
            WireEvent::HarnessSpawn { id, path, pid } => {
                Self::print_harness_spawn(*id, path, *pid);
            }
            WireEvent::WsSend { id, payload } => Self::print_ws_send(*id, payload),
            WireEvent::WsReceive { id, payload } => Self::print_ws_receive(*id, payload),
            WireEvent::HarnessStderr { id, line } => Self::print_harness_stderr(*id, line),
        }
    }
}

#[cfg(test)]
#[path = "printer_tests.rs"]
mod tests;
