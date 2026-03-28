# Enum Wire Formats & Unknown Variants

This document captures:
1. **Wire formats** for enums in the Gemini Interactions API (the official docs are sometimes wrong)
2. **Unknown variant types** that implement Evergreen soft-typing for forward compatibility

## Types with Unknown Variants

All types below implement graceful handling of unrecognized values via an `Unknown` variant. This ensures the library doesn't break when Google adds new enum values.

| # | Type | Location | Context Field | Notes |
|---|------|----------|---------------|-------|
| 1 | `Resolution` | src/content.rs | `resolution_type` | Image/video quality |
| 2 | `Content` | src/content.rs | `content_type` | 18+ content types |
| 3 | `StreamChunk` | src/wire_streaming.rs | `chunk_type` | Low-level SSE chunks |
| 4 | `AutoFunctionStreamChunk` | src/streaming.rs | `chunk_type` | High-level streaming |
| 5 | `FileState` | src/http/files.rs | `state_type` | File upload states |
| 6 | `Tool` | src/tools.rs | `tool_type` | Tool types |
| 7 | `FunctionCallingMode` | src/tools.rs | `mode_type` | AUTO/ANY/NONE/VALIDATED |
| 8 | `Role` | src/request.rs | `role_type` | user/model/system |
| 9 | `ThinkingLevel` | src/request.rs | `level_type` | minimal/low/medium/high |
| 10 | `ThinkingSummaries` | src/request.rs | `summaries_type` | Context-dependent format |
| 11 | `InteractionStatus` | src/response.rs | `status_type` | Response status |
| 12 | `CodeExecutionLanguage` | src/content.rs | `language_type` | Programming language |
| 13 | `UrlRetrievalStatus` | src/response.rs | `status_type` | URL fetch status |
| 14 | `ImageAspectRatio` | src/request.rs | `ratio_type` | Image aspect ratios (14 values) |
| 15 | `ImageSize` | src/request.rs | `size_type` | Image resolution (512/1K/2K/4K) |
| 16 | `SearchType` | src/tools.rs | `search_type` | Google Search types (web_search/image_search) |

### Unknown Variant Pattern

All Unknown variants follow this naming convention:

```rust,ignore
Unknown {
    <context>_type: String,      // The unrecognized type from API
    data: serde_json::Value,     // Full JSON preserved for roundtrip
}
```

Helper methods on each type:
- `is_unknown()` - Check if this is an Unknown variant
- `unknown_<context>_type()` - Get the unrecognized type string
- `unknown_data()` - Get the preserved JSON data

### `strict-unknown` Feature Flag

- **Default (disabled)**: Unknown values deserialize into `Unknown` variant, logs warning
- **Strict mode (enabled)**: Unknown values cause deserialization error (fail-fast)
- Enable: `cargo test --features strict-unknown`
- CI runs dedicated `test-strict-unknown` job

## Quick Reference

