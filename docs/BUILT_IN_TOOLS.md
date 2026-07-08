# Built-in Tools Guide

Gemini provides several server-side tools that execute automatically without requiring client-side code. This guide covers all built-in tools and when to use each.

## Table of Contents

- [Overview](#overview)
- [Google Search](#google-search)
- [Code Execution](#code-execution)
- [URL Context](#url-context)
- [Computer Use](#computer-use)
- [File Search](#file-search)
- [Retrieval](#retrieval)
- [Google Maps](#google-maps)
- [MCP Servers](#mcp-servers)
- [Combining Tools](#combining-tools)

## Overview

| Tool | Purpose | Execution |
|------|---------|-----------|
| Google Search | Real-time web data | Server-side |
| Code Execution | Run Python code | Server-side sandbox |
| URL Context | Fetch and analyze URLs | Server-side |
| Computer Use | Browser automation | Server-side |
| File Search | Semantic document search | Server-side |
| Retrieval | External retrieval backends (Vertex AI Search, RAG, Exa.ai, Parallel.ai) | Server-side |
| Google Maps | Place and location data | Server-side |
| MCP Servers | Remote MCP tool calls | Server-side |

**Key distinction**: These are *server-side* tools executed by Google's infrastructure, unlike *client-side* function calling where your code executes the functions.

**Where tool activity appears**: Under API revision 2026-05-20, server-side tool activity is reported as dedicated step variants in `response.steps` (e.g., `Step::GoogleSearchCall`, `Step::GoogleSearchResult`, `Step::CodeExecutionCall`, `Step::UrlContextResult`, `Step::McpServerToolCall`, ...). The response helpers shown below (`google_search_results()`, `code_execution_calls()`, ...) iterate those steps for you. The old `grounding_metadata` and `url_context_metadata` response fields no longer exist — grounding information comes from the steps themselves plus inline `Annotation` citations, and `usage.grounding_tool_count` reports per-tool grounding counts.

## Google Search

Ground responses in real-time web data.

### Basic Usage

```rust,ignore
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("What are the latest Rust 2024 features?")
    .with_google_search()
    .create()
    .await?;

// Access the response text
println!("{}", response.as_text().unwrap());

// Check if grounded with search
if response.has_google_search_calls() {
    // Get search queries used
    for query in response.google_search_calls() {
        println!("Searched: {}", query);
    }

    // Get source URLs
    for result in response.google_search_results() {
        println!("Source: {} - {}", result.title, result.url);
    }
}
```

### Search Types

Restrict or extend the kinds of search performed with `GoogleSearchConfig`:

```rust,ignore
use genai_rs::{GoogleSearchConfig, SearchType};

let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Find enterprise deployment guides for Kubernetes")
    .add_tool(GoogleSearchConfig::new().with_search_types(vec![
        SearchType::WebSearch,
        SearchType::EnterpriseWebSearch,  // wire: "enterprise_web_search"
    ]))
    .create()
    .await?;
```

| Variant | Wire value |
|---------|-----------|
| `SearchType::WebSearch` | `web_search` |
| `SearchType::ImageSearch` | `image_search` |
| `SearchType::EnterpriseWebSearch` | `enterprise_web_search` |

### With Annotations (Citations)

Annotations are a discriminated union (`UrlCitation`, `FileCitation`, `PlaceCitation`, plus an `Unknown` fallback). Use the accessor methods rather than matching fields directly:

```rust,ignore
if response.has_annotations() {
    let text = response.all_text();
    for annotation in response.all_annotations() {
        if let Some(span) = annotation.extract_span(&text) {
            println!(
                "'{}' sourced from: {}",
                span,
                annotation.source().unwrap_or("<no source>")
            );
        }
    }
}
```

### Streaming

```rust,ignore
use futures_util::StreamExt;

let mut stream = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Latest AI news?")
    .with_google_search()
    .create_stream();

while let Some(Ok(event)) = stream.next().await {
    // delta_text() extracts text from StepDelta chunks
    if let Some(text) = event.chunk.delta_text() {
        print!("{}", text);
    }
}
```

**When to use**: Current events, real-time data, fact verification, research tasks.

**Example**: `cargo run --example google_search`

## Code Execution

Execute Python code in a secure sandbox.

### Basic Usage

```rust,ignore
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Calculate the first 20 Fibonacci numbers")
    .with_code_execution()
    .create()
    .await?;

// Check for code execution
if response.has_code_execution_calls() {
    // Get the code that was executed
    for call in response.code_execution_calls() {
        println!("Code:\n{}", call.code);
        println!("Language: {}", call.language);  // prints "python"
    }

    // Get execution results
    for result in response.code_execution_results() {
        if result.is_error {
            println!("Error: {}", result.result);
        } else {
            println!("Output: {}", result.result);
        }
    }
}
```

Code execution appears in `response.steps` as `Step::CodeExecutionCall { id, language, code, .. }` followed by `Step::CodeExecutionResult { call_id, result, is_error, .. }`. The `CodeExecutionLanguage` wire format is lowercase (`"python"`).

### Convenience Methods

```rust,ignore
// Get successful output directly
if let Some(output) = response.successful_code_output() {
    println!("Result: {}", output);
}

// Check execution activity
if response.has_code_execution_results() {
    println!("Code was executed");
}
```

**When to use**: Mathematical calculations, data processing, algorithm implementation, generating visualizations.

**Limitations**:
- Python only
- Sandboxed environment (no network, limited filesystem)
- Execution timeout limits

**Example**: `cargo run --example code_execution`

## URL Context

Fetch and analyze web pages.

### Basic Usage

```rust,ignore
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Summarize this article: https://example.com/article")
    .with_url_context()
    .create()
    .await?;

// URLs the tool fetched
for url in response.url_context_call_urls() {
    println!("Fetched: {}", url);
}

// Per-URL fetch status
for result in response.url_context_results() {
    for item in result.items {
        println!("URL: {} - status: {}", item.url, item.status);
    }
}
```

### Multiple URLs

```rust,ignore
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Compare https://example.com/page1 and https://example.com/page2")
    .with_url_context()
    .create()
    .await?;
```

### Handling Retrieval Errors

Each `UrlContextResultItem` carries a `status` string. Known values under revision 2026-05-20 are `"success"`, `"error"`, `"paywall"`, and `"unsafe"`:

```rust,ignore
for result in response.url_context_results() {
    for item in result.items {
        if item.is_success() {
            println!("Retrieved: {}", item.url);
        } else {
            println!("Failed to retrieve {} ({})", item.url, item.status);
        }
    }
}
```

**When to use**: Summarizing articles, comparing pages, extracting structured data from websites.

**Limitations**:
- Some sites block automated access
- Large pages may be truncated
- Dynamic content may not render

## Computer Use

Browser automation for web interactions. Configure via `ComputerUseConfig` and `add_tool()`. Computer-use actions flow through `function_call` steps that your agent loop executes against a browser.

### Basic Usage

```rust,ignore
use genai_rs::ComputerUseConfig;

let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Go to example.com and describe what you see")
    .add_tool(ComputerUseConfig::new())  // defaults to the "browser" environment
    .create()
    .await?;
```

### Configuration

```rust,ignore
use genai_rs::ComputerUseConfig;

let config = ComputerUseConfig::new()
    // Operating environment: "browser" (default), "mobile", or "desktop"
    .with_environment("browser")
    // Disable specific predefined functions for safety
    .excluding(vec!["submit_form".to_string(), "download_file".to_string()])
    // Detect prompt injection attempts in page content
    .with_prompt_injection_detection(true)
    // Opt out of specific safety policies (use with care)
    .disabling_safety_policies(vec!["financial_transactions".to_string()]);

let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Search for Rust tutorials")
    .add_tool(config)
    .create()
    .await?;
```

**When to use**: Web scraping, form filling, interactive web tasks.

**Safety considerations**:
- Always review what actions are enabled
- Use `excluding()` for read-only tasks
- Enable prompt injection detection when browsing untrusted pages
- Be cautious with authentication flows

**Example**: `cargo run --example computer_use`

## File Search

Semantic search across documents in pre-configured file search stores.

### Setup: Create a Store First

File Search operates on *file search stores* (identifiers like `stores/my-store-123`), not on ad-hoc Files API uploads. Create a store and upload documents to it via Google AI Studio or the Stores API, then reference the store identifier in requests.

### Basic Search

```rust,ignore
use genai_rs::FileSearchConfig;

let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("What does the report say about Q4 revenue?")
    .add_tool(FileSearchConfig::new(vec!["stores/my-store-123".to_string()]))
    .create()
    .await?;

// Access search results
for result in response.file_search_results() {
    println!("Title: {}", result.title);
    println!("Text: {}", result.text);
    println!("Store: {}", result.store);
}
```

### With Configuration

```rust,ignore
use genai_rs::FileSearchConfig;

let config = FileSearchConfig::new(vec!["stores/my-store-123".to_string()])
    .with_top_k(10)                            // max retrieval chunks
    .with_metadata_filter("category:technical"); // filter by document metadata

let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Find all mentions of 'performance optimization'")
    .add_tool(config)
    .create()
    .await?;
```

**When to use**: Document Q&A, research across multiple files, finding specific information in large documents.

**Example**: `cargo run --example file_search`

## Retrieval

Ground responses in external retrieval backends: Vertex AI Search engines and
datastores, Vertex RAG Store corpora, or third-party search APIs (Exa.ai,
Parallel.ai). Configure via [`RetrievalConfig`], which keeps the enabled
`retrieval_types` in sync with the per-backend configs.

> **Note**: These backends require pre-provisioned resources (search engines,
> RAG corpora) or third-party API keys. Pending live verification against the
> 2026-05-20 revision.

### Vertex AI Search

```rust,no_run
use genai_rs::{Client, RetrievalConfig, VertexAiSearchConfig};

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
# let client = Client::new("api-key".to_string());
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("What does our handbook say about vacation policy?")
    .add_tool(RetrievalConfig::new().with_vertex_ai_search(
        VertexAiSearchConfig::new()
            .with_engine("projects/p/locations/global/engines/my-engine"),
    ))
    .create()
    .await?;
# Ok(())
# }
```

### RAG Store

```rust,no_run
use genai_rs::{
    Client, RagFilter, RagRanking, RagResource, RagRetrievalConfig, RagStoreConfig,
    RetrievalConfig,
};

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
# let client = Client::new("api-key".to_string());
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Summarize the design documents about caching")
    .add_tool(RetrievalConfig::new().with_rag_store(
        RagStoreConfig::new(vec![
            RagResource::new("projects/p/locations/us/ragCorpora/docs"),
        ])
        .with_rag_retrieval_config(
            RagRetrievalConfig::new()
                .with_top_k(8)
                .with_hybrid_search_alpha(0.5)
                .with_filter(RagFilter {
                    vector_distance_threshold: Some(0.7),
                    vector_similarity_threshold: None,
                    metadata_filter: Some("category = \"design\"".to_string()),
                })
                .with_ranking(RagRanking::rank_service().with_model_name("ranker-v2")),
        ),
    ))
    .create()
    .await?;
# Ok(())
# }
```

### Third-Party Search (Exa.ai / Parallel.ai)

```rust,no_run
use genai_rs::{Client, ExaAiSearchConfig, ParallelAiSearchConfig, RetrievalConfig};

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
# let client = Client::new("api-key".to_string());
let exa_key = std::env::var("EXA_API_KEY")?;
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Find recent papers on speculative decoding")
    .add_tool(RetrievalConfig::new().with_exa_ai_search(
        ExaAiSearchConfig::new(exa_key)
            .with_custom_config(serde_json::json!({"num_results": 5})),
    ))
    .create()
    .await?;
# Ok(())
# }
```

**Security**: Exa.ai / Parallel.ai `api_key` values are sent on the wire in
the tool config — load them from secrets management and treat request logs as
sensitive.

**When to use**: Enterprise search over provisioned Vertex resources, RAG
corpora with fine-grained retrieval control, or third-party web-search APIs.
For Google-hosted document stores prefer [File Search](#file-search); for
general web grounding prefer [Google Search](#google-search).

**Example**: `cargo run --example retrieval_grounding`

## Google Maps

Ground responses in place and location data.

### Basic Usage

```rust,ignore
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Find highly rated coffee shops near the Ferry Building in San Francisco")
    .with_google_maps()
    .create()
    .await?;

for result in response.google_maps_results() {
    for item in result.items {
        for place in item.places.iter().flatten() {
            println!(
                "{} - {}",
                place.name.as_deref().unwrap_or("<unnamed>"),
                place.formatted_address.as_deref().unwrap_or("<no address>")
            );
            if let Some(url) = &place.url {
                println!("  Maps URL: {}", url);
            }
        }
    }
}
```

### Location Biasing and Widgets

```rust,ignore
use genai_rs::GoogleMapsConfig;

let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("What are the best lunch spots nearby?")
    .add_tool(
        GoogleMapsConfig::new()
            .with_location(37.7955, -122.3937)  // bias results toward lat/lng
            .with_widget(),                      // request a widget context token
    )
    .create()
    .await?;
```

Maps grounding can also surface `Annotation::PlaceCitation` annotations (place id, name, URL, and review snippets) attached to the response text.

**Example**: `cargo run --example google_maps`

## MCP Servers

Let the API call tools on a remote MCP (Model Context Protocol) server on the model's behalf.

```rust,ignore
use genai_rs::McpServerConfig;
use std::collections::HashMap;

let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("List the files in the project root")
    .add_tool(
        McpServerConfig::new("filesystem", "https://mcp.example.com/fs")
            // Restrict which tools the model may call
            .with_allowed_tools(vec!["read_file".to_string(), "list_dir".to_string()])
            .with_headers(HashMap::from([(
                "Authorization".to_string(),
                "Bearer token".to_string(),
            )])),
    )
    .create()
    .await?;
```

For per-mode restrictions use `with_allowed_tools_config()` with explicit `AllowedTools` entries:

```rust,ignore
use genai_rs::{AllowedTools, FunctionCallingMode, McpServerConfig};

let config = McpServerConfig::new("filesystem", "https://mcp.example.com/fs")
    .with_allowed_tools_config(vec![
        AllowedTools::new(vec!["read_file".to_string()]).with_mode(FunctionCallingMode::Auto),
    ]);
```

MCP activity appears in `response.steps` as `Step::McpServerToolCall { name, server_name, arguments, .. }` and `Step::McpServerToolResult { .. }`.

## Combining Tools

Multiple built-in tools can be enabled simultaneously.

### Research Assistant

```rust,ignore
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Research the latest Rust async features and write example code")
    .with_google_search()      // Find current information
    .with_code_execution()     // Write and test code
    .create()
    .await?;
```

### Document Analysis with Web Context

```rust,ignore
use genai_rs::FileSearchConfig;

let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Compare our internal report with public benchmarks")
    .add_tool(FileSearchConfig::new(vec!["stores/reports".to_string()]))
    .with_url_context()
    .create()
    .await?;
```

### With Client-Side Functions

Built-in tools can also combine with your own functions:

```rust,ignore
use genai_rs_macros::tool;

#[tool(description = "Get current user's preferences")]
fn get_user_prefs() -> String {
    // Your implementation
    r#"{"theme": "dark", "language": "en"}"#.to_string()
}

let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Personalize search results based on my preferences")
    .with_google_search()
    .add_function(get_user_prefs.declaration())
    .create_with_auto_functions()
    .await?;
```

## Response Helpers Reference

| Method | Tool | Returns |
|--------|------|---------|
| `has_google_search_calls()` | Google Search | `bool` |
| `google_search_calls()` | Google Search | `Vec<&str>` (queries) |
| `has_google_search_results()` | Google Search | `bool` |
| `google_search_results()` | Google Search | `Vec<&GoogleSearchResultItem>` |
| `has_code_execution_calls()` | Code Execution | `bool` |
| `code_execution_calls()` | Code Execution | `Vec<CodeExecutionCallInfo>` |
| `has_code_execution_results()` | Code Execution | `bool` |
| `code_execution_results()` | Code Execution | `Vec<CodeExecutionResultInfo>` |
| `successful_code_output()` | Code Execution | `Option<&str>` |
| `has_url_context_calls()` | URL Context | `bool` |
| `url_context_call_urls()` | URL Context | `Vec<&str>` |
| `has_url_context_results()` | URL Context | `bool` |
| `url_context_results()` | URL Context | `Vec<UrlContextResultInfo>` |
| `has_file_search_results()` | File Search | `bool` |
| `file_search_results()` | File Search | `Vec<&FileSearchResultItem>` |
| `has_google_maps_results()` | Google Maps | `bool` |
| `google_maps_results()` | Google Maps | `Vec<GoogleMapsResultInfo>` |
| `has_annotations()` | Any grounded | `bool` |
| `all_annotations()` | Any grounded | `Iterator<Item = &Annotation>` |
| `step_summary()` | All | `StepSummary` (per-step-type counts) |

## Examples

| Example | Tools Demonstrated |
|---------|-------------------|
| `google_search` | Google Search with streaming |
| `code_execution` | Python execution and result handling |
| `computer_use` | Browser automation |
| `file_search` | Document search over file search stores |
| `google_maps` | Place grounding |
| `deep_research` | Multi-tool research agent |

Run examples with:
```bash
cargo run --example <name>
```
