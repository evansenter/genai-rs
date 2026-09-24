//! A stateless multi-turn support agent with a manual function-calling loop.
//!
//! With `with_store_disabled()` the server keeps nothing: there is no
//! interaction ID to chain from, so the client holds the whole conversation
//! and sends it on every request. (`create_with_auto_functions()` needs
//! stored interactions and refuses to run in this mode, hence the manual loop.)
//!
//! The history must be replayed exactly as the model produced it. Each round
//! appends `response.output_steps()` — the model's thought steps and function
//! calls, with their signatures — before the function results. Rebuilding the
//! calls by hand with `Step::function_call` would drop those signatures, which
//! the API needs to accept stateless function-calling history.
//!
//! The "CRM" is an in-memory stub.
//!
//! Run with: `cargo run --example multi_turn_agent_manual_stateless`

use genai_rs::{Client, FunctionDeclaration, Step};
use serde_json::{Value, json};
use std::env;
use std::error::Error;

const MAX_ROUNDS: usize = 5;

const SYSTEM_PROMPT: &str = "You are a concise support agent for TechGadgets Inc. Use the tools \
    to look up real data before answering questions about an account or order.";

fn declarations() -> Vec<FunctionDeclaration> {
    vec![
        FunctionDeclaration::builder("lookup_customer")
            .description("Look up a customer by full name or email address")
            .parameter("identifier", json!({"type": "string"}))
            .required(vec!["identifier".to_string()])
            .build(),
        FunctionDeclaration::builder("list_orders")
            .description("List a customer's orders")
            .parameter(
                "customer_id",
                json!({"type": "string", "description": "e.g. CUST-001"}),
            )
            .required(vec!["customer_id".to_string()])
            .build(),
    ]
}

fn execute(name: &str, args: &Value) -> Value {
    match (
        name,
        args["identifier"].as_str(),
        args["customer_id"].as_str(),
    ) {
        ("lookup_customer", Some(id), _)
            if id.eq_ignore_ascii_case("alice johnson") || id == "alice@example.com" =>
        {
            json!({"id": "CUST-001", "name": "Alice Johnson", "tier": "premium"})
        }
        ("lookup_customer", Some(id), _) => json!({"error": format!("no customer matches '{id}'")}),
        // Wrapped in an object: the API reads a top-level array in a
        // function result as a list of content blocks, and rejects it.
        ("list_orders", _, Some("CUST-001")) => json!({"orders": [
            {"id": "ORD-1234", "status": "delivered", "items": ["Wireless Headphones"], "total": 89.99},
            {"id": "ORD-1235", "status": "processing", "items": ["Laptop Stand"], "total": 49.99}
        ]}),
        ("list_orders", _, Some(id)) => json!({"error": format!("no customer '{id}'")}),
        _ => json!({"error": format!("unknown function or missing arguments: {name}")}),
    }
}

struct StatelessSession {
    client: Client,
    history: Vec<Step>,
}

impl StatelessSession {
    async fn send(&mut self, message: &str) -> Result<String, Box<dyn Error>> {
        self.history.push(Step::user_text(message));

        for _ in 0..MAX_ROUNDS {
            let response = self
                .client
                .interaction()
                .with_model(genai_rs::DEFAULT_MODEL)
                .with_store_disabled()
                .with_system_instruction(SYSTEM_PROMPT)
                .add_functions(declarations())
                .with_history(self.history.clone())
                .create()
                .await?;

            // Replay the model's own steps (thoughts, calls, text) verbatim.
            self.history.extend(response.output_steps());

            let calls = response.function_calls();
            if calls.is_empty() {
                return Ok(response.as_text().ok_or("no text in response")?.to_string());
            }
            for call in calls {
                let result = execute(call.name, call.args);
                println!("  [tool] {}({}) -> {result}", call.name, call.args);
                self.history
                    .push(Step::function_result(call.name, call.id, result));
            }
        }
        Err(format!("still calling tools after {MAX_ROUNDS} rounds").into())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let mut session = StatelessSession {
        client: Client::builder(api_key).build()?,
        history: Vec::new(),
    };

    for message in [
        "Hi, I'm Alice Johnson. What orders do I have?",
        "Which of those hasn't shipped yet?",
    ] {
        println!("Customer: {message}");
        println!("Agent: {}", session.send(message).await?);
        println!("  (client-side history: {} steps)\n", session.history.len());
    }

    Ok(())
}
