//! A multi-turn customer-support agent: server-side state plus `#[tool]`
//! functions executed by `create_with_auto_functions()`.
//!
//! Each turn chains to the previous one with `previous_interaction_id`, so
//! the conversation history lives on the server. The system instruction and
//! the tools are *not* inherited that way, so every turn sends both.
//!
//! The "CRM" is an in-memory stub; the tools read it and return real
//! lookups, but refunds and tickets are not persisted anywhere.
//!
//! Run with: `cargo run --example multi_turn_agent_auto`

use genai_rs::{CallableFunction, Client, FunctionDeclaration};
use genai_rs_macros::tool;
use serde::Serialize;
use serde_json::{Value, json};
use std::env;
use std::error::Error;
use std::sync::OnceLock;

#[derive(Serialize)]
struct Customer {
    id: &'static str,
    name: &'static str,
    email: &'static str,
    tier: &'static str,
}

#[derive(Serialize)]
struct Order {
    id: &'static str,
    customer_id: &'static str,
    status: &'static str,
    items: &'static [&'static str],
    total: f64,
}

struct Crm {
    customers: Vec<Customer>,
    orders: Vec<Order>,
}

fn crm() -> &'static Crm {
    static CRM: OnceLock<Crm> = OnceLock::new();
    CRM.get_or_init(|| Crm {
        customers: vec![Customer {
            id: "CUST-001",
            name: "Alice Johnson",
            email: "alice@example.com",
            tier: "premium",
        }],
        orders: vec![
            Order {
                id: "ORD-1234",
                customer_id: "CUST-001",
                status: "delivered",
                items: &["Wireless Headphones", "USB-C Cable"],
                total: 89.99,
            },
            Order {
                id: "ORD-1235",
                customer_id: "CUST-001",
                status: "processing",
                items: &["Laptop Stand"],
                total: 49.99,
            },
        ],
    })
}

/// Look up a customer by ID, email address, or full name
#[tool(identifier(description = "Customer ID (e.g. CUST-001), email, or full name"))]
fn lookup_customer(identifier: String) -> Value {
    match crm().customers.iter().find(|c| {
        [c.id, c.email, c.name]
            .iter()
            .any(|v| v.eq_ignore_ascii_case(&identifier))
    }) {
        Some(customer) => json!(customer),
        None => json!({"error": format!("no customer matches '{identifier}'")}),
    }
}

/// List a customer's orders
#[tool(customer_id(description = "Customer ID (e.g. CUST-001)"))]
fn list_orders(customer_id: String) -> Value {
    let orders: Vec<&Order> = crm()
        .orders
        .iter()
        .filter(|o| o.customer_id == customer_id)
        .collect();
    json!(orders)
}

/// Start a refund for a delivered order
#[tool(
    order_id(description = "Order ID (e.g. ORD-1234)"),
    reason(description = "Why the customer wants a refund")
)]
fn initiate_refund(order_id: String, reason: String) -> Value {
    match crm().orders.iter().find(|o| o.id == order_id) {
        Some(order) if order.status == "delivered" => json!({
            "order_id": order.id,
            "amount": order.total,
            "reason": reason,
            "status": "refund_pending",
        }),
        Some(order) => {
            json!({"error": format!("order {} is {}, not delivered", order.id, order.status)})
        }
        None => json!({"error": format!("no order '{order_id}'")}),
    }
}

const SYSTEM_PROMPT: &str = "You are a concise support agent for TechGadgets Inc. Look up the \
    customer before discussing their account, confirm order details before a refund, and only \
    call initiate_refund once the customer has confirmed.";

struct SupportSession {
    client: Client,
    tools: Vec<FunctionDeclaration>,
    last_interaction_id: Option<String>,
}

impl SupportSession {
    async fn send(&mut self, message: &str) -> Result<String, Box<dyn Error>> {
        let mut builder = self
            .client
            .interaction()
            .with_model(genai_rs::DEFAULT_MODEL)
            .with_system_instruction(SYSTEM_PROMPT)
            .add_functions(self.tools.clone())
            .with_text(message);
        if let Some(previous) = &self.last_interaction_id {
            builder = builder.with_previous_interaction(previous);
        }
        let result = builder.create_with_auto_functions().await?;

        for exec in &result.executions {
            println!("  [tool] {}({}) -> {}", exec.name, exec.args, exec.result);
        }
        if result.reached_max_loops {
            return Err("the agent was still calling tools when the loop limit hit".into());
        }
        self.last_interaction_id = result.response.id.clone();
        Ok(result
            .response
            .as_text()
            .ok_or("no text in response")?
            .to_string())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let mut session = SupportSession {
        client: Client::builder(api_key).build()?,
        tools: vec![
            LookupCustomerCallable.declaration(),
            ListOrdersCallable.declaration(),
            InitiateRefundCallable.declaration(),
        ],
        last_interaction_id: None,
    };

    for message in [
        "Hi, I'm Alice Johnson. What orders do I have?",
        "The headphones in ORD-1234 stopped working. Can I get a refund?",
        "Yes, please go ahead with the refund.",
    ] {
        println!("Customer: {message}");
        println!("Agent: {}\n", session.send(message).await?);
    }

    Ok(())
}
