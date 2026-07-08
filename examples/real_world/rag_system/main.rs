//! # RAG System Example
//!
//! This example demonstrates a Retrieval-Augmented Generation (RAG) system that:
//! - Maintains a document store with simulated embeddings
//! - Retrieves relevant context based on user queries
//! - Augments prompts with retrieved context for accurate answers
//!
//! ## Production Patterns Demonstrated
//!
//! - Document chunking and storage
//! - Similarity-based retrieval (simulated with keyword matching)
//! - Context injection into prompts
//! - Source attribution in responses
//! - Error handling for retrieval failures
//!
//! ## Running
//!
//! ```bash
//! cargo run --example rag_system
//! ```
//!
//! ## Prerequisites
//!
//! Set the `GEMINI_API_KEY` environment variable with your API key.

use genai_rs::Client;
use std::env;
use std::error::Error;

/// Represents a document chunk with metadata
#[derive(Debug, Clone)]
struct DocumentChunk {
    id: String,
    content: String,
    source: String,
    /// Simulated embedding (in production, use actual vector embeddings)
    keywords: Vec<String>,
}

/// Simple document store for RAG
struct DocumentStore {
    chunks: Vec<DocumentChunk>,
}

impl DocumentStore {
    fn new() -> Self {
        Self { chunks: Vec::new() }
    }

    /// Add a document chunk to the store
    fn add_chunk(&mut self, id: &str, content: &str, source: &str, keywords: Vec<&str>) {
        self.chunks.push(DocumentChunk {
            id: id.to_string(),
            content: content.to_string(),
            source: source.to_string(),
            keywords: keywords.into_iter().map(String::from).collect(),
        });
    }

    /// Retrieve relevant chunks based on query (simulated semantic search)
    /// In production, this would use vector similarity with actual embeddings
    fn retrieve(&self, query: &str, top_k: usize) -> Vec<&DocumentChunk> {
        let query_lower = query.to_lowercase();
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();

        // Score each chunk by keyword overlap (simulating semantic similarity)
        let mut scored: Vec<(&DocumentChunk, usize)> = self
            .chunks
            .iter()
            .map(|chunk| {
                let score = chunk
                    .keywords
                    .iter()
                    .filter(|kw| {
                        query_words
                            .iter()
                            .any(|qw| kw.contains(*qw) || qw.contains(kw.as_str()))
                    })
                    .count();
                (chunk, score)
            })
            .filter(|(_, score)| *score > 0)
            .collect();

        // Sort by score descending
        scored.sort_by_key(|entry| std::cmp::Reverse(entry.1));

        // Return top-k results
        scored
            .into_iter()
            .take(top_k)
            .map(|(chunk, _)| chunk)
            .collect()
    }
}

/// Build a context string from retrieved chunks
fn build_context(chunks: &[&DocumentChunk]) -> String {
    if chunks.is_empty() {
        return String::new();
    }

    let mut context = String::from("Relevant context from knowledge base:\n\n");
    for (i, chunk) in chunks.iter().enumerate() {
        context.push_str(&format!(
            "[Source {}: {}]\n{}\n\n",
            i + 1,
            chunk.source,
            chunk.content
        ));
    }
    context
}

/// Build a RAG-augmented prompt
fn build_rag_prompt(query: &str, context: &str) -> String {
    if context.is_empty() {
        return format!(
            "I don't have specific information in my knowledge base about this topic. \
             Please answer based on general knowledge:\n\n{}",
            query
        );
    }

    format!(
        "{}\n\
         Based on the context above, please answer the following question. \
         If the context doesn't contain relevant information, say so.\n\n\
         Question: {}",
        context, query
    )
}

