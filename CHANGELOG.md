# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `InteractionStatus::Incomplete` variant for interactions that end before completion
- Google Maps built-in tool support (`Tool::GoogleMaps`, `GoogleMapsConfig`, `GoogleMapsCall`, `GoogleMapsResult` content types)
- `SearchType` enum for Google Search `search_types` configuration
- `allowed_tools` and `headers` optional fields on `Tool::McpServer`
- Unified `add_tool(impl Into<Tool>)` method on `InteractionBuilder`
- Tool configuration structs: `GoogleMapsConfig`, `GoogleSearchConfig`, `McpServerConfig`, `ComputerUseConfig`, `FileSearchConfig`
- `with_google_maps()` shorthand on `InteractionBuilder`
- `Place` struct with Evergreen forward-compatible `extra` field
- `GoogleMapsResultInfo` view type and response accessors (`has_google_maps_results()`, `google_maps_results()`)

### Changed

- `Tool::GoogleSearch` is now a struct variant with optional `search_types` field (was unit variant)
- `Tool::McpServer` has additional optional fields `allowed_tools` and `headers`
- Proptest roundtrip comparisons use `serde_json::Value` for HashMap key order independence

### Removed

- `with_computer_use()` and `with_computer_use_excluding()` — use `add_tool(ComputerUseConfig::new())`
- `add_mcp_server()` — use `add_tool(McpServerConfig::new(name, url))`
- `with_file_search()` and `with_file_search_config()` — use `add_tool(FileSearchConfig::new(stores))`

## [0.7.2] - 2026-01-17

### Changed

- **BREAKING**: `AutoFunctionStreamChunk::ExecutingFunctions` changed from tuple variant to struct variant with `pending_calls` field:
  ```rust
  // Before
  AutoFunctionStreamChunk::ExecutingFunctions(response) => {
      // response.function_calls() was often empty in streaming mode
  }

  // After
  AutoFunctionStreamChunk::ExecutingFunctions { response, pending_calls } => {
      // pending_calls always contains the validated function calls
      for call in pending_calls {
          println!("Executing: {}({})", call.name, call.args);
      }
  }
  ```

### Added

- `PendingFunctionCall` type: Represents a function call about to be executed, with `name`, `call_id`, and `args` fields. Available in `ExecutingFunctions` events before function execution begins.

### Fixed

- `ExecutingFunctions` chunk now provides function call information via `pending_calls` field. Previously, `response.function_calls()` was often empty in streaming mode because function calls arrived via Delta chunks rather than the Complete response.

## [0.7.1] - 2026-01-17

### Changed

- **BREAKING**: Removed typestate pattern from `InteractionBuilder`. The builder no longer uses `FirstTurn`, `Chained`, or `StoreDisabled` marker types. Invalid combinations (e.g., `store=false` with `with_previous_interaction()`) are now caught at runtime in `build()` with descriptive error messages instead of compile-time type errors. This enables conditional chaining patterns that were previously impossible:
  ```rust
  // Now possible - conditional chaining
  let mut builder = client.interaction()
      .with_model("gemini-3-flash-preview")
      .with_text("Hello");

  if let Some(prev_id) = previous_interaction_id {
      builder = builder.with_previous_interaction(prev_id);
  }

  let response = builder.create().await?;
  ```
- **BREAKING**: `FunctionExecutionResult::new()` now requires `args` parameter (position 3, before `result`)
- `FunctionExecutionResult` now includes `args` field for complete execution context - enables logging function calls with their arguments after execution completes

### Fixed

- Auto-function execution (`create_with_auto_functions()` and `create_stream_with_auto_functions()`) now reports accurate accumulated token usage across all API calls. Previously, the final response could show 0 input tokens because the API only reports input tokens on the first call.

### Migration

If you relied on compile-time enforcement of builder constraints, you'll now get runtime errors from `build()` instead:
- `with_store_disabled()` + `with_previous_interaction()` → `GenaiError::InvalidInput("Chained interactions require storage...")`
- `with_store_disabled()` + `with_background(true)` → `GenaiError::InvalidInput("Background execution requires storage...")`
- `with_store_disabled()` + `create_with_auto_functions()` → `GenaiError::InvalidInput("create_with_auto_functions() requires storage...")`

## [0.7.0] - 2026-01-15

### Added

- New `docs/BUILDER_API.md` documenting the InteractionBuilder API, method naming conventions, and validation errors
- `build()` now validates that `with_agent_config()` requires `with_agent()` - returns error instead of silently ignoring
- **New Content API**: Static constructors on `Content` for all content types:
  - `Content::text()`, `Content::image_data()`, `Content::image_uri()`, `Content::audio_data()`, `Content::audio_uri()`, `Content::video_data()`, `Content::video_uri()`, `Content::document_data()`, `Content::document_uri()`
  - `Content::from_file(&FileMetadata)` - create content from Files API upload
  - `Content::from_uri_and_mime(uri, mime)` - generic URI content
  - Resolution variants: `Content::image_data_with_resolution()`, etc.
- **Content builder methods**:
  - `Content::with_resolution(Resolution)` - chain resolution setting
  - `Content::with_result(value)` - convert `FunctionCall` to `FunctionResult`
  - `Content::with_result_error(value)` - convert `FunctionCall` to error `FunctionResult`

### Changed

