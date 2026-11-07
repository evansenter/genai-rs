//! [`PollOptions`] and the wait behind every `wait_until_active`
//! ([`Files`](crate::Files) and file-search documents).

use crate::errors::GenaiError;
use std::time::Duration;

/// How long a wait such as
/// [`Files::wait_until_active`](crate::Files::wait_until_active) lasts, and
/// how often it polls. A value left unset takes that method's default.
///
/// ```
/// use genai_rs::PollOptions;
/// use std::time::Duration;
///
/// let poll = PollOptions::new()
///     .with_timeout(Duration::from_secs(300))
///     .with_poll_interval(Duration::from_secs(5));
/// # let _ = poll;
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PollOptions {
    timeout: Option<Duration>,
    poll_interval: Option<Duration>,
}

impl PollOptions {
    /// The shortest interval a wait polls at: 10 ms. A shorter one, zero
    /// included, is raised to it.
    ///
    /// A zero interval would send each status request as soon as the last
    /// one answered, for as long as the timeout allows. 10 ms is far below
    /// any status request's round trip, so it slows no real wait.
    pub const MIN_POLL_INTERVAL: Duration = Duration::from_millis(10);

    /// The waiting method's defaults for both values.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            timeout: None,
            poll_interval: None,
        }
    }

    /// Gives up once `timeout` has passed since the wait started, and never
    /// later: each status request is cut off at the deadline, and a poll
    /// that would start past it isn't made (the wait returns then instead).
    ///
    /// The timeout must be longer than one status request's round trip, or
    /// even a resource that is already active can't be seen: the first
    /// request is cut off too, and the error says so.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Sleeps `poll_interval` between status requests, or longer when a
    /// rate-limited (429) status request carries a `Retry-After`. An
    /// interval under [`MIN_POLL_INTERVAL`](Self::MIN_POLL_INTERVAL)
    /// (10 ms), such as `Duration::ZERO`, polls at that minimum instead.
    #[must_use]
    pub const fn with_poll_interval(mut self, poll_interval: Duration) -> Self {
        self.poll_interval = Some(poll_interval);
        self
    }

    /// The timeout, or `default` when unset.
    pub(crate) fn timeout_or(self, default: Duration) -> Duration {
        self.timeout.unwrap_or(default)
    }

    /// The poll interval, or `default` when unset, raised to
    /// [`MIN_POLL_INTERVAL`](Self::MIN_POLL_INTERVAL).
    pub(crate) fn poll_interval_or(self, default: Duration) -> Duration {
        self.poll_interval
            .unwrap_or(default)
            .max(Self::MIN_POLL_INTERVAL)
    }

    /// Polls `get` until `check` reports the resource ready, `check` or the
    /// API fails it, or the deadline passes. The waiting behind every
    /// `wait_until_active`.
    ///
    /// The deadline is never overshot. Each status request is bounded by the
    /// time left, so one hung request can't outlive it (the default client
    /// has no read timeout), and the wait gives up as soon as the next poll
    /// would start at or past it rather than sleeping into it. The first
    /// status request is always made, so a zero timeout still reports an
    /// error `get` returns at once (an invalid name). A request the deadline
    /// cuts off is reported as cut off, not as failed: with a timeout
    /// shorter than one round trip, that is how every wait ends.
    ///
    /// A retryable error (a 5xx, a 429, a dropped connection) is logged and
    /// polled through rather than ending a long wait; any other error returns
    /// at once. When that error carries a `Retry-After`, the next poll waits
    /// at least that long.
    ///
    /// Time is tokio time, so the wait runs under a paused test clock. The
    /// deadline error is [`GenaiError::PollTimeout`], whose `is_retryable()`
    /// is false: the resource may still be processing, and retrying the
    /// whole wait is the caller's call.
    pub(crate) async fn wait<T, Fut>(
        self,
        what: &str,
        defaults: (Duration, Duration),
        get: impl FnMut() -> Fut,
        check: impl FnMut(T) -> Result<PollStep<T>, GenaiError>,
    ) -> Result<T, GenaiError>
    where
        Fut: std::future::Future<Output = Result<T, GenaiError>>,
    {
        self.wait_with_advice(
            what,
            "It may still be processing - try again with a longer timeout.",
            defaults,
            get,
            check,
        )
        .await
    }

    /// [`wait`](Self::wait), ending its deadline error with `advice` instead
    /// of the advice to wait again, for a caller whose retry is not a wait
    /// (an import, which a second call repeats).
    pub(crate) async fn wait_with_advice<T, Fut>(
        self,
        what: &str,
        advice: &str,
        defaults: (Duration, Duration),
        mut get: impl FnMut() -> Fut,
        mut check: impl FnMut(T) -> Result<PollStep<T>, GenaiError>,
    ) -> Result<T, GenaiError>
    where
        Fut: std::future::Future<Output = Result<T, GenaiError>>,
    {
        let timeout = self.timeout_or(defaults.0);
        let interval = self.poll_interval_or(defaults.1);
        let start = tokio::time::Instant::now();
        // `None` for a timeout too large to add to an instant: no deadline.
        let deadline = start.checked_add(timeout);
        let time_left = || {
            deadline.map_or(Duration::MAX, |deadline| {
                deadline.saturating_duration_since(tokio::time::Instant::now())
            })
        };
        let mut last_state = "unknown".to_string();
        loop {
            let bound = time_left();
            // What went wrong with this round's status request, if anything,
            // so a deadline caused by an outage, or by the deadline itself,
            // says so.
            let mut retry_after = None;
            let last_problem = match tokio::time::timeout(bound, get()).await {
                Ok(Ok(current)) => {
                    match check(current)? {
                        PollStep::Ready(done) => return Ok(done),
                        PollStep::Pending(state) => last_state = state,
                    }
                    None
                }
                Ok(Err(e)) if e.is_retryable() => {
                    tracing::warn!("Polling {what} failed ({e}); retrying until the deadline");
                    retry_after = e.retry_after();
                    Some(format!("the last status request failed: {e}"))
                }
                Ok(Err(e)) => return Err(e),
                // Nothing failed: the wait's own deadline ended the request.
                Err(_) => {
                    tracing::warn!("Polling {what}: status request cut off at the deadline");
                    Some(format!(
                        "the last status request was cut off by the wait's deadline, \
                         unanswered after {bound:?}"
                    ))
                }
            };

            let delay = retry_after.map_or(interval, |asked| asked.max(interval));
            if delay >= time_left() {
                let waited = start.elapsed();
                let mut cause = last_problem
                    .map(|problem| format!("; {problem}"))
                    .unwrap_or_default();
                if let Some(asked) = retry_after.filter(|asked| *asked > interval) {
                    cause.push_str(&format!(
                        "; the server asked to retry after {asked:?}, past the deadline"
                    ));
                }
                return Err(GenaiError::PollTimeout {
                    timeout,
                    message: format!(
                        "Timeout waiting for {what} to become active (timeout {timeout:?}, \
                         waited {waited:?}, last state: {last_state}{cause}). {advice}"
                    ),
                });
            }
            tracing::debug!("{what} not active yet, waiting {delay:?}...");
            tokio::time::sleep(delay).await;
        }
    }
}

/// What one status read during [`PollOptions::wait`] found.
pub(crate) enum PollStep<T> {
    /// Done: the wait returns it.
    Ready(T),
    /// Not yet. The string names its state, for the timeout message.
    Pending(String),
}

#[cfg(test)]
#[path = "poll_tests.rs"]
mod tests;
