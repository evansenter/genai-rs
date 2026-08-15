//! Example: Text Document Input
//!
//! This example demonstrates how to send text-based documents (TXT, Markdown,
//! JSON, CSV, HTML, XML) to the Gemini API for analysis.
//!
//! # Running
//!
//! ```bash
//! cargo run --example text_input
//! ```
//!
//! # Prerequisites
//!
//! Set the `GEMINI_API_KEY` environment variable with your API key.
//!
//! # Supported Formats
//!
//! The `document_from_file()` function supports these text formats:
//!
//! | Extension | MIME Type | Notes |
//! |-----------|-----------|-------|
//! | `.txt` | `text/plain` | Best for general text |
//! | `.md` | `text/markdown` | Markdown formatting preserved |
//! | `.pdf` | `application/pdf` | Native PDF support |
//!
//! For JSON, CSV, HTML, and XML files, use `text/plain` as the MIME type
//! when using `Content::document_data()`. The model can still parse the content.

use base64::Engine;
use genai_rs::{Client, Content, document_from_file};
use std::env;

/// Helper to base64-encode text for document_data_content
fn encode_text(text: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(text.as_bytes())
}

// Sample JSON data for demonstration
const SAMPLE_JSON: &str = r#"{
  "users": [
    {"name": "Alice", "age": 30, "role": "engineer"},
    {"name": "Bob", "age": 25, "role": "designer"},
    {"name": "Carol", "age": 35, "role": "manager"}
  ],
  "metadata": {
    "version": "1.0",
    "generated": "2024-01-15"
  }
}"#;

// Sample CSV data for demonstration
const SAMPLE_CSV: &str = r#"name,age,role,department
Alice,30,engineer,R&D
Bob,25,designer,Product
Carol,35,manager,Operations
David,28,analyst,Finance"#;

// Sample Markdown for demonstration
const SAMPLE_MARKDOWN: &str = r#"# Project Status Report

## Summary
The project is on track for Q1 delivery.

## Key Metrics
- **Completion**: 75%
- **Budget**: Under by 10%
- **Team Size**: 8 members

## Next Steps
1. Complete integration testing
2. Finalize documentation
3. Prepare deployment scripts
"#;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY environment variable not set");

    let client = Client::builder(api_key).build()?;

    println!("=== TEXT DOCUMENT INPUT EXAMPLE ===\n");

    // ==========================================================================
    // Example 1: JSON Analysis
    // ==========================================================================
    println!("--- Example 1: JSON Analysis ---\n");

    let response = client
        .interaction()
        .with_model("gemini-3.6-flash")
        .with_content(vec![
            Content::text("Analyze this JSON data. How many users are there and what roles exist?"),
            // Note: Use text/plain for JSON - API doesn't support application/json as document type
            Content::document_data(encode_text(SAMPLE_JSON), "text/plain"),
        ])
        .create()
        .await?;

    if let Some(text) = response.as_text() {
        println!("JSON Analysis:\n{}\n", text);
    }

    // ==========================================================================
    // Example 2: CSV Data Extraction
    // ==========================================================================
    println!("--- Example 2: CSV Data Extraction ---\n");

    let response = client
        .interaction()
        .with_model("gemini-3.6-flash")
        .with_content(vec![
            Content::text("Parse this CSV and calculate the average age of all employees."),
            // Note: Use text/plain for CSV - API doesn't support text/csv as document type
            Content::document_data(encode_text(SAMPLE_CSV), "text/plain"),
        ])
        .create()
        .await?;

    if let Some(text) = response.as_text() {
        println!("CSV Analysis:\n{}\n", text);
    }

    // ==========================================================================
    // Example 3: Markdown Summarization (inline data)
    // ==========================================================================
    println!("--- Example 3: Markdown Summarization (inline) ---\n");

    let response = client
        .interaction()
        .with_model("gemini-3.6-flash")
        .with_content(vec![
            Content::text("Summarize this markdown document in one sentence."),
            // text/markdown is supported for markdown content
            Content::document_data(encode_text(SAMPLE_MARKDOWN), "text/markdown"),
        ])
        .create()
        .await?;

    if let Some(text) = response.as_text() {
        println!("Markdown Summary:\n{}\n", text);
    }

    // ==========================================================================
    // Example 4: File-based Loading with document_from_file()
    // ==========================================================================
    println!("--- Example 4: File-based Loading ---\n");

    // Load a markdown file from disk - MIME type is auto-detected from extension
    let readme_content = document_from_file("README.md").await?;

    let response = client
        .interaction()
        .with_model("gemini-3.6-flash")
        .with_content(vec![
            Content::text("What is the main purpose of this project based on the README?"),
            readme_content,
        ])
        .create()
        .await?;

    if let Some(text) = response.as_text() {
        println!("README Analysis:\n{}\n", text);
    }

    // ==========================================================================
    // Example 5: Builder Pattern with document_from_file()
    // ==========================================================================
    println!("--- Example 5: Builder Pattern ---\n");

    let doc = document_from_file("CHANGELOG.md").await?;
    let response = client
        .interaction()
        .with_model("gemini-3.6-flash")
        .with_content(vec![
            Content::text("List the main sections in this markdown file."),
            doc,
        ])
        .create()
        .await?;

    if let Some(text) = response.as_text() {
        println!("CHANGELOG Sections:\n{}\n", text);
    }

    // ==========================================================================
    // Usage Notes
    // ==========================================================================
    println!("--- Usage Notes ---\n");
    println!("Text Document Input Tips:");
    println!("  1. Use document_from_file() for automatic file loading:");
    println!("     let doc = document_from_file(\"data.json\").await?;");
    println!();
    println!("  2. Combine with with_content():");
    println!("     client.interaction()");
    println!("         .with_content(vec![");
    println!("             Content::text(\"Summarize this\"),");
    println!("             doc,");
    println!("         ])");
    println!("         .create().await?;");
    println!();
    println!("  3. For inline data, use Content::document_data() with base64:");
    println!("     let encoded = base64::engine::general_purpose::STANDARD.encode(text);");
    println!("     Content::document_data(&encoded, \"text/plain\")");
    println!();
    println!("Native document types: .txt, .md, .pdf");
    println!("For JSON/CSV/HTML/XML: use text/plain as MIME type");

    // =========================================================================
    // Summary
    // =========================================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Text Document Input Demo Complete\n");

    println!("--- Key Takeaways ---");
    println!("• Content::document_data(base64, mime_type) for inline documents");
    println!("• document_from_file() auto-loads and encodes files");
    println!("• with_content(vec![...]) for combining text and documents");
    println!("• Native types: text/plain, text/markdown, application/pdf\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("  [REQ#1] POST with text + inlineData (JSON content)");
    println!("  [RES#1] completed: data analysis\n");
    println!("  [REQ#2] POST with text + inlineData (CSV content)");
    println!("  [RES#2] completed: calculated results\n");
    println!("File-based:");
    println!("  [REQ#3] POST with text + inlineData (file content)");
    println!("  [RES#3] completed: file analysis\n");

    println!("--- Production Considerations ---");
    println!("• Content::document_data() requires base64-encoded input");
    println!("• Use text/plain for JSON, CSV, HTML, XML content");
    println!("• Model can still parse structured formats from plain text");
    println!("• For very large text files, use Files API");

    Ok(())
}
