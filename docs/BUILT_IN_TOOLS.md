# Built-in Tools Guide


| Tool | Purpose | Who executes |
|------|---------|--------------|
| Google Search | Real-time web data | API |
| Code Execution | Run Python code | API (sandbox) |
| URL Context | Fetch and analyze URLs | API |
| File Search | Semantic search over file search stores | API |
| Google Maps | Place and location data | API |
| MCP Servers | Calls to a remote MCP server | API |
| Computer Use | Browser/desktop automation | **Your code**: the model emits `function_call` steps and your loop performs them |
| Retrieval | Vertex AI Search, RAG Store, Exa.ai, Parallel.ai | **Vertex-only**: rejected by the Gemini API |

Apart from Computer Use, these run on Google's side, unlike
[function calling](FUNCTION_CALLING.md), where your code executes the
functions.

Enable a tool with its shortcut (`with_google_search()`, `with_code_execution()`,
`with_url_context()`, `with_google_maps()`) or, for tools with options, with
`add_tool(<Config>)`. Calling a shortcut again replaces that tool's earlier
entry.

**Where tool activity appears.** Tool activity arrives as step variants in
`response.steps`: `Step::GoogleSearchCall` / `GoogleSearchResult`,
`CodeExecutionCall` / `CodeExecutionResult`, `UrlContextCall` /
`UrlContextResult`, `GoogleMapsCall` / `GoogleMapsResult`, and so on. MCP is
the exception (see [MCP Servers](#mcp-servers)). The response helpers below
iterate those steps for you. Grounding shows up as inline `Annotation`
citations, and `usage.grounding_tool_count` counts it per tool.

## Google Search

Ground responses in real-time web data.

### Basic Usage

```rust,ignore
let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
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
    .with_model(genai_rs::DEFAULT_MODEL)
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

**Example**: `cargo run --example google_search` (also streams).

## Code Execution

Runs Python in a sandbox.

### Basic Usage

```rust,ignore
let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
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

`response.successful_code_output()` returns the first non-error result. Python is the only language (`CodeExecutionLanguage::Python`).

**Example**: `cargo run --example code_execution`

## URL Context

Fetches the URLs in the prompt.

```rust,ignore
let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
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

Each `UrlContextResultItem` has a `status` string. The known values are
`"success"`, `"error"`, `"paywall"` and `"unsafe"`, and `item.is_success()`
checks for the first. A page the tool could not fetch shows up as a
non-success `status`, not as an error.

**Example**: `cargo run --example url_context`

## Computer Use

Browser, mobile or desktop automation, configured with `ComputerUseConfig`.
The model emits its actions as `function_call` steps, and **your** loop
performs them against a browser or device and returns the results.

```rust,ignore
use genai_rs::ComputerUseConfig;

let config = ComputerUseConfig::new()
    // Operating environment: "browser" (default), "mobile", or "desktop"
    .with_environment("browser")
    // Disable specific predefined functions for safety
    .with_excluded_predefined_functions(vec!["submit_form".to_string(), "download_file".to_string()])
    // Detect prompt injection attempts in page content
    .with_prompt_injection_detection(true)
    // Opt out of specific safety policies (use with care)
    .with_disabled_safety_policies(vec!["financial_transactions".to_string()]);

let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("Search for Rust tutorials")
    .add_tool(config)
    .create()
    .await?;
```

`ComputerUseConfig::new()` defaults to the `"browser"` environment. Computer
use is allowlisted, and most API keys can't enable it. Exclude actions a task
doesn't need, and turn on prompt-injection detection when browsing untrusted
pages.

**Example**: `cargo run --example computer_use`

## File Search

Semantic search over *file search stores* (resource names like
`fileSearchStores/my-store-123`), not over Files API uploads. Create a store,
upload documents into it, and wait for indexing before searching:

```rust,ignore
use genai_rs::CreateFileSearchStoreRequest;

let store = client
    .create_file_search_store(&CreateFileSearchStoreRequest::new().with_display_name("my-docs"))
    .await?;

let document = client
    .upload_to_file_search_store(&store.name, "handbook.pdf", Some("handbook"))
    .await?;

// Required. A document still in STATE_PENDING is not an error — file search
// just returns no matches for it, so an upload-then-query sequence silently
// finds nothing without this.
client.wait_for_document_active(&document.name, None, None).await?;
```

A store created in Google AI Studio works the same way; pass its resource
name. `examples/file_search.rs` shows the full lifecycle, including cleanup.

```rust,ignore
use genai_rs::FileSearchConfig;

let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("What does the report say about Q4 revenue?")
    .add_tool(FileSearchConfig::new(vec![store.name.clone()]))
    .create()
    .await?;

// The answer itself is the reliable output.
println!("{}", response.as_text().unwrap_or_default());
```

`FileSearchConfig` also takes `.with_top_k(n)` (maximum retrieved chunks) and
`.with_metadata_filter("category:technical")`.

> **`file_search_results()` returns nothing on this API.** The live API never
> emits the step it reads. This was verified against a store whose indexed,
> `STATE_ACTIVE` documents demonstrably grounded the answer. Retrieved chunks
> are folded into the response text, so an empty result set is expected
> (#429).

## Retrieval

Ground responses in external retrieval backends: Vertex AI Search engines and
datastores, Vertex RAG Store corpora, or third-party search APIs (Exa.ai,
Parallel.ai). Configure via `RetrievalConfig`, which keeps the enabled
`retrieval_types` in sync with the per-backend configs.

> **Vertex-only.** The Gemini API rejects the `retrieval` tool: live probing
> (2026-07) returned "allowed on the Gemini Enterprise Agent Platform", and
> the Gemini tool types are `google_maps`, `mcp_server`, `function`,
> `google_search`, `file_search`, `computer_use`, `code_execution` and
> `url_context`. The types below are modeled for spec parity and compile, but
> a request carrying them fails with a 400 on `generativelanguage.googleapis.com`.
> On the Gemini API, use [File Search](#file-search) for your own documents
> and [Google Search](#google-search) for the web.

### Vertex AI Search

```rust,no_run
use genai_rs::{Client, RetrievalConfig, VertexAiSearchConfig};

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
# let client = Client::new("api-key".to_string());
let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
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

The other backends are configured the same way, through
`RetrievalConfig::with_*`:

| Backend | Config | Notes |
|---------|--------|-------|
| Vertex RAG Store | `RagStoreConfig::new(vec![RagResource::new(corpus)])`, `.with_rag_retrieval_config(RagRetrievalConfig::new().with_top_k(..).with_hybrid_search_alpha(..).with_filter(..).with_ranking(..))` | Hybrid search, filters, ranking |
| Exa.ai | `ExaAiSearchConfig::new(api_key)`, `.with_custom_config(json)` | The `api_key` is sent on the wire in the tool config |
| Parallel.ai | `ParallelAiSearchConfig::new()` (with its `api_key`) | As above |

Each `with_*` backend method also enables the matching `RetrievalType`.
Treat request logs as sensitive when a third-party key is configured.

## Google Maps

Grounds responses in place data.

```rust,ignore
let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
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

`add_tool(GoogleMapsConfig::new().with_location(lat, lng).with_widget())`
biases results toward a location and requests a widget context token. Maps
grounding can also attach `Annotation::PlaceCitation` annotations (place id,
name, URL, review snippets) to the text.

**Example**: `cargo run --example google_maps`

## MCP Servers

The API calls tools on a remote MCP (Model Context Protocol) server for the
model.

```rust,ignore
use genai_rs::McpServerConfig;
use std::collections::HashMap;

let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
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

For a restriction with a mode, pass `AllowedTools` entries to
`with_allowed_tools_config()`:
`AllowedTools::new(vec!["read_file".into()]).with_mode(FunctionCallingMode::Auto)`.

**MCP calls arrive as generic `Step::ToolCall { id, signature }` steps**,
not as `Step::McpServerToolCall` (verified live 2026-08-16 against a real MCP
server). So a match on `McpServerToolCall` never fires, and
`step_summary().mcp_server_tool_call_count` reads 0 even on a successful call.
Count calls with `step_summary().tool_call_count` or `response.tool_calls()`.
Which server or tool ran can't be recovered from the response. The
`McpServerToolCall` / `McpServerToolResult` variants remain modeled from the
spec (#459).

## Combining Tools

Tools can be combined in one request, with two exceptions (verified live
2026-08-16 against `gemini-3.7-flash`):

| Combination | Result |
|-------------|--------|
| `file_search` + `google_search` | **400**: "`'google_search'` and `'file_search'` cannot be combined in the same request. Please choose one to continue." |
| `file_search` + `url_context` | **400**, the same message naming `url_context` |
| `file_search` + `code_execution` | Accepted |

To ground on both internal documents and the web, run two interactions and
combine the results.

Built-in tools also combine with your own functions. With
`create_with_auto_functions()`, add your `#[tool]` declarations explicitly,
because setting any tool switches off registry discovery (see
[Function Calling](FUNCTION_CALLING.md#automatic-execution-create_with_auto_functions)):

```rust,ignore
// get_user_prefs is a #[tool] function
let result = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("Personalize search results based on my preferences")
    .with_google_search()
    .add_function(get_user_prefs_declaration())
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
| `has_tool_calls()` | MCP / generic | `bool` |
| `tool_calls()` | MCP / generic | `Vec<ToolCallInfo>` |
| `has_annotations()` | Any grounded | `bool` |
| `all_annotations()` | Any grounded | `Iterator<Item = &Annotation>` |
| `step_summary()` | All | `StepSummary` (per-step-type counts) |

## Examples

| Example | Tools Demonstrated |
|---------|-------------------|
| `google_search` | Google Search with streaming |
| `code_execution` | Python execution and result handling |
| `url_context` | URL fetching |
| `computer_use` | Browser automation |
| `file_search` | Document search over file search stores |
| `google_maps` | Place grounding |
