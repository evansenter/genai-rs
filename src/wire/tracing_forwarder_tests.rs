use super::*;
use crate::wire::TRACING_TARGET;

#[test]
fn test_tracing_forwarder_body_rendering_redacts() {
    // TracingForwarder must apply the same redaction guarantees as
    // LoudWirePrinter to JSON bodies and raw string payloads.
    let body = serde_json::json!({"secret": "whsec_x", "api_key": "k"});
    let rendered = redacted_body_string(&body);
    assert!(!rendered.contains("whsec_x"));
    assert!(!rendered.contains("\"k\""));

    let raw_json = r#"{"new_signing_secret":"whsec_y"}"#;
    assert!(!redacted_raw_string(raw_json).contains("whsec_y"));

    let credential = serde_json::json!({
        "type": "environment_variable", "value": "val-z", "token": "tok-z",
        "env": {"V": {"value": "env-z"}},
    });
    let rendered = redacted_body_string(&credential);
    for secret in ["val-z", "tok-z", "env-z"] {
        assert!(!rendered.contains(secret), "{secret} leaked: {rendered}");
    }

    // Non-JSON payloads pass through unchanged.
    assert_eq!(redacted_raw_string("plain text"), "plain text");
}

#[test]
fn test_tracing_forwarder_emits_to_wire_target() {
    let targets = crate::test_subscriber::capture_targets(|| {
        TracingForwarder::new().on_event(&WireEvent::ResponseStatus { id: 7, status: 200 });
    });
    assert_eq!(targets.as_slice(), [TRACING_TARGET]);
}

/// Every field of every event `f` emits, rendered `name=value`.
fn capture_fields(f: impl FnOnce()) -> Vec<String> {
    use std::sync::{Arc, Mutex};
    use tracing::span;

    struct Fields(Arc<Mutex<Vec<String>>>);
    impl tracing::Subscriber for Fields {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &span::Attributes<'_>) -> span::Id {
            span::Id::from_u64(1)
        }
        fn record(&self, _: &span::Id, _: &span::Record<'_>) {}
        fn record_follows_from(&self, _: &span::Id, _: &span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            struct Render(String);
            impl tracing::field::Visit for Render {
                fn record_debug(
                    &mut self,
                    field: &tracing::field::Field,
                    value: &dyn std::fmt::Debug,
                ) {
                    use std::fmt::Write;
                    let _ = write!(self.0, "{}={value:?} ", field.name());
                }
            }
            let mut render = Render(String::new());
            event.record(&mut render);
            self.0.lock().unwrap().push(render.0);
        }
        fn enter(&self, _: &span::Id) {}
        fn exit(&self, _: &span::Id) {}
    }

    let out = Arc::new(Mutex::new(Vec::new()));
    tracing::subscriber::with_default(Fields(Arc::clone(&out)), || {
        tracing::callsite::rebuild_interest_cache();
        f();
    });
    let out = out.lock().unwrap();
    out.clone()
}

#[test]
fn test_tracing_forwarder_redacts_harness_messages_both_ways() {
    // A message the harness sends back is logged through the same walk as
    // one sent to it (#555 item 2): a WsReceive reaches the forwarder as
    // received.
    let payload = serde_json::json!({
        "config": {"models": [{"geminiApiEndpoint": {
            "apiKey": "ws-secret-1",
            "httpHeaders": {"X-Auth": "ws-secret-2"}
        }}]},
        "mcpServers": [{"stdio": {"env": {"TOKEN_NAME": "ws-secret-3"}}}],
    });
    let lines = capture_fields(|| {
        let forwarder = TracingForwarder::new();
        forwarder.on_event(&WireEvent::WsReceive {
            id: 1,
            payload: payload.clone(),
        });
        forwarder.on_event(&WireEvent::WsSend {
            id: 1,
            payload: payload.clone(),
        });
    });
    assert_eq!(lines.len(), 2, "{lines:?}");
    for line in &lines {
        assert!(!line.contains("ws-secret-"), "leaked: {line}");
        for name in ["X-Auth", "TOKEN_NAME", "[REDACTED]"] {
            assert!(line.contains(name), "{name}: {line}");
        }
    }
}
