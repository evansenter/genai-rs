//! Example: PDF Document Input
//!
//! This example demonstrates how to send PDF documents to the Gemini API
//! for analysis, extraction, and question answering.
//!
//! # Running
//!
//! ```bash
//! cargo run --example pdf_input
//! ```
//!
//! # Prerequisites
//!
//! Set the `GEMINI_API_KEY` environment variable with your API key.
//!
//! # PDF Support
//!
//! Gemini models can process PDF documents natively, understanding:
//! - Text content (both native text and OCR from scanned documents)
//! - Images, diagrams, charts, and tables
//! - Document structure and layout
//!
//! PDFs can be up to 1000 pages and are tokenized at approximately
//! 258 tokens per page. Since each page is treated as an image, costs follow
//! Gemini's image pricing (see <https://ai.google.dev/gemini-api/docs/document-processing>).

use futures_util::StreamExt;
use genai_rs::{Client, Content, StreamChunk};
use std::env;
use std::io::{Write, stdout};

// A minimal PDF document containing "Hello World" text for demonstration
// In real applications, you would read a PDF file and base64 encode it
const SAMPLE_PDF_BASE64: &str = "JVBERi0xLjQKMSAwIG9iago8PCAvVHlwZSAvQ2F0YWxvZyAvUGFnZXMgMiAwIFIgPj4KZW5kb2JqCjIgMCBvYmoKPDwgL1R5cGUgL1BhZ2VzIC9LaWRzIFszIDAgUl0gL0NvdW50IDEgPj4KZW5kb2JqCjMgMCBvYmoKPDwgL1R5cGUgL1BhZ2UgL1BhcmVudCAyIDAgUiAvTWVkaWFCb3ggWzAgMCA3MiA3Ml0gL0NvbnRlbnRzIDQgMCBSIC9SZXNvdXJjZXMgPDwgPj4gPj4KZW5kb2JqCjQgMCBvYmoKPDwgL0xlbmd0aCA0NCA+PgpzdHJlYW0KQlQgL0YxIDEyIFRmIDEwIDUwIFRkIChIZWxsbyBXb3JsZCkgVGogRVQKZW5kc3RyZWFtCmVuZG9iagp4cmVmCjAgNQowMDAwMDAwMDAwIDY1NTM1IGYgCjAwMDAwMDAwMDkgMDAwMDAgbiAKMDAwMDAwMDA1OCAwMDAwMCBuIAowMDAwMDAwMTE1IDAwMDAwIG4gCjAwMDAwMDAyMjQgMDAwMDAgbiAKdHJhaWxlcgo8PCAvU2l6ZSA1IC9Sb290IDEgMCBSID4+CnN0YXJ0eHJlZgozMjAKJSVFT0Y=";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get API key from environment
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY environment variable not set");

    let client = Client::builder(api_key).build()?;

    println!("=== PDF DOCUMENT INPUT EXAMPLE ===\n");

    // ==========================================================================
    // Example 1: Basic PDF Analysis (Fluent Builder Pattern)
    // ==========================================================================
    println!("--- Example 1: Basic PDF Analysis ---\n");

    // Using with_content() with Content::document_data()
    // In a real application, use document_from_file() for automatic file loading:
    //   let doc = document_from_file("document.pdf").await?;
    //
    println!("Sending PDF to Gemini for analysis...\n");

    let response = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_content(vec![
            Content::text("What text content does this PDF document contain?"),
            Content::document_data(SAMPLE_PDF_BASE64, "application/pdf"),
        ])
        .with_store_enabled()
        .create()
        .await?;

    println!("Status: {:?}\n", response.status);

    if let Some(text) = response.as_text() {
        println!("Analysis:\n{}\n", text);
    }

    // ==========================================================================
    // Example 2: PDF with Follow-up Questions
    // ==========================================================================
    println!("--- Example 2: Follow-up Questions ---\n");

    // Use stateful conversation to ask follow-up questions about the PDF
    let follow_up = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_previous_interaction(
            response
                .id
                .as_ref()
                .expect("id should exist when store=true"),
        )
        .with_text("What format is this document? Is it a valid PDF structure?")
        .with_store_enabled()
        .create()
        .await?;

    if let Some(text) = follow_up.as_text() {
        println!("Follow-up Response:\n{}\n", text);
    }

    // ==========================================================================
    // Example 3: PDF with Streaming Response
    // ==========================================================================
    println!("--- Example 3: Streaming PDF Analysis ---\n");

    print!("Streaming Response: ");
    // Flush to ensure the prompt appears before streaming starts (stdout is line-buffered)
    stdout().flush()?;

    // Alternative: Using with_content() for dynamic content construction
    let stream_contents = vec![
        Content::text("Describe the structure of this PDF document in detail."),
        Content::document_data(SAMPLE_PDF_BASE64, "application/pdf"),
    ];

    let mut stream = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_content(stream_contents)
        .create_stream();

    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => match event.chunk {
                StreamChunk::StepDelta { delta, .. } => {
                    if let Some(text) = delta.as_text() {
                        print!("{}", text);
                        // Flush each chunk immediately for real-time streaming effect
                        stdout().flush()?;
                    }
                }
                StreamChunk::Completed(response) => {
                    println!("\n");
                    if let Some(usage) = response.usage {
                        if let Some(input) = usage.total_input_tokens {
                            println!("Input tokens: {}", input);
                        }
                        if let Some(output) = usage.total_output_tokens {
                            println!("Output tokens: {}", output);
                        }
                    }
                }
                _ => {} // Handle unknown variants
            },
            Err(e) => {
                eprintln!("\nStream error: {}", e);
                break;
            }
        }
    }

    // ==========================================================================
    // Usage Notes
    // ==========================================================================
    println!("\n--- Usage Notes ---\n");
    println!("PDF Document Input Tips:");
    println!("  1. Use Content::document_data(base64, \"application/pdf\") for inline");
    println!("  2. Use document_from_file() to load and encode files");
    println!("  3. PDFs up to 1000 pages are supported");
    println!("  4. Each page costs approximately 258 tokens");
    println!("  5. Native text extraction works for most PDFs");
    println!("  6. OCR is applied automatically to scanned pages");
    println!("\nRecommended: Use document_from_file() helper:");
    println!("  use genai_rs::document_from_file;");
    println!("  let doc = document_from_file(\"doc.pdf\").await?;");

    // =========================================================================
    // Summary
    // =========================================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ PDF Document Input Demo Complete\n");

    println!("--- Key Takeaways ---");
    println!("• Content::document_data(base64, \"application/pdf\") for inline PDF");
    println!("• document_from_file() helper loads and encodes files");
    println!("• with_content() accepts mixed Content::text() + Content::document_data()");
    println!("• PDFs up to 1000 pages supported (~258 tokens/page)\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("  [REQ#1] POST with text + inlineData (PDF base64 truncated)");
    println!("  [RES#1] completed: text extraction or analysis\n");
    println!("Follow-up:");
    println!("  [REQ#2] POST with text + previousInteractionId");
    println!("  [RES#2] completed: answer using PDF context\n");
    println!("Streaming:");
    println!("  [REQ#3] POST streaming with PDF content");
    println!("  [RES#3] SSE stream: analysis chunks → completed\n");

    println!("--- Production Considerations ---");
    println!("• Native text extraction + OCR for scanned pages");
    println!("• Images, diagrams, tables are understood");
    println!("• Each page tokenized as image (~258 tokens)");
    println!("• For large PDFs, use Files API for efficiency");

    Ok(())
}
