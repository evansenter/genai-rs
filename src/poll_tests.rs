use super::*;

#[test]
fn poll_options_fall_back_to_the_given_defaults() {
    let (timeout, interval) = (Duration::from_secs(120), Duration::from_secs(2));
    let unset = PollOptions::new();
    assert_eq!(unset, PollOptions::default());
    assert_eq!(unset.timeout_or(timeout), timeout);
    assert_eq!(unset.poll_interval_or(interval), interval);

    let set = PollOptions::new()
        .with_timeout(Duration::from_secs(5))
        .with_poll_interval(Duration::from_millis(250))
        // `with_*` replaces.
        .with_timeout(Duration::from_secs(9));
    assert_eq!(set.timeout_or(timeout), Duration::from_secs(9));
    assert_eq!(set.poll_interval_or(interval), Duration::from_millis(250));

    // Each value falls back on its own.
    let only_interval = PollOptions::new().with_poll_interval(Duration::from_secs(1));
    assert_eq!(only_interval.timeout_or(timeout), timeout);
}

// =============================================================================
// PollOptions::wait on the paused clock
// =============================================================================

/// One scripted status read: a pending state, ready, or an error.
enum Read {
    Pending,
    Ready,
    Fail(GenaiError),
    /// Never answers.
    Hang,
}

fn rate_limited(retry_after: Option<Duration>) -> GenaiError {
    GenaiError::Api {
        status_code: 429,
        message: "RESOURCE_EXHAUSTED: slow down".to_string(),
        request_id: None,
        retry_after,
    }
}

/// Runs `wait` over `script` (the last read repeats) and returns its
/// result plus when, on the paused clock, each status read started.
async fn scripted_wait(
    poll: PollOptions,
    script: Vec<Read>,
) -> (Result<&'static str, GenaiError>, Vec<Duration>) {
    let start = tokio::time::Instant::now();
    let reads = std::cell::RefCell::new(Vec::new());
    let script = std::cell::RefCell::new(std::collections::VecDeque::from(script));
    let result = poll
        .wait(
            "resource",
            (Duration::from_secs(120), Duration::from_secs(2)),
            || {
                reads.borrow_mut().push(start.elapsed());
                let mut script = script.borrow_mut();
                let read = if script.len() > 1 {
                    script.pop_front().unwrap()
                } else {
                    match script.front().unwrap() {
                        Read::Pending => Read::Pending,
                        Read::Ready => Read::Ready,
                        Read::Hang => Read::Hang,
                        // `GenaiError` isn't `Clone`; the scripts only
                        // repeat these two kinds.
                        Read::Fail(GenaiError::InvalidInput(m)) => {
                            Read::Fail(GenaiError::InvalidInput(m.clone()))
                        }
                        Read::Fail(e) => Read::Fail(rate_limited(e.retry_after())),
                    }
                };
                async move {
                    match read {
                        Read::Pending => Ok("pending"),
                        Read::Ready => Ok("ready"),
                        Read::Fail(e) => Err(e),
                        Read::Hang => std::future::pending().await,
                    }
                }
            },
            |state| {
                Ok(if state == "ready" {
                    PollStep::Ready(state)
                } else {
                    PollStep::Pending(state.to_string())
                })
            },
        )
        .await;
    (result, reads.into_inner())
}

fn secs(values: &[u64]) -> Vec<Duration> {
    values.iter().map(|&s| Duration::from_secs(s)).collect()
}

#[tokio::test(start_paused = true)]
async fn wait_polls_on_the_interval_until_ready() {
    let poll = PollOptions::new().with_poll_interval(Duration::from_secs(3));
    let (result, reads) =
        scripted_wait(poll, vec![Read::Pending, Read::Pending, Read::Ready]).await;
    assert_eq!(result.unwrap(), "ready");
    assert_eq!(reads, secs(&[0, 3, 6]));
}

#[tokio::test(start_paused = true)]
async fn wait_honors_retry_after_on_a_429() {
    let poll = PollOptions::new().with_poll_interval(Duration::from_secs(1));
    let (result, reads) = scripted_wait(
        poll,
        vec![
            Read::Pending,
            Read::Fail(rate_limited(Some(Duration::from_secs(7)))),
            Read::Ready,
        ],
    )
    .await;
    assert_eq!(result.unwrap(), "ready");
    // 1 s interval, then the 7 s the 429 asked for.
    assert_eq!(reads, secs(&[0, 1, 8]));
}