| Enum | Wire Format | Example | Notes |
|------|-------------|---------|-------|
| `ThinkingSummaries` | `THINKING_SUMMARIES_*` | `"THINKING_SUMMARIES_AUTO"` | Docs claim `auto`/`none` - **wrong** |
| `ThinkingLevel` | lowercase | `"low"`, `"medium"`, `"high"` | Docs are correct |
| `FunctionCallingMode` | SCREAMING_CASE | `"AUTO"`, `"ANY"`, `"NONE"`, `"VALIDATED"` | |
| `InteractionStatus` | snake_case | `"in_progress"`, `"requires_action"` | Response-only |
| `Resolution` | snake_case | `"low"`, `"medium"`, `"high"`, `"ultra_high"` | Image/video content |
| `Tool::FileSearch` | snake_case object | `{"type": "file_search", ...}` | Rust: `store_names`, Wire: `file_search_store_names` |
| `FileSearchResult` | snake_case | `{"type": "file_search_result", "call_id": ...}` | Rust: `store`, Wire: `file_search_store` |
| `Content::GoogleMapsCall` | snake_case + arguments object | `{"type": "google_maps_call", "id": "...", "arguments": {"queries": [...]}}` |
| `Content::GoogleMapsResult` | snake_case + result array | `{"type": "google_maps_result", "call_id": "...", "result": [...]}` |
| `Tool::GoogleSearch` | snake_case + optional array | `{"type": "google_search", "search_types": ["web_search"]}` |
| `Tool::GoogleMaps` | snake_case + optional field | `{"type": "google_maps", "enable_widget": true}` |
| `SearchType` | snake_case string | `"web_search"`, `"image_search"` |
| `Tool::ComputerUse` | snake_case | `"computer_use"` | **UNVERIFIED** - from docs |
| `Content::ComputerUseCall` | snake_case | `"computer_use_call"` | **UNVERIFIED** - from docs |
| `Content::ComputerUseResult` | snake_case | `"computer_use_result"` | **UNVERIFIED** - from docs |
| `SpeechConfig` | camelCase fields | `{"voice": "Kore", "language": "en-US"}` | Inside generationConfig |
| Audio MIME type (TTS response) | with params | `"audio/L16;codec=pcm;rate=24000"` | Raw PCM audio |
| `Content::Thought` | snake_case + signature | `{"type": "thought", "signature": "..."}` | Cryptographic, not text |
| `Content::GoogleSearchCall` | snake_case + nested args | `{"type": "google_search_call", "id": "...", "arguments": {"queries": [...]}}` | Verified 2026-01-13 |
| `Content::GoogleSearchResult` | snake_case | `{"type": "google_search_result", "call_id": "...", "result": [...]}` | Verified 2026-01-13 |
| `GoogleSearchResultItem` | snake_case | `{"title": "...", "url": "...", "rendered_content": "..."}` | Verified 2026-01-13 - all snake_case |
| `Content::UrlContextCall` | snake_case + nested args | `{"type": "url_context_call", "id": "...", "arguments": {"urls": [...]}}` | Verified 2026-01-13 |
| `Content::UrlContextResult` | snake_case + result array | `{"type": "url_context_result", "call_id": "...", "result": [...]}` | Verified 2026-01-13 |
| `UrlContextResultItem` | snake_case | `{"url": "...", "status": "success"}` | Verified 2026-01-13 - no paywall field |
| `Content::CodeExecutionCall` | snake_case + nested args | `{"type": "code_execution_call", "id": "...", "arguments": {"language": "...", "code": "..."}}` | Verified 2026-01-13 |
| `Content::CodeExecutionResult` | snake_case + is_error/result | `{"type": "code_execution_result", "call_id": "...", "is_error": false, "result": "..."}` | Verified 2026-01-13 - no signature field |
| `CodeExecutionLanguage` | SCREAMING_SNAKE_CASE | `"PYTHON"` | Currently only Python supported |
| `UrlRetrievalStatus` | SCREAMING_SNAKE_CASE | `"URL_RETRIEVAL_STATUS_SUCCESS"` | URL fetch result status |
| `ImageAspectRatio` | ratio string | `"1:1"`, `"16:9"`, `"9:16"` | 14 aspect ratios |
| `ImageSize` | size string | `"512"`, `"1K"`, `"2K"`, `"4K"` | Image resolution |

## Details

### ThinkingSummaries (agent_config)

Used in `agent_config.thinking_summaries` for Deep Research agent.

```json
{
  "agent_config": {
    "type": "deep-research",
    "thinking_summaries": "THINKING_SUMMARIES_AUTO"
  }
}
```

| Rust Enum | Wire Value | Doc Claims (wrong) |
|-----------|------------|-------------------|
| `ThinkingSummaries::Auto` | `"THINKING_SUMMARIES_AUTO"` | `"auto"` |
| `ThinkingSummaries::None` | `"THINKING_SUMMARIES_NONE"` | `"none"` |

**Discovered**: 2026-01-04 - API returned `"unknown enum value: 'auto'"` until we tested the fully-qualified format.