- **BREAKING**: Renamed `InteractionContent` → `Content` for ergonomics. Update imports: `use genai_rs::Content;`
- **BREAKING**: Renamed `Content::text()` getter → `Content::as_text()` to follow Rust getter conventions
- **BREAKING**: Renamed `InteractionResponse::text()` getter → `InteractionResponse::as_text()` for consistency
- **BREAKING**: Renamed `TurnContent::parts()` → `TurnContent::as_parts()` for consistency with `as_text()`
- **BREAKING**: Renamed `with_turns()` to `with_history()`. The new name better reflects that this sets conversation history, and now composes correctly with `with_text()`: calling both produces `[...history, Turn::user(current_message)]` regardless of call order.
- **BREAKING**: `with_text()` now sets `current_message` instead of replacing `input`. This fixes issue #359 where `with_turns().with_text()` silently overwrote the history.
- **BREAKING**: `with_system_instruction()` is now available on ALL builder states (FirstTurn, Chained, StoreDisabled), not just FirstTurn. The API does NOT inherit system instructions via `previousInteractionId`, so users should set it explicitly on each turn if needed. For `create_with_auto_functions()`, the SDK automatically includes system_instruction on all internal turns.
- Method naming consistency overhaul:
  - `with_function()` → `add_function()` (accumulates)
  - `with_functions()` → `add_functions()` (accumulates)
- `build()` now returns an error if content input is combined with history (incompatible modes), with a helpful error message explaining the workaround

### Removed

- **BREAKING**: Removed all `add_*` multimodal methods from `InteractionBuilder`:
  - `add_image_data()`, `add_image_uri()`, `add_image_file()`, `add_image_bytes()`
  - `add_audio_data()`, `add_audio_uri()`, `add_audio_file()`, `add_audio_bytes()`
  - `add_video_data()`, `add_video_uri()`, `add_video_file()`, `add_video_bytes()`
  - `add_document_data()`, `add_document_uri()`, `add_document_file()`, `add_document_bytes()`
  - `add_file()`, `add_file_uri()` (Files API methods)
  - All `*_with_resolution()` variants

  **Migration**: Use `with_content(vec![Content::*(...)])` instead. See migration guide below.

- **BREAKING**: Removed all `*_content()` free functions from `interactions_api`:
  - `text_content()`, `image_data_content()`, `image_uri_content()`, `audio_data_content()`, `audio_uri_content()`
  - `video_data_content()`, `video_uri_content()`, `document_data_content()`, `document_uri_content()`
  - `function_call_content()`, `function_result_content()`, `file_data_content()`, `file_uri_content()`

  **Migration**: Use `Content::*()` static constructors instead (e.g., `text_content("hi")` → `Content::text("hi")`).

  **Note**: Model output constructors for testing remain in `interactions_api`: `code_execution_*`, `google_search_*`, `url_context_*`, `file_search_*`.

### Fixed

- `AgentConfig` (DeepResearchConfig) now serializes `thinking_summaries` with snake_case per API spec, not camelCase `thinkingSummaries`
- **BREAKING**: `document_from_file()` now correctly rejects non-PDF files. The Gemini API only supports `application/pdf` for document content type. For text-based files (CSV, TXT, JSON, etc.), read the file and send as `Content::text()` instead.
- `FileSearchResult` now serializes `call_id` with snake_case per API spec, not camelCase `callId`
- `CodeExecutionCall` now serializes with nested `arguments` object containing `language` and `code` per API spec
- `GoogleSearchResultItem.rendered_content` now uses snake_case per API spec

### Changed

- **BREAKING**: Removed `CodeExecutionOutcome` enum - actual wire format uses `is_error: bool` and `result: String` fields directly, not `outcome`/`output` as documented
- `CodeExecutionResultInfo` now has `is_error: bool` and `result: &str` fields instead of `outcome: CodeExecutionOutcome` and `output: &str`
- `Content::CodeExecutionResult` variant now uses `is_error: bool, result: String` instead of `outcome: CodeExecutionOutcome, output: String`
- `InteractionResponse::successful_code_output()` now checks `!is_error` instead of `outcome.is_success()`
- **BREAKING**: `FunctionCallInfo` and `OwnedFunctionCallInfo` no longer have `thought_signature` field - API never sends this on function calls
- Renamed `Content::new_function_call_with_signature()` to `Content::function_call_with_id()` and removed `thought_signature` parameter
- Renamed `function_call_content_with_signature()` to `function_call_content_with_id()` and removed `thought_signature` parameter

### Removed

- **BREAKING**: `CodeExecutionOutcome` enum - the actual API wire format doesn't use this enum
- **BREAKING**: `thought_signature` field from `Content::FunctionCall` variant - API does not send this field on function calls (thought signatures appear only on `Thought` content blocks)

### Migration Guide

**Type rename - `InteractionContent` → `Content`:**
```rust
// Before (0.6.0)
use genai_rs::InteractionContent;
let content = InteractionContent::new_text("Hello");

// After (0.7.0)
use genai_rs::Content;
let content = Content::text("Hello");  // Static constructor
```

**Multimodal content - `add_*()` methods removed:**
```rust
// Before (0.6.0)
let response = client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Describe this image")
    .add_image_file("photo.jpg").await?
    .create()
    .await?;

// After (0.7.0) - Option A: Content constructors
let response = client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_content(vec![
        Content::text("Describe this image"),
        Content::image_data(base64_data, "image/png"),
    ])
    .create()
    .await?;

// After (0.7.0) - Option B: File helpers
use genai_rs::image_from_file;
let image = image_from_file("photo.jpg").await?;
let response = client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_content(vec![
        Content::text("Describe this image"),
        image,
    ])
    .create()
    .await?;
```

**Files API - `add_file()` removed:**
```rust
// Before (0.6.0)
let file = client.upload_file("video.mp4").await?;
let response = client.interaction()
    .with_model("gemini-3-flash-preview")
    .add_file(&file)
    .with_text("Describe this video")
    .create()
    .await?;

// After (0.7.0)
let file = client.upload_file("video.mp4").await?;
let response = client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_content(vec![
        Content::text("Describe this video"),
        Content::from_file(&file),
    ])
    .create()
    .await?;
```

