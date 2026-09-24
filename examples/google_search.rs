//! Google Search grounding: answers informed by live web results.
//!
//! The search runs server-side. The response carries the queries the model
//! issued (`google_search_calls()`) and annotations tying spans of the
//! answer to their sources. The `google_search_result` steps currently carry
//! only `search_suggestions`, an HTML widget Google asks you to display
//! alongside grounded answers; their `title`/`url` fields are empty, so the
//! citations are where the sources are.
//!
//! Run with: `cargo run --example google_search`

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
        .with_text("What is the latest stable Rust release, and one notable feature in it?")
        .with_google_search()
        .create()
        .await?;

    let text = response.as_text().ok_or("no text in response")?;
    println!("{text}\n");

    for query in response.google_search_calls() {
        println!("Searched: {query}");
    }
    let suggestions = response
        .google_search_results()
        .iter()
        .filter(|r| r.search_suggestions.is_some())
        .count();
    println!("Search suggestion widgets to display: {suggestions}");

    // Annotation offsets index into the concatenated response text.
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
