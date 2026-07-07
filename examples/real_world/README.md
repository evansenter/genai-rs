# Real-World Example Applications

This directory contains comprehensive example applications demonstrating practical use cases for the `genai-rs` library. Each example showcases production patterns, error handling, and best practices.

## Examples Overview

| Example | Description | Key Features |
|---------|-------------|--------------|
| [RAG System](./rag_system/) | Document Q&A with retrieval | Context injection, source attribution |
| [Multi-Turn Agent (Auto)](./multi_turn_agent_auto/) | Customer support bot | `create_with_auto_functions()`, stateful conversations |
| [Multi-Turn Agent (Manual)](./multi_turn_agent_manual/) | Customer support bot | Manual function loop, full control |
| [Multi-Turn Agent (Stateless)](./multi_turn_agent_manual_stateless/) | Customer support bot | `store: false`, manual history |
| [Code Assistant](./code_assistant/) | Code analysis tool | Structured output, multi-task analysis |
| [Data Analysis](./data_analysis/) | CSV analysis with NL queries | Function calling, data operations |
| [Web Scraper Agent](./web_scraper_agent/) | Web research assistant | Google Search grounding, streaming |
| [Testing Assistant](./testing_assistant/) | Test generation from code | Coverage analysis, property tests |
| [Repo Auditor](./repo_auditor/) | Agentic security audit (Antigravity harness) | Subagents, policies + hooks, structured report (`--features antigravity`) |

## Running Examples

All examples require the `GEMINI_API_KEY` environment variable:

```bash
export GEMINI_API_KEY=your_api_key
```

Run any example with:

```bash
cargo run --example rag_system
cargo run --example multi_turn_agent_auto
cargo run --example multi_turn_agent_manual
cargo run --example multi_turn_agent_manual_stateless
cargo run --example code_assistant
cargo run --example data_analysis
cargo run --example web_scraper_agent
cargo run --example testing_assistant
```

The Antigravity-based example additionally needs the `localharness` binary
(`pip install google-antigravity==0.1.5`) and the crate feature:

```bash
cargo run --example repo_auditor --features antigravity
```

## Features Demonstrated

### Core API Features

- **Client Building**: `Client::builder(api_key).build()`
- **Interaction Building**: Fluent builder pattern with `with_*` methods
- **Streaming**: Real-time response handling with `create_stream()`
- **Stateful Conversations**: Using `with_previous_interaction()` and `with_store(true)`

### Advanced Features

| Feature | Examples |
|---------|----------|
| Function Calling | Multi-Turn Agent (all variants), Data Analysis |
| Structured Output | Code Assistant, Testing Assistant |
| Google Search Grounding | Web Scraper Agent |
| Auto Function Execution | Multi-Turn Agent (Auto), Data Analysis |
| System Instructions | All examples |

### Production Patterns

Each example demonstrates:

- **Error Handling**: Graceful degradation and informative error messages
- **Type Safety**: Structured outputs parsed into Rust structs
- **Modularity**: Separation of concerns with clear interfaces
- **Documentation**: Comprehensive code comments and READMEs

## Example Summaries

### RAG System

Retrieval-Augmented Generation for document Q&A:

```rust
// Retrieve relevant documents
let retrieved = store.retrieve(query, 2);

// Build augmented prompt
let context = build_context(&retrieved);
let prompt = build_rag_prompt(query, &context);

// Generate response with context
let response = client.interaction()
    .with_text(&prompt)
    .create().await?;
```

### Multi-Turn Agent (Auto Functions)

Stateful customer support bot with automatic function execution:

```rust
// Auto handles function calling loop
let result = client.interaction()
    .with_previous_interaction(&last_id)
    .add_functions(tools)
    .create_with_auto_functions().await?;
```

### Multi-Turn Agent (Manual Functions)

Same bot with manual control over function execution:

```rust
// Manual function calling loop
let response = client.interaction()
    .with_previous_interaction(&last_id)
    .set_content(function_results)
    .create().await?;
```

### Multi-Turn Agent (Stateless)

Stateless conversations with client-side history management:

```rust
// No server state - maintain history locally
let response = client.interaction()
    .with_input(InteractionInput::Content(history.clone()))
    .with_store_disabled()  // No previous_interaction_id
    .create().await?;
```

### Code Assistant

Structured code analysis:

```rust
// Get structured analysis output
let analysis: CodeAnalysis = client.interaction()
    .with_response_format(schema)
    .create().await?
    .as_text()
    .and_then(|t| serde_json::from_str(t).ok())
    .unwrap();
```

### Data Analysis

Natural language data queries:

```rust
// NL question -> function calls -> answer
let result = client.interaction()
    .with_text("What are total sales by region?")
    .add_functions(data_tools)
    .create_with_auto_functions().await?;
```

### Web Scraper Agent

Real-time web research:

```rust
// Grounded search with source attribution
let response = client.interaction()
    .with_google_search()
    .create().await?;

// Access sources (GoogleSearchResult steps)
for source in response.google_search_results() {
    println!("{}: {}", source.title, source.url);
}
```

### Testing Assistant

AI-powered test generation:

```rust
// Generate structured test suite
let suite: TestSuite = assistant
    .generate_test_suite(code, "rust").await?;

// Analyze coverage gaps
let analysis = assistant
    .analyze_coverage(code, tests, "rust").await?;
```

## Architecture Patterns

### Wrapper Structs

Each example wraps the client for domain-specific operations:

```rust
struct SupportSession {
    client: Client,
    last_interaction_id: Option<String>,
}

impl SupportSession {
    async fn process_message(&mut self, msg: &str) -> Result<String, Error> {
        // Domain-specific logic
    }
}
```

### Tool Registration

Function calling with the `#[tool]` macro:

```rust
#[tool(column(description = "Column name to analyze"))]
fn get_stats(column: String) -> String {
    // Implementation
}

// Use in interaction
client.interaction()
    .add_functions(vec![GetStatsCallable.declaration()])
```

### Structured Output

JSON schema for consistent parsing:

```rust
let schema = json!({
    "type": "object",
    "properties": {
        "summary": {"type": "string"},
        "items": {"type": "array", "items": {"type": "string"}}
    }
});

client.interaction()
    .with_response_format(schema)
```

## Production Considerations

Each example includes notes on:

- **Scaling**: Database connections, caching, rate limiting
- **Security**: Authentication, input validation
- **Monitoring**: Logging, metrics, error tracking
- **Testing**: Unit tests, integration tests

## Contributing

When adding new examples:

1. Create a directory under `examples/real_world/`
2. Add `main.rs` with comprehensive comments
3. Include a `README.md` with usage instructions
4. Update this README and `Cargo.toml`
5. Ensure the example compiles and runs successfully