**Resolution control:**
```rust
// Before (0.6.0)
.add_image_data_with_resolution(base64, "image/png", Resolution::High)

// After (0.7.0) - Constructor
Content::image_data_with_resolution(base64, "image/png", Resolution::High)

// After (0.7.0) - Builder chain
Content::image_data(base64, "image/png").with_resolution(Resolution::High)
```

**`with_turns()` renamed to `with_history()` and composes with `with_text()`:**
```rust
// Before (0.6.0)
// with_turns().with_text() silently overwrote history - bug!
let response = client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_turns(history)
    .create()
    .await?;

// After (0.7.0)
// Renamed to with_history(), and now composes correctly with with_text()
let response = client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_history(history)
    .with_text("Current message")  // Appended as final user turn
    .create()
    .await?;
// Produces: [...history, Turn::user("Current message")]
// Order doesn't matter - with_text().with_history() produces same result
```

**`CodeExecutionOutcome` removal:**
```rust
// Before
if result.outcome.is_success() {
    println!("Output: {}", result.output);
}

// After
if !result.is_error {
    println!("Output: {}", result.result);
}
```

**`thought_signature` removal from FunctionCall:**
```rust
// Before
let call = InteractionContent::new_function_call_with_signature(
    Some("call_123"),
    "get_weather",
    json!({"location": "SF"}),
    Some("signature".to_string())  // No longer needed - API doesn't send this
);
if let InteractionContent::FunctionCall { thought_signature, .. } = content {
    // thought_signature was always None
}

// After
let call = InteractionContent::new_function_call_with_id(
    Some("call_123"),
    "get_weather",
    json!({"location": "SF"})
);
// Note: Thought signatures appear on Thought content blocks, not function calls.
// Use response.thought_signatures() to iterate over them.
```

## [0.6.0] - 2025-01-11

### Added

- `InteractionBuilder::build()`: Build requests without executing, enabling retry patterns and request serialization
- `Client::execute()` and `Client::execute_stream()`: Execute pre-built `InteractionRequest` objects
- `GenaiError::is_retryable()`: Helper to identify transient errors (429, 5xx, timeouts) for retry logic
- `GenaiError::Api::retry_after`: Extracts `Retry-After` header from 429 rate limit responses (seconds or HTTP date format)
- `GenaiError::retry_after()`: Accessor method for the retry delay (consistent with `is_retryable()` pattern)
- `Deserialize` derive on `InteractionRequest`: Enables loading requests from JSON/config files
- `#[tracing::instrument]` on `execute()` and `execute_stream()`: Automatic span creation with model/agent context
- `docs/RETRY_PATTERNS.md`: Documents retry philosophy and recommended patterns using `backon` crate
- `examples/retry_with_backoff.rs`: Demonstrates retry patterns using the `backon` crate

### Changed

- **BREAKING**: Renamed `CreateInteractionRequest` to `InteractionRequest` for consistency
- **BREAKING**: Migrated from `log` crate to `tracing` crate for structured logging and spans
- Updated `docs/LOGGING_STRATEGY.md` to document tracing integration and instrumentation patterns

### Removed

- **BREAKING**: `Client::create_interaction()` - use `Client::execute()` instead
- **BREAKING**: `Client::create_interaction_stream()` - use `Client::execute_stream()` instead

### Migration Guide

**`create_interaction()` → `execute()`:**
```rust
// Before
let response = client.create_interaction(request).await?;
let stream = client.create_interaction_stream(request);

// After
let response = client.execute(request).await?;
let stream = client.execute_stream(request);
```

**`CreateInteractionRequest` → `InteractionRequest`:**
```rust
// Before
use genai_rs::CreateInteractionRequest;

// After
use genai_rs::InteractionRequest;
```

**`log` → `tracing`:**
If you were filtering logs with `RUST_LOG=genai_rs=debug`, this continues to work.
For tracing subscribers, use `tracing_subscriber` instead of `env_logger`:

```rust
// Before (with env_logger)
env_logger::init();

// After (with tracing-subscriber)
tracing_subscriber::fmt::init();
```

## [0.5.3] - 2026-01-10

### Fixed

- Release workflow: Use `--tests` to exclude doctests from `--include-ignored` run (v0.5.2 release workflow failed because `--include-ignored` compiles `ignore` doctest snippets)

## [0.5.2] - 2026-01-10

### Fixed

- Doctest compilation failures in `INTERACTIONS_API_FEEDBACK.md` (missing `ignore` annotation)
- Updated `thought_content()` docstring to clarify it's for testing only (API rejects thought blocks in user input)

### Changed

- `INTERACTIONS_API_FEEDBACK.md`: Downgraded thought signature issue from P0 to P2, clarified that signatures ARE present on `Thought` outputs (not `FunctionCall`), and documented that API rejects thought blocks in user input

## [0.5.1] - 2026-01-10

### Fixed

- Streaming tests no longer assert `event_id` presence (optional per API spec)
- `test_get_interaction_stream` handles API not replaying completed interactions
- Updated `STREAMING_API.md` with notes about optional `event_id` field

## [0.5.0] - 2026-01-10

### Added

- `docs/INTERACTIONS_API_FEEDBACK.md`: Comprehensive feedback report for Google Gemini API team documenting 9 issues discovered while building genai-rs
- Thought signature test coverage across 7 configurations (stateful, stateless, parallel, sequential, ThinkingLevel::High, FunctionCallingMode::Any, streaming)
- `test_speech_config_nested_format_fails_flat_succeeds`: Test proving only flat SpeechConfig format works (nested format returns 400)

### Changed

- `docs/ENUM_WIRE_FORMATS.md`: Updated SpeechConfig section - nested format fails with 400 error
- `docs/MULTI_TURN_FUNCTION_CALLING.md`: Added thought signature matrix with verified test links

