//! # Stateless Multi-Turn Customer Support Agent (Manual Functions)
//!
//! This example demonstrates multi-turn function calling with `store: false`.
//! When store is disabled:
//! - The server doesn't keep conversation state
//! - You CANNOT use `previous_interaction_id`
//! - You MUST manually build conversation history
//!
//! ## Key Differences from Stateful (store: true)
//!
//! | Aspect | Stateful | Stateless |
//! |--------|----------|-----------|
//! | Server state | Yes | No |
//! | `previous_interaction_id` | Yes | No |
//! | Manual history | No | Yes |
//! | Auto functions | Available | Blocked at runtime |
//!
//! ## When to Use Stateless
//!
//! - Privacy-sensitive applications (no server-side storage)
//! - Custom conversation persistence (your own database)
//! - Conversation filtering/modification between turns
//! - Testing and debugging
//!
//! ## Running
//!
//! ```bash
//! LOUD_WIRE=1 cargo run --example multi_turn_agent_manual_stateless
//! ```
//!
//! ## Prerequisites
//!
//! Set the `GEMINI_API_KEY` environment variable with your API key.

use genai_rs::{Client, FunctionDeclaration, Step};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::env;
use std::error::Error;

// ============================================================================
// Simulated Backend Systems
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Customer {
    id: String,
    name: String,
    email: String,
    tier: String,
    account_balance: f64,
    open_tickets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Order {
    id: String,
    customer_id: String,
    status: String,
    items: Vec<String>,
    total: f64,
    created_at: String,
}

struct Database {
    customers: HashMap<String, Customer>,
    orders: HashMap<String, Order>,
}

impl Database {
    fn new() -> Self {
        let mut customers = HashMap::new();
        let mut orders = HashMap::new();

        customers.insert(
            "CUST-001".to_string(),
            Customer {
                id: "CUST-001".to_string(),
                name: "Alice Johnson".to_string(),
                email: "alice@example.com".to_string(),
                tier: "premium".to_string(),
                account_balance: 150.00,
                open_tickets: vec!["TKT-2024-001".to_string()],
            },
        );

        orders.insert(
            "ORD-2024-1234".to_string(),
            Order {
                id: "ORD-2024-1234".to_string(),
                customer_id: "CUST-001".to_string(),
                status: "shipped".to_string(),
                items: vec!["Wireless Headphones".to_string(), "USB-C Cable".to_string()],
                total: 89.99,
                created_at: "2024-12-20".to_string(),
            },
        );

        orders.insert(
            "ORD-2024-1235".to_string(),
            Order {
                id: "ORD-2024-1235".to_string(),
                customer_id: "CUST-001".to_string(),
                status: "processing".to_string(),
                items: vec!["Laptop Stand".to_string()],
                total: 49.99,
                created_at: "2024-12-24".to_string(),
            },
        );

        Self { customers, orders }
    }
}

fn get_database() -> &'static Database {
    static DATABASE: std::sync::OnceLock<Database> = std::sync::OnceLock::new();
    DATABASE.get_or_init(Database::new)
}

// ============================================================================
// Function Implementations
// ============================================================================

fn execute_function(name: &str, args: &Value) -> Value {
    match name {
        "lookup_customer" => {
            let identifier = args["identifier"].as_str().unwrap_or("");
            println!("  [Tool: lookup_customer({})]", identifier);

            let customer = get_database()
                .customers
                .values()
                .find(|c| c.id == identifier || c.email == identifier);

            match customer {
                Some(c) => json!(c),
                None => json!({"error": format!("Customer not found: {}", identifier)}),
            }
        }
        "get_order" => {
            let order_id = args["order_id"].as_str().unwrap_or("");
            println!("  [Tool: get_order({})]", order_id);

            match get_database().orders.get(order_id) {
                Some(order) => json!(order),
                None => json!({"error": format!("Order not found: {}", order_id)}),
            }
        }
        "list_customer_orders" => {
            let customer_id = args["customer_id"].as_str().unwrap_or("");
            println!("  [Tool: list_customer_orders({})]", customer_id);

            let orders: Vec<&Order> = get_database()
                .orders
                .values()
                .filter(|o| o.customer_id == customer_id)
                .collect();

            json!(orders)
        }
        "initiate_refund" => {
            let order_id = args["order_id"].as_str().unwrap_or("");
            let reason = args["reason"].as_str().unwrap_or("");
            println!("  [Tool: initiate_refund({}, '{}')]", order_id, reason);

            match get_database().orders.get(order_id) {
                Some(order) => json!({
                    "refund_id": format!("REF-{}", rand_simple()),
                    "order_id": order_id,
                    "amount": order.total,
                    "status": "pending",
                    "message": "Refund initiated. Amount will be credited within 5-7 business days."
                }),
                None => json!({"error": format!("Order not found: {}", order_id)}),
            }
        }
        _ => json!({"error": format!("Unknown function: {}", name)}),
    }
}

fn rand_simple() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| (d.as_millis() % 10000) as u32)
        .unwrap_or(1234)
}

// ============================================================================
// Function Declarations
// ============================================================================