/// Initialize the document store with sample company knowledge base
fn create_sample_knowledge_base() -> DocumentStore {
    let mut store = DocumentStore::new();

    // Company policies
    store.add_chunk(
        "policy-001",
        "Our company offers 25 days of paid time off (PTO) per year for full-time employees. \
         PTO accrues at approximately 2.08 days per month. Unused PTO can be carried over \
         up to a maximum of 10 days into the next calendar year.",
        "HR Policy Manual - Chapter 4: Time Off",
        vec!["pto", "vacation", "time off", "days", "leave", "holiday"],
    );

    store.add_chunk(
        "policy-002",
        "Remote work is permitted up to 3 days per week for eligible positions. \
         Employees must maintain core hours of 10am-3pm in their local timezone. \
         A stable internet connection and dedicated workspace are required.",
        "HR Policy Manual - Chapter 7: Remote Work",
        vec!["remote", "work", "home", "wfh", "hybrid", "office"],
    );

    store.add_chunk(
        "policy-003",
        "The company provides health insurance coverage for employees and dependents. \
         Plans include PPO and HMO options with varying deductibles. \
         Enrollment is available during annual open enrollment or qualifying life events.",
        "Benefits Guide 2024",
        vec![
            "health",
            "insurance",
            "medical",
            "benefits",
            "ppo",
            "hmo",
            "coverage",
        ],
    );

    // Technical documentation
    store.add_chunk(
        "tech-001",
        "The authentication service uses JWT tokens with a 1-hour expiration. \
         Refresh tokens are valid for 30 days. All API endpoints require the \
         Authorization header with 'Bearer <token>' format.",
        "API Documentation - Authentication",
        vec![
            "auth",
            "jwt",
            "token",
            "api",
            "login",
            "authentication",
            "bearer",
        ],
    );

    store.add_chunk(
        "tech-002",
        "Database connections should use the connection pool with a maximum of 20 connections. \
         Queries exceeding 5 seconds trigger automatic timeout. Use prepared statements \
         to prevent SQL injection attacks.",
        "Engineering Handbook - Database Best Practices",
        vec!["database", "sql", "connection", "pool", "query", "timeout"],
    );

    store.add_chunk(
        "tech-003",
        "Deployment to production requires approval from at least one senior engineer. \
         All changes must pass CI/CD pipeline including unit tests (>80% coverage), \
         integration tests, and security scans. Deployments are frozen on Fridays.",
        "Engineering Handbook - Deployment Procedures",
        vec![
            "deploy",
            "deployment",
            "production",
            "ci",
            "cd",
            "release",
            "pipeline",
        ],
    );

    store
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize client
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in environment");
    let client = Client::builder(api_key).build()?;

    println!("=== RAG System Example ===\n");

    // Initialize document store
    let store = create_sample_knowledge_base();
    println!(
        "Knowledge base loaded with {} document chunks\n",
        store.chunks.len()
    );

    // Define sample queries to demonstrate RAG
    let queries = vec![
        "How many vacation days do I get per year?",
        "Can I work from home?",
        "How do I authenticate API requests?",
        "What's the deployment process?",
        "What's the company's policy on pets in the office?", // No relevant docs
    ];

    for query in queries {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("📝 User Query: {}\n", query);

        // Step 1: Retrieve relevant documents
        let retrieved = store.retrieve(query, 2);

        if retrieved.is_empty() {
            println!("🔍 No relevant documents found in knowledge base\n");
        } else {
            println!("🔍 Retrieved {} relevant document(s):", retrieved.len());
            for chunk in &retrieved {
                println!("   - [{}] {}", chunk.id, chunk.source);
            }
            println!();
        }

        // Step 2: Build context from retrieved documents
        let context = build_context(&retrieved);

        // Step 3: Build RAG-augmented prompt
        let augmented_prompt = build_rag_prompt(query, &context);

        // Step 4: Send to model with system instruction for RAG behavior
        let response = client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_system_instruction(
                "You are a helpful assistant answering questions about company policies and \
                 technical documentation. Always cite your sources when referencing the \
                 provided context. If the context doesn't contain relevant information, \
                 clearly state that the information is not available in the knowledge base.",
            )
            .with_text(&augmented_prompt)
            .create()
            .await?;

        // Step 5: Display response with attribution
        if let Some(text) = response.as_text() {
            println!("🤖 Assistant:\n{}\n", text);
        }

        // Show sources used (for transparency)
        if !retrieved.is_empty() {
            println!("📚 Sources:");
            for chunk in &retrieved {
                println!("   • {}", chunk.source);
            }
            println!();
        }
    }

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ RAG System Demo Complete\n");

    // Display performance summary
    println!("--- Production Considerations ---");
    println!("• Replace keyword matching with vector embeddings (e.g., via embedding API)");
    println!("• Use a vector database (Pinecone, Qdrant, Weaviate) for scalable retrieval");
    println!("• Implement document chunking strategies for large documents");
    println!("• Add caching for frequently accessed queries");
    println!("• Monitor retrieval quality and tune similarity thresholds");

    Ok(())
}
