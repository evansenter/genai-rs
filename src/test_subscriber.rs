//! Test-only scoped `tracing` subscriber helpers, for pinning that a
//! log signal actually fires — value assertions cannot distinguish a
//! degradation arm that warns from one that went silent.

use std::sync::{Arc, Mutex};
use tracing::span;

/// Minimal subscriber recording one extracted string per event.
struct Recorder {
    extract: fn(&tracing::Event<'_>) -> String,
    out: Arc<Mutex<Vec<String>>>,
}

impl tracing::Subscriber for Recorder {
    fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _span: &span::Attributes<'_>) -> span::Id {
        span::Id::from_u64(1)
    }
    fn record(&self, _span: &span::Id, _values: &span::Record<'_>) {}
    fn record_follows_from(&self, _span: &span::Id, _follows: &span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        self.out.lock().unwrap().push((self.extract)(event));
    }
    fn enter(&self, _span: &span::Id) {}
    fn exit(&self, _span: &span::Id) {}
}

fn capture(extract: fn(&tracing::Event<'_>) -> String, f: impl FnOnce()) -> Vec<String> {
    let out = Arc::new(Mutex::new(Vec::new()));
    let recorder = Recorder {
        extract,
        out: Arc::clone(&out),
    };
    tracing::subscriber::with_default(recorder, || {
        // Other tests exercise the same callsites with no subscriber
        // installed, which can cache their interest as `never`
        // process-wide. Under nextest (process per test) that's
        // invisible, but under plain `cargo test` (e.g. the coverage
        // job) the cache is shared across threads — rebuild so the
        // scoped Recorder is consulted.
        tracing::callsite::rebuild_interest_cache();
        f();
    });
    let out = out.lock().unwrap();
    out.clone()
}

/// Runs `f` under a scoped subscriber and returns each event's `message`
/// field rendered via its `Debug` impl.
pub(crate) fn capture_messages(f: impl FnOnce()) -> Vec<String> {
    fn extract(event: &tracing::Event<'_>) -> String {
        struct MessageVisitor<'a>(&'a mut String);
        impl tracing::field::Visit for MessageVisitor<'_> {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    use std::fmt::Write;
                    let _ = write!(self.0, "{value:?}");
                }
            }
        }
        let mut message = String::new();
        event.record(&mut MessageVisitor(&mut message));
        message
    }
    capture(extract, f)
}

/// Runs `f` under a scoped subscriber and returns each event's target.
pub(crate) fn capture_targets(f: impl FnOnce()) -> Vec<String> {
    fn extract(event: &tracing::Event<'_>) -> String {
        event.metadata().target().to_string()
    }
    capture(extract, f)
}
