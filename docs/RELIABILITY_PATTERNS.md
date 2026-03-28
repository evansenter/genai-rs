# Reliability Patterns Guide

This guide covers production patterns for building reliable applications with `genai-rs`, including retry strategies, error handling, timeout management, and graceful degradation.

## Table of Contents

- [Overview](#overview)
- [Retry Strategies](#retry-strategies)
- [Exponential Backoff](#exponential-backoff)
- [Transient Error Detection](#transient-error-detection)
- [Timeout Management](#timeout-management)
- [Cancellation](#cancellation)
- [Rate Limiting](#rate-limiting)
- [Graceful Degradation](#graceful-degradation)
- [Production Checklist](#production-checklist)

## Overview

Building reliable AI applications requires handling:

| Challenge | Solution |
|-----------|----------|
| Transient errors | Retry with backoff |
| Rate limits | Backoff, queuing |
| Timeouts | Appropriate limits, streaming |
| Long-running tasks | Background execution, polling |
| Partial failures | Graceful degradation |

## Retry Strategies

> **Note**: genai-rs provides built-in retry primitives. See [Retry Patterns](RETRY_PATTERNS.md) for the philosophy and available primitives (`is_retryable()`, `retry_after()`).

### Using Built-in Primitives

The simplest approach uses `GenaiError::is_retryable()`:

```rust,ignore
use genai_rs::{Client, GenaiError};
use std::time::Duration;
use tokio::time::sleep;

async fn with_retry<T, F, Fut>(
    max_retries: u32,
    operation: F,
) -> Result<T, GenaiError>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, GenaiError>>,
{
    let mut last_error = None;

    for attempt in 0..=max_retries {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) if e.is_retryable() && attempt < max_retries => {
                // Use server-suggested delay if available, otherwise exponential backoff
                let delay = e.retry_after().unwrap_or(Duration::from_secs(1 << attempt));
                println!("Attempt {} failed, retrying in {:?}", attempt + 1, delay);
                last_error = Some(e);
                sleep(delay).await;
            }
            Err(e) => return Err(e),
        }
    }

    Err(last_error.unwrap())
}
```

### Usage

```rust,ignore
let response = with_retry(3, || async {
    client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Hello")
        .create()
        .await
}).await?;
```

## Exponential Backoff

Exponential backoff prevents overwhelming the API during recovery.

### Simple Backoff

```rust,ignore
use std::time::Duration;
use tokio::time::sleep;

async fn exponential_backoff<T, F, Fut>(
    max_retries: u32,
    base_delay: Duration,
    max_delay: Duration,
    operation: F,
) -> Result<T, GenaiError>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, GenaiError>>,
{
    let mut delay = base_delay;

    for attempt in 0..=max_retries {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) if should_retry(&e) && attempt < max_retries => {
                println!("Attempt {} failed, waiting {:?}", attempt + 1, delay);
                sleep(delay).await;
                delay = (delay * 2).min(max_delay);
            }
            Err(e) => return Err(e),
        }
    }

    Err(GenaiError::Internal("Max retries exceeded".to_string()))
}
```

### With Jitter

Add randomization to prevent thundering herd:

```rust,ignore
use rand::Rng;

fn delay_with_jitter(base: Duration, attempt: u32, max: Duration) -> Duration {
    let exponential = base * (1 << attempt);
    let capped = exponential.min(max);

    // Add 0-25% jitter
    let jitter_factor = rand::thread_rng().gen_range(0.0..0.25);
    let jitter = Duration::from_secs_f64(capped.as_secs_f64() * jitter_factor);

    capped + jitter
}
```

### Recommended Backoff Settings

| Use Case | Base Delay | Max Delay | Max Retries |
|----------|------------|-----------|-------------|
| Simple queries | 1s | 10s | 3 |
| Rate limit recovery | 2s | 60s | 5 |
| Background polling | 2s | 30s | 10 |

## Transient Error Detection

### Built-in Detection

Use `GenaiError::is_retryable()` for standard transient error detection (429, 5xx, timeouts, connection errors):

```rust,ignore
if error.is_retryable() {
    // Safe to retry
}
```

### Custom Detection

For application-specific transient errors, extend the built-in detection:

```rust,ignore
fn is_transient_error(err: &GenaiError) -> bool {
    // Start with built-in detection
    if err.is_retryable() {
        return true;
    }

    // Add application-specific patterns
    if let GenaiError::Api { message, .. } = err {
        let lower = message.to_lowercase();
        // Known Google backend issue
        if lower.contains("spanner") && lower.contains("utf-8") {
            return true;
        }
    }

    false
}
```

### Retry on Any Error (Flaky Operations)

For operations that may fail unpredictably:

```rust,ignore
async fn retry_on_any_error<T, E, F, Fut>(
    max_retries: u32,
    delay: Duration,
    operation: F,
) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Debug,
{
    let mut last_error = None;

    for attempt in 0..=max_retries {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(err) if attempt < max_retries => {
                println!("Attempt {} failed: {:?}", attempt + 1, err);
                last_error = Some(err);
                sleep(delay).await;
            }
            Err(err) => return Err(err),
        }
    }

    Err(last_error.unwrap())
}
```

## Timeout Management

### Client-Level Timeouts

```rust,ignore
use std::time::Duration;

let client = Client::builder(api_key)
    .with_timeout(Duration::from_secs(120))       // Request timeout
    .with_connect_timeout(Duration::from_secs(10)) // Connection timeout
    .build()?;
```

### Request-Level Timeouts

Override for specific requests:

```rust,ignore
// Long analysis task
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text(&large_document)
    .with_timeout(Duration::from_secs(300))  // 5 minutes
    .create()
    .await?;
```

### Timeout Guidelines

| Operation | Recommended Timeout |
|-----------|---------------------|
| Simple query | 30-60s |
| Complex analysis | 120-180s |
| Document processing | 300s |
| Streaming | 300-600s (higher due to incremental delivery) |
| Background agents | Use polling instead |

### Handling Timeouts

```rust,ignore
match client.interaction().create().await {
    Ok(response) => { /* success */ }
    Err(GenaiError::Timeout(duration)) => {
        println!("Request timed out after {:?}", duration);
        // Consider:
        // - Retrying with longer timeout
        // - Breaking into smaller requests
        // - Using background execution
    }
    Err(e) => return Err(e.into()),
}
```

## Cancellation

Cancel long-running background tasks.

### Basic Cancellation

```rust,ignore
// Start background task
let response = client
    .interaction()
    .with_agent("deep-research-pro-preview-12-2025")
    .with_text("Research topic")
    .with_background(true)
    .with_store_enabled()
    .create()
    .await?;

let interaction_id = response.id.as_ref().unwrap();

// Later, cancel if needed
if response.status == InteractionStatus::InProgress {
    match client.cancel_interaction(interaction_id).await {
        Ok(cancelled) => {
            println!("Cancelled: {:?}", cancelled.status);
        }
        Err(GenaiError::Api { status_code: 400, .. }) => {
            // Already completed - can't cancel
            println!("Interaction already finished");
        }
        Err(e) => return Err(e.into()),
    }
}
```

### User-Initiated Cancellation Pattern

```rust,ignore
use tokio::select;
use tokio::sync::oneshot;

async fn interruptible_request(
    client: &Client,
    prompt: &str,
    cancel_rx: oneshot::Receiver<()>,
) -> Result<Option<InteractionResponse>, GenaiError> {
    // Start background task
    let response = client
        .interaction()
        .with_agent("deep-research-pro-preview-12-2025")
        .with_text(prompt)
        .with_background(true)
        .with_store_enabled()
        .create()
        .await?;

    let interaction_id = response.id.as_ref().unwrap().clone();

    // Poll with cancellation support
    loop {
        select! {
            _ = cancel_rx => {
                // User cancelled
                let _ = client.cancel_interaction(&interaction_id).await;
                return Ok(None);
            }
            result = client.get_interaction(&interaction_id) => {
                let status = result?;
                match status.status {
                    InteractionStatus::Completed => return Ok(Some(status)),
                    InteractionStatus::Failed => return Err(GenaiError::Internal("Failed".into())),
                    InteractionStatus::Cancelled => return Ok(None),
                    _ => {
                        sleep(Duration::from_secs(2)).await;
                    }
                }
            }
        }
    }
}
```

## Rate Limiting

### Request Queuing

```rust,ignore
use tokio::sync::Semaphore;
use std::sync::Arc;

struct RateLimitedClient {
    client: Client,
    semaphore: Arc<Semaphore>,
}

impl RateLimitedClient {
    fn new(client: Client, max_concurrent: usize) -> Self {
        Self {
            client,
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
        }
    }

    async fn interaction(&self) -> InteractionBuilder<'_> {
        let _permit = self.semaphore.acquire().await.unwrap();
        self.client.interaction()
    }
}

// Usage
let rate_limited = RateLimitedClient::new(client, 5);  // Max 5 concurrent
```

### Token Bucket Pattern

```rust,ignore
use std::time::Instant;

struct TokenBucket {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64,  // tokens per second
    last_refill: Instant,
}

impl TokenBucket {
    fn new(max_tokens: f64, refill_rate: f64) -> Self {
        Self {
            tokens: max_tokens,
            max_tokens,
            refill_rate,
            last_refill: Instant::now(),
        }
    }

    async fn acquire(&mut self, tokens: f64) {
        self.refill();

        while self.tokens < tokens {
            let wait = (tokens - self.tokens) / self.refill_rate;
            sleep(Duration::from_secs_f64(wait)).await;
            self.refill();
        }

        self.tokens -= tokens;
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.max_tokens);
        self.last_refill = now;
    }
}
```

## Graceful Degradation

### Fallback Responses

```rust,ignore
async fn with_fallback(
    client: &Client,
    prompt: &str,
    fallback: &str,
) -> String {
    match client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text(prompt)
        .create()
        .await
    {
        Ok(response) => {
            response.as_text().unwrap_or(fallback).to_string()
        }
        Err(e) => {
            log::warn!("API error, using fallback: {}", e);
            fallback.to_string()
        }
    }
}
```

### Tiered Model Fallback

```rust,ignore
async fn with_model_fallback(
    client: &Client,
    prompt: &str,
) -> Result<String, GenaiError> {
    let models = [
        "gemini-3-flash-preview",
        "gemini-3.1-pro-preview",
    ];

    for model in models {
        match client
            .interaction()
            .with_model(model)
            .with_text(prompt)
            .create()
            .await
        {
            Ok(response) => {
                return Ok(response.as_text().unwrap_or("").to_string());
            }
            Err(GenaiError::Api { status_code: 429, .. }) => {
                log::warn!("Rate limited on {}, trying next model", model);
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    Err(GenaiError::Internal("All models failed".to_string()))
}
```

### Cached Responses

```rust,ignore
use std::collections::HashMap;
use std::sync::RwLock;

struct CachedClient {
    client: Client,
    cache: RwLock<HashMap<String, String>>,
}

impl CachedClient {
    async fn query(&self, prompt: &str) -> Result<String, GenaiError> {
        // Check cache first
        if let Some(cached) = self.cache.read().unwrap().get(prompt) {
            return Ok(cached.clone());
        }

        // Make request
        let response = self.client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_text(prompt)
            .create()
            .await?;

        let text = response.as_text().unwrap_or("").to_string();

        // Cache result
        self.cache.write().unwrap().insert(prompt.to_string(), text.clone());

        Ok(text)
    }
}
```

## Production Checklist

### Error Handling

- [ ] Retry transient errors with exponential backoff
- [ ] Handle rate limits (429) gracefully
- [ ] Log errors with request IDs for debugging
- [ ] Implement fallback strategies

### Timeouts

- [ ] Set appropriate client-level timeouts
- [ ] Override for long-running operations
- [ ] Use background execution for agent tasks
- [ ] Implement polling with reasonable intervals

### Resource Management

- [ ] Limit concurrent requests
- [ ] Cancel unused background tasks
- [ ] Clean up uploaded files
- [ ] Monitor token usage

### Monitoring

```rust,ignore
// Log all requests with timing
async fn timed_request<T, F, Fut>(
    operation_name: &str,
    operation: F,
) -> Result<T, GenaiError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T, GenaiError>>,
{
    let start = Instant::now();
    let result = operation().await;
    let duration = start.elapsed();

    match &result {
        Ok(_) => log::info!("{} completed in {:?}", operation_name, duration),
        Err(e) => log::error!("{} failed after {:?}: {}", operation_name, duration, e),
    }

    result
}
```

### Health Checks

```rust,ignore
async fn health_check(client: &Client) -> bool {
    match client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("ping")
        .with_timeout(Duration::from_secs(10))
        .create()
        .await
    {
        Ok(response) => response.status == InteractionStatus::Completed,
        Err(_) => false,
    }
}
```

## Example Patterns

### Complete Retry Wrapper

```rust,ignore
use genai_rs::{Client, GenaiError, InteractionResponse};
use std::time::Duration;
use tokio::time::sleep;

pub struct ReliableClient {
    client: Client,
    max_retries: u32,
    base_delay: Duration,
    max_delay: Duration,
}

impl ReliableClient {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            max_retries: 3,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
        }
    }

    pub async fn query(&self, prompt: &str) -> Result<InteractionResponse, GenaiError> {
        let mut delay = self.base_delay;

        for attempt in 0..=self.max_retries {
            let result = self.client
                .interaction()
                .with_model("gemini-3-flash-preview")
                .with_text(prompt)
                .create()
                .await;

            match result {
                Ok(response) => return Ok(response),
                Err(e) if e.is_retryable() && attempt < self.max_retries => {
                    // Prefer server-suggested delay, fall back to exponential backoff
                    let wait = e.retry_after().unwrap_or(delay);
                    log::warn!(
                        "Attempt {} failed, retrying in {:?}: {}",
                        attempt + 1, wait, e
                    );
                    sleep(wait).await;
                    delay = (delay * 2).min(self.max_delay);
                }
                Err(e) => return Err(e),
            }
        }

        unreachable!()
    }
}
```

## Related Documentation

- [Retry Patterns](RETRY_PATTERNS.md) - Philosophy and built-in retry primitives
- [Error Handling](ERROR_HANDLING.md) - Error types and recovery
- [Agents and Background](AGENTS_AND_BACKGROUND.md) - Background execution patterns
- [Configuration](CONFIGURATION.md) - Timeout and client configuration
