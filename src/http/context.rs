//! Shared per-client context for the HTTP layer.

use crate::wire::{WireEvent, WireInspector};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Everything the HTTP layer needs to make and observe API requests:
/// the reqwest client, the API key, the installed wire inspectors, and a
/// per-client request-id counter for correlating wire events.
#[derive(Clone)]
pub struct HttpContext {
    pub http_client: reqwest::Client,
    pub api_key: String,
    pub inspectors: Arc<[Arc<dyn WireInspector>]>,
    request_counter: Arc<AtomicU64>,
}

// Custom Debug that redacts the API key and elides inspectors.
impl std::fmt::Debug for HttpContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpContext")
            .field("http_client", &self.http_client)
            .field("api_key", &"[REDACTED]")
            .field("inspectors", &self.inspectors.len())
            .finish_non_exhaustive()
    }
}

impl HttpContext {
    /// Creates a new context. The request-id counter starts at 1.
    pub fn new(
        http_client: reqwest::Client,
        api_key: String,
        inspectors: Vec<Arc<dyn WireInspector>>,
    ) -> Self {
        Self {
            http_client,
            api_key,
            inspectors: inspectors.into(),
            request_counter: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Returns true if any wire inspectors are installed.
    ///
    /// Call sites use this to skip event construction (in particular request
    /// body serialization) when nobody is listening.
    #[must_use]
    pub fn has_inspectors(&self) -> bool {
        !self.inspectors.is_empty()
    }

    /// Returns the next request id for wire-event correlation.
    #[must_use]
    pub fn next_request_id(&self) -> u64 {
        self.request_counter.fetch_add(1, Ordering::Relaxed)
    }

    /// Fans a wire event out to every installed inspector.
    ///
    /// Cheap when no inspectors are installed, but callers should still guard
    /// any allocation-heavy event construction with [`Self::has_inspectors`].
    pub fn emit(&self, event: WireEvent) {
        for inspector in self.inspectors.iter() {
            inspector.on_event(&event);
        }
    }

    /// Serializes a request body for wire inspection.
    ///
    /// Returns `None` when no inspectors are installed (avoiding the
    /// serialization cost entirely) or when serialization fails (logged
    /// at `warn`).
    #[must_use]
    pub fn serialize_wire_body<B: serde::Serialize>(&self, body: &B) -> Option<serde_json::Value> {
        if !self.has_inspectors() {
            return None;
        }
        match serde_json::to_value(body) {
            Ok(value) => Some(value),
            Err(e) => {
                tracing::warn!(
                    "Failed to serialize request body for wire inspection: {}",
                    e
                );
                None
            }
        }
    }

    /// Emits a [`WireEvent::Request`]. No-op when no inspectors are installed.
    pub fn emit_request(&self, id: u64, method: &str, url: &str, body: Option<serde_json::Value>) {
        if !self.has_inspectors() {
            return;
        }
        self.emit(WireEvent::Request {
            id,
            method: method.to_string(),
            url: url.to_string(),
            body,
        });
    }

    /// Emits a [`WireEvent::ResponseBody`], parsing the body as JSON.
    /// Non-JSON bodies are preserved as a JSON string. No-op when no
    /// inspectors are installed.
    pub fn emit_response_body(&self, id: u64, body_text: &str) {
        if !self.has_inspectors() {
            return;
        }
        let body = serde_json::from_str::<serde_json::Value>(body_text)
            .unwrap_or_else(|_| serde_json::Value::String(body_text.to_string()));
        self.emit(WireEvent::ResponseBody { id, body });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Test inspector that records every event it receives.
    struct Collector {
        events: Mutex<Vec<WireEvent>>,
    }

    impl Collector {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                events: Mutex::new(Vec::new()),
            })
        }

        fn kinds(&self) -> Vec<String> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .map(|e| {
                    serde_json::to_value(e).unwrap()["kind"]
                        .as_str()
                        .unwrap()
                        .to_string()
                })
                .collect()
        }
    }

    impl WireInspector for Collector {
        fn on_event(&self, event: &WireEvent) {
            self.events.lock().unwrap().push(event.clone());
        }
    }

    fn test_ctx(inspectors: Vec<Arc<dyn WireInspector>>) -> HttpContext {
        HttpContext::new(reqwest::Client::new(), "test-key".to_string(), inspectors)
    }

    #[test]
    fn test_request_ids_increment_from_one() {
        let ctx = test_ctx(vec![]);
        assert_eq!(ctx.next_request_id(), 1);
        assert_eq!(ctx.next_request_id(), 2);
        assert_eq!(ctx.next_request_id(), 3);
    }

    #[test]
    fn test_request_ids_shared_across_clones() {
        let ctx = test_ctx(vec![]);
        let clone = ctx.clone();
        assert_eq!(ctx.next_request_id(), 1);
        assert_eq!(clone.next_request_id(), 2);
        assert_eq!(ctx.next_request_id(), 3);
    }

    #[test]
    fn test_has_inspectors() {
        assert!(!test_ctx(vec![]).has_inspectors());
        assert!(test_ctx(vec![Collector::new()]).has_inspectors());
    }

    #[test]
    fn test_emit_fans_out_to_all_inspectors() {
        let first = Collector::new();
        let second = Collector::new();
        let ctx = test_ctx(vec![first.clone(), second.clone()]);

        ctx.emit(WireEvent::ResponseStatus { id: 1, status: 200 });
        ctx.emit(WireEvent::SseFrame {
            id: 1,
            event_type: None,
            data: "{}".to_string(),
        });

        for collector in [&first, &second] {
            assert_eq!(collector.kinds(), ["response_status", "sse_frame"]);
        }
    }

    #[test]
    fn test_emit_with_no_inspectors_is_noop() {
        let ctx = test_ctx(vec![]);
        // Must not panic or block.
        ctx.emit(WireEvent::ResponseStatus { id: 1, status: 200 });
    }

    #[test]
    fn test_debug_redacts_api_key() {
        let ctx = test_ctx(vec![]);
        let debug = format!("{ctx:?}");
        assert!(!debug.contains("test-key"));
        assert!(debug.contains("[REDACTED]"));
    }
}