### BREAKING CHANGES

#### Enum Unknown Variant Upgrade (#329)

Three enums upgraded from `#[serde(other)]` fallback to full Evergreen Unknown variant pattern. This enables logging and debugging of unrecognized API values.

**Affected types:**
- `UrlRetrievalStatus`: Variant renames for consistency
- `CodeExecutionOutcome`: Full Unknown pattern with data preservation
- `CodeExecutionLanguage`: Full Unknown pattern, `Unspecified` variant removed

**UrlRetrievalStatus variant renames:**
| Before | After |
|--------|-------|
| `UrlRetrievalStatusUnspecified` | `Unspecified` |
| `UrlRetrievalStatusSuccess` | `Success` |
| `UrlRetrievalStatusUnsafe` | `Unsafe` |
| `UrlRetrievalStatusError` | `Error` |

**CodeExecutionLanguage changes:**
- `Unspecified` variant removed (API only returns known languages)
- `Unknown { language_type, data }` variant added for forward compatibility

**Copy trait removed** from all three types (Unknown variants contain `serde_json::Value`).

**Migration guide:**

```rust
// UrlRetrievalStatus: Update variant names
// Before:
match status {
    UrlRetrievalStatus::UrlRetrievalStatusSuccess => { ... }
    UrlRetrievalStatus::UrlRetrievalStatusError => { ... }
    _ => { ... }
}

// After:
match status {
    UrlRetrievalStatus::Success => { ... }
    UrlRetrievalStatus::Error => { ... }
    UrlRetrievalStatus::Unknown { status_type, .. } => {
        log::warn!("Unknown status: {}", status_type);
    }
    _ => { ... }
}

// CodeExecutionLanguage: Handle Unknown instead of Unspecified
// Before:
match language {
    CodeExecutionLanguage::Python => { ... }
    CodeExecutionLanguage::Unspecified => { ... }
}

// After:
match language {
    CodeExecutionLanguage::Python => { ... }
    CodeExecutionLanguage::Unknown { language_type, .. } => {
        log::warn!("Unknown language: {}", language_type);
    }
    _ => { ... }
}

// Copy trait removal: Use .clone() where needed
// Before:
let outcome = *some_outcome_ref;

// After:
let outcome = some_outcome_ref.clone();
```

#### InteractionContent Field Type Audit (#318)

Wire format alignment fixes for `InteractionContent` variants. These changes fix critical mismatches where real API data was silently falling back to `Unknown` variants.

- **`InteractionContent::Thought`**: Field `text` renamed to `signature`
  - Thoughts contain cryptographic signatures for verification, not human-readable reasoning
  - Use `response.thought_signatures()` to iterate over signatures
  - Use `response.has_thoughts()` to check for thought presence

- **`InteractionContent::UrlContextCall`**: Field `url` split into `id` + `urls`
  - `id: String` - Call identifier for matching results
  - `urls: Vec<String>` - List of URLs requested
  - Use `response.url_context_call_id()` and `response.url_context_call_urls()`

- **`InteractionContent::UrlContextResult`**: Fields `url`/`content` replaced with `call_id` + `result`
  - `call_id: String` - Matches the corresponding call
  - `result: Vec<UrlContextResultItem>` - Results for each URL
  - New `UrlContextResultItem` type with `url`, `status` fields and `is_success()`/`is_error()`/`is_unsafe()` helpers

- **`InteractionResponse::thoughts()`**: Method removed
  - Was returning signatures but named incorrectly
  - Use `thought_signatures()` instead

- **`InteractionResponse::url_context_call()`**: Method renamed to `url_context_call_id()`
  - New `url_context_call_urls()` method returns the list of URLs

**Migration guide:**

```rust
// Before: Thought had text field
InteractionContent::Thought { text: Some(t) } => println!("{}", t);

// After: Thought has signature field (cryptographic, not readable)
InteractionContent::Thought { signature: Some(s) } => {
    // s is a cryptographic signature, not human-readable text
    println!("Has thought signature: {}", s.len() > 0);
}

// Before: UrlContextCall had single url
InteractionContent::UrlContextCall { url } => println!("{}", url);

// After: UrlContextCall has id + urls
InteractionContent::UrlContextCall { id, urls } => {
    println!("Call {}: {:?}", id, urls);
}

// Before: UrlContextResult had url/content
InteractionContent::UrlContextResult { url, content } => { ... }

// After: UrlContextResult has call_id + result array
InteractionContent::UrlContextResult { call_id, result } => {
    for item in result {
        if item.is_success() {
            println!("Fetched: {}", item.url);
        }
    }
}

// Before: Using thoughts() method
for thought in response.thoughts() { ... }

// After: Use thought_signatures()
for sig in response.thought_signatures() { ... }
```

### Added

