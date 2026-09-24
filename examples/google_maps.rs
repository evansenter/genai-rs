//! Google Maps grounding: location-aware answers with structured place data.
//!
//! `with_google_maps()` enables the tool with defaults. `GoogleMapsConfig`
//! adds options such as a widget context token for rendering an interactive
//! map next to the answer.
//!
//! Every `Place` field is optional, and in practice results carry little
//! more than a place ID, a name and a Maps URL — one entry for the place
//! plus one per cited review, sharing the place ID. Fields this crate doesn't
//! model yet are kept in `Place::extra`.
//!
//! Run with: `cargo run --example google_maps`

use genai_rs::{Client, GoogleMapsConfig};
use std::collections::HashSet;
use std::env;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = Client::builder(api_key).build()?;
    let model = genai_rs::DEFAULT_MODEL;

    println!("--- Places ---");
    let response = client
        .interaction()
        .with_model(model)
        .with_text("Find three well-rated Italian restaurants near the Eiffel Tower.")
        .with_google_maps()
        .create()
        .await?;
    let mut seen = HashSet::new();
    for result in response.google_maps_results() {
        for place in result.items.iter().flat_map(|i| i.places.iter().flatten()) {
            // Review entries repeat the place ID; list each place once.
            if place
                .place_id
                .as_ref()
                .is_some_and(|id| !seen.insert(id.clone()))
            {
                continue;
            }
            println!(
                "{} <{}>",
                place.name.as_deref().unwrap_or("(unnamed)"),
                place.url.as_deref().unwrap_or("no URL")
            );
        }
    }
    println!("\n{}\n", response.as_text().ok_or("no text in response")?);

    println!("--- Widget context token ---");
    let response = client
        .interaction()
        .with_model(model)
        .with_text("Find a coffee shop in downtown Seattle.")
        .add_tool(GoogleMapsConfig::new().with_widget())
        .create()
        .await?;
    let token = response
        .google_maps_results()
        .iter()
        .flat_map(|r| r.items)
        .find_map(|i| i.widget_context_token.clone());
    match token {
        Some(token) => println!("Token for the Maps widget: {} chars", token.len()),
        None => println!("No widget token returned for this answer"),
    }
    println!("{}", response.as_text().ok_or("no text in response")?);

    Ok(())
}
