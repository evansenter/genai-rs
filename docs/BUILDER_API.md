# InteractionBuilder API Guide

This guide covers the `InteractionBuilder` fluent API, including method naming conventions, interactions, and common patterns.

## Table of Contents

- [Overview](#overview)
- [Method Naming Conventions](#method-naming-conventions)
- [Input Methods](#input-methods)
- [Method Interactions](#method-interactions)
- [Validation Errors](#validation-errors)
- [Storage Constraints](#storage-constraints)
- [Best Practices](#best-practices)
- [Retrieving Stored Interactions](#retrieving-stored-interactions)

## Overview

The `InteractionBuilder` provides a fluent interface for constructing requests to the Gemini API. Methods can be chained in any order, and the request is validated and built when you call `create()`, `create_stream()`, or `build()`.

```rust,ignore
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_system_instruction("You are helpful")
    .with_text("Hello!")
    .create()
    .await?;
```

## Method Naming Conventions

Methods follow a consistent naming pattern based on their behavior:

| Prefix | Behavior | Example |
|--------|----------|---------|
| `with_*` | **Configures** a setting (replaces if called twice) | `with_model()`, `with_text()`, `with_content()` |
| `add_*` | **Accumulates** items to a collection | `add_function()`, `add_tool()` |

### Complete Method Reference

| Method | Prefix | Behavior | Notes |
|--------|--------|----------|-------|
| **Configuration** |
| `with_model()` | with | replaces | Mutually exclusive with `with_agent()` |
| `with_agent()` | with | replaces | Mutually exclusive with `with_model()` |
| `with_agent_config()` | with | replaces | Requires `with_agent()` |
| `with_system_instruction()` | with | replaces | Plain string; not inherited across turns |
| `with_timeout()` | with | replaces | |
| `with_service_tier(ServiceTier)` | with | replaces | `Flex`/`Standard`/`Priority` (wire: lowercase `"flex"`/`"standard"`/`"priority"`) |
| `with_cached_content(impl Into<String>)` | with | replaces | References an explicit context cache (e.g. `"cachedContents/xyz"`) |
| **Input** |
| `with_text()` | with | replaces | Composes with `with_history()` |
| `with_history(Vec<Step>)` | with | replaces | Composes with `with_text()` |
| `with_content()` | with | replaces | For multimodal; incompatible with history |
| **Tools** |
| `add_function()` | add | accumulates | Single function declaration |
| `add_functions()` | add | accumulates | Multiple function declarations |
| `with_tool_service()` | with | replaces | Dependency-injected tools |
| **Server-Side Tools (enable capabilities)** |
| `add_tool(impl Into<Tool>)` | add | accumulates | Unified tool entry point (see [Tool Configuration Structs](#tool-configuration-structs)) |
| `with_google_search()` | with | accumulates | Enables Google Search |
| `with_google_maps()` | with | accumulates | Enables Google Maps |
| `with_code_execution()` | with | accumulates | Enables code execution |
| `with_url_context()` | with | accumulates | Enables URL fetching |
| ~~`with_computer_use()`~~ | — | — | **Removed** — use `add_tool(ComputerUseConfig::new())` |
| ~~`with_computer_use_excluding()`~~ | — | — | **Removed** — use `add_tool(ComputerUseConfig::new().excluding(...))` |
| ~~`add_mcp_server()`~~ | — | — | **Removed** — use `add_tool(McpServerConfig::new(name, url))` |
| ~~`with_file_search()`~~ | — | — | **Removed** — use `add_tool(FileSearchConfig::new(stores))` |
| ~~`with_file_search_config()`~~ | — | — | **Removed** — use `add_tool(FileSearchConfig::new(stores))` |
| **Generation Config** |
| `with_function_calling_mode()` | with | replaces | Auto/Any/None/Validated (wire: lowercase `"auto"`/`"any"`/`"none"`/`"validated"`); sets `tool_choice` to the plain-mode form |
| `with_tool_choice(ToolChoice)` | with | replaces | Sets the full `tool_choice` union directly; escape hatch for custom shapes |
| `with_allowed_tools(Vec<String>)` | with | replaces | Restricts model to named tools; sets `tool_choice` to the `AllowedTools` restriction object (preserves a previously set mode) |
| `with_image_config(ImageConfig)` | with | replaces | Image generation aspect ratio and size |
| `with_thinking_level()` | with | replaces | Chain-of-thought reasoning level |
| `with_seed()` | with | replaces | Deterministic output |
| `with_stop_sequences()` | with | replaces | Halt generation on sequences |
| `with_presence_penalty(f32)` | with | replaces | Penalizes tokens already present; range [-2.0, 2.0] |
| `with_frequency_penalty(f32)` | with | replaces | Penalizes tokens by frequency; range [-2.0, 2.0] |
| **Response Format** |
| `with_response_format(serde_json::Value)` | with | replaces | JSON schema for structured output |
| `with_response_mime_type()` | with | replaces | **Deprecated** — the API deprecated `response_mime_type`; use `with_response_format()` instead |

Note: `GenerationConfig` no longer has a `top_k` field (removed in API revision 2026-05-20), so there is no `with_top_k()` builder method. `FileSearchConfig::with_top_k()` (a file-search retrieval setting) is unrelated and still exists.

### Tool Configuration Structs

For tools with optional configuration, use a config struct with `add_tool()`:

| Config Struct | Required Fields | Optional Methods |
|---|---|---|
| `GoogleSearchConfig` | (none) | `.with_search_types(Vec<SearchType>)` |
| `GoogleMapsConfig` | (none) | `.with_widget()`, `.with_location(latitude, longitude)` |
| `McpServerConfig` | `name`, `url` | `.with_allowed_tools(Vec<String>)`, `.with_allowed_tools_config(Vec<AllowedTools>)`, `.with_headers(...)` |
| `ComputerUseConfig` | (none) | `.with_environment(env)`, `.excluding(Vec<String>)`, `.with_prompt_injection_detection(bool)`, `.disabling_safety_policies(Vec<String>)` |
| `FileSearchConfig` | `store_names` | `.with_top_k(i32)`, `.with_metadata_filter(String)` |

Notes:

- `ComputerUseConfig::with_environment()` accepts `"browser"` (default), `"mobile"`, or `"desktop"`.
- `McpServerConfig::with_allowed_tools(Vec<String>)` wraps the names into a single `AllowedTools` entry; use `.with_allowed_tools_config(Vec<AllowedTools>)` to supply pre-built `AllowedTools` objects (e.g. with a mode via `AllowedTools::new(tools).with_mode(mode)`).

**Convention**: Prefer struct variants with optional fields over unit variants when the API has configuration options. This avoids breaking changes when adding fields later.

## Input Methods

The builder has three ways to set the input content:

| Method | Purpose | Composes With |
|--------|---------|---------------|
| `with_text(str)` | Simple text message | `with_history()`, `with_content()` |
| `with_history(Vec<Step>)` | Conversation history | `with_text()` |
| `with_content(Vec<Content>)` | Multimodal content | `with_text()` |
| `conversation()...done()` | Fluent conversation builder | — |

### How Inputs Compose at Build Time

```text
content_input set?
├── Yes
│   └── history set?
│       ├── Yes → ERROR (incompatible)
│       └── No
│           └── current_message set?
│               ├── Yes → Content([Content::text(message), ...content_items])
│               └── No  → Content([...content_items])
└── No
    └── history set?
        ├── Yes
        │   └── current_message set?
        │       ├── Yes → Steps([...history, Step::user_text(message)])
        │       └── No  → Steps([...history])
        └── No
            └── current_message set?
                ├── Yes → Text(message)
                └── No  → ERROR ("Input is required")
```

### Multimodal Input with Content

For multimodal requests, use `with_content()` with `Content` constructors:

```rust,ignore
use genai_rs::{Client, Content};

let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_content(vec![
        Content::text("Describe this image"),
        Content::image_data(base64_data, "image/png"),
    ])
    .create()
    .await?;
```

For file-based content, use helper functions:

```rust,ignore
use genai_rs::{Client, Content, image_from_file};

let image_content = image_from_file("photo.jpg").await?;
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_content(vec![
        Content::text("What's in this image?"),
        image_content,
    ])
    .create()
    .await?;
```

## Method Interactions

### Order Independence

Most input methods are **order-independent** - calling them in different orders produces the same result:

```rust,ignore
// These are equivalent:
.with_history(h).with_text("question")
.with_text("question").with_history(h)
```

### Replacement vs Accumulation

```rust,ignore
// Replacement: second call wins
.with_text("first").with_text("second")  // → "second"

// Accumulation for tools
.add_function(func1).add_function(func2)  // → [func1, func2]
```

## Validation Errors

The builder validates configuration at `build()` time and returns clear errors:

### 1. Content Cannot Combine with History

Content input (via `with_content()`) is for single-turn multimodal messages. It cannot be combined with multi-turn history.

```rust,ignore
// ERROR: Cannot combine content with history
client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_history(conversation_history)
    .with_content(vec![Content::image_data(base64, "image/png")])
    .build()  // Returns Err!
```

**Workaround**: For multimodal multi-turn, build `Step` objects with content arrays:

```rust,ignore
use genai_rs::{Step, Content};

let multimodal_step = Step::user_input(vec![
    Content::text("What's in this image?"),
    Content::image_data(base64_data, "image/png"),
]);

let mut history = existing_history;
history.push(multimodal_step);

client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_history(history)
    .create()
    .await?;
```

### 2. Model vs Agent is Mutually Exclusive

You must specify exactly one of `with_model()` or `with_agent()`:

```rust,ignore
// ERROR: Both specified
client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_agent("deep-research-pro-preview-12-2025")
    .build()  // Returns Err!

// ERROR: Neither specified
client.interaction()
    .with_text("Hello")
    .build()  // Returns Err!
```

### 3. Agent Config Requires Agent

`with_agent_config()` is only valid when using `with_agent()`:

```rust,ignore
// ERROR: agent_config without agent
client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_agent_config(DeepResearchConfig::new())
    .with_text("Research AI trends")
    .build()  // Returns Err!
```

## Storage Constraints

Certain combinations of builder methods are invalid because they conflict with storage requirements. The builder validates these constraints at `build()` time:

### Invalid Combinations

| If You Call | Cannot Also Call | Reason |
|-------------|------------------|--------|
| `with_store_disabled()` | `with_previous_interaction()` | Chaining requires storage |
| `with_store_disabled()` | `with_background(true)` | Background requires storage |
| `with_store_disabled()` | `create_with_auto_functions()` | Auto-functions require storage |

```rust,ignore
// Runtime error from build(): "Chained interactions require storage..."
client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Hello")
    .with_previous_interaction("id-123")
    .with_store_disabled()
    .build()  // Returns Err!
```

### Conditional Chaining

Since validation happens at runtime, you can conditionally chain methods:

```rust,ignore
// Your application tracks conversation state
let previous_interaction_id: Option<String> = session.last_interaction_id();
let should_disable_storage: bool = config.privacy_mode;

let mut builder = client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Hello");

// Conditionally add previous interaction
if let Some(prev_id) = previous_interaction_id {
    builder = builder.with_previous_interaction(prev_id);
}

// Conditionally disable storage (only valid if not chaining)
if previous_interaction_id.is_none() && should_disable_storage {
    builder = builder.with_store_disabled();
}

let response = builder.create().await?;
```

## Best Practices

### 1. Use Specific Input Methods

Prefer the specific method for your use case:

```rust,ignore
// Good: Clear intent
.with_text("Hello")  // Simple text
.with_history(steps)  // Multi-turn (Vec<Step>)
.with_content(vec![Content::text("Question"), Content::image_data(...)]) // Multimodal

// Avoid: Generic method is less clear
.with_input(InteractionInput::Text("Hello".to_string()))
```

### 2. Chain Related Configuration

Group related builder calls together:

```rust,ignore
client.interaction()
    // Target
    .with_model("gemini-3-flash-preview")
    // Context
    .with_system_instruction("You are helpful")
    .with_history(history)
    // Input
    .with_text("Current question")
    // Tools
    .add_function(get_weather.declaration())
    // Execute
    .create()
    .await?;
```

### 3. Handle Errors at Build Time

Call `build()` explicitly when you want to validate without executing:

```rust,ignore
let request = client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Hello")
    .build()?;  // Validate configuration

// Later: execute
let response = client.execute(request).await?;
```

### 4. Use ConversationBuilder for Inline Conversations

For test fixtures or inline conversation construction. `.user()` and `.model()` produce `user_input`/`model_output` `Step`s under the hood:

```rust,ignore
let response = client.interaction()
    .with_model("gemini-3-flash-preview")
    .conversation()
        .user("What is 2+2?")
        .model("4")
        .user("Times 3?")
        .done()
    .create()
    .await?;
```

## Retrieving Stored Interactions

When storage is enabled, interactions can be fetched back by ID via the `Client`:

| Method | Behavior |
|--------|----------|
| `client.get_interaction(id)` | Fetches the interaction; response `input` field is `None` |
| `client.get_interaction_with_input(id)` | Fetches with `include_input=true` so the response's `input: Option<InteractionInput>` is populated |

## Related Documentation

- [Conversation Patterns](CONVERSATION_PATTERNS.md) - Multi-turn conversation strategies
- [Multimodal](MULTIMODAL.md) - Working with images, audio, video
- [Function Calling](FUNCTION_CALLING.md) - Tool integration
- [Multi-Turn Function Calling](MULTI_TURN_FUNCTION_CALLING.md) - Function calling in conversations
