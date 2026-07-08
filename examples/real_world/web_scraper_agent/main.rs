//! # Web Scraper Agent Example
//!
//! This example demonstrates an automated web research agent that:
//! - Uses Google Search grounding for real-time web data
//! - Synthesizes information from multiple sources
//! - Provides cited, verified responses
//! - Handles research workflows with structured output
//!
//! ## Production Patterns Demonstrated
//!
//! - Google Search grounding for real-time data
//! - Source attribution and verification
//! - Multi-step research workflows
//! - Structured output for reports
//!
//! ## Running
//!
//! ```bash
//! cargo run --example web_scraper_agent
//! ```
//!
//! ## Prerequisites
//!
//! Set the `GEMINI_API_KEY` environment variable with your API key.
//!
//! ## Note
//!
//! Google Search grounding may not be available in all regions or accounts.

use futures_util::StreamExt;
use genai_rs::{Client, StreamChunk};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;
use std::error::Error;
use std::io::{Write, stdout};

// ============================================================================
// Research Output Types
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
struct ResearchReport {
    topic: String,
    summary: String,
    key_findings: Vec<KeyFinding>,
    sources_used: i32,
}

#[derive(Debug, Serialize, Deserialize)]
struct KeyFinding {
    finding: String,
    confidence: String,
    source_hint: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CompetitorAnalysis {
    company: String,
    competitors: Vec<Competitor>,
    market_trends: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Competitor {
    name: String,
    strengths: Vec<String>,
    recent_news: String,
}

// ============================================================================
// Web Research Agent
// ============================================================================

struct WebResearchAgent {
    client: Client,
}

impl WebResearchAgent {
    fn new(client: Client) -> Self {
        Self { client }
    }

    /// Perform a grounded research query with structured output
    async fn research_topic(&self, topic: &str) -> Result<ResearchReport, Box<dyn Error>> {
        let schema = json!({
            "type": "object",
            "properties": {
                "topic": {"type": "string"},
                "summary": {"type": "string"},
                "key_findings": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "finding": {"type": "string"},
                            "confidence": {"type": "string", "enum": ["high", "medium", "low"]},
                            "source_hint": {"type": "string"}
                        },
                        "required": ["finding", "confidence", "source_hint"]
                    }
                },
                "sources_used": {"type": "integer"}
            },
            "required": ["topic", "summary", "key_findings", "sources_used"]
        });

        let prompt = format!(
            "Research the following topic and provide a comprehensive summary with key findings. \
             Use web search to find the most current information.\n\n\
             Topic: {}",
            topic
        );

        let response = self
            .client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_system_instruction(
                "You are a professional research analyst. Synthesize information from \
                 multiple sources, verify facts when possible, and clearly indicate \
                 confidence levels. Always cite your sources.",
            )
            .with_text(&prompt)
            .with_google_search()
            .with_response_format(schema)
            .create()
            .await?;

        // Display grounding steps
        let queries = response.google_search_calls();
        if !queries.is_empty() {
            println!("🔍 Search queries used:");
            for query in &queries {
                println!("   • {}", query);
            }
        }
        let sources = response.google_search_results();
        if !sources.is_empty() {
            println!("📚 Sources retrieved: {}", sources.len());
            for source in sources.iter().take(3) {
                println!("   • {} [{}]", source.title, source.url);
            }
            println!();
        }