- **`UrlContextResultItem` type** (#318): New struct for URL context result items
  - `url: String` - The URL that was fetched
  - `status: String` - Result status ("success", "error", "unsafe")
  - Helper methods: `is_success()`, `is_error()`, `is_unsafe()`

- **`UsageMetadata::total_thought_tokens` field** (#318): Token count for thinking/reasoning
  - Use `response.thought_tokens()` helper method

### Fixed

- **`CodeExecutionResult` outcome derivation** (#318): When `is_error` is `None`, outcome now correctly defaults to `Ok` instead of `Unspecified`

## [0.4.0] - 2026-01-08

### BREAKING CHANGES

#### Crate Renamed to `genai-rs`
- **`rust-genai` is now `genai-rs`** - Update your Cargo.toml dependencies
- **`rust-genai-macros` is now `genai-rs-macros`** - Update macro imports
- Change `use rust_genai::*` to `use genai_rs::*` in your code

#### MSRV Bumped to Rust 1.88
- **Minimum Supported Rust Version is now 1.88** (was 1.85)
- Required for Edition 2024 `let` chains feature
- Update your Rust toolchain: `rustup update`

#### Crate Consolidation (#302)
- **`genai-client` crate merged into `genai-rs`**
- Internal HTTP and type modules are now `pub(crate)` instead of separate crate
- Users only depend on `genai-rs` - no change to public API

### Added

#### Text-to-Speech Audio Output (#303)
- **New `with_audio_output()` method** - Generate speech from text
- **New `with_voice(name)` method** - Select voice (Kore, Puck, Aoede, etc.)
- **New `with_speech_config(SpeechConfig)` method** - Full voice/language/speaker control
- **New `SpeechConfig` type** with constructors:
  - `SpeechConfig::with_voice("Kore")`
  - `SpeechConfig::with_voice_and_language("Puck", "en-GB")`
- **New response helpers**: `first_audio()`, `audios()`, `has_audio()`
- **New `AudioInfo` type** with `bytes()`, `mime_type()`, `extension()` methods
- Use model `gemini-2.5-pro-preview-tts` for TTS

```rust
let response = client
    .interaction()
    .with_model("gemini-2.5-pro-preview-tts")
    .with_text("Hello, world!")
    .with_audio_output()
    .with_voice("Kore")
    .create()
    .await?;

if let Some(audio) = response.first_audio() {
    std::fs::write("speech.wav", audio.bytes()?)?;
}
```

#### New Built-in Tools
- **File Search tool (#299)** - Semantic document retrieval from vector stores
  - New `with_file_search(store_ids)` method
- **Computer Use tool (#298)** - Browser automation via Gemini
  - New `with_computer_use()` method (requires allowlisted API key)
- **MCP Server convenience (#295)** - Connect to Model Context Protocol servers
  - New `add_mcp_server(uri)` method

#### Explicit Multi-Turn Conversations (#296)
- **New `with_turns(Vec<Turn>)` method** - Provide full conversation history
- **New `Turn` type** with `user()` and `model()` constructors
- Alternative to `previous_interaction_id` for stateless deployments

#### Typed Agent Configuration (#293)
- **New `AgentConfig` type** for Deep Research and Dynamic agents
- **`DeepResearchConfig`** with `with_thinking_summaries()` builder
- **`DynamicConfig`** for dynamic agent interactions
- Use `with_agent_config(config.into())` on builder

#### Resolution Control for Media (#297)
- **New `with_resolution(MediaResolution)` method** on `ImageInput` and `VideoInput`
- Control processing resolution: `Low`, `Medium`, `High`, `Native`

### Infrastructure

#### CI/CD Workflows
- **Automated crates.io publishing** on version tags
- **Release Drafter** for automatic release notes from PR labels
- **MSRV Check** (Rust 1.88), **Cross-platform testing** (Linux, macOS, Windows)
- **Code coverage** with Codecov integration
- **Security audit** with cargo-audit

#### Comprehensive Documentation
- 12 new documentation guides in `docs/`
- New `TROUBLESHOOTING.md` for common issues

### Fixed

- **ThinkingSummaries wire format** (#272): Fixed serialization to use `THINKING_SUMMARIES_AUTO` and `THINKING_SUMMARIES_NONE` (API's actual wire format) instead of `auto`/`none` (what the documentation claims). This enables `agent_config` with `thinking_summaries` to work correctly with the Deep Research agent.
- **Clippy lints for Rust 1.92** - Use `is_multiple_of()` and collapsed `if let` chains

### BREAKING CHANGES

#### Timestamp Fields Use chrono::DateTime<Utc> (#273)
- **`FileMetadata.create_time`**: Changed from `Option<String>` to `Option<DateTime<Utc>>`
- **`FileMetadata.expiration_time`**: Changed from `Option<String>` to `Option<DateTime<Utc>>`
- **`InteractionResponse`**: Added `created: Option<DateTime<Utc>>` and `updated: Option<DateTime<Utc>>` fields
- **New dependency**: `chrono` crate with serde support
- Internal `loud_wire.rs` timestamp generation simplified to use chrono

**Migration guide:**
```rust
// Before (FileMetadata timestamps were strings):
if let Some(created) = file.create_time {
    println!("Created: {}", created);  // String
}

// After (timestamps are DateTime<Utc>):
use chrono::{DateTime, Utc};
if let Some(created) = file.create_time {
    println!("Created: {}", created.to_rfc3339());  // DateTime<Utc>
    // Or use chrono's formatting:
    println!("Created: {}", created.format("%Y-%m-%d %H:%M:%S"));
}

// InteractionResponse now has created/updated fields:
if let Some(created) = response.created {
    println!("Interaction created at: {}", created);
}
```

#### Streaming Returns StreamEvent Wrapper (#262)
- **`create_stream()`** now returns `Stream<Item = Result<StreamEvent, GenaiError>>` instead of `Stream<Item = Result<StreamChunk, GenaiError>>`
- **`create_stream_with_auto_functions()`** now returns `Stream<Item = Result<AutoFunctionStreamEvent, GenaiError>>` instead of `Stream<Item = Result<AutoFunctionStreamChunk, GenaiError>>`
- **New `StreamEvent` struct**: Wraps `StreamChunk` with `event_id` field for stream resume support
- **New `AutoFunctionStreamEvent` struct**: Wraps `AutoFunctionStreamChunk` with `event_id` field
- **New `get_interaction_stream()` method**: Resume streams from a specific `event_id` position

**Migration guide:**
```rust
// Before:
while let Some(chunk) = stream.next().await {
    match chunk? {
        StreamChunk::Delta(content) => { /* ... */ }
        StreamChunk::Complete(response) => { /* ... */ }
        _ => {}
    }
}

// After:
while let Some(result) = stream.next().await {
    let event = result?;
    // Optionally track event_id for resume support
    if let Some(id) = &event.event_id {
        last_event_id = Some(id.clone());
    }
    match event.chunk {  // Access .chunk on the event
        StreamChunk::Delta(content) => { /* ... */ }
        StreamChunk::Complete(response) => { /* ... */ }
        _ => {}
    }
}

// To resume an interrupted stream:
let resumed = client.get_interaction_stream(&interaction_id, Some(&last_event_id));
```

#### Additional Enums Now #[non_exhaustive] (#196)
- **`GenaiError`**: Match statements must include a wildcard arm
- **`FunctionError`**: Match statements must include a wildcard arm
- **`InteractionInput`**: Match statements must include a wildcard arm
- **New `GenaiError::MalformedResponse` variant**: For cases where the API returns 200 OK but with unexpected/malformed content
- This follows [Evergreen principles](https://github.com/google-deepmind/evergreen-spec) for forward-compatible API design

**Migration guide:**
```rust
// Before (exhaustive match):
match error {
    GenaiError::Http(e) => ...,
    GenaiError::Api { .. } => ...,
    // etc.
}

// After (must include wildcard):
match error {
    GenaiError::Http(e) => ...,
    GenaiError::Api { .. } => ...,
    GenaiError::MalformedResponse(msg) => ...,
    _ => ...,  // Required for forward compatibility
}
```

#### URI Content Helpers Require mime_type (#131)
- **Changed signatures**: `image_uri_content()`, `audio_uri_content()`, `video_uri_content()`, `document_uri_content()` now require `mime_type` as a mandatory parameter instead of `Option<String>`
- **Rationale**: Gemini API requires mime_type for URI-based content; making this compile-time enforced prevents runtime API errors

**Migration guide:**
```rust
// Before:
image_uri_content("https://example.com/image.png", Some("image/png".to_string()))

// After:
image_uri_content("https://example.com/image.png", "image/png")
```

#### Tool Enum is Now #[non_exhaustive] (#131)
- **`Tool` enum now includes `#[non_exhaustive]`**: Match statements must include a wildcard arm
- **New `Tool::Unknown` variant**: Captures unrecognized tool types from the API without failing deserialization
- This follows [Evergreen principles](https://github.com/google-deepmind/evergreen-spec) for forward-compatible API design

#### Error Type Consolidation (#131)
- **`InternalError` renamed to `GenaiError`** in `genai-client` crate
- New `Internal` and `InvalidInput` variants for better error categorization
- Users of the public `genai-rs` crate are unaffected (uses the same `GenaiError`)

#### create_with_auto_functions() Returns AutoFunctionResult (#148)
- **Changed return type**: `create_with_auto_functions()` now returns `AutoFunctionResult` instead of `InteractionResponse`
- **New `AutoFunctionResult` type**: Contains both the final response and execution history
- Provides visibility into which functions were called, enabling debugging, logging, and evaluation

**Migration guide:**
```rust
// Before:
let response = builder.create_with_auto_functions().await?;
println!("{}", response.text().unwrap());

// After:
let result = builder.create_with_auto_functions().await?;
println!("{}", result.response.text().unwrap());

// New: Access execution history with timing
for exec in &result.executions {
    println!("Called {} ({:?}) -> {}", exec.name, exec.duration, exec.result);
}
```

### Added

- **Request timeout and token usage helpers** (#228):
  - New `with_timeout(Duration)` on `InteractionBuilder` for per-request timeouts
  - For `create()`: Overall request timeout
  - For `create_stream()`: Per-chunk timeout to detect stalled connections
  - New `GenaiError::Timeout(Duration)` variant returned when requests exceed timeout
  - Token usage helper methods on `InteractionResponse`:
    - `input_tokens()`, `output_tokens()`, `total_tokens()`
    - `reasoning_tokens()`, `cached_tokens()`, `tool_use_tokens()`
  - Warning logged when timeout used with auto-function methods (not yet supported)

- **ToolService trait for dependency injection** (#197):
  - New `ToolService` trait enables tools to access shared state (DB connections, API clients, config)
  - Use `with_tool_service(Arc<dyn ToolService>)` on `InteractionBuilder` to provide tools
  - Service-provided functions take precedence over global `#[tool]` registry functions
  - When a service function shadows a global function, a warning is logged
  - Works with both `create_with_auto_functions()` and `create_stream_with_auto_functions()`

- **Partial results when max_function_call_loops exceeded** (#172):
  - `create_with_auto_functions()` now returns partial results instead of error when limit is hit
  - New `reached_max_loops: bool` field on `AutoFunctionResult` indicates if limit was reached
  - The `response` field contains the last API response (likely with pending function calls)
  - The `executions` vector preserves all function calls that were executed before hitting the limit
  - Enables debugging stuck function loops and accessing partial work
  - New `AutoFunctionStreamChunk::MaxLoopsReached` variant for streaming (parallel change)
  - `AutoFunctionResultAccumulator` now handles `MaxLoopsReached` and sets `reached_max_loops: true`
  - Legacy JSON without `reached_max_loops` field deserializes with default `false`

- **Function execution timing** (#148):
  - `FunctionExecutionResult.duration` tracks how long each function took to execute
  - Duration is serialized as milliseconds for JSON compatibility
  - Useful for performance monitoring, debugging, and optimization

- **Streaming accumulator helper** (#148):
  - New `AutoFunctionResultAccumulator` type to collect `AutoFunctionResult` from streaming
  - Allows combining streaming UI updates with execution history collection
  - Example:
    ```rust
    let mut accumulator = AutoFunctionResultAccumulator::new();
    while let Some(chunk) = stream.next().await {
        if let Some(result) = accumulator.push(chunk?) {
            // Stream complete, result contains full execution history
            println!("Executed {} functions", result.executions.len());
        }
    }
    ```

- **Full `Serialize`/`Deserialize` support for save/resume semantics** (#148, #151):
  - `InteractionResponse` now implements `Serialize` for logging, caching, and persistence
  - `AutoFunctionResult` implements `Serialize` and `Deserialize` for full execution history
  - `FunctionExecutionResult` now implements `Deserialize` for roundtrip serialization
  - `StreamChunk` and `AutoFunctionStreamChunk` implement both traits for streaming event replay
  - New `AutoFunctionStreamChunk::Unknown` variant for forward-compatible deserialization
  - Enables offline replay, testing/mocking, and state persistence for long-running agents

- **New convenience helpers on `InteractionResponse`** (#131):
  - `google_search_call()` - returns first Google Search call (singular)
  - `code_execution_call()` - returns first Code Execution call (singular)
  - `url_context_call()` - returns first URL Context call (singular)

#### Unified Streaming Content Types (#39, #27)
- **`StreamDelta` enum removed**: Streaming deltas now use `InteractionContent` directly
  - `StreamChunk::Delta(InteractionContent)` contains incremental content during streaming
  - `StreamChunk::Complete(InteractionResponse)` contains the final complete response
- **New `InteractionContent::ThoughtSignature` variant**: Captures streaming thought signatures
- **New helper methods on `InteractionContent`**: `text()`, `is_text()`, `is_thought()`, `is_thought_signature()`, `is_function_call()`
- **New type exported**: `StreamChunk` (note: `StreamDelta` is no longer exported)

**Migration guide:**
```rust
// Before:
match chunk {
    StreamChunk::Delta(delta) => match delta {
        StreamDelta::Text { text } => println!("{}", text),
        StreamDelta::Thought { text } => println!("[thinking: {}]", text),
        _ => {}
    }
    StreamChunk::Complete(response) => { /* ... */ }
}

// After:
match chunk {
    StreamChunk::Delta(content) => match content {
        InteractionContent::Text { text } => println!("{}", text.as_deref().unwrap_or("")),
        InteractionContent::Thought { text } => println!("[thinking: {}]", text.as_deref().unwrap_or("")),
        InteractionContent::FunctionCall { name, args, .. } => {
            println!("Function call: {}({:?})", name, args);
        }
        _ => {}
    }
    StreamChunk::Complete(response) => { /* ... */ }
}

// Helper methods still work the same:
if let Some(text) = delta.text() { /* ... */ }
```

### Added
- **Google Search grounding support** (#25): Enable real-time web search integration with Gemini models
  - New `with_google_search()` builder method on `InteractionBuilder`
  - New types: `GroundingMetadata`, `GroundingChunk`, `WebSource`
  - New helper methods: `has_google_search_metadata()`, `google_search_metadata()`, `has_google_search_calls()`, `google_search_calls()` on `InteractionResponse`
  - Full streaming support via `StreamChunk::Complete`

- **Code execution support** (#26): Enable Python code execution via Gemini's built-in sandbox
  - New `with_code_execution()` builder method on `InteractionBuilder`
  - New `CodeExecutionOutcome` enum with `Ok`, `Failed`, `DeadlineExceeded`, `Unspecified` variants
  - Updated `InteractionContent::CodeExecutionCall` with typed fields: `id`, `language`, `code`
  - Updated `InteractionContent::CodeExecutionResult` with typed fields: `call_id`, `outcome`, `output`
  - New helper methods on `InteractionResponse`: `code_execution_calls()`, `code_execution_results()`, `successful_code_output()`
  - New helper functions: `code_execution_call_content()`, `code_execution_result_content()`, `code_execution_success()`, `code_execution_error()`
  - Backward-compatible deserialization for old API response format
  - **Breaking (serialization)**: `CodeExecutionCall` now serializes `language` and `code` as top-level fields instead of nested in `arguments`. Deserialization remains backward-compatible with both formats.

- **URL context support** (#63): Enable URL content fetching and analysis
  - New `with_url_context()` builder method on `InteractionBuilder`
  - New types: `UrlContextMetadata`, `UrlMetadataEntry`, `UrlRetrievalStatus`
  - New helper methods: `has_url_context_metadata()`, `url_context_metadata()`, `has_url_context_calls()`, `url_context_calls()` on `InteractionResponse`
  - Supports up to 20 URLs per request, max 34MB per URL

- **Structured output JSON schema support** (#80): Enforce JSON schema constraints on model responses
  - Use `.with_response_format(schema)` to specify a JSON schema for structured output
  - Works standalone for structured data extraction
  - Combines with built-in tools (Google Search, URL Context)
  - New comprehensive example: `examples/structured_output.rs`

- **Function call/result structs**: New `FunctionCallInfo` and `FunctionResultInfo` structs with named fields for cleaner access
  - `function_calls()` now returns `Vec<FunctionCallInfo>` with fields: `id`, `name`, `args`, `thought_signature`
  - `function_results()` now returns `Vec<FunctionResultInfo>` with fields: `name`, `call_id`, `result`
  - New `has_function_results()` method on `InteractionResponse` for parity with `has_function_calls()`

- **Logging strategy documentation** (#203): New `docs/LOGGING_STRATEGY.md` with comprehensive guidelines
  - Log level definitions (error/warn/debug) with concrete examples
  - Sensitive data handling (API keys redacted, user content at debug only)
  - Evergreen pattern logging (Unknown variants log at warn level)
  - Debug logging for auto-function loop lifecycle (iteration tracking, execution timing)
  - Enable with `RUST_LOG=genai_rs=debug`

### Changed
- **`InteractionContent` is now `#[non_exhaustive]`** (#44): Match statements must include a wildcard arm (`_ => {}`). This allows adding new variants in minor version updates without breaking downstream code.
- **Deep Research example now requires background mode** (#179): Updated `deep_research.rs` example to reflect API requirement that `background=true` is mandatory for agent interactions. Removed synchronous mode demonstration since it is no longer supported by the API.
- **Function execution failures now log at `warn!` instead of `error!`** (#203): Since function failures are recoverable (the error is sent to the model which can retry or adapt), they are now correctly logged as warnings rather than errors. This aligns with the new logging strategy documented in `docs/LOGGING_STRATEGY.md`.

### Fixed
- **Streaming with function calls now works** (#27): Function call deltas are now properly parsed instead of causing errors
- **Streaming now properly yields content chunks** (#17): The streaming API was returning 0 chunks because the code expected all SSE events to have an `interaction` field, but the API sends different event types (`content.delta` and `interaction.complete`)

#### Simplified Client API
- **`Client::new()` signature simplified**: No longer takes `api_version` parameter
  - Before: `Client::new(api_key, None)`
  - After: `Client::new(api_key)`
  - The `api_version` was stored but never used; the library defaults to V1Beta internally
- **`ApiVersion` no longer re-exported** from genai-rs (still available in genai-client for internal use)

#### Removed deprecated function calling helpers
- **`function_response_content()` helper removed**: Use `function_result_content()` instead
  - Before: `function_response_content("get_weather", json!({"temp": 72}))`
  - After: `function_result_content("get_weather", "call_123", json!({"temp": 72}))`
  - The `call_id` parameter is required for proper API response matching
- **`InteractionContent::FunctionResponse` variant removed**: Use `FunctionResult` variant instead

#### UsageMetadata field names updated (#24)
- **Field names now match Interactions API**: The old GenerateContent API field names have been replaced
  - `prompt_tokens` → `total_input_tokens`
  - `candidates_tokens` → `total_output_tokens`
  - `total_tokens` remains unchanged
- **New fields added**: `total_cached_tokens`, `total_reasoning_tokens`, `total_tool_use_tokens`
- **Token usage now works**: Previously always returned `None` due to field name mismatch

## [0.2.0] - 2025-12-23

### BREAKING CHANGES

This release removes the legacy GenerateContent API in favor of the unified Interactions API. This is a major breaking change that requires code migration.

#### Removed
- **GenerateContent API**: All `GenerateContentBuilder` methods and related functionality removed
  - `Client::with_model()` method removed
  - `GenerateContentBuilder` type removed
  - `generate_from_request()` and `stream_from_request()` methods removed
  - `GenerateContentResponse` type removed (use `InteractionResponse` instead)

- **Helper modules**:
  - `content_api` module removed (use `interactions_api` instead)
  - `internal/response_processing` module removed

- **Examples**: Removed all GenerateContent examples
  - `simple_request.rs`
  - `stream_request.rs`
  - `code_execution.rs`
  - `function_call.rs`
  - `gemini3_thought_signatures.rs`

- **Internal crates**:
  - `genai-client/src/core.rs` removed
  - `genai-client/src/models/request.rs` removed
  - `genai-client/src/models/response.rs` removed

### Added

- **Enhanced InteractionResponse**:
  - New `.text()` convenience method to extract text from interaction responses
  - New `.function_calls()` convenience method to extract function calls with thought signatures

- **Automatic function calling for Interactions API**:
  - New `InteractionBuilder::create_with_auto_functions()` method
  - Auto-discovers and executes functions from the global registry
  - Supports multi-turn function calling with automatic loop handling

- **New helper functions**:
  - `function_result_content()` for sending function execution results (correct API format)
  - Enhanced `function_call_content_with_signature()` to include optional call ID

### Fixed

- **Function calling implementation** now correctly follows Google's Interactions API specification:
  - Added `id` field to `FunctionCall` to capture the call identifier from the API
  - Added new `FunctionResult` content type with `call_id` field (replaces `FunctionResponse`)
  - `create_with_auto_functions()` now sends only function results (not the original calls)
  - The API server maintains function call context via `previous_interaction_id`
  - Deprecated `FunctionResponse` variant (use `FunctionResult` instead)
  - Improved error message when max function call loops (5) is exceeded

### Changed

- **Primary API**: The Interactions API is now the only supported API
- **Migration Path**:
  - Replace `client.with_model(...).with_prompt(...).generate()`
  - With `client.interaction().with_model(...).with_text(...).create()`
  - Replace `generate_with_auto_functions()` with `create_with_auto_functions()`
  - Use `interactions_api` helper functions instead of `content_api`

### Migration Guide

#### Before (v0.1.x - GenerateContent API):
```rust
let response = client
    .with_model("gemini-3-flash-preview")
    .with_prompt("Hello, world!")
    .generate()
    .await?;

println!("{}", response.text.unwrap());
```

#### After (v0.2.0 - Interactions API):
```rust
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Hello, world!")
    .create()
    .await?;

println!("{}", response.text().unwrap_or("No text"));
```

#### Streaming:
```rust
// Before
let stream = client
    .with_model("gemini-3-flash-preview")
    .with_prompt("Hello")
    .generate_stream()?;

// After
let stream = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Hello")
    .create_stream();
```

#### Automatic Function Calling:
```rust
// Before
let response = client
    .with_model("gemini-3-flash-preview")
    .with_prompt("What's the weather?")
    .generate_with_auto_functions()
    .await?;

// After
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("What's the weather?")
    .create_with_auto_functions()
    .await?;
```

## [0.1.0] - 2024-12-XX

### Added
- Initial release
- Support for GenerateContent API
- Support for Interactions API
- Function calling with automatic discovery via macros
- Streaming support for both APIs
- Comprehensive test suite
- Example programs for both APIs
