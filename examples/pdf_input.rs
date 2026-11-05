//! Document input: a PDF, and a CSV file sent as a document.
//!
//! `Content::document_data(base64, mime_type)` sends a document inline.
//! `document_from_file(path)` loads one from disk, picking the MIME type from
//! the extension. Inline documents must be PDF, CSV or Office/OpenDocument
//! files: since 2026-11-04 the API rejects `text/plain` and `text/markdown`
//! documents, so send plain text as `Content::text`, or upload it to the Files
//! API and reference it by URI (`files_api`), which also suits large or reused
//! documents.
//!
//! Run with: `cargo run --example pdf_input`

use genai_rs::{Client, Content, document_from_file};
use std::env;
use std::error::Error;
use std::io::Write;

// A one-page PDF whose only text is "Hello World".
const SAMPLE_PDF_BASE64: &str = "JVBERi0xLjQKMSAwIG9iago8PCAvVHlwZSAvQ2F0YWxvZyAvUGFnZXMgMiAwIFIgPj4KZW5kb2JqCjIgMCBvYmoKPDwgL1R5cGUgL1BhZ2VzIC9LaWRzIFszIDAgUl0gL0NvdW50IDEgPj4KZW5kb2JqCjMgMCBvYmoKPDwgL1R5cGUgL1BhZ2UgL1BhcmVudCAyIDAgUiAvTWVkaWFCb3ggWzAgMCA3MiA3Ml0gL0NvbnRlbnRzIDQgMCBSIC9SZXNvdXJjZXMgPDwgPj4gPj4KZW5kb2JqCjQgMCBvYmoKPDwgL0xlbmd0aCA0NCA+PgpzdHJlYW0KQlQgL0YxIDEyIFRmIDEwIDUwIFRkIChIZWxsbyBXb3JsZCkgVGogRVQKZW5kc3RyZWFtCmVuZG9iagp4cmVmCjAgNQowMDAwMDAwMDAwIDY1NTM1IGYgCjAwMDAwMDAwMDkgMDAwMDAgbiAKMDAwMDAwMDA1OCAwMDAwMCBuIAowMDAwMDAwMTE1IDAwMDAwIG4gCjAwMDAwMDAyMjQgMDAwMDAgbiAKdHJhaWxlcgo8PCAvU2l6ZSA1IC9Sb290IDEgMCBSID4+CnN0YXJ0eHJlZgozMjAKJSVFT0Y=";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = Client::builder(api_key).build()?;
    let model = genai_rs::DEFAULT_MODEL;

    println!("--- PDF ---");
    let response = client
        .interaction()
        .with_model(model)
        .with_content(vec![
            Content::text("What text does this PDF contain?"),
            Content::document_data(SAMPLE_PDF_BASE64, "application/pdf"),
        ])
        .create()
        .await?;
    println!("{}\n", response.as_text().ok_or("no text in response")?);

    // The document stays in the stored conversation for follow-ups.
    let follow_up = client
        .interaction()
        .with_model(model)
        .with_previous_interaction(
            response
                .id
                .as_deref()
                .ok_or("stored interaction has no ID")?,
        )
        .with_text("How many pages does it have? Just the number.")
        .create()
        .await?;
    println!(
        "Pages: {}\n",
        follow_up.as_text().ok_or("no text in response")?
    );

    println!("--- CSV document from a file ---");
    let mut inventory = tempfile::Builder::new().suffix(".csv").tempfile()?;
    write!(inventory, "item,crates\nbolts,417\nnuts,52\nwashers,9\n")?;
    let document = document_from_file(inventory.path()).await?;
    let response = client
        .interaction()
        .with_model(model)
        .with_content(vec![
            Content::text("How many crates of bolts are there? Just the number."),
            document,
        ])
        .create()
        .await?;
    let answer = response.as_text().ok_or("no text in response")?;
    println!("Bolts: {answer}");
    if !answer.contains("417") {
        return Err(format!("the model did not read 417 out of the CSV: {answer}").into());
    }

    Ok(())
}
