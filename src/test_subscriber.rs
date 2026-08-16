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

// =============================================================================
// LOUD_WIRE serialization
// =============================================================================

/// Serializes tests whose result depends on `LOUD_WIRE`.
///
/// Env vars are process-global while `Client::builder().build()` reads
/// `LOUD_WIRE` at construction — so *every* test that builds a client reads
/// it, not just the ones that set it. Under nextest (process per test) that
/// never shows; under plain `cargo test` a client built concurrently with a
/// set/remove window picks up a stray `LoudWirePrinter` and its inspector
/// count is off by one.
///
/// Observed as ~11 failures in 25 runs of `cargo test --lib --test-threads=16`,
/// and as the coverage job's flake in #418.
static LOUD_WIRE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Exclusive access to the `LOUD_WIRE` environment variable, restoring
/// whatever was there on drop.
///
/// Restoring rather than clearing matters because `LOUD_WIRE=1 cargo test`
/// is a real invocation: a test that unconditionally removed the variable
/// would leave the rest of the process running without it, changing what
/// unrelated tests observe.
pub(crate) struct LoudWireGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    prior: Option<String>,
}

impl LoudWireGuard {
    /// Takes the lock and records the ambient value.
    ///
    /// Poisoning is ignored deliberately: the lock orders access, it does not
    /// protect an invariant, so a panic in one holder should not cascade into
    /// unrelated failures.
    pub(crate) fn acquire() -> Self {
        let lock = LOUD_WIRE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        Self {
            _lock: lock,
            prior: std::env::var("LOUD_WIRE").ok(),
        }
    }

    /// Sets `LOUD_WIRE` for the lifetime of this guard.
    pub(crate) fn set(&self, value: &str) {
        // SAFETY: test-only env mutation, serialized against every other
        // LOUD_WIRE-sensitive test by the lock this guard holds.
        unsafe { std::env::set_var("LOUD_WIRE", value) };
    }

    /// Clears `LOUD_WIRE` for the lifetime of this guard.
    pub(crate) fn unset(&self) {
        // SAFETY: as above.
        unsafe { std::env::remove_var("LOUD_WIRE") };
    }
}

impl Drop for LoudWireGuard {
    fn drop(&mut self) {
        // SAFETY: as above — still holding the lock at drop time.
        match &self.prior {
            Some(value) => unsafe { std::env::set_var("LOUD_WIRE", value) },
            None => unsafe { std::env::remove_var("LOUD_WIRE") },
        }
    }
}
