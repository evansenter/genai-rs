//! Test-only support shared across the crate's unit tests.
//!
//! Two things live here:
//!
//! - Scoped `tracing` subscriber helpers, for pinning that a log signal
//!   actually fires — value assertions cannot distinguish a degradation arm
//!   that warns from one that went silent.
//! - [`LoudWireGuard`], which serializes the crate's `LOUD_WIRE` mutators
//!   and restores the ambient value. It lives here rather than beside either
//!   mutator because both `client.rs` and `wire.rs` need it (#418).

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
/// Measured: 30 runs of `cargo test --lib --test-threads=16` produced 19
/// failures, 11 of them `test_add_wire_inspector_accumulates` specifically.
/// The same stress after this guard: 0 in 30. Also the coverage job's flake
/// in #418.
static LOUD_WIRE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Exclusive access to the `LOUD_WIRE` environment variable, restoring
/// whatever was there on drop.
///
/// Restoring rather than clearing matters because `LOUD_WIRE=1 cargo test`
/// is a real invocation: a test that unconditionally removed the variable
/// would leave the rest of the process running without it, changing what
/// unrelated tests observe.
#[must_use = "the guard releases the lock and restores LOUD_WIRE when dropped, \
              so discarding it leaves the code that follows unserialized"]
pub(crate) struct LoudWireGuard {
    _lock: Option<std::sync::MutexGuard<'static, ()>>,
    prior: Option<String>,
}

impl LoudWireGuard {
    /// The one place the ambient value is read.
    ///
    /// Both constructors go through here so the restore-path test exercises
    /// the same capture every production caller gets — two independent
    /// `env::var` reads would let `acquire()` drift (capture after a
    /// mutation, or not at all) with the test still green.
    fn with_lock(lock: Option<std::sync::MutexGuard<'static, ()>>) -> Self {
        Self {
            _lock: lock,
            prior: std::env::var("LOUD_WIRE").ok(),
        }
    }

    /// Takes the lock and records the ambient value.
    ///
    /// Poisoning is ignored deliberately: the lock orders access, it does not
    /// protect an invariant, so a panic in one holder should not cascade into
    /// unrelated failures.
    pub(crate) fn acquire() -> Self {
        Self::with_lock(Some(
            LOUD_WIRE_LOCK.lock().unwrap_or_else(|e| e.into_inner()),
        ))
    }

    /// Same save/restore behavior, but without taking the lock — for a caller
    /// that already holds it.
    ///
    /// The mutex is not reentrant, so the test that asserts on the restore
    /// path cannot call [`LoudWireGuard::acquire`] a second time to get a
    /// guard to observe. It holds one real guard for its duration and builds
    /// the guards under test with this.
    fn nested() -> Self {
        Self::with_lock(None)
    }

    /// Sets `LOUD_WIRE` until the next mutation on this guard, or until it
    /// drops — whichever comes first.
    pub(crate) fn set(&self, value: &str) {
        // SAFETY: test-only env mutation, serialized against every other
        // LOUD_WIRE *mutator* by the lock this guard holds.
        //
        // What the lock cannot exclude is a concurrent env *reader*. Building
        // a `Client` also constructs a `reqwest::Client`, which reads
        // `HTTP_PROXY`/`NO_PROXY` via `std::env::var`, and any test in the
        // binary can be inside that call while this `setenv` runs — glibc may
        // free the `environ` entry a concurrent `getenv` is still reading,
        // which is the UB that made `set_var` unsafe in Rust 2024. No
        // LOUD_WIRE-scoped lock can order that.
        //
        // Left as a note rather than fixed: these tests mutated the
        // environment before this change and it strictly narrows the window,
        // and nextest's process-per-test confines the hazard to plain
        // `cargo test`.
        unsafe { std::env::set_var("LOUD_WIRE", value) };
    }

    /// Clears `LOUD_WIRE` until the next mutation on this guard, or until it
    /// drops — whichever comes first.
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

#[cfg(test)]
mod loud_wire_guard_tests {
    use super::LoudWireGuard;

    /// The restore-on-drop path, which is the subtlest part of the guard and
    /// was otherwise unasserted.
    #[test]
    fn restores_the_ambient_value_on_drop() {
        // Held for the whole test. It serializes against the other
        // LOUD_WIRE-sensitive tests, and its own drop puts the real ambient
        // value back afterwards. The guards under test are built with
        // `nested()` because the mutex is not reentrant.
        let ambient = LoudWireGuard::acquire();

        // A guard taken over a set value must put that value back.
        ambient.set("ambient-value");
        {
            let guard = LoudWireGuard::nested();
            guard.set("temporary");
            assert_eq!(
                std::env::var("LOUD_WIRE").as_deref(),
                Ok("temporary"),
                "set() should take effect while the guard is held"
            );
        }
        assert_eq!(
            std::env::var("LOUD_WIRE").as_deref(),
            Ok("ambient-value"),
            "drop must restore the value the guard found, not clear it"
        );

        // And an absent one must come back absent.
        ambient.unset();
        {
            let guard = LoudWireGuard::nested();
            guard.set("temporary");
            assert_eq!(
                std::env::var("LOUD_WIRE").as_deref(),
                Ok("temporary"),
                "set() should take effect while the guard is held"
            );
        }
        assert!(
            std::env::var("LOUD_WIRE").is_err(),
            "drop must clear when the guard found nothing set"
        );
    }
}