### ThinkingLevel (generation_config)

Used in `generationConfig.thinkingLevel`.

```json
{
  "generationConfig": {
    "thinkingLevel": "low"
  }
}
```

| Rust Enum | Wire Value |
|-----------|------------|
| `ThinkingLevel::Minimal` | `"minimal"` |
| `ThinkingLevel::Low` | `"low"` |
| `ThinkingLevel::Medium` | `"medium"` |
| `ThinkingLevel::High` | `"high"` |

### FunctionCallingMode (generation_config)

Used in `generationConfig.toolChoice`.

```json
{
  "generationConfig": {
    "toolChoice": "ANY"
  }
}
```

| Rust Enum | Wire Value |
|-----------|------------|
| `FunctionCallingMode::Auto` | `"AUTO"` |
| `FunctionCallingMode::Any` | `"ANY"` |
| `FunctionCallingMode::None` | `"NONE"` |
| `FunctionCallingMode::Validated` | `"VALIDATED"` |

### InteractionStatus (response)

Returned in API responses - we only deserialize, never serialize.

| Rust Enum | Wire Value |
|-----------|------------|
| `InteractionStatus::Completed` | `"completed"` |
| `InteractionStatus::InProgress` | `"in_progress"` |
| `InteractionStatus::RequiresAction` | `"requires_action"` |
| `InteractionStatus::Failed` | `"failed"` |
| `InteractionStatus::Cancelled` | `"cancelled"` |
| `InteractionStatus::Incomplete` | `"incomplete"` | SDK-sourced, not yet in official API docs |

### Resolution (content)

Used in image and video content for quality vs. token cost trade-off.

```json
{
  "input": [{
    "type": "image",
    "data": "base64...",
    "mime_type": "image/png",
    "resolution": "low"
  }]
}
```

| Rust Enum | Wire Value |
|-----------|------------|
| `Resolution::Low` | `"low"` |
| `Resolution::Medium` | `"medium"` |
| `Resolution::High` | `"high"` |
| `Resolution::UltraHigh` | `"ultra_high"` |

**Verified**: 2026-01-05 - Tested with `LOUD_WIRE=1 cargo run --example multimodal_image`.

### Tool::FileSearch (request)

Used to enable semantic document retrieval from file search stores.

```json
{
  "tools": [{
    "type": "file_search",
    "file_search_store_names": ["stores/my-store-123"],
    "top_k": 10,
    "metadata_filter": "category = 'technical'"
  }]
}
```

| Rust Field | Wire Name | Required | Notes |
|------------|-----------|----------|-------|
| `store_names` | `file_search_store_names` | Yes | Array of store identifiers |
| `top_k` | `top_k` | No | Number of results to return |
| `metadata_filter` | `metadata_filter` | No | Filter expression |

**Note**: The RFC proposed `file_ids` but the actual API uses `file_search_store_names` (stores, not individual files).

**Verified**: 2026-01-05 - Request format tested with `LOUD_WIRE=1 cargo run --example file_search`.

### FileSearchResult (response content)

Returned when the model retrieves documents from file search stores.

```json
{
  "type": "file_search_result",
  "call_id": "call_abc123",
  "result": [
    {
      "title": "Document.pdf",
      "text": "Relevant content from the document...",
      "file_search_store": "stores/my-store-123"
    }
  ]
}
```

| Rust Field | Wire Name | Notes |
|------------|-----------|-------|
| `call_id` | `call_id` | snake_case in JSON |
| `result` | `result` | Array of FileSearchResultItem |
| `result[].title` | `title` | Document title |
| `result[].text` | `text` | Retrieved text snippet |
| `result[].store` | `file_search_store` | snake_case in JSON |

**Added**: 2026-01-05 - Response format based on API documentation. Response cannot be verified without configured file search stores.

### GoogleMapsCall (response content)

Wire type: `"google_maps_call"`

