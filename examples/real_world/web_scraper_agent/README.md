# Web Research Agent Example

An automated web research agent using Google Search grounding for real-time information retrieval.

## Overview

This example demonstrates a research agent that:

1. **Topic Research**: Synthesizes information into structured reports
2. **Competitor Analysis**: Gathers competitive intelligence
3. **Fact Checking**: Verifies claims against web sources
4. **Streaming Research**: Real-time response streaming with sources

## Features

### Google Search Grounding

Uses Gemini's built-in search capability:

```rust
client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_text(query)
    .with_google_search()  // Enable real-time web search
    .create()
    .await?
```

### Source Attribution

Access grounding steps for transparency:

```rust
for source in response.google_search_results() {
    println!("{} - {}", source.title, source.url);
}
```

### Structured Output

Combine search with JSON schema for parseable results:

```rust
.with_google_search()
.with_response_format(schema)
```

## Running

```bash
export GEMINI_API_KEY=your_api_key
cargo run --example web_scraper_agent
```

**Note**: Google Search grounding may not be available in all regions.

## Sample Output

```
📊 TOPIC RESEARCH

Researching: Latest developments in Rust programming language 2024

🔍 Search queries used:
   • Rust programming language 2024 updates
   • Rust new features 2024
📚 Sources retrieved: 8
   • Rust Blog [blog.rust-lang.org]
   • The New Stack [thenewstack.io]

📝 Research Report
==================
Topic: Rust Programming Language 2024

Summary:
Rust continues to gain momentum in 2024 with major releases...

Key Findings:
  1. [HIGH] Rust 1.75 introduced async trait stabilization
     Source: Official Rust Blog
  2. [HIGH] Linux kernel now includes Rust support
     Source: Linux kernel mailing list
```

## Research Capabilities

### Topic Research

```rust
let report = agent.research_topic("AI regulations 2024").await?;
println!("Summary: {}", report.summary);
for finding in report.key_findings {
    println!("[{}] {}", finding.confidence, finding.finding);
}
```

### Competitor Analysis

```rust
let analysis = agent.analyze_competitors("Tesla").await?;
for competitor in analysis.competitors {
    println!("{}: {}", competitor.name, competitor.strengths.join(", "));
}
```

### Fact Checking

```rust
let result = agent.fact_check("The Eiffel Tower is 330m tall").await?;
println!("Verdict: {}", result);
```

### Streaming Research

```rust
agent.stream_research("Latest tech news").await?;
// Output streams in real-time with final sources
```

## Grounding Metadata

Access detailed source information:

```rust
struct GroundingMetadata {
    web_search_queries: Vec<String>,  // Queries performed
    grounding_chunks: Vec<GroundingChunk>,  // Sources retrieved
}

struct GroundingChunk {
    web: WebSource {
        uri: String,     // Full URL
        title: String,   // Page title
        domain: String,  // Site domain
    }
}
```

## Production Enhancements

### Rate Limiting

```rust
struct RateLimitedAgent {
    agent: WebResearchAgent,
    limiter: RateLimiter,  // Prevent API abuse
}
```

### Result Caching

```rust
struct CachedResearch {
    cache: HashMap<String, (ResearchReport, Instant)>,
    ttl: Duration,  // Cache validity period
}
```

### Source Quality Scoring

```rust
fn score_source(domain: &str) -> f32 {
    match domain {
        "reuters.com" | "apnews.com" => 1.0,
        "wikipedia.org" => 0.8,
        _ => 0.5,
    }
}
```

### Export Formats

```rust
impl ResearchReport {
    fn to_markdown(&self) -> String { ... }
    fn to_pdf(&self) -> Vec<u8> { ... }
    fn to_json(&self) -> String { ... }
}
```

## Error Handling

The example handles common scenarios:

- Google Search unavailable in region
- Rate limiting from API
- Network timeouts
- Empty search results

## Best Practices

1. **Verify Critical Claims**: Cross-reference important facts
2. **Check Source Quality**: Prioritize authoritative sources
3. **Monitor Freshness**: Note when information was last updated
4. **Handle Ambiguity**: Be clear when sources conflict
5. **Respect Rate Limits**: Implement backoff for API limits

## Limitations

- Google Search grounding may have regional restrictions
- Real-time data depends on search index freshness
- Complex queries may require multiple research iterations
- Some domains may not be indexed or accessible
