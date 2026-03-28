# Troubleshooting Guide

This guide covers common issues, debugging techniques, and solutions when working with `genai-rs`.

## Table of Contents

- [Wire-Level Debugging](#wire-level-debugging)
- [Common Errors](#common-errors)
- [Function Calling Issues](#function-calling-issues)
- [Streaming Issues](#streaming-issues)
- [Multimodal Issues](#multimodal-issues)
- [Regional Availability](#regional-availability)
- [Performance Issues](#performance-issues)
- [FAQ](#faq)

## Wire-Level Debugging

### Enable LOUD_WIRE

See exactly what's sent to and received from the API:

```bash
LOUD_WIRE=1 cargo run --example simple_interaction
```

Output shows:
```text
[REQ#1] POST https://generativelanguage.googleapis.com/v1beta/interactions
{
  "model": "gemini-3-flash-preview",
  "input": "Hello!",
  ...
}

[RES#1] 200 OK
{
  "status": "completed",
  "outputs": [...],
  ...
}
```

### Enable Library Logging

For internal library behavior:

```bash
RUST_LOG=genai_rs=debug cargo run --example simple_interaction
```

Log levels:
- `error` - Unrecoverable errors
- `warn` - Recoverable issues, unknown enum variants
- `info` - High-level operations
- `debug` - Detailed API lifecycle

### Combined Debugging

```bash
LOUD_WIRE=1 RUST_LOG=genai_rs=debug cargo run --example auto_function_calling
```

## Common Errors

### Invalid API Key (401)

```text
GenaiError::Api { status_code: 401, message: "API key not valid..." }
```

**Solutions:**
1. Check the key is set: `echo $GEMINI_API_KEY`
2. Verify key format (should start with `AI`)
3. Regenerate key in [Google AI Studio](https://ai.dev/)

```rust,ignore
let api_key = env::var("GEMINI_API_KEY")
    .expect("GEMINI_API_KEY must be set");

// Validate format
if !api_key.starts_with("AI") {
    panic!("Invalid API key format");
}
```

### Model Not Found (404)

```text
GenaiError::Api { status_code: 404, message: "Model not found..." }
```

**Solutions:**
1. Check model name spelling: `gemini-3-flash-preview` (not `gemini-flash`)
2. Verify model availability in your region
3. Check if model requires special access

```rust,ignore
// Correct model names
const STANDARD_MODEL: &str = "gemini-3-flash-preview";
const IMAGE_MODEL: &str = "gemini-3-pro-image-preview";
const TTS_MODEL: &str = "gemini-2.5-pro-preview-tts";
```

### Rate Limited (429)

```text
GenaiError::Api { status_code: 429, message: "Resource exhausted..." }
```

**Solutions:**
1. Implement exponential backoff
2. Reduce request frequency
3. Check quota in Google Cloud Console

```rust,ignore
async fn with_backoff<T>(operation: impl Fn() -> Future<Output = Result<T, GenaiError>>) {
    let mut delay = Duration::from_secs(1);
    for _ in 0..5 {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(GenaiError::Api { status_code: 429, .. }) => {
                sleep(delay).await;
                delay *= 2;
            }
            Err(e) => return Err(e),
        }
    }
}
```

### Request Timeout

```text
GenaiError::Timeout(duration)
```

**Solutions:**
1. Increase timeout for complex requests
2. Use streaming for long responses
3. Use background execution for agents

```rust,ignore
// Increase timeout
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text(complex_prompt)
    .with_timeout(Duration::from_secs(180))
    .create()
    .await?;
```

### Spanner UTF-8 Error (Transient)

```text
GenaiError::Api { message: "...Spanner...UTF-8..." }
```

This is a known transient Google backend issue.

**Solution:** Retry the request:

```rust,ignore
// Use the test utility pattern
async fn retry_on_transient<T>(operation: impl Fn() -> Future<Output = Result<T, GenaiError>>) {
    for attempt in 0..3 {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(GenaiError::Api { message, .. })
                if message.to_lowercase().contains("spanner")
                    && message.to_lowercase().contains("utf-8") =>
            {
                sleep(Duration::from_secs(1 << attempt)).await;
            }
            Err(e) => return Err(e),
        }
    }
}
```

## Function Calling Issues

### Function Not Being Called

**Symptoms:** Model responds with text instead of calling your function.

**Diagnose with LOUD_WIRE:**
```bash
LOUD_WIRE=1 cargo run --example auto_function_calling
```

Check that `tools` array is in the request.

**Solutions:**

1. **Function not registered:**
```rust,ignore
// Ensure #[tool] functions are in scope
use crate::my_tools::*;  // Bring into scope

let result = client
    .interaction()
    .with_text("What's the weather?")
    .create_with_auto_functions()  // Auto-discovers from registry
    .await?;
```

2. **Function not declared:**
```rust,ignore
// Explicitly add function
let result = client
    .interaction()
    .with_text("What's the weather?")
    .add_function(GetWeatherCallable.declaration())  // Explicit
    .create_with_auto_functions()
    .await?;
```

3. **Prompt doesn't trigger function:**
```rust,ignore
// Be explicit about needing the function
let prompt = "Use the get_weather function to check Tokyo's weather";
```

4. **Force function calling:**
```rust,ignore
use genai_rs::FunctionCallingMode;

let result = client
    .interaction()
    .with_text("What's the weather?")
    .add_function(decl)
    .with_function_calling_mode(FunctionCallingMode::Any)  // MUST call
    .create()
    .await?;
```

### Function Called with Wrong Arguments

**Diagnose:**
```rust,ignore
#[tool(city(description = "City name"))]
fn get_weather(city: String) -> String {
    println!("DEBUG: city = {:?}", city);  // Log arguments
    // ...
}
```

**Solutions:**

1. **Better parameter descriptions:**
```rust,ignore
#[tool(city(description = "The city name, e.g., 'Tokyo', 'New York City'"))]
fn get_weather(city: String) -> String
```

2. **Use enums for constrained values:**
```rust,ignore
let decl = FunctionDeclaration::builder("set_unit")
    .parameter("unit", json!({
        "type": "string",
        "enum": ["celsius", "fahrenheit"],
        "description": "Temperature unit"
    }))
    .build();
```

### Missing call_id Error

```text
"Function call 'get_weather' is missing call_id"
```

**Cause:** `store=false` disables call IDs needed for multi-turn.

**Solution:** Ensure storage is enabled (it is by default):
```rust,ignore
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("What's the weather?")
    .with_store_enabled()  // Explicit, but default is true
    .create()
    .await?;
```

### Infinite Function Loop

**Cause:** Function returns data that triggers another call.

**Solution:** Set max loops:
```rust,ignore
let result = client
    .interaction()
    .with_text(prompt)
    .add_function(func)
    .with_max_function_call_loops(3)  // Default is 10
    .create_with_auto_functions()
    .await?;
```

## Streaming Issues

### Stream Stops Unexpectedly

**Diagnose:**
```rust,ignore
while let Some(result) = stream.next().await {
    match result {
        Ok(event) => println!("Event: {:?}", event),
        Err(e) => {
            println!("Stream error: {:?}", e);
            break;
        }
    }
}
```

**Solutions:**

1. **Network timeout:** Increase client timeout
2. **Parse error:** Check for malformed SSE (use LOUD_WIRE)
3. **Server disconnect:** Implement resume with `event_id`

### Stream Resume After Error

```rust,ignore
let mut last_event_id: Option<String> = None;
let mut interaction_id: Option<String> = None;

loop {
    let mut stream = if let (Some(ref iid), Some(ref eid)) = (&interaction_id, &last_event_id) {
        // Resume from where we left off
        client.get_interaction_stream(iid, Some(eid.as_str()))
    } else {
        // Start fresh
        client.interaction().with_text(prompt).create_stream()
    };

    while let Some(result) = stream.next().await {
        if let Ok(event) = result {
            last_event_id = event.event_id.clone();
            // Capture interaction_id from the Complete chunk
            if let StreamChunk::Complete(response) = &event.chunk {
                interaction_id = response.id.clone();
            }
            // Process...
        }
    }
}
```

## Multimodal Issues

### Image Not Recognized

**Symptoms:** Model says "I can't see any image" or ignores the image.

**Solutions:**

1. **Check MIME type:**
```rust,ignore
// Correct MIME types
Content::image_data(base64, "image/png")   // Not "png"
Content::image_data(base64, "image/jpeg")  // Not "jpg"
```

2. **Verify base64 encoding:**
```rust,ignore
use base64::{Engine as _, engine::general_purpose::STANDARD};
let encoded = STANDARD.encode(&image_bytes);
```

3. **Check file size:** Images >20MB should use Files API

### Files API Upload Stuck Processing

**Symptoms:** File state remains `Processing` indefinitely.

**Solution:** Wait with timeout:
```rust,ignore
match client
    .wait_for_file_active(&file.name, Duration::from_secs(120))
    .await
{
    Ok(()) => println!("File ready"),
    Err(e) => {
        // Check state manually
        let metadata = client.get_file(&file.name).await?;
        println!("State: {:?}", metadata.state);
    }
}
```

### Image Generation Returns Text

**Cause:** Model interpreted prompt as conversation instead of generation.

**Solutions:**

1. **Ensure correct model:**
```rust,ignore
.with_model("gemini-3-pro-image-preview")  // NOT gemini-3-flash-preview
```

2. **Ensure image output enabled:**
```rust,ignore
.with_image_output()  // Required
```

3. **Check for images:**
```rust,ignore
if response.has_images() {
    // Success
} else if let Some(text) = response.as_text() {
    println!("Got text instead: {}", text);
}
```

## Regional Availability

### Feature Not Available

Some features have regional restrictions:

| Feature | Availability |
|---------|-------------|
| Image generation | Limited regions |
| Google Search | Most regions |
| Computer Use | Limited access |
| Deep Research | Limited access |

**Check availability:**
```rust,ignore
match result {
    Err(GenaiError::Api { message, .. })
        if message.contains("not available")
            || message.contains("not supported")
            || message.contains("permission") =>
    {
        println!("Feature not available in your region/account");
    }
    _ => {}
}
```

## Performance Issues

### Slow Responses

**Diagnose:**
```rust,ignore
let start = Instant::now();
let response = client.interaction().create().await?;
println!("Request took: {:?}", start.elapsed());
```

**Solutions:**

1. **Use streaming** for perceived speed:
```rust,ignore
let mut stream = client.interaction().create_stream();
// First token arrives faster
```

2. **Reduce input size:**
```rust,ignore
// Summarize large documents before sending
```

3. **Use appropriate model:**
```rust,ignore
// gemini-3-flash-preview is faster than gemini-3.1-pro-preview
```

### High Token Usage

**Monitor usage:**
```rust,ignore
// Using convenience methods (recommended)
println!("Input: {:?}", response.input_tokens());
println!("Output: {:?}", response.output_tokens());
println!("Reasoning: {:?}", response.reasoning_tokens());

// Or using the UsageMetadata struct directly
if let Some(usage) = &response.usage {
    println!("Input: {:?}", usage.total_input_tokens);
    println!("Output: {:?}", usage.total_output_tokens);
    println!("Reasoning: {:?}", usage.total_reasoning_tokens);
}
```

**Reduce tokens:**

1. **Lower thinking level:**
```rust,ignore
.with_thinking_level(ThinkingLevel::Low)  // Instead of High
```

2. **Limit output:**
```rust,ignore
.with_generation_config(GenerationConfig {
    max_output_tokens: Some(500),
    ..Default::default()
})
```

3. **Use lower resolution for images:**
```rust,ignore
Content::image_data(data, "image/png").with_resolution(Resolution::Low)
```

## FAQ

### Why does my test fail intermittently?

LLM outputs are non-deterministic. Solutions:
1. Use `with_seed()` for reproducibility
2. Validate structure, not exact content
3. Use semantic validation (ask model to verify)
4. Retry with backoff

### Why is my function called multiple times?

The model may call functions in parallel or sequentially. This is normal behavior for multi-step tasks.

### Can I use this library synchronously?

No, `genai-rs` is async-only. Use a runtime like Tokio:
```rust,ignore
#[tokio::main]
async fn main() {
    // Your code
}
```

### How do I debug what's sent to the API?

```bash
LOUD_WIRE=1 cargo run --example your_example
```

### Why do I get "unknown variant" warnings?

The Evergreen pattern logs when the API returns values not yet in the library's enums. This is informational - your code continues working. Consider updating the library if you see these frequently.

### How do I report issues?

1. Enable `LOUD_WIRE=1` and capture output
2. Include Rust version: `rustc --version`
3. Include library version from `Cargo.toml`
4. Open issue at [GitHub](https://github.com/evansenter/genai-rs/issues)

## Getting Help

1. **Check examples:** `cargo run --example <name>`
2. **Read docs:** See `docs/` directory
3. **Enable debugging:** `LOUD_WIRE=1 RUST_LOG=genai_rs=debug`
4. **Open issue:** Include debug output and reproduction steps