| Rust field | Wire field | Notes |
|-----------|-----------|-------|
| `id` | `id` | Unique call identifier |
| `queries` | `arguments.queries` | Nested inside `arguments` object |
| `signature` | `signature` | Optional, opaque backend validation |

Example wire format:
```json
{
  "type": "google_maps_call",
  "id": "qs19a0jm",
  "arguments": { "queries": ["coffee shops near Times Square"] },
  "signature": "ErIE..."
}
```

**Status**: Verified via `LOUD_WIRE=1` integration test — confirmed wire format matches.

### GoogleMapsResult (response content)

Wire type: `"google_maps_result"`

| Rust field | Wire field | Notes |
|-----------|-----------|-------|
| `call_id` | `call_id` | Maps call ID |
| `result` | `result` | Array of `GoogleMapsResultItem` |
| `signature` | `signature` | Optional, opaque backend validation |

Each `GoogleMapsResultItem` contains:
- `places`: Optional array of `Place` objects (with `name`, `formatted_address`, `place_id`, `lat`, `lng`, etc.)
- `widget_context_token`: Optional string for widget rendering

**Status**: Verified via `LOUD_WIRE=1` integration test — confirmed wire format matches.

### Computer Use (tool and content types)

**⚠️ UNVERIFIED** - Wire format derived from [Interactions API docs](https://ai.google.dev/static/api/interactions.md.txt). Pending verification with `LOUD_WIRE=1` once API access is available.

Tool request format (assumed):
```json
{
  "tools": [{
    "type": "computer_use",
    "environment": "browser",
    "excludedPredefinedFunctions": ["submit_form", "download"]
  }]
}
```

Content types (assumed):
```json
// ComputerUseCall (in response outputs)
{
  "type": "computer_use_call",
  "id": "call_123",
  "action": "navigate",
  "parameters": {"url": "https://example.com"}
}

// ComputerUseResult (in response outputs)
{
  "type": "computer_use_result",
  "call_id": "call_123",
  "success": true,
  "output": {"title": "Example Domain"},
  "screenshot": "base64..."
}
```

| Rust Type | Wire Value | Notes |
|-----------|------------|-------|
| `Tool::ComputerUse` | `"computer_use"` | Tool type |
| `Tool::ComputerUse.environment` | `"browser"` | Only supported value |
| `Tool::ComputerUse.excluded_predefined_functions` | `"excludedPredefinedFunctions"` | camelCase field name |
| `Content::ComputerUseCall` | `"computer_use_call"` | Content type |
| `Content::ComputerUseResult` | `"computer_use_result"` | Content type |

**TODO**: Verify with `LOUD_WIRE=1 cargo run --example computer_use` when API access is available.

### SpeechConfig (generation_config)

Used in `generationConfig.speechConfig` for text-to-speech audio output.

```json
{
  "model": "gemini-2.5-pro-preview-tts",
  "input": "Hello, world!",
  "generationConfig": {
    "responseModalities": ["AUDIO"],
    "speechConfig": {
      "voice": "Kore",
      "language": "en-US"
    }
  }
}
```

| Rust Field | Wire Name | Required | Notes |
|------------|-----------|----------|-------|
| `voice` | `voice` | No* | Voice name (e.g., "Kore", "Puck", "Charon") |
| `language` | `language` | Yes** | Language code (e.g., "en-US", "es-ES") |
| `speaker` | `speaker` | No | For multi-speaker TTS scenarios |

*Voice defaults to a system voice if not specified.
**Language is required by the API when voice is specified.

**Important**: The Google docs suggest a nested structure (`voiceConfig.prebuiltVoiceConfig.voiceName`) but **that format returns 400 error**. Only the flat structure shown above works with the Interactions API.

**Verified**: 2026-01-10 - Tested both formats in `test_speech_config_nested_format_fails_flat_succeeds`. Nested format fails with `no such field: 'voiceConfig'`.

### Audio Response (TTS output)

TTS responses return audio content with a specific MIME type:

```json
{
  "outputs": [{
    "type": "audio",
    "data": "base64-encoded-pcm-data...",
    "mime_type": "audio/L16;codec=pcm;rate=24000"
  }]
}
```

| MIME Type | Format | Notes |
|-----------|--------|-------|
| `audio/L16;codec=pcm;rate=24000` | Raw PCM | 16-bit linear PCM at 24kHz |

The `AudioInfo::extension()` method maps this to `"pcm"` for file saving.

**Verified**: 2026-01-07 - Response format captured from live TTS generation.

### Thought (response content)

Returned when the model uses internal reasoning (thinking mode).

```json
{
  "type": "thought",
  "signature": "Eq0JCqoJAXLI2nyuo7yupoglxIQxc5h0..."
}
```

| Rust Field | Wire Name | Notes |
|------------|-----------|-------|
| `signature` | `signature` | Cryptographic signature for verification, NOT readable text |

**Important**: The `signature` field contains a cryptographic value for thought verification, not human-readable reasoning. Use `response.has_thoughts()` to detect thinking and `response.thought_signatures()` to iterate.

**Verified**: 2026-01-09 - Captured from `LOUD_WIRE=1 cargo run --example thinking`. Previous incorrect assumption was that thoughts had a `text` field.

### UrlContextCall (response content)

Returned when the model requests URL content for context.

```json
{
  "type": "url_context_call",
  "id": "fpo8xd3s",
  "arguments": {
    "urls": ["https://example.com", "https://example.org"]
  }
}
```

| Rust Field | Wire Name | Notes |
|------------|-----------|-------|
| `id` | `id` | Call identifier for matching results |
| `urls` | `arguments.urls` | Array of URLs, nested inside `arguments` |

**Note**: The `urls` are nested inside an `arguments` object in the wire format. The library extracts them to a flat `urls: Vec<String>` field for convenience.

**Verified**: 2026-01-09 - Captured from `LOUD_WIRE=1 cargo run --example url_context`. Previous incorrect assumption was a single `url` field.

### UrlContextResult (response content)

Returned with the results of URL fetching.

```json
{
  "type": "url_context_result",
  "call_id": "fpo8xd3s",
  "result": [
    {
      "url": "https://example.com",
      "status": "success"
    },
    {
      "url": "https://example.org",
      "status": "error"
    }
  ]
}
```

| Rust Field | Wire Name | Notes |
|------------|-----------|-------|
| `call_id` | `call_id` | Matches the corresponding UrlContextCall |
| `result` | `result` | Array of UrlContextResultItem |
| `result[].url` | `url` | The URL that was fetched |
| `result[].status` | `status` | "success", "error", or "unsafe" |

**Note**: Each item in `result` is a `UrlContextResultItem` with helper methods `is_success()`, `is_error()`, and `is_unsafe()`.

**Verified**: 2026-01-09 - Captured from `LOUD_WIRE=1 cargo run --example url_context`. Previous incorrect assumption was `url`/`content` fields.

### CodeExecutionResult (content)

Returned when code execution completes. Uses simple `is_error` boolean and `result` string fields.

```json
{
  "type": "code_execution_result",
  "call_id": "exec_123",
  "is_error": false,
  "result": "Hello, World!\n"
}
```

| Rust Field | Wire Name | Type | Notes |
|------------|-----------|------|-------|
| `call_id` | `call_id` | `Option<String>` | Matches the CodeExecutionCall id |
| `is_error` | `is_error` | `bool` | `false` = success, `true` = error |
| `result` | `result` | `String` | Output text (stdout) or error message |

**Important**: The official API documentation mentions `outcome` enum with values like `OUTCOME_OK`, but the **actual wire format** uses `is_error: bool` and `result: String`. This was discovered via `LOUD_WIRE=1` testing.

**Verified**: 2026-01-12 - Captured from `LOUD_WIRE=1 cargo test test_code_execution -- --include-ignored --nocapture`. Actual wire format is `{"is_error": false, "result": "3628800\n"}`, NOT `{"outcome": "OUTCOME_OK", "output": "..."}` as docs suggest.

### CodeExecutionLanguage (content)

Specifies the programming language for code execution.

```json
{
  "type": "code_execution_call",
  "id": "exec_123",
  "language": "PYTHON",
  "code": "print('Hello')"
}
```

| Rust Enum | Wire Value | Notes |
|-----------|------------|-------|
| `CodeExecutionLanguage::Python` | `"PYTHON"` | Currently only supported language |
| `CodeExecutionLanguage::Unknown { ... }` | `"*"` | Future languages preserved |

Helper methods: `is_unknown()`, `unknown_language_type()`, `unknown_data()`

**Verified**: 2026-01-10 - Only Python supported; wire format is SCREAMING_SNAKE_CASE.

### UrlRetrievalStatus (response metadata)

Status of URL retrieval attempts in `url_context_metadata`.

```json
{
  "urlContextMetadata": {
    "urlMetadata": [
      {
        "retrievedUrl": "https://example.com",
        "urlRetrievalStatus": "URL_RETRIEVAL_STATUS_SUCCESS"
      },
      {
        "retrievedUrl": "https://blocked.com",
        "urlRetrievalStatus": "URL_RETRIEVAL_STATUS_UNSAFE"
      }
    ]
  }
}
```

| Rust Enum | Wire Value | Notes |
|-----------|------------|-------|
| `UrlRetrievalStatus::Unspecified` | `"URL_RETRIEVAL_STATUS_UNSPECIFIED"` | Default/missing |
| `UrlRetrievalStatus::Success` | `"URL_RETRIEVAL_STATUS_SUCCESS"` | URL fetched OK |
| `UrlRetrievalStatus::Unsafe` | `"URL_RETRIEVAL_STATUS_UNSAFE"` | Blocked by safety filter |
| `UrlRetrievalStatus::Error` | `"URL_RETRIEVAL_STATUS_ERROR"` | Fetch failed |
| `UrlRetrievalStatus::Unknown { ... }` | `"URL_RETRIEVAL_STATUS_*"` | Future values preserved |

Helper methods: `is_success()`, `is_error()`, `is_unknown()`, `unknown_status_type()`, `unknown_data()`

**Verified**: 2026-01-10 - Wire format uses fully-qualified `URL_RETRIEVAL_STATUS_` prefix.

## Testing New Enums

When adding new enums, always test the actual wire format with `curl`:

```bash
# Test what the API actually accepts
curl -s "https://generativelanguage.googleapis.com/v1beta/interactions?key=$GEMINI_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model": "gemini-3-flash-preview", "input": "test", ...}'
```

Common patterns to try:
1. lowercase: `"auto"`
2. SCREAMING_CASE: `"AUTO"`
3. Fully-qualified: `"ENUM_NAME_VALUE"` (e.g., `"THINKING_SUMMARIES_AUTO"`)

## Evergreen Pattern

All enums implement the Evergreen pattern with an `Unknown` variant that preserves unrecognized values:

```rust,ignore
#[non_exhaustive]
pub enum ThinkingSummaries {
    Auto,
    None,
    Unknown {
        summaries_type: String,
        data: serde_json::Value,
    },
}
```

This ensures forward compatibility when Google adds new enum values.

### Structs and `#[non_exhaustive]`

Response structs (e.g., `AutoFunctionResult`, `InteractionResponse`) also use `#[non_exhaustive]` so we can add fields without breaking user code. This has a trade-off:

**Users cannot construct these types directly** (no struct literal syntax outside the crate). This is intentional:
- Response types represent API responses, not user-constructed data
- Mocking them in unit tests would give false confidence
- We can add fields (like `executions` was added to `AutoFunctionResult`) without breaking changes

**For testing**, users should:
1. Use integration tests with real API calls (recommended)
2. Mock at the HTTP layer, not the response type layer
3. Test their own logic separately from API response handling

If a `test-support` feature for constructing mock instances becomes commonly requested, we'll consider adding it.
