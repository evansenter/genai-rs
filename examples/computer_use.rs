//! Computer Use: the model drives a browser by requesting UI actions.
//!
//! The actions are predefined functions (`navigate`, `click_at`,
//! `type_text_at`, `scroll_document`, ...) that arrive as ordinary
//! `Step::FunctionCall`s. Nothing runs server-side: your code performs each
//! action in a real browser, then replies with a `Step::function_result`
//! carrying a screenshot and the current URL, and the loop continues until
//! the model answers in text. That harness is out of scope here, so this
//! example stops at the first requested action.
//!
//! Treat every action as untrusted input: review what the model asks for,
//! exclude actions you never want, and keep prompt-injection detection on
//! for pages you don't control.
//!
//! Run with: `cargo run --example computer_use`

use genai_rs::{Client, ComputerUseConfig};
use std::env;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = Client::builder(api_key).build()?;

    let response = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_text("Open https://example.com and tell me the page's main heading.")
        .add_tool(
            ComputerUseConfig::new()
                .with_environment("browser")
                .excluding(vec![
                    "drag_and_drop".to_string(),
                    "key_combination".to_string(),
                ])
                .with_prompt_injection_detection(true),
        )
        .create()
        .await?;

    println!("Status: {:?}", response.status);
    let actions = response.function_calls();
    if actions.is_empty() {
        return Err("expected the model to request a browser action".into());
    }
    for action in actions {
        println!(
            "Requested action: {}({}) [call id {}]",
            action.name, action.args, action.id
        );
    }

    Ok(())
}