fn get_function_declarations() -> Vec<FunctionDeclaration> {
    vec![
        FunctionDeclaration::builder("lookup_customer")
            .description("Look up customer information by ID or email")
            .parameter(
                "identifier",
                json!({
                    "type": "string",
                    "description": "Customer ID (e.g., CUST-001) or email address"
                }),
            )
            .required(vec!["identifier".to_string()])
            .build(),
        FunctionDeclaration::builder("get_order")
            .description("Get order details by order ID")
            .parameter(
                "order_id",
                json!({
                    "type": "string",
                    "description": "Order ID (e.g., ORD-2024-1234)"
                }),
            )
            .required(vec!["order_id".to_string()])
            .build(),
        FunctionDeclaration::builder("list_customer_orders")
            .description("List all orders for a customer")
            .parameter(
                "customer_id",
                json!({
                    "type": "string",
                    "description": "Customer ID (e.g., CUST-001)"
                }),
            )
            .required(vec!["customer_id".to_string()])
            .build(),
        FunctionDeclaration::builder("initiate_refund")
            .description("Initiate a refund for an order")
            .parameter(
                "order_id",
                json!({
                    "type": "string",
                    "description": "Order ID to refund"
                }),
            )
            .parameter(
                "reason",
                json!({
                    "type": "string",
                    "description": "Reason for the refund"
                }),
            )
            .required(vec!["order_id".to_string(), "reason".to_string()])
            .build(),
    ]
}

// ============================================================================
// Stateless Support Session
// ============================================================================

/// Stateless conversation session manager.
///
/// Unlike the stateful version, this maintains conversation history locally
/// and does NOT use `previous_interaction_id`. All state is managed client-side.
struct StatelessSupportSession {
    client: Client,
    /// Full conversation history maintained locally (as steps)
    conversation_history: Vec<Step>,
    functions: Vec<FunctionDeclaration>,
    system_instruction: String,
}

impl StatelessSupportSession {
    fn new(client: Client) -> Self {
        Self {
            client,
            conversation_history: Vec::new(),
            functions: get_function_declarations(),
            system_instruction: Self::system_prompt().to_string(),
        }
    }

    /// Process a customer message with manual history management.
    ///
    /// Key differences from stateful version:
    /// 1. We build conversation history manually instead of using `previous_interaction_id`
    /// 2. We use `with_store_disabled()` on every request
    async fn process_message(&mut self, message: &str) -> Result<String, Box<dyn Error>> {
        // Add user message to history
        self.conversation_history.push(Step::user_text(message));

        // Build request with full history
        let mut response = self
            .client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_history(self.conversation_history.clone())
            .add_functions(self.functions.clone())
            .with_system_instruction(&self.system_instruction)
            .with_store_disabled() // <-- Key: no server-side state
            .create()
            .await?;

        // Manual function calling loop
        const MAX_ITERATIONS: usize = 5;
        for iteration in 0..MAX_ITERATIONS {
            let function_calls = response.function_calls();

            if function_calls.is_empty() {
                break;
            }

            println!(
                "  [Iteration {}: {} function call(s)]",
                iteration + 1,
                function_calls.len()
            );

            // Process each function call
            for call in &function_calls {
                // Add the function call step to history (echo the model's call)
                self.conversation_history.push(Step::function_call(
                    call.id,
                    call.name,
                    call.args.clone(),
                ));

                // Execute the function
                let result = execute_function(call.name, call.args);

                // Add the function result step to history
                self.conversation_history
                    .push(Step::function_result(call.name, call.id, result));
            }

            // Send updated history back to model
            response = self
                .client
                .interaction()
                .with_model("gemini-3-flash-preview")
                .with_history(self.conversation_history.clone())
                .add_functions(self.functions.clone())
                .with_system_instruction(&self.system_instruction)
                .with_store_disabled()
                .create()
                .await?;
        }

        // Add model's output steps to history (preserves thoughts/signatures)
        self.conversation_history.extend(response.output_steps());
        if let Some(text) = response.as_text() {
            Ok(text.to_string())
        } else {
            Ok("I apologize, but I couldn't process that request.".to_string())
        }
    }

    fn system_prompt() -> &'static str {
        r#"You are a friendly customer support agent for TechGadgets Inc.

Your responsibilities:
1. Help customers with order inquiries, returns, and refunds
2. Look up customer and order information using the available tools
3. Provide accurate, helpful responses based on real data

Guidelines:
- Always verify customer identity before sharing account details
- Be empathetic and solution-oriented
- For refunds, confirm the order details before proceeding

Remember to use the available tools to look up real data."#
    }

    /// Get current history size for debugging
    fn history_size(&self) -> usize {
        self.conversation_history.len()
    }
}

// ============================================================================
// Main Demo
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in environment");
    let client = Client::builder(api_key).build()?;

    println!("=== Stateless Multi-Turn Support Agent (store: false) ===\n");
    println!("This example demonstrates manual history management.\n");

    let mut session = StatelessSupportSession::new(client);

    // Simulate a multi-turn conversation
    let conversation = [
        "Hi, I'm Alice Johnson and I need help with my recent order.",
        "Can you tell me the status of order ORD-2024-1234?",
        "I received the headphones but they're not working. Can I get a refund?",
    ];

    for (turn, message) in conversation.iter().enumerate() {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("👤 Customer (Turn {}): {}\n", turn + 1, message);
        println!("  [History size before: {}]", session.history_size());

        match session.process_message(message).await {
            Ok(response) => {
                println!("  [History size after: {}]", session.history_size());
                println!("\n🤖 Agent:\n{}\n", response);
            }
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }
    }

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Conversation Complete\n");

    println!("--- Key Takeaways ---");
    println!("1. store: false means NO server-side state");
    println!("2. Must manually build conversation history");
    println!("3. Cannot use previous_interaction_id");
    println!("4. create_with_auto_functions() returns a runtime error");
    println!("\nFinal history size: {} items", session.history_size());

    Ok(())
}
