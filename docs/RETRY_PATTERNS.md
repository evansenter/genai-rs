# Retry Patterns

This document explains genai-rs's approach to retry logic and transient error handling.

## Philosophy: Primitives, Not Policies

**We provide retry primitives, not a retry executor.**

Retry policies are highly application-specific:
- Max attempts vary by use case (real-time chat vs. batch processing)
- Backoff strategies differ (exponential, linear, fixed)
- Circuit breakers may be needed for sustained failures
- Retry budgets prevent cascading failures
- Metrics/logging hooks vary by infrastructure

Rather than building an opinionated retry system that won't fit everyone's needs, we provide clean primitives that integrate with battle-tested retry libraries like [`backon`](https://docs.rs/backon).

## Primitives We Provide

### 1. Cloneable Requests

`InteractionRequest` implements `Clone`, allowing you to retry the same request:

```rust
let request = client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Hello")
    .build()?;

// Retry loop can clone the request for each attempt
let response = client.execute(request.clone()).await?;
```

### 2. Retryable Error Detection

`GenaiError::is_retryable()` identifies transient errors worth retrying:

| Error Type | Retryable? | Reason |
|------------|------------|--------|
| 429 (Rate Limited) | Yes | Temporary capacity limit |
| 500-599 (Server Error) | Yes | Transient server issues |
| Timeout | Yes | Network/load issues |
| 400 (Bad Request) | No | Request is malformed |
| 401/403 (Auth) | No | Credentials issue |
| 404 (Not Found) | No | Resource doesn't exist |

```rust
match client.execute(request).await {
    Ok(response) => process(response),
    Err(e) if e.is_retryable() => retry_later(),
    Err(e) => fail_permanently(e),
}
```

### 3. Server-Suggested Delay

`GenaiError::retry_after()` extracts the `Retry-After` header from 429 responses:

```rust
if let Some(delay) = error.retry_after() {
    // Server says "wait this long"
    tokio::time::sleep(delay).await;
}
```

The header is parsed from both formats:
- Integer seconds: `Retry-After: 120`
- HTTP date: `Retry-After: Tue, 31 Dec 2030 23:59:59 GMT`

## Recommended Approach: Use `backon`

We recommend the [`backon`](https://docs.rs/backon) crate for production retry logic:

```rust
use backon::{ExponentialBuilder, Retryable};

let backoff = ExponentialBuilder::default()
    .with_min_delay(Duration::from_millis(100))
    .with_max_delay(Duration::from_secs(30))
    .with_max_times(3);

let response = (|| async {
    client.execute(request.clone()).await
})
    .retry(backoff)
    .when(|e: &GenaiError| e.is_retryable())
    .notify(|err, dur| tracing::warn!("Retry in {:?}: {}", dur, err))
    .await?;
```

See `examples/retry_with_backoff.rs` for a complete example.

## Streaming Limitations

**Streaming responses cannot be retried mid-stream.**

When using `execute_stream()`, the response is consumed as it arrives. If an error occurs partway through:
- Chunks already received are in your buffer
- Re-sending the request starts generation from scratch (different output due to LLM non-determinism)

**Recommendations for streaming:**
1. Accept partial loss on transient errors
2. Fall back to non-streaming `execute()` with retry for critical requests
3. Buffer chunks yourself if you need partial recovery
4. For stored interactions (`with_store_enabled()`), track each event's
   `event_id` and resume the same generation via
   `client.get_interaction_stream(interaction_id, Some(last_event_id))` —
   see [Error Handling](ERROR_HANDLING.md#stream-resume-on-error)

## Auto-Functions and Retry

The auto-function loop (`create_with_auto_functions`) makes multiple API calls internally:

```
User prompt → API call → Function call → API call → Function call → API call → Response
```

**Current behavior**: If any API call fails, the entire loop fails.

**Why we don't auto-retry inside the loop**: Function calls may have side effects (DB writes, external API calls). Re-running the entire loop would re-execute those functions, potentially causing:
- Duplicate database entries
- Double-charging payments
- Inconsistent state

**Recommended pattern**: Ensure your tool functions are idempotent, or wrap the entire `create_with_auto_functions()` call in your own retry logic with awareness of side effects.

**Future consideration**: We may add per-API-call retry within the loop (retrying the API call without re-executing functions that already succeeded). This is safe because:
- Functions that ran successfully don't re-run
- Only the "send results back to model" step retries

## When NOT to Retry

Some errors should fail immediately:

| Error | Why Not Retry |
|-------|---------------|
| 400 Bad Request | Request is malformed; fix the code |
| 401 Unauthorized | API key is invalid; fix credentials |
| 403 Forbidden | Permission denied; check access |
| 404 Not Found | Resource doesn't exist |
| Invalid JSON | Serialization bug; fix the code |

`is_retryable()` returns `false` for these cases.

## Circuit Breakers

For high-throughput applications, consider adding a circuit breaker to prevent hammering a failing service:

```rust
// Pseudocode - use a circuit breaker library
if circuit_breaker.is_open() {
    return Err(CircuitOpen);
}

match client.execute(request).await {
    Ok(r) => { circuit_breaker.record_success(); Ok(r) }
    Err(e) => { circuit_breaker.record_failure(); Err(e) }
}
```

Libraries like [`recloser`](https://docs.rs/recloser) or [`failsafe`](https://docs.rs/failsafe) provide this functionality.