        let text = response.as_text().ok_or("No response text")?;
        let report: ResearchReport = serde_json::from_str(text)?;
        Ok(report)
    }

    /// Research competitors with real-time web data
    async fn analyze_competitors(
        &self,
        company: &str,
    ) -> Result<CompetitorAnalysis, Box<dyn Error>> {
        let schema = json!({
            "type": "object",
            "properties": {
                "company": {"type": "string"},
                "competitors": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "strengths": {
                                "type": "array",
                                "items": {"type": "string"}
                            },
                            "recent_news": {"type": "string"}
                        },
                        "required": ["name", "strengths", "recent_news"]
                    }
                },
                "market_trends": {
                    "type": "array",
                    "items": {"type": "string"}
                }
            },
            "required": ["company", "competitors", "market_trends"]
        });

        let prompt = format!(
            "Analyze the competitive landscape for {}. Identify top 3 competitors, \
             their key strengths, recent news, and overall market trends. \
             Use web search to find the most current information.",
            company
        );

        let response = self
            .client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_system_instruction(
                "You are a market research analyst. Provide accurate, up-to-date \
                 competitive intelligence. Focus on verifiable facts and recent \
                 developments.",
            )
            .with_text(&prompt)
            .with_google_search()
            .with_response_format(schema)
            .create()
            .await?;

        // Show grounding info
        let sources = response.google_search_results();
        if !sources.is_empty() {
            println!("🔍 Grounded with {} sources", sources.len());
        }

        let text = response.as_text().ok_or("No response text")?;
        let analysis: CompetitorAnalysis = serde_json::from_str(text)?;
        Ok(analysis)
    }

    /// Stream a research response for real-time feedback
    async fn stream_research(&self, query: &str) -> Result<(), Box<dyn Error>> {
        println!("Streaming research response...\n");

        let mut stream = self
            .client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_system_instruction(
                "You are a research assistant. Provide well-sourced, accurate information. \
                 Organize your response with clear sections and cite sources inline.",
            )
            .with_text(query)
            .with_google_search()
            .create_stream();

        while let Some(result) = stream.next().await {
            match result {
                Ok(event) => match event.chunk {
                    StreamChunk::StepDelta { delta, .. } => {
                        if let Some(text) = delta.as_text() {
                            print!("{}", text);
                            stdout().flush()?;
                        }
                    }
                    StreamChunk::Completed(response) => {
                        println!("\n");
                        let sources = response.google_search_results();
                        if !sources.is_empty() {
                            println!("--- Sources ---");
                            for (i, source) in sources.iter().take(5).enumerate() {
                                println!("{}. {} - {}", i + 1, source.title, source.url);
                            }
                        }
                    }
                    _ => {}
                },
                Err(e) => {
                    eprintln!("\nStream error: {}", e);
                    break;
                }
            }
        }

        Ok(())
    }

    /// Quick fact check using web grounding
    async fn fact_check(&self, claim: &str) -> Result<String, Box<dyn Error>> {
        let prompt = format!(
            "Fact check the following claim. Search the web for evidence and provide \
             a verdict (TRUE, FALSE, PARTIALLY TRUE, or UNVERIFIABLE) with explanation.\n\n\
             Claim: {}",
            claim
        );

        let response = self
            .client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_system_instruction(
                "You are a fact-checker. Verify claims using reliable sources. \
                 Be objective and cite your sources. If evidence is inconclusive, \
                 say so clearly.",
            )
            .with_text(&prompt)
            .with_google_search()
            .create()
            .await?;

        // Show sources used
        let sources = response.google_search_results();
        if !sources.is_empty() {
            println!("📰 Sources consulted: {}", sources.len());
            for source in sources.iter().take(3) {
                println!("   • {}", source.url);
            }
            println!();
        }

        Ok(response
            .as_text()
            .unwrap_or("Unable to verify claim.")
            .to_string())
    }
}

// ============================================================================
// Main Demo
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in environment");
    let client = Client::builder(api_key).build()?;
    let agent = WebResearchAgent::new(client);

    println!("=== Web Research Agent Example ===\n");
    println!("Using Google Search grounding for real-time web data\n");

    // Demo 1: Topic Research with Structured Output
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📊 TOPIC RESEARCH\n");

    let topic = "Latest developments in Rust programming language 2024";
    println!("Researching: {}\n", topic);

    match agent.research_topic(topic).await {
        Ok(report) => {
            println!("📝 Research Report");
            println!("==================");
            println!("Topic: {}\n", report.topic);
            println!("Summary:\n{}\n", report.summary);
            println!("Key Findings:");
            for (i, finding) in report.key_findings.iter().enumerate() {
                println!(
                    "  {}. [{}] {}",
                    i + 1,
                    finding.confidence.to_uppercase(),
                    finding.finding
                );
                println!("     Source: {}", finding.source_hint);
            }
            println!("\nSources used: {}", report.sources_used);
        }
        Err(e) => {
            eprintln!("Research failed: {}", e);
            println!("Note: Google Search grounding may not be available in your region.");
        }
    }

    // Demo 2: Competitor Analysis
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("🏢 COMPETITOR ANALYSIS\n");

    let company = "OpenAI";
    println!("Analyzing competitors for: {}\n", company);

    match agent.analyze_competitors(company).await {
        Ok(analysis) => {
            println!("Competitive Analysis: {}", analysis.company);
            println!("=======================\n");
            println!("Top Competitors:");
            for comp in &analysis.competitors {
                println!("  📌 {}", comp.name);
                println!("     Strengths: {}", comp.strengths.join(", "));
                println!("     Recent: {}", comp.recent_news);
                println!();
            }
            println!("Market Trends:");
            for trend in &analysis.market_trends {
                println!("  • {}", trend);
            }
        }
        Err(e) => {
            eprintln!("Analysis failed: {}", e);
        }
    }

    // Demo 3: Fact Checking
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ FACT CHECKING\n");

    let claim = "Rust was voted the most loved programming language for 8 years in a row in the Stack Overflow Developer Survey";
    println!("Claim: {}\n", claim);

    match agent.fact_check(claim).await {
        Ok(result) => {
            println!("Verdict:\n{}", result);
        }
        Err(e) => {
            eprintln!("Fact check failed: {}", e);
        }
    }

    // Demo 4: Streaming Research
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("🌊 STREAMING RESEARCH\n");

    let query = "What are the top 3 AI news stories this week?";
    println!("Query: {}\n", query);

    if let Err(e) = agent.stream_research(query).await {
        eprintln!("Streaming failed: {}", e);
    }

    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Web Research Agent Demo Complete\n");

    println!("--- Production Considerations ---");
    println!("• Implement rate limiting for search queries");
    println!("• Cache research results to reduce API calls");
    println!("• Add source quality scoring and filtering");
    println!("• Implement retry logic for transient failures");
    println!("• Store research history for trend analysis");
    println!("• Add export capabilities (PDF, Markdown, etc.)");

    Ok(())
}
