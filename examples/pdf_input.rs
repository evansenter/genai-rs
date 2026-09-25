//! Document input: PDFs, and plain-text files sent as documents.
//!
//! `Content::document_data(base64, mime_type)` sends a document inline.
//! `document_from_file(path)` loads a PDF from disk; it rejects other
//! extensions, so text files go through `document_from_file_with_mime` with an
//! explicit `text/plain` (or are simply read into `Content::text`). Large or
//! reused documents belong in the Files API (`files_api`).
//!
//! Run with: `cargo run --example pdf_input`

use genai_rs::{Client, Content, document_from_file_with_mime};
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

    println!("--- Plain-text document from a file ---");
    let mut notes = tempfile::Builder::new().suffix(".txt").tempfile()?;
    writeln!(
        notes,
        "Release checklist: bump versions, tag, then watch the release workflow."
    )?;
    let document = document_from_file_with_mime(notes.path(), "text/plain").await?;
    let response = client
        .interaction()
        .with_model(model)
        .with_content(vec![
            Content::text("What is the last step of this checklist? One short phrase."),
            document,
        ])
        .create()
        .await?;
    println!("{}", response.as_text().ok_or("no text in response")?);

    Ok(())
}
