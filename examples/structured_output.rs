//! Structured output: constrain the response to a JSON schema.
//!
//! `with_response_format(schema)` makes the model return JSON matching the
//! schema, so it can be deserialized straight into your own types. It
//! composes with tools (Google Search here) and with streaming, where the
//! JSON arrives in fragments and parses once complete.
//!
//! Run with: `cargo run --example structured_output`

use futures_util::StreamExt;
use genai_rs::{Client, StreamChunk};
use serde::Deserialize;
use serde_json::json;
use std::env;
use std::error::Error;

#[derive(Debug, Deserialize)]
struct MovieReview {
    title: String,
    year: i32,
    rating_out_of_10: f64,
    pros: Vec<String>,
    cons: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Recipe {
    name: String,
    difficulty: Difficulty,
    ingredients: Vec<Ingredient>,
    steps: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Difficulty {
    Easy,
    Medium,
    Hard,
}

#[derive(Debug, Deserialize)]
struct Ingredient {
    item: String,
    amount: String,
}

#[derive(Debug, Deserialize)]
struct Release {
    project: String,
    latest_version: String,
    highlights: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = Client::builder(api_key).build()?;
    let model = genai_rs::DEFAULT_MODEL;

    println!("--- Flat schema ---");
    let response = client
        .interaction()
        .with_model(model)
        .with_text("Review the movie 'Inception' (2010). Be concise.")
        .with_response_format(json!({
            "type": "object",
            "properties": {
                "title": {"type": "string"},
                "year": {"type": "integer"},
                "rating_out_of_10": {"type": "number", "minimum": 0, "maximum": 10},
                "pros": {"type": "array", "items": {"type": "string"}},
                "cons": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["title", "year", "rating_out_of_10", "pros", "cons"]
        }))
        .create()
        .await?;
    let review: MovieReview =
        serde_json::from_str(response.as_text().ok_or("no text in response")?)?;
    println!(
        "{} ({}): {}/10\n  pros: {:?}\n  cons: {:?}\n",
        review.title, review.year, review.rating_out_of_10, review.pros, review.cons
    );

    println!("--- Nested objects and enums ---");
    let response = client
        .interaction()
        .with_model(model)
        .with_text("A simple pasta recipe with at most 5 ingredients and 4 steps.")
        .with_response_format(json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "difficulty": {"type": "string", "enum": ["easy", "medium", "hard"]},
                "ingredients": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {"item": {"type": "string"}, "amount": {"type": "string"}},
                        "required": ["item", "amount"]
                    }
                },
                "steps": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["name", "difficulty", "ingredients", "steps"]
        }))
        .create()
        .await?;
    let recipe: Recipe = serde_json::from_str(response.as_text().ok_or("no text in response")?)?;
    println!("{} ({:?})", recipe.name, recipe.difficulty);
    for ingredient in &recipe.ingredients {
        println!("  - {} {}", ingredient.amount, ingredient.item);
    }
    for (i, step) in recipe.steps.iter().enumerate() {
        println!("  {}. {step}", i + 1);
    }

    println!("\n--- Schema + Google Search ---");
    let response = client
        .interaction()
        .with_model(model)
        .with_text("What is the latest stable release of the Rust programming language?")
        .with_google_search()
        .with_response_format(json!({
            "type": "object",
            "properties": {
                "project": {"type": "string"},
                "latest_version": {"type": "string"},
                "highlights": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["project", "latest_version", "highlights"]
        }))
        .create()
        .await?;
    let release: Release = serde_json::from_str(response.as_text().ok_or("no text in response")?)?;
    println!(
        "{} {} (searched: {:?})\n  {:?}",
        release.project,
        release.latest_version,
        response.google_search_calls(),
        release.highlights
    );

    println!("\n--- Streaming ---");
    let mut stream = client
        .interaction()
        .with_model(model)
        .with_text("Review the movie 'Arrival' (2016). Be concise.")
        .with_response_format(json!({
            "type": "object",
            "properties": {
                "title": {"type": "string"},
                "year": {"type": "integer"},
                "rating_out_of_10": {"type": "number"},
                "pros": {"type": "array", "items": {"type": "string"}},
                "cons": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["title", "year", "rating_out_of_10", "pros", "cons"]
        }))
        .create_stream();

    // Fragments are not valid JSON on their own; parse the assembled text.
    let mut json_text = String::new();
    while let Some(event) = stream.next().await {
        match event?.chunk {
            StreamChunk::StepDelta { delta, .. } => {
                if let Some(text) = delta.as_text() {
                    json_text.push_str(text);
                }
            }
            StreamChunk::Error { message, code } => {
                return Err(format!("stream error ({code:?}): {message}").into());
            }
            _ => {}
        }
    }
    let review: MovieReview = serde_json::from_str(&json_text)?;
    println!(
        "{} ({}): {}/10",
        review.title, review.year, review.rating_out_of_10
    );

    Ok(())
}
