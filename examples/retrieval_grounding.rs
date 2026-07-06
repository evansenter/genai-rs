//! Example: Retrieval tool grounding
//!
//! Demonstrates the built-in `retrieval` tool, which grounds model responses
//! in external retrieval backends:
//!
//! - **Vertex AI Search** (`vertex_ai_search`): enterprise search engines
//!   and datastores
//! - **RAG Store** (`rag_store`): Vertex RAG corpora with hybrid search,
//!   filters, and ranking
//! - **Exa.ai** (`exa_ai_search`) and **Parallel.ai** (`parallel_ai_search`):
//!   third-party search APIs (bring your own API key)
//!
//! The retrieval backends require pre-provisioned resources (search engines,
//! RAG corpora) or third-party API keys, so this example prints the request
//! wire shapes and only calls the API when both `GEMINI_API_KEY` and
//! `VERTEX_AI_SEARCH_ENGINE` are set.
//!
//! Run with: cargo run --example retrieval_grounding

use genai_rs::{
    Client, ExaAiSearchConfig, RagFilter, RagRanking, RagResource, RagRetrievalConfig,
    RagStoreConfig, RetrievalConfig, VertexAiSearchConfig,
};
use std::env;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").ok();
    let engine = env::var("VERTEX_AI_SEARCH_ENGINE").ok();

    let client = Client::new(api_key.clone().unwrap_or_else(|| "unused".to_string()));

    // -------------------------------------------------------------------
    // 1. Vertex AI Search grounding
    // -------------------------------------------------------------------
    let vertex_request = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("What does our handbook say about vacation policy?")
        .add_tool(
            RetrievalConfig::new().with_vertex_ai_search(
                VertexAiSearchConfig::new().with_engine(
                    engine
                        .clone()
                        .unwrap_or_else(|| "projects/p/locations/global/engines/my-engine".into()),
                ),
            ),
        )
        .build()?;

    println!("=== Vertex AI Search retrieval request ===");
    println!("{}\n", serde_json::to_string_pretty(&vertex_request)?);

    // -------------------------------------------------------------------
    // 2. RAG Store grounding with full retrieval config
    // -------------------------------------------------------------------
    let rag_request = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Summarize the design documents about caching")
        .add_tool(
            RetrievalConfig::new().with_rag_store(
                RagStoreConfig::new(vec![
                    RagResource::new("projects/p/locations/us/ragCorpora/docs")
                        .with_rag_file_ids(vec!["file-1".to_string()]),
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
            ),
        )
        .build()?;

    println!("=== RAG Store retrieval request ===");
    println!("{}\n", serde_json::to_string_pretty(&rag_request)?);

    // -------------------------------------------------------------------
    // 3. Third-party search backends (Exa.ai / Parallel.ai)
    // -------------------------------------------------------------------
    let exa_request = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Find recent papers on speculative decoding")
        .add_tool(
            RetrievalConfig::new().with_exa_ai_search(
                ExaAiSearchConfig::new("YOUR_EXA_API_KEY")
                    .with_custom_config(serde_json::json!({"num_results": 5})),
            ),
        )
        .build()?;

    println!("=== Exa.ai retrieval request ===");
    println!("{}\n", serde_json::to_string_pretty(&exa_request)?);

    // -------------------------------------------------------------------
    // 4. Live call (only with a real engine configured)
    // -------------------------------------------------------------------
    match (api_key.is_some(), engine.is_some()) {
        (true, true) => {
            println!("=== Live Vertex AI Search call ===");
            match client.execute(vertex_request).await {
                Ok(response) => {
                    println!("Status: {:?}", response.status);
                    if let Some(text) = response.as_text() {
                        println!("Grounded answer:\n{text}");
                    }
                    // Citations arrive as annotations on text content
                    let annotations = response.all_annotations().count();
                    println!("Annotations (citations): {annotations}");
                }
                Err(e) => println!("Retrieval call failed: {e}"),
            }
        }
        (true, false) => {
            println!("VERTEX_AI_SEARCH_ENGINE not set - skipping live retrieval call.");
        }
        _ => {
            println!("GEMINI_API_KEY not set - skipping live API calls.");
        }
    }

    println!("\n=== Example Complete ===\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("  [REQ#1] POST with input + retrieval tool");
    println!("          (retrieval_types + per-backend config, snake_case)");
    println!("  [RES#1] completed: text grounded in retrieved documents,");
    println!("          citations as annotations\n");

    println!("--- Production Considerations ---");
    println!("• retrieval_types must match the configs you provide -");
    println!("  RetrievalConfig keeps them in sync automatically");
    println!("• Exa.ai/Parallel.ai api_key values are sent on the wire -");
    println!("  load them from secrets management, never hardcode");
    println!("• rag_store similarity_top_k / vector_distance_threshold are");
    println!("  deprecated by the API - prefer rag_retrieval_config");
    println!("• Vertex AI Search engines and RAG corpora must be provisioned");
    println!("  in advance; invalid resources return structured API errors");

    Ok(())
}
