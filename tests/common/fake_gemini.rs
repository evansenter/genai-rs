//! A scripted Gemini model for key-free Antigravity harness tests.
//!
//! The harness makes its model calls itself, to
//! `{base_url}/v1beta/models/<model>:streamGenerateContent?alt=sse`. Pointed
//! here with `AgentBuilder::with_gemini_base_url`, it gets the scripted
//! replies below instead of a real model's, so a test drives the real
//! harness (builtins, hooks, containment, triggers, cancellation) with no
//! API key and a deterministic outcome. Built on the HTTP stub in
//! `http_stub.rs`.

use std::sync::OnceLock;
use std::time::Duration;

use serde_json::{Value, json};

use super::http_stub::{Recorded, Reply, Stub};

/// One scripted model response.
#[derive(Clone, Debug)]
pub enum ModelTurn {
    /// The model answers with text.
    Text(String),
    /// The model calls a function (a builtin like `view_file`, or a custom
    /// or MCP tool) with these arguments.
    Call { name: String, args: Value },
    /// The backend fails with this HTTP status and message.
    Error { status: u16, message: String },
    /// The same turn, answered only after a delay.
    Delayed(Duration, Box<ModelTurn>),
}

impl ModelTurn {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    pub fn call(name: impl Into<String>, args: Value) -> Self {
        Self::Call {
            name: name.into(),
            args,
        }
    }

    pub fn error(status: u16, message: impl Into<String>) -> Self {
        Self::Error {
            status,
            message: message.into(),
        }
    }

    pub fn delayed(self, delay: Duration) -> Self {
        Self::Delayed(delay, Box::new(self))
    }

    fn reply(&self) -> Reply {
        match self {
            Self::Text(text) => sse_reply(json!([{"text": text}])),
            Self::Call { name, args } => {
                sse_reply(json!([{"functionCall": {"name": name, "args": args}}]))
            }
            Self::Error { status, message } => Reply::json(
                *status,
                json!({"error": {"code": status, "message": message, "status": "INVALID_ARGUMENT"}}),
            ),
            Self::Delayed(delay, turn) => turn.reply().delayed(*delay),
        }
    }
}

/// One `streamGenerateContent` SSE event carrying `parts`, then EOF.
fn sse_reply(parts: Value) -> Reply {
    let chunk = json!({
        "candidates": [{
            "content": {"role": "model", "parts": parts},
            "finishReason": "STOP",
            "index": 0
        }],
        "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": 1, "totalTokenCount": 2},
        "modelVersion": "scripted"
    });
    Reply {
        status: 200,
        headers: vec![("content-type".into(), "text/event-stream".into())],
        delay: Duration::ZERO,
        parts: vec![(
            Duration::ZERO,
            format!("data: {chunk}\r\n\r\n").into_bytes(),
        )],
        content_length: true,
    }
}

/// A local server answering model request `n` with `script[n]`; the last
/// turn repeats for any request past the end of the script.
pub struct FakeGemini {
    stub: Stub,
    /// The harness's app data directory for this test, made on first use and
    /// removed when the fake drops ([`FakeGemini::app_data_dir`]).
    app_data: OnceLock<tempfile::TempDir>,
}

impl FakeGemini {
    pub async fn start(script: Vec<ModelTurn>) -> Self {
        assert!(!script.is_empty(), "a script needs at least one turn");
        let stub = Stub::start(move |_, n| script[n.min(script.len() - 1)].reply()).await;
        Self {
            stub,
            app_data: OnceLock::new(),
        }
    }

    /// A model whose reply is chosen per request by `route` (handed the
    /// request and its 0-based index), for tests where other model calls
    /// interleave with the agent's own: the auto policy's assessor, a
    /// subagent on its own model, the image model.
    pub async fn routed(
        route: impl Fn(&Recorded, usize) -> ModelTurn + Send + Sync + 'static,
    ) -> Self {
        let stub = Stub::start(move |request, n| route(request, n).reply()).await;
        Self {
            stub,
            app_data: OnceLock::new(),
        }
    }

    /// The URL to hand to `with_gemini_base_url`.
    pub fn base_url(&self) -> &str {
        &self.stub.base_url
    }

    /// A directory to hand to `with_app_data_dir`, one per fake and so per
    /// test, removed when the fake drops. Without one the harness keeps each
    /// conversation's working files (`brain/<conversation>/`) under the
    /// runner's `~/.gemini/antigravity`, and every run would leave them
    /// there.
    pub fn app_data_dir(&self) -> String {
        self.app_data
            .get_or_init(|| {
                tempfile::Builder::new()
                    .prefix("agy-scripted-appdata")
                    .tempdir()
                    .expect("create the app data dir")
            })
            .path()
            .to_string_lossy()
            .into_owned()
    }

    /// Every model request so far, in order.
    pub fn requests(&self) -> Vec<Recorded> {
        self.stub.requests()
    }

    /// The JSON body of request `n`, or `None` before it arrives.
    pub fn request_json(&self, n: usize) -> Option<Value> {
        self.requests().get(n).map(Recorded::json)
    }

    /// The `functionResponse` objects in request `n`: what the harness told
    /// the model each tool call returned.
    pub fn function_responses(&self, n: usize) -> Vec<Value> {
        let Some(body) = self.request_json(n) else {
            return Vec::new();
        };
        body["contents"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|content| content["parts"].as_array().into_iter().flatten())
            .filter_map(|part| part.get("functionResponse").cloned())
            .collect()
    }

    /// The system prompt the harness sent with request `n` (its text
    /// parts, joined), or an empty string.
    pub fn system_prompt(&self, n: usize) -> String {
        self.request_json(n)
            .and_then(|body| {
                body["systemInstruction"]["parts"].as_array().map(|parts| {
                    parts
                        .iter()
                        .filter_map(|part| part["text"].as_str())
                        .collect::<String>()
                })
            })
            .unwrap_or_default()
    }

    /// The names of the tools the harness declared to the model in request
    /// `n`.
    pub fn tool_names(&self, n: usize) -> Vec<String> {
        let Some(body) = self.request_json(n) else {
            return Vec::new();
        };
        body["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|tool| {
                tool["functionDeclarations"]
                    .as_array()
                    .into_iter()
                    .flatten()
            })
            .filter_map(|declaration| declaration["name"].as_str().map(str::to_string))
            .collect()
    }

    /// Waits up to `timeout` for at least `count` model requests.
    pub async fn wait_for_requests(&self, count: usize, timeout: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        while self.requests().len() < count {
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        true
    }
}