#[tokio::test(start_paused = true)]
async fn wait_keeps_its_interval_when_retry_after_is_shorter_or_absent() {
    let poll = PollOptions::new().with_poll_interval(Duration::from_secs(4));
    let (result, reads) = scripted_wait(
        poll,
        vec![
            Read::Fail(rate_limited(Some(Duration::from_secs(1)))),
            Read::Fail(rate_limited(None)),
            Read::Ready,
        ],
    )
    .await;
    assert_eq!(result.unwrap(), "ready");
    assert_eq!(reads, secs(&[0, 4, 8]));
}

#[tokio::test(start_paused = true)]
async fn wait_gives_up_now_when_retry_after_passes_the_deadline() {
    let poll = PollOptions::new()
        .with_timeout(Duration::from_secs(30))
        .with_poll_interval(Duration::from_secs(1));
    let start = tokio::time::Instant::now();
    let (result, reads) = scripted_wait(
        poll,
        vec![Read::Fail(rate_limited(Some(Duration::from_secs(60))))],
    )
    .await;
    let err = result.unwrap_err();
    assert_eq!(reads, secs(&[0]), "no poll before the server's delay");
    assert_eq!(start.elapsed(), Duration::ZERO, "no sleep it can't finish");
    assert!(matches!(err, GenaiError::Internal(_)), "{err:?}");
    let message = err.to_string();
    assert!(message.contains("retry after 60s"), "{message}");
    assert!(message.contains("429"), "{message}");
}

#[tokio::test(start_paused = true)]
async fn wait_never_overshoots_its_deadline() {
    // 5 s timeout, 2 s interval: polls at 0, 2 and 4; a poll at 6 would be
    // past the deadline, so the wait ends at 4 rather than sleeping to 6.
    let poll = PollOptions::new()
        .with_timeout(Duration::from_secs(5))
        .with_poll_interval(Duration::from_secs(2));
    let start = tokio::time::Instant::now();
    let (result, reads) = scripted_wait(poll, vec![Read::Pending]).await;
    let err = result.unwrap_err();
    assert_eq!(reads, secs(&[0, 2, 4]));
    assert!(
        start.elapsed() <= Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
    assert!(err.to_string().contains("last state: pending"), "{err}");
    assert!(!err.is_retryable());
}

#[tokio::test(start_paused = true)]
async fn wait_cuts_a_hung_status_read_at_the_deadline() {
    let poll = PollOptions::new()
        .with_timeout(Duration::from_secs(10))
        .with_poll_interval(Duration::from_secs(4));
    let start = tokio::time::Instant::now();
    let (result, reads) = scripted_wait(poll, vec![Read::Pending, Read::Hang]).await;
    let err = result.unwrap_err();
    assert_eq!(reads, secs(&[0, 4]));
    // Exactly the deadline: no one-second floor, no extra interval.
    assert_eq!(start.elapsed(), Duration::from_secs(10));
    assert!(err.to_string().contains("no answer within 6s"), "{err}");
}

#[tokio::test(start_paused = true)]
async fn wait_with_a_zero_timeout_reads_once() {
    // The first read always happens, so an error `get` returns at once
    // (an invalid name) is reported rather than a timeout.
    let poll = PollOptions::new().with_timeout(Duration::ZERO);
    let (result, reads) = scripted_wait(
        poll,
        vec![Read::Fail(GenaiError::InvalidInput("bad name".to_string()))],
    )
    .await;
    assert!(
        matches!(result, Err(GenaiError::InvalidInput(_))),
        "{result:?}"
    );
    assert_eq!(reads, secs(&[0]));

    let (result, _) = scripted_wait(
        PollOptions::new().with_timeout(Duration::ZERO),
        vec![Read::Ready],
    )
    .await;
    assert_eq!(result.unwrap(), "ready");
}

#[tokio::test(start_paused = true)]
async fn wait_with_a_huge_timeout_has_no_deadline() {
    let poll = PollOptions::new()
        .with_timeout(Duration::MAX)
        .with_poll_interval(Duration::from_secs(3600));
    let (result, reads) = scripted_wait(poll, vec![Read::Pending, Read::Ready]).await;
    assert_eq!(result.unwrap(), "ready");
    assert_eq!(reads, secs(&[0, 3600]));
}
