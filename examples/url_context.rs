//! URL context: the model fetches and reads web pages named in the prompt.
//!
//! Fetching happens server-side. Each URL the model requested comes back
//! with a status ("success", "error", "paywall", "unsafe", ...), so a failed
//! fetch is visible rather than silently answered from memory.
//!
//! Run with: `cargo run --example url_context`

use genai_rs::Client;
use std::env;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = Client::builder(api_key).build()?;

    let response = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_text(
            "Summarize https://www.rust-lang.org in two sentences: what does the page \
             say Rust is for?",
        )
        .with_url_context()
        .create()
        .await?;

    for url in response.url_context_call_urls() {
        println!("Requested: {url}");
    }
    for result in response.url_context_results() {
        for item in result.items {
            println!("Fetched {} -> {}", item.url, item.status);
        }
    }
    println!("\n{}", response.as_text().ok_or("no text in response")?);

    let all_text = response.all_text();
    for annotation in response.all_annotations() {
        if let Some(span) = annotation.extract_span(&all_text) {
            let preview: String = span.chars().take(60).collect::<String>().replace('\n', " ");
            println!(
                "Cited \"{preview}\" -> {}",
                annotation.source().unwrap_or("<no source>")
            );
        }
    }

    Ok(())
}
