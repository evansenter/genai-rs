# Error Handling Guide

This guide covers error types, common error scenarios, and recovery patterns in `genai-rs`.

## Table of Contents

- [Error Types](#error-types)
- [Handling API Errors](#handling-api-errors)
- [Common Error Scenarios](#common-error-scenarios)
- [Retry Strategies](#retry-strategies)
- [Function Calling Errors](#function-calling-errors)
- [Streaming Errors](#streaming-errors)

## Error Types

### GenaiError

The primary error type for all API operations.

```rust,ignore
use genai_rs::GenaiError;

match client.interaction().create().await {
    Ok(response) => { /* success */ }
    Err(e) => match e {
        GenaiError::Api { status_code, message, request_id, retry_after } => {
            // HTTP error from API (retry_after populated for 429 responses)
        }
        GenaiError::Http(e) => {
            // Network/connection error
        }
        GenaiError::Timeout(duration) => {
            // Request timed out
        }
        GenaiError::Json(e) => {
            // Response parsing error
        }
        GenaiError::InvalidInput(msg) => {
            // Invalid request parameters
        }
        GenaiError::MalformedResponse(msg) => {
            // Unexpected API response format
        }
        GenaiError::Parse(msg) => {
            // SSE stream parsing error
        }
        GenaiError::Utf8(e) => {
            // UTF-8 decoding error
        }
        GenaiError::Internal(msg) => {
            // Internal client error
        }
        GenaiError::ClientBuild(msg) => {
            // Failed to build HTTP client
        }
        _ => {
            // Future variants (non_exhaustive)
        }
    }
}
```

### FunctionError

Errors from function execution (client-side function calling).

```rust,ignore
use genai_rs::FunctionError;

match result {
    Err(FunctionError::ArgumentMismatch(msg)) => {
        println!("Arguments didn't match the schema: {}", msg);
    }
    Err(FunctionError::ExecutionError(source)) => {
        println!("Function failed: {}", source);
    }
    _ => {
        // Future variants (non_exhaustive)
    }
}
```

## Handling API Errors

### By Status Code

```rust,ignore
match client.interaction().create().await {
    Err(GenaiError::Api { status_code, message, request_id, .. }) => {
        match status_code {
            400 => {
                // Bad request - check your parameters
                println!("Invalid request: {}", message);
            }
            401 => {
                // Authentication failed
                println!("Invalid API key");
            }
            403 => {
                // Permission denied
                println!("Access forbidden: {}", message);
            }
            404 => {
                // Resource not found (e.g., invalid model name)
                println!("Not found: {}", message);
            }
            429 => {
                // Rate limited - implement backoff
                println!("Rate limited, retry after backoff");
            }
            500..=599 => {
                // Server error - safe to retry
                println!("Server error ({}): {}", status_code, message);
            }
            _ => {
                println!("API error {}: {}", status_code, message);
            }
        }

        // Log request_id for debugging with Google support
        if let Some(id) = request_id {
            println!("Request ID: {}", id);
        }
    }
    _ => {}
}
```

For 429 responses, the `Api` variant also carries a `retry_after` field parsed
from the `Retry-After` header; `GenaiError::retry_after()` exposes it as a
`Duration` (see [Retry Patterns](RETRY_PATTERNS.md)).

### Using request_id

The `request_id` field (from `x-goog-request-id` header) is valuable for:
- Debugging with Google support
- Correlating logs across systems
- Tracking specific failed requests

```rust,ignore
if let Err(GenaiError::Api { request_id: Some(id), .. }) = result {
    log::error!("Request {} failed - save this ID for support", id);
}
```

## Common Error Scenarios

### Invalid API Key

```rust,ignore
// Error: GenaiError::Api { status_code: 401, message: "API key not valid..." }

// Prevention: Validate key format before use
let api_key = env::var("GEMINI_API_KEY")
    .expect("GEMINI_API_KEY must be set");

if api_key.is_empty() || !api_key.starts_with("AI") {
    panic!("Invalid API key format");
}
```

### Invalid Model Name

```rust,ignore
// Error: GenaiError::Api { status_code: 404, message: "Model not found..." }

// Prevention: Use known model constants
const MODEL: &str = "gemini-3-flash-preview";
```

### Rate Limiting

```rust,ignore
// Error: GenaiError::Api { status_code: 429, ... }

// Solution: Implement exponential backoff (see Retry Strategies below)
```

### Request Timeout

```rust,ignore
// Error: GenaiError::Timeout(Duration)

// Prevention: Set appropriate timeout
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Complex analysis task...")
    .with_timeout(Duration::from_secs(120))  // 2 minutes
    .create()
    .await?;
```

### Network Errors

```rust,ignore
// Error: GenaiError::Http(reqwest::Error)

match result {
    Err(GenaiError::Http(e)) => {
        if e.is_connect() {
            println!("Connection failed - check network");
        } else if e.is_timeout() {
            println!("Connection timed out");
        }
    }
    _ => {}
}
```

### Malformed Response

```rust,ignore
// Error: GenaiError::MalformedResponse("...")

// This indicates the API returned unexpected data.
// Usually a sign of API evolution - check for library updates.
```

## Retry Strategies

### Simple Retry with Backoff

```rust,ignore
use std::time::Duration;
use tokio::time::sleep;

async fn retry_with_backoff<T, F, Fut>(
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
            Err(e) if should_retry(&e) && attempt < max_retries => {
                let delay = Duration::from_secs(1 << attempt); // 1s, 2s, 4s...
                println!("Attempt {} failed, retrying in {:?}", attempt + 1, delay);
                last_error = Some(e);
                sleep(delay).await;
            }
            Err(e) => return Err(e),
        }
    }

    Err(last_error.unwrap())
}

fn should_retry(error: &GenaiError) -> bool {
    match error {
        GenaiError::Api { status_code, .. } => {
            // Retry on rate limits and server errors
            *status_code == 429 || *status_code >= 500
        }
        GenaiError::Http(e) => {
            // Retry on connection errors
            e.is_connect() || e.is_timeout()
        }
        GenaiError::Timeout(_) => true,
        _ => false,
    }
}
```

### Retry on Transient Errors

Some errors are known to be transient (e.g., Google's Spanner UTF-8 errors):

```rust,ignore
fn is_transient_error(err: &GenaiError) -> bool {
    match err {
        GenaiError::Api { message, .. } => {
            let lower = message.to_lowercase();
            // Known transient Google backend issue
            lower.contains("spanner") && lower.contains("utf-8")
        }
        _ => false,
    }
}
```

### Using the Test Utilities

The `tests/common/mod.rs` module provides retry helpers you can adapt:

```rust,ignore
// From tests/common/mod.rs - adapt for production use
pub async fn retry_on_transient<F, Fut, T>(
    max_retries: u32,
    operation: F,
) -> Result<T, GenaiError>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, GenaiError>>,
{
    // ... implementation
}
```

## Function Calling Errors

### Missing Functions

The auto-function loop does not fail when the model calls a function that isn't
registered. Instead it sends an error result back to the model
(`{"error": "Function '<name>' is not available or not found."}`) so the
conversation can continue. A missing function is usually a typo or a missing
`#[tool]` / `with_function()` registration — watch the logs for:

```text
Function not found in registry or tool service: function='...'
```

### Execution Errors

```rust,ignore
#[tool(description = "Fetch data from API")]
async fn fetch_data(url: String) -> Result<String, String> {
    // Return Err for graceful failure
    reqwest::get(&url)
        .await
        .map_err(|e| format!("HTTP error: {}", e))?
        .text()
        .await
        .map_err(|e| format!("Read error: {}", e))
}

// The model sees: {"error": "HTTP error: connection refused"}
// It can then inform the user or try alternative approaches
```

### Argument Parsing Errors

```rust,ignore
Err(FunctionError::ArgumentMismatch(message)) => {
    // Model sent arguments that don't match the schema
    log::warn!("Model sent invalid args: {}", message);
    // Usually recoverable - model will retry with corrected args
}
```

### Best Practices for Function Errors

1. **Return `Result` from functions** - Let the model handle errors gracefully
2. **Use descriptive error messages** - The model sees these and can adapt
3. **Don't panic** - Return errors so the conversation can continue

```rust,ignore
#[tool(description = "Get user by ID")]
fn get_user(id: i32) -> Result<String, String> {
    if id <= 0 {
        return Err("User ID must be positive".to_string());
    }

    match database.find_user(id) {
        Some(user) => Ok(serde_json::to_string(&user).unwrap()),
        None => Err(format!("User {} not found", id)),
    }
}
```

## Streaming Errors

### Handling Stream Errors

```rust,ignore
use futures_util::StreamExt;

let mut stream = client.interaction().create_stream();

while let Some(result) = stream.next().await {
    match result {
        Ok(event) => {
            // Process event
        }
        Err(GenaiError::Parse(msg)) => {
            // SSE parsing error - stream may be corrupted
            log::error!("Stream parse error: {}", msg);
            break;
        }
        Err(e) => {
            log::error!("Stream error: {}", e);
            break;
        }
    }
}
```

### Stream Resume on Error

Streams support resumption via `event_id`. Track the interaction ID (from the
`Created` chunk) and the last received `event_id`, then resume with
`client.get_interaction_stream(interaction_id, last_event_id)`:

```rust,ignore
let mut interaction_id: Option<String> = None;
let mut last_event_id: Option<String> = None;
let mut collected_text = String::new();

loop {
    let mut stream = if let (Some(id), Some(event_id)) = (&interaction_id, &last_event_id) {
        // Resume from last known position
        client.get_interaction_stream(id, Some(event_id))
    } else {
        client.interaction()
            .with_model("gemini-3-flash-preview")
            .with_text("Tell me a story")
            .with_store_enabled()  // Required to resume by interaction ID
            .create_stream()
    };

    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => {
                if let Some(id) = &event.event_id {
                    last_event_id = Some(id.clone());
                }
                if let Some(text) = event.chunk.delta_text() {
                    collected_text.push_str(text);
                }
                match event.chunk {
                    StreamChunk::Created { interaction } => {
                        interaction_id = interaction.id.clone();
                    }
                    StreamChunk::Completed(_) => return Ok(collected_text),
                    _ => {}
                }
            }
            Err(e) if should_retry(&e) => {
                log::warn!("Stream error, resuming: {}", e);
                break; // Break inner loop to retry
            }
            Err(e) => return Err(e),
        }
    }
}
```

## Error Logging

### Recommended Pattern

```rust,ignore
use log::{error, warn, info, debug};

match result {
    Ok(response) => {
        debug!("Request succeeded: {:?}", response.id);
    }
    Err(GenaiError::Api { status_code: 429, request_id, .. }) => {
        warn!("Rate limited (request: {:?}), backing off", request_id);
    }
    Err(GenaiError::Api { status_code, message, request_id, .. }) if status_code >= 500 => {
        error!("Server error {} (request: {:?}): {}",
               status_code, request_id, message);
    }
    Err(e) => {
        error!("Request failed: {}", e);
    }
}
```

### Enable Library Logging

```bash
RUST_LOG=genai_rs=debug cargo run --example simple_interaction
```

### Wire-Level Debugging

```bash
LOUD_WIRE=1 cargo run --example simple_interaction
```

## Error Type Reference

| Error | Cause | Recovery |
|-------|-------|----------|
| `Api { 400, .. }` | Invalid request | Fix parameters |
| `Api { 401, .. }` | Bad API key | Check credentials |
| `Api { 403, .. }` | Permission denied | Check API access |
| `Api { 404, .. }` | Resource not found | Check model/file name |
| `Api { 429, .. }` | Rate limited | Backoff and retry |
| `Api { 5xx, .. }` | Server error | Retry with backoff |
| `Http(_)` | Network error | Check connection, retry |
| `Timeout(_)` | Request too slow | Increase timeout, retry |
| `Json(_)` | Parse error | Check for API updates |
| `InvalidInput(_)` | Bad parameters | Fix before sending |
| `MalformedResponse(_)` | Unexpected format | Check for library updates |
| `Parse(_)` | SSE parse error | Resume stream |
| `ClientBuild(_)` | TLS/client init | Check environment |
